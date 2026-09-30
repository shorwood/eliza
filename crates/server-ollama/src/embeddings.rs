//! Ollama-compatible deterministic embeddings adapter.

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use eliza_http::context::ProviderAuth;
use eliza_http::extraction::ExtractionError;
use eliza_http::model::ModelId;
use eliza_modality_embedding as embedding;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::errors::{OllamaError, OllamaFailureResponse, OllamaRejection};
use crate::context::AppState;

// -----------------------------------------------------------------------------
// OllamaEmbedding: Adapts Ollama's embedding contract.
// -----------------------------------------------------------------------------

/// Ollama text input shapes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
enum OllamaEmbeddingInput {
    /// One text value.
    Text(
        /// Text to embed.
        #[schemars(title = "", description = "")]
        String,
    ),
    /// Ordered text batch.
    Batch(
        /// Text values to embed.
        #[schemars(title = "", description = "")]
        Vec<String>,
    ),
    /// Any remaining JSON shape.
    Unsupported(
        /// Unrecognized input retained only for classification.
        #[schemars(title = "", description = "")]
        serde_json::Value,
    ),
}

impl OllamaEmbeddingInput {
    /// Lower supported text shapes.
    ///
    /// # Errors
    ///
    /// Returns an error when the input is neither text nor a text batch.
    fn into_texts(self) -> Result<Vec<String>, OllamaError> {
        match self {
            Self::Text(text) => Ok(vec![text]),
            Self::Batch(texts) => Ok(texts),
            Self::Unsupported(value) => {
                drop(value);
                Err(OllamaError::UnsupportedEmbeddingInput)
            }
        }
    }
}

/// Ollama embedding request.
#[derive(Debug, Deserialize, JsonSchema)]
struct OllamaEmbeddingRequest {
    /// Fixed local embedding model.
    model: ModelId,
    /// Text or ordered text batch.
    input: OllamaEmbeddingInput,
    /// Optional vector size.
    dimensions: Option<i64>,
}

/// Ollama embedding response.
#[derive(Debug, Serialize, JsonSchema)]
struct OllamaEmbeddingResponse {
    /// Fixed model that produced the vectors.
    model: ModelId,
    /// Ordered vectors.
    embeddings: Vec<Vec<f32>>,
    /// Deterministic total duration in nanoseconds.
    total_duration: usize,
    /// Deterministic model-load duration in nanoseconds.
    load_duration: usize,
    /// Approximate input token count.
    prompt_eval_count: usize,
}

impl OllamaEmbeddingResponse {
    /// Restore the provider model around one provider-neutral result.
    fn from_embedding(model: ModelId, response: embedding::engine::Response) -> Self {
        Self {
            model,
            embeddings: response.embeddings,
            total_duration: 0,
            load_duration: 0,
            prompt_eval_count: response.prompt_tokens,
        }
    }
}

/// Authenticate, validate, embed, and render one request.
async fn handle(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<OllamaEmbeddingRequest>, JsonRejection>,
) -> Response {
    // Authentication failures use Ollama's native error envelope.
    if let Err(error) = state.config.authenticate(&headers, ProviderAuth::Bearer) {
        return OllamaRejection::from_error(&error).into_response();
    }

    // Decode the authenticated request body.
    let Json(payload) = match payload {
        Ok(payload) => payload,
        // Malformed JSON retains Ollama's extraction error details.
        Err(error) => {
            return OllamaRejection::from_error(&ExtractionError::from(error)).into_response();
        }
    };

    // Model routing is a provider concern, not an engine concern.
    if payload.model.as_str() != embedding::engine::MODEL_ID {
        return OllamaRejection::from_error(&OllamaError::EmbeddingModelRequired).into_response();
    }

    // Lower Ollama's supported text input shapes.
    let texts = match payload.input.into_texts() {
        Ok(texts) => texts,
        // Unsupported input shapes retain Ollama's error schema.
        Err(error) => {
            return OllamaRejection::from_error(&error).into_response();
        }
    };

    // Build one provider-neutral batch with a shared vector size.
    let dimensions = payload
        .dimensions
        .unwrap_or(embedding::engine::MODEL_DEFAULT_DIMENSIONS);
    let inputs = texts
        .into_iter()
        .map(|text| embedding::engine::Input { text, dimensions })
        .collect();

    // Assemble the provider-neutral embedding batch.
    let request = embedding::engine::Request { inputs };

    // Execute the shared engine and restore Ollama's envelope.
    let response = match request.complete(state.config.limits.max_input_chars()) {
        Ok(response) => response,
        // Shared validation failures must retain Ollama's error envelope.
        Err(error) => {
            return OllamaRejection::from(&error).into_response();
        }
    };

    // Restore Ollama's response envelope after provider-neutral execution.
    Json(OllamaEmbeddingResponse::from_embedding(
        payload.model,
        response,
    ))
    .into_response()
}

// -----------------------------------------------------------------------------
// Router: Publishes the native embeddings endpoint.
// -----------------------------------------------------------------------------

/// Build the Ollama embeddings route.
pub(super) fn router() -> ApiRouter<AppState> {
    ApiRouter::new().api_route(
        "/api/embed",
        post_with(handle, |operation| {
            operation
                .summary("Ollama embeddings")
                .tag("ollama")
                .response::<200, Json<OllamaEmbeddingResponse>>()
                .default_response::<Json<OllamaFailureResponse>>()
        }),
    )
}
