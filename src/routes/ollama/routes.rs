//! Ollama chat and model catalog adapter.
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
    ChatOutputMessage, ChatRequest, ChatResponse, MessageContent, MessageRole, ModelDescriptor,
    ModelDetails, ModelListResponse, ToolCall,
};
use crate::routes::errors::ExtractionError;
use crate::types::http::{NdjsonResponse, stream_chunks};
use crate::types::lower::Lower;
use crate::types::model::ModelId;
use crate::types::turn::{CompatOutput, CompatTurn, CompatTurnRequest, CompatTurnResponse};

// -----------------------------------------------------------------------------
// CreatedAt: Defines deterministic model and response timestamps.
// -----------------------------------------------------------------------------

/// Stable timestamp used by the timeless ELIZA algorithm.
const CREATED_AT: &str = "1966-01-01T00:00:00Z";

// -----------------------------------------------------------------------------
// ChatLowering: Lowers one chat request into the neutral contract.
// -----------------------------------------------------------------------------

/// Ollama chat input awaiting provider-neutral conversion.
struct ChatLowering(
    /// Typed provider request.
    ChatRequest,
);

impl Lower for ChatLowering {
    type Canonical = CompatTurnRequest;

    type Error = OllamaError;

    fn lower(self) -> Result<Self::Canonical, Self::Error> {
        let payload = self.0;

        // Reject unsupported structured-output requests.
        if let Some(format) = &payload.format {
            return Err(OllamaError::StructuredOutputUnsupported {
                kind: format.label(),
            });
        }

        // Reject requests for a hidden reasoning trace.
        if payload.think.is_some() {
            return Err(OllamaError::ReasoningUnsupported);
        }

        // Separate instructions from replayable conversation turns.
        let mut system = Vec::new();
        let mut turns = Vec::new();
        for message in payload.messages {
            let content = MessageContent::into_text(message.content);
            match message.role {
                MessageRole::System => system.push(content),
                MessageRole::User => turns.push(CompatTurn::User(content)),
                MessageRole::Assistant => {
                    assistant_lower(content, message.tool_calls, &mut turns)?;
                }
                MessageRole::Tool => turns.push(CompatTurn::ToolResult(content)),
                // Unknown provider roles cannot be replayed safely.
                MessageRole::Unsupported => return Err(OllamaError::UnsupportedRole),
            }
        }

        // Lower tool declarations and selection policy independently.
        let tools = payload.tools.unwrap_or_default().lower()?;
        let tool_choice = payload
            .tool_choice
            .map(Lower::lower)
            .transpose()?
            .unwrap_or_default();

        Ok(CompatTurnRequest::builder()
            .model(payload.model)
            .system_text(system)
            .turns(turns)
            .tools(tools)
            .tool_choice(tool_choice)
            .build())
    }
}

// -----------------------------------------------------------------------------
// AssistantLower: Lowers assistant text and tool calls in source order.
// -----------------------------------------------------------------------------

/// Lower assistant text and calls while preserving their provider order.
///
/// # Errors
///
/// Returns [`OllamaError`] when a tool call is unsupported or incomplete.
fn assistant_lower(
    content: String,
    tool_calls: Option<Vec<ToolCall>>,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), OllamaError> {
    if !content.is_empty() {
        turns.push(CompatTurn::Assistant(content));
    }
    for call in tool_calls.unwrap_or_default() {
        turns.push(CompatTurn::ToolCall(call.lower("messages.tool_calls")?));
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// StreamRecords: Renders Ollama's NDJSON response sequence.
// -----------------------------------------------------------------------------

/// Render one record for each deterministic text chunk.
fn stream_records_text(response: &CompatTurnResponse, text: &str) -> Vec<ChatResponse> {
    stream_chunks(text)
        .into_iter()
        .map(|chunk| ChatResponse {
            model: response.model.clone(),
            created_at: CREATED_AT,
            message: ChatOutputMessage {
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

/// Render all NDJSON records for one completed neutral response.
fn stream_records(response: &CompatTurnResponse) -> Vec<ChatResponse> {
    let mut records = match &response.output {
        CompatOutput::Text(text) => stream_records_text(response, text),
        CompatOutput::ToolCall(_) => vec![ChatResponse {
            model: response.model.clone(),
            created_at: CREATED_AT,
            message: ChatOutputMessage::from(&response.output),
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
        message: ChatOutputMessage {
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

// -----------------------------------------------------------------------------
// Chat: Authenticates and executes Ollama chat requests.
// -----------------------------------------------------------------------------

/// Handle one unary or streaming Ollama chat request.
async fn chat(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<ChatRequest>, JsonRejection>,
) -> Response {
    // Authentication failures use Ollama's native error envelope.
    if let Err(error) = provider_authenticate(&headers, &state.config, ProviderAuth::Bearer) {
        return OllamaRejection::from_error(&error).into_response();
    }
    let Json(payload) = match payload {
        Ok(payload) => payload,
        // Extraction failures retain their typed diagnostic code.
        Err(error) => {
            return OllamaRejection::from_error(&ExtractionError::from(error)).into_response();
        }
    };

    // Capture transport policy before lowering consumes the request body.
    let should_stream = payload.should_stream.unwrap_or(true);
    let request = match ChatLowering(payload).lower() {
        Ok(request) => request,
        // Provider validation failures use Ollama's native envelope.
        Err(error) => {
            return OllamaRejection::from_error(&error).into_response();
        }
    };

    // Enforce shared request bounds after provider-specific lowering.
    // Complete one neutral turn before rendering Ollama output.
    let response = match request.complete(state.config.limits) {
        Ok(response) => response,
        // Shared execution failures still render as Ollama errors.
        Err(error) => {
            return OllamaRejection::from_error(&error).into_response();
        }
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

// -----------------------------------------------------------------------------
// Tags: Renders Ollama's configured model catalog.
// -----------------------------------------------------------------------------

/// Build the model catalog for the configured ELIZA identity.
fn tags_response_for_eliza(model: ModelId) -> ModelListResponse {
    ModelListResponse {
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
    }
}

/// List the configured model in Ollama's native envelope.
async fn tags(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Authentication failures use Ollama's native error envelope.
    if let Err(error) = provider_authenticate(&headers, &state.config, ProviderAuth::Bearer) {
        return OllamaRejection::from_error(&error).into_response();
    }
    Json(tags_response_for_eliza(state.config.model.clone())).into_response()
}

// -----------------------------------------------------------------------------
// Ollama: Mounts native chat and model-list endpoints.
// -----------------------------------------------------------------------------

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
                    .response::<200, Json<ModelListResponse>>()
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
