//! Internal diagnostics for CLI validation and server startup.

use std::fmt;
use std::net::SocketAddr;

use miette::Diagnostic;
use thiserror::Error;

/// Internal startup and configuration error surface.
#[derive(Debug, Diagnostic, Error)]
pub(super) enum AppError {
    /// Bearer auth token was provided as an empty CLI value.
    #[error("bearer token must not be empty")]
    #[diagnostic(code(eliza::config::empty_bearer_token))]
    EmptyBearerToken,

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

    /// TCP listener failed to bind.
    #[error("failed to bind {addr}: {source}")]
    #[diagnostic(code(eliza::serve::bind))]
    Bind {
        /// Socket address the server attempted to bind.
        addr: SocketAddr,
        /// Operating-system error returned by the listener.
        #[source]
        source: std::io::Error,
    },

    /// Axum server returned an IO runtime error.
    #[error("server error: {0}")]
    #[diagnostic(code(eliza::serve::runtime))]
    Serve(
        /// Runtime I/O failure returned by Axum.
        #[source]
        std::io::Error,
    ),
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
