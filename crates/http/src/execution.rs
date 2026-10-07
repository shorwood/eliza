//! Retains hosted admission inside native workers after HTTP cancellation.

use std::sync::Arc;

// -----------------------------------------------------------------------------
// Execution: Transfers opaque owned permits into transport-neutral workers.
// -----------------------------------------------------------------------------

/// Request extension retaining hosted work admission through actual completion.
#[derive(Clone)]
pub struct Execution {
    /// Owned admission; its concrete transport policy is private to the origin.
    guard: Arc<dyn Send + Sync>,
}

impl Execution {
    /// Retain a concrete owned permit bundle without coupling engines to HTTP policy.
    #[must_use]
    pub fn new(guard: impl Send + Sync + 'static) -> Self {
        Self {
            guard: Arc::new(guard),
        }
    }

    /// Transfer another reference into an actual native worker.
    #[must_use]
    pub fn guard(&self) -> Arc<dyn Send + Sync> {
        Arc::clone(&self.guard)
    }
}
