//! State owned by native Gemini routes.

use std::sync::Arc;

use eliza_http::context::RouteConfig;
use eliza_modality_speech::service::Service as SpeechService;

/// State shared by native Gemini handlers.
#[derive(Clone)]
pub(super) struct AppState {
    /// Validated route behavior.
    pub(super) config: Arc<RouteConfig>,
    /// Bounded speech executor used by audio generation.
    pub(super) speech: SpeechService,
}
