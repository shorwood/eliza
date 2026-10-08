//! Assembly of provider-compatible routes.

use std::sync::Arc;

use axum::Router;
use eliza_http::context::RouteConfig;
use eliza_modality_speech::service::Service as SpeechService;

use crate::docs::ApiDocs as _;
use crate::health;

/// Configured complete provider-compatible route tree.
pub(super) struct Routes {
    /// Validated behavior shared by provider adapters.
    config: RouteConfig,
}

impl Routes {
    /// Capture validated route behavior for composition.
    pub(super) fn for_config(config: RouteConfig) -> Self {
        Self { config }
    }

    /// Build every provider route and generate its `OpenAPI` document.
    pub(super) fn into_router(self) -> Router {
        let config = Arc::new(self.config);
        let speech = SpeechService::with_worker_limit(config.speech_workers);
        let router = health::router();
        let openai = eliza_server_openai::router::Routes::new(Arc::clone(&config), speech.clone())
            .into_router();
        let anthropic =
            eliza_server_anthropic::router::Routes::from(Arc::clone(&config)).into_router();
        let openai_compat = eliza_server_openai::router::Routes::compatibility(Arc::clone(&config));
        let gemini = eliza_server_gemini::router::Routes::new(Arc::clone(&config), speech)
            .into_router()
            .nest("/v1beta/openai", openai_compat);
        let ollama = eliza_server_ollama::router::Routes::from(config).into_router();
        let router = router.nest("/openai", openai);
        let router = router.nest("/anthropic", anthropic);
        let router = router.nest("/gemini", gemini);
        let router = router.nest("/ollama", ollama);
        router.finish_docs()
    }
}
