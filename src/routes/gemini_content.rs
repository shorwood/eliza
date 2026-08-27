//! Gemini Generate Content wire adapter.
//!
//! Gemini encodes the action in the model path. This adapter parses that path,
//! lowers textual `contents` and `systemInstruction` parts into the replay
//! contract, and renders native Gemini JSON or SSE.
//! Native Gemini streaming can be returned either as a JSON array or as SSE
//! when callers pass `alt=sse`; both use the same generated chunk bodies.

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
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::context::{AppState, provider_authenticate};
use crate::types::http::{ProviderRejection, SseEvents, required_text_parts, stream_chunks};
use crate::types::model::ModelId;
use crate::types::turn::{CompatTurnRequest, CompatTurnResponse, RequestLimits, TokenUsage};

// -----------------------------------------------------------------------------
// GeminiContent: Captures one textual content entry.
// -----------------------------------------------------------------------------

/// Represents `GeminiContent` state within this module.
#[derive(Debug, Deserialize, JsonSchema)]
struct GeminiContent {
    /// Stores the role value owned by this contract.
    #[serde(default)]
    role: Option<String>,
    /// Stores the parts value owned by this contract.
    #[serde(default = "missing_parts_are_empty")]
    parts: Vec<Value>,
}

// -----------------------------------------------------------------------------
// GenerateContentRequest: Captures one native Gemini request.
// -----------------------------------------------------------------------------

/// Represents `GenerateContentRequest` state within this module.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GenerateContentRequest {
    /// Stores the contents value owned by this contract.
    #[serde(default = "missing_contents_are_empty")]
    contents: Vec<GeminiContent>,
    /// Stores the system instruction value owned by this contract.
    #[serde(rename = "systemInstruction")]
    #[serde(default)]
    system_instruction: Option<GeminiContent>,
}

/// Preserve Gemini's empty-part validation path when parts are omitted.
fn missing_parts_are_empty() -> Vec<Value> {
    Vec::new()
}

/// Preserve Gemini's empty-request validation path when contents are omitted.
fn missing_contents_are_empty() -> Vec<GeminiContent> {
    Vec::new()
}

// -----------------------------------------------------------------------------
// GeminiAction: Parses model ids and generation actions from paths.
// -----------------------------------------------------------------------------

/// Enumerates the supported `GeminiActionKind` cases.
#[derive(Debug, Clone, Copy)]
enum GeminiActionKind {
    /// Represents the `GenerateContent` case.
    GenerateContent,
    /// Represents the `StreamGenerateContent` case.
    StreamGenerateContent,
}

impl FromStr for GeminiActionKind {
    type Err = ProviderRejection;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "generateContent" => Ok(Self::GenerateContent),
            "streamGenerateContent" => Ok(Self::StreamGenerateContent),
            action => Err(ProviderRejection::unsupported(
                "model",
                format!("unsupported Gemini model action `{action}`"),
            )),
        }
    }
}

impl GeminiActionKind {
    /// Performs the is stream operation for this abstraction.
    const fn is_stream(self) -> bool {
        matches!(self, Self::StreamGenerateContent)
    }
}

/// Represents `GeminiModelAction` state within this module.
#[derive(Debug, Clone)]
struct GeminiAction {
    /// Stores the model value owned by this contract.
    model: ModelId,
    /// Stores the kind value owned by this contract.
    kind: GeminiActionKind,
}

impl FromStr for GeminiAction {
    type Err = ProviderRejection;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        // Axum captures everything after `/v1beta/models/`; split the
        // provider model id from the requested generation action.
        let Some((model, action)) = value.split_once(':') else {
            return Err(ProviderRejection::invalid(
                "model",
                "Gemini model path must include :generateContent or :streamGenerateContent",
            ));
        };

        let model = model.parse::<ModelId>().map_err(|_| {
            ProviderRejection::invalid("model", "Gemini model id must not be empty")
        })?;

        Ok(Self {
            model,
            kind: action.parse()?,
        })
    }
}

// -----------------------------------------------------------------------------
// GeminiTextPart: Models one generated text part.
// -----------------------------------------------------------------------------

/// Represents `GeminiTextPart` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
struct GeminiTextPart {
    /// Stores the text value owned by this contract.
    text: String,
}

// -----------------------------------------------------------------------------
// GeminiContentResponse: Models generated content.
// -----------------------------------------------------------------------------

/// Represents `GeminiContentResponse` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
struct GeminiContentResponse {
    /// Stores the role value owned by this contract.
    role: &'static str,
    /// Stores the parts value owned by this contract.
    parts: Vec<GeminiTextPart>,
}

// -----------------------------------------------------------------------------
// GeminiCandidate: Models one generated candidate.
// -----------------------------------------------------------------------------

/// Represents `GeminiCandidate` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
struct GeminiCandidate {
    /// Stores the content value owned by this contract.
    content: GeminiContentResponse,
    /// Stores the finish reason value owned by this contract.
    #[serde(rename = "finishReason")]
    finish_reason: &'static str,
    /// Stores the index value owned by this contract.
    index: usize,
}

// -----------------------------------------------------------------------------
// GeminiUsageMetadata: Models Gemini token accounting.
// -----------------------------------------------------------------------------

/// Represents `GeminiUsageMetadata` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
struct GeminiUsageMetadata {
    /// Stores the prompt value owned by this contract.
    #[serde(rename = "promptTokenCount")]
    prompt: usize,
    /// Stores the candidates value owned by this contract.
    #[serde(rename = "candidatesTokenCount")]
    candidates: usize,
    /// Stores the total value owned by this contract.
    #[serde(rename = "totalTokenCount")]
    total: usize,
}

impl From<TokenUsage> for GeminiUsageMetadata {
    fn from(usage: TokenUsage) -> Self {
        Self {
            prompt: usage.prompt,
            candidates: usage.completion,
            total: usage.total,
        }
    }
}

// -----------------------------------------------------------------------------
// GeminiGenerateResponse: Models generated response chunks.
// -----------------------------------------------------------------------------

/// Represents `GeminiGenerateResponse` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct GeminiGenerateResponse {
    /// Stores the candidates value owned by this contract.
    candidates: Vec<GeminiCandidate>,
    /// Stores the model version value owned by this contract.
    model_version: ModelId,
    /// Stores the usage metadata value owned by this contract.
    usage_metadata: GeminiUsageMetadata,
}

impl GeminiGenerateResponse {
    /// Build one Gemini response envelope from completed ELIZA output.
    fn new(output: String, model: ModelId, usage: TokenUsage) -> Self {
        Self {
            candidates: vec![GeminiCandidate {
                content: GeminiContentResponse {
                    role: "model",
                    parts: vec![GeminiTextPart { text: output }],
                },
                finish_reason: "STOP",
                index: 0,
            }],
            model_version: model,
            usage_metadata: GeminiUsageMetadata::from(usage),
        }
    }
}

impl From<GeminiGenerateResponse> for Event {
    fn from(response: GeminiGenerateResponse) -> Self {
        Event::default()
            .json_data(response)
            .expect("Gemini stream chunk should serialize")
    }
}

/// Ordered Gemini response chunks rendered as JSON or SSE.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(transparent)]
struct GeminiGenerateResponseList(
    /// Chunks delivered in insertion order.
    Vec<GeminiGenerateResponse>,
);

impl GeminiGenerateResponseList {
    /// Split one completed response into incremental Gemini chunks.
    fn from_completed(response: CompatTurnResponse) -> Self {
        let mut chunks = Vec::new();

        // Intermediate chunks carry prompt usage with zero candidate usage.
        for chunk in stream_chunks(&response.output) {
            chunks.push(GeminiGenerateResponse::new(
                chunk,
                response.model.clone(),
                TokenUsage {
                    prompt: response.usage.prompt,
                    completion: 0,
                    total: response.usage.prompt,
                },
            ));
        }

        // The final empty chunk carries the complete usage summary.
        chunks.push(GeminiGenerateResponse::new(
            String::new(),
            response.model,
            response.usage,
        ));
        Self(chunks)
    }
}

// -----------------------------------------------------------------------------
// GeminiFailure: Models native Gemini error envelopes.
// -----------------------------------------------------------------------------

/// Represents `GeminiFailureBody` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
struct GeminiFailureBody {
    /// Stores the code value owned by this contract.
    code: u16,
    /// Stores the message value owned by this contract.
    message: String,
    /// Stores the status value owned by this contract.
    status: &'static str,
}

/// Represents `GeminiFailureResponse` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
struct GeminiFailureResponse {
    /// Stores the error value owned by this contract.
    error: GeminiFailureBody,
}

// -----------------------------------------------------------------------------
// LowerGenerateContent: Lowers text parts into replay input.
// -----------------------------------------------------------------------------

/// Lower a Gemini generateContent request into the provider-neutral replay shape.
///
/// # Errors
///
/// Returns [`ProviderRejection`] when roles are unsupported or any content part
/// is not textual.
fn lower_generate_content(
    model: ModelId,
    payload: GenerateContentRequest,
) -> Result<CompatTurnRequest, ProviderRejection> {
    let mut system_text = Vec::new();

    // Gemini's system instruction is textual context only; ELIZA has no
    // separate instruction channel, so it is counted but not replayed.
    if let Some(system_instruction) = payload.system_instruction {
        system_text.push(required_text_parts(
            &system_instruction.parts,
            "systemInstruction.parts",
        )?);
    }

    let mut user_turns = Vec::new();
    for content in payload.contents {
        match content.role.as_deref().unwrap_or("user") {
            // Omitted roles default to user in Gemini examples and become
            // replayed ELIZA turns.
            "user" => user_turns.push(required_text_parts(&content.parts, "contents.parts")?),
            // Model turns are prior assistant output from the client side.
            "model" => {}
            // Other roles cannot be represented in the shared transcript.
            role => {
                return Err(ProviderRejection::unsupported(
                    "contents.role",
                    format!("unsupported Gemini role `{role}`"),
                ));
            }
        }
    }

    Ok(CompatTurnRequest::new(model, system_text, user_turns))
}

// -----------------------------------------------------------------------------
// GeminiRejection: Renders Gemini failures.
// -----------------------------------------------------------------------------

/// Provider rejection rendered in Gemini's error envelope.
struct GeminiRejection(
    /// Rejection facts rendered by this provider.
    ProviderRejection,
);

impl IntoResponse for GeminiRejection {
    fn into_response(self) -> Response {
        let error = self.0;
        (
            error.status,
            Json(GeminiFailureResponse {
                error: GeminiFailureBody {
                    code: error.status.as_u16(),
                    message: error.message,
                    status: error.gemini_status,
                },
            }),
        )
            .into_response()
    }
}

// -----------------------------------------------------------------------------
// GeminiActionHandler: Dispatches generation actions.
// -----------------------------------------------------------------------------

/// Axum handler owner for Gemini model actions encoded in request paths.
struct GeminiActionHandler;

impl GeminiActionHandler {
    /// Authenticate, lower, execute, and render one Gemini model action.
    async fn handle(
        State(state): State<AppState>,
        headers: HeaderMap,
        Path(model_action): Path<String>,
        Query(query): Query<HashMap<String, String>>,
        Json(payload): Json<GenerateContentRequest>,
    ) -> Response {
        std::future::ready(()).await;

        // Render authentication failures in Gemini's envelope immediately.
        if let Err(error) = provider_authenticate(&headers, &state.config) {
            return GeminiRejection(error).into_response();
        }

        // The path chooses both model id and generation mode.
        let action = match model_action.parse::<GeminiAction>() {
            Ok(action) => action,
            // Invalid model actions stop before request lowering.
            Err(error) => return GeminiRejection(error).into_response(),
        };

        let request = match lower_generate_content(action.model.clone(), payload) {
            Ok(request) => request,
            // Invalid payloads stop before the ELIZA engine is invoked.
            Err(error) => return GeminiRejection(error).into_response(),
        };
        let limits = RequestLimits::new(
            state.config.max_input_chars,
            state.config.max_history_messages,
        );
        let response = match request.complete(limits) {
            Ok(response) => response,
            // Engine rejections retain Gemini's error shape.
            Err(error) => return GeminiRejection(error).into_response(),
        };

        if action.kind.is_stream() {
            let chunks = GeminiGenerateResponseList::from_completed(response);

            // Gemini supports SSE through an `alt=sse` query flag; otherwise
            // return the same chunks as a JSON array for simple raw REST clients.
            if query.get("alt").is_some_and(|value| value == "sse") {
                SseEvents::from(chunks.0.into_iter().map(Event::from).collect::<Vec<_>>())
                    .with_delay(state.config.stream_delay_ms)
                    .into_response()
            } else {
                Json(chunks).into_response()
            }
        } else {
            Json(GeminiGenerateResponse::new(
                response.output,
                response.model,
                response.usage,
            ))
            .into_response()
        }
    }
}

// -----------------------------------------------------------------------------
// GeminiContentRoute: Mounts typed Gemini contracts into Aide.
// -----------------------------------------------------------------------------

/// Gemini content generation endpoint and contract.
pub(super) struct GeminiContentRoute;

impl GeminiContentRoute {
    /// Mount the native Gemini content route from its owning contracts.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/v1beta/models/{model_action}",
            post_with(GeminiActionHandler::handle, |operation| {
                operation
                    .summary("Gemini content")
                    .tag("gemini")
                    .response::<200, Json<GeminiGenerateResponse>>()
                    .default_response::<Json<GeminiFailureResponse>>()
            }),
        )
    }
}
