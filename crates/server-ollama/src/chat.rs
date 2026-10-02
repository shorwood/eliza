//! Ollama chat adapter.
use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use eliza_http::context::ProviderAuth;
use eliza_http::extraction::ExtractionError;
use eliza_http::model::ModelId;
use eliza_http::response::{NdjsonResponse, stream_chunks};
use eliza_modality_chat as chat;
use eliza_modality_image as image;

use super::errors::{OllamaError, OllamaFailureResponse, OllamaRejection};
use super::types::{
    ChatOutputMessage, ChatRequest, ChatResponse, MessageContent, MessageRole, ToolCall,
};
use crate::context::AppState;

impl TryFrom<ChatRequest> for chat::turn::Request {
    type Error = OllamaError;

    fn try_from(payload: ChatRequest) -> Result<Self, Self::Error> {
        let output_format = payload
            .format
            .map(TryInto::try_into)
            .transpose()?
            .unwrap_or_default();

        // Reject requests for a hidden reasoning trace.
        if payload.think.is_some() {
            return Err(OllamaError::ReasoningUnsupported);
        }

        // Separate instructions from replayable conversation turns.
        let mut system = Vec::new();
        let mut turns = Vec::new();
        for message in payload.messages {
            let content = MessageContent::into_text(message.content);
            let images = message.images.unwrap_or_default();

            // Image payloads are valid only on user-authored turns.
            if !matches!(message.role, MessageRole::User) && !images.is_empty() {
                return Err(OllamaError::ImagesRequireUserRole);
            }

            // Lower each validated provider role into neutral conversation state.
            match message.role {
                MessageRole::System => system.push(content),
                MessageRole::User => {
                    let images = images
                        .into_iter()
                        .map(image::source::Source::inline_base64)
                        .collect::<Result<Vec<_>, _>>()?;
                    turns.push(chat::turn::Turn::user_with_images(content, images));
                }
                MessageRole::Assistant => {
                    assistant_lower(content, message.tool_calls, &mut turns)?;
                }
                MessageRole::Tool => turns.push(chat::turn::Turn::ToolResult(content)),
                // Unknown provider roles cannot be replayed safely.
                MessageRole::Unsupported => return Err(OllamaError::UnsupportedRole),
            }
        }

        // Lower tool declarations and selection policy independently.
        let tools = payload.tools.unwrap_or_default().try_into()?;
        let tool_choice = payload
            .tool_choice
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
// CreatedAt: Defines deterministic response timestamps.
// -----------------------------------------------------------------------------

/// Stable timestamp used by the timeless ELIZA algorithm.
const CREATED_AT: &str = "1966-01-01T00:00:00Z";

// -----------------------------------------------------------------------------
// AssistantLower: Lowers assistant text and tool calls in source order.
// -----------------------------------------------------------------------------

/// Lower assistant text and calls while preserving their provider order.
///
/// # Errors
///
/// Returns [`OllamaError`] when a tool call is unsupported or incomplete.
fn assistant_lower(
    content: String,
    tool_calls: Option<Vec<ToolCall>>,
    turns: &mut Vec<chat::turn::Turn>,
) -> Result<(), OllamaError> {
    if !content.is_empty() {
        turns.push(chat::turn::Turn::Assistant(content));
    }
    for call in tool_calls.unwrap_or_default() {
        turns.push(chat::turn::Turn::ToolCall(
            call.lower("messages.tool_calls")?,
        ));
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// StreamRecords: Renders Ollama's NDJSON response sequence.
// -----------------------------------------------------------------------------

/// Render one record for each deterministic text chunk.
fn stream_records_text(model: &ModelId, text: &str) -> Vec<ChatResponse> {
    stream_chunks(text)
        .into_iter()
        .map(|chunk| ChatResponse {
            model: model.clone(),
            created_at: CREATED_AT,
            message: ChatOutputMessage {
                role: "assistant",
                content: chunk,
                tool_calls: Vec::new(),
            },
            is_done: false,
            done_reason: None,
            total_duration: None,
            load_duration: None,
            prompt_eval_count: None,
            prompt_eval_duration: None,
            eval_count: None,
            eval_duration: None,
        })
        .collect()
}

/// Render all NDJSON records for one completed neutral response.
fn stream_records(model: &ModelId, response: &chat::turn::Response) -> Vec<ChatResponse> {
    let mut records = match &response.output {
        chat::turn::Output::Text(text) => stream_records_text(model, text),
        chat::turn::Output::ToolCall(_) => vec![ChatResponse {
            model: model.clone(),
            created_at: CREATED_AT,
            message: ChatOutputMessage::from(&response.output),
            is_done: false,
            done_reason: None,
            total_duration: None,
            load_duration: None,
            prompt_eval_count: None,
            prompt_eval_duration: None,
            eval_count: None,
            eval_duration: None,
        }],
    };
    records.push(ChatResponse {
        model: model.clone(),
        created_at: CREATED_AT,
        message: ChatOutputMessage {
            role: "assistant",
            content: String::new(),
            tool_calls: Vec::new(),
        },
        is_done: true,
        done_reason: Some("stop"),
        total_duration: Some(0),
        load_duration: Some(0),
        prompt_eval_count: Some(response.usage.prompt),
        prompt_eval_duration: Some(0),
        eval_count: Some(response.usage.completion),
        eval_duration: Some(0),
    });
    records
}

// -----------------------------------------------------------------------------
// Chat: Authenticates and executes Ollama chat requests.
// -----------------------------------------------------------------------------

/// Handle one unary or streaming Ollama chat request.
async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<ChatRequest>, JsonRejection>,
) -> Response {
    // Authentication failures use Ollama's native error envelope.
    if let Err(error) = state.config.authenticate(&headers, ProviderAuth::Bearer) {
        return OllamaRejection::from_error(&error).into_response();
    }
    let Json(payload) = match payload {
        Ok(payload) => payload,
        // Extraction failures retain their typed diagnostic code.
        Err(error) => {
            return OllamaRejection::from_error(&ExtractionError::from(error)).into_response();
        }
    };

    // Capture transport policy before lowering consumes the request body.
    let model = payload.model.clone();
    let should_stream = payload.should_stream.unwrap_or(true);
    let request: chat::turn::Request = match payload.try_into() {
        Ok(request) => request,
        // Provider validation failures use Ollama's native envelope.
        Err(error) => {
            return OllamaRejection::from_error(&error).into_response();
        }
    };

    // Enforce shared request bounds after provider-specific lowering.
    // Complete one neutral turn before rendering Ollama output.
    let response = match request.complete(
        state.config.limits.max_input_chars(),
        state.config.limits.max_history_messages(),
    ) {
        Ok(response) => response,
        // Shared execution failures still render as Ollama errors.
        Err(error) => {
            return OllamaRejection::from(&error).into_response();
        }
    };

    if should_stream {
        match NdjsonResponse::new(
            stream_records(&model, &response),
            state.config.stream_delay_ms,
        ) {
            Ok(response) => response.into_response(),
            Err(error) => OllamaRejection::from_error(&error).into_response(),
        }
    } else {
        Json(ChatResponse::from_compat(model, &response)).into_response()
    }
}

// -----------------------------------------------------------------------------
// Router: Publishes the native chat endpoint.
// -----------------------------------------------------------------------------

/// Build the native Ollama chat endpoint.
pub(super) fn router() -> ApiRouter<AppState> {
    ApiRouter::new().api_route(
        "/api/chat",
        post_with(chat, |operation| {
            operation
                .summary("Ollama chat")
                .tag("ollama")
                .response::<200, Json<ChatResponse>>()
                .default_response::<Json<OllamaFailureResponse>>()
        })
        .layer(DefaultBodyLimit::max(image::limits::LIMIT_JSON_BODY)),
    )
}
