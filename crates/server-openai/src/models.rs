//! `OpenAI` model catalog route.

use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use eliza_http::context::ProviderAuth;
use eliza_modality_embedding as embedding;
use eliza_modality_image as image;
use eliza_modality_speech as speech;

use super::errors::{OpenAiFailureResponse, OpenAiRejection};
use super::types::{ModelDescriptor, ModelListResponse};
use crate::context::AppState;

// -----------------------------------------------------------------------------
// Models: Lists the configured provider models.
// -----------------------------------------------------------------------------

/// List aliases for each local modality, retaining the first descriptor per ID.
async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Authenticate before revealing configured model names.
    if let Err(error) = state.config.authenticate(&headers, ProviderAuth::Bearer) {
        return OpenAiRejection::from_error(&error).into_response();
    }

    // Collect chat and speech aliases in their configured order.
    let config = &state.config;
    let aliases = &config.models;
    let chat_and_speech = aliases
        .chat
        .ids(config.chat_model.as_str())
        .chain(aliases.speech.ids(speech::core::MODEL_ID));

    // Include the remaining engines in the complete catalog.
    let names = chat_and_speech
        .chain(config.models.embeddings.ids(embedding::engine::MODEL_ID))
        .chain(config.models.images.ids(image::generation::MODEL_ID));

    // Render the first descriptor for each unique name.
    let mut data: Vec<ModelDescriptor> = Vec::new();
    for id in names {
        // Advertise each name once, even when assigned to several modalities.
        if data.iter().any(|model| model.id == id) {
            continue;
        }
        data.push(ModelDescriptor {
            id,
            object: "model",
            created: 0,
            owned_by: "eliza",
        });
    }
    Json(ModelListResponse {
        object: "list",
        data,
    })
    .into_response()
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
