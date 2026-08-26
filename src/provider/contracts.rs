//! Provider-neutral compatibility contracts.
//!
//! `OpenAI`, Anthropic, and Gemini all arrive with different JSON envelopes,
//! but the engine only needs a bounded transcript and a model id. Provider
//! modules lower into this middle contract, then render their own success or
//! failure shape after ELIZA produces one turn.
//! The shared contract deliberately avoids provider-specific concepts such as
//! choices, content blocks, candidates, and event names; those stay in the
//! adapter that owns the public wire shape.
//!
//! ```text
//! provider handler
//!   -> CompatTurnRequest { model, system_text, user_turns }
//!   -> complete_eliza
//!   -> CompatTurnResponse { model, output, usage }
//!   -> provider response or provider-shaped error
//! ```

use std::fmt;
use std::num::NonZeroUsize;
use std::str::FromStr;
use std::time::Duration;

use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::{StreamExt, stream};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;

use crate::eliza::{ElizaSession, doctor_script};
use crate::errors::AppError;

// -----------------------------------------------------------------------------
// ModelId: Validates the provider-visible model identifier.
// -----------------------------------------------------------------------------

/// Provider-visible model identifier accepted from CLI and JSON bodies.
///
/// Empty or whitespace-only model ids are rejected at the boundary. The default
/// is the bundled DOCTOR model id used by the server.
///
/// ```
/// use eliza::provider::contracts::ModelId;
///
/// let model: ModelId = "eliza-doctor".parse().unwrap();
/// assert_eq!(model.as_str(), "eliza-doctor");
/// assert_eq!(ModelId::default().as_str(), "eliza-doctor");
/// assert!("   ".parse::<ModelId>().is_err());
/// ```
/// Stores the wrapped value owned by this declaration.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub(crate) struct ModelId(
    /// Validated provider-visible identifier.
    String,
);

impl ModelId {
    /// Return the provider-visible model id.
    ///
    /// ```
    /// use eliza::provider::contracts::ModelId;
    ///
    /// let model: ModelId = "custom-eliza".parse().unwrap();
    /// assert_eq!(model.as_str(), "custom-eliza");
    /// ```
    #[must_use]
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ModelId {
    fn default() -> Self {
        Self("eliza-doctor".to_owned())
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl TryFrom<String> for ModelId {
    type Error = AppError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            Err(AppError::EmptyModelId)
        } else {
            Ok(Self(value))
        }
    }
}

impl<'de> Deserialize<'de> for ModelId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.try_into().map_err(de::Error::custom)
    }
}

impl FromStr for ModelId {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

// -----------------------------------------------------------------------------
// TranscriptText: Retains replayable provider text.
// -----------------------------------------------------------------------------

/// Stores the wrapped value owned by this declaration.
#[derive(Debug, Clone, Eq, PartialEq, derive_more::From)]
pub(crate) struct TranscriptText(
    /// Provider text retained for replay and accounting.
    String,
);

impl TranscriptText {
    /// Performs the as str operation for this abstraction.
    fn as_str(&self) -> &str {
        &self.0
    }

    /// Performs the char count operation for this abstraction.
    fn char_count(&self) -> usize {
        self.0.chars().count()
    }
}

// -----------------------------------------------------------------------------
// CompatTurnRequest: Owns one provider-neutral replay request.
// -----------------------------------------------------------------------------

/// Represents `CompatTurnRequest` state within this module.
#[derive(Debug)]
pub(super) struct CompatTurnRequest {
    /// Provider-visible model id echoed by the selected adapter.
    model: ModelId,
    /// Non-user instructions accepted for accounting and future inspection.
    ///
    /// Classic ELIZA has no system prompt concept, so these strings are not
    /// replayed through the engine.
    system_text: Vec<TranscriptText>,
    /// Ordered user turns replayed through a fresh ELIZA session.
    user_turns: Vec<TranscriptText>,
}

impl CompatTurnRequest {
    /// Performs the new operation for this abstraction.
    pub(super) fn new(model: ModelId, system_text: Vec<String>, user_turns: Vec<String>) -> Self {
        Self {
            model,
            system_text: system_text.into_iter().map(TranscriptText::from).collect(),
            user_turns: user_turns.into_iter().map(TranscriptText::from).collect(),
        }
    }

    /// Execute this provider-neutral request through a fresh ELIZA session.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderRejection`] when no user turn exists or configured
    /// transcript limits are exceeded.
    pub(super) fn complete(
        self,
        limits: RequestLimits,
    ) -> Result<CompatTurnResponse, ProviderRejection> {
        // Start by rejecting requests that cannot produce one ELIZA turn.
        if self.user_turns.is_empty() {
            return Err(ProviderRejection::invalid(
                "messages",
                "at least one user text turn is required",
            ));
        }

        // Bound replay work before allocating a session or walking text.
        if self.user_turns.len() > limits.max_history_messages.get() {
            return Err(ProviderRejection::too_large(format!(
                "too many user turns: {} > {}",
                self.user_turns.len(),
                limits.max_history_messages
            )));
        }

        // Count every accepted text fragment, including non-replayed system text.
        let transcript_text = self.system_text.iter().chain(self.user_turns.iter());
        let input_chars = transcript_text
            .map(TranscriptText::char_count)
            .sum::<usize>();

        // Reject oversized transcripts before allocating a session for replay.
        if input_chars > limits.max_input_chars.get() {
            return Err(ProviderRejection::too_large(format!(
                "input text is too large: {input_chars} > {}",
                limits.max_input_chars
            )));
        }

        // Replay only user turns; the final turn is the provider response.
        let mut session = ElizaSession::from(doctor_script());
        let mut output = String::new();
        for turn in &self.user_turns {
            output = session.respond(turn.as_str()).output;
        }

        // Report approximate usage in the vocabulary each adapter expects.
        let prompt = approximate_tokens(
            self.system_text
                .iter()
                .chain(self.user_turns.iter())
                .map(TranscriptText::as_str),
        );
        let completion = approximate_tokens(std::iter::once(output.as_str()));
        let usage = TokenUsage {
            prompt,
            completion,
            total: prompt + completion,
        };
        Ok(CompatTurnResponse::completed(self.model, output, usage))
    }
}

// -----------------------------------------------------------------------------
// TokenUsage: Records approximate provider token counts.
// -----------------------------------------------------------------------------

/// Represents `TokenUsage` state within this module.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
pub(super) struct TokenUsage {
    /// Approximate input token count derived from whitespace-separated text.
    #[serde(rename = "prompt_tokens")]
    pub(super) prompt: usize,
    /// Approximate output token count derived from the final ELIZA response.
    #[serde(rename = "completion_tokens")]
    pub(super) completion: usize,
    /// Prompt plus completion tokens.
    #[serde(rename = "total_tokens")]
    pub(super) total: usize,
}

// -----------------------------------------------------------------------------
// CompatTurnResponse: Owns one completed provider-neutral turn.
// -----------------------------------------------------------------------------

/// Represents `CompatTurnResponse` state within this module.
#[derive(Debug)]
pub(super) struct CompatTurnResponse {
    /// Model id copied from the request so adapters can echo provider shape.
    pub(super) model: ModelId,
    /// Final ELIZA output from the last replayed user turn.
    pub(super) output: String,
    /// Approximate usage rendered into provider-specific counters.
    pub(super) usage: TokenUsage,
}

impl CompatTurnResponse {
    /// Assemble a completed response from engine output and usage accounting.
    fn completed(model: ModelId, output: String, usage: TokenUsage) -> Self {
        Self {
            model,
            output,
            usage,
        }
    }
}

// -----------------------------------------------------------------------------
// RequestLimits: Bounds transcript size and replay work.
// -----------------------------------------------------------------------------

/// Represents `RequestLimits` state within this module.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RequestLimits {
    /// Maximum accepted character count across all textual request inputs.
    max_input_chars: NonZeroUsize,
    /// Maximum number of user turns replayed into one fresh session.
    max_history_messages: NonZeroUsize,
}

impl RequestLimits {
    /// Build limits from validated positive bounds.
    pub(super) const fn new(
        max_input_chars: NonZeroUsize,
        max_history_messages: NonZeroUsize,
    ) -> Self {
        Self {
            max_input_chars,
            max_history_messages,
        }
    }
}

// -----------------------------------------------------------------------------
// ProviderRejection: Stores failures rendered by provider adapters.
// -----------------------------------------------------------------------------

/// Represents `ProviderRejection` state within this module.
#[derive(Debug, Clone)]
pub(crate) struct ProviderRejection {
    /// Stores the status value owned by this contract.
    pub(super) status: StatusCode,
    /// Stores the openai type value owned by this contract.
    pub(super) openai_type: &'static str,
    /// Stores the gemini status value owned by this contract.
    pub(super) gemini_status: &'static str,
    /// Stores the message value owned by this contract.
    pub(super) message: String,
    /// Stores the param value owned by this contract.
    pub(super) param: Option<&'static str>,
}

impl ProviderRejection {
    /// Build a provider validation rejection for malformed input.
    pub(super) fn invalid(param: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            openai_type: "invalid_request_error",
            gemini_status: "INVALID_ARGUMENT",
            message: message.into(),
            param: Some(param),
        }
    }

    /// Build a provider validation rejection for unsupported features.
    pub(super) fn unsupported(param: &'static str, message: impl Into<String>) -> Self {
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
    fn too_large(message: impl Into<String>) -> Self {
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

/// Enumerates the supported `TextArrayKind` cases.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TextArrayKind {
    /// Represents the `Parts` case.
    Parts,
    /// Represents the `Blocks` case.
    Blocks,
}

impl TextArrayKind {
    /// Performs the unsupported message operation for this abstraction.
    const fn unsupported_message(self) -> &'static str {
        match self {
            Self::Parts => "only text content parts are supported",
            Self::Blocks => "only text content blocks are supported",
        }
    }

    /// Performs the missing text message operation for this abstraction.
    const fn missing_text_message(self) -> &'static str {
        match self {
            Self::Parts => "text content part is missing text",
            Self::Blocks => "text content block is missing text",
        }
    }

    /// Performs the outer message operation for this abstraction.
    const fn outer_message(self) -> &'static str {
        match self {
            Self::Parts => "content must be a string or text parts",
            Self::Blocks => "content must be a string or text blocks",
        }
    }
}

/// Performs the approximate tokens operation for this abstraction.
fn approximate_tokens<'a>(texts: impl Iterator<Item = &'a str>) -> usize {
    texts
        .flat_map(str::split_whitespace)
        .filter(|part| !part.is_empty())
        .count()
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
pub(super) fn required_text_parts(
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

/// Join an OpenAI/Anthropic typed text-item array.
///
/// # Errors
///
/// Returns [`ProviderRejection`] when an item has a non-text type or is missing
/// its text payload.
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
pub(super) fn optional_text_content(
    content: &Value,
    param: &'static str,
    kind: TextArrayKind,
) -> Result<Option<String>, ProviderRejection> {
    // Null means the provider field was present but carries no text.
    match content {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        Value::Array(items) => join_typed_text_items(items, param, kind).map(Some),
        _ => Err(ProviderRejection::unsupported(param, kind.outer_message())),
    }
}

// -----------------------------------------------------------------------------
// StreamChunks: Splits provider output for streaming.
// -----------------------------------------------------------------------------

/// Performs the stream chunks operation for this abstraction.
pub(super) fn stream_chunks(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    // Keep whitespace with the preceding word so SDK stream parsers see
    // readable incremental text without claiming tokenizer fidelity.
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
// SseEvents: Owns optionally paced server-sent events.
// -----------------------------------------------------------------------------

/// Owned SSE events ready for optional paced delivery.
#[derive(derive_more::From)]
pub(super) struct SseEvents(
    /// Events delivered in insertion order.
    Vec<Event>,
);

impl SseEvents {
    /// Convert these events into an Axum SSE response with optional pacing.
    pub(super) fn into_response(self, delay_ms: u64) -> Response {
        let base = stream::iter(self.0.into_iter().map(Ok::<_, std::convert::Infallible>));
        if delay_ms == 0 {
            Sse::new(base)
                .keep_alive(KeepAlive::default())
                .into_response()
        } else {
            // Serialize pacing through the stream so dropped responses cancel it.
            Sse::new(base.then(move |event| async move {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                event
            }))
            .keep_alive(KeepAlive::default())
            .into_response()
        }
    }
}

/// Performs the unix timestamp operation for this abstraction.
pub(super) fn unix_timestamp() -> u64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(_) => 0,
    }
}
