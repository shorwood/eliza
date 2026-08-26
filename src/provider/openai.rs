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

use super::contracts::{
    CompatTurnRequest, CompatTurnResponse, ModelId, ProviderRejection, RequestLimits, SseEvents,
    TextArrayKind, TokenUsage, optional_text_content, stream_chunks, unix_timestamp,
};
use crate::serve::{AppState, provider_authenticate};

// -----------------------------------------------------------------------------
// OpenAiMessage: Models one accepted chat message.
// -----------------------------------------------------------------------------

/// Represents `OpenAiMessage` state within this module.
#[derive(Debug, Deserialize, JsonSchema)]
struct OpenAiMessage {
    /// Stores the role value owned by this contract.
    role: String,
    /// Stores the content value owned by this contract.
    #[serde(default = "missing_content_is_null")]
    content: Value,
}

// -----------------------------------------------------------------------------
// ChatCompletionRequest: Models the accepted chat request.
// -----------------------------------------------------------------------------

/// Represents `ChatCompletionRequest` state within this module.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ChatCompletionRequest {
    /// Stores the model value owned by this contract.
    model: ModelId,
    /// Stores the messages value owned by this contract.
    messages: Vec<OpenAiMessage>,
    /// Stores the stream value owned by this contract.
    #[serde(default = "should_stream_by_default", rename = "stream")]
    should_stream: bool,
}

// -----------------------------------------------------------------------------
// ResponsesRequest: Models the accepted Responses request.
// -----------------------------------------------------------------------------

/// Represents `ResponsesRequest` state within this module.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ResponsesRequest {
    /// Stores the model value owned by this contract.
    model: ModelId,
    /// Stores the input value owned by this contract.
    input: Value,
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
// Model: Models the OpenAI model catalog.
// -----------------------------------------------------------------------------

/// Represents `ModelDescriptor` state within this module.
#[derive(Debug, Serialize)]
struct ModelDescriptor {
    /// Stores the id value owned by this contract.
    id: ModelId,
    /// Stores the object value owned by this contract.
    object: &'static str,
    /// Stores the created value owned by this contract.
    created: u64,
    /// Stores the owned by value owned by this contract.
    owned_by: &'static str,
}

/// Represents `ModelListResponse` state within this module.
#[derive(Debug, Serialize)]
struct ModelListResponse {
    /// Stores the object value owned by this contract.
    object: &'static str,
    /// Stores the data value owned by this contract.
    data: Vec<ModelDescriptor>,
}

// -----------------------------------------------------------------------------
// Chat: Models successful Chat Completions output.
// -----------------------------------------------------------------------------

/// Represents `ChatMessage` state within this module.
#[derive(Debug, Serialize)]
struct ChatMessage {
    /// Stores the role value owned by this contract.
    role: &'static str,
    /// Stores the content value owned by this contract.
    content: String,
}

/// Represents `ChatChoice` state within this module.
#[derive(Debug, Serialize)]
struct ChatChoice {
    /// Stores the index value owned by this contract.
    index: usize,
    /// Stores the message value owned by this contract.
    message: ChatMessage,
    /// Stores the finish reason value owned by this contract.
    finish_reason: &'static str,
}

/// Represents `ChatCompletionResponse` state within this module.
#[derive(Debug, Serialize)]
struct ChatCompletionResponse {
    /// Stores the id value owned by this contract.
    id: String,
    /// Stores the object value owned by this contract.
    object: &'static str,
    /// Stores the created value owned by this contract.
    created: u64,
    /// Stores the model value owned by this contract.
    model: ModelId,
    /// Stores the choices value owned by this contract.
    choices: Vec<ChatChoice>,
    /// Stores the usage value owned by this contract.
    usage: TokenUsage,
}

// -----------------------------------------------------------------------------
// Response: Models successful Responses output.
// -----------------------------------------------------------------------------

/// Represents `ResponseContent` state within this module.
#[derive(Debug, Serialize)]
struct ResponseContent {
    /// Stores the content type value owned by this contract.
    #[serde(rename = "type")]
    content_type: &'static str,
    /// Stores the text value owned by this contract.
    text: String,
    /// Stores the annotations value owned by this contract.
    annotations: Vec<serde_json::Value>,
}

/// Represents `ResponseOutput` state within this module.
#[derive(Debug, Serialize)]
struct ResponseOutput {
    /// Stores the id value owned by this contract.
    id: String,
    /// Stores the output type value owned by this contract.
    #[serde(rename = "type")]
    output_type: &'static str,
    /// Stores the status value owned by this contract.
    status: &'static str,
    /// Stores the role value owned by this contract.
    role: &'static str,
    /// Stores the content value owned by this contract.
    content: Vec<ResponseContent>,
}

/// Represents `ResponsesUsage` state within this module.
#[derive(Debug, Serialize)]
struct ResponseUsage {
    /// Stores the input value owned by this contract.
    #[serde(rename = "input_tokens")]
    input: usize,
    /// Stores the output value owned by this contract.
    #[serde(rename = "output_tokens")]
    output: usize,
    /// Stores the total value owned by this contract.
    #[serde(rename = "total_tokens")]
    total: usize,
}

impl From<TokenUsage> for ResponseUsage {
    fn from(usage: TokenUsage) -> Self {
        Self {
            input: usage.prompt,
            output: usage.completion,
            total: usage.total,
        }
    }
}

/// Represents `ResponsesResponse` state within this module.
#[derive(Debug, Serialize)]
struct ResponseEnvelope {
    /// Stores the id value owned by this contract.
    id: String,
    /// Stores the object value owned by this contract.
    object: &'static str,
    /// Stores the created at value owned by this contract.
    created_at: u64,
    /// Stores the status value owned by this contract.
    status: &'static str,
    /// Stores the model value owned by this contract.
    model: ModelId,
    /// Stores the output value owned by this contract.
    output: Vec<ResponseOutput>,
    /// Stores the output text value owned by this contract.
    output_text: String,
    /// Stores the usage value owned by this contract.
    usage: ResponseUsage,
}

impl From<CompatTurnResponse> for ResponseEnvelope {
    fn from(response: CompatTurnResponse) -> Self {
        let output_id = format!("msg_{}", Uuid::now_v7().simple());
        let output_text = response.output;
        Self {
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
            usage: ResponseUsage::from(response.usage),
        }
    }
}

// -----------------------------------------------------------------------------
// OpenAiFailure: Models OpenAI error envelopes.
// -----------------------------------------------------------------------------

/// Represents `OpenAiFailureBody` state within this module.
#[derive(Debug, Serialize)]
struct OpenAiFailureBody {
    /// Stores the message value owned by this contract.
    message: String,
    /// Stores the error type value owned by this contract.
    #[serde(rename = "type")]
    error_type: &'static str,
    /// Stores the param value owned by this contract.
    param: Option<&'static str>,
    /// Stores the code value owned by this contract.
    code: Option<&'static str>,
}

/// Represents `OpenAiFailureResponse` state within this module.
#[derive(Debug, Serialize)]
struct OpenAiFailureResponse {
    /// Stores the error value owned by this contract.
    error: OpenAiFailureBody,
}

// -----------------------------------------------------------------------------
// OpenAiChatTurn: Lowers chat roles into bounded replay turns.
// -----------------------------------------------------------------------------

/// Lower an `OpenAI` Chat Completions request into the replay shape.
///
/// # Errors
///
/// Returns [`ProviderRejection`] when message roles are unsupported or message
/// content is not textual.
struct OpenAiChatTurn(
    /// Provider-neutral request produced from Chat Completions input.
    CompatTurnRequest,
);

impl TryFrom<ChatCompletionRequest> for OpenAiChatTurn {
    type Error = ProviderRejection;

    fn try_from(payload: ChatCompletionRequest) -> Result<Self, Self::Error> {
        let mut system_text = Vec::new();
        let mut user_turns = Vec::new();

        for message in payload.messages {
            match message.role.as_str() {
                // System/developer text is counted but not replayed; ELIZA has
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
                // User turns are the only inputs that become historical ELIZA
                // conversation state.
                "user" => {
                    // User turns must contain text that ELIZA can replay.
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
                // Assistant/tool messages are already transcript history from
                // the client side; replaying them would make ELIZA answer itself.
                "assistant" | "tool" => {}
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
// ResponseItemText: Extracts user-authored Responses history.
// -----------------------------------------------------------------------------

/// Extract one user-authored text item from Responses history.
/// Extract text from one Responses API input item.
///
/// # Errors
///
/// Returns [`ProviderRejection`] when user content is missing or non-textual.
fn response_item_text(item: &Value) -> Result<Option<String>, ProviderRejection> {
    let role = item.get("role").and_then(Value::as_str).unwrap_or("user");

    // Prior assistant output is context, not a new ELIZA turn.
    if role != "user" {
        return Ok(None);
    }
    let content = item.get("content").ok_or_else(|| {
        ProviderRejection::invalid("input.content", "input item is missing content")
    })?;
    optional_text_content(content, "input.content", TextArrayKind::Parts)
}

// -----------------------------------------------------------------------------
// OpenAiResponsesTurn: Lowers Responses input into replay turns.
// -----------------------------------------------------------------------------

/// Lower an `OpenAI` Responses request into the replay shape.
///
/// # Errors
///
/// Returns [`ProviderRejection`] when the input shape is unsupported or message
/// content is not textual.
struct OpenAiResponsesTurn(
    /// Provider-neutral request produced from Responses input.
    CompatTurnRequest,
);

impl TryFrom<ResponsesRequest> for OpenAiResponsesTurn {
    type Error = ProviderRejection;

    fn try_from(payload: ResponsesRequest) -> Result<Self, Self::Error> {
        let mut user_turns = Vec::new();

        match payload.input {
            // The compact Responses form is one direct user turn.
            Value::String(text) => user_turns.push(text),
            // The array form is treated like message history and only user
            // entries are replayed.
            Value::Array(items) => {
                for item in items {
                    let Some(text) = response_item_text(&item)? else {
                        continue;
                    };
                    user_turns.push(text);
                }
            }
            // Other JSON shapes cannot represent a Responses transcript.
            _ => {
                return Err(ProviderRejection::unsupported(
                    "input",
                    "Responses input must be a string or an array of message-like objects",
                ));
            }
        }

        Ok(Self(CompatTurnRequest::new(
            payload.model,
            Vec::new(),
            user_turns,
        )))
    }
}

// -----------------------------------------------------------------------------
// OpenAi: Chunks, errors, and routes stay OpenAI-shaped here.
// -----------------------------------------------------------------------------

/// Render `OpenAI` Chat Completions streaming chunks.
///
/// # Panics
///
/// Panics only if the locally constructed JSON chunk payload cannot serialize.
fn open_ai_chat_stream(output: &str, model: &ModelId, delay_ms: u64) -> Response {
    let id = format!("chatcmpl-{}", Uuid::now_v7().simple());
    let created = unix_timestamp();
    let mut events = Vec::new();

    // OpenAI streams begin by announcing the assistant role before any
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

    // ELIZA emits one complete sentence; split it into deterministic text
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

    // Finish with an empty delta and the `[DONE]` sentinel used by OpenAI
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

    SseEvents::from(events).into_response(delay_ms)
}

/// Performs the error response operation for this abstraction.
fn open_ai_error_response(error: ProviderRejection) -> Response {
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
// OpenAiRoute: Authenticates and renders compatible responses.
// -----------------------------------------------------------------------------

/// Performs the models operation for this abstraction.
pub(crate) async fn open_ai_route_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    // Render authentication failures in OpenAI's envelope immediately.
    if let Err(error) = provider_authenticate(&headers, &state.config) {
        return open_ai_error_response(error);
    }

    // The server exposes exactly one configured model id.
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

/// Performs the chat completions operation for this abstraction.
pub(crate) async fn open_ai_route_chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ChatCompletionRequest>,
) -> Response {
    // Render authentication failures in OpenAI's envelope immediately.
    if let Err(error) = provider_authenticate(&headers, &state.config) {
        return open_ai_error_response(error);
    }

    // Capture the stream flag before lowering consumes the request body.
    let stream = payload.should_stream;
    let request = match OpenAiChatTurn::try_from(payload) {
        Ok(request) => request.0,
        // Invalid chat payloads stop before the ELIZA engine is invoked.
        Err(error) => return open_ai_error_response(error),
    };
    let limits = RequestLimits::new(
        state.config.max_input_chars,
        state.config.max_history_messages,
    );
    let response = match request.complete(limits) {
        Ok(response) => response,
        // Engine rejections retain OpenAI's error shape.
        Err(error) => return open_ai_error_response(error),
    };

    if stream {
        open_ai_chat_stream(
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

/// Performs the responses operation for this abstraction.
pub(crate) async fn open_ai_route_responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ResponsesRequest>,
) -> Response {
    // Render authentication failures in OpenAI's envelope immediately.
    if let Err(error) = provider_authenticate(&headers, &state.config) {
        return open_ai_error_response(error);
    }

    // Responses streaming has a different event contract; keep the supported
    // streaming surface on Chat Completions until that shape earns its place.
    if payload.should_stream {
        return open_ai_error_response(ProviderRejection::unsupported(
            "stream",
            "OpenAI Responses streaming is not implemented; use /v1/chat/completions streaming",
        ));
    }

    let request = match OpenAiResponsesTurn::try_from(payload) {
        Ok(request) => request.0,
        // Invalid Responses payloads stop before the ELIZA engine is invoked.
        Err(error) => return open_ai_error_response(error),
    };
    let limits = RequestLimits::new(
        state.config.max_input_chars,
        state.config.max_history_messages,
    );
    let response = match request.complete(limits) {
        Ok(response) => response,
        // Engine rejections retain OpenAI's error shape.
        Err(error) => return open_ai_error_response(error),
    };
    Json(ResponseEnvelope::from(response)).into_response()
}
