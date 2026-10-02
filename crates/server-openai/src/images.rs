//! OpenAI-compatible deterministic image generation.

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use eliza_http::context::ProviderAuth;
use eliza_http::extraction::ExtractionError;
use eliza_http::model::ModelId;
use eliza_http::response::unix_timestamp;
use eliza_modality_image as image;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::errors::{OpenAiError, OpenAiFailureResponse, OpenAiRejection};
use crate::context::AppState;

// -----------------------------------------------------------------------------
// Image: Defines the supported OpenAI Images subset.
// -----------------------------------------------------------------------------

/// Output dimensions accepted by the bounded fixture.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
enum ImageSize {
    /// 256-by-256 square.
    #[serde(rename = "256x256")]
    Square256,
    /// 512-by-512 square.
    #[serde(rename = "512x512")]
    Square512,
    /// 1024-by-1024 square and default.
    #[default]
    #[serde(rename = "1024x1024")]
    Square1024,
    /// 1536-by-1024 landscape image.
    #[serde(rename = "1536x1024")]
    Landscape1536,
    /// 1024-by-1536 portrait image.
    #[serde(rename = "1024x1536")]
    Portrait1536,
    /// Unknown or currently unsupported dimensions.
    #[serde(other)]
    Unsupported,
}

impl TryFrom<ImageSize> for image::generation::Size {
    type Error = OpenAiError;

    fn try_from(size: ImageSize) -> Result<Self, Self::Error> {
        match size {
            ImageSize::Square256 => Ok(Self::Square256),
            ImageSize::Square512 => Ok(Self::Square512),
            ImageSize::Square1024 => Ok(Self::Square1024),
            ImageSize::Landscape1536 => Ok(Self::Landscape1536),
            ImageSize::Portrait1536 => Ok(Self::Portrait1536),
            ImageSize::Unsupported => Err(OpenAiError::UnsupportedImageSize),
        }
    }
}

/// Current image encoding selector.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum ImageOutputFormat {
    /// Portable Network Graphics.
    #[default]
    Png,
    /// JPEG output is deliberately not implemented.
    Jpeg,
    /// WebP output is deliberately not implemented.
    Webp,
    /// Future or unknown output format.
    #[serde(other)]
    Unsupported,
}

/// Deprecated image response selector retained for older clients.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ImageResponseFormat {
    /// Base64 JSON field.
    #[default]
    B64Json,
    /// Hosted URL output, which would require storage.
    Url,
    /// Future or unknown response shape.
    #[serde(other)]
    Unsupported,
}

/// OpenAI-compatible image-generation request.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ImageGenerationRequest {
    /// Text selecting the deterministic procedural output.
    prompt: String,
    /// Optional model, defaulting to the fixed local image model.
    model: Option<ModelId>,
    /// Number of ordered variations.
    n: Option<i64>,
    /// Requested fixed dimensions.
    size: Option<ImageSize>,
    /// Current image encoding control.
    output_format: Option<ImageOutputFormat>,
    /// Deprecated response representation control.
    response_format: Option<ImageResponseFormat>,
    /// End-user metadata accepted as a documented no-op.
    user: Option<String>,
    /// Unsupported quality control.
    quality: Option<serde_json::Value>,
    /// Unsupported legacy style control.
    style: Option<serde_json::Value>,
    /// Unsupported background control.
    background: Option<serde_json::Value>,
    /// Unsupported moderation control.
    moderation: Option<serde_json::Value>,
    /// Unsupported lossy-output compression control.
    output_compression: Option<serde_json::Value>,
    /// Unsupported progressive-image count.
    partial_images: Option<serde_json::Value>,
    /// Unsupported streaming selector.
    stream: Option<serde_json::Value>,
}

impl ImageGenerationRequest {
    /// Return the first unsupported optional control in provider field order.
    fn unsupported_control(&self) -> Option<&'static str> {
        match () {
            () if self.quality.is_some() => Some("quality"),
            () if self.style.is_some() => Some("style"),
            () if self.background.is_some() => Some("background"),
            () if self.moderation.is_some() => Some("moderation"),
            () if self.output_compression.is_some() => Some("output_compression"),
            () if self.partial_images.is_some() => Some("partial_images"),
            () if self.stream.is_some() => Some("stream"),
            () => None,
        }
    }
}

// -----------------------------------------------------------------------------
// LoweredImageRequest: Owns one validated provider-neutral request.
// -----------------------------------------------------------------------------

/// Validated request ready for provider-neutral generation.
struct LoweredImageRequest {
    /// Provider-neutral generation request.
    request: image::generation::Request,
    /// Validated dimensions retained for response metadata.
    size: image::generation::Size,
}

impl LoweredImageRequest {
    /// Run the bounded shared engine and restore `OpenAI` response metadata.
    ///
    /// # Errors
    ///
    /// Returns a typed generation error when the prompt or count is invalid.
    fn complete(
        self,
        max_input_chars: std::num::NonZeroUsize,
    ) -> Result<ImageGenerationResponse, image::generation::Error> {
        let response = self.request.complete(max_input_chars)?;
        Ok(ImageGenerationResponse::from_generation(
            self.size, response,
        ))
    }
}

impl TryFrom<ImageGenerationRequest> for LoweredImageRequest {
    type Error = OpenAiError;

    fn try_from(payload: ImageGenerationRequest) -> Result<Self, Self::Error> {
        let model = payload
            .model
            .as_ref()
            .map_or(image::generation::MODEL_ID, ModelId::as_str);

        // This route exposes one fixed local generation model.
        if model != image::generation::MODEL_ID {
            return Err(OpenAiError::ImageGenerationModelRequired);
        }

        let output_format = payload.output_format.unwrap_or_default();

        // The deterministic fixture encodes only lossless PNG bytes.
        if !matches!(output_format, ImageOutputFormat::Png) {
            return Err(OpenAiError::UnsupportedImageOutputFormat {
                param: "output_format",
            });
        }
        let response_format = payload.response_format.unwrap_or_default();

        // URL responses would require persistent image storage.
        if !matches!(response_format, ImageResponseFormat::B64Json) {
            return Err(OpenAiError::UnsupportedImageOutputFormat {
                param: "response_format",
            });
        }

        // Semantic and streaming controls cannot alter the fixed local fixture.
        if let Some(param) = payload.unsupported_control() {
            return Err(OpenAiError::UnsupportedImageControl { param });
        }

        // Tracking metadata does not affect deterministic fixture output.
        drop(payload.user);
        let size: image::generation::Size = payload.size.unwrap_or_default().try_into()?;
        let count = payload.n.unwrap_or(1);

        // Bind validated provider input to the bounded shared engine.
        let request = image::generation::Request {
            prompt: payload.prompt,
            count,
            size,
        };

        // Retain the validated size for provider response metadata.
        Ok(Self { request, size })
    }
}

// -----------------------------------------------------------------------------
// ImageGeneration: Restores OpenAI's base64 image envelope.
// -----------------------------------------------------------------------------

/// One generated image in an `OpenAI` response.
#[derive(Debug, Serialize, JsonSchema)]
struct ImageGenerationData {
    /// Standard-base64 PNG bytes.
    b64_json: String,
    /// Whitespace-normalized prompt used by the fixture.
    revised_prompt: String,
}

/// Complete `OpenAI` image-generation response.
#[derive(Debug, Serialize, JsonSchema)]
struct ImageGenerationResponse {
    /// Unix time at response construction.
    created: u64,
    /// Generated images in variation order.
    data: Vec<ImageGenerationData>,
    /// Fixed generated encoding.
    output_format: &'static str,
    /// Generated dimensions.
    size: &'static str,
}

impl ImageGenerationResponse {
    /// Wrap provider-neutral PNGs in `OpenAI`'s response contract.
    fn from_generation(
        size: image::generation::Size,
        response: image::generation::Response,
    ) -> Self {
        let data = response
            .images
            .into_iter()
            .map(|png| ImageGenerationData {
                b64_json: STANDARD.encode(png),
                revised_prompt: response.normalized_prompt.clone(),
            })
            .collect();
        Self {
            created: unix_timestamp(),
            data,
            output_format: "png",
            size: size.as_str(),
        }
    }
}

// -----------------------------------------------------------------------------
// Handle: Authenticates, validates, generates, and renders one request.
// -----------------------------------------------------------------------------

/// Handle one `OpenAI` image-generation request.
async fn handle(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<ImageGenerationRequest>, JsonRejection>,
) -> Response {
    // Authentication failures use OpenAI's native error envelope.
    if let Err(error) = state.config.authenticate(&headers, ProviderAuth::Bearer) {
        return OpenAiRejection::from_error(&error).into_response();
    }

    let Json(payload) = match payload {
        Ok(payload) => payload,
        // Malformed JSON cannot proceed to provider validation.
        Err(error) => {
            return OpenAiRejection::from_error(&ExtractionError::from(error)).into_response();
        }
    };

    // Lower the decoded provider shape before invoking shared generation.
    let lowered = match LoweredImageRequest::try_from(payload) {
        Ok(lowered) => lowered,
        // Unsupported provider controls stop before the shared engine runs.
        Err(error) => return OpenAiRejection::from_error(&error).into_response(),
    };

    // The shared engine owns prompt and count validation.
    match lowered.complete(state.config.limits.max_input_chars()) {
        Ok(response) => Json(response).into_response(),
        Err(error) => OpenAiRejection::from(&error).into_response(),
    }
}

// -----------------------------------------------------------------------------
// Router: Publishes the OpenAI Images endpoint.
// -----------------------------------------------------------------------------

/// Mount deterministic image generation.
pub(super) fn router() -> ApiRouter<AppState> {
    ApiRouter::new().api_route(
        "/v1/images/generations",
        post_with(handle, |operation| {
            operation
                .summary("OpenAI image generation")
                .tag("openai")
                .response::<200, Json<ImageGenerationResponse>>()
                .default_response::<Json<OpenAiFailureResponse>>()
        }),
    )
}
