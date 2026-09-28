//! `OpenAI` Chat Completions route and wire contracts.
//!
//! The Gemini `OpenAI` alias reuses the same handler so clients pointed at
//! `/v1beta/openai` see the `OpenAI` response contract.
//! The adapter accepts only textual message content. `OpenAI` features that need
//! tool calls, multimodal parts, structured output, or assistant-state replay do
//! not have an ELIZA equivalent and are rejected or ignored at lowering time.

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::context::{AppState, provider_authenticate};
use crate::types::http::{
    ProviderRejection, SseEvents, TextArrayKind, optional_text_content, stream_chunks,
    unix_timestamp,
};
use crate::types::model::ModelId;
use crate::types::turn::{CompatTurnRequest, RequestLimits, TokenUsage};

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

/// Supply JSON null when message content is omitted.
fn missing_content_is_null() -> Value {
    Value::Null
}

/// Preserve non-streaming behavior when clients omit the stream flag.
const fn should_stream_by_default() -> bool {
    false
}

// -----------------------------------------------------------------------------
// Chat: Models successful Chat Completions output.
// -----------------------------------------------------------------------------

/// Represents `ChatMessage` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
struct ChatMessage {
    /// Stores the role value owned by this contract.
    role: &'static str,
    /// Stores the content value owned by this contract.
    content: String,
}

/// Represents `ChatChoice` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
struct ChatChoice {
    /// Stores the index value owned by this contract.
    index: usize,
    /// Stores the message value owned by this contract.
    message: ChatMessage,
    /// Stores the finish reason value owned by this contract.
    finish_reason: &'static str,
}

/// Represents `ChatCompletionResponse` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
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
// OpenAiFailure: Models OpenAI error envelopes.
// -----------------------------------------------------------------------------

/// Represents `OpenAiFailureBody` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
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
#[derive(Debug, Serialize, JsonSchema)]
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
// OpenAiRejection: Renders OpenAI failures.
// -----------------------------------------------------------------------------

/// Provider rejection rendered in `OpenAI`'s error envelope.
struct OpenAiRejection(
    /// Rejection facts rendered by this provider.
    ProviderRejection,
);

impl IntoResponse for OpenAiRejection {
    fn into_response(self) -> Response {
        let error = self.0;
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
}

// -----------------------------------------------------------------------------
// OpenAiChatStream: Renders OpenAI server-sent events.
// -----------------------------------------------------------------------------

/// Render `OpenAI` Chat Completions streaming chunks.
///
/// # Panics
///
/// Panics only if the locally constructed JSON chunk payload cannot serialize.
fn open_ai_chat_stream(output: &str, model: &ModelId) -> SseEvents {
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

    SseEvents::from(events)
}

// -----------------------------------------------------------------------------
// OpenAiRouteChatCompletions: Executes chat completion requests.
// -----------------------------------------------------------------------------

/// Performs the chat completions operation for this abstraction.
async fn open_ai_route_chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ChatCompletionRequest>,
) -> Response {
    // Render authentication failures in OpenAI's envelope immediately.
    if let Err(error) = provider_authenticate(&headers, &state.config) {
        return OpenAiRejection(error).into_response();
    }

    // Capture the stream flag before lowering consumes the request body.
    let stream = payload.should_stream;
    let request = match OpenAiChatTurn::try_from(payload) {
        Ok(request) => request.0,
        // Invalid chat payloads stop before the ELIZA engine is invoked.
        Err(error) => return OpenAiRejection(error).into_response(),
    };
    let limits = RequestLimits::new(
        state.config.max_input_chars,
        state.config.max_history_messages,
    );
    let response = match request.complete(limits) {
        Ok(response) => response,
        // Engine rejections retain OpenAI's error shape.
        Err(error) => return OpenAiRejection(error).into_response(),
    };

    if stream {
        open_ai_chat_stream(&response.output, &response.model)
            .with_delay(state.config.stream_delay_ms)
            .into_response()
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

// -----------------------------------------------------------------------------
// OpenAiChatCompletions: Mounts Chat Completions and its provider alias.
// -----------------------------------------------------------------------------

/// `OpenAI` Chat Completions endpoints and shared contract.
pub(super) struct OpenAiChatCompletions;

impl OpenAiChatCompletions {
    /// Mount `OpenAI` Chat Completions and Gemini's `OpenAI`-compatible alias.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        let router = router.api_route(
            "/v1/chat/completions",
            post_with(open_ai_route_chat_completions, |operation| {
                operation
                    .summary("OpenAI chat completion")
                    .tag("openai")
                    .response::<200, Json<ChatCompletionResponse>>()
                    .default_response::<Json<OpenAiFailureResponse>>()
            }),
        );
        router.api_route(
            "/v1beta/openai/chat/completions",
            post_with(open_ai_route_chat_completions, |operation| {
                operation
                    .summary("Gemini OpenAI chat")
                    .tag("gemini")
                    .response::<200, Json<ChatCompletionResponse>>()
                    .default_response::<Json<OpenAiFailureResponse>>()
            }),
        )
    }
}
