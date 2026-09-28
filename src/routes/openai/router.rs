//! `OpenAI` route-tree assembly.

use aide::axum::ApiRouter;

use super::chat_completions::OpenAiChatCompletions;
use super::models::OpenAiModels;
use super::responses::OpenAiResponses;
use crate::routes::context::AppState;

/// Complete `OpenAI`-compatible route tree.
pub(crate) struct ProviderRoutes;

impl ProviderRoutes {
    /// Build the `OpenAI`-compatible route tree.
    pub(crate) fn build() -> ApiRouter<AppState> {
        let router = OpenAiModels::mount(ApiRouter::new());
        let router = OpenAiChatCompletions::mount(router);
        OpenAiResponses::mount(router)
    }
}
