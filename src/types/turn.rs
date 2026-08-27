//! Provider-neutral ELIZA turn types.

use std::num::NonZeroUsize;

use schemars::JsonSchema;
use serde::Serialize;

use super::http::ProviderRejection;
use super::model::ModelId;
use crate::eliza::engine::{ElizaSession, doctor_script};

// -----------------------------------------------------------------------------
// TranscriptText: Retains replayable provider text.
// -----------------------------------------------------------------------------

/// Provider text retained for replay and accounting.
#[derive(Debug, Clone, Eq, PartialEq, derive_more::From)]
struct TranscriptText(
    /// Provider text retained for replay and accounting.
    String,
);

impl TranscriptText {
    /// Return the retained provider text.
    fn as_str(&self) -> &str {
        &self.0
    }

    /// Count Unicode scalar values for request limiting.
    fn char_count(&self) -> usize {
        self.0.chars().count()
    }
}

// -----------------------------------------------------------------------------
// CompatTurnRequest: Owns one provider-neutral replay request.
// -----------------------------------------------------------------------------

/// One provider-neutral replay request.
#[derive(Debug)]
pub(crate) struct CompatTurnRequest {
    /// Provider-visible model id echoed by the selected adapter.
    model: ModelId,
    /// Non-user instructions retained for accounting.
    system_text: Vec<TranscriptText>,
    /// Ordered user turns replayed through a fresh ELIZA session.
    user_turns: Vec<TranscriptText>,
}

impl CompatTurnRequest {
    /// Build a provider-neutral request from provider text.
    pub(crate) fn new(model: ModelId, system_text: Vec<String>, user_turns: Vec<String>) -> Self {
        Self {
            model,
            system_text: system_text.into_iter().map(TranscriptText::from).collect(),
            user_turns: user_turns.into_iter().map(TranscriptText::from).collect(),
        }
    }

    /// Execute this request through a fresh ELIZA session.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderRejection`] when no user turn exists or configured
    /// transcript limits are exceeded.
    pub(crate) fn complete(
        self,
        limits: RequestLimits,
    ) -> Result<CompatTurnResponse, ProviderRejection> {
        // Validate that at least one user turn exists.
        if self.user_turns.is_empty() {
            return Err(ProviderRejection::invalid(
                "messages",
                "at least one user text turn is required",
            ));
        }

        // Check transcript size limits before replaying the session.
        if self.user_turns.len() > limits.max_history_messages.get() {
            return Err(ProviderRejection::too_large(format!(
                "too many user turns: {} > {}",
                self.user_turns.len(),
                limits.max_history_messages
            )));
        }

        // Compute the total input character count across system and user turns.
        let input_chars = self
            .system_text
            .iter()
            .chain(self.user_turns.iter())
            .map(TranscriptText::char_count)
            .sum::<usize>();

        // Check the total input character count against the configured limit.
        if input_chars > limits.max_input_chars.get() {
            return Err(ProviderRejection::too_large(format!(
                "input text is too large: {input_chars} > {}",
                limits.max_input_chars
            )));
        }

        let mut session = ElizaSession::from(doctor_script());
        let mut output = String::new();
        for turn in &self.user_turns {
            output = session.respond(turn.as_str()).output;
        }

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

/// Approximate provider token counts.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, JsonSchema)]
pub(crate) struct TokenUsage {
    /// Approximate input token count.
    #[serde(rename = "prompt_tokens")]
    pub(crate) prompt: usize,
    /// Approximate output token count.
    #[serde(rename = "completion_tokens")]
    pub(crate) completion: usize,
    /// Prompt plus completion tokens.
    #[serde(rename = "total_tokens")]
    pub(crate) total: usize,
}

// -----------------------------------------------------------------------------
// CompatTurnResponse: Owns one completed provider-neutral turn.
// -----------------------------------------------------------------------------

/// One completed provider-neutral turn.
#[derive(Debug)]
pub(crate) struct CompatTurnResponse {
    /// Model id echoed by the provider adapter.
    pub(crate) model: ModelId,
    /// Final ELIZA output.
    pub(crate) output: String,
    /// Approximate token usage.
    pub(crate) usage: TokenUsage,
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

/// Bounds transcript size and replay work.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RequestLimits {
    /// Maximum accepted character count across textual inputs.
    max_input_chars: NonZeroUsize,
    /// Maximum accepted number of user turns.
    max_history_messages: NonZeroUsize,
}

impl RequestLimits {
    /// Build limits from validated positive bounds.
    pub(crate) const fn new(
        max_input_chars: NonZeroUsize,
        max_history_messages: NonZeroUsize,
    ) -> Self {
        Self {
            max_input_chars,
            max_history_messages,
        }
    }
}

/// Approximate token usage using whitespace-separated text.
fn approximate_tokens<'a>(texts: impl Iterator<Item = &'a str>) -> usize {
    texts
        .flat_map(str::split_whitespace)
        .filter(|part| !part.is_empty())
        .count()
}
