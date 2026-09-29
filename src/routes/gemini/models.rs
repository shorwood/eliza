//! Gemini model catalog route.

use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};

use super::super::context::{AppState, ProviderAuth, provider_authenticate};
use super::errors::{GeminiFailureResponse, GeminiRejection};
use super::types::{
    GeminiModel, GeminiModelCapabilities, GeminiModelIdentity, GeminiModelListResponse,
};
use crate::speech::core as speech;

/// List the configured model in Gemini's native envelope.
async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Authentication failures use the provider's own error envelope.
    if let Err(error) = provider_authenticate(
        &headers,
        &state.config,
        ProviderAuth::ApiKey("x-goog-api-key"),
    ) {
        return GeminiRejection::from_error(&error).into_response();
    }

    // Render the configured model with Gemini's fixed capability metadata.
    let identity = GeminiModelIdentity {
        name: format!("models/{}", state.config.model),
        version: "1966-doctor",
        display_name: "ELIZA DOCTOR",
        description: "Classic ELIZA DOCTOR script served through Gemini-compatible JSON.",
    };

    // Advertise the generation modes and configured input bound.
    let capabilities = GeminiModelCapabilities {
        supported_generation_methods: vec!["generateContent", "streamGenerateContent"],
        input_token_limit: state.config.limits.max_input_chars().get(),
        output_token_limit: 512,
    };

    // Keep identity and capabilities explicit in the typed wire contract.
    let model = GeminiModel {
        identity,
        capabilities,
    };

    let mut models = vec![model];
    if state.config.model.as_str() != speech::MODEL_ID {
        models.push(GeminiModel {
            identity: GeminiModelIdentity {
                name: format!("models/{}", speech::MODEL_ID),
                version: "flite-kal-8khz",
                display_name: "ELIZA Retro TTS",
                description: "Deterministic retro speech using Flite's bundled diphone voice.",
            },
            capabilities: GeminiModelCapabilities {
                supported_generation_methods: vec!["generateContent", "streamGenerateContent"],
                input_token_limit: state.config.limits.max_input_chars().get(),
                output_token_limit: 6_000,
            },
        });
    }

    // Return the model through Gemini's catalog envelope.
    Json(GeminiModelListResponse { models }).into_response()
}

/// Gemini model catalog endpoint.
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
                    .response::<200, Json<GeminiModelListResponse>>()
                    .default_response::<Json<GeminiFailureResponse>>()
            }),
        )
    }
}
