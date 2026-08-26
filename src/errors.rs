//! Internal diagnostics for startup, CLI validation, and script loading.
//!
//! Provider request failures are deliberately not modeled here; those are
//! rendered as OpenAI/Anthropic/Gemini-compatible HTTP error bodies.

use std::net::SocketAddr;

use miette::Diagnostic;
use thiserror::Error;

/// Internal startup, configuration, and script-loading error surface.
#[derive(Debug, Diagnostic, Error)]
pub(super) enum AppError {
    /// Bearer auth token was provided as an empty CLI value.
    #[error("bearer token must not be empty")]
    #[diagnostic(code(eliza::config::empty_bearer_token))]
    EmptyBearerToken,

    /// Provider-visible model id was empty or whitespace-only.
    #[error("model id must not be empty")]
    #[diagnostic(code(eliza::config::empty_model_id))]
    EmptyModelId,

    /// Script parser reached EOF while still expecting a token or close paren.
    #[error("unexpected end of script at {line}:{column}")]
    #[diagnostic(code(eliza::script::unexpected_end))]
    ScriptUnexpectedEnd {
        /// One-based source line where the parser stopped.
        line: usize,
        /// One-based source column where the parser stopped.
        column: usize,
    },

    /// Script parser saw a close paren without a matching open paren.
    #[error("unexpected closing parenthesis at {line}:{column}")]
    #[diagnostic(code(eliza::script::unexpected_close))]
    ScriptUnexpectedClose {
        /// One-based source line containing the unmatched delimiter.
        line: usize,
        /// One-based source column containing the unmatched delimiter.
        column: usize,
    },

    /// Script lowering found the wrong S-expression shape for a known form.
    #[error("expected {expected} at {line}:{column}")]
    #[diagnostic(code(eliza::script::expected))]
    ScriptExpected {
        /// Human-readable grammar element required at this position.
        expected: &'static str,
        /// One-based source line containing the invalid form.
        line: usize,
        /// One-based source column containing the invalid form.
        column: usize,
    },

    /// Script did not provide the greeting form before `START`.
    #[error("script is missing a greeting at {line}:{column}")]
    #[diagnostic(code(eliza::script::missing_greeting))]
    ScriptMissingGreeting {
        /// One-based source line at which lowering detected the omission.
        line: usize,
        /// One-based source column at which lowering detected the omission.
        column: usize,
    },

    /// Bearer auth mode was selected without a bearer token.
    #[error("--auth bearer requires --bearer-token")]
    #[diagnostic(
        code(eliza::serve::missing_bearer_token),
        help("pass --bearer-token or use --auth none")
    )]
    MissingBearerToken,

    /// Numeric request limit flag was set to zero.
    #[error("--{flag} must be greater than zero")]
    #[diagnostic(code(eliza::serve::zero_limit))]
    ZeroLimit {
        /// CLI flag whose value must be positive.
        flag: &'static str,
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
