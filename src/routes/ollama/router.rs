//! Ollama route-tree assembly.

use aide::axum::ApiRouter;

use super::routes::Ollama;
use crate::routes::context::AppState;

/// Build the complete Ollama-compatible route tree for mounting.
pub(crate) fn mount() -> ApiRouter<AppState> {
    Ollama::mount(ApiRouter::new())
}
