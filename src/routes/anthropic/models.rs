//! Anthropic model catalog route.

use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};

use super::super::context::{AppState, ProviderAuth, provider_authenticate};
use super::errors::{AnthropicFailureResponse, AnthropicRejection};
use super::types::{ModelDescriptor, ModelListResponse};

/// List the configured model in Anthropic's native envelope.
async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Authentication failures use the provider's own error envelope.
    if let Err(error) =
        provider_authenticate(&headers, &state.config, ProviderAuth::ApiKey("x-api-key"))
    {
        return AnthropicRejection::from_error(&error).into_response();
    }

    let model = state.config.model.clone();
    Json(ModelListResponse {
        data: vec![ModelDescriptor {
            kind: "model",
            id: model.clone(),
            display_name: "ELIZA DOCTOR",
            created_at: "1966-01-01T00:00:00Z",
        }],
        has_more: false,
        first_id: model.clone(),
        last_id: model,
    })
    .into_response()
}

/// Anthropic model-list endpoint.
pub(super) struct AnthropicModels;

impl AnthropicModels {
    /// Mount the model-list route.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
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
}
