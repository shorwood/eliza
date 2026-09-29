//! Assembly of provider-compatible routes.

use std::sync::Arc;

use aide::axum::ApiRouter;
use axum::Router;

use super::context::{AppState, RouteConfig};
use super::docs::Docs;
use super::health::Health;

/// Complete provider-compatible HTTP surface.
pub(crate) struct Routes;

impl Routes {
    /// Build every provider route and generate its `OpenAPI` document.
    pub(crate) fn build(config: RouteConfig) -> Router {
        let router = Health::mount(ApiRouter::new());
        let router = router.nest("/openai", super::openai::router::mount());
        let router = router.nest("/anthropic", super::anthropic::router::mount());
        let router = router.nest("/gemini", super::gemini::router::mount());
        let router = router.nest("/ollama", super::ollama::router::mount());
        Docs::finish(router).with_state(AppState {
            config: Arc::new(config),
        })
    }
}
