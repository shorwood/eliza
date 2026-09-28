//! Anthropic model catalog.

use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use super::context::{AppState, ProviderAuth, provider_authenticate};

/// List the configured model in Anthropic's native envelope.
async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Reject invalid provider credentials before exposing the catalog.
    if let Err(error) =
        provider_authenticate(&headers, &state.config, ProviderAuth::ApiKey("x-api-key"))
    {
        return error.anthropic_response();
    }

    let model = state.config.model.as_str();
    Json(json!({
        "data":[{
            "type":"model",
            "id":model,
            "display_name":"ELIZA DOCTOR",
            "created_at":"1966-01-01T00:00:00Z"
        }],
        "has_more":false,
        "first_id":model,
        "last_id":model
    }))
    .into_response()
}

/// Anthropic model-list endpoint.
pub(super) struct AnthropicModels;

impl AnthropicModels {
    /// Mount the model-list route.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/anthropic/v1/models",
            get_with(models, |operation| {
                operation
                    .summary("Anthropic models")
                    .tag("anthropic")
                    .response::<200, Json<Value>>()
                    .default_response::<Json<Value>>()
            }),
        )
    }
}
