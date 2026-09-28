//! Ollama chat and model catalog adapter.
#![expect(
    rlib::missing_section_dividers,
    rlib::undocumented_early_returns,
    rlib::undocumented_items,
    reason = "private adapter stages stay in request flow; validation errors describe each guard"
)]

use aide::axum::ApiRouter;
use aide::axum::routing::{get_with, post_with};
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};

use super::super::context::{AppState, ProviderAuth, provider_authenticate};
use super::errors::{OllamaError, OllamaFailureResponse, OllamaRejection};
use super::types::{
    ChatRequest, ChatResponse, MessageContent, MessageRole, ModelDescriptor, ModelDetails,
    OutputMessage, TagsResponse,
};
use crate::routes::errors::ExtractionError;
use crate::types::http::{NdjsonResponse, stream_chunks};
use crate::types::turn::{
    CompatOutput, CompatTurn, CompatTurnRequest, CompatTurnResponse, RequestLimits,
};

const CREATED_AT: &str = "1966-01-01T00:00:00Z";

struct OllamaTurn(CompatTurnRequest);

impl TryFrom<ChatRequest> for OllamaTurn {
    type Error = OllamaError;

    fn try_from(payload: ChatRequest) -> Result<Self, Self::Error> {
        if let Some(format) = &payload.format {
            return Err(OllamaError::StructuredOutputUnsupported {
                kind: format.label(),
            });
        }
        if payload.think.is_some() {
            return Err(OllamaError::ReasoningUnsupported);
        }

        let mut system = Vec::new();
        let mut turns = Vec::new();
        for message in payload.messages {
            let content = MessageContent::into_text(message.content);
            match message.role {
                MessageRole::System => system.push(content),
                MessageRole::User => turns.push(CompatTurn::User(content)),
                MessageRole::Assistant => {
                    if !content.is_empty() {
                        turns.push(CompatTurn::Assistant(content));
                    }
                    for call in message.tool_calls.unwrap_or_default() {
                        turns.push(CompatTurn::ToolCall(call.lower("messages.tool_calls")?));
                    }
                }
                MessageRole::Tool => turns.push(CompatTurn::ToolResult(content)),
                MessageRole::Unsupported => {
                    return Err(OllamaError::UnsupportedRole);
                }
            }
        }

        Ok(Self(CompatTurnRequest::new(
            payload.model,
            system,
            turns,
            payload.tools.unwrap_or_default().into_domain()?,
            payload.tool_choice.try_into()?,
        )))
    }
}

fn stream_records(response: &CompatTurnResponse) -> Vec<ChatResponse> {
    let mut records = match &response.output {
        CompatOutput::Text(text) => text_stream_records(response, text),
        CompatOutput::ToolCall(_) => vec![ChatResponse {
            model: response.model.clone(),
            created_at: CREATED_AT,
            message: OutputMessage::from(&response.output),
            is_done: false,
            done_reason: None,
            total_duration: None,
            load_duration: None,
            prompt_eval_count: None,
            prompt_eval_duration: None,
            eval_count: None,
            eval_duration: None,
        }],
    };
    records.push(ChatResponse {
        model: response.model.clone(),
        created_at: CREATED_AT,
        message: OutputMessage {
            role: "assistant",
            content: String::new(),
            tool_calls: Vec::new(),
        },
        is_done: true,
        done_reason: Some("stop"),
        total_duration: Some(0),
        load_duration: Some(0),
        prompt_eval_count: Some(response.usage.prompt),
        prompt_eval_duration: Some(0),
        eval_count: Some(response.usage.completion),
        eval_duration: Some(0),
    });
    records
}

fn text_stream_records(response: &CompatTurnResponse, text: &str) -> Vec<ChatResponse> {
    stream_chunks(text)
        .into_iter()
        .map(|chunk| ChatResponse {
            model: response.model.clone(),
            created_at: CREATED_AT,
            message: OutputMessage {
                role: "assistant",
                content: chunk,
                tool_calls: Vec::new(),
            },
            is_done: false,
            done_reason: None,
            total_duration: None,
            load_duration: None,
            prompt_eval_count: None,
            prompt_eval_duration: None,
            eval_count: None,
            eval_duration: None,
        })
        .collect()
}

async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<ChatRequest>, JsonRejection>,
) -> Response {
    if let Err(error) = provider_authenticate(&headers, &state.config, ProviderAuth::Bearer) {
        return OllamaRejection::from_error(&error).into_response();
    }
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(error) => {
            return OllamaRejection::from_error(&ExtractionError::from(error)).into_response();
        }
    };

    // Capture transport policy before lowering consumes the request body.
    let should_stream = payload.should_stream.unwrap_or(true);
    let request = match OllamaTurn::try_from(payload) {
        Ok(request) => request.0,
        Err(error) => return OllamaRejection::from_error(&error).into_response(),
    };
    let limits = RequestLimits::new(
        state.config.max_input_chars,
        state.config.max_history_messages,
    );
    let response = match request.complete(limits) {
        Ok(response) => response,
        Err(error) => return OllamaRejection::from_error(&error).into_response(),
    };

    if should_stream {
        match NdjsonResponse::new(stream_records(&response), state.config.stream_delay_ms) {
            Ok(response) => response.into_response(),
            Err(error) => OllamaRejection::from_error(&error).into_response(),
        }
    } else {
        Json(ChatResponse::from(&response)).into_response()
    }
}

async fn tags(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(error) = provider_authenticate(&headers, &state.config, ProviderAuth::Bearer) {
        return OllamaRejection::from_error(&error).into_response();
    }
    let model = state.config.model.clone();
    Json(TagsResponse {
        models: vec![ModelDescriptor {
            name: model.clone(),
            model,
            modified_at: CREATED_AT,
            size: 0,
            digest: "eliza-doctor",
            details: ModelDetails {
                parent_model: "",
                format: "eliza",
                family: "eliza",
                families: vec!["eliza"],
                parameter_size: "DOCTOR",
                quantization_level: "none",
            },
        }],
    })
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
                    .response::<200, Json<TagsResponse>>()
                    .default_response::<Json<OllamaFailureResponse>>()
            }),
        );
        router.api_route(
            "/api/chat",
            post_with(chat, |operation| {
                operation
                    .summary("Ollama chat")
                    .tag("ollama")
                    .response::<200, Json<ChatResponse>>()
                    .default_response::<Json<OllamaFailureResponse>>()
            }),
        )
    }
}
