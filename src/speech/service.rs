//! CPU-bounded execution facade for deterministic speech jobs.

use std::num::NonZeroUsize;
use std::sync::Arc;

use tokio::sync::{Semaphore, mpsc};

use super::core::{AudioFormat, RenderedAudio, SpeechRequest};
use super::errors::SpeechError;
use super::{audio, synth};

// -----------------------------------------------------------------------------
// Speech: Bounds and executes audio work off the async runtime.
// -----------------------------------------------------------------------------

/// One message produced by an incremental speech job.
pub(crate) enum SpeechStreamItem {
    /// Encoded audio ready for immediate delivery.
    Chunk(
        /// Provider-ready bytes.
        Vec<u8>,
    ),
    /// Successful completion with the exact synthesized sample count.
    Done {
        /// Number of mono PCM samples represented by the stream.
        sample_count: usize,
    },
    /// Failure after streaming had already started.
    Failed(
        /// Engine or encoder failure terminating the stream.
        SpeechError,
    ),
}

/// Incremental encoded audio and its fixed response metadata.
pub(crate) struct SpeechStream {
    /// Provider-safe MIME type.
    pub(crate) media_type: String,
    /// Samples per second in the uncompressed signal.
    pub(crate) sample_rate: u32,
    /// Bounded receiver that applies backpressure to synthesis.
    pub(crate) items: mpsc::Receiver<SpeechStreamItem>,
}

/// Maximum rendered duration accepted by the compatibility fixture.
const SPEECH_MAX_SECONDS: usize = 120;

/// Maximum number of speech jobs allowed to retain audio buffers concurrently.
const SPEECH_WORKERS: usize = 2;

/// Encoded chunks retained between a synthesis worker and its HTTP consumer.
const SPEECH_STREAM_BUFFER: usize = 4;

/// CPU-bounded facade around deterministic synthesis and encoding.
#[derive(Debug, Clone)]
pub(crate) struct SpeechService {
    /// Fixed permits keep memory use independent of host CPU count.
    permits: Arc<Semaphore>,
}

impl Default for SpeechService {
    fn default() -> Self {
        Self {
            permits: Arc::new(Semaphore::new(SPEECH_WORKERS)),
        }
    }
}

impl SpeechService {
    /// Run one streaming encoder until completion or client disconnect.
    ///
    /// # Errors
    ///
    /// Returns a synthesis, encoding, or disconnected-consumer failure.
    fn stream_audio(
        request: &SpeechRequest,
        mut encoder: audio::StreamingEncoder,
        sender: &mpsc::Sender<SpeechStreamItem>,
    ) -> Result<(), SpeechError> {
        let send = |bytes: Vec<u8>| {
            // Some codecs need more PCM before they can emit a frame.
            if bytes.is_empty() {
                return Ok(());
            }
            sender
                .blocking_send(SpeechStreamItem::Chunk(bytes))
                .map_err(|_| SpeechError::Unavailable)
        };
        if let Some(header) = encoder.begin() {
            send(header)?;
        }
        let maximum_samples = request.sample_rate as usize * SPEECH_MAX_SECONDS;
        let sample_count = synth::synthesize_streaming(request, maximum_samples, |samples| {
            send(encoder.push(samples)?)
        })?;
        send(encoder.finish()?)?;
        sender
            .blocking_send(SpeechStreamItem::Done { sample_count })
            .map_err(|_| SpeechError::Unavailable)
    }

    /// Report a worker failure unless the HTTP consumer already disconnected.
    async fn report_failure(sender: &mpsc::Sender<SpeechStreamItem>, error: SpeechError) {
        match sender.send(SpeechStreamItem::Failed(error)).await {
            Ok(()) | Err(_) => {}
        }
    }

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
            let samples = synth::synthesize(&request, sample_rate as usize * SPEECH_MAX_SECONDS)?;
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

    /// Start incremental synthesis and encoding on a bounded worker.
    ///
    /// # Errors
    ///
    /// Returns request validation or worker-pool failures before response delivery begins.
    pub(crate) async fn stream(
        &self,
        request: SpeechRequest,
        format: AudioFormat,
        max_chars: NonZeroUsize,
    ) -> Result<SpeechStream, SpeechError> {
        request.validate(max_chars)?;
        let permit = Arc::clone(&self.permits)
            .acquire_owned()
            .await
            .map_err(|_| SpeechError::Unavailable)?;
        let sample_rate = request.sample_rate;
        let encoder = audio::StreamingEncoder::new(format, sample_rate);
        let media_type = encoder.media_type();
        let (sender, items) = mpsc::channel(SPEECH_STREAM_BUFFER);
        let panic_sender = sender.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            Self::stream_audio(&request, encoder, &sender)
        });
        tokio::spawn(async move {
            match worker.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => Self::report_failure(&panic_sender, error).await,
                Err(source) => {
                    Self::report_failure(&panic_sender, SpeechError::Worker { source }).await;
                }
            }
        });
        Ok(SpeechStream {
            media_type,
            sample_rate,
            items,
        })
    }
}

// -----------------------------------------------------------------------------
// Tests: Locks down bounded, progressive worker delivery.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::speech::core::SpeechSegment;

    /// Streaming PCM arrives in multiple chunks and terminates with an exact count.
    ///
    /// # Panics
    ///
    /// Panics if the worker limit, stream metadata, chunking, or accounting regresses.
    #[tokio::test]
    async fn streams_pcm_with_bounded_worker_count() {
        let service = SpeechService::default();
        assert_eq!(service.permits.available_permits(), SPEECH_WORKERS);
        let request = SpeechRequest {
            segments: vec![SpeechSegment {
                text: "Hello from a genuinely progressive speech stream.".to_owned(),
                voice: "Kore".to_owned(),
                style: String::new(),
                speed: 1.0,
                pause_after_ms: 0,
            }],
            sample_rate: 8_000,
        };
        let max_chars = NonZeroUsize::MAX;
        let mut stream = service
            .stream(request, AudioFormat::Pcm, max_chars)
            .await
            .expect("valid speech request should start");
        assert_eq!(stream.media_type, "audio/pcm;rate=8000");

        let mut chunks = 0;
        let mut byte_count = 0;
        let sample_count = loop {
            match stream.items.recv().await {
                Some(SpeechStreamItem::Chunk(bytes)) => {
                    chunks += 1;
                    byte_count += bytes.len();
                }
                Some(SpeechStreamItem::Done { sample_count }) => break sample_count,
                Some(SpeechStreamItem::Failed(error)) => panic!("speech stream failed: {error}"),
                None => panic!("speech stream closed without a terminal item"),
            }
        };
        assert!(chunks > 1);
        assert_eq!(byte_count, sample_count * size_of::<i16>());
        assert!(stream.items.recv().await.is_none());
        assert_eq!(service.permits.available_permits(), SPEECH_WORKERS);
    }
}
