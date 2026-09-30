//! `OpenAI` route-tree assembly.

use aide::axum::ApiRouter;
use eliza_http::context::AppState;

use super::chat_completions::OpenAiChatCompletions;
use super::embeddings::OpenAiEmbeddings;
use super::models::OpenAiModels;
use super::responses::OpenAiResponses;
use super::speech::OpenAiSpeech;

/// Build the complete `OpenAI`-compatible route tree for mounting.
pub fn mount() -> ApiRouter<AppState> {
    let router = OpenAiModels::mount(ApiRouter::new());
    let router = OpenAiChatCompletions::mount(router);
    let router = OpenAiEmbeddings::mount(router);
    let router = OpenAiResponses::mount(router);
    OpenAiSpeech::mount(router)
}
