//! Anthropic route-tree assembly.

use aide::axum::ApiRouter;

use super::messages::AnthropicMessages;
use super::models::AnthropicModels;
use crate::routes::context::AppState;

/// Complete Anthropic-compatible route tree.
pub(crate) struct ProviderRoutes;

impl ProviderRoutes {
    /// Build the Anthropic-compatible route tree.
    pub(crate) fn build() -> ApiRouter<AppState> {
        let router = AnthropicModels::mount(ApiRouter::new());
        AnthropicMessages::mount(router)
    }
}
