//! Shared provider HTTP boundary types and helpers.

use std::time::Duration;

use axum::Json;
use axum::body::{Body, Bytes};
use axum::http::{StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::{StreamExt, stream};
use serde_json::{Value, json};

// -----------------------------------------------------------------------------
// ProviderRejection: Stores failures rendered by provider adapters.
// -----------------------------------------------------------------------------

/// Provider-neutral rejection facts rendered by each adapter.
#[derive(Debug, Clone)]
pub(crate) struct ProviderRejection {
    /// HTTP status returned to the client.
    pub(crate) status: StatusCode,
    /// OpenAI-compatible error type.
    pub(crate) openai_type: &'static str,
    /// Gemini-compatible status.
    pub(crate) gemini_status: &'static str,
    /// Human-readable failure message.
    pub(crate) message: String,
    /// Invalid provider parameter, when applicable.
    pub(crate) param: Option<&'static str>,
}

impl ProviderRejection {
    /// Build a provider validation rejection for malformed input.
    pub(crate) fn invalid(param: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            openai_type: "invalid_request_error",
            gemini_status: "INVALID_ARGUMENT",
            message: message.into(),
            param: Some(param),
        }
    }

    /// Build a provider validation rejection for unsupported features.
    pub(crate) fn unsupported(param: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            openai_type: "unsupported_request_error",
            gemini_status: "INVALID_ARGUMENT",
            message: message.into(),
            param: Some(param),
        }
    }

    /// Build a provider authentication rejection.
    pub(crate) fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            openai_type: "authentication_error",
            gemini_status: "UNAUTHENTICATED",
            message: "authentication failed".to_owned(),
            param: None,
        }
    }

    /// Build a provider validation rejection for configured size limits.
    pub(super) fn too_large(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            openai_type: "request_too_large",
            gemini_status: "RESOURCE_EXHAUSTED",
            message: message.into(),
            param: None,
        }
    }

    /// Render this rejection using the `OpenAI` error envelope.
    pub(crate) fn openai_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": {
                    "message": self.message,
                    "type": self.openai_type,
                    "param": self.param,
                    "code": null
                }
            })),
        )
            .into_response()
    }

    /// Render this rejection using the Anthropic error envelope.
    pub(crate) fn anthropic_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "type": "error",
                "error": { "type": self.openai_type, "message": self.message }
            })),
        )
            .into_response()
    }

    /// Render this rejection using the native Gemini error envelope.
    pub(crate) fn gemini_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": {
                    "code": self.status.as_u16(),
                    "message": self.message,
                    "status": self.gemini_status
                }
            })),
        )
            .into_response()
    }

    /// Render this rejection using the Ollama error envelope.
    pub(crate) fn ollama_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

// -----------------------------------------------------------------------------
// TextArrayKind: Describes provider-specific typed text arrays.
// -----------------------------------------------------------------------------

/// Provider-specific typed text array shape.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TextArrayKind {
    /// OpenAI/Gemini-style parts.
    Parts,
    /// `OpenAI` Responses input parts.
    ResponsesParts,
    /// Anthropic-style blocks.
    Blocks,
}

impl TextArrayKind {
    /// Return whether a wire type represents text for this provider shape.
    fn is_text_type(self, item_type: Option<&str>) -> bool {
        match self {
            Self::Parts | Self::Blocks => item_type == Some("text"),
            Self::ResponsesParts => {
                matches!(item_type, Some("input_text" | "output_text" | "text"))
            }
        }
    }

    /// Return the rejection for a non-text typed item.
    const fn unsupported_message(self) -> &'static str {
        match self {
            Self::Parts | Self::ResponsesParts => "only text content parts are supported",
            Self::Blocks => "only text content blocks are supported",
        }
    }

    /// Return the rejection for a typed item without text.
    const fn missing_text_message(self) -> &'static str {
        match self {
            Self::Parts | Self::ResponsesParts => "text content part is missing text",
            Self::Blocks => "text content block is missing text",
        }
    }

    /// Return the rejection for a non-textual outer content shape.
    const fn outer_message(self) -> &'static str {
        match self {
            Self::Parts | Self::ResponsesParts => "content must be a string or text parts",
            Self::Blocks => "content must be a string or text blocks",
        }
    }
}

// -----------------------------------------------------------------------------
// JoinedText: Accumulates text fragments with separators.
// -----------------------------------------------------------------------------

/// Text fragments accumulated with newline separators.
#[derive(Default)]
struct JoinedText(
    /// Accumulated text including inserted separators.
    String,
);

impl JoinedText {
    /// Append one fragment, inserting a separator when needed.
    fn push(&mut self, text: &str) {
        if !self.0.is_empty() {
            self.0.push('\n');
        }
        self.0.push_str(text);
    }

    /// Return the accumulated text.
    fn into_string(self) -> String {
        self.0
    }
}

/// Split provider output into readable streaming chunks.
pub(crate) fn stream_chunks(text: &str) -> Vec<String> {
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

/// Join an OpenAI/Anthropic typed text-item array.
///
/// # Errors
///
/// Returns [`ProviderRejection`] when an item is not textual or lacks text.
fn join_typed_text_items(
    items: &[Value],
    param: &'static str,
    kind: TextArrayKind,
) -> Result<String, ProviderRejection> {
    let mut text = JoinedText::default();
    for item in items {
        // Reject non-text blocks before provider-neutral lowering.
        if !kind.is_text_type(item.get("type").and_then(Value::as_str)) {
            return Err(ProviderRejection::unsupported(
                param,
                kind.unsupported_message(),
            ));
        }
        let item_text = item
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| ProviderRejection::invalid(param, kind.missing_text_message()))?;
        text.push(item_text);
    }
    Ok(text.into_string())
}

/// Extract optional text content from string, null, or typed text arrays.
///
/// # Errors
///
/// Returns [`ProviderRejection`] when the content shape is not textual or a
/// typed text array item is malformed.
pub(crate) fn optional_text_content(
    content: &Value,
    param: &'static str,
    kind: TextArrayKind,
) -> Result<Option<String>, ProviderRejection> {
    match content {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        Value::Array(items) => join_typed_text_items(items, param, kind).map(Some),
        _ => Err(ProviderRejection::unsupported(param, kind.outer_message())),
    }
}

// -----------------------------------------------------------------------------
// JsonEventExt: Encodes materialized JSON as SSE data.
// -----------------------------------------------------------------------------

/// SSE encoding behavior for already-materialized JSON values.
pub(crate) trait JsonEventExt {
    /// Encode this value as one SSE data event without a fallible re-serialization step.
    fn to_sse_event(&self) -> Event;
}

impl JsonEventExt for Value {
    fn to_sse_event(&self) -> Event {
        Event::default().data(self.to_string())
    }
}

// -----------------------------------------------------------------------------
// SseResponse: Owns an optionally paced server-sent event response.
// -----------------------------------------------------------------------------

/// SSE response with delivery pacing owned by its response type.
pub(crate) struct SseResponse {
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
pub(crate) struct SseEvents(
    /// Events delivered in insertion order.
    Vec<Event>,
);

impl SseEvents {
    /// Attach optional pacing before rendering these events.
    pub(crate) fn with_delay(self, delay_ms: u64) -> SseResponse {
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
pub(crate) struct NdjsonResponse {
    /// Records delivered in insertion order.
    records: Vec<Value>,
    /// Delay inserted before each record.
    delay_ms: u64,
}

impl NdjsonResponse {
    /// Build a paced NDJSON response from ordered JSON records.
    pub(crate) fn new(records: Vec<Value>, delay_ms: u64) -> Self {
        Self { records, delay_ms }
    }
}

impl IntoResponse for NdjsonResponse {
    fn into_response(self) -> Response {
        let delay_ms = self.delay_ms;
        let chunks = self.records.into_iter().map(|record| {
            let mut line = record.to_string();
            line.push('\n');
            Bytes::from(line)
        });
        let body = Body::from_stream(stream::iter(chunks).then(move |chunk| async move {
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            Ok::<_, std::convert::Infallible>(chunk)
        }));
        ([(header::CONTENT_TYPE, "application/x-ndjson")], body).into_response()
    }
}

/// Return the current Unix timestamp, or zero before the Unix epoch.
pub(crate) fn unix_timestamp() -> u64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(_) => 0,
    }
}
