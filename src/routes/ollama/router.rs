//! Ollama route-tree assembly.

use aide::axum::ApiRouter;

use super::routes::Ollama;
use crate::routes::context::AppState;

/// Complete Ollama-compatible route tree.
pub(crate) struct ProviderRoutes;

impl ProviderRoutes {
    /// Build the Ollama-compatible route tree.
    pub(crate) fn build() -> ApiRouter<AppState> {
        Ollama::mount(ApiRouter::new())
    }
}
