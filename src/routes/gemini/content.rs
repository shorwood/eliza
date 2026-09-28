//! Native Gemini Generate Content adapter.
#![expect(
    clippy::missing_errors_doc,
    rlib::missing_section_dividers,
    rlib::undocumented_early_returns,
    rlib::undocumented_items,
    reason = "private adapter stages stay in request flow; validation errors describe each guard"
)]

use std::str::FromStr;

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use super::super::context::{AppState, ProviderAuth, provider_authenticate};
use super::types::{
    Candidate, ContentRole, FinishReason, FunctionCallOutput, FunctionResponseValue, GeminiContent,
    GeminiFailureResponse, GeminiRejection, GenerateContentRequest, GenerateContentResponse,
    GenerateQuery, OutputContent, OutputPart, Part, StreamFormat, ToolConfig,
};
use crate::types::http::{ProviderRejection, SseEvents, json_event, stream_chunks};
use crate::types::model::ModelId;
use crate::types::turn::{
    CompatOutput, CompatTurn, CompatTurnRequest, CompatTurnResponse, FunctionCall, RequestLimits,
};

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

fn lower_request(
    model: ModelId,
    payload: GenerateContentRequest,
) -> Result<CompatTurnRequest, ProviderRejection> {
    let mut system = Vec::new();
    if let Some(instruction) = payload.system_instruction {
        system.push(text_parts(
            instruction.parts.unwrap_or_default(),
            "systemInstruction.parts",
        )?);
    }

    let mut turns = Vec::new();
    for content in payload.contents.unwrap_or_default() {
        lower_content(content, &mut turns)?;
    }
    Ok(CompatTurnRequest::new(
        model,
        system,
        turns,
        payload.tools.unwrap_or_default().into_domain()?,
        ToolConfig::into_domain(payload.tool_config, "toolConfig")?,
    ))
}

fn lower_content(
    content: GeminiContent,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), ProviderRejection> {
    let parts = content.parts.unwrap_or_default();
    match content.role.unwrap_or(ContentRole::User) {
        ContentRole::User | ContentRole::Function => lower_user_parts(parts, turns),
        ContentRole::Model => lower_model_parts(parts, turns),
        ContentRole::Unsupported => Err(ProviderRejection::unsupported(
            "contents.role",
            "unsupported Gemini role",
        )),
    }
}

fn lower_user_parts(
    parts: Vec<Part>,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), ProviderRejection> {
    if parts.is_empty() {
        return Err(ProviderRejection::invalid(
            "contents.parts",
            "content parts are required",
        ));
    }
    let mut text = Vec::new();
    for part in parts {
        match part {
            Part::Text { text: part } => text.push(part),
            Part::FunctionResponse { function_response } => {
                flush_user_text(&mut text, turns);
                let response = function_response.response.ok_or_else(|| {
                    ProviderRejection::invalid(
                        "contents.parts",
                        "functionResponse is missing response",
                    )
                })?;
                turns.push(CompatTurn::ToolResult(match response {
                    FunctionResponseValue::Text(text) => text,
                    FunctionResponseValue::Object(object) => object.serialized(),
                }));
            }
            Part::FunctionCall { .. } | Part::Unsupported { .. } => {
                return Err(ProviderRejection::unsupported(
                    "contents.parts",
                    "only text and functionResponse parts are supported",
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

fn lower_model_parts(
    parts: Vec<Part>,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), ProviderRejection> {
    for part in parts {
        match part {
            Part::Text { text } => turns.push(CompatTurn::Assistant(text)),
            Part::FunctionCall { function_call } => {
                let name = function_call
                    .name
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| {
                        ProviderRejection::invalid("contents.parts", "functionCall is missing name")
                    })?;
                turns.push(CompatTurn::ToolCall(FunctionCall {
                    name,
                    arguments: function_call.args.unwrap_or_default(),
                }));
            }
            Part::FunctionResponse { .. } | Part::Unsupported { .. } => {
                return Err(ProviderRejection::unsupported(
                    "contents.parts",
                    "only text and functionCall parts are supported",
                ));
            }
        }
    }
    Ok(())
}

fn text_parts(parts: Vec<Part>, param: &'static str) -> Result<String, ProviderRejection> {
    if parts.is_empty() {
        return Err(ProviderRejection::invalid(param, "text parts are required"));
    }
    parts
        .into_iter()
        .map(|part| match part {
            Part::Text { text } => Ok(text),
            Part::FunctionCall { .. }
            | Part::FunctionResponse { .. }
            | Part::Unsupported { .. } => Err(ProviderRejection::unsupported(
                param,
                "only text parts are supported",
            )),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("\n"))
}

fn stream_records(response: &CompatTurnResponse) -> Vec<GenerateContentResponse> {
    match &response.output {
        CompatOutput::Text(text) => text_stream_records(response, text),
        CompatOutput::ToolCall(call) => vec![GenerateContentResponse {
            candidates: vec![Candidate {
                content: OutputContent {
                    role: "model",
                    parts: vec![OutputPart::FunctionCall {
                        function_call: FunctionCallOutput {
                            id: format!("call_{}", Uuid::now_v7().simple()),
                            name: call.name.clone(),
                            args: call.arguments.clone(),
                        },
                    }],
                },
                finish_reason: Some(FinishReason::Stop),
                index: 0,
            }],
            model_version: response.model.clone(),
            usage_metadata: Some(response.usage.into()),
        }],
    }
}

fn text_stream_records(response: &CompatTurnResponse, text: &str) -> Vec<GenerateContentResponse> {
    let mut records = stream_chunks(text)
        .into_iter()
        .map(|chunk| GenerateContentResponse {
            candidates: vec![Candidate {
                content: OutputContent {
                    role: "model",
                    parts: vec![OutputPart::Text { text: chunk }],
                },
                finish_reason: None,
                index: 0,
            }],
            model_version: response.model.clone(),
            usage_metadata: None,
        })
        .collect::<Vec<_>>();
    records.push(GenerateContentResponse {
        candidates: vec![Candidate {
            content: OutputContent {
                role: "model",
                parts: vec![OutputPart::Text {
                    text: String::new(),
                }],
            },
            finish_reason: Some(FinishReason::Stop),
            index: 0,
        }],
        model_version: response.model.clone(),
        usage_metadata: Some(response.usage.into()),
    });
    records
}

async fn generate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(model_action): Path<String>,
    Query(query): Query<GenerateQuery>,
    Json(payload): Json<GenerateContentRequest>,
) -> Response {
    if let Err(error) = provider_authenticate(
        &headers,
        &state.config,
        ProviderAuth::ApiKey("x-goog-api-key"),
    ) {
        return GeminiRejection::from(error).into_response();
    }

    // Decode the model and generation action carried by Gemini's path grammar.
    let action = match model_action.parse::<GeminiAction>() {
        Ok(action) => action,
        Err(error) => return GeminiRejection::from(error).into_response(),
    };

    // Lower and execute the provider request under shared resource limits.
    let request = match lower_request(action.model.clone(), payload) {
        Ok(request) => request,
        Err(error) => return GeminiRejection::from(error).into_response(),
    };
    let limits = RequestLimits::new(
        state.config.max_input_chars,
        state.config.max_history_messages,
    );
    let response = match request.complete(limits) {
        Ok(response) => response,
        Err(error) => return GeminiRejection::from(error).into_response(),
    };

    if !action.kind.is_stream() {
        return Json(GenerateContentResponse::from(response)).into_response();
    }
    let records = stream_records(&response);
    if matches!(query.alt, Some(StreamFormat::Sse)) {
        let events = records
            .iter()
            .map(json_event)
            .collect::<Result<Vec<Event>, _>>();
        return match events {
            Ok(events) => SseEvents::from(events)
                .with_delay(state.config.stream_delay_ms)
                .into_response(),
            Err(error) => GeminiRejection::from(error).into_response(),
        };
    }
    Json(records).into_response()
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
                    .response::<200, Json<GenerateContentResponse>>()
                    .default_response::<Json<GeminiFailureResponse>>()
            }),
        )
    }
}
