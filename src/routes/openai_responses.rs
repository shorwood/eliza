//! `OpenAI` Responses route and wire contracts.

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::context::{AppState, provider_authenticate};
use crate::types::http::{ProviderRejection, TextArrayKind, optional_text_content, unix_timestamp};
use crate::types::model::ModelId;
use crate::types::turn::{CompatTurnRequest, CompatTurnResponse, RequestLimits, TokenUsage};

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

/// Preserve non-streaming behavior when clients omit the stream flag.
const fn should_stream_by_default() -> bool {
    false
}
// -----------------------------------------------------------------------------
// Response: Models successful Responses output.
// -----------------------------------------------------------------------------

/// Represents `ResponseContent` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
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
#[derive(Debug, Serialize, JsonSchema)]
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
#[derive(Debug, Serialize, JsonSchema)]
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
#[derive(Debug, Serialize, JsonSchema)]
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

    // User-authored content is required for a replay turn.
    let content = item.get("content").ok_or_else(|| {
        ProviderRejection::invalid("input.content", "input item is missing content")
    })?;

    // Only textual content is supported for replay.
    optional_text_content(content, "input.content", TextArrayKind::ResponsesParts)
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
// OpenAiRouteResponses: Authenticates and executes Responses requests.
// -----------------------------------------------------------------------------

/// Execute one `OpenAI` Responses request.
async fn open_ai_route_responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ResponsesRequest>,
) -> Response {
    // Render authentication failures in OpenAI's envelope immediately.
    if let Err(error) = provider_authenticate(&headers, &state.config) {
        return OpenAiRejection(error).into_response();
    }

    // Responses streaming has a different event contract; keep the supported
    // streaming surface on Chat Completions until that shape earns its place.
    if payload.should_stream {
        return OpenAiRejection(ProviderRejection::unsupported(
            "stream",
            "OpenAI Responses streaming is not implemented; use /v1/chat/completions streaming",
        ))
        .into_response();
    }

    let request = match OpenAiResponsesTurn::try_from(payload) {
        Ok(request) => request.0,
        // Invalid Responses payloads stop before the ELIZA engine is invoked.
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
    Json(ResponseEnvelope::from(response)).into_response()
}

// -----------------------------------------------------------------------------
// OpenAiResponses: Mounts the typed Responses contract into Aide.
// -----------------------------------------------------------------------------

/// `OpenAI` Responses endpoint and contract.
pub(super) struct OpenAiResponses;

impl OpenAiResponses {
    /// Mount the `OpenAI` Responses route.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/v1/responses",
            post_with(open_ai_route_responses, |operation| {
                operation
                    .summary("OpenAI response")
                    .tag("openai")
                    .response::<200, Json<ResponseEnvelope>>()
                    .default_response::<Json<OpenAiFailureResponse>>()
            }),
        )
    }
}
