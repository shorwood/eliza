//! State owned by Ollama-compatible routes.

use std::sync::Arc;

use eliza_http::context::RouteConfig;

/// State shared by Ollama-compatible handlers.
#[derive(Clone)]
pub(super) struct AppState {
    /// Validated route behavior.
    pub(super) config: Arc<RouteConfig>,
}
