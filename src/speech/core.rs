//! Provider-neutral speech requests, responses, and errors.
use std::num::NonZeroUsize;

use miette::Diagnostic;
use thiserror::Error;

use crate::problem::{ProblemClass, ProblemDetails};
use crate::types::model::ModelId;

// -----------------------------------------------------------------------------
// ModelId: Publishes the built-in speech model identifier.
// -----------------------------------------------------------------------------

/// Provider-visible identifier for the bundled retro voice.
pub(crate) const MODEL_ID: &str = "eliza-retro-tts";

/// Construct the fixed speech model ID without exposing unchecked strings.
///
/// # Panics
///
/// Panics only if the source constant is changed to an empty identifier.
#[must_use]
pub(crate) fn model_id() -> ModelId {
    MODEL_ID
        .parse()
        .expect("the built-in speech model id is nonempty")
}

// -----------------------------------------------------------------------------
// AudioFormat: Names the supported output encodings.
// -----------------------------------------------------------------------------

/// Audio container or sample encoding requested by a provider adapter.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum AudioFormat {
    /// MPEG Layer III.
    Mp3,
    /// RIFF/WAVE containing signed 16-bit PCM.
    Wav,
    /// Headerless signed 16-bit little-endian PCM.
    Pcm,
    /// Headerless signed 16-bit big-endian PCM for RFC L16.
    L16,
    /// Headerless G.711 mu-law.
    MuLaw,
    /// Headerless G.711 A-law.
    ALaw,
}

// -----------------------------------------------------------------------------
// Speech: Defines provider-neutral synthesis input.
// -----------------------------------------------------------------------------

/// One independently voiced piece of a speech request.
#[derive(Debug, Clone)]
pub(crate) struct SpeechSegment {
    /// Text pronounced by Flite's English speech pipeline.
    pub(crate) text: String,
    /// Arbitrary provider voice identifier.
    pub(crate) voice: String,
    /// Optional provider prose interpreted as a small set of style keywords.
    pub(crate) style: String,
    /// Explicit speaking-rate multiplier.
    pub(crate) speed: f32,
    /// Silence appended after this segment.
    pub(crate) pause_after_ms: u16,
}

/// Provider-neutral speech request.
#[derive(Debug, Clone)]
pub(crate) struct SpeechRequest {
    /// Ordered text and voice segments.
    pub(crate) segments: Vec<SpeechSegment>,
    /// Output samples per second.
    pub(crate) sample_rate: u32,
}

impl SpeechRequest {
    /// Approximate input tokens using the convention used by chat adapters.
    #[must_use]
    pub(crate) fn input_tokens(&self) -> usize {
        self.segments
            .iter()
            .map(|segment| segment.text.split_whitespace().count())
            .sum()
    }

    /// Validate provider-neutral limits before occupying a synthesis worker.
    ///
    /// # Errors
    ///
    /// Returns a typed request failure for invalid text, rate, speed, or size.
    pub(super) fn validate(&self, max_chars: NonZeroUsize) -> Result<(), SpeechError> {
        // The synthesizer and exposed encodings intentionally support three rates.
        if !matches!(self.sample_rate, 8_000 | 16_000 | 24_000) {
            return Err(SpeechError::UnsupportedSampleRate {
                sample_rate: self.sample_rate,
            });
        }

        // A request with no pronounceable text cannot produce meaningful audio.
        if self.segments.is_empty()
            || self
                .segments
                .iter()
                .all(|segment| segment.text.trim().is_empty())
        {
            return Err(SpeechError::EmptyInput);
        }

        let chars = self
            .segments
            .iter()
            .map(|segment| segment.text.chars().count())
            .sum::<usize>();

        // Bound parsing and synthesis work before acquiring a worker permit.
        if chars > max_chars.get() {
            return Err(SpeechError::InputTooLarge {
                actual: chars,
                limit: max_chars.get(),
            });
        }

        for segment in &self.segments {
            // Empty pieces make speaker ordering and usage accounting ambiguous.
            if segment.text.trim().is_empty() {
                return Err(SpeechError::EmptyInput);
            }

            // Keep the deliberately small English fixture surface ASCII-only.
            if let Some(character) = segment.text.chars().find(|character| {
                !character.is_ascii()
                    || (character.is_ascii_control() && !character.is_ascii_whitespace())
            }) {
                return Err(SpeechError::UnsupportedCharacter { character });
            }

            // Enforce the provider speaking-rate range.
            if !(0.25..=4.0).contains(&segment.speed) {
                return Err(SpeechError::InvalidSpeed);
            }
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// RenderedAudio: Carries encoded engine output to provider adapters.
// -----------------------------------------------------------------------------

/// Encoded audio returned to a provider adapter.
#[derive(Debug)]
pub(crate) struct RenderedAudio {
    /// Complete encoded payload.
    pub(crate) bytes: Vec<u8>,
    /// Provider-safe MIME type including the sample rate where relevant.
    pub(crate) media_type: String,
    /// Number of uncompressed mono samples represented by the payload.
    pub(super) sample_count: usize,
    /// Samples per second in the uncompressed signal.
    pub(super) sample_rate: u32,
}

impl RenderedAudio {
    /// Approximate audio tokens as 20 ms frames.
    #[must_use]
    pub(crate) fn output_tokens(&self) -> usize {
        self.sample_count
            .div_ceil((self.sample_rate as usize).div_ceil(50))
    }
}

// -----------------------------------------------------------------------------
// SpeechError: Classifies provider-neutral speech failures.
// -----------------------------------------------------------------------------

/// Typed failures raised by provider-neutral speech handling.
#[derive(Debug, Diagnostic, Error)]
pub(crate) enum SpeechError {
    /// No pronounceable input was provided.
    #[error("speech input must not be empty")]
    #[diagnostic(code(eliza::speech::empty_input))]
    EmptyInput,

    /// The bundled English voice intentionally accepts ASCII input only.
    #[error("speech input contains unsupported character `{character}`")]
    #[diagnostic(code(eliza::speech::unsupported_character))]
    UnsupportedCharacter {
        /// First unsupported character.
        character: char,
    },

    /// Input exceeds the configured request bound.
    #[error("speech input is too large: {actual} > {limit}")]
    #[diagnostic(code(eliza::speech::input_too_large))]
    InputTooLarge {
        /// Submitted character count.
        actual: usize,
        /// Configured maximum character count.
        limit: usize,
    },

    /// The provider requested a rate outside the deliberately small set.
    #[error("unsupported speech sample rate `{sample_rate}`")]
    #[diagnostic(code(eliza::speech::unsupported_sample_rate))]
    UnsupportedSampleRate {
        /// Requested samples per second.
        sample_rate: u32,
    },

    /// Speaking rate is outside the OpenAI-compatible range.
    #[error("speech speed must be between 0.25 and 4.0")]
    #[diagnostic(code(eliza::speech::invalid_speed))]
    InvalidSpeed,

    /// Generated PCM would exceed the bounded fixture duration.
    #[error("rendered speech exceeds 120 seconds")]
    #[diagnostic(code(eliza::speech::output_too_long))]
    OutputTooLong,

    /// Pure-Rust audio encoding failed.
    #[error("failed to encode speech audio: {message}")]
    #[diagnostic(code(eliza::speech::encoding))]
    Encoding {
        /// Codec error rendered without implementation details in responses.
        message: String,
    },

    /// Tokio could not run the blocking synthesis task.
    #[error("speech worker failed: {source}")]
    #[diagnostic(code(eliza::speech::worker))]
    Worker {
        /// Blocking task failure.
        #[source]
        source: tokio::task::JoinError,
    },

    /// The worker semaphore was unexpectedly closed.
    #[error("speech worker pool is unavailable")]
    #[diagnostic(code(eliza::speech::unavailable))]
    Unavailable,
}

impl ProblemDetails for SpeechError {
    fn class(&self) -> ProblemClass {
        match self {
            Self::InputTooLarge { .. } | Self::OutputTooLong => ProblemClass::RequestTooLarge,
            Self::EmptyInput
            | Self::UnsupportedCharacter { .. }
            | Self::UnsupportedSampleRate { .. }
            | Self::InvalidSpeed => ProblemClass::InvalidRequest,
            Self::Encoding { .. } | Self::Worker { .. } | Self::Unavailable => {
                ProblemClass::Internal
            }
        }
    }

    fn param(&self) -> Option<&'static str> {
        match self {
            Self::EmptyInput | Self::UnsupportedCharacter { .. } | Self::InputTooLarge { .. } => {
                Some("input")
            }
            Self::UnsupportedSampleRate { .. } => Some("sample_rate"),
            Self::InvalidSpeed => Some("speed"),
            Self::OutputTooLong
            | Self::Encoding { .. }
            | Self::Worker { .. }
            | Self::Unavailable => None,
        }
    }
}
