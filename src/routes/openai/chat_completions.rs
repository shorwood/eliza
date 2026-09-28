//! `OpenAI` Chat Completions and Gemini's OpenAI-compatible alias.
#![allow(
    rlib::missing_section_dividers,
    rlib::undocumented_early_returns,
    rlib::undocumented_items,
    reason = "private wire plumbing stays clearer without per-field docs, per-concept dividers, or comments restating errors"
)]

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::super::context::{AppState, ProviderAuth, provider_authenticate};
use crate::types::http::{
    JsonEventExt, ProviderRejection, SseEvents, TextArrayKind, optional_text_content,
    stream_chunks, unix_timestamp,
};
use crate::types::json::JsonObject;
use crate::types::model::ModelId;
use crate::types::turn::{
    CompatOutput, CompatTurn, CompatTurnRequest, CompatTurnResponse, FunctionCall, FunctionTool,
    RequestLimits, TokenUsage, ToolChoice,
};

#[derive(Debug, Deserialize, JsonSchema)]
struct OpenAiMessage {
    role: String,
    #[serde(default = "missing_content_is_null")]
    content: Value,
    #[serde(default = "missing_values_are_empty")]
    tool_calls: Vec<Value>,
}

impl OpenAiMessage {
    /// Append this wire message to normalized system or conversation state.
    ///
    /// # Errors
    ///
    /// Returns a rejection for malformed content, calls, or unsupported roles.
    fn lower_into(
        self,
        system: &mut Vec<String>,
        turns: &mut Vec<CompatTurn>,
    ) -> Result<(), ProviderRejection> {
        let Self {
            role,
            content,
            tool_calls,
        } = self;
        match role.as_str() {
            "system" | "developer" => {
                if let Some(text) =
                    optional_text_content(&content, "messages.content", TextArrayKind::Parts)?
                {
                    system.push(text);
                }
            }
            "user" => {
                let Some(text) =
                    optional_text_content(&content, "messages.content", TextArrayKind::Parts)?
                else {
                    return Err(ProviderRejection::invalid(
                        "messages",
                        "user message content must contain text",
                    ));
                };
                turns.push(CompatTurn::User(text));
            }
            "assistant" => {
                if let Some(text) =
                    optional_text_content(&content, "messages.content", TextArrayKind::Parts)?
                {
                    turns.push(CompatTurn::Assistant(text));
                }
                for call in tool_calls {
                    turns.push(CompatTurn::ToolCall(lower_openai_call(
                        &call,
                        "messages.tool_calls",
                    )?));
                }
            }
            "tool" => turns.push(CompatTurn::ToolResult(value_text(
                &content,
                "messages.content",
            )?)),
            role => {
                return Err(ProviderRejection::unsupported(
                    "messages.role",
                    format!("unsupported message role `{role}`"),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
struct StreamOptions {
    #[serde(default = "should_not_include_usage", rename = "include_usage")]
    should_include_usage: bool,
}

/// Accepted `OpenAI` Chat Completions input.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ChatCompletionRequest {
    model: ModelId,
    messages: Vec<OpenAiMessage>,
    #[serde(default = "should_not_stream", rename = "stream")]
    should_stream: bool,
    #[serde(default = "StreamOptions::default")]
    stream_options: StreamOptions,
    #[serde(default = "missing_values_are_empty")]
    tools: Vec<Value>,
    #[serde(default)]
    tool_choice: Option<Value>,
    #[serde(default)]
    response_format: Option<Value>,
}

fn missing_content_is_null() -> Value {
    Value::Null
}

fn missing_values_are_empty() -> Vec<Value> {
    Vec::new()
}

const fn should_not_include_usage() -> bool {
    false
}

const fn should_not_stream() -> bool {
    false
}

struct OpenAiChatTurn(CompatTurnRequest);

impl TryFrom<ChatCompletionRequest> for OpenAiChatTurn {
    type Error = ProviderRejection;

    fn try_from(payload: ChatCompletionRequest) -> Result<Self, Self::Error> {
        if payload.response_format.is_some() {
            return Err(ProviderRejection::unsupported(
                "response_format",
                "structured output is not supported",
            ));
        }

        let mut system = Vec::new();
        let mut turns = Vec::new();
        for message in payload.messages {
            message.lower_into(&mut system, &mut turns)?;
        }
        let tools = lower_tools(payload.tools)?;
        let choice = lower_tool_choice(payload.tool_choice)?;
        Ok(Self(CompatTurnRequest::new(
            payload.model,
            system,
            turns,
            tools,
            choice,
        )))
    }
}

/// Validate and retain Chat Completions function definitions.
///
/// # Errors
///
/// Returns a rejection for unsupported or unnamed tool definitions.
fn lower_tools(tools: Vec<Value>) -> Result<Vec<FunctionTool>, ProviderRejection> {
    tools
        .into_iter()
        .map(|tool| {
            if tool.get("type").and_then(Value::as_str) != Some("function") {
                return Err(ProviderRejection::unsupported(
                    "tools",
                    "only client function tools are supported",
                ));
            }
            let name = tool
                .pointer("/function/name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    ProviderRejection::invalid("tools", "function tool is missing its name")
                })?;
            Ok(FunctionTool::new(
                name.to_owned(),
                tool.to_string().chars().count(),
            ))
        })
        .collect()
}

/// Normalize Chat Completions tool-selection policy.
///
/// # Errors
///
/// Returns a rejection for malformed or unsupported policy values.
fn lower_tool_choice(choice: Option<Value>) -> Result<ToolChoice, ProviderRejection> {
    let Some(choice) = choice else {
        return Ok(ToolChoice::Auto);
    };
    if let Some(choice) = choice.as_str() {
        return match choice {
            "auto" => Ok(ToolChoice::Auto),
            "none" => Ok(ToolChoice::None),
            "required" => Ok(ToolChoice::Required),
            _ => Err(ProviderRejection::invalid(
                "tool_choice",
                format!("unsupported tool_choice `{choice}`"),
            )),
        };
    }
    if choice.get("type").and_then(Value::as_str) == Some("function") {
        let name = choice
            .pointer("/function/name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderRejection::invalid("tool_choice", "named tool_choice is missing its name")
            })?;
        return Ok(ToolChoice::Named(name.to_owned()));
    }
    Err(ProviderRejection::invalid(
        "tool_choice",
        "tool_choice must be auto, none, required, or a named function",
    ))
}

/// Decode one OpenAI-compatible function call.
///
/// # Errors
///
/// Returns a rejection for unsupported call types, missing fields, or invalid arguments.
fn lower_openai_call(
    value: &Value,
    param: &'static str,
) -> Result<FunctionCall, ProviderRejection> {
    if value.get("type").and_then(Value::as_str) != Some("function") {
        return Err(ProviderRejection::unsupported(
            param,
            "only function tool calls are supported",
        ));
    }
    let function = value.get("function").ok_or_else(|| {
        ProviderRejection::invalid(param, "function tool call is missing function")
    })?;
    let name = function
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| ProviderRejection::invalid(param, "function tool call is missing name"))?;
    let arguments = function.get("arguments").ok_or_else(|| {
        ProviderRejection::invalid(param, "function tool call is missing arguments")
    })?;
    let arguments = match arguments {
        Value::String(arguments) => serde_json::from_str(arguments).map_err(|error| {
            ProviderRejection::invalid(param, format!("invalid function arguments: {error}"))
        })?,
        arguments => arguments.clone(),
    };
    let arguments = JsonObject::from_value(arguments).ok_or_else(|| {
        ProviderRejection::invalid(param, "function arguments must be a JSON object")
    })?;
    Ok(FunctionCall {
        name: name.to_owned(),
        arguments,
    })
}

/// Convert provider tool-result content into replayable text.
///
/// # Errors
///
/// Returns a rejection when the content is null.
fn value_text(value: &Value, param: &'static str) -> Result<String, ProviderRejection> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Null => Err(ProviderRejection::invalid(
            param,
            "tool result content is required",
        )),
        other => Ok(other.to_string()),
    }
}

fn usage_json(usage: TokenUsage) -> Value {
    json!({
        "prompt_tokens": usage.prompt,
        "completion_tokens": usage.completion,
        "total_tokens": usage.total
    })
}

fn completion_json(response: CompatTurnResponse) -> Value {
    let id = format!("chatcmpl-{}", Uuid::now_v7().simple());
    let (message, finish_reason) = match response.output {
        CompatOutput::Text(text) => (json!({"role":"assistant","content":text}), "stop"),
        CompatOutput::ToolCall(call) => {
            let arguments = call.arguments.serialized();
            (
                json!({
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": format!("call_{}", Uuid::now_v7().simple()),
                        "type": "function",
                        "function": {"name": call.name, "arguments": arguments}
                    }]
                }),
                "tool_calls",
            )
        }
    };
    json!({
        "id": id,
        "object": "chat.completion",
        "created": unix_timestamp(),
        "model": response.model,
        "choices": [{"index":0,"message":message,"finish_reason":finish_reason}],
        "usage": usage_json(response.usage)
    })
}

#[derive(Clone, Copy)]
enum UsageStream {
    Include,
    Omit,
}

impl UsageStream {
    fn for_options(options: StreamOptions) -> Self {
        if options.should_include_usage {
            Self::Include
        } else {
            Self::Omit
        }
    }

    fn should_include(self) -> bool {
        matches!(self, Self::Include)
    }
}

struct ChatStreamContext<'a> {
    id: String,
    created: u64,
    model: &'a str,
}

fn stream_json(response: &CompatTurnResponse, usage_stream: UsageStream) -> SseEvents {
    let context = ChatStreamContext {
        id: format!("chatcmpl-{}", Uuid::now_v7().simple()),
        created: unix_timestamp(),
        model: response.model.as_str(),
    };
    let mut events = vec![
        json!({
            "id": context.id,
            "object": "chat.completion.chunk",
            "created": context.created,
            "model": context.model,
            "choices": [{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]
        })
        .to_sse_event(),
    ];

    match &response.output {
        CompatOutput::Text(text) => push_text_events(&mut events, &context, text),
        CompatOutput::ToolCall(call) => push_tool_events(&mut events, &context, call),
    }
    if usage_stream.should_include() {
        events.push(
            json!({
                "id": context.id,
                "object": "chat.completion.chunk",
                "created": context.created,
                "model": context.model,
                "choices": [],
                "usage": usage_json(response.usage)
            })
            .to_sse_event(),
        );
    }
    events.push(Event::default().data("[DONE]"));
    SseEvents::from(events)
}

fn push_text_events(events: &mut Vec<Event>, context: &ChatStreamContext<'_>, text: &str) {
    events.extend(stream_chunks(text).into_iter().map(|chunk| {
        json!({
            "id": context.id,
            "object": "chat.completion.chunk",
            "created": context.created,
            "model": context.model,
            "choices": [{"index":0,"delta":{"content":chunk},"finish_reason":null}]
        })
        .to_sse_event()
    }));
    events.push(
        json!({
            "id": context.id,
            "object": "chat.completion.chunk",
            "created": context.created,
            "model": context.model,
            "choices": [{"index":0,"delta":{},"finish_reason":"stop"}]
        })
        .to_sse_event(),
    );
}

fn push_tool_events(events: &mut Vec<Event>, context: &ChatStreamContext<'_>, call: &FunctionCall) {
    let call_id = format!("call_{}", Uuid::now_v7().simple());
    events.push(
        json!({
            "id": context.id,
            "object": "chat.completion.chunk",
            "created": context.created,
            "model": context.model,
            "choices": [{"index":0,"delta":{"tool_calls":[{
                "index":0,
                "id":call_id,
                "type":"function",
                "function":{"name":call.name,"arguments":""}
            }]},"finish_reason":null}]
        })
        .to_sse_event(),
    );
    events.push(
        json!({
            "id": context.id,
            "object": "chat.completion.chunk",
            "created": context.created,
            "model": context.model,
            "choices": [{"index":0,"delta":{"tool_calls":[{
                "index":0,
                "function":{"arguments":call.arguments.serialized()}
            }]},"finish_reason":null}]
        })
        .to_sse_event(),
    );
    events.push(
        json!({
            "id": context.id,
            "object": "chat.completion.chunk",
            "created": context.created,
            "model": context.model,
            "choices": [{"index":0,"delta":{},"finish_reason":"tool_calls"}]
        })
        .to_sse_event(),
    );
}

/// `OpenAI` Chat Completions endpoints.
pub(crate) struct OpenAiChatCompletions;

impl OpenAiChatCompletions {
    /// Handle one OpenAI-compatible Chat Completions request.
    #[expect(
        clippy::unused_async,
        reason = "Axum handlers must return a future even when their work is synchronous"
    )]
    pub(crate) async fn handle(
        State(state): State<AppState>,
        headers: HeaderMap,
        Json(payload): Json<ChatCompletionRequest>,
    ) -> Response {
        if let Err(error) = provider_authenticate(&headers, &state.config, ProviderAuth::Bearer) {
            return error.openai_response();
        }

        // Retain rendering preferences before lowering consumes the request.
        let should_stream = payload.should_stream;
        let usage_stream = UsageStream::for_options(payload.stream_options);
        let request = match OpenAiChatTurn::try_from(payload) {
            Ok(request) => request.0,
            Err(error) => return error.openai_response(),
        };
        let limits = RequestLimits::new(
            state.config.max_input_chars,
            state.config.max_history_messages,
        );
        let response = match request.complete(limits) {
            Ok(response) => response,
            Err(error) => return error.openai_response(),
        };

        if should_stream {
            stream_json(&response, usage_stream)
                .with_delay(state.config.stream_delay_ms)
                .into_response()
        } else {
            Json(completion_json(response)).into_response()
        }
    }

    /// Mount the `OpenAI` Chat Completions route.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/v1/chat/completions",
            post_with(Self::handle, |operation| {
                operation
                    .summary("OpenAI chat completion")
                    .tag("openai")
                    .response::<200, Json<Value>>()
                    .default_response::<Json<Value>>()
            }),
        )
    }
}
