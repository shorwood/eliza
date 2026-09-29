//! Anthropic route-tree assembly.

use aide::axum::ApiRouter;

use super::messages::AnthropicMessages;
use super::models::AnthropicModels;
use crate::routes::context::AppState;

/// Build the complete Anthropic-compatible route tree for mounting.
pub(crate) fn mount() -> ApiRouter<AppState> {
    let router = AnthropicModels::mount(ApiRouter::new());
    AnthropicMessages::mount(router)
}
