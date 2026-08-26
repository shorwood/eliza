//! OpenAI-compatible wire adapter.
//!
//! Chat Completions and Responses both lower into `CompatTurnRequest`; the
//! Gemini `OpenAI` alias reuses the same handler so clients pointed at
//! `/v1beta/openai` see the `OpenAI` response contract.
//! The adapter accepts only textual message content. `OpenAI` features that need
//! tool calls, multimodal parts, structured output, or assistant-state replay do
//! not have an ELIZA equivalent and are rejected or ignored at lowering time.

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
    optional_text_content, sse_response, stream_chunks, unix_timestamp,
};
use crate::serve::{AppState, require_provider_auth};

// -----------------------------------------------------------------------------
// OpenAI request contracts: model only the text-bearing fields needed for replay.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
struct OpenAiMessage {
    role: String,
    #[serde(default)]
    content: Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ChatCompletionRequest {
    model: ModelId,
    messages: Vec<OpenAiMessage>,
    #[serde(default)]
    stream: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ResponsesRequest {
    model: ModelId,
    input: Value,
    #[serde(default)]
    stream: bool,
}

// -----------------------------------------------------------------------------
// OpenAI response contracts: local structs mirror the Chat Completions,
// Responses, model-list, and error envelopes emitted by this adapter.
// -----------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct ModelDescriptor {
    id: ModelId,
    object: &'static str,
    created: u64,
    owned_by: &'static str,
}

#[derive(Debug, Serialize)]
struct ModelListResponse {
    object: &'static str,
    data: Vec<ModelDescriptor>,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: &'static str,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatChoice {
    index: usize,
    message: ChatMessage,
    finish_reason: &'static str,
}

#[derive(Debug, Serialize)]
struct ChatCompletionResponse {
    id: String,
    object: &'static str,
    created: u64,
    model: ModelId,
    choices: Vec<ChatChoice>,
    usage: TokenUsage,
}

#[derive(Debug, Serialize)]
struct ResponseContent {
    #[serde(rename = "type")]
    content_type: &'static str,
    text: String,
    annotations: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct ResponseOutput {
    id: String,
    #[serde(rename = "type")]
    output_type: &'static str,
    status: &'static str,
    role: &'static str,
    content: Vec<ResponseContent>,
}

#[derive(Debug, Serialize)]
struct ResponsesUsage {
    #[serde(rename = "input_tokens")]
    input: usize,
    #[serde(rename = "output_tokens")]
    output: usize,
    #[serde(rename = "total_tokens")]
    total: usize,
}

impl From<TokenUsage> for ResponsesUsage {
    fn from(usage: TokenUsage) -> Self {
        Self {
            input: usage.prompt,
            output: usage.completion,
            total: usage.total,
        }
    }
}

#[derive(Debug, Serialize)]
struct ResponsesResponse {
    id: String,
    object: &'static str,
    created_at: u64,
    status: &'static str,
    model: ModelId,
    output: Vec<ResponseOutput>,
    output_text: String,
    usage: ResponsesUsage,
}

#[derive(Debug, Serialize)]
struct OpenAiFailureBody {
    message: String,
    #[serde(rename = "type")]
    error_type: &'static str,
    param: Option<&'static str>,
    code: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct OpenAiFailureResponse {
    error: OpenAiFailureBody,
}

// -----------------------------------------------------------------------------
// OpenAI lowering: accepted roles and content arrays become one bounded replay
// transcript for the historical engine.
// -----------------------------------------------------------------------------

/// Lower an `OpenAI` Chat Completions request into the replay shape.
///
/// # Errors
///
/// Returns [`ProviderRejection`] when message roles are unsupported or message
/// content is not textual.
fn lower_chat(payload: ChatCompletionRequest) -> Result<CompatTurnRequest, ProviderRejection> {
    let mut system_text = Vec::new();
    let mut user_turns = Vec::new();

    for message in payload.messages {
        match message.role.as_str() {
            // --- System/developer text is counted but not replayed; ELIZA has
            // no instruction channel distinct from the conversation text.
            "system" | "developer" => {
                if let Some(text) = optional_text_content(
                    &message.content,
                    "messages.content",
                    TextArrayKind::Parts,
                )? {
                    system_text.push(text);
                }
            }
            // --- User turns are the only inputs that become historical ELIZA
            // conversation state.
            "user" => {
                let Some(text) = optional_text_content(
                    &message.content,
                    "messages.content",
                    TextArrayKind::Parts,
                )?
                else {
                    return Err(ProviderRejection::invalid(
                        "messages",
                        "user message content must contain text",
                    ));
                };
                user_turns.push(text);
            }
            // --- Assistant/tool messages are already transcript history from
            // the client side; replaying them would make ELIZA answer itself.
            "assistant" | "tool" => {}
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

/// Lower an `OpenAI` Responses request into the replay shape.
///
/// # Errors
///
/// Returns [`ProviderRejection`] when the input shape is unsupported or message
/// content is not textual.
fn lower_responses(payload: ResponsesRequest) -> Result<CompatTurnRequest, ProviderRejection> {
    let mut user_turns = Vec::new();

    match payload.input {
        // --- The compact Responses form is one direct user turn.
        Value::String(text) => user_turns.push(text),
        // --- The array form is treated like message history and only user
        // entries are replayed.
        Value::Array(items) => {
            for item in items {
                let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
                if role == "user" {
                    let content = item.get("content").ok_or_else(|| {
                        ProviderRejection::invalid("input.content", "input item is missing content")
                    })?;
                    if let Some(text) =
                        optional_text_content(content, "input.content", TextArrayKind::Parts)?
                    {
                        user_turns.push(text);
                    }
                }
            }
        }
        _ => {
            return Err(ProviderRejection::unsupported(
                "input",
                "Responses input must be a string or an array of message-like objects",
            ));
        }
    }

    Ok(CompatTurnRequest::new(
        payload.model,
        Vec::new(),
        user_turns,
    ))
}

// -----------------------------------------------------------------------------
// OpenAI streaming and failures: chunks and errors stay OpenAI-shaped here.
// -----------------------------------------------------------------------------

/// Render `OpenAI` Chat Completions streaming chunks.
///
/// # Panics
///
/// Panics only if the locally constructed JSON chunk payload cannot serialize.
fn chat_stream(output: &str, model: &ModelId, delay_ms: u64) -> Response {
    let id = format!("chatcmpl-{}", Uuid::now_v7().simple());
    let created = unix_timestamp();
    let mut events = Vec::new();
    // --- OpenAI streams begin by announcing the assistant role before any
    // content deltas.
    events.push(
        Event::default()
            .json_data(json!({
                "id": id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model.as_str(),
                "choices": [{
                    "index": 0,
                    "delta": { "role": "assistant" },
                    "finish_reason": null
                }]
            }))
            .expect("OpenAI role chunk should serialize"),
    );

    // --- ELIZA emits one complete sentence; split it into deterministic text
    // deltas for SDK stream compatibility.
    for chunk in stream_chunks(output) {
        events.push(
            Event::default()
                .json_data(json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "created": created,
                    "model": model.as_str(),
                    "choices": [{
                        "index": 0,
                        "delta": { "content": chunk },
                        "finish_reason": null
                    }]
                }))
                .expect("OpenAI content chunk should serialize"),
        );
    }

    // --- Finish with an empty delta and the `[DONE]` sentinel used by OpenAI
    // clients to close the stream.
    events.push(
        Event::default()
            .json_data(json!({
                "id": id,
                "object": "chat.completion.chunk",
                "created": created,
                "model": model.as_str(),
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": "stop"
                }]
            }))
            .expect("OpenAI stop chunk should serialize"),
    );
    events.push(Event::default().data("[DONE]"));

    sse_response(events, delay_ms)
}

fn error_response(error: ProviderRejection) -> Response {
    (
        error.status,
        Json(OpenAiFailureResponse {
            error: OpenAiFailureBody {
                message: error.message,
                error_type: error.openai_type,
                param: error.param,
                code: None,
            },
        }),
    )
        .into_response()
}

// -----------------------------------------------------------------------------
// OpenAI routes: authenticate, lower the body, execute ELIZA, then render the
// OpenAI-compatible response shape.
// -----------------------------------------------------------------------------

pub(crate) async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(error) = require_provider_auth(&headers, &state.config) {
        return error_response(error);
    }

    // --- The server exposes exactly one configured model id.
    Json(ModelListResponse {
        object: "list",
        data: vec![ModelDescriptor {
            id: state.config.model.clone(),
            object: "model",
            created: 0,
            owned_by: "eliza",
        }],
    })
    .into_response()
}

pub(crate) async fn chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ChatCompletionRequest>,
) -> Response {
    if let Err(error) = require_provider_auth(&headers, &state.config) {
        return error_response(error);
    }

    // --- Capture the stream flag before lowering consumes the request body.
    let stream = payload.stream;
    let request = match lower_chat(payload) {
        Ok(request) => request,
        Err(error) => return error_response(error),
    };
    let response = match complete_eliza(request, state.config.limits()) {
        Ok(response) => response,
        Err(error) => return error_response(error),
    };

    if stream {
        chat_stream(
            &response.output,
            &response.model,
            state.config.stream_delay_ms,
        )
    } else {
        Json(ChatCompletionResponse {
            id: format!("chatcmpl-{}", Uuid::now_v7().simple()),
            object: "chat.completion",
            created: unix_timestamp(),
            model: response.model,
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant",
                    content: response.output,
                },
                finish_reason: "stop",
            }],
            usage: response.usage,
        })
        .into_response()
    }
}

pub(crate) async fn responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ResponsesRequest>,
) -> Response {
    if let Err(error) = require_provider_auth(&headers, &state.config) {
        return error_response(error);
    }

    // --- Responses streaming has a different event contract; keep the supported
    // streaming surface on Chat Completions until that shape earns its place.
    if payload.stream {
        return error_response(ProviderRejection::unsupported(
            "stream",
            "OpenAI Responses streaming is not implemented; use /v1/chat/completions streaming",
        ));
    }

    let request = match lower_responses(payload) {
        Ok(request) => request,
        Err(error) => return error_response(error),
    };
    let response = match complete_eliza(request, state.config.limits()) {
        Ok(response) => response,
        Err(error) => return error_response(error),
    };
    let output_id = format!("msg_{}", Uuid::now_v7().simple());
    let output_text = response.output;

    Json(ResponsesResponse {
        id: format!("resp_{}", Uuid::now_v7().simple()),
        object: "response",
        created_at: unix_timestamp(),
        status: "completed",
        model: response.model,
        output: vec![ResponseOutput {
            id: output_id,
            output_type: "message",
            status: "completed",
            role: "assistant",
            content: vec![ResponseContent {
                content_type: "output_text",
                text: output_text.clone(),
                annotations: Vec::new(),
            }],
        }],
        output_text,
        usage: ResponsesUsage::from(response.usage),
    })
    .into_response()
}
