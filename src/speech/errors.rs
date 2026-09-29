//! Provider-neutral speech diagnostics.

use miette::Diagnostic;
use thiserror::Error;

use crate::problem::{ProblemClass, ProblemDetails};

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
    #[error("failed to encode speech audio: {source}")]
    #[diagnostic(code(eliza::speech::mp3_encoding))]
    Mp3Encoding {
        /// Concrete codec failure.
        #[source]
        source: rusty_mp3::Error,
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
    #[error("speech worker failed: {source}")]
    #[diagnostic(code(eliza::speech::worker))]
    Worker {
        /// Blocking task failure.
        #[source]
        source: tokio::task::JoinError,
    },
}

impl ProblemDetails for SpeechError {
    fn class(&self) -> ProblemClass {
        match self {
            Self::InputTooLarge { .. } | Self::OutputTooLong => ProblemClass::RequestTooLarge,
            Self::EmptyInput
            | Self::InvalidSpeed
            | Self::UnsupportedCharacter { .. }
            | Self::UnsupportedSampleRate { .. } => ProblemClass::InvalidRequest,
            Self::Mp3Encoding { .. }
            | Self::Mp3NoFrames
            | Self::Unavailable
            | Self::Worker { .. } => ProblemClass::Internal,
        }
    }

    fn param(&self) -> Option<&'static str> {
        match self {
            Self::EmptyInput | Self::InputTooLarge { .. } | Self::UnsupportedCharacter { .. } => {
                Some("input")
            }
            Self::InvalidSpeed => Some("speed"),
            Self::UnsupportedSampleRate { .. } => Some("sample_rate"),
            Self::Mp3Encoding { .. }
            | Self::Mp3NoFrames
            | Self::OutputTooLong
            | Self::Unavailable
            | Self::Worker { .. } => None,
        }
    }
}
