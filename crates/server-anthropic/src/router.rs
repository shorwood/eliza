//! Anthropic route-tree assembly.

use aide::axum::ApiRouter;
use eliza_http::context::AppState;

use super::messages::AnthropicMessages;
use super::models::AnthropicModels;

/// Build the complete Anthropic-compatible route tree for mounting.
pub fn mount() -> ApiRouter<AppState> {
    let router = AnthropicModels::mount(ApiRouter::new());
    AnthropicMessages::mount(router)
}
