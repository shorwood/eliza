//! Diagnostics raised while lowering CLI arguments into server configuration.

use std::fmt;
use std::num::NonZeroUsize;

use miette::Diagnostic;
use thiserror::Error;

// -----------------------------------------------------------------------------
// ConfigError: Rejects invalid combinations of otherwise parsed options.
// -----------------------------------------------------------------------------

/// CLI-to-server configuration failure.
#[derive(Debug, Diagnostic, Error)]
pub(super) enum ConfigError {
    /// Numeric request limit flag was set to zero.
    #[error("--{limit} must be greater than zero")]
    #[diagnostic(code(eliza::serve::zero_limit))]
    ZeroLimit {
        /// Typed request limit whose CLI value must be positive.
        limit: RequestLimit,
    },
}

// -----------------------------------------------------------------------------
// RequestLimit: Names bounded request settings without stringly error context.
// -----------------------------------------------------------------------------

/// Request limit configured by one CLI flag.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum RequestLimit {
    /// Maximum accepted conversation messages.
    HistoryMessages,
    /// Maximum accepted input characters.
    InputChars,
}

impl RequestLimit {
    /// Convert a raw CLI value into its positive runtime representation.
    ///
    /// # Errors
    ///
    /// Returns a configuration diagnostic when `value` is zero.
    pub(super) fn validate(self, value: usize) -> Result<NonZeroUsize, ConfigError> {
        NonZeroUsize::new(value).ok_or(ConfigError::ZeroLimit { limit: self })
    }
}

impl fmt::Display for RequestLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::HistoryMessages => "max-history-messages",
            Self::InputChars => "max-input-chars",
        })
    }
}
