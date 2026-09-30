//! Diagnostics raised while lowering CLI arguments into server configuration.

use std::fmt;

use miette::Diagnostic;
use thiserror::Error;

// -----------------------------------------------------------------------------
// ConfigError: Rejects invalid combinations of otherwise parsed options.
// -----------------------------------------------------------------------------

/// CLI-to-server configuration failure.
#[derive(Debug, Diagnostic, Error)]
pub(super) enum ConfigError {
    /// Bearer auth mode was selected without a bearer token.
    #[error("--auth bearer requires --bearer-token")]
    #[diagnostic(
        code(eliza::serve::missing_bearer_token),
        help("pass --bearer-token or use --auth none")
    )]
    MissingBearerToken,

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

impl fmt::Display for RequestLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::HistoryMessages => "max-history-messages",
            Self::InputChars => "max-input-chars",
        })
    }
}
