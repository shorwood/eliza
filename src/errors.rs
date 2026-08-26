//! Internal diagnostics for startup, CLI validation, and script loading.
//!
//! Provider request failures are deliberately not modeled here; those are
//! rendered as OpenAI/Anthropic/Gemini-compatible HTTP error bodies.

use std::net::SocketAddr;

use miette::Diagnostic;
use thiserror::Error;

/// Internal startup, configuration, and script-loading error surface.
#[derive(Debug, Diagnostic, Error)]
pub enum AppError {
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
    ScriptUnexpectedEnd { line: usize, column: usize },

    /// Script parser saw a close paren without a matching open paren.
    #[error("unexpected closing parenthesis at {line}:{column}")]
    #[diagnostic(code(eliza::script::unexpected_close))]
    ScriptUnexpectedClose { line: usize, column: usize },

    /// Script lowering found the wrong S-expression shape for a known form.
    #[error("expected {expected} at {line}:{column}")]
    #[diagnostic(code(eliza::script::expected))]
    ScriptExpected {
        expected: &'static str,
        line: usize,
        column: usize,
    },

    /// Script did not provide the greeting form before `START`.
    #[error("script is missing a greeting at {line}:{column}")]
    #[diagnostic(code(eliza::script::missing_greeting))]
    ScriptMissingGreeting { line: usize, column: usize },

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
    ZeroLimit { flag: &'static str },

    /// TCP listener failed to bind.
    #[error("failed to bind {addr}: {source}")]
    #[diagnostic(code(eliza::serve::bind))]
    Bind {
        addr: SocketAddr,
        #[source]
        source: std::io::Error,
    },

    /// Axum server returned an IO runtime error.
    #[error("server error: {0}")]
    #[diagnostic(code(eliza::serve::runtime))]
    Serve(#[source] std::io::Error),
}
