//! CPU-bounded execution facade for deterministic speech jobs.

use std::num::NonZeroUsize;
use std::sync::Arc;

use tokio::sync::{Semaphore, mpsc};

use super::core::{Audio, AudioFormat, Request};
use super::errors::Error;
use super::{audio, synth};

// -----------------------------------------------------------------------------
// Stream: Delivers an incremental encoded speech response.
// -----------------------------------------------------------------------------

/// One message produced by an incremental speech job.
pub enum StreamItem {
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
        Error,
    ),
}

/// Incremental encoded audio and its fixed response metadata.
pub struct Stream {
    /// Provider-safe MIME type.
    pub media_type: String,
    /// Samples per second in the uncompressed signal.
    pub sample_rate: u32,
    /// Bounded receiver that applies backpressure to synthesis.
    pub items: mpsc::Receiver<StreamItem>,
}

// -----------------------------------------------------------------------------
// Service: Bounds and executes audio work off the async runtime.
// -----------------------------------------------------------------------------

/// Maximum rendered duration accepted by the compatibility fixture.
const SERVICE_MAX_SECONDS: usize = 120;

/// Default number of speech jobs allowed to retain audio buffers concurrently.
const SERVICE_WORKERS: usize = 2;

/// Encoded chunks retained between a synthesis worker and its HTTP consumer.
const SERVICE_STREAM_BUFFER: usize = 4;

/// CPU-bounded facade around deterministic synthesis and encoding.
#[derive(Debug, Clone)]
pub struct Service {
    /// Fixed permits keep memory use independent of host CPU count.
    permits: Arc<Semaphore>,
}

impl Default for Service {
    fn default() -> Self {
        Self {
            permits: Arc::new(Semaphore::new(SERVICE_WORKERS)),
        }
    }
}

impl Service {
    /// Bound concurrent synthesis jobs across all clones of this service.
    ///
    /// # Panics
    /// Panics if the limit exceeds Tokio's maximum semaphore permits.
    #[must_use]
    pub fn with_worker_limit(workers: NonZeroUsize) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(workers.get())),
        }
    }

    /// Run one streaming encoder until completion or client disconnect.
    ///
    /// # Errors
    ///
    /// Returns a synthesis, encoding, or disconnected-consumer failure.
    fn stream_audio(
        request: &Request,
        mut encoder: audio::StreamingEncoder,
        sender: &mpsc::Sender<StreamItem>,
    ) -> Result<(), Error> {
        let send = |bytes: Vec<u8>| {
            // Some codecs need more PCM before they can emit a frame.
            if bytes.is_empty() {
                return Ok(());
            }
            sender
                .blocking_send(StreamItem::Chunk(bytes))
                .map_err(|_| Error::Unavailable)
        };
        if let Some(header) = encoder.begin() {
            send(header)?;
        }
        let maximum_samples = request.sample_rate as usize * SERVICE_MAX_SECONDS;
        let sample_count = synth::synthesize_streaming(request, maximum_samples, |samples| {
            send(encoder.push(samples)?)
        })?;
        send(encoder.finish()?)?;
        sender
            .blocking_send(StreamItem::Done { sample_count })
            .map_err(|_| Error::Unavailable)
    }

    /// Report a worker failure unless the HTTP consumer already disconnected.
    async fn report_failure(sender: &mpsc::Sender<StreamItem>, error: Error) {
        match sender.send(StreamItem::Failed(error)).await {
            Ok(()) | Err(_) => {}
        }
    }

    /// Run hosted work with owned admission retained until actual completion.
    ///
    /// # Errors
    /// Returns validation, saturation, synthesis or encoding failures.
    pub async fn render_guarded(
        &self,
        request: Request,
        format: AudioFormat,
        max_chars: NonZeroUsize,
        guard: Option<Arc<dyn Send + Sync>>,
    ) -> Result<Audio, Error> {
        request.validate(max_chars)?;
        let capacity = Arc::clone(&self.permits);
        let permit = if guard.is_some() {
            capacity
                .try_acquire_owned()
                .map_err(|_| Error::Overloaded)?
        } else {
            capacity
                .acquire_owned()
                .await
                .map_err(|_| Error::Unavailable)?
        };

        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            let _permit = permit;
            let sample_rate = request.sample_rate;
            let samples = synth::synthesize(&request, sample_rate as usize * SERVICE_MAX_SECONDS)?;
            let sample_count = samples.len();
            let encoded = audio::EncodedAudio::encode(&samples, sample_rate, format)?;
            Ok(Audio {
                bytes: encoded.bytes,
                media_type: encoded.media_type,
                sample_count,
                sample_rate,
            })
        })
        .await
        .map_err(|source| Error::Worker {
            detail: source.to_string(),
        })?
    }

    /// Validate, synthesize, and encode one complete request off the async runtime.
    ///
    /// # Errors
    ///
    /// Returns a typed request, worker, synthesis, or encoding failure.
    pub async fn render(
        &self,
        request: Request,
        format: AudioFormat,
        max_chars: NonZeroUsize,
    ) -> Result<Audio, Error> {
        self.render_guarded(request, format, max_chars, None).await
    }

    /// Run hosted work with owned admission retained until actual completion.
    ///
    /// # Errors
    /// Returns validation, saturation, synthesis or encoding failures.
    pub async fn stream_guarded(
        &self,
        request: Request,
        format: AudioFormat,
        max_chars: NonZeroUsize,
        guard: Option<Arc<dyn Send + Sync>>,
    ) -> Result<Stream, Error> {
        request.validate(max_chars)?;
        let capacity = Arc::clone(&self.permits);
        let permit = if guard.is_some() {
            capacity
                .try_acquire_owned()
                .map_err(|_| Error::Overloaded)?
        } else {
            capacity
                .acquire_owned()
                .await
                .map_err(|_| Error::Unavailable)?
        };
        let sample_rate = request.sample_rate;
        let encoder = audio::StreamingEncoder::new(format, sample_rate);
        let media_type = encoder.media_type();
        let (sender, items) = mpsc::channel(SERVICE_STREAM_BUFFER);
        let panic_sender = sender.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let _guard = guard;
            let _permit = permit;
            Self::stream_audio(&request, encoder, &sender)
        });
        tokio::spawn(async move {
            match worker.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => Self::report_failure(&panic_sender, error).await,
                Err(source) => {
                    let error = Error::Worker {
                        detail: source.to_string(),
                    };
                    Self::report_failure(&panic_sender, error).await;
                }
            }
        });
        Ok(Stream {
            media_type,
            sample_rate,
            items,
        })
    }

    /// Start incremental synthesis and encoding on a bounded worker.
    ///
    /// # Errors
    ///
    /// Returns request validation or worker-pool failures before response delivery begins.
    pub async fn stream(
        &self,
        request: Request,
        format: AudioFormat,
        max_chars: NonZeroUsize,
    ) -> Result<Stream, Error> {
        self.stream_guarded(request, format, max_chars, None).await
    }
}

// -----------------------------------------------------------------------------
// Tests: Locks down bounded, progressive worker delivery.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::future::{Future as _, poll_fn};
    use std::task::Poll;

    use super::*;
    use crate::core::Segment;

    /// Capacity distinct from the production default of two.
    const TEST_WORKERS: usize = 4;

    /// Hosted synthesis fails immediately when its native worker slot is occupied.
    ///
    /// # Panics
    /// Panics if guarded work queues or ignores the shared speech limit.
    #[tokio::test]
    async fn guarded_speech_never_queues() {
        let service = Service::with_worker_limit(NonZeroUsize::MIN);
        let occupied = Arc::clone(&service.permits).try_acquire_owned().unwrap();
        let request = Request {
            segments: vec![Segment {
                text: "Hello".into(),
                voice: "Kore".into(),
                style: String::new(),
                speed: 1.0,
                pause_after_ms: 0,
            }],
            sample_rate: 8_000,
        };
        let result = service
            .render_guarded(
                request,
                AudioFormat::Pcm,
                NonZeroUsize::MAX,
                Some(Arc::new(())),
            )
            .await;
        assert!(matches!(result, Err(Error::Overloaded)));
        drop(occupied);
        let remaining = service.permits.available_permits();
        assert_eq!(remaining, 1);
    }

    /// A saturated service also blocks jobs submitted through another provider's clone.
    ///
    /// # Panics
    /// Panics if the configured bound is not shared or queued rendering bypasses it.
    #[tokio::test]
    async fn configured_limit_is_shared_and_blocks_rendering() {
        let count = TEST_WORKERS;
        let service = Service::with_worker_limit(NonZeroUsize::new(count).unwrap());
        let provider = service.clone();
        let capacity = Arc::clone(&service.permits);
        let permits = capacity
            .acquire_many_owned(u32::try_from(count).unwrap())
            .await
            .unwrap();
        let request = Request {
            segments: vec![Segment {
                text: "Hello.".to_owned(),
                voice: "Kore".to_owned(),
                style: String::new(),
                speed: 1.0,
                pause_after_ms: 0,
            }],
            sample_rate: 8_000,
        };
        let mut render = Box::pin(provider.render(request, AudioFormat::Pcm, NonZeroUsize::MAX));
        let is_queued =
            poll_fn(|context| Poll::Ready(render.as_mut().poll(context).is_pending())).await;
        assert!(is_queued);
        drop(permits);
        let audio = render
            .await
            .expect("released capacity should allow rendering");
        assert!(audio.sample_count > 0);
        let available = service.permits.available_permits();
        assert_eq!(available, count);
    }

    /// Streaming PCM arrives in multiple chunks and terminates with an exact count.
    ///
    /// # Panics
    ///
    /// Panics if the worker limit, stream metadata, chunking, or accounting regresses.
    #[tokio::test]
    async fn streams_pcm_with_bounded_worker_count() {
        let service = Service::default();
        let available_permits = service.permits.available_permits();
        assert_eq!(available_permits, SERVICE_WORKERS);
        let request = Request {
            segments: vec![Segment {
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
                Some(StreamItem::Chunk(bytes)) => {
                    chunks += 1;
                    byte_count += bytes.len();
                }
                Some(StreamItem::Done { sample_count }) => break sample_count,
                Some(StreamItem::Failed(error)) => panic!("speech stream failed: {error}"),
                None => panic!("speech stream closed without a terminal item"),
            }
        };
        assert!(chunks > 1);
        assert_eq!(byte_count, sample_count * size_of::<i16>());
        assert!(stream.items.recv().await.is_none());
        let available_permits = service.permits.available_permits();
        assert_eq!(available_permits, SERVICE_WORKERS);
    }
}
