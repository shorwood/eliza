//! Anthropic Messages adapter.
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

use super::context::{AppState, ProviderAuth, provider_authenticate};
use crate::types::http::{
    JsonEventExt, ProviderRejection, SseEvents, TextArrayKind, optional_text_content, stream_chunks,
};
use crate::types::model::ModelId;
use crate::types::turn::{
    CompatOutput, CompatTurn, CompatTurnRequest, CompatTurnResponse, FunctionCall, FunctionTool,
    RequestLimits, TokenUsage, ToolChoice,
};

#[derive(Debug, Deserialize, JsonSchema)]
struct AnthropicMessage {
    role: String,
    #[serde(default = "missing_content_is_null")]
    content: Value,
}

/// Accepted Anthropic Messages input.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MessagesRequest {
    model: ModelId,
    #[serde(default = "missing_content_is_null")]
    system: Value,
    messages: Vec<AnthropicMessage>,
    #[serde(default = "should_not_stream", rename = "stream")]
    should_stream: bool,
    #[serde(default = "missing_values_are_empty")]
    tools: Vec<Value>,
    #[serde(default)]
    tool_choice: Option<Value>,
}

fn missing_content_is_null() -> Value {
    Value::Null
}

fn missing_values_are_empty() -> Vec<Value> {
    Vec::new()
}

const fn should_not_stream() -> bool {
    false
}

struct AnthropicTurn(CompatTurnRequest);

impl TryFrom<MessagesRequest> for AnthropicTurn {
    type Error = ProviderRejection;

    fn try_from(payload: MessagesRequest) -> Result<Self, Self::Error> {
        let mut system = Vec::new();
        if let Some(text) = optional_text_content(&payload.system, "system", TextArrayKind::Blocks)?
        {
            system.push(text);
        }

        let mut turns = Vec::new();
        for message in payload.messages {
            match message.role.as_str() {
                "user" => lower_user_content(&message.content, &mut turns)?,
                "assistant" => lower_assistant_content(&message.content, &mut turns)?,
                role => {
                    return Err(ProviderRejection::unsupported(
                        "messages.role",
                        format!("unsupported Anthropic role `{role}`"),
                    ));
                }
            }
        }

        Ok(Self(CompatTurnRequest::new(
            payload.model,
            system,
            turns,
            lower_tools(payload.tools)?,
            lower_tool_choice(payload.tool_choice)?,
        )))
    }
}

/// Lower one Anthropic user message without changing content-block order.
///
/// # Errors
///
/// Returns a rejection for empty, malformed, or unsupported content blocks.
fn lower_user_content(
    content: &Value,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), ProviderRejection> {
    if let Value::String(text) = content {
        turns.push(CompatTurn::User(text.clone()));
        return Ok(());
    }
    let Value::Array(blocks) = content else {
        return Err(ProviderRejection::unsupported(
            "messages.content",
            "user content must be text or content blocks",
        ));
    };
    if blocks.is_empty() {
        return Err(ProviderRejection::invalid(
            "messages.content",
            "user content blocks are required",
        ));
    }

    let mut text = Vec::new();
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                let part = block.get("text").and_then(Value::as_str).ok_or_else(|| {
                    ProviderRejection::invalid("messages.content", "text block is missing text")
                })?;
                text.push(part.to_owned());
            }
            Some("tool_result") => {
                flush_user_text(&mut text, turns);
                let result = block.get("content").ok_or_else(|| {
                    ProviderRejection::invalid(
                        "messages.content",
                        "tool_result block is missing content",
                    )
                })?;
                turns.push(CompatTurn::ToolResult(block_text(result)?));
            }
            _ => {
                return Err(ProviderRejection::unsupported(
                    "messages.content",
                    "only text and tool_result blocks are supported",
                ));
            }
        }
    }
    flush_user_text(&mut text, turns);
    Ok(())
}

fn flush_user_text(text: &mut Vec<String>, turns: &mut Vec<CompatTurn>) {
    if text.is_empty() {
        return;
    }
    turns.push(CompatTurn::User(std::mem::take(text).join("\n")));
}

/// Lower one Anthropic assistant message into text and function-call turns.
///
/// # Errors
///
/// Returns a rejection for malformed or unsupported content blocks.
fn lower_assistant_content(
    content: &Value,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), ProviderRejection> {
    if let Value::String(text) = content {
        turns.push(CompatTurn::Assistant(text.clone()));
        return Ok(());
    }
    let Value::Array(blocks) = content else {
        return Err(ProviderRejection::unsupported(
            "messages.content",
            "assistant content must be text or content blocks",
        ));
    };
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = block.get("text").and_then(Value::as_str).ok_or_else(|| {
                    ProviderRejection::invalid("messages.content", "text block is missing text")
                })?;
                turns.push(CompatTurn::Assistant(text.to_owned()));
            }
            Some("tool_use") => turns.push(CompatTurn::ToolCall(lower_tool_use(block)?)),
            _ => {
                return Err(ProviderRejection::unsupported(
                    "messages.content",
                    "only text and tool_use blocks are supported",
                ));
            }
        }
    }
    Ok(())
}

/// Decode one Anthropic `tool_use` content block.
///
/// # Errors
///
/// Returns a rejection when the name or object-valued input is missing.
fn lower_tool_use(block: &Value) -> Result<FunctionCall, ProviderRejection> {
    let name = block
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            ProviderRejection::invalid("messages.content", "tool_use block is missing name")
        })?;
    let input = block.get("input").cloned().ok_or_else(|| {
        ProviderRejection::invalid("messages.content", "tool_use block is missing input")
    })?;
    if !input.is_object() {
        return Err(ProviderRejection::invalid(
            "messages.content",
            "tool_use input must be a JSON object",
        ));
    }
    Ok(FunctionCall {
        name: name.to_owned(),
        arguments: input,
    })
}

/// Decode the textual payload of an Anthropic tool result.
///
/// # Errors
///
/// Returns a rejection for empty or non-text content blocks.
fn block_text(value: &Value) -> Result<String, ProviderRejection> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Array(_) => optional_text_content(value, "messages.content", TextArrayKind::Blocks)?
            .ok_or_else(|| {
                ProviderRejection::invalid("messages.content", "tool result content is required")
            }),
        other => Ok(other.to_string()),
    }
}

/// Validate and retain Anthropic function definitions.
///
/// # Errors
///
/// Returns a rejection when a definition lacks a name or input schema.
fn lower_tools(tools: Vec<Value>) -> Result<Vec<FunctionTool>, ProviderRejection> {
    tools
        .into_iter()
        .map(|tool| {
            let name = tool
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| ProviderRejection::invalid("tools", "tool is missing its name"))?;
            if tool.get("input_schema").is_none() {
                return Err(ProviderRejection::invalid(
                    "tools",
                    "function tool is missing input_schema",
                ));
            }
            Ok(FunctionTool::new(name.to_owned(), tool.clone()))
        })
        .collect()
}

/// Normalize Anthropic tool-selection policy.
///
/// # Errors
///
/// Returns a rejection for malformed or unsupported policy values.
fn lower_tool_choice(choice: Option<Value>) -> Result<ToolChoice, ProviderRejection> {
    let Some(choice) = choice else {
        return Ok(ToolChoice::Auto);
    };
    let choice_type = choice
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderRejection::invalid("tool_choice", "tool_choice is missing type"))?;
    match choice_type {
        "auto" => Ok(ToolChoice::Auto),
        "none" => Ok(ToolChoice::None),
        "any" => Ok(ToolChoice::Required),
        "tool" => choice
            .get("name")
            .and_then(Value::as_str)
            .map(|name| ToolChoice::Named(name.to_owned()))
            .ok_or_else(|| {
                ProviderRejection::invalid("tool_choice", "named tool_choice is missing name")
            }),
        _ => Err(ProviderRejection::invalid(
            "tool_choice",
            format!("unsupported tool_choice `{choice_type}`"),
        )),
    }
}

fn response_json(response: CompatTurnResponse) -> Value {
    let (content, stop_reason) = match response.output {
        CompatOutput::Text(text) => (vec![json!({"type":"text","text":text})], "end_turn"),
        CompatOutput::ToolCall(call) => (
            vec![json!({
                "type":"tool_use",
                "id":format!("toolu_{}", Uuid::now_v7().simple()),
                "name":call.name,
                "input":call.arguments
            })],
            "tool_use",
        ),
    };
    json!({
        "id":format!("msg_{}", Uuid::now_v7().simple()),
        "type":"message",
        "role":"assistant",
        "model":response.model,
        "content":content,
        "stop_reason":stop_reason,
        "stop_sequence":null,
        "usage":usage_json(response.usage)
    })
}

fn usage_json(usage: TokenUsage) -> Value {
    json!({
        "input_tokens":usage.prompt,
        "output_tokens":usage.completion
    })
}

struct AnthropicStream<'a>(&'a CompatTurnResponse);

struct AnthropicEvents(SseEvents);

impl From<AnthropicStream<'_>> for AnthropicEvents {
    fn from(stream: AnthropicStream<'_>) -> Self {
        let response = stream.0;
        let id = format!("msg_{}", Uuid::now_v7().simple());
        let mut events = vec![anthropic_event(
            "message_start",
            &json!({
                "type":"message_start",
                "message":{
                    "id":id,
                    "type":"message",
                    "role":"assistant",
                    "model":response.model,
                    "content":[],
                    "stop_reason":null,
                    "stop_sequence":null,
                    "usage":{"input_tokens":response.usage.prompt,"output_tokens":0}
                }
            }),
        )];

        let stop_reason = push_content_events(&mut events, &response.output);
        events.push(anthropic_event(
            "content_block_stop",
            &json!({"type":"content_block_stop","index":0}),
        ));
        events.push(anthropic_event(
            "message_delta",
            &json!({
                "type":"message_delta",
                "delta":{"stop_reason":stop_reason,"stop_sequence":null},
                "usage":{"output_tokens":response.usage.completion}
            }),
        ));
        events.push(anthropic_event(
            "message_stop",
            &json!({"type":"message_stop"}),
        ));
        Self(SseEvents::from(events))
    }
}

fn push_content_events(events: &mut Vec<Event>, output: &CompatOutput) -> &'static str {
    match output {
        CompatOutput::Text(text) => {
            push_text_events(events, text);
            "end_turn"
        }
        CompatOutput::ToolCall(call) => {
            push_tool_events(events, call);
            "tool_use"
        }
    }
}

fn push_text_events(events: &mut Vec<Event>, text: &str) {
    events.push(anthropic_event(
        "content_block_start",
        &json!({
            "type":"content_block_start",
            "index":0,
            "content_block":{"type":"text","text":""}
        }),
    ));
    events.extend(stream_chunks(text).into_iter().map(|chunk| {
        anthropic_event(
            "content_block_delta",
            &json!({
                "type":"content_block_delta",
                "index":0,
                "delta":{"type":"text_delta","text":chunk}
            }),
        )
    }));
}

fn push_tool_events(events: &mut Vec<Event>, call: &FunctionCall) {
    events.push(anthropic_event(
        "content_block_start",
        &json!({
            "type":"content_block_start",
            "index":0,
            "content_block":{
                "type":"tool_use",
                "id":format!("toolu_{}", Uuid::now_v7().simple()),
                "name":call.name,
                "input":{}
            }
        }),
    ));
    events.push(anthropic_event(
        "content_block_delta",
        &json!({
            "type":"content_block_delta",
            "index":0,
            "delta":{
                "type":"input_json_delta",
                "partial_json":call.arguments.to_string()
            }
        }),
    ));
}

fn anthropic_event(name: &'static str, value: &Value) -> Event {
    value.to_sse_event().event(name)
}

async fn messages(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<MessagesRequest>,
) -> Response {
    if let Err(error) =
        provider_authenticate(&headers, &state.config, ProviderAuth::ApiKey("x-api-key"))
    {
        return error.anthropic_response();
    }

    let should_stream = payload.should_stream;
    let request = match AnthropicTurn::try_from(payload) {
        Ok(request) => request.0,
        Err(error) => return error.anthropic_response(),
    };
    let limits = RequestLimits::new(
        state.config.max_input_chars,
        state.config.max_history_messages,
    );
    let response = match request.complete(limits) {
        Ok(response) => response,
        Err(error) => return error.anthropic_response(),
    };

    if should_stream {
        AnthropicEvents::from(AnthropicStream(&response))
            .0
            .with_delay(state.config.stream_delay_ms)
            .into_response()
    } else {
        Json(response_json(response)).into_response()
    }
}

/// Anthropic Messages endpoint.
pub(super) struct AnthropicMessages;

impl AnthropicMessages {
    /// Mount the Anthropic Messages route.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/anthropic/v1/messages",
            post_with(messages, |operation| {
                operation
                    .summary("Anthropic message")
                    .tag("anthropic")
                    .response::<200, Json<Value>>()
                    .default_response::<Json<Value>>()
            }),
        )
    }
}
