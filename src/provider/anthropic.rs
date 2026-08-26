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

use super::{
    CompatTurnRequest, ModelId, ProviderRejection, TextArrayKind, TokenUsage, complete_eliza,
    optional_text_content, sse_response, stream_chunks,
};
use crate::serve::{AppState, require_provider_auth};

// -----------------------------------------------------------------------------
// Anthropic request contract: model the text-bearing subset accepted by
// Messages without owning provider features ELIZA cannot replay.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
struct AnthropicMessage {
    role: String,
    #[serde(default)]
    content: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MessagesRequest {
    model: ModelId,
    #[serde(default)]
    system: Value,
    messages: Vec<AnthropicMessage>,
    #[serde(default)]
    stream: bool,
}

// -----------------------------------------------------------------------------
// Anthropic response contracts: local structs mirror the JSON envelopes emitted
// by this adapter.
// -----------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct TextBlock {
    #[serde(rename = "type")]
    block_type: &'static str,
    text: String,
}

#[derive(Debug, Serialize)]
struct AnthropicUsage {
    input_tokens: usize,
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

#[derive(Debug, Serialize)]
struct MessagesResponse {
    id: String,
    #[serde(rename = "type")]
    response_type: &'static str,
    role: &'static str,
    model: ModelId,
    content: Vec<TextBlock>,
    stop_reason: &'static str,
    stop_sequence: Option<&'static str>,
    usage: AnthropicUsage,
}

#[derive(Debug, Serialize)]
struct AnthropicFailureBody {
    #[serde(rename = "type")]
    error_type: &'static str,
    message: String,
}

#[derive(Debug, Serialize)]
struct AnthropicFailureResponse {
    #[serde(rename = "type")]
    response_type: &'static str,
    error: AnthropicFailureBody,
}

// -----------------------------------------------------------------------------
// Anthropic lowering: system text blocks and user turns become a bounded replay
// transcript; assistant turns are history already represented by user turns.
// -----------------------------------------------------------------------------

/// Lower an Anthropic Messages request into the provider-neutral replay shape.
///
/// # Errors
///
/// Returns [`ProviderRejection`] when message roles are unsupported or content
/// blocks are not textual.
fn lower_messages(payload: MessagesRequest) -> Result<CompatTurnRequest, ProviderRejection> {
    let mut system_text = Vec::new();
    // --- Anthropic allows system text outside the message list; keep it for
    // limits and accounting without replaying it as a user turn.
    if let Some(text) = optional_text_content(&payload.system, "system", TextArrayKind::Blocks)? {
        system_text.push(text);
    }

    let mut user_turns = Vec::new();
    for message in payload.messages {
        match message.role.as_str() {
            // --- User turns are the only Anthropic messages that become ELIZA
            // conversation input.
            "user" => {
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
            // --- Assistant turns are previous model output supplied by the
            // client; they are history context, not new ELIZA input.
            "assistant" => {}
            _ => {
                return Err(ProviderRejection::unsupported(
                    "messages.role",
                    format!("unsupported message role `{}`", message.role),
                ));
            }
        }
    }

    Ok(CompatTurnRequest::new(
        payload.model,
        system_text,
        user_turns,
    ))
}

// -----------------------------------------------------------------------------
// Anthropic streaming and failures: named SSE events and error bodies stay in
// Anthropic's public vocabulary.
// -----------------------------------------------------------------------------

/// Render Anthropic's named SSE event sequence.
///
/// # Panics
///
/// Panics only if the locally constructed JSON event payload cannot serialize.
fn message_stream(output: &str, model: &ModelId, output_tokens: usize, delay_ms: u64) -> Response {
    let id = format!("msg_{}", Uuid::now_v7().simple());
    let mut events = Vec::new();
    // --- Anthropic streams open with a message envelope before any content
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
    // --- The single ELIZA response is represented as one text content block.
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

    // --- Chunk only the text delta; the surrounding block/message events stay
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

    // --- Close the content block before emitting final message-level usage.
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
    // --- Anthropic clients expect a distinct terminal event after the delta.
    events.push(
        Event::default()
            .event("message_stop")
            .json_data(json!({ "type": "message_stop" }))
            .expect("Anthropic message_stop should serialize"),
    );

    sse_response(events, delay_ms)
}

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
// Anthropic route: authenticate, lower Messages JSON, execute replay, and render
// either the final message or named event stream.
// -----------------------------------------------------------------------------

pub(crate) async fn messages(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<MessagesRequest>,
) -> Response {
    if let Err(error) = require_provider_auth(&headers, &state.config) {
        return error_response(error);
    }

    // --- Capture the stream preference before lowering consumes the payload.
    let stream = payload.stream;
    let request = match lower_messages(payload) {
        Ok(request) => request,
        Err(error) => return error_response(error),
    };
    let response = match complete_eliza(request, state.config.limits()) {
        Ok(response) => response,
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
