//! Anthropic model-catalog route.

use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use eliza_http::context::{ProviderAuth, provider_authenticate};

use super::errors::{AnthropicFailureResponse, AnthropicRejection};
use super::types::{ModelDescriptor, ModelListResponse};
use crate::context::AppState;

// -----------------------------------------------------------------------------
// Models: Lists the configured provider model.
// -----------------------------------------------------------------------------

/// List the configured model in Anthropic's native envelope.
async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Authentication failures use the provider's own error envelope.
    if let Err(error) =
        provider_authenticate(&headers, &state.config, ProviderAuth::ApiKey("x-api-key"))
    {
        return AnthropicRejection::from_error(&error).into_response();
    }

    // Render the configured model with Anthropic's fixed catalog metadata.
    let model = state.config.model.clone();
    let descriptor = ModelDescriptor {
        kind: "model",
        id: model.clone(),
        display_name: "ELIZA DOCTOR",
        created_at: "1966-01-01T00:00:00Z",
    };

    // Wrap the descriptor with Anthropic's pagination contract.
    let response = ModelListResponse {
        data: vec![descriptor],
        has_more: false,
        first_id: model.clone(),
        last_id: model,
    };
    Json(response).into_response()
}

// -----------------------------------------------------------------------------
// Router: Publishes the model catalog endpoint.
// -----------------------------------------------------------------------------

/// Build the model-list route.
pub(super) fn router() -> ApiRouter<AppState> {
    ApiRouter::new().api_route(
        "/v1/models",
        get_with(models, |operation| {
            operation
                .summary("Anthropic models")
                .tag("anthropic")
                .response::<200, Json<ModelListResponse>>()
                .default_response::<Json<AnthropicFailureResponse>>()
        }),
    )
}
