//! Native Gemini Generate Content adapter.
use std::str::FromStr;

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use eliza_http::context::ProviderAuth;
use eliza_http::execution::Execution;
use eliza_http::extraction::ExtractionError;
use eliza_http::model::ModelId;
use eliza_http::response::{SseEvents, json_event, stream_chunks};
use eliza_modality_chat as chat;
use eliza_modality_image as image;
use eliza_modality_speech as speech;
use uuid::Uuid;

use super::errors::{GeminiError, GeminiFailureResponse, GeminiRejection};
use super::images::ImageTransport;
use super::types::{
    Content, ContentFunctionResponseValue, ContentPart, ContentRole, GenerateCandidate,
    GenerateContentRequest, GenerateContentResponse, GenerateDelivery, GenerateFinishReason,
    GenerateFunctionCall, GenerateOutputContent, GenerateOutputPart, GenerateQuery,
    GenerateResponseModality, GenerateStreamFormat,
};
use crate::context::AppState;

// -----------------------------------------------------------------------------
// GeminiAction: Parses the model and operation encoded in Gemini's path.
// -----------------------------------------------------------------------------

/// Supported generation operation encoded after the model name.
#[derive(Debug, Clone, Copy)]
enum GeminiActionKind {
    /// Return one complete JSON response.
    Generate,
    /// Return incremental response records.
    Stream,
}

impl GeminiActionKind {
    /// Report whether this operation requests streaming output.
    const fn is_stream(self) -> bool {
        matches!(self, Self::Stream)
    }
}

impl FromStr for GeminiActionKind {
    type Err = GeminiError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "generateContent" => Ok(Self::Generate),
            "streamGenerateContent" => Ok(Self::Stream),
            action => Err(GeminiError::UnsupportedModelAction {
                action: action.to_owned(),
            }),
        }
    }
}

/// Model identifier and generation operation parsed from one route segment.
struct GeminiAction {
    /// Provider-visible model identifier.
    model: ModelId,
    /// Generation operation requested after the colon.
    kind: GeminiActionKind,
}

impl FromStr for GeminiAction {
    type Err = GeminiError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        // Gemini path grammar requires `<model>:<action>`.
        let Some((model, action)) = value.split_once(':') else {
            return Err(GeminiError::MissingModelAction);
        };
        let model = model.parse().map_err(|_| GeminiError::EmptyModel)?;
        Ok(Self {
            model,
            kind: action.parse()?,
        })
    }
}

// -----------------------------------------------------------------------------
// GenerateRequestMode: Selects the engine before request lowering.
// -----------------------------------------------------------------------------

/// Generation engine selected by `responseModalities` and model defaults.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum GenerateRequestMode {
    /// Existing ELIZA text generation.
    Text,
    /// Retro diphone audio generation.
    Audio,
    /// Cellular-automaton PNG generation.
    Image {
        /// Whether the candidate also includes a leading ELIZA text part.
        composition: super::images::ImageComposition,
    },
}

impl GenerateRequestMode {
    /// Interpret response modalities without consuming the request.
    ///
    /// # Errors
    ///
    /// Returns a typed error for unsupported modality combinations or controls.
    fn select(model: &ModelId, payload: &GenerateContentRequest) -> Result<Self, GeminiError> {
        let generation = payload.generation_config.as_ref();
        let modalities = generation.and_then(|config| config.response_modalities.as_deref());
        match modalities {
            None if model.as_str() == image::generation::MODEL_ID => Ok(Self::Image {
                composition: super::images::ImageComposition::TextAndImage,
            }),
            None | Some([GenerateResponseModality::Text]) => Ok(Self::Text),
            Some([GenerateResponseModality::Audio])
                if generation
                    .is_some_and(super::types::GenerateConfig::has_text_format_controls) =>
            {
                Err(GeminiError::TextFormatForAudio)
            }
            Some([GenerateResponseModality::Audio])
                if generation.is_some_and(|config| {
                    config
                        .response_format
                        .as_ref()
                        .is_some_and(|format| format.image.is_some())
                }) =>
            {
                Err(GeminiError::ImageFormatForAudio)
            }
            Some([GenerateResponseModality::Audio]) => Ok(Self::Audio),
            Some([GenerateResponseModality::Image]) => Ok(Self::Image {
                composition: super::images::ImageComposition::ImageOnly,
            }),
            Some(
                [
                    GenerateResponseModality::Text,
                    GenerateResponseModality::Image,
                ]
                | [
                    GenerateResponseModality::Image,
                    GenerateResponseModality::Text,
                ],
            ) => Ok(Self::Image {
                composition: super::images::ImageComposition::TextAndImage,
            }),
            _ => Err(GeminiError::InvalidResponseModalities),
        }
    }

    /// Select native modality controls and validate dedicated model policy.
    ///
    /// # Errors
    /// Rejects image models requested with a non-image modality.
    fn for_payload(model: &ModelId, payload: &GenerateContentRequest) -> Result<Self, GeminiError> {
        let mode = Self::select(model, payload)?;

        // Dedicated image models never silently fall back to another engine.
        if model.as_str() == image::generation::MODEL_ID && !matches!(mode, Self::Image { .. }) {
            return Err(GeminiError::ImageModelRequiresImage);
        }
        Ok(mode)
    }
}

impl TryFrom<GenerateContentRequest> for chat::turn::Request {
    type Error = GeminiError;

    fn try_from(payload: GenerateContentRequest) -> Result<Self, Self::Error> {
        // Compile generation formatting before consuming transcript controls.
        let output_format = payload
            .generation_config
            .map(TryInto::try_into)
            .transpose()?
            .unwrap_or_default();

        // Separate the system instruction from replayable conversation turns.
        let mut system = Vec::new();
        if let Some(instruction) = payload.system_instruction {
            system.push(text_parts(
                instruction.parts.unwrap_or_default(),
                "systemInstruction.parts",
            )?);
        }

        // Lower conversation content in provider order.
        let mut turns = Vec::new();
        for content in payload.contents.unwrap_or_default() {
            content_lower(content, &mut turns)?;
        }

        // Lower tool declarations and selection policy independently.
        let tools = payload.tools.unwrap_or_default().try_into()?;
        let tool_choice = payload
            .tool_config
            .map(TryInto::try_into)
            .transpose()?
            .unwrap_or_default();

        Ok(chat::turn::Request::new(
            system,
            turns,
            tools,
            tool_choice,
            output_format,
        ))
    }
}

// -----------------------------------------------------------------------------
// ContentLower: Dispatches content according to its typed role.
// -----------------------------------------------------------------------------

/// Lower one Gemini content object according to its role.
///
/// # Errors
///
/// Returns [`GeminiError`] for an unsupported role or invalid role-specific part.
fn content_lower(content: Content, turns: &mut Vec<chat::turn::Turn>) -> Result<(), GeminiError> {
    let parts = content.parts.unwrap_or_default();
    match content.role.unwrap_or(ContentRole::User) {
        ContentRole::User => user_parts_lower(parts, turns, UserImagePolicy::Allow),
        ContentRole::Function => user_parts_lower(parts, turns, UserImagePolicy::Reject),
        ContentRole::Model => model_parts_lower(parts, turns),
        ContentRole::Unsupported => Err(GeminiError::UnsupportedRole),
    }
}

// -----------------------------------------------------------------------------
// UserImagePolicy: Controls image admission by Gemini content role.
// -----------------------------------------------------------------------------

/// Whether one Gemini role admits image parts.
#[derive(Clone, Copy, Eq, PartialEq)]
enum UserImagePolicy {
    /// Admit image parts on ordinary user content.
    Allow,
    /// Reject image parts on function-response content.
    Reject,
}

// -----------------------------------------------------------------------------
// User: Lowers user text and function responses in source order.
// -----------------------------------------------------------------------------

/// Flush accumulated user text and images before a function-response boundary.
fn user_content_flush(
    text: &mut Vec<String>,
    images: &mut Vec<image::source::Source>,
    turns: &mut Vec<chat::turn::Turn>,
) {
    // Empty buffers do not represent a conversation turn.
    if text.is_empty() && images.is_empty() {
        return;
    }
    turns.push(chat::turn::Turn::user_with_images(
        std::mem::take(text).join("\n"),
        std::mem::take(images),
    ));
}

/// Normalize one Gemini inline-image payload.
///
/// # Errors
///
/// Returns a typed failure for missing or unsupported inline data.
fn user_inline_image_source(
    inline: super::types::ContentInlineData,
) -> Result<image::source::Source, GeminiError> {
    let missing = || GeminiError::Image(image::errors::Error::MissingSource);
    let media_type = inline.mime_type.ok_or_else(missing)?;
    let data = inline.data.ok_or_else(missing)?;
    image::source::Source::typed_inline_base64(&media_type, data).map_err(GeminiError::Image)
}

/// Normalize one Gemini provider-file payload.
///
/// # Errors
///
/// Returns a typed failure for a missing or unsupported file reference.
fn user_file_image_source(
    file: super::types::ContentFileData,
) -> Result<image::source::Source, GeminiError> {
    let missing = || GeminiError::Image(image::errors::Error::MissingSource);
    let media_type = file.mime_type.ok_or_else(missing)?;
    let uri = file.file_uri.ok_or_else(missing)?;
    image::source::Source::provider_reference(&uri, Some(&media_type)).map_err(GeminiError::Image)
}

/// Lower one required function response into replayable text.
///
/// # Errors
///
/// Returns [`GeminiError`] when the response payload is absent.
fn user_function_response_text(
    response: Option<ContentFunctionResponseValue>,
) -> Result<String, GeminiError> {
    match response.ok_or(GeminiError::MissingFunctionResponse)? {
        ContentFunctionResponseValue::Text(text) => Ok(text),
        ContentFunctionResponseValue::Object(object) => Ok(object.serialized()),
    }
}

/// Lower user-authored parts into neutral user and tool-result turns.
///
/// # Errors
///
/// Returns [`GeminiError`] when parts are empty, unsupported, or contain an
/// incomplete function response.
fn user_parts_lower(
    parts: Vec<ContentPart>,
    turns: &mut Vec<chat::turn::Turn>,
    image_policy: UserImagePolicy,
) -> Result<(), GeminiError> {
    // Gemini content must contain at least one typed part.
    if parts.is_empty() {
        return Err(GeminiError::MissingContentParts);
    }
    let mut text = Vec::new();
    let mut images = Vec::new();
    for part in parts {
        match part {
            ContentPart::Text { text: part, .. } => text.push(part),
            ContentPart::InlineData { inline_data } if image_policy == UserImagePolicy::Allow => {
                images.push(user_inline_image_source(inline_data)?);
            }
            ContentPart::FileData { file_data } if image_policy == UserImagePolicy::Allow => {
                images.push(user_file_image_source(file_data)?);
            }
            ContentPart::FunctionResponse { function_response } => {
                user_content_flush(&mut text, &mut images, turns);
                let response = user_function_response_text(function_response.response)?;
                turns.push(chat::turn::Turn::ToolResult(response));
            }
            // User content cannot originate model function calls.
            ContentPart::InlineData { .. }
            | ContentPart::FileData { .. }
            | ContentPart::FunctionCall { .. }
            | ContentPart::Unsupported { .. } => {
                return Err(GeminiError::UnsupportedUserPart);
            }
        }
    }
    user_content_flush(&mut text, &mut images, turns);
    Ok(())
}

// -----------------------------------------------------------------------------
// ModelPartsLower: Lowers model text and function calls in source order.
// -----------------------------------------------------------------------------

/// Lower model-authored parts into neutral assistant and tool-call turns.
///
/// # Errors
///
/// Returns [`GeminiError`] when a function call omits its name or a model part
/// has an unsupported shape.
fn model_parts_lower(
    parts: Vec<ContentPart>,
    turns: &mut Vec<chat::turn::Turn>,
) -> Result<(), GeminiError> {
    for part in parts {
        match part {
            ContentPart::Text { text, .. } => turns.push(chat::turn::Turn::Assistant(text)),
            ContentPart::FunctionCall { function_call } => {
                turns.push(function_call.try_into()?);
            }
            // Model content cannot contain client function responses.
            ContentPart::InlineData { .. }
            | ContentPart::FileData { .. }
            | ContentPart::FunctionResponse { .. }
            | ContentPart::Unsupported { .. } => {
                return Err(GeminiError::UnsupportedModelPart);
            }
        }
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// TextParts: Validates text-only Gemini content.
// -----------------------------------------------------------------------------

/// Join a required sequence of text-only parts.
///
/// # Errors
///
/// Returns [`GeminiError`] when the sequence is empty or contains a non-text part.
fn text_parts(parts: Vec<ContentPart>, param: &'static str) -> Result<String, GeminiError> {
    // Required text containers cannot be empty.
    if parts.is_empty() {
        return Err(GeminiError::MissingTextParts { param });
    }
    let text = parts
        .into_iter()
        .map(|part| match part {
            ContentPart::Text { text, .. } => Ok(text),
            ContentPart::InlineData { .. }
            | ContentPart::FileData { .. }
            | ContentPart::FunctionCall { .. }
            | ContentPart::FunctionResponse { .. }
            | ContentPart::Unsupported { .. } => Err(GeminiError::UnsupportedTextPart { param }),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(text.join("\n"))
}

// -----------------------------------------------------------------------------
// StreamRecords: Renders Gemini's streaming record sequence.
// -----------------------------------------------------------------------------

/// Render incremental text records followed by Gemini's terminal record.
fn stream_records_text(
    model: &ModelId,
    response: &chat::turn::Response,
    text: &str,
) -> Vec<GenerateContentResponse> {
    let mut records = stream_chunks(text)
        .into_iter()
        .map(|chunk| GenerateContentResponse {
            candidates: vec![GenerateCandidate {
                content: GenerateOutputContent {
                    role: "model",
                    parts: vec![GenerateOutputPart::Text { text: chunk }],
                },
                finish_reason: None,
                index: 0,
            }],
            model_version: model.clone(),
            usage_metadata: None,
        })
        .collect::<Vec<_>>();
    records.push(GenerateContentResponse {
        candidates: vec![GenerateCandidate {
            content: GenerateOutputContent {
                role: "model",
                parts: vec![GenerateOutputPart::Text {
                    text: String::new(),
                }],
            },
            finish_reason: Some(GenerateFinishReason::Stop),
            index: 0,
        }],
        model_version: model.clone(),
        usage_metadata: Some(response.usage.into()),
    });
    records
}

/// Render all streaming records for one completed neutral response.
fn stream_records(
    model: &ModelId,
    response: &chat::turn::Response,
) -> Vec<GenerateContentResponse> {
    match &response.output {
        chat::turn::Output::Text(text) => stream_records_text(model, response, text),
        chat::turn::Output::ToolCall(call) => vec![GenerateContentResponse {
            candidates: vec![GenerateCandidate {
                content: GenerateOutputContent {
                    role: "model",
                    parts: vec![GenerateOutputPart::FunctionCall {
                        function_call: GenerateFunctionCall {
                            id: format!("call_{}", Uuid::now_v7().simple()),
                            name: call.name.clone(),
                            args: call.arguments.clone(),
                        },
                    }],
                },
                finish_reason: Some(GenerateFinishReason::Stop),
                index: 0,
            }],
            model_version: model.clone(),
            usage_metadata: Some(response.usage.into()),
        }],
    }
}

// -----------------------------------------------------------------------------
// GenerateTransport: Owns text response rendering after execution.
// -----------------------------------------------------------------------------

/// Completed Gemini text result with its selected delivery contract.
struct GenerateTransport {
    /// Unary or streaming action selected by the path.
    kind: GeminiActionKind,
    /// Optional streaming media selector.
    format: Option<GenerateStreamFormat>,
    /// Provider-visible model parsed from the request path.
    model: ModelId,
    /// Completed neutral result.
    response: chat::turn::Response,
    /// Optional delay between SSE events.
    delay_ms: u64,
}

impl GenerateTransport {
    /// Join provider output with its selected delivery contract.
    fn new(
        action: GeminiAction,
        format: Option<GenerateStreamFormat>,
        response: chat::turn::Response,
        delay_ms: u64,
    ) -> Self {
        Self {
            kind: action.kind,
            format,
            model: action.model,
            response,
            delay_ms,
        }
    }
}

impl IntoResponse for GenerateTransport {
    fn into_response(self) -> Response {
        // Unary generation has no incremental transport to render.
        if !self.kind.is_stream() {
            return Json(GenerateContentResponse::from_compat(
                self.model,
                self.response,
            ))
            .into_response();
        }
        let records = stream_records(&self.model, &self.response);

        // Explicit SSE selection bypasses JSON-array rendering.
        if matches!(self.format, Some(GenerateStreamFormat::Sse)) {
            let events = records
                .iter()
                .map(json_event)
                .collect::<Result<Vec<Event>, _>>();
            return match events {
                Ok(events) => SseEvents::from(events)
                    .with_delay(self.delay_ms)
                    .into_response(),
                Err(error) => GeminiRejection::from_error(&error).into_response(),
            };
        }
        Json(records).into_response()
    }
}

// -----------------------------------------------------------------------------
// Generate: Authenticates and executes native Gemini generation.
// -----------------------------------------------------------------------------

/// Handle one unary or streaming Gemini generation request.
async fn generate(
    State(state): State<AppState>,
    headers: HeaderMap,
    execution: Option<Extension<Execution>>,
    model_action: Result<Path<String>, PathRejection>,
    query: Result<Query<GenerateQuery>, QueryRejection>,
    payload: Result<Json<GenerateContentRequest>, JsonRejection>,
) -> Response {
    // Authentication failures use Gemini's native error envelope.
    if let Err(error) = state
        .config
        .authenticate(&headers, ProviderAuth::ApiKey("x-goog-api-key"))
    {
        return GeminiRejection::from_error(&error).into_response();
    }

    // Normalize transport extraction before interpreting provider semantics.
    let Path(model_action) = match model_action {
        Ok(model_action) => model_action,
        // Path extraction failures retain their typed diagnostic code.
        Err(error) => {
            return GeminiRejection::from_error(&ExtractionError::from(error)).into_response();
        }
    };

    // Decode optional stream formatting from the query string.
    let Query(query) = match query {
        Ok(query) => query,
        // Query extraction failures retain their typed diagnostic code.
        Err(error) => {
            return GeminiRejection::from_error(&ExtractionError::from(error)).into_response();
        }
    };

    // Decode the provider request body only after path and query extraction.
    let Json(payload) = match payload {
        Ok(payload) => payload,
        // Body extraction failures retain their typed diagnostic code.
        Err(error) => {
            return GeminiRejection::from_error(&ExtractionError::from(error)).into_response();
        }
    };

    // Decode the model and generation action carried by Gemini's path grammar.
    let action = match model_action.parse::<GeminiAction>() {
        Ok(action) => action,
        // Invalid path grammar uses Gemini's native error envelope.
        Err(error) => {
            return GeminiRejection::from_error(&error).into_response();
        }
    };

    let delivery = if !action.kind.is_stream() {
        GenerateDelivery::Unary
    } else if matches!(query.alt, Some(GenerateStreamFormat::Sse)) {
        GenerateDelivery::Sse
    } else {
        GenerateDelivery::JsonStream
    };

    let mode = match GenerateRequestMode::for_payload(&action.model, &payload) {
        Ok(mode) => mode,
        // Invalid modality controls cannot proceed to provider lowering.
        Err(error) => return GeminiRejection::from_error(&error).into_response(),
    };

    let execution = execution.map(|Extension(value)| value);
    match mode {
        // Audio generation owns its provider-native response.
        GenerateRequestMode::Audio => {
            return super::speech::generate(&state, action.model, delivery, payload, execution)
                .await;
        }
        // Image generation owns its provider-native multipart response.
        GenerateRequestMode::Image { composition } => {
            let transport =
                ImageTransport::generate(&state, action.model, delivery, payload, composition);
            return transport.into_response();
        }
        GenerateRequestMode::Text => {}
    }

    // The dedicated speech model does not silently fall back to text.
    if action.model.as_str() == speech::core::MODEL_ID {
        return GeminiRejection::from_error(&GeminiError::SpeechModelAudioOnly).into_response();
    }

    // Lower and execute the provider request under shared resource limits.
    // Render provider validation failures with Gemini's native envelope.
    let request: chat::turn::Request = match payload.try_into() {
        Ok(request) => request,
        // Invalid provider input cannot proceed to neutral execution.
        Err(error) => {
            return GeminiRejection::from_error(&error).into_response();
        }
    };

    // Apply shared resource bounds to the lowered request.
    // Complete one neutral turn before rendering Gemini output.
    let response = match request.complete(
        state.config.limits.max_input_chars(),
        state.config.limits.max_history_messages(),
    ) {
        Ok(response) => response,
        // Shared execution failures still render as Gemini errors.
        Err(error) => {
            return GeminiRejection::from(&error).into_response();
        }
    };

    // Hand the completed turn back to the selected Gemini transport.
    GenerateTransport::new(action, query.alt, response, state.config.stream_delay_ms)
        .into_response()
}

// -----------------------------------------------------------------------------
// Router: Publishes native Gemini generation endpoints.
// -----------------------------------------------------------------------------

/// Build unary and streaming model actions.
pub(super) fn router() -> ApiRouter<AppState> {
    ApiRouter::new().api_route(
        "/v1beta/models/{model_action}",
        post_with(generate, |operation| {
            operation
                .summary("Gemini content")
                .tag("gemini")
                .response::<200, Json<GenerateContentResponse>>()
                .response::<429, Json<GeminiFailureResponse>>()
                .response::<503, Json<GeminiFailureResponse>>()
                .default_response::<Json<GeminiFailureResponse>>()
        })
        .layer(DefaultBodyLimit::max(image::limits::LIMIT_JSON_BODY)),
    )
}
