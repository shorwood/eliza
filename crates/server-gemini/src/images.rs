//! Gemini deterministic image-generation lowering and rendering.

use axum::Json;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use eliza_http::model::ModelId;
use eliza_http::response::{SseEvents, json_event};
use eliza_modality_chat as chat;
use eliza_modality_image as image;

use super::errors::{GeminiError, GeminiRejection};
use super::types::{
    ContentPart, ContentRole, GenerateCandidate, GenerateContentRequest, GenerateContentResponse,
    GenerateDelivery, GenerateFinishReason, GenerateInlineData, GenerateOutputContent,
    GenerateOutputPart, GenerateUsage, ImageAspectRatio, ImageResponseFormat, ImageSize,
};
use crate::context::AppState;

// -----------------------------------------------------------------------------
// ImageComposition: Selects image-only or mixed candidate content.
// -----------------------------------------------------------------------------

/// Candidate content emitted around the generated image.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum ImageComposition {
    /// Emit only the generated inline PNG.
    ImageOnly,
    /// Emit an ELIZA text part followed by the generated inline PNG.
    TextAndImage,
}

impl ImageComposition {
    /// Build the optional text request selected by this composition.
    fn text_request(self, prompt: &str) -> Option<chat::turn::Request> {
        match self {
            Self::ImageOnly => None,
            Self::TextAndImage => Some(chat::turn::Request::new(
                Vec::new(),
                vec![prompt.to_owned().into()],
                Vec::new(),
                chat::turn::ToolChoice::default(),
                chat::structured_output::StructuredOutput::default(),
            )),
        }
    }
}

/// Map Gemini's optional image block to one shared output size.
///
/// # Errors
///
/// Returns a typed provider error for an unsupported size or aspect ratio.
fn image_format_size(
    format: Option<&ImageResponseFormat>,
) -> Result<image::generation::Size, GeminiError> {
    // An omitted image format selects Gemini's documented square default.
    let Some(format) = format else {
        return Ok(image::generation::Size::Square1024);
    };

    // The local fixture exposes one deterministic resolution tier.
    if matches!(format.image_size, Some(ImageSize::Unsupported)) {
        return Err(GeminiError::UnsupportedImageSize);
    }
    match format.aspect_ratio.unwrap_or_default() {
        ImageAspectRatio::Square => Ok(image::generation::Size::Square1024),
        ImageAspectRatio::Landscape => Ok(image::generation::Size::Landscape1536),
        ImageAspectRatio::Portrait => Ok(image::generation::Size::Portrait1536),
        ImageAspectRatio::Unsupported => Err(GeminiError::UnsupportedImageAspectRatio),
    }
}

/// Validate image-specific response controls and map them to fixed dimensions.
///
/// # Errors
///
/// Returns a typed provider error for unsupported image controls.
fn image_size(
    generation: Option<&super::types::GenerateConfig>,
) -> Result<image::generation::Size, GeminiError> {
    // A missing generation config selects Gemini's square default.
    let Some(generation) = generation else {
        return Ok(image::generation::Size::Square1024);
    };

    // Speech synthesis controls cannot affect a PNG fixture.
    if generation.speech_config.is_some() {
        return Err(GeminiError::UnsupportedImageControl {
            param: "generationConfig.speechConfig",
        });
    }

    // Legacy text formatting is not an image-generation control.
    if generation.has_legacy_text_format_controls() {
        return Err(GeminiError::UnsupportedImageControl {
            param: "generationConfig.responseMimeType",
        });
    }

    // An omitted current format also selects the square default.
    let Some(format) = generation.response_format.as_ref() else {
        return Ok(image::generation::Size::Square1024);
    };

    // Structured text formatting is unavailable on the image path.
    if format.text.is_some() {
        return Err(GeminiError::UnsupportedImageControl {
            param: "generationConfig.responseFormat.text",
        });
    }

    // Audio formatting is unavailable on the image path.
    if format.audio.is_some() {
        return Err(GeminiError::UnsupportedImageControl {
            param: "generationConfig.responseFormat.audio",
        });
    }
    image_format_size(format.image.as_ref())
}

/// Join one user content record containing only ordinary text parts.
///
/// # Errors
///
/// Returns a typed provider error when the prompt is missing or non-textual.
fn image_prompt(contents: Option<Vec<super::types::Content>>) -> Result<String, GeminiError> {
    let mut contents = contents.ok_or(GeminiError::ImageTextOnly)?;

    // Image generation accepts exactly one user content record.
    if contents.len() != 1 {
        return Err(GeminiError::ImageTextOnly);
    }
    let content = contents.pop().ok_or(GeminiError::ImageTextOnly)?;

    // Model and function history would make prompt selection ambiguous.
    if !matches!(content.role, None | Some(ContentRole::User)) {
        return Err(GeminiError::ImageTextOnly);
    }
    let parts = content.parts.ok_or(GeminiError::ImageTextOnly)?;

    // At least one text part is required by the shared generation engine.
    if parts.is_empty() {
        return Err(GeminiError::ImageTextOnly);
    }
    let text = parts
        .into_iter()
        .map(|part| match part {
            ContentPart::Text {
                text,
                speech_metadata: None,
                ..
            } => Ok(text),
            ContentPart::Text {
                speech_metadata: Some(_),
                ..
            }
            | ContentPart::InlineData { .. }
            | ContentPart::FileData { .. }
            | ContentPart::FunctionCall { .. }
            | ContentPart::FunctionResponse { .. }
            | ContentPart::Unsupported { .. } => Err(GeminiError::ImageTextOnly),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(text.join("\n"))
}

// -----------------------------------------------------------------------------
// LoweredImage: Owns validated provider-neutral generation requests.
// -----------------------------------------------------------------------------

/// Validated image request plus optional ELIZA text generation.
struct LoweredImage {
    /// Provider-neutral image request.
    image: image::generation::Request,
    /// Text-only request used by mixed-modality responses.
    text: Option<chat::turn::Request>,
}

impl LoweredImage {
    /// Lower the supported Gemini image-generation subset.
    ///
    /// # Errors
    ///
    /// Returns a typed Gemini error for unsupported controls or content.
    fn from_payload(
        payload: GenerateContentRequest,
        composition: ImageComposition,
    ) -> Result<Self, GeminiError> {
        // System instructions would require a second prompt interpretation rule.
        if payload.system_instruction.is_some() {
            return Err(GeminiError::UnsupportedImageControl {
                param: "systemInstruction",
            });
        }

        // Tool definitions are unrelated to deterministic image generation.
        if payload.tools.is_some() {
            return Err(GeminiError::UnsupportedImageControl { param: "tools" });
        }

        // Tool selection is likewise unavailable without callable tools.
        if payload.tool_config.is_some() {
            return Err(GeminiError::UnsupportedImageControl {
                param: "toolConfig",
            });
        }

        let size = image_size(payload.generation_config.as_ref())?;
        let prompt = image_prompt(payload.contents)?;
        let text = composition.text_request(&prompt);

        // Bind validated provider input to the bounded shared engine.
        let image = image::generation::Request {
            prompt,
            count: 1,
            size,
        };

        // Preserve the paired requests for one coordinated completion.
        Ok(Self { image, text })
    }

    /// Execute both requested local generators under shared input limits.
    ///
    /// # Errors
    ///
    /// Returns a rendered Gemini rejection for either neutral engine.
    fn complete(self, state: &AppState) -> Result<CompletedImage, GeminiRejection> {
        let generated = self
            .image
            .complete(state.config.limits.max_input_chars())
            .map_err(|error| GeminiRejection::from(&error))?;
        let text = self
            .text
            .map(|request| {
                request.complete(
                    state.config.limits.max_input_chars(),
                    state.config.limits.max_history_messages(),
                )
            })
            .transpose()
            .map_err(|error| GeminiRejection::from(&error))?;
        Ok(CompletedImage {
            image: generated,
            text,
        })
    }
}

// -----------------------------------------------------------------------------
// CompletedImage: Restores Gemini's ordered text and inline-data parts.
// -----------------------------------------------------------------------------

/// Completed provider-neutral image and optional text output.
struct CompletedImage {
    /// One generated PNG response.
    image: image::generation::Response,
    /// Optional completed ELIZA response.
    text: Option<chat::turn::Response>,
}

impl CompletedImage {
    /// Build one complete candidate with deterministic usage.
    fn into_response(self, model: ModelId) -> GenerateContentResponse {
        let mut parts = Vec::new();
        let usage = match self.text {
            Some(response) => {
                let chat::turn::Output::Text(text) = response.output else {
                    unreachable!("image text request does not offer tools")
                };
                parts.push(GenerateOutputPart::Text { text });
                GenerateUsage::from(response.usage)
            }
            None => GenerateUsage {
                prompt_token_count: self.image.prompt_tokens,
                candidates_token_count: 0,
                thoughts_token_count: None,
                total_token_count: self.image.prompt_tokens,
            },
        };
        let png = self.image.images.into_iter().next().unwrap_or_default();
        parts.push(GenerateOutputPart::InlineData {
            inline_data: GenerateInlineData {
                mime_type: "image/png".to_owned(),
                data: STANDARD.encode(png),
            },
        });
        GenerateContentResponse {
            candidates: vec![GenerateCandidate {
                content: GenerateOutputContent {
                    role: "model",
                    parts,
                },
                finish_reason: Some(GenerateFinishReason::Stop),
                index: 0,
            }],
            model_version: model,
            usage_metadata: Some(usage),
        }
    }
}

// -----------------------------------------------------------------------------
// ImageTransport: Owns Gemini image success and rejection rendering.
// -----------------------------------------------------------------------------

/// Completed image response or native rejection ready for transport.
pub(super) enum ImageTransport {
    /// Successfully generated response paired with its requested transport.
    Complete {
        /// Unary, JSON-array, or SSE delivery.
        delivery: GenerateDelivery,
        /// Complete Gemini response record.
        response: GenerateContentResponse,
        /// Optional delay before the SSE event.
        delay_ms: u64,
    },
    /// Provider-native generation rejection.
    Rejected(
        /// Typed rejection rendered through Gemini's native envelope.
        GeminiRejection,
    ),
}

impl ImageTransport {
    /// Generate one Gemini image response or complete stream record.
    pub(super) fn generate(
        state: &AppState,
        model: ModelId,
        delivery: GenerateDelivery,
        payload: GenerateContentRequest,
        composition: ImageComposition,
    ) -> Self {
        // Only the dedicated image model may enter this provider path.
        if !state
            .config
            .models
            .images
            .has_model(&model, image::generation::MODEL_ID)
        {
            return Self::Rejected(GeminiRejection::from_error(
                &GeminiError::ImageGenerationModelRequired {
                    expected: state
                        .config
                        .models
                        .images
                        .expected(image::generation::MODEL_ID),
                },
            ));
        }

        // Provider validation and both local engines share one rejection path.
        let completed = LoweredImage::from_payload(payload, composition)
            .map_err(|error| GeminiRejection::from_error(&error))
            .and_then(|lowered| lowered.complete(state));

        // Resolve the shared result before selecting a wire transport.
        let completed = match completed {
            Ok(completed) => completed,
            // A shared-engine failure has no partial response to preserve.
            Err(error) => {
                return Self::Rejected(error);
            }
        };

        // Bind successful output to the transport selected by the outer route.
        Self::Complete {
            delivery,
            response: completed.into_response(model),
            delay_ms: state.config.stream_delay_ms,
        }
    }
}

impl IntoResponse for ImageTransport {
    fn into_response(self) -> Response {
        match self {
            Self::Complete {
                delivery: GenerateDelivery::Unary,
                response,
                ..
            } => Json(response).into_response(),
            Self::Complete {
                delivery: GenerateDelivery::JsonStream,
                response,
                ..
            } => Json(vec![response]).into_response(),
            Self::Complete {
                delivery: GenerateDelivery::Sse,
                response,
                delay_ms,
            } => match json_event(&response) {
                Ok(event) => SseEvents::from(vec![event])
                    .with_delay(delay_ms)
                    .into_response(),
                Err(error) => GeminiRejection::from_error(&error).into_response(),
            },
            Self::Rejected(error) => error.into_response(),
        }
    }
}
