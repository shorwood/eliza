//! Ollama model catalog adapter.

use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use eliza_http::context::{ProviderAuth, RouteConfig};
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
// ModelCatalogWithEmbeddings: Builds the native catalog.
// -----------------------------------------------------------------------------

/// Advertise chat and embedding aliases with their engine's fixed metadata.
fn model_catalog_with_embeddings(config: &RouteConfig) -> ModelListResponse {
    let mut models: Vec<ModelDescriptor> = config
        .models
        .chat
        .ids(config.chat_model.as_str())
        .map(|model| ModelDescriptor {
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
        })
        .collect();
    for model in config.models.embeddings.ids(embedding::engine::MODEL_ID) {
        if models.iter().any(|descriptor| descriptor.model == model) {
            continue;
        }
        models.push(ModelDescriptor {
            name: model.clone(),
            model,
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

/// List configured models in Ollama's native envelope.
async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Authenticate before revealing configured model names.
    if let Err(error) = state.config.authenticate(&headers, ProviderAuth::Bearer) {
        return OllamaRejection::from_error(&error).into_response();
    }
    Json(model_catalog_with_embeddings(&state.config)).into_response()
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
