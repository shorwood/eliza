//! Gemini Generate Content wire adapter.
//!
//! Gemini encodes the action in the model path. This adapter parses that path,
//! lowers textual `contents` and `systemInstruction` parts into the replay
//! contract, and renders native Gemini JSON or SSE.
//! Native Gemini streaming can be returned either as a JSON array or as SSE
//! when callers pass `alt=sse`; both use the same generated chunk bodies.

use std::collections::HashMap;
use std::str::FromStr;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    CompatTurnRequest, CompatTurnResponse, ModelId, ProviderRejection, TokenUsage, complete_eliza,
    required_text_parts, sse_response, stream_chunks,
};
use crate::serve::{AppState, require_provider_auth};

// -----------------------------------------------------------------------------
// Gemini request contract: capture the textual `contents` and optional
// `systemInstruction` parts that can become an ELIZA transcript.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
struct GeminiContent {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    parts: Vec<Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GenerateContentRequest {
    #[serde(default)]
    contents: Vec<GeminiContent>,
    #[serde(rename = "systemInstruction")]
    #[serde(default)]
    system_instruction: Option<GeminiContent>,
}

// -----------------------------------------------------------------------------
// Gemini model-action path parsing: split `models/{model}:action` into the
// provider-visible model id and supported generation mode.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum GeminiActionKind {
    GenerateContent,
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
    const fn is_stream(self) -> bool {
        matches!(self, Self::StreamGenerateContent)
    }
}

#[derive(Debug, Clone)]
struct GeminiModelAction {
    model: ModelId,
    kind: GeminiActionKind,
}

impl FromStr for GeminiModelAction {
    type Err = ProviderRejection;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        // --- Axum captures everything after `/v1beta/models/`; split the
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
// Gemini response contracts: local structs mirror native Gemini JSON envelopes.
// -----------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct GeminiModel {
    name: String,
    version: &'static str,
    #[serde(rename = "displayName")]
    display_name: &'static str,
    description: &'static str,
    #[serde(rename = "supportedGenerationMethods")]
    supported_generation_methods: Vec<&'static str>,
    #[serde(rename = "inputTokenLimit")]
    input_token_limit: usize,
    #[serde(rename = "outputTokenLimit")]
    output_token_limit: usize,
}

#[derive(Debug, Serialize)]
struct GeminiModelsResponse {
    models: Vec<GeminiModel>,
}

#[derive(Debug, Serialize)]
struct GeminiTextPart {
    text: String,
}

#[derive(Debug, Serialize)]
struct GeminiContentResponse {
    role: &'static str,
    parts: Vec<GeminiTextPart>,
}

#[derive(Debug, Serialize)]
struct GeminiCandidate {
    content: GeminiContentResponse,
    #[serde(rename = "finishReason")]
    finish_reason: &'static str,
    index: usize,
}

#[derive(Debug, Serialize)]
struct GeminiUsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt: usize,
    #[serde(rename = "candidatesTokenCount")]
    candidates: usize,
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

#[derive(Debug, Serialize)]
struct GeminiGenerateResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "modelVersion")]
    model_version: ModelId,
    #[serde(rename = "usageMetadata")]
    usage_metadata: GeminiUsageMetadata,
}

impl GeminiGenerateResponse {
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

#[derive(Debug, Serialize)]
struct GeminiFailureBody {
    code: u16,
    message: String,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct GeminiFailureResponse {
    error: GeminiFailureBody,
}

// -----------------------------------------------------------------------------
// Gemini lowering and streaming: text parts become replay input, then native
// Gemini chunk shapes are emitted for streaming callers.
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
    // --- Gemini's system instruction is textual context only; ELIZA has no
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
            // --- Omitted roles default to user in Gemini examples and become
            // replayed ELIZA turns.
            "user" => user_turns.push(required_text_parts(&content.parts, "contents.parts")?),
            // --- Model turns are prior assistant output from the client side.
            "model" => {}
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

fn gemini_chunks(response: CompatTurnResponse) -> Vec<GeminiGenerateResponse> {
    let mut chunks = Vec::new();
    // --- Intermediate chunks carry prompt usage with zero candidate usage so
    // callers can parse native Gemini bodies incrementally.
    for chunk in stream_chunks(&response.output) {
        chunks.push(GeminiGenerateResponse::new(
            chunk,
            response.model.clone(),
            TokenUsage::new(response.usage.prompt, 0),
        ));
    }
    // --- The final empty chunk carries the complete usage summary.
    chunks.push(GeminiGenerateResponse::new(
        String::new(),
        response.model,
        response.usage,
    ));
    chunks
}

/// Render Gemini stream chunks as SSE data events.
///
/// # Panics
///
/// Panics only if the locally constructed Gemini chunk payload cannot serialize.
fn gemini_sse(chunks: Vec<GeminiGenerateResponse>, delay_ms: u64) -> Response {
    let events = chunks
        .into_iter()
        .map(|chunk| {
            Event::default()
                .json_data(chunk)
                .expect("Gemini stream chunk should serialize")
        })
        .collect::<Vec<_>>();
    sse_response(events, delay_ms)
}

fn error_response(error: ProviderRejection) -> Response {
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

// -----------------------------------------------------------------------------
// Gemini routes: list the configured model and dispatch generate actions encoded
// in the model path.
// -----------------------------------------------------------------------------

pub(crate) async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(error) = require_provider_auth(&headers, &state.config) {
        return error_response(error);
    }

    // --- Native Gemini lists models with the `models/` prefix even though
    // requests accept the bare configured model id.
    Json(GeminiModelsResponse {
        models: vec![GeminiModel {
            name: format!("models/{}", state.config.model),
            version: "1966-doctor",
            display_name: "ELIZA DOCTOR",
            description: "Classic ELIZA DOCTOR script served through Gemini-compatible JSON.",
            supported_generation_methods: vec!["generateContent", "streamGenerateContent"],
            input_token_limit: state.config.max_input_chars.get(),
            output_token_limit: 512,
        }],
    })
    .into_response()
}

pub(crate) async fn model_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(model_action): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    Json(payload): Json<GenerateContentRequest>,
) -> Response {
    if let Err(error) = require_provider_auth(&headers, &state.config) {
        return error_response(error);
    }

    // --- The path chooses both model id and generation mode.
    let action = match model_action.parse::<GeminiModelAction>() {
        Ok(action) => action,
        Err(error) => return error_response(error),
    };

    let request = match lower_generate_content(action.model.clone(), payload) {
        Ok(request) => request,
        Err(error) => return error_response(error),
    };
    let response = match complete_eliza(request, state.config.limits()) {
        Ok(response) => response,
        Err(error) => return error_response(error),
    };

    if action.kind.is_stream() {
        let chunks = gemini_chunks(response);
        // --- Gemini supports SSE through an `alt=sse` query flag; otherwise
        // return the same chunks as a JSON array for simple raw REST clients.
        if query.get("alt").is_some_and(|value| value == "sse") {
            gemini_sse(chunks, state.config.stream_delay_ms)
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
