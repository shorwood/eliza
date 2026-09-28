//! Ollama chat and model catalog adapter.
#![allow(
    rlib::missing_section_dividers,
    rlib::undocumented_early_returns,
    rlib::undocumented_items,
    reason = "private wire plumbing stays clearer without per-field docs, per-concept dividers, or comments restating errors"
)]

use aide::axum::ApiRouter;
use aide::axum::routing::{get_with, post_with};
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::super::context::{AppState, ProviderAuth, provider_authenticate};
use crate::types::http::{NdjsonResponse, ProviderRejection, stream_chunks};
use crate::types::json::JsonObject;
use crate::types::model::ModelId;
use crate::types::turn::{
    CompatOutput, CompatTurn, CompatTurnRequest, CompatTurnResponse, FunctionCall, FunctionTool,
    RequestLimits, ToolChoice,
};

/// Stable timestamp used by this deterministic historical fixture.
const CREATED_AT: &str = "1966-01-01T00:00:00Z";

#[derive(Debug, Deserialize, JsonSchema)]
struct OllamaMessage {
    role: String,
    #[serde(default = "missing_content_is_null")]
    content: Value,
    #[serde(default = "missing_values_are_empty")]
    tool_calls: Vec<Value>,
}

/// Accepted Ollama chat input.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct OllamaChatRequest {
    model: ModelId,
    messages: Vec<OllamaMessage>,
    #[serde(default = "should_stream_by_default", rename = "stream")]
    should_stream: bool,
    #[serde(default = "missing_values_are_empty")]
    tools: Vec<Value>,
    #[serde(default)]
    tool_choice: Option<Value>,
    #[serde(default)]
    format: Option<Value>,
    #[serde(default)]
    think: Option<Value>,
}

fn missing_content_is_null() -> Value {
    Value::Null
}

fn missing_values_are_empty() -> Vec<Value> {
    Vec::new()
}

const fn should_stream_by_default() -> bool {
    true
}

struct OllamaTurn(CompatTurnRequest);

impl TryFrom<OllamaChatRequest> for OllamaTurn {
    type Error = ProviderRejection;

    fn try_from(payload: OllamaChatRequest) -> Result<Self, Self::Error> {
        if payload.format.is_some() {
            return Err(ProviderRejection::unsupported(
                "format",
                "structured output is not supported",
            ));
        }
        if payload.think.is_some() {
            return Err(ProviderRejection::unsupported(
                "think",
                "reasoning output is not supported",
            ));
        }

        let mut system = Vec::new();
        let mut turns = Vec::new();
        for message in payload.messages {
            let content = value_text(&message.content);
            match message.role.as_str() {
                "system" => system.push(content),
                "user" => turns.push(CompatTurn::User(content)),
                "assistant" => {
                    if !content.is_empty() {
                        turns.push(CompatTurn::Assistant(content));
                    }
                    for call in message.tool_calls {
                        turns.push(CompatTurn::ToolCall(lower_ollama_call(
                            &call,
                            "messages.tool_calls",
                        )?));
                    }
                }
                "tool" => turns.push(CompatTurn::ToolResult(content)),
                role => {
                    return Err(ProviderRejection::unsupported(
                        "messages.role",
                        format!("unsupported Ollama role `{role}`"),
                    ));
                }
            }
        }

        Ok(Self(CompatTurnRequest::new(
            payload.model,
            system,
            turns,
            lower_tools(payload.tools)?,
            parse_ollama_tool_choice_policy(payload.tool_choice, "tool_choice")?,
        )))
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Decode one Ollama function call.
///
/// # Errors
///
/// Returns a rejection for missing fields or non-object arguments.
fn lower_ollama_call(
    value: &Value,
    param: &'static str,
) -> Result<FunctionCall, ProviderRejection> {
    let function = value
        .get("function")
        .ok_or_else(|| ProviderRejection::invalid(param, "tool call is missing function"))?;
    let name = function
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| ProviderRejection::invalid(param, "tool call is missing name"))?;
    let arguments = function
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let arguments = JsonObject::from_value(arguments).ok_or_else(|| {
        ProviderRejection::invalid(param, "tool call arguments must be a JSON object")
    })?;
    Ok(FunctionCall {
        name: name.to_owned(),
        arguments,
    })
}

/// Validate and retain Ollama function definitions.
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
                    ProviderRejection::invalid("tools", "function tool is missing name")
                })?;
            Ok(FunctionTool::new(
                name.to_owned(),
                tool.to_string().chars().count(),
            ))
        })
        .collect()
}

/// Normalize Ollama tool-selection policy.
///
/// # Errors
///
/// Returns a rejection for malformed or unsupported policy values.
fn parse_ollama_tool_choice_policy(
    choice: Option<Value>,
    param: &'static str,
) -> Result<ToolChoice, ProviderRejection> {
    let Some(choice) = choice else {
        return Ok(ToolChoice::Auto);
    };
    if let Some(choice) = choice.as_str() {
        return match choice {
            "auto" => Ok(ToolChoice::Auto),
            "none" => Ok(ToolChoice::None),
            "required" => Ok(ToolChoice::Required),
            _ => Err(ProviderRejection::invalid(
                param,
                format!("unsupported tool_choice `{choice}`"),
            )),
        };
    }
    let name = choice
        .pointer("/function/name")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderRejection::invalid(param, "named tool_choice is missing name"))?;
    Ok(ToolChoice::Named(name.to_owned()))
}

fn message_json(output: &CompatOutput) -> Value {
    match output {
        CompatOutput::Text(text) => json!({"role":"assistant","content":text}),
        CompatOutput::ToolCall(call) => json!({
            "role":"assistant",
            "content":"",
            "tool_calls":[{
                "type":"function",
                "function":{"name":call.name,"arguments":call.arguments}
            }]
        }),
    }
}

fn response_json(response: &CompatTurnResponse) -> Value {
    json!({
        "model":response.model,
        "created_at":CREATED_AT,
        "message":message_json(&response.output),
        "done":true,
        "done_reason":"stop",
        "total_duration":0,
        "load_duration":0,
        "prompt_eval_count":response.usage.prompt,
        "prompt_eval_duration":0,
        "eval_count":response.usage.completion,
        "eval_duration":0
    })
}

fn stream_records(response: &CompatTurnResponse) -> Vec<Value> {
    let mut records = match &response.output {
        CompatOutput::Text(text) => stream_chunks(text)
            .into_iter()
            .map(|chunk| {
                json!({
                    "model":response.model,
                    "created_at":CREATED_AT,
                    "message":{"role":"assistant","content":chunk},
                    "done":false
                })
            })
            .collect(),
        CompatOutput::ToolCall(_) => vec![json!({
            "model":response.model,
            "created_at":CREATED_AT,
            "message":message_json(&response.output),
            "done":false
        })],
    };
    records.push(json!({
        "model":response.model,
        "created_at":CREATED_AT,
        "message":{"role":"assistant","content":""},
        "done":true,
        "done_reason":"stop",
        "total_duration":0,
        "load_duration":0,
        "prompt_eval_count":response.usage.prompt,
        "prompt_eval_duration":0,
        "eval_count":response.usage.completion,
        "eval_duration":0
    }));
    records
}

async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<OllamaChatRequest>,
) -> Response {
    if let Err(error) = provider_authenticate(&headers, &state.config, ProviderAuth::Bearer) {
        return error.ollama_response();
    }
    let should_stream = payload.should_stream;
    let request = match OllamaTurn::try_from(payload) {
        Ok(request) => request.0,
        Err(error) => return error.ollama_response(),
    };
    let limits = RequestLimits::new(
        state.config.max_input_chars,
        state.config.max_history_messages,
    );
    let response = match request.complete(limits) {
        Ok(response) => response,
        Err(error) => return error.ollama_response(),
    };
    if should_stream {
        NdjsonResponse::new(stream_records(&response), state.config.stream_delay_ms).into_response()
    } else {
        Json(response_json(&response)).into_response()
    }
}

async fn tags(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(error) = provider_authenticate(&headers, &state.config, ProviderAuth::Bearer) {
        return error.ollama_response();
    }
    let model = state.config.model.as_str();
    Json(json!({
        "models":[{
            "name":model,
            "model":model,
            "modified_at":CREATED_AT,
            "size":0,
            "digest":"eliza-doctor",
            "details":{
                "parent_model":"",
                "format":"eliza",
                "family":"eliza",
                "families":["eliza"],
                "parameter_size":"DOCTOR",
                "quantization_level":"none"
            }
        }]
    }))
    .into_response()
}

/// Ollama chat and model-list endpoints.
pub(super) struct Ollama;

impl Ollama {
    /// Mount the Ollama API surface.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        let router = router.api_route(
            "/api/tags",
            get_with(tags, |operation| {
                operation
                    .summary("Ollama models")
                    .tag("ollama")
                    .response::<200, Json<Value>>()
                    .default_response::<Json<Value>>()
            }),
        );
        router.api_route(
            "/api/chat",
            post_with(chat, |operation| {
                operation
                    .summary("Ollama chat")
                    .tag("ollama")
                    .response::<200, Json<Value>>()
                    .default_response::<Json<Value>>()
            }),
        )
    }
}
