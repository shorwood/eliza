//! Shared provider HTTP boundary types and helpers.

use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::header;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::{StreamExt, stream};
use serde::Serialize;

use super::errors::EncodingError;

// -----------------------------------------------------------------------------
// StreamChunks: Splits provider output at readable boundaries.
// -----------------------------------------------------------------------------

/// Split provider output into readable streaming chunks.
#[must_use]
pub fn stream_chunks(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        current.push(character);

        // Retain non-boundary characters in the current chunk.
        if !character.is_whitespace() {
            continue;
        }
        chunks.push(std::mem::take(&mut current));
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

// -----------------------------------------------------------------------------
// JsonEvent: Encodes one typed server-sent event.
// -----------------------------------------------------------------------------

/// Encode a typed provider record as one SSE data event.
///
/// # Errors
///
/// Returns an internal rejection when response serialization fails.
pub fn json_event<T: Serialize>(value: &T) -> Result<Event, EncodingError> {
    Event::default()
        .json_data(value)
        .map_err(|source| EncodingError::Sse {
            detail: source.to_string(),
        })
}

// -----------------------------------------------------------------------------
// SseResponse: Owns an optionally paced server-sent event response.
// -----------------------------------------------------------------------------

/// SSE response with delivery pacing owned by its response type.
pub struct SseResponse {
    /// Events delivered in insertion order.
    events: Vec<Event>,
    /// Delay inserted before each event.
    delay_ms: u64,
}

impl IntoResponse for SseResponse {
    fn into_response(self) -> Response {
        let base = stream::iter(
            self.events
                .into_iter()
                .map(Ok::<_, std::convert::Infallible>),
        );

        // If no pacing is requested, return the base stream directly.
        if self.delay_ms == 0 {
            Sse::new(base)
                .keep_alive(KeepAlive::default())
                .into_response()
        } else {
            // Insert a pacing delay before each event.
            let delay_ms = self.delay_ms;
            Sse::new(base.then(move |event| async move {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                event
            }))
            .keep_alive(KeepAlive::default())
            .into_response()
        }
    }
}

// -----------------------------------------------------------------------------
// SseEvents: Owns server-sent events before pacing is attached.
// -----------------------------------------------------------------------------

/// Owned SSE events ready for optional paced delivery.
#[derive(derive_more::From)]
pub struct SseEvents(
    /// Events delivered in insertion order.
    Vec<Event>,
);

impl SseEvents {
    /// Attach optional pacing before rendering these events.
    #[must_use]
    pub fn with_delay(self, delay_ms: u64) -> SseResponse {
        SseResponse {
            events: self.0,
            delay_ms,
        }
    }
}

// -----------------------------------------------------------------------------
// NdjsonResponse: Owns an optionally paced newline-delimited JSON response.
// -----------------------------------------------------------------------------

/// Newline-delimited JSON records with optional delivery pacing.
pub struct NdjsonResponse {
    /// Records delivered in insertion order.
    records: Vec<Bytes>,
    /// Delay inserted before each record.
    delay_ms: u64,
}

impl NdjsonResponse {
    /// Build a paced NDJSON response from ordered JSON records.
    ///
    /// # Errors
    ///
    /// Returns an internal rejection when a typed record cannot be serialized.
    pub fn new(records: Vec<impl Serialize>, delay_ms: u64) -> Result<Self, EncodingError> {
        let records = records
            .into_iter()
            .map(|record| {
                let mut line =
                    serde_json::to_vec(&record).map_err(|source| EncodingError::Ndjson {
                        detail: source.to_string(),
                    })?;
                line.push(b'\n');
                Ok(Bytes::from(line))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { records, delay_ms })
    }
}

impl IntoResponse for NdjsonResponse {
    fn into_response(self) -> Response {
        let delay_ms = self.delay_ms;
        let body = Body::from_stream(stream::iter(self.records).then(move |chunk| async move {
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            Ok::<_, std::convert::Infallible>(chunk)
        }));
        ([(header::CONTENT_TYPE, "application/x-ndjson")], body).into_response()
    }
}

/// Return the current Unix timestamp, or zero before the Unix epoch.
#[must_use]
pub fn unix_timestamp() -> u64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(_) => 0,
    }
}
