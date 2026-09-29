//! CPU-bounded execution facade for deterministic speech jobs.

use std::num::NonZeroUsize;
use std::sync::Arc;

use tokio::sync::Semaphore;

use super::core::{AudioFormat, RenderedAudio, SpeechError, SpeechRequest};
use super::{audio, synth};

/// Maximum rendered duration accepted by the compatibility fixture.
const MAX_SECONDS: usize = 120;

/// CPU-bounded facade around deterministic synthesis and encoding.
#[derive(Debug, Clone)]
pub(crate) struct SpeechService {
    /// Permits tied to the host's available CPU parallelism.
    permits: Arc<Semaphore>,
}

impl Default for SpeechService {
    fn default() -> Self {
        let workers = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
        Self {
            permits: Arc::new(Semaphore::new(workers)),
        }
    }
}

impl SpeechService {
    /// Validate, synthesize, and encode one complete request off the async runtime.
    ///
    /// # Errors
    ///
    /// Returns a typed request, worker, synthesis, or encoding failure.
    pub(crate) async fn render(
        &self,
        request: SpeechRequest,
        format: AudioFormat,
        max_chars: NonZeroUsize,
    ) -> Result<RenderedAudio, SpeechError> {
        request.validate(max_chars)?;
        let permit = Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .map_err(|_| SpeechError::Unavailable)?;

        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let sample_rate = request.sample_rate;
            let samples = synth::synthesize(&request, sample_rate as usize * MAX_SECONDS)?;
            let sample_count = samples.len();
            let encoded = audio::EncodedAudio::encode(&samples, sample_rate, format)?;
            Ok(RenderedAudio {
                bytes: encoded.bytes,
                media_type: encoded.media_type,
                sample_count,
                sample_rate,
            })
        })
        .await
        .map_err(|source| SpeechError::Worker { source })?
    }
}
