//! `OpenAI` route-tree assembly and state.

use std::sync::Arc;

use aide::axum::ApiRouter;
use eliza_http::context::RouteConfig;
use eliza_modality_speech::service::Service as SpeechService;

use super::context::{AppState, SpeechState};
use super::{chat_completions, compatibility, embeddings, images, models, responses, speech};

/// Configured OpenAI-compatible route tree.
pub struct Routes {
    /// Validated route behavior shared by provider endpoints.
    config: Arc<RouteConfig>,
    /// Bounded speech executor used by the audio endpoint.
    speech: SpeechService,
}

impl Routes {
    /// Capture the dependencies required by OpenAI-compatible endpoints.
    #[must_use]
    pub fn new(config: Arc<RouteConfig>, speech: SpeechService) -> Self {
        Self { config, speech }
    }

    /// Build reusable OpenAI-compatible alias routes.
    pub fn compatibility(config: Arc<RouteConfig>) -> ApiRouter {
        compatibility::router().with_state(AppState { config })
    }

    /// Build the native OpenAI-compatible router.
    pub fn into_router(self) -> ApiRouter {
        let router = models::router().merge(chat_completions::router());
        let router = router.merge(embeddings::router());
        let router = router.merge(images::router());
        let router = router.merge(responses::router());
        let router = router.with_state(AppState {
            config: Arc::clone(&self.config),
        });
        let speech = speech::router().with_state(SpeechState {
            config: self.config,
            speech: self.speech,
        });
        router.merge(speech)
    }
}
