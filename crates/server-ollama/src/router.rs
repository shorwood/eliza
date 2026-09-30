//! Ollama route-tree assembly.

use aide::axum::ApiRouter;
use eliza_http::context::AppState;

use super::embeddings::OllamaEmbeddings;
use super::routes::Ollama;

/// Build the complete Ollama-compatible route tree for mounting.
pub fn mount() -> ApiRouter<AppState> {
    let router = Ollama::mount(ApiRouter::new());
    OllamaEmbeddings::mount(router)
}
