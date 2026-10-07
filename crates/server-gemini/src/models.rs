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
// Models: Lists configured aliases with their engine's fixed metadata.
// -----------------------------------------------------------------------------

/// List aliases once per ID, retaining the first modality's descriptor.
async fn models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let config = &state.config;

    // Authenticate before revealing configured model names.
    if let Err(error) = config.authenticate(&headers, ProviderAuth::ApiKey("x-goog-api-key")) {
        return GeminiRejection::from_error(&error).into_response();
    }

    let input_token_limit = config.limits.max_input_chars().get();
    let mut models: Vec<GeminiModel> = Vec::new();
    for id in config.models.chat.ids(config.chat_model.as_str()) {
        models.push(GeminiModel {
            identity: GeminiModelIdentity {
                name: format!("models/{id}"),
                version: "1966-doctor",
                display_name: "ELIZA DOCTOR",
                description: "Classic ELIZA DOCTOR script served through Gemini-compatible JSON.",
            },
            capabilities: GeminiModelCapabilities {
                supported_generation_methods: vec!["generateContent", "streamGenerateContent"],
                input_token_limit,
                output_token_limit: 512,
            },
        });
    }
    for id in config.models.speech.ids(speech::core::MODEL_ID) {
        let name = format!("models/{id}");
        if models.iter().any(|model| model.identity.name == name) {
            continue;
        }
        models.push(GeminiModel {
            identity: GeminiModelIdentity {
                name,
                version: "flite-kal-8khz",
                display_name: "ELIZA Retro TTS",
                description: "Deterministic retro speech using Flite's bundled diphone voice.",
            },
            capabilities: GeminiModelCapabilities {
                supported_generation_methods: vec!["generateContent", "streamGenerateContent"],
                input_token_limit,
                output_token_limit: 6_000,
            },
        });
    }
    for id in config.models.embeddings.ids(embedding::engine::MODEL_ID) {
        let name = format!("models/{id}");
        if models.iter().any(|model| model.identity.name == name) {
            continue;
        }
        models.push(GeminiModel {
            identity: GeminiModelIdentity {
                name, version: "fnv1a-v1", display_name: "FNV Embed",
                description: "Deterministic FNV-1a feature-hashed text embeddings for compatibility testing.",
            },
            capabilities: GeminiModelCapabilities {
                supported_generation_methods: vec!["embedContent", "batchEmbedContents"],
                input_token_limit,
                output_token_limit: usize::try_from(embedding::engine::MODEL_MAX_DIMENSIONS).unwrap_or(1_024),
            },
        });
    }
    for id in config.models.images.ids(image::generation::MODEL_ID) {
        let name = format!("models/{id}");
        if models.iter().any(|model| model.identity.name == name) {
            continue;
        }
        models.push(GeminiModel {
            identity: GeminiModelIdentity {
                name,
                version: "fnv1a-ca-v1",
                display_name: "ELIZA Retro Image",
                description: "Deterministic cellular-automaton PNG image fixture.",
            },
            capabilities: GeminiModelCapabilities {
                supported_generation_methods: vec!["generateContent", "streamGenerateContent"],
                input_token_limit,
                output_token_limit: 512,
            },
        });
    }
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
                .default_response::<Json<GeminiFailureResponse>>()
        }),
    )
}
