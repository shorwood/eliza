//! Shared provider HTTP boundary types and helpers.

use std::time::Duration;

use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::{StreamExt, stream};
use serde_json::Value;

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
}

// -----------------------------------------------------------------------------
// TextArrayKind: Describes provider-specific typed text arrays.
// -----------------------------------------------------------------------------

/// Provider-specific typed text array shape.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TextArrayKind {
    /// OpenAI/Gemini-style parts.
    Parts,
    /// Anthropic-style blocks.
    Blocks,
}

impl TextArrayKind {
    /// Return the rejection for a non-text typed item.
    const fn unsupported_message(self) -> &'static str {
        match self {
            Self::Parts => "only text content parts are supported",
            Self::Blocks => "only text content blocks are supported",
        }
    }

    /// Return the rejection for a typed item without text.
    const fn missing_text_message(self) -> &'static str {
        match self {
            Self::Parts => "text content part is missing text",
            Self::Blocks => "text content block is missing text",
        }
    }

    /// Return the rejection for a non-textual outer content shape.
    const fn outer_message(self) -> &'static str {
        match self {
            Self::Parts => "content must be a string or text parts",
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

    /// Report whether no text has been accumulated.
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Extract required Gemini text parts from a native parts array.
///
/// # Errors
///
/// Returns [`ProviderRejection`] when any part is non-textual or the resulting
/// text is empty.
pub(crate) fn required_text_parts(
    parts: &[Value],
    param: &'static str,
) -> Result<String, ProviderRejection> {
    let mut text = JoinedText::default();
    for part in parts {
        // Native Gemini parts are accepted only when they are textual.
        let Some(part_text) = part.get("text").and_then(Value::as_str) else {
            return Err(ProviderRejection::unsupported(
                param,
                "only text parts are supported",
            ));
        };
        text.push(part_text);
    }

    if text.is_empty() {
        Err(ProviderRejection::invalid(param, "text parts are required"))
    } else {
        Ok(text.into_string())
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
        if item.get("type").and_then(Value::as_str) != Some("text") {
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
        if self.delay_ms == 0 {
            Sse::new(base)
                .keep_alive(KeepAlive::default())
                .into_response()
        } else {
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

/// Return the current Unix timestamp, or zero before the Unix epoch.
pub(crate) fn unix_timestamp() -> u64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(_) => 0,
    }
}
