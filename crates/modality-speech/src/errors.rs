//! Provider-neutral speech diagnostics.

use miette::Diagnostic;
use thiserror::Error;

// -----------------------------------------------------------------------------
// Error: Classifies failures without transport semantics.
// -----------------------------------------------------------------------------

/// Broad failure category exposed to provider adapters.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ErrorKind {
    /// Hosted native capacity is occupied and cannot queue more work.
    Overloaded,
    /// Provider input cannot be synthesized.
    InvalidInput,
    /// Provider input or output exceeds a configured bound.
    Limit,
    /// Synthesis or encoding failed unexpectedly.
    Internal,
}

/// Provider-neutral request field associated with a failure.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ErrorField {
    /// Text submitted for synthesis.
    Input,
    /// Requested sample rate.
    SampleRate,
    /// Requested speaking speed.
    Speed,
}

/// Typed failures raised by provider-neutral speech handling.
#[derive(Debug, Diagnostic, Error)]
pub enum Error {
    /// Hosted native worker capacity is occupied; no generation queue is allowed.
    #[error("speech capacity temporarily exhausted")]
    #[diagnostic(code(eliza::speech::overloaded))]
    Overloaded,

    /// No pronounceable input was provided.
    #[error("speech input must not be empty")]
    #[diagnostic(code(eliza::speech::empty_input))]
    EmptyInput,

    /// Input exceeds the configured request bound.
    #[error("speech input is too large: {actual} > {limit}")]
    #[diagnostic(code(eliza::speech::input_too_large))]
    InputTooLarge {
        /// Submitted character count.
        actual: usize,
        /// Configured maximum character count.
        limit: usize,
    },

    /// Speaking rate is outside the OpenAI-compatible range.
    #[error("speech speed must be between 0.25 and 4.0")]
    #[diagnostic(code(eliza::speech::invalid_speed))]
    InvalidSpeed,

    /// Generated PCM would exceed the bounded fixture duration.
    #[error("rendered speech exceeds 120 seconds")]
    #[diagnostic(code(eliza::speech::output_too_long))]
    OutputTooLong,

    /// The MP3 codec rejected PCM input or failed while draining output.
    #[error("failed to encode speech audio: {detail}")]
    #[diagnostic(code(eliza::speech::mp3_encoding))]
    Mp3Encoding {
        /// Stable owned codec failure detail.
        detail: String,
    },

    /// The MP3 codec completed without producing an audio frame.
    #[error("failed to encode speech audio: MP3 encoder produced no frames")]
    #[diagnostic(code(eliza::speech::mp3_no_frames))]
    Mp3NoFrames,

    /// The bundled English voice intentionally accepts ASCII input only.
    #[error("speech input contains unsupported character `{character}`")]
    #[diagnostic(code(eliza::speech::unsupported_character))]
    UnsupportedCharacter {
        /// First unsupported character.
        character: char,
    },

    /// The provider requested a rate outside the deliberately small set.
    #[error("unsupported speech sample rate `{sample_rate}`")]
    #[diagnostic(code(eliza::speech::unsupported_sample_rate))]
    UnsupportedSampleRate {
        /// Requested samples per second.
        sample_rate: u32,
    },

    /// The worker semaphore was unexpectedly closed.
    #[error("speech worker pool is unavailable")]
    #[diagnostic(code(eliza::speech::unavailable))]
    Unavailable,

    /// Tokio could not run the blocking synthesis task.
    #[error("speech worker failed: {detail}")]
    #[diagnostic(code(eliza::speech::worker))]
    Worker {
        /// Stable owned blocking-task failure detail.
        detail: String,
    },
}

impl Error {
    /// Classify the failure without imposing HTTP or provider semantics.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::Overloaded => ErrorKind::Overloaded,
            Self::InputTooLarge { .. } | Self::OutputTooLong => ErrorKind::Limit,
            Self::EmptyInput
            | Self::InvalidSpeed
            | Self::UnsupportedCharacter { .. }
            | Self::UnsupportedSampleRate { .. } => ErrorKind::InvalidInput,
            Self::Mp3Encoding { .. }
            | Self::Mp3NoFrames
            | Self::Unavailable
            | Self::Worker { .. } => ErrorKind::Internal,
        }
    }

    /// Identify the provider-neutral request field associated with this failure.
    #[must_use]
    pub const fn field(&self) -> Option<ErrorField> {
        match self {
            Self::EmptyInput | Self::InputTooLarge { .. } | Self::UnsupportedCharacter { .. } => {
                Some(ErrorField::Input)
            }
            Self::InvalidSpeed => Some(ErrorField::Speed),
            Self::UnsupportedSampleRate { .. } => Some(ErrorField::SampleRate),
            Self::Mp3Encoding { .. }
            | Self::Mp3NoFrames
            | Self::OutputTooLong
            | Self::Unavailable
            | Self::Overloaded
            | Self::Worker { .. } => None,
        }
    }
}
