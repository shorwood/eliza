//! `OpenAI` Responses API adapter.
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

/// Accepted Responses API input.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ResponsesRequest {
    model: ModelId,
    input: Value,
    #[serde(default)]
    instructions: Option<Value>,
    #[serde(default = "should_not_stream", rename = "stream")]
    should_stream: bool,
    #[serde(default = "missing_values_are_empty")]
    tools: Vec<Value>,
    #[serde(default)]
    tool_choice: Option<Value>,
    #[serde(default)]
    text: Option<Value>,
}

fn missing_values_are_empty() -> Vec<Value> {
    Vec::new()
}

const fn should_not_stream() -> bool {
    false
}

struct OpenAiResponsesTurn(CompatTurnRequest);

impl TryFrom<ResponsesRequest> for OpenAiResponsesTurn {
    type Error = ProviderRejection;

    fn try_from(payload: ResponsesRequest) -> Result<Self, Self::Error> {
        validate_text_config(payload.text.as_ref())?;
        let mut system = Vec::new();
        if let Some(instructions) = payload.instructions {
            let Some(text) = optional_text_content(
                &instructions,
                "instructions",
                TextArrayKind::ResponsesParts,
            )?
            else {
                return Err(ProviderRejection::invalid(
                    "instructions",
                    "instructions must contain text",
                ));
            };
            system.push(text);
        }

        let turns = lower_input(payload.input, &mut system)?;
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

/// Accept only the Responses API's plain-text output configuration.
///
/// # Errors
///
/// Returns a rejection when structured output is requested.
fn validate_text_config(text: Option<&Value>) -> Result<(), ProviderRejection> {
    let Some(format) = text.and_then(|text| text.get("format")) else {
        return Ok(());
    };
    if format.get("type").and_then(Value::as_str) == Some("text") {
        Ok(())
    } else {
        Err(ProviderRejection::unsupported(
            "text.format",
            "structured output is not supported",
        ))
    }
}

/// Lower string or item-array input into normalized turns.
///
/// # Errors
///
/// Returns a rejection for unsupported top-level input or malformed items.
fn lower_input(
    input: Value,
    system: &mut Vec<String>,
) -> Result<Vec<CompatTurn>, ProviderRejection> {
    match input {
        Value::String(text) => Ok(vec![CompatTurn::User(text)]),
        Value::Array(items) => {
            let mut turns = Vec::new();
            for item in items {
                lower_item(&item, system, &mut turns)?;
            }
            Ok(turns)
        }
        _ => Err(ProviderRejection::unsupported(
            "input",
            "Responses input must be a string or an array of input items",
        )),
    }
}

/// Append one Responses input item to normalized conversation state.
///
/// # Errors
///
/// Returns a rejection for unsupported item types or malformed item fields.
fn lower_item(
    item: &Value,
    system: &mut Vec<String>,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), ProviderRejection> {
    match item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message")
    {
        "message" => lower_message(item, system, turns)?,
        "function_call" => turns.push(CompatTurn::ToolCall(lower_function_call(item)?)),
        "function_call_output" => {
            let output = item.get("output").ok_or_else(|| {
                ProviderRejection::invalid("input.output", "function_call_output is missing output")
            })?;
            turns.push(CompatTurn::ToolResult(value_text(output)));
        }
        item_type => {
            return Err(ProviderRejection::unsupported(
                "input.type",
                format!("unsupported Responses input item `{item_type}`"),
            ));
        }
    }
    Ok(())
}

/// Append one Responses message item according to its role.
///
/// # Errors
///
/// Returns a rejection for missing text or an unsupported role.
fn lower_message(
    item: &Value,
    system: &mut Vec<String>,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), ProviderRejection> {
    let role = item.get("role").and_then(Value::as_str).unwrap_or("user");
    let content = item.get("content").ok_or_else(|| {
        ProviderRejection::invalid("input.content", "message input is missing content")
    })?;
    let text = optional_text_content(content, "input.content", TextArrayKind::ResponsesParts)?
        .ok_or_else(|| {
            ProviderRejection::invalid("input.content", "message input must contain text")
        })?;
    match role {
        "user" => turns.push(CompatTurn::User(text)),
        "system" | "developer" => system.push(text),
        "assistant" => turns.push(CompatTurn::Assistant(text)),
        _ => {
            return Err(ProviderRejection::unsupported(
                "input.role",
                format!("unsupported Responses role `{role}`"),
            ));
        }
    }
    Ok(())
}

/// Decode one Responses function-call item.
///
/// # Errors
///
/// Returns a rejection for a missing name or non-object arguments.
fn lower_function_call(item: &Value) -> Result<FunctionCall, ProviderRejection> {
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            ProviderRejection::invalid("input.name", "function_call is missing its name")
        })?;
    let raw = item.get("arguments").ok_or_else(|| {
        ProviderRejection::invalid("input.arguments", "function_call is missing its arguments")
    })?;
    let arguments = match raw {
        Value::String(arguments) => serde_json::from_str(arguments).map_err(|error| {
            ProviderRejection::invalid(
                "input.arguments",
                format!("invalid function arguments: {error}"),
            )
        })?,
        value => value.clone(),
    };
    let arguments = JsonObject::from_value(arguments).ok_or_else(|| {
        ProviderRejection::invalid(
            "input.arguments",
            "function arguments must be a JSON object",
        )
    })?;
    Ok(FunctionCall {
        name: name.to_owned(),
        arguments,
    })
}

/// Validate and retain Responses function definitions.
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
                .get("name")
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

/// Normalize Responses tool-selection policy.
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
        let name = choice.get("name").and_then(Value::as_str).ok_or_else(|| {
            ProviderRejection::invalid("tool_choice", "named tool_choice is missing its name")
        })?;
        return Ok(ToolChoice::Named(name.to_owned()));
    }
    Err(ProviderRejection::invalid(
        "tool_choice",
        "tool_choice must be auto, none, required, or a named function",
    ))
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn usage_json(usage: TokenUsage) -> Value {
    json!({
        "input_tokens": usage.prompt,
        "input_tokens_details": {"cached_tokens":0},
        "output_tokens": usage.completion,
        "output_tokens_details": {"reasoning_tokens":0},
        "total_tokens": usage.total
    })
}

struct ResponseEnvelope {
    id: String,
    item_id: String,
    call_id: String,
    created_at: u64,
    body: Value,
}

impl From<&CompatTurnResponse> for ResponseEnvelope {
    fn from(response: &CompatTurnResponse) -> Self {
        let id = format!("resp_{}", Uuid::now_v7().simple());
        let item_id = match response.output {
            CompatOutput::Text(_) => format!("msg_{}", Uuid::now_v7().simple()),
            CompatOutput::ToolCall(_) => format!("fc_{}", Uuid::now_v7().simple()),
        };
        let call_id = format!("call_{}", Uuid::now_v7().simple());
        let created_at = unix_timestamp();
        let output = match &response.output {
            CompatOutput::Text(text) => vec![json!({
                "type":"message",
                "id":item_id,
                "status":"completed",
                "role":"assistant",
                "content":[{
                    "type":"output_text",
                    "text":text,
                    "annotations":[],
                    "logprobs":[]
                }]
            })],
            CompatOutput::ToolCall(call) => vec![json!({
                "type":"function_call",
                "id":item_id,
                "call_id":call_id,
                "name":call.name,
                "arguments":call.arguments.serialized(),
                "status":"completed"
            })],
        };
        let output_text = match &response.output {
            CompatOutput::Text(text) => text.as_str(),
            CompatOutput::ToolCall(_) => "",
        };
        let body = json!({
            "id":id,
            "object":"response",
            "created_at":created_at,
            "status":"completed",
            "error":null,
            "incomplete_details":null,
            "model":response.model,
            "output":output,
            "output_text":output_text,
            "usage":usage_json(response.usage)
        });
        Self {
            id,
            item_id,
            call_id,
            created_at,
            body,
        }
    }
}

fn response_stream(response: &CompatTurnResponse) -> SseEvents {
    let envelope = ResponseEnvelope::from(response);
    let mut sequence = 0;
    let mut events = Vec::new();
    push_event(
        &mut events,
        &mut sequence,
        "response.created",
        json!({"response": {
            "id":envelope.id,
            "object":"response",
            "created_at":envelope.created_at,
            "status":"in_progress",
            "model":response.model,
            "output":[],
            "error":null,
            "incomplete_details":null
        }}),
    );

    push_output_events(&mut events, &mut sequence, &envelope, &response.output);
    push_event(
        &mut events,
        &mut sequence,
        "response.output_item.done",
        json!({
            "output_index":0,
            "item":envelope.body["output"][0].clone()
        }),
    );
    push_event(
        &mut events,
        &mut sequence,
        "response.completed",
        json!({"response":envelope.body}),
    );
    SseEvents::from(events)
}

fn push_output_events(
    events: &mut Vec<Event>,
    sequence: &mut usize,
    envelope: &ResponseEnvelope,
    output: &CompatOutput,
) {
    match output {
        CompatOutput::Text(text) => push_text_events(events, sequence, envelope, text),
        CompatOutput::ToolCall(call) => push_tool_events(events, sequence, envelope, call),
    }
}

fn push_text_events(
    events: &mut Vec<Event>,
    sequence: &mut usize,
    envelope: &ResponseEnvelope,
    text: &str,
) {
    push_event(
        events,
        sequence,
        "response.output_item.added",
        json!({"output_index":0,"item":{
            "type":"message",
            "id":envelope.item_id,
            "status":"in_progress",
            "role":"assistant",
            "content":[]
        }}),
    );
    for chunk in stream_chunks(text) {
        push_event(
            events,
            sequence,
            "response.output_text.delta",
            json!({
                "item_id":envelope.item_id,
                "output_index":0,
                "content_index":0,
                "delta":chunk
            }),
        );
    }
    push_event(
        events,
        sequence,
        "response.output_text.done",
        json!({
            "item_id":envelope.item_id,
            "output_index":0,
            "content_index":0,
            "text":text
        }),
    );
}

fn push_tool_events(
    events: &mut Vec<Event>,
    sequence: &mut usize,
    envelope: &ResponseEnvelope,
    call: &FunctionCall,
) {
    let arguments = call.arguments.serialized();
    push_event(
        events,
        sequence,
        "response.output_item.added",
        json!({"output_index":0,"item":{
            "type":"function_call",
            "id":envelope.item_id,
            "call_id":envelope.call_id,
            "name":call.name,
            "arguments":"",
            "status":"in_progress"
        }}),
    );
    push_event(
        events,
        sequence,
        "response.function_call_arguments.delta",
        json!({
            "item_id":envelope.item_id,
            "output_index":0,
            "call_id":envelope.call_id,
            "delta":arguments
        }),
    );
    push_event(
        events,
        sequence,
        "response.function_call_arguments.done",
        json!({
            "item_id":envelope.item_id,
            "output_index":0,
            "call_id":envelope.call_id,
            "name":call.name,
            "arguments":arguments
        }),
    );
}

fn push_event(
    events: &mut Vec<Event>,
    sequence: &mut usize,
    event_type: &'static str,
    value: Value,
) {
    let Value::Object(mut object) = value else {
        unreachable!("Responses stream events are JSON objects")
    };
    object.insert("type".to_owned(), Value::String(event_type.to_owned()));
    object.insert("sequence_number".to_owned(), json!(*sequence));
    *sequence += 1;
    events.push(Value::Object(object).to_sse_event().event(event_type));
}

async fn responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ResponsesRequest>,
) -> Response {
    if let Err(error) = provider_authenticate(&headers, &state.config, ProviderAuth::Bearer) {
        return error.openai_response();
    }

    let should_stream = payload.should_stream;
    let request = match OpenAiResponsesTurn::try_from(payload) {
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
        response_stream(&response)
            .with_delay(state.config.stream_delay_ms)
            .into_response()
    } else {
        Json(ResponseEnvelope::from(&response).body).into_response()
    }
}

/// `OpenAI` Responses endpoint.
pub(super) struct OpenAiResponses;

impl OpenAiResponses {
    /// Mount the Responses route.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/v1/responses",
            post_with(responses, |operation| {
                operation
                    .summary("OpenAI response")
                    .tag("openai")
                    .response::<200, Json<Value>>()
                    .default_response::<Json<Value>>()
            }),
        )
    }
}
