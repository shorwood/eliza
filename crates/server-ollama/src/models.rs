//! Ollama model catalog adapter.

use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use eliza_http::context::ProviderAuth;
use eliza_http::model::ModelId;
use eliza_modality_embedding as embedding;

use super::errors::{OllamaFailureResponse, OllamaRejection};
use super::types::{ModelDescriptor, ModelDetails, ModelListResponse};
use crate::context::AppState;

// -----------------------------------------------------------------------------
// CreatedAt: Defines deterministic model timestamps.
// -----------------------------------------------------------------------------

/// Stable timestamp used by the timeless ELIZA algorithm.
const CREATED_AT: &str = "1966-01-01T00:00:00Z";

// -----------------------------------------------------------------------------
// EmbeddingModelId: Converts the built-in embedding identifier.
// -----------------------------------------------------------------------------

/// Construct the fixed embedding model ID for provider response types.
///
/// # Panics
///
/// Panics only if the modality's built-in identifier becomes invalid.
fn embedding_model_id() -> ModelId {
    embedding::engine::MODEL_ID
        .parse()
        .expect("the built-in embedding model id should be valid")
}

// -----------------------------------------------------------------------------
// ModelCatalogWithEmbedding: Builds the native catalog.
// -----------------------------------------------------------------------------

/// Build the model catalog for the configured ELIZA identity.
fn model_catalog_with_embedding(model: ModelId) -> ModelListResponse {
    let include_embedding = model.as_str() != embedding::engine::MODEL_ID;
    let mut models = vec![ModelDescriptor {
        name: model.clone(),
        model,
        modified_at: CREATED_AT,
        size: 0,
        digest: "eliza-1966",
        details: ModelDetails {
            parent_model: "",
            format: "eliza",
            family: "eliza",
            families: vec!["eliza"],
            parameter_size: "DOCTOR",
            quantization_level: "none",
        },
    }];
    if include_embedding {
        let embedding_model = embedding_model_id();
        models.push(ModelDescriptor {
            name: embedding_model.clone(),
            model: embedding_model,
            modified_at: CREATED_AT,
            size: 0,
            digest: embedding::engine::MODEL_ID,
            details: ModelDetails {
                parent_model: "",
                format: "eliza",
                family: "eliza-embed",
                families: vec!["eliza-embed"],
                parameter_size: "1024D",
                quantization_level: "none",
            },
        });
    }
    ModelListResponse { models }
}

// -----------------------------------------------------------------------------
// Models: Handles the native model catalog request.
// -----------------------------------------------------------------------------

/// List the configured models in Ollama's native envelope.
async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Authentication failures use Ollama's native error envelope.
    if let Err(error) = state.config.authenticate(&headers, ProviderAuth::Bearer) {
        return OllamaRejection::from_error(&error).into_response();
    }
    Json(model_catalog_with_embedding(state.config.model.clone())).into_response()
}

// -----------------------------------------------------------------------------
// Router: Publishes the native model catalog endpoint.
// -----------------------------------------------------------------------------

/// Build the native Ollama model-list endpoint.
pub(super) fn router() -> ApiRouter<AppState> {
    ApiRouter::new().api_route(
        "/api/tags",
        get_with(models, |operation| {
            operation
                .summary("Ollama models")
                .tag("ollama")
                .response::<200, Json<ModelListResponse>>()
                .default_response::<Json<OllamaFailureResponse>>()
        }),
    )
}
