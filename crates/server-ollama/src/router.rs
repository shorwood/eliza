//! Ollama route-tree assembly and state.

use std::sync::Arc;

use aide::axum::ApiRouter;
use eliza_http::context::RouteConfig;

use super::context::AppState;
use super::{chat, embeddings, models};

/// Configured Ollama-compatible route tree.
pub struct Routes {
    /// Dependencies shared by every Ollama-compatible endpoint.
    state: AppState,
}

impl Routes {
    /// Build the Ollama-compatible router.
    pub fn into_router(self) -> ApiRouter {
        models::router()
            .merge(chat::router())
            .merge(embeddings::router())
            .with_state(self.state)
    }
}

impl From<Arc<RouteConfig>> for Routes {
    fn from(config: Arc<RouteConfig>) -> Self {
        Self {
            state: AppState { config },
        }
    }
}
