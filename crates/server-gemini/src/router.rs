//! Gemini route-tree assembly.

use aide::axum::ApiRouter;
use eliza_http::context::AppState;
use eliza_server_openai::alias::GeminiAlias;

use super::content::Route as ContentRoute;
use super::embeddings::GeminiEmbeddings;
use super::models::GeminiModels;

/// Build the complete Gemini-compatible route tree for mounting.
pub fn mount() -> ApiRouter<AppState> {
    let router = GeminiModels::mount(ApiRouter::new());
    let router = GeminiEmbeddings::mount(router);
    let router = ContentRoute::mount(router);
    GeminiAlias::mount(router)
}
