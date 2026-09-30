//! Gemini route-tree assembly.

use aide::axum::ApiRouter;

use super::content::Route as ContentRoute;
use super::embeddings::GeminiEmbeddings;
use super::models::GeminiModels;
use super::openai::OpenAiAlias;
use crate::routes::context::AppState;

/// Build the complete Gemini-compatible route tree for mounting.
pub(crate) fn mount() -> ApiRouter<AppState> {
    let router = GeminiModels::mount(ApiRouter::new());
    let router = GeminiEmbeddings::mount(router);
    let router = ContentRoute::mount(router);
    OpenAiAlias::mount(router)
}
