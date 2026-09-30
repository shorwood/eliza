//! Anthropic route-tree assembly and state.

use std::sync::Arc;

use aide::axum::ApiRouter;
use eliza_http::context::RouteConfig;

use super::context::AppState;
use super::{messages, models};

/// Configured Anthropic-compatible route tree.
pub struct Routes {
    /// Dependencies shared by every Anthropic-compatible endpoint.
    state: AppState,
}

impl Routes {
    /// Build the Anthropic-compatible router.
    pub fn into_router(self) -> ApiRouter {
        models::router()
            .merge(messages::router())
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
