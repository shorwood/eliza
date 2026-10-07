//! Native Gemini deterministic embeddings adapter.

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use eliza_http::context::ProviderAuth;
use eliza_http::extraction::ExtractionError;
use eliza_modality_embedding as embedding;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::errors::{GeminiError, GeminiFailureResponse, GeminiRejection};
use super::types::{Content, ContentPart};
use crate::context::AppState;

// -----------------------------------------------------------------------------
// Gemini: Adapts native unary and batch embedding contracts.
// -----------------------------------------------------------------------------

/// Current Gemini embedding controls.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct GeminiEmbeddingConfig {
    /// Retrieval title accepted as a no-op.
    title: Option<String>,
    /// Task hint accepted as a no-op.
    task_type: Option<String>,
    /// Requested vector size.
    output_dimensionality: Option<i64>,
    /// Automatic truncation is unsupported when enabled.
    auto_truncate: Option<bool>,
    /// Document OCR is unsupported when enabled.
    document_ocr: Option<bool>,
    /// Audio extraction is unsupported when enabled.
    audio_track_extraction: Option<bool>,
}

impl GeminiEmbeddingConfig {
    /// Validate current controls while retaining documented no-op hints.
    ///
    /// # Errors
    ///
    /// Returns an error when any unsupported control is enabled.
    fn validate(self) -> Result<(), GeminiError> {
        gemini_validation_control(self.auto_truncate, "embedContentConfig.autoTruncate")?;
        gemini_validation_control(self.document_ocr, "embedContentConfig.documentOcr")?;
        gemini_validation_control(
            self.audio_track_extraction,
            "embedContentConfig.audioTrackExtraction",
        )?;
        drop((self.title, self.task_type));
        Ok(())
    }
}

/// Gemini vector object.
#[derive(Debug, Serialize, JsonSchema)]
struct GeminiContentEmbedding {
    /// Embedding values.
    values: Vec<f32>,
}

/// Gemini embedding usage metadata.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct GeminiEmbeddingUsage {
    /// Approximate tokens consumed by request text.
    prompt_token_count: usize,
}

/// Native Gemini single embedding response.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct GeminiEmbedContentResponse {
    /// Generated embedding.
    embedding: GeminiContentEmbedding,
    /// Approximate input usage.
    usage_metadata: GeminiEmbeddingUsage,
}

/// Native Gemini batch embedding response.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct GeminiBatchEmbedContentsResponse {
    /// Vectors in request order.
    embeddings: Vec<GeminiContentEmbedding>,
    /// Approximate input usage across the batch.
    usage_metadata: GeminiEmbeddingUsage,
}

/// One Gemini `embedContent` request, also reused inside batches.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct GeminiEmbedContentRequest {
    /// Fixed model resource name.
    model: Option<String>,
    /// Text content to embed.
    content: Option<Content>,
    /// Legacy task hint accepted as a no-op.
    task_type: Option<String>,
    /// Legacy retrieval title accepted as a no-op.
    title: Option<String>,
    /// Legacy requested vector size.
    output_dimensionality: Option<i64>,
    /// Current embedding controls.
    embed_content_config: Option<GeminiEmbeddingConfig>,
}

/// Native Gemini synchronous batch request.
#[derive(Debug, Deserialize, JsonSchema)]
struct GeminiBatchEmbedContentsRequest {
    /// Ordered embedding requests.
    requests: Option<Vec<GeminiEmbedContentRequest>>,
}

/// Resource name required inside native Gemini embedding bodies.
const GEMINI_MODEL_RESOURCE: &str = "models/fnv-embed";

impl TryFrom<GeminiEmbedContentRequest> for embedding::engine::Input {
    type Error = GeminiError;

    fn try_from(request: GeminiEmbedContentRequest) -> Result<Self, Self::Error> {
        // The body resource must agree with the fixed route model.
        if request.model.as_deref() != Some(GEMINI_MODEL_RESOURCE) {
            return Err(GeminiError::EmbeddingModelRequired);
        }

        let dimensions = gemini_validation_dimensions(
            request.output_dimensionality,
            request.embed_content_config.as_ref(),
        )?;

        if let Some(config) = request.embed_content_config {
            config.validate()?;
        }
        drop((request.title, request.task_type));

        Ok(embedding::engine::Input {
            text: gemini_validation_content_text(request.content)?,
            dimensions,
        })
    }
}

// -----------------------------------------------------------------------------
// GeminiValidation: Resolves dimensions, controls, and text-only content.
// -----------------------------------------------------------------------------

/// Reconcile legacy and nested output dimension fields.
///
/// # Errors
///
/// Returns an error when both fields are present with different values.
fn gemini_validation_dimensions(
    legacy: Option<i64>,
    config: Option<&GeminiEmbeddingConfig>,
) -> Result<i64, GeminiError> {
    let current = config.and_then(|config| config.output_dimensionality);
    match (legacy, current) {
        // Two different sizes cannot describe one response vector.
        (Some(legacy), Some(current)) if legacy != current => {
            Err(GeminiError::ConflictingEmbeddingDimensions)
        }
        (Some(dimensions), _) | (_, Some(dimensions)) => Ok(dimensions),
        (None, None) => Ok(embedding::engine::MODEL_DEFAULT_DIMENSIONS),
    }
}

/// Reject an enabled native control outside the deterministic fixture.
///
/// # Errors
///
/// Returns an unsupported-control error when `enabled` is true.
fn gemini_validation_control(
    enabled: Option<bool>,
    param: &'static str,
) -> Result<(), GeminiError> {
    // False and omitted controls preserve deterministic input unchanged.
    if enabled == Some(true) {
        return Err(GeminiError::UnsupportedEmbeddingControl { param });
    }
    Ok(())
}

/// Join the text parts of one Gemini content object.
///
/// # Errors
///
/// Returns an error when content is missing or contains non-text parts.
fn gemini_validation_content_text(content: Option<Content>) -> Result<String, GeminiError> {
    let content = content.ok_or(GeminiError::MissingEmbeddingContent)?;
    let parts = content
        .parts
        .filter(|parts| !parts.is_empty())
        .ok_or(GeminiError::MissingEmbeddingContent)?;
    let _ = content.role;

    let mut text = Vec::with_capacity(parts.len());
    for part in parts {
        match part {
            ContentPart::Text {
                text: part,
                speech_metadata,
            } => {
                drop(speech_metadata);
                text.push(part);
            }
            // Feature hashing has no representation for structured parts.
            ContentPart::InlineData { .. }
            | ContentPart::FileData { .. }
            | ContentPart::FunctionCall { .. }
            | ContentPart::FunctionResponse { .. }
            | ContentPart::Unsupported { .. } => return Err(GeminiError::EmbeddingTextOnly),
        }
    }
    Ok(text.join("\n"))
}

// -----------------------------------------------------------------------------
// GeminiEmbeddings: Executes requests against the shared engine.
// -----------------------------------------------------------------------------

/// Execute provider-neutral embeddings for native Gemini.
///
/// # Errors
///
/// Returns any validation error raised by the shared embedding engine.
fn gemini_embeddings_complete(
    state: &AppState,
    inputs: Vec<embedding::engine::Input>,
) -> Result<embedding::engine::Response, embedding::engine::Error> {
    embedding::engine::Request { inputs }.complete(state.config.limits.max_input_chars())
}

/// Handle one native Gemini embedding request.
async fn gemini_embeddings_embed(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<GeminiEmbedContentRequest>, JsonRejection>,
) -> Response {
    // Authentication failures use Gemini's native error envelope.
    if let Err(error) = state
        .config
        .authenticate(&headers, ProviderAuth::ApiKey("x-goog-api-key"))
    {
        return GeminiRejection::from_error(&error).into_response();
    }

    // Decode the authenticated native request body.
    let Json(payload) = match payload {
        Ok(payload) => payload,
        // Malformed JSON retains Gemini's extraction error details.
        Err(error) => {
            return GeminiRejection::from_error(&ExtractionError::from(error)).into_response();
        }
    };

    // Lower native fields into one shared input.
    let input: embedding::engine::Input = match payload.try_into() {
        Ok(input) => input,
        // Native validation failures retain Gemini's error schema.
        Err(error) => {
            return GeminiRejection::from_error(&error).into_response();
        }
    };

    // Execute shared bounds and deterministic feature hashing.
    let response = match gemini_embeddings_complete(&state, vec![input]) {
        Ok(response) => response,
        // Shared validation failures use the same native schema.
        Err(error) => {
            return GeminiRejection::from(&error).into_response();
        }
    };

    // Render Gemini's single-vector envelope and usage metadata.
    let prompt_token_count = response.prompt_tokens;
    let embedding = response.embeddings.into_iter().next().unwrap_or_default();
    Json(GeminiEmbedContentResponse {
        embedding: GeminiContentEmbedding { values: embedding },
        usage_metadata: GeminiEmbeddingUsage { prompt_token_count },
    })
    .into_response()
}

/// Handle one native Gemini batch embedding request.
async fn gemini_embeddings_batch_embed(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<GeminiBatchEmbedContentsRequest>, JsonRejection>,
) -> Response {
    // Authentication failures use Gemini's native error envelope.
    if let Err(error) = state
        .config
        .authenticate(&headers, ProviderAuth::ApiKey("x-goog-api-key"))
    {
        return GeminiRejection::from_error(&error).into_response();
    }

    // Decode the authenticated native request body.
    let Json(payload) = match payload {
        Ok(payload) => payload,
        // Malformed JSON retains Gemini's extraction error details.
        Err(error) => {
            return GeminiRejection::from_error(&ExtractionError::from(error)).into_response();
        }
    };

    // A missing collection is distinct from a present but empty batch.
    let Some(requests) = payload.requests else {
        return GeminiRejection::from_error(&GeminiError::MissingEmbeddingRequests).into_response();
    };

    // Lower each native item without disturbing its batch position.
    let lowered = requests.into_iter().map(TryInto::try_into);

    // Collect the batch only when every native item is valid.
    let inputs = match lowered.collect::<Result<Vec<_>, _>>() {
        Ok(inputs) => inputs,
        // Stop at the first invalid native batch item.
        Err(error) => {
            return GeminiRejection::from_error(&error).into_response();
        }
    };

    // Execute shared bounds and deterministic feature hashing.
    let response = match gemini_embeddings_complete(&state, inputs) {
        Ok(response) => response,
        // Shared validation failures use Gemini's native schema.
        Err(error) => {
            return GeminiRejection::from(&error).into_response();
        }
    };

    // Restore Gemini's vector objects without disturbing batch order.
    let embeddings = response
        .embeddings
        .into_iter()
        .map(|values| GeminiContentEmbedding { values })
        .collect();

    // Render ordered vectors and aggregate usage metadata.
    Json(GeminiBatchEmbedContentsResponse {
        embeddings,
        usage_metadata: GeminiEmbeddingUsage {
            prompt_token_count: response.prompt_tokens,
        },
    })
    .into_response()
}

// -----------------------------------------------------------------------------
// Router: Publishes native Gemini embedding endpoints.
// -----------------------------------------------------------------------------

/// Build fixed-model unary and batch embedding routes.
pub(super) fn router() -> ApiRouter<AppState> {
    let router = ApiRouter::new().api_route(
        "/v1beta/models/fnv-embed:embedContent",
        post_with(gemini_embeddings_embed, |operation| {
            operation
                .summary("Gemini embedding")
                .tag("gemini")
                .response::<200, Json<GeminiEmbedContentResponse>>()
                .response::<429, Json<GeminiFailureResponse>>()
                .response::<503, Json<GeminiFailureResponse>>()
                .default_response::<Json<GeminiFailureResponse>>()
        }),
    );
    router.api_route(
        "/v1beta/models/fnv-embed:batchEmbedContents",
        post_with(gemini_embeddings_batch_embed, |operation| {
            operation
                .summary("Gemini batch embeddings")
                .tag("gemini")
                .response::<200, Json<GeminiBatchEmbedContentsResponse>>()
                .response::<429, Json<GeminiFailureResponse>>()
                .response::<503, Json<GeminiFailureResponse>>()
                .default_response::<Json<GeminiFailureResponse>>()
        }),
    )
}
