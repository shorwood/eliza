//! State owned by OpenAI-compatible routes.

use std::sync::Arc;

use eliza_http::context::RouteConfig;
use eliza_modality_speech::service::Service as SpeechService;

// -----------------------------------------------------------------------------
// AppState: Shares state used by non-speech endpoints.
// -----------------------------------------------------------------------------

/// State shared by non-speech OpenAI-compatible handlers.
#[derive(Clone)]
pub(super) struct AppState {
    /// Validated route behavior.
    pub(super) config: Arc<RouteConfig>,
}

// -----------------------------------------------------------------------------
// SpeechState: Shares state used by the speech endpoint.
// -----------------------------------------------------------------------------

/// State used only by the speech endpoint.
#[derive(Clone)]
pub(super) struct SpeechState {
    /// Validated route behavior.
    pub(super) config: Arc<RouteConfig>,
    /// Bounded speech executor.
    pub(super) service: SpeechService,
}
