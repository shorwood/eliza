//! Assembly of provider-compatible routes.

use aide::axum::ApiRouter;
use axum::Router;
use eliza_http::context::{AppState, RouteConfig};

use crate::docs::Docs;
use crate::health::Health;

/// Complete provider-compatible HTTP surface.
pub(super) struct Routes;

impl Routes {
    /// Build every provider route and generate its `OpenAPI` document.
    pub(super) fn build(config: RouteConfig) -> Router {
        let router = Health::mount(ApiRouter::new());
        let router = router.nest("/openai", eliza_server_openai::router::mount());
        let router = router.nest("/anthropic", eliza_server_anthropic::router::mount());
        let router = router.nest("/gemini", eliza_server_gemini::router::mount());
        let router = router.nest("/ollama", eliza_server_ollama::router::mount());
        Docs::finish(router).with_state(AppState::from(config))
    }
}
