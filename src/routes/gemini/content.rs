//! Native Gemini Generate Content adapter.
#![allow(
    rlib::missing_section_dividers,
    rlib::undocumented_early_returns,
    rlib::undocumented_items,
    reason = "private wire plumbing stays clearer without per-field docs, per-concept dividers, or comments restating errors"
)]

use std::collections::HashMap;
use std::str::FromStr;

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::super::context::{AppState, ProviderAuth, provider_authenticate};
use crate::types::http::{JsonEventExt, ProviderRejection, SseEvents, stream_chunks};
use crate::types::model::ModelId;
use crate::types::turn::{
    CompatOutput, CompatTurn, CompatTurnRequest, CompatTurnResponse, FunctionCall, FunctionTool,
    RequestLimits, TokenUsage, ToolChoice,
};

#[derive(Debug, Deserialize, JsonSchema)]
struct GeminiContent {
    role: Option<String>,
    #[serde(default = "missing_parts_are_empty")]
    parts: Vec<Value>,
}

/// Accepted native Gemini input.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenerateContentRequest {
    #[serde(default = "missing_contents_are_empty")]
    contents: Vec<GeminiContent>,
    #[serde(default)]
    system_instruction: Option<GeminiContent>,
    #[serde(default = "missing_parts_are_empty")]
    tools: Vec<Value>,
    #[serde(default)]
    tool_config: Option<Value>,
}

fn missing_contents_are_empty() -> Vec<GeminiContent> {
    Vec::new()
}

fn missing_parts_are_empty() -> Vec<Value> {
    Vec::new()
}

#[derive(Debug, Clone, Copy)]
enum GeminiActionKind {
    Generate,
    Stream,
}

impl GeminiActionKind {
    const fn is_stream(self) -> bool {
        matches!(self, Self::Stream)
    }
}

impl FromStr for GeminiActionKind {
    type Err = ProviderRejection;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "generateContent" => Ok(Self::Generate),
            "streamGenerateContent" => Ok(Self::Stream),
            action => Err(ProviderRejection::unsupported(
                "model",
                format!("unsupported Gemini model action `{action}`"),
            )),
        }
    }
}

struct GeminiAction {
    model: ModelId,
    kind: GeminiActionKind,
}

impl FromStr for GeminiAction {
    type Err = ProviderRejection;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some((model, action)) = value.split_once(':') else {
            return Err(ProviderRejection::invalid(
                "model",
                "Gemini model path must include a generation action",
            ));
        };
        let model = model.parse().map_err(|_| {
            ProviderRejection::invalid("model", "Gemini model id must not be empty")
        })?;
        Ok(Self {
            model,
            kind: action.parse()?,
        })
    }
}

/// Lower a native Gemini request into the shared execution model.
///
/// # Errors
///
/// Returns a rejection for malformed content, tools, or tool configuration.
fn lower_request(
    model: ModelId,
    payload: GenerateContentRequest,
) -> Result<CompatTurnRequest, ProviderRejection> {
    let mut system = Vec::new();
    if let Some(instruction) = payload.system_instruction {
        system.push(text_parts(&instruction.parts, "systemInstruction.parts")?);
    }

    let mut turns = Vec::new();
    for content in payload.contents {
        match content.role.as_deref().unwrap_or("user") {
            "user" => lower_user_parts(&content.parts, &mut turns)?,
            "model" => lower_model_parts(&content.parts, &mut turns)?,
            role => {
                return Err(ProviderRejection::unsupported(
                    "contents.role",
                    format!("unsupported Gemini role `{role}`"),
                ));
            }
        }
    }
    let tools = lower_tools(payload.tools)?;
    let choice = ToolChoice::try_from(GeminiToolConfig(payload.tool_config.as_ref()))?;
    Ok(CompatTurnRequest::new(model, system, turns, tools, choice))
}

/// Lower ordered Gemini user parts into text and function-result turns.
///
/// # Errors
///
/// Returns a rejection for empty, malformed, or unsupported parts.
fn lower_user_parts(parts: &[Value], turns: &mut Vec<CompatTurn>) -> Result<(), ProviderRejection> {
    if parts.is_empty() {
        return Err(ProviderRejection::invalid(
            "contents.parts",
            "content parts are required",
        ));
    }
    let mut text = Vec::new();
    for part in parts {
        if let Some(value) = part.get("text").and_then(Value::as_str) {
            text.push(value.to_owned());
            continue;
        }
        let Some(function) = part.get("functionResponse") else {
            return Err(ProviderRejection::unsupported(
                "contents.parts",
                "only text and functionResponse parts are supported",
            ));
        };
        if !text.is_empty() {
            turns.push(CompatTurn::User(std::mem::take(&mut text).join("\n")));
        }
        let response = function.get("response").ok_or_else(|| {
            ProviderRejection::invalid("contents.parts", "functionResponse is missing response")
        })?;
        turns.push(CompatTurn::ToolResult(response.to_string()));
    }
    if !text.is_empty() {
        turns.push(CompatTurn::User(text.join("\n")));
    }
    Ok(())
}

/// Lower ordered Gemini model parts into text and function-call turns.
///
/// # Errors
///
/// Returns a rejection for malformed or unsupported parts.
fn lower_model_parts(
    parts: &[Value],
    turns: &mut Vec<CompatTurn>,
) -> Result<(), ProviderRejection> {
    for part in parts {
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            turns.push(CompatTurn::Assistant(text.to_owned()));
            continue;
        }
        let Some(function) = part.get("functionCall") else {
            return Err(ProviderRejection::unsupported(
                "contents.parts",
                "only text and functionCall parts are supported",
            ));
        };
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                ProviderRejection::invalid("contents.parts", "functionCall is missing name")
            })?;
        let arguments = function.get("args").cloned().unwrap_or_else(|| json!({}));
        if !arguments.is_object() {
            return Err(ProviderRejection::invalid(
                "contents.parts",
                "functionCall args must be a JSON object",
            ));
        }
        turns.push(CompatTurn::ToolCall(FunctionCall {
            name: name.to_owned(),
            arguments,
        }));
    }
    Ok(())
}

/// Join a required list of native Gemini text parts.
///
/// # Errors
///
/// Returns a rejection when the list is empty or contains a non-text part.
fn text_parts(parts: &[Value], param: &'static str) -> Result<String, ProviderRejection> {
    if parts.is_empty() {
        return Err(ProviderRejection::invalid(param, "text parts are required"));
    }
    parts
        .iter()
        .map(|part| {
            part.get("text")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    ProviderRejection::unsupported(param, "only text parts are supported")
                })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("\n"))
}

/// Flatten native Gemini function-declaration groups.
///
/// # Errors
///
/// Returns a rejection for unsupported groups or declarations without names.
fn lower_tools(tools: Vec<Value>) -> Result<Vec<FunctionTool>, ProviderRejection> {
    let mut functions = Vec::new();
    for tool in tools {
        let Some(declarations) = tool.get("functionDeclarations").and_then(Value::as_array) else {
            return Err(ProviderRejection::unsupported(
                "tools",
                "only client function declarations are supported",
            ));
        };
        for declaration in declarations {
            let name = declaration
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    ProviderRejection::invalid("tools", "function declaration is missing name")
                })?;
            functions.push(FunctionTool::new(name.to_owned(), declaration.clone()));
        }
    }
    Ok(functions)
}

struct GeminiToolConfig<'a>(Option<&'a Value>);

struct GeminiAllowedFunctions<'a>(&'a Value);

impl From<GeminiAllowedFunctions<'_>> for ToolChoice {
    fn from(config: GeminiAllowedFunctions<'_>) -> Self {
        let allowed = config
            .0
            .get("allowedFunctionNames")
            .and_then(Value::as_array)
            .map(|names| {
                names
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        match allowed.as_slice() {
            [] => Self::Required,
            [name] => Self::Named(name.clone()),
            _ => Self::Allowed(allowed),
        }
    }
}

impl TryFrom<GeminiToolConfig<'_>> for ToolChoice {
    type Error = ProviderRejection;

    fn try_from(config: GeminiToolConfig<'_>) -> Result<Self, Self::Error> {
        let Some(calling) = config
            .0
            .and_then(|config| config.get("functionCallingConfig"))
        else {
            return Ok(Self::Auto);
        };
        let mode = calling
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("AUTO");
        match mode {
            "AUTO" | "VALIDATED" => Ok(Self::Auto),
            "NONE" => Ok(Self::None),
            "ANY" => Ok(Self::from(GeminiAllowedFunctions(calling))),
            _ => Err(ProviderRejection::invalid(
                "toolConfig",
                format!("unsupported function calling mode `{mode}`"),
            )),
        }
    }
}

fn usage_json(usage: TokenUsage) -> Value {
    json!({
        "promptTokenCount":usage.prompt,
        "candidatesTokenCount":usage.completion,
        "totalTokenCount":usage.total
    })
}

fn response_json(response: CompatTurnResponse) -> Value {
    let part = match response.output {
        CompatOutput::Text(text) => json!({"text":text}),
        CompatOutput::ToolCall(call) => json!({
            "functionCall":{
                "id":format!("call_{}", Uuid::now_v7().simple()),
                "name":call.name,
                "args":call.arguments
            }
        }),
    };
    json!({
        "candidates":[{
            "content":{"role":"model","parts":[part]},
            "finishReason":"STOP",
            "index":0
        }],
        "modelVersion":response.model,
        "usageMetadata":usage_json(response.usage)
    })
}

fn stream_records(response: &CompatTurnResponse) -> Vec<Value> {
    match &response.output {
        CompatOutput::Text(text) => text_stream_records(response, text),
        CompatOutput::ToolCall(call) => vec![json!({
            "candidates":[{
                "content":{"role":"model","parts":[{
                    "functionCall":{
                        "id":format!("call_{}", Uuid::now_v7().simple()),
                        "name":call.name,
                        "args":call.arguments
                    }
                }]},
                "finishReason":"STOP",
                "index":0
            }],
            "modelVersion":response.model,
            "usageMetadata":usage_json(response.usage)
        })],
    }
}

fn text_stream_records(response: &CompatTurnResponse, text: &str) -> Vec<Value> {
    let mut records = stream_chunks(text)
        .into_iter()
        .map(|chunk| {
            json!({
                "candidates":[{
                    "content":{"role":"model","parts":[{"text":chunk}]},
                    "index":0
                }],
                "modelVersion":response.model
            })
        })
        .collect::<Vec<_>>();
    records.push(json!({
        "candidates":[{
            "content":{"role":"model","parts":[{"text":""}]},
            "finishReason":"STOP",
            "index":0
        }],
        "modelVersion":response.model,
        "usageMetadata":usage_json(response.usage)
    }));
    records
}

async fn generate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(model_action): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    Json(payload): Json<GenerateContentRequest>,
) -> Response {
    // Authenticate with Gemini's native API-key header before lowering input.
    if let Err(error) = provider_authenticate(
        &headers,
        &state.config,
        ProviderAuth::ApiKey("x-goog-api-key"),
    ) {
        return error.gemini_response();
    }

    // The path selects both the model identifier and streaming behavior.
    let action = match model_action.parse::<GeminiAction>() {
        Ok(action) => action,
        Err(error) => return error.gemini_response(),
    };

    // Lower and execute the submitted provider transcript under shared limits.
    let request = match lower_request(action.model.clone(), payload) {
        Ok(request) => request,
        Err(error) => return error.gemini_response(),
    };
    let limits = RequestLimits::new(
        state.config.max_input_chars,
        state.config.max_history_messages,
    );
    let response = match request.complete(limits) {
        Ok(response) => response,
        Err(error) => return error.gemini_response(),
    };

    if !action.kind.is_stream() {
        return Json(response_json(response)).into_response();
    }
    let records = stream_records(&response);
    if query.get("alt").is_some_and(|value| value == "sse") {
        let events: Vec<Event> = records
            .into_iter()
            .map(|record| record.to_sse_event())
            .collect();
        SseEvents::from(events)
            .with_delay(state.config.stream_delay_ms)
            .into_response()
    } else {
        Json(Value::Array(records)).into_response()
    }
}

/// Native Gemini generation route.
pub(super) struct Route;

impl Route {
    /// Mount unary and streaming model actions.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/v1beta/models/{model_action}",
            post_with(generate, |operation| {
                operation
                    .summary("Gemini content")
                    .tag("gemini")
                    .response::<200, Json<Value>>()
                    .default_response::<Json<Value>>()
            }),
        )
    }
}
