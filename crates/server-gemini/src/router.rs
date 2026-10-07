//! Gemini route-tree assembly and state.

use std::sync::Arc;

use aide::axum::ApiRouter;
use eliza_http::context::RouteConfig;
use eliza_modality_embedding as embedding;
use eliza_modality_speech::service::Service as SpeechService;

use super::context::AppState;
use super::{embeddings, generate, models};

/// Configured Gemini route tree.
pub struct Routes {
    /// Dependencies shared by every native Gemini endpoint.
    state: AppState,
}

impl Routes {
    /// Capture the dependencies required by Gemini endpoints.
    #[must_use]
    pub fn new(config: Arc<RouteConfig>, speech: SpeechService) -> Self {
        Self {
            state: AppState { config, speech },
        }
    }

    /// Build the native Gemini-compatible router.
    pub fn into_router(self) -> ApiRouter {
        models::router()
            .merge(embeddings::router(
                self.state
                    .config
                    .models
                    .embeddings
                    .ids(embedding::engine::MODEL_ID),
            ))
            .merge(generate::router())
            .with_state(self.state)
    }
}
