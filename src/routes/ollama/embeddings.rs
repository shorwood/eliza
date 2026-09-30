//! Ollama-compatible deterministic embeddings adapter.

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::super::context::{AppState, ProviderAuth, provider_authenticate};
use super::errors::{OllamaError, OllamaFailureResponse, OllamaRejection};
use crate::embedding::{
    EMBEDDING_DEFAULT_DIMENSIONS, EmbeddingInput, EmbeddingRequest, EmbeddingResponse,
};
use crate::routes::errors::ExtractionError;
use crate::types::model::ModelId;

// -----------------------------------------------------------------------------
// Ollama: Adapts Ollama's unary and batch embedding contract.
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

impl From<EmbeddingResponse> for OllamaEmbeddingResponse {
    fn from(response: EmbeddingResponse) -> Self {
        Self {
            model: response.model,
            embeddings: response.embeddings,
            total_duration: 0,
            load_duration: 0,
            prompt_eval_count: response.prompt_tokens,
        }
    }
}

/// Ollama embeddings endpoint.
pub(super) struct OllamaEmbeddings;

impl OllamaEmbeddings {
    /// Mount the Ollama embeddings route.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/api/embed",
            post_with(Self::handle, |operation| {
                operation
                    .summary("Ollama embeddings")
                    .tag("ollama")
                    .response::<200, Json<OllamaEmbeddingResponse>>()
                    .default_response::<Json<OllamaFailureResponse>>()
            }),
        )
    }

    /// Authenticate, validate, embed, and render one request.
    #[expect(
        clippy::unused_async,
        reason = "Axum handlers must return a future even when their work is synchronous"
    )]
    async fn handle(
        State(state): State<AppState>,
        headers: HeaderMap,
        payload: Result<Json<OllamaEmbeddingRequest>, JsonRejection>,
    ) -> Response {
        // Authentication failures use Ollama's native error envelope.
        if let Err(error) = provider_authenticate(&headers, &state.config, ProviderAuth::Bearer) {
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

        // Lower Ollama's supported text input shapes.
        let texts = match payload.input.into_texts() {
            Ok(texts) => texts,
            // Unsupported input shapes retain Ollama's error schema.
            Err(error) => {
                return OllamaRejection::from_error(&error).into_response();
            }
        };

        // Build one provider-neutral batch with a shared vector size.
        let dimensions = payload.dimensions.unwrap_or(EMBEDDING_DEFAULT_DIMENSIONS);
        let inputs = texts
            .into_iter()
            .map(|text| EmbeddingInput { text, dimensions })
            .collect();

        // Attach the fixed model selection to the lowered inputs.
        let request = EmbeddingRequest {
            model: payload.model,
            inputs,
        };

        // Execute the shared engine and restore Ollama's envelope.
        match request.complete(state.config.limits.max_input_chars()) {
            Ok(response) => Json(OllamaEmbeddingResponse::from(response)).into_response(),
            Err(error) => OllamaRejection::from_error(&error).into_response(),
        }
    }
}
