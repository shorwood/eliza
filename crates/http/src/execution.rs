//! Retains an optional request guard inside native workers after HTTP cancellation.

use std::sync::{Arc, LazyLock};

use tokio::sync::Semaphore;

use crate::problem::NativeError;

/// Shared across every provider's image generation and analysis.
static IMAGE_WORKERS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(2)));

/// Run image computation away from async workers, rejecting excess work.
///
/// Text-only work stays inline. An admitted blocking job retains its permit even
/// if the HTTP future is canceled, so disconnects cannot bypass the CPU bound.
///
/// # Errors
/// Returns service-unavailable when image capacity is full or a worker fails.
pub async fn image_work<T: Send + 'static>(
    needs_worker: bool,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, NativeError> {
    if !needs_worker {
        return Ok(work());
    }
    let permit = Arc::clone(&IMAGE_WORKERS)
        .try_acquire_owned()
        .map_err(|_| NativeError::unavailable())?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .map_err(|_| NativeError::unavailable())
}

// -----------------------------------------------------------------------------
// Execution: Transfers opaque owned permits into transport-neutral workers.
// -----------------------------------------------------------------------------

/// Request extension retaining owned work through actual completion.
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
