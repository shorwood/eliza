//! Assembly of provider-compatible routes.

use std::sync::Arc;

use aide::axum::ApiRouter;
use axum::Router;

use super::anthropic::router::ProviderRoutes as AnthropicRoutes;
use super::context::{AppState, RouteConfig};
use super::docs::Docs;
use super::gemini::router::ProviderRoutes as GeminiRoutes;
use super::health::Health;
use super::ollama::router::ProviderRoutes as OllamaRoutes;
use super::openai::router::ProviderRoutes as OpenAiRoutes;

/// Complete provider-compatible HTTP surface.
pub(crate) struct Routes;

impl Routes {
    /// Build every provider route and generate its `OpenAPI` document.
    pub(crate) fn build(config: RouteConfig) -> Router {
        let router = Health::mount(ApiRouter::new());
        let router = router.nest("/openai", OpenAiRoutes::build());
        let router = router.nest("/anthropic", AnthropicRoutes::build());
        let router = router.nest("/gemini", GeminiRoutes::build());
        let router = router.nest("/ollama", OllamaRoutes::build());
        Docs::finish(router).with_state(AppState {
            config: Arc::new(config),
        })
    }
}
