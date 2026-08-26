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

pub(crate) mod anthropic;
pub(crate) mod gemini;
pub(crate) mod openai;

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
// Provider-neutral turn contracts: the only data shape shared by OpenAI,
// Anthropic, Gemini, and the historical engine.
// -----------------------------------------------------------------------------

/// Provider-visible model identifier accepted from CLI and JSON bodies.
///
/// Empty or whitespace-only model ids are rejected at the boundary. The default
/// is the bundled DOCTOR model id used by the server.
///
/// ```
/// use eliza::ModelId;
///
/// let model: ModelId = "eliza-doctor".parse().unwrap();
/// assert_eq!(model.as_str(), "eliza-doctor");
/// assert_eq!(ModelId::default().as_str(), "eliza-doctor");
/// assert!("   ".parse::<ModelId>().is_err());
/// ```
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct ModelId(String);

impl ModelId {
    /// Return the provider-visible model id.
    ///
    /// ```
    /// use eliza::ModelId;
    ///
    /// let model: ModelId = "custom-eliza".parse().unwrap();
    /// assert_eq!(model.as_str(), "custom-eliza");
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
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

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct TranscriptText(String);

impl TranscriptText {
    fn as_str(&self) -> &str {
        &self.0
    }

    fn char_count(&self) -> usize {
        self.0.chars().count()
    }
}

impl From<String> for TranscriptText {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug)]
pub(crate) struct CompatTurnRequest {
    /// Provider-visible model id echoed by the selected adapter.
    pub(crate) model: ModelId,
    /// Non-user instructions accepted for accounting and future inspection.
    ///
    /// Classic ELIZA has no system prompt concept, so these strings are not
    /// replayed through the engine.
    pub(crate) system_text: Vec<TranscriptText>,
    /// Ordered user turns replayed through a fresh ELIZA session.
    pub(crate) user_turns: Vec<TranscriptText>,
}

impl CompatTurnRequest {
    pub(crate) fn new(model: ModelId, system_text: Vec<String>, user_turns: Vec<String>) -> Self {
        Self {
            model,
            system_text: system_text.into_iter().map(TranscriptText::from).collect(),
            user_turns: user_turns.into_iter().map(TranscriptText::from).collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
pub(crate) struct TokenUsage {
    /// Approximate input token count derived from whitespace-separated text.
    #[serde(rename = "prompt_tokens")]
    pub(crate) prompt: usize,
    /// Approximate output token count derived from the final ELIZA response.
    #[serde(rename = "completion_tokens")]
    pub(crate) completion: usize,
    /// Prompt plus completion tokens.
    #[serde(rename = "total_tokens")]
    pub(crate) total: usize,
}

impl TokenUsage {
    pub(crate) const fn new(prompt_tokens: usize, completion_tokens: usize) -> Self {
        Self {
            prompt: prompt_tokens,
            completion: completion_tokens,
            total: prompt_tokens + completion_tokens,
        }
    }
}

#[derive(Debug)]
pub(crate) struct CompatTurnResponse {
    /// Model id copied from the request so adapters can echo provider shape.
    pub(crate) model: ModelId,
    /// Final ELIZA output from the last replayed user turn.
    pub(crate) output: String,
    /// Approximate usage rendered into provider-specific counters.
    pub(crate) usage: TokenUsage,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RequestLimits {
    /// Maximum accepted character count across all textual request inputs.
    pub(crate) max_input_chars: NonZeroUsize,
    /// Maximum number of user turns replayed into one fresh session.
    pub(crate) max_history_messages: NonZeroUsize,
}

// -----------------------------------------------------------------------------
// Provider rejections: one internal failure fact rendered by the owning adapter
// as OpenAI, Anthropic, or Gemini's public error envelope.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct ProviderRejection {
    pub(crate) status: StatusCode,
    pub(crate) openai_type: &'static str,
    pub(crate) gemini_status: &'static str,
    pub(crate) message: String,
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

    /// Build a provider validation rejection for configured size limits.
    pub(crate) fn too_large(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            openai_type: "request_too_large",
            gemini_status: "RESOURCE_EXHAUSTED",
            message: message.into(),
            param: None,
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
}

// -----------------------------------------------------------------------------
// Provider text syntax: names the accepted content-array dialect so shared
// extraction can report errors in provider terms.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub(crate) enum TextArrayKind {
    Parts,
    Blocks,
}

impl TextArrayKind {
    const fn unsupported_message(self) -> &'static str {
        match self {
            Self::Parts => "only text content parts are supported",
            Self::Blocks => "only text content blocks are supported",
        }
    }

    const fn missing_text_message(self) -> &'static str {
        match self {
            Self::Parts => "text content part is missing text",
            Self::Blocks => "text content block is missing text",
        }
    }

    const fn outer_message(self) -> &'static str {
        match self {
            Self::Parts => "content must be a string or text parts",
            Self::Blocks => "content must be a string or text blocks",
        }
    }
}

// -----------------------------------------------------------------------------
// Replay execution: validates transcript bounds, replays user turns through a
// fresh ELIZA session, and returns only the final turn.
// -----------------------------------------------------------------------------

fn approximate_tokens<'a>(texts: impl Iterator<Item = &'a str>) -> usize {
    texts
        .flat_map(str::split_whitespace)
        .filter(|part| !part.is_empty())
        .count()
}

/// Execute the provider-neutral ELIZA turn request.
///
/// # Errors
///
/// Returns [`ProviderRejection`] when the request has no user turns, exceeds the
/// configured history length, or exceeds the configured text character limit.
pub(crate) fn complete_eliza(
    request: CompatTurnRequest,
    limits: RequestLimits,
) -> Result<CompatTurnResponse, ProviderRejection> {
    // --- Start by rejecting requests that cannot produce one ELIZA turn.
    if request.user_turns.is_empty() {
        return Err(ProviderRejection::invalid(
            "messages",
            "at least one user text turn is required",
        ));
    }

    // --- Bound replay work before allocating a session or walking text.
    if request.user_turns.len() > limits.max_history_messages.get() {
        return Err(ProviderRejection::too_large(format!(
            "too many user turns: {} > {}",
            request.user_turns.len(),
            limits.max_history_messages
        )));
    }

    // --- Count every accepted text fragment, including non-replayed system text.
    let input_chars = request
        .system_text
        .iter()
        .chain(request.user_turns.iter())
        .map(TranscriptText::char_count)
        .sum::<usize>();
    if input_chars > limits.max_input_chars.get() {
        return Err(ProviderRejection::too_large(format!(
            "input text is too large: {input_chars} > {}",
            limits.max_input_chars
        )));
    }

    // --- Replay only user turns; the final turn is the provider response.
    let mut session = ElizaSession::new(doctor_script());
    let mut output = String::new();
    for turn in &request.user_turns {
        output = session.respond(turn.as_str()).output;
    }

    // --- Report approximate usage in the vocabulary each adapter expects.
    let prompt_tokens = approximate_tokens(
        request
            .system_text
            .iter()
            .chain(request.user_turns.iter())
            .map(TranscriptText::as_str),
    );
    let completion_tokens = approximate_tokens(std::iter::once(output.as_str()));

    Ok(CompatTurnResponse {
        model: request.model,
        output,
        usage: TokenUsage::new(prompt_tokens, completion_tokens),
    })
}

// -----------------------------------------------------------------------------
// Provider text extraction: accepts only textual content before a provider body
// is allowed to become an ELIZA transcript.
// -----------------------------------------------------------------------------

fn push_joined_text(output: &mut String, text: &str) {
    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str(text);
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
    let mut text = String::new();
    for item in items {
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
        push_joined_text(&mut text, item_text);
    }
    Ok(text)
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
    // --- Null means the provider field was present but carries no text.
    match content {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        Value::Array(items) => join_typed_text_items(items, param, kind).map(Some),
        _ => Err(ProviderRejection::unsupported(param, kind.outer_message())),
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
    let mut text = String::new();
    for part in parts {
        // --- Native Gemini parts are accepted only when they are textual.
        let Some(part_text) = part.get("text").and_then(Value::as_str) else {
            return Err(ProviderRejection::unsupported(
                param,
                "only text parts are supported",
            ));
        };
        push_joined_text(&mut text, part_text);
    }

    if text.is_empty() {
        Err(ProviderRejection::invalid(param, "text parts are required"))
    } else {
        Ok(text)
    }
}

// -----------------------------------------------------------------------------
// Streaming utilities: adapters choose event payloads while this module owns
// chunk boundaries, wall-clock stamps, and optional demo delay.
// -----------------------------------------------------------------------------

pub(crate) fn stream_chunks(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    // --- Keep whitespace with the preceding word so SDK stream parsers see
    // readable incremental text without claiming tokenizer fidelity.
    for character in text.chars() {
        current.push(character);
        if character.is_whitespace() {
            chunks.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

pub(crate) fn sse_response(events: Vec<Event>, delay_ms: u64) -> Response {
    let base = stream::iter(events.into_iter().map(Ok::<_, std::convert::Infallible>));
    if delay_ms == 0 {
        Sse::new(base)
            .keep_alive(KeepAlive::default())
            .into_response()
    } else {
        // --- Optional pacing is serialized through the stream itself so no
        // background task survives after the response is dropped.
        Sse::new(base.then(move |event| async move {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            event
        }))
        .keep_alive(KeepAlive::default())
        .into_response()
    }
}

pub(crate) fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
