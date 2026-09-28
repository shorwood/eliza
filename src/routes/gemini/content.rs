//! Native Gemini Generate Content adapter.
use std::str::FromStr;

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use super::super::context::{AppState, ProviderAuth, provider_authenticate};
use super::errors::{GeminiError, GeminiFailureResponse, GeminiRejection};
use super::types::{
    Content, ContentFunctionResponseValue, ContentPart, ContentRole, GenerateCandidate,
    GenerateContentRequest, GenerateContentResponse, GenerateFinishReason, GenerateFunctionCall,
    GenerateOutputContent, GenerateOutputPart, GenerateQuery, GenerateStreamFormat, ToolConfig,
};
use crate::routes::errors::ExtractionError;
use crate::types::http::{SseEvents, json_event, stream_chunks};
use crate::types::model::ModelId;
use crate::types::turn::{
    CompatOutput, CompatTurn, CompatTurnRequest, CompatTurnResponse, RequestLimits,
};

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
// RequestLower: Lowers the complete Gemini request envelope.
// -----------------------------------------------------------------------------

/// Lower one Gemini request into the provider-neutral execution contract.
///
/// # Errors
///
/// Returns [`GeminiError`] when instructions, content, tools, or tool policy
/// cannot be represented by the neutral contract.
fn request_lower(
    model: ModelId,
    payload: GenerateContentRequest,
) -> Result<CompatTurnRequest, GeminiError> {
    let mut system = Vec::new();
    if let Some(instruction) = payload.system_instruction {
        system.push(text_parts(
            instruction.parts.unwrap_or_default(),
            "systemInstruction.parts",
        )?);
    }

    let mut turns = Vec::new();
    for content in payload.contents.unwrap_or_default() {
        content_lower(content, &mut turns)?;
    }

    // Assemble the neutral request after all provider content is lowered.
    Ok(CompatTurnRequest::new(
        model,
        system,
        turns,
        payload.tools.unwrap_or_default().into_domain()?,
        ToolConfig::into_domain(payload.tool_config, "toolConfig")?,
    ))
}

// -----------------------------------------------------------------------------
// Content: Dispatches content according to its typed role.
// -----------------------------------------------------------------------------

/// Lower one Gemini content object according to its role.
///
/// # Errors
///
/// Returns [`GeminiError`] for an unsupported role or invalid role-specific part.
fn content_lower(content: Content, turns: &mut Vec<CompatTurn>) -> Result<(), GeminiError> {
    let parts = content.parts.unwrap_or_default();
    match content.role.unwrap_or(ContentRole::User) {
        ContentRole::User | ContentRole::Function => user_parts_lower(parts, turns),
        ContentRole::Model => model_parts_lower(parts, turns),
        ContentRole::Unsupported => Err(GeminiError::UnsupportedRole),
    }
}

// -----------------------------------------------------------------------------
// User: Lowers user text and function responses in source order.
// -----------------------------------------------------------------------------

/// Flush accumulated user text before a function-response boundary.
fn user_text_flush(text: &mut Vec<String>, turns: &mut Vec<CompatTurn>) {
    // Empty buffers do not represent a conversation turn.
    if text.is_empty() {
        return;
    }
    turns.push(CompatTurn::User(std::mem::take(text).join("\n")));
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
    turns: &mut Vec<CompatTurn>,
) -> Result<(), GeminiError> {
    // Gemini content must contain at least one typed part.
    if parts.is_empty() {
        return Err(GeminiError::MissingContentParts);
    }
    let mut text = Vec::new();
    for part in parts {
        match part {
            ContentPart::Text { text: part } => text.push(part),
            ContentPart::FunctionResponse { function_response } => {
                user_text_flush(&mut text, turns);
                let response = user_function_response_text(function_response.response)?;
                turns.push(CompatTurn::ToolResult(response));
            }
            // User content cannot originate model function calls.
            ContentPart::FunctionCall { .. } | ContentPart::Unsupported { .. } => {
                return Err(GeminiError::UnsupportedUserPart);
            }
        }
    }
    user_text_flush(&mut text, turns);
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
    turns: &mut Vec<CompatTurn>,
) -> Result<(), GeminiError> {
    for part in parts {
        match part {
            ContentPart::Text { text } => turns.push(CompatTurn::Assistant(text)),
            ContentPart::FunctionCall { function_call } => {
                turns.push(function_call.try_into()?);
            }
            // Model content cannot contain client function responses.
            ContentPart::FunctionResponse { .. } | ContentPart::Unsupported { .. } => {
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
            ContentPart::Text { text } => Ok(text),
            ContentPart::FunctionCall { .. }
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
fn stream_records_text(response: &CompatTurnResponse, text: &str) -> Vec<GenerateContentResponse> {
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
            model_version: response.model.clone(),
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
        model_version: response.model.clone(),
        usage_metadata: Some(response.usage.into()),
    });
    records
}

/// Render all streaming records for one completed neutral response.
fn stream_records(response: &CompatTurnResponse) -> Vec<GenerateContentResponse> {
    match &response.output {
        CompatOutput::Text(text) => stream_records_text(response, text),
        CompatOutput::ToolCall(call) => vec![GenerateContentResponse {
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
            model_version: response.model.clone(),
            usage_metadata: Some(response.usage.into()),
        }],
    }
}

// -----------------------------------------------------------------------------
// Generate: Authenticates and executes native Gemini generation.
// -----------------------------------------------------------------------------

/// Handle one unary or streaming Gemini generation request.
async fn generate(
    State(state): State<AppState>,
    headers: HeaderMap,
    model_action: Result<Path<String>, PathRejection>,
    query: Result<Query<GenerateQuery>, QueryRejection>,
    payload: Result<Json<GenerateContentRequest>, JsonRejection>,
) -> Response {
    // Authentication failures use Gemini's native error envelope.
    if let Err(error) = provider_authenticate(
        &headers,
        &state.config,
        ProviderAuth::ApiKey("x-goog-api-key"),
    ) {
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

    // Lower and execute the provider request under shared resource limits.
    let request = match request_lower(action.model.clone(), payload) {
        Ok(request) => request,
        // Provider validation failures use Gemini's native envelope.
        Err(error) => {
            return GeminiRejection::from_error(&error).into_response();
        }
    };

    // Apply shared resource bounds to the lowered request.
    let limits = RequestLimits::new(
        state.config.max_input_chars,
        state.config.max_history_messages,
    );

    // Complete one neutral turn before rendering Gemini output.
    let response = match request.complete(limits) {
        Ok(response) => response,
        // Shared execution failures still render as Gemini errors.
        Err(error) => {
            return GeminiRejection::from_error(&error).into_response();
        }
    };

    // Unary actions return one complete GenerateContent response.
    if !action.kind.is_stream() {
        return Json(GenerateContentResponse::from(response)).into_response();
    }
    let records = stream_records(&response);

    // Choose SSE only when the query explicitly requests it.
    if matches!(query.alt, Some(GenerateStreamFormat::Sse)) {
        let events = records
            .iter()
            .map(json_event)
            .collect::<Result<Vec<Event>, _>>();

        // Finish the request through an SSE response or native encoding error.
        return match events {
            Ok(events) => SseEvents::from(events)
                .with_delay(state.config.stream_delay_ms)
                .into_response(),
            Err(error) => GeminiRejection::from_error(&error).into_response(),
        };
    }
    Json(records).into_response()
}

// -----------------------------------------------------------------------------
// Route: Mounts native Gemini generation operations.
// -----------------------------------------------------------------------------

/// Native Gemini generation route.
pub(super) struct Route;

impl Route {
    /// Mount unary and streaming model actions.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/v1beta/models/{model_action}",
            post_with(generate, |operation| {
                operation
                    .summary("Gemini content")
                    .tag("gemini")
                    .response::<200, Json<GenerateContentResponse>>()
                    .default_response::<Json<GeminiFailureResponse>>()
            }),
        )
    }
}
