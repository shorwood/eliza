//! Gemini model catalog route and wire contracts.

use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::Serialize;

use super::context::{AppState, provider_authenticate};
use crate::provider::contracts::ProviderRejection;

// -----------------------------------------------------------------------------
// Gemini: Models the native Gemini model catalog.
// -----------------------------------------------------------------------------

/// Represents `GeminiModel` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct GeminiModel {
    /// Stores the name value owned by this contract.
    name: String,
    /// Stores the version value owned by this contract.
    version: &'static str,
    /// Stores the display name value owned by this contract.
    display_name: &'static str,
    /// Stores the description value owned by this contract.
    description: &'static str,
    /// Stores the supported generation methods value owned by this contract.
    supported_generation_methods: Vec<&'static str>,
    /// Stores the input token limit value owned by this contract.
    input_token_limit: usize,
    /// Stores the output token limit value owned by this contract.
    output_token_limit: usize,
}

/// Represents `GeminiModelsResponse` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
struct GeminiModelsResponse {
    /// Stores the models value owned by this contract.
    models: Vec<GeminiModel>,
}

// -----------------------------------------------------------------------------
// GeminiFailure: Models native Gemini error envelopes.
// -----------------------------------------------------------------------------

/// Represents `GeminiFailureBody` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
struct GeminiFailureBody {
    /// Stores the code value owned by this contract.
    code: u16,
    /// Stores the message value owned by this contract.
    message: String,
    /// Stores the status value owned by this contract.
    status: &'static str,
}

/// Represents `GeminiFailureResponse` state within this module.
#[derive(Debug, Serialize, JsonSchema)]
struct GeminiFailureResponse {
    /// Stores the error value owned by this contract.
    error: GeminiFailureBody,
}

// -----------------------------------------------------------------------------
// GeminiRejection: Renders Gemini failures.
// -----------------------------------------------------------------------------

/// Provider rejection rendered in Gemini's error envelope.
struct GeminiRejection(
    /// Rejection facts rendered by this provider.
    ProviderRejection,
);

impl IntoResponse for GeminiRejection {
    fn into_response(self) -> Response {
        let error = self.0;
        (
            error.status,
            Json(GeminiFailureResponse {
                error: GeminiFailureBody {
                    code: error.status.as_u16(),
                    message: error.message,
                    status: error.gemini_status,
                },
            }),
        )
            .into_response()
    }
}

// -----------------------------------------------------------------------------
// Models: Lists the configured Gemini model.
// -----------------------------------------------------------------------------

/// Performs the models operation for this abstraction.
async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Render authentication failures in Gemini's envelope immediately.
    if let Err(error) = provider_authenticate(&headers, &state.config) {
        return GeminiRejection(error).into_response();
    }

    // Prefix native Gemini model names for discovery responses.
    Json(GeminiModelsResponse {
        models: vec![GeminiModel {
            name: format!("models/{}", state.config.model),
            version: "1966-doctor",
            display_name: "ELIZA DOCTOR",
            description: "Classic ELIZA DOCTOR script served through Gemini-compatible JSON.",
            supported_generation_methods: vec!["generateContent", "streamGenerateContent"],
            input_token_limit: state.config.max_input_chars.get(),
            output_token_limit: 512,
        }],
    })
    .into_response()
}

// -----------------------------------------------------------------------------
// GeminiModels: Mounts the typed model catalog contract into Aide.
// -----------------------------------------------------------------------------

/// Gemini model catalog endpoint and contract.
pub(super) struct GeminiModels;

impl GeminiModels {
    /// Mount the native Gemini model catalog route.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/v1beta/models",
            get_with(models, |operation| {
                operation
                    .summary("Gemini models")
                    .tag("gemini")
                    .response::<200, Json<GeminiModelsResponse>>()
                    .default_response::<Json<GeminiFailureResponse>>()
            }),
        )
    }
}

// -----------------------------------------------------------------------------
