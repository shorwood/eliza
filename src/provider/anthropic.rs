//! Anthropic Messages wire adapter.
//!
//! The Messages route accepts Anthropic text blocks, lowers them into
//! `CompatTurnRequest`, and renders either a single message body or Anthropic's
//! named SSE event sequence.
//! The adapter accepts only text blocks because image/tool/content-block
//! variants have no representation in the historical ELIZA script.

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::contracts::{
    CompatTurnRequest, ModelId, ProviderRejection, RequestLimits, SseEvents, TextArrayKind,
    TokenUsage, optional_text_content, stream_chunks,
};
use crate::serve::{AppState, provider_authenticate};

// -----------------------------------------------------------------------------
// AnthropicMessage: Models one replayable input message.
// -----------------------------------------------------------------------------

/// Represents `AnthropicMessage` state within this module.
#[derive(Debug, Deserialize, JsonSchema)]
struct AnthropicMessage {
    /// Stores the role value owned by this contract.
    role: String,
    /// Stores the content value owned by this contract.
    #[serde(default = "missing_content_is_null")]
    content: Value,
}

// -----------------------------------------------------------------------------
// MessagesRequest: Models the accepted Anthropic request.
// -----------------------------------------------------------------------------

/// Represents `MessagesRequest` state within this module.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MessagesRequest {
    /// Stores the model value owned by this contract.
    model: ModelId,
    /// Stores the system value owned by this contract.
    #[serde(default = "missing_content_is_null")]
    system: Value,
    /// Stores the messages value owned by this contract.
    messages: Vec<AnthropicMessage>,
    /// Stores the stream value owned by this contract.
    #[serde(default = "should_stream_by_default", rename = "stream")]
    should_stream: bool,
}

/// Preserve provider compatibility by treating omitted content as JSON null.
fn missing_content_is_null() -> Value {
    Value::Null
}

/// Preserve non-streaming behavior when clients omit the stream flag.
const fn should_stream_by_default() -> bool {
    false
}

// -----------------------------------------------------------------------------
// TextBlock: Models one Anthropic output text block.
// -----------------------------------------------------------------------------

/// Represents `TextBlock` state within this module.
#[derive(Debug, Serialize)]
struct TextBlock {
    /// Stores the block type value owned by this contract.
    #[serde(rename = "type")]
    block_type: &'static str,
    /// Stores the text value owned by this contract.
    text: String,
}

// -----------------------------------------------------------------------------
// AnthropicUsage: Models Anthropic token accounting.
// -----------------------------------------------------------------------------

/// Represents `AnthropicUsage` state within this module.
#[derive(Debug, Serialize)]
struct AnthropicUsage {
    /// Stores the input tokens value owned by this contract.
    input_tokens: usize,
    /// Stores the output tokens value owned by this contract.
    output_tokens: usize,
}

impl From<TokenUsage> for AnthropicUsage {
    fn from(usage: TokenUsage) -> Self {
        Self {
            input_tokens: usage.prompt,
            output_tokens: usage.completion,
        }
    }
}

// -----------------------------------------------------------------------------
// MessagesResponse: Models one successful Anthropic response.
// -----------------------------------------------------------------------------

/// Represents `MessagesResponse` state within this module.
#[derive(Debug, Serialize)]
struct MessagesResponse {
    /// Stores the id value owned by this contract.
    id: String,
    /// Stores the response type value owned by this contract.
    #[serde(rename = "type")]
    response_type: &'static str,
    /// Stores the role value owned by this contract.
    role: &'static str,
    /// Stores the model value owned by this contract.
    model: ModelId,
    /// Stores the content value owned by this contract.
    content: Vec<TextBlock>,
    /// Stores the stop reason value owned by this contract.
    stop_reason: &'static str,
    /// Stores the stop sequence value owned by this contract.
    stop_sequence: Option<&'static str>,
    /// Stores the usage value owned by this contract.
    usage: AnthropicUsage,
}

// -----------------------------------------------------------------------------
// AnthropicFailure: Models Anthropic error envelopes.
// -----------------------------------------------------------------------------

/// Represents `AnthropicFailureBody` state within this module.
#[derive(Debug, Serialize)]
struct AnthropicFailureBody {
    /// Stores the error type value owned by this contract.
    #[serde(rename = "type")]
    error_type: &'static str,
    /// Stores the message value owned by this contract.
    message: String,
}

/// Represents `AnthropicFailureResponse` state within this module.
#[derive(Debug, Serialize)]
struct AnthropicFailureResponse {
    /// Stores the response type value owned by this contract.
    #[serde(rename = "type")]
    response_type: &'static str,
    /// Stores the error value owned by this contract.
    error: AnthropicFailureBody,
}

// -----------------------------------------------------------------------------
// AnthropicTurn: Lowers messages into bounded replay turns.
// -----------------------------------------------------------------------------

/// Lower an Anthropic Messages request into the provider-neutral replay shape.
///
/// # Errors
///
/// Returns [`ProviderRejection`] when message roles are unsupported or content
/// blocks are not textual.
struct AnthropicTurn(
    /// Provider-neutral request produced from Anthropic input.
    CompatTurnRequest,
);

impl TryFrom<MessagesRequest> for AnthropicTurn {
    type Error = ProviderRejection;

    fn try_from(payload: MessagesRequest) -> Result<Self, Self::Error> {
        let mut system_text = Vec::new();

        // Anthropic allows system text outside the message list; keep it for
        // limits and accounting without replaying it as a user turn.
        if let Some(text) = optional_text_content(&payload.system, "system", TextArrayKind::Blocks)?
        {
            system_text.push(text);
        }

        let mut user_turns = Vec::new();
        for message in payload.messages {
            match message.role.as_str() {
                // User turns are the only Anthropic messages that become ELIZA
                // conversation input.
                "user" => {
                    // User turns must contain text that ELIZA can replay.
                    let Some(text) = optional_text_content(
                        &message.content,
                        "messages.content",
                        TextArrayKind::Blocks,
                    )?
                    else {
                        return Err(ProviderRejection::invalid(
                            "messages",
                            "user message content must contain text",
                        ));
                    };
                    user_turns.push(text);
                }
                // Assistant turns are previous model output supplied by the
                // client; they are history context, not new ELIZA input.
                "assistant" => {}
                // Other roles cannot be represented in the shared transcript.
                _ => {
                    return Err(ProviderRejection::unsupported(
                        "messages.role",
                        format!("unsupported message role `{}`", message.role),
                    ));
                }
            }
        }

        Ok(Self(CompatTurnRequest::new(
            payload.model,
            system_text,
            user_turns,
        )))
    }
}

// -----------------------------------------------------------------------------
// MessageStream: Renders named Anthropic SSE events.
// -----------------------------------------------------------------------------

/// Render Anthropic's named SSE event sequence.
///
/// # Panics
///
/// Panics only if the locally constructed JSON event payload cannot serialize.
fn message_stream(output: &str, model: &ModelId, output_tokens: usize, delay_ms: u64) -> Response {
    let id = format!("msg_{}", Uuid::now_v7().simple());
    let mut events = Vec::new();

    // Anthropic streams open with a message envelope before any content
    // block exists.
    events.push(
        Event::default()
            .event("message_start")
            .json_data(json!({
                "type": "message_start",
                "message": {
                    "id": id,
                    "type": "message",
                    "role": "assistant",
                    "model": model.as_str(),
                    "content": [],
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": { "input_tokens": 0, "output_tokens": 0 }
                }
            }))
            .expect("Anthropic message_start should serialize"),
    );

    // The single ELIZA response is represented as one text content block.
    events.push(
        Event::default()
            .event("content_block_start")
            .json_data(json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": { "type": "text", "text": "" }
            }))
            .expect("Anthropic content_block_start should serialize"),
    );

    // Chunk only the text delta; the surrounding block/message events stay
    // stable so SDK state machines can parse the stream.
    for chunk in stream_chunks(output) {
        events.push(
            Event::default()
                .event("content_block_delta")
                .json_data(json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": { "type": "text_delta", "text": chunk }
                }))
                .expect("Anthropic content_block_delta should serialize"),
        );
    }

    // Close the content block before emitting final message-level usage.
    events.push(
        Event::default()
            .event("content_block_stop")
            .json_data(json!({ "type": "content_block_stop", "index": 0 }))
            .expect("Anthropic content_block_stop should serialize"),
    );
    events.push(
        Event::default()
            .event("message_delta")
            .json_data(json!({
                "type": "message_delta",
                "delta": { "stop_reason": "end_turn", "stop_sequence": null },
                "usage": { "output_tokens": output_tokens }
            }))
            .expect("Anthropic message_delta should serialize"),
    );

    // Anthropic clients expect a distinct terminal event after the delta.
    events.push(
        Event::default()
            .event("message_stop")
            .json_data(json!({ "type": "message_stop" }))
            .expect("Anthropic message_stop should serialize"),
    );

    SseEvents::from(events).into_response(delay_ms)
}

// -----------------------------------------------------------------------------
// ErrorResponse: Renders Anthropic failures.
// -----------------------------------------------------------------------------

/// Performs the error response operation for this abstraction.
fn error_response(error: ProviderRejection) -> Response {
    (
        error.status,
        Json(AnthropicFailureResponse {
            response_type: "error",
            error: AnthropicFailureBody {
                error_type: error.openai_type,
                message: error.message,
            },
        }),
    )
        .into_response()
}

// -----------------------------------------------------------------------------
// Messages: Authenticates and executes Messages requests.
// -----------------------------------------------------------------------------

/// Performs the messages operation for this abstraction.
pub(crate) async fn messages(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<MessagesRequest>,
) -> Response {
    // Render authentication failures in Anthropic's envelope immediately.
    if let Err(error) = provider_authenticate(&headers, &state.config) {
        return error_response(error);
    }

    // Capture the stream preference before lowering consumes the payload.
    let stream = payload.should_stream;
    let request = match AnthropicTurn::try_from(payload) {
        Ok(request) => request.0,
        // Invalid payloads stop before the ELIZA engine is invoked.
        Err(error) => return error_response(error),
    };
    let limits = RequestLimits::new(
        state.config.max_input_chars,
        state.config.max_history_messages,
    );
    let response = match request.complete(limits) {
        Ok(response) => response,
        // Engine rejections retain Anthropic's error shape.
        Err(error) => return error_response(error),
    };

    if stream {
        message_stream(
            &response.output,
            &response.model,
            response.usage.completion,
            state.config.stream_delay_ms,
        )
    } else {
        Json(MessagesResponse {
            id: format!("msg_{}", Uuid::now_v7().simple()),
            response_type: "message",
            role: "assistant",
            model: response.model,
            content: vec![TextBlock {
                block_type: "text",
                text: response.output,
            }],
            stop_reason: "end_turn",
            stop_sequence: None,
            usage: AnthropicUsage::from(response.usage),
        })
        .into_response()
    }
}
