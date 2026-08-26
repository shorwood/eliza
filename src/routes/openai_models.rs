//! `OpenAI` model catalog route and wire contracts.

use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::Serialize;

use super::context::{AppState, provider_authenticate};
use crate::provider::contracts::{ModelId, ProviderRejection};

// -----------------------------------------------------------------------------
// Model: Models the OpenAI model catalog.
// -----------------------------------------------------------------------------

/// Represents `ModelDescriptor` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
struct ModelDescriptor {
    /// Stores the id value owned by this contract.
    id: ModelId,
    /// Stores the object value owned by this contract.
    object: &'static str,
    /// Stores the created value owned by this contract.
    created: u64,
    /// Stores the owned by value owned by this contract.
    owned_by: &'static str,
}

/// Represents `ModelListResponse` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
struct ModelListResponse {
    /// Stores the object value owned by this contract.
    object: &'static str,
    /// Stores the data value owned by this contract.
    data: Vec<ModelDescriptor>,
}

// -----------------------------------------------------------------------------
// OpenAiFailure: Models OpenAI error envelopes.
// -----------------------------------------------------------------------------

/// Represents `OpenAiFailureBody` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
struct OpenAiFailureBody {
    /// Stores the message value owned by this contract.
    message: String,
    /// Stores the error type value owned by this contract.
    #[serde(rename = "type")]
    error_type: &'static str,
    /// Stores the param value owned by this contract.
    param: Option<&'static str>,
    /// Stores the code value owned by this contract.
    code: Option<&'static str>,
}

/// Represents `OpenAiFailureResponse` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
struct OpenAiFailureResponse {
    /// Stores the error value owned by this contract.
    error: OpenAiFailureBody,
}

// -----------------------------------------------------------------------------
// OpenAiRejection: Renders OpenAI failures.
// -----------------------------------------------------------------------------

/// Provider rejection rendered in `OpenAI`'s error envelope.
struct OpenAiRejection(
    /// Rejection facts rendered by this provider.
    ProviderRejection,
);

impl IntoResponse for OpenAiRejection {
    fn into_response(self) -> Response {
        let error = self.0;
        (
            error.status,
            Json(OpenAiFailureResponse {
                error: OpenAiFailureBody {
                    message: error.message,
                    error_type: error.openai_type,
                    param: error.param,
                    code: None,
                },
            }),
        )
            .into_response()
    }
}

// -----------------------------------------------------------------------------
// OpenAiRouteModels: Authenticates and renders compatible responses.
// -----------------------------------------------------------------------------

/// Performs the models operation for this abstraction.
async fn open_ai_route_models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Render authentication failures in OpenAI's envelope immediately.
    if let Err(error) = provider_authenticate(&headers, &state.config) {
        return OpenAiRejection(error).into_response();
    }

    // The server exposes exactly one configured model id.
    Json(ModelListResponse {
        object: "list",
        data: vec![ModelDescriptor {
            id: state.config.model.clone(),
            object: "model",
            created: 0,
            owned_by: "eliza",
        }],
    })
    .into_response()
}

// -----------------------------------------------------------------------------
// OpenAiModels: Mounts the typed model catalog contract into Aide.
// -----------------------------------------------------------------------------

/// `OpenAI` model catalog endpoint and contract.
pub(super) struct OpenAiModels;

impl OpenAiModels {
    /// Mount the `OpenAI` model catalog route.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/v1/models",
            get_with(open_ai_route_models, |operation| {
                operation
                    .summary("OpenAI models")
                    .tag("openai")
                    .response::<200, Json<ModelListResponse>>()
                    .default_response::<Json<OpenAiFailureResponse>>()
            }),
        )
    }
}
