//! `OpenAI` model catalog route.

use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use eliza_http::context::ProviderAuth;
use eliza_http::model::ModelId;
use eliza_modality_embedding as embedding;
use eliza_modality_speech as speech;

use super::errors::{OpenAiFailureResponse, OpenAiRejection};
use super::types::{ModelDescriptor, ModelListResponse};
use crate::context::AppState;

// -----------------------------------------------------------------------------
// ModelId: Converts built-in identifiers into transport values.
// -----------------------------------------------------------------------------

/// Convert one built-in modality identifier into its transport representation.
///
/// # Panics
///
/// Panics only if a modality's built-in identifier becomes invalid.
fn model_id(value: &'static str) -> ModelId {
    value
        .parse()
        .expect("built-in modality model ids should be valid")
}

// -----------------------------------------------------------------------------
// Models: Lists the configured provider models.
// -----------------------------------------------------------------------------

/// List the configured model in `OpenAI`'s native envelope.
async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Authentication failures use the provider's own error envelope.
    if let Err(error) = state.config.authenticate(&headers, ProviderAuth::Bearer) {
        return OpenAiRejection::from_error(&error).into_response();
    }

    // Render the configured model with OpenAI's fixed catalog metadata.
    let descriptor = ModelDescriptor {
        id: state.config.model.clone(),
        object: "model",
        created: 0,
        owned_by: "eliza",
    };

    // Include the fixed speech capability unless it is already configured.
    let mut data = vec![descriptor];
    if state.config.model.as_str() != speech::core::MODEL_ID {
        data.push(ModelDescriptor {
            id: model_id(speech::core::MODEL_ID),
            object: "model",
            created: 0,
            owned_by: "eliza",
        });
    }

    // Include the fixed embedding capability unless it is already configured.
    if state.config.model.as_str() != embedding::engine::MODEL_ID {
        data.push(ModelDescriptor {
            id: model_id(embedding::engine::MODEL_ID),
            object: "model",
            created: 0,
            owned_by: "eliza",
        });
    }

    // Wrap every descriptor with OpenAI's list contract.
    let response = ModelListResponse {
        object: "list",
        data,
    };
    Json(response).into_response()
}

// -----------------------------------------------------------------------------
// Router: Publishes the model catalog endpoint.
// -----------------------------------------------------------------------------

/// Build the `OpenAI` model catalog route.
pub(super) fn router() -> ApiRouter<AppState> {
    ApiRouter::new().api_route(
        "/v1/models",
        get_with(models, |operation| {
            operation
                .summary("OpenAI models")
                .tag("openai")
                .response::<200, Json<ModelListResponse>>()
                .default_response::<Json<OpenAiFailureResponse>>()
        }),
    )
}
