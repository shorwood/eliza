//! Gemini model catalog route.

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

use super::errors::{GeminiFailureResponse, GeminiRejection};
use super::types::{
    GeminiModel, GeminiModelCapabilities, GeminiModelIdentity, GeminiModelListResponse,
};
use crate::context::AppState;

// -----------------------------------------------------------------------------
// Models: Lists the configured provider models.
// -----------------------------------------------------------------------------

/// List the configured chat model and fixed modality models.
async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Authentication failures use the provider's own error envelope.
    if let Err(error) = state
        .config
        .authenticate(&headers, ProviderAuth::ApiKey("x-goog-api-key"))
    {
        return GeminiRejection::from_error(&error).into_response();
    }

    // Render the configured chat model with Gemini's fixed capability metadata.
    let identity = GeminiModelIdentity {
        name: format!("models/{}", state.config.chat_model),
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
    if state.config.chat_model.as_str() != speech::core::MODEL_ID {
        models.push(GeminiModel {
            identity: GeminiModelIdentity {
                name: format!("models/{}", speech::core::MODEL_ID),
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
    if state.config.chat_model.as_str() != embedding::engine::MODEL_ID {
        models.push(GeminiModel {
            identity: GeminiModelIdentity {
                name: format!("models/{}", embedding::engine::MODEL_ID),
                version: "fnv1a-v1",
                display_name: "FNV Embed",
                description: "Deterministic FNV-1a feature-hashed text embeddings for compatibility testing.",
            },
            capabilities: GeminiModelCapabilities {
                supported_generation_methods: vec!["embedContent", "batchEmbedContents"],
                input_token_limit: state.config.limits.max_input_chars().get(),
                output_token_limit: usize::try_from(embedding::engine::MODEL_MAX_DIMENSIONS)
                    .unwrap_or(1_024),
            },
        });
    }
    if state.config.chat_model.as_str() != image::generation::MODEL_ID {
        models.push(GeminiModel {
            identity: GeminiModelIdentity {
                name: format!("models/{}", image::generation::MODEL_ID),
                version: "fnv1a-ca-v1",
                display_name: "ELIZA Retro Image",
                description: "Deterministic cellular-automaton PNG image fixture.",
            },
            capabilities: GeminiModelCapabilities {
                supported_generation_methods: vec!["generateContent", "streamGenerateContent"],
                input_token_limit: state.config.limits.max_input_chars().get(),
                output_token_limit: 512,
            },
        });
    }

    // Return the model through Gemini's catalog envelope.
    Json(GeminiModelListResponse { models }).into_response()
}

// -----------------------------------------------------------------------------
// Router: Publishes the model catalog endpoint.
// -----------------------------------------------------------------------------

/// Build the native Gemini model catalog route.
pub(super) fn router() -> ApiRouter<AppState> {
    ApiRouter::new().api_route(
        "/v1beta/models",
        get_with(models, |operation| {
            operation
                .summary("Gemini models")
                .tag("gemini")
                .response::<200, Json<GeminiModelListResponse>>()
                .response::<429, Json<GeminiFailureResponse>>()
                .response::<503, Json<GeminiFailureResponse>>()
                .default_response::<Json<GeminiFailureResponse>>()
        }),
    )
}
