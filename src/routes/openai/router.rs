//! `OpenAI` route-tree assembly.

use aide::axum::ApiRouter;

use super::chat_completions::OpenAiChatCompletions;
use super::models::OpenAiModels;
use super::responses::OpenAiResponses;
use crate::routes::context::AppState;

/// Build the complete `OpenAI`-compatible route tree for mounting.
pub(crate) fn mount() -> ApiRouter<AppState> {
    let router = OpenAiModels::mount(ApiRouter::new());
    let router = OpenAiChatCompletions::mount(router);
    OpenAiResponses::mount(router)
}
