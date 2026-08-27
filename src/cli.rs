//! CLI contract for the standalone binary.
//!
//! `clap` is the only runtime configuration surface. The parsed `ServeArgs`
//! lower into `ServerConfig`; there are no Nano config files, hidden env-only
//! switches, or route-surface toggles.

use std::net::IpAddr;
use std::str::FromStr;

use clap::{Parser, Subcommand, ValueEnum};

use crate::errors::AppError;
use crate::types::model::ModelId;

// -----------------------------------------------------------------------------
// AuthMode: Controls provider endpoint authentication.
// -----------------------------------------------------------------------------

/// Authentication mode for provider-compatible endpoints.
#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
pub(super) enum AuthMode {
    /// Do not require provider authentication.
    None,
    /// Accept `Authorization: Bearer` or `x-api-key`.
    Bearer,
}

// -----------------------------------------------------------------------------
// CorsMode: Controls browser cross-origin access.
// -----------------------------------------------------------------------------

/// CORS policy for browser clients.
#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
pub(super) enum CorsMode {
    /// Do not install a CORS layer.
    None,
    /// Install Axum's permissive CORS layer.
    Permissive,
}

// -----------------------------------------------------------------------------
// LogFormat: Controls tracing event rendering.
// -----------------------------------------------------------------------------

/// Log rendering mode.
#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
pub(super) enum LogFormat {
    /// Human-readable tracing output.
    Text,
    /// JSON tracing output.
    Json,
}

// -----------------------------------------------------------------------------
// BearerToken: Semantic CLI value that validates once and redacts debug output.
// -----------------------------------------------------------------------------

/// Shared bearer/API-key token used when bearer auth is enabled.
///
/// ```
/// use eliza::cli::BearerToken;
///
/// let token: BearerToken = "secret".parse().unwrap();
/// assert_eq!(token.as_str(), "secret");
/// assert_eq!(format!("{token:?}"), "BearerToken(<redacted>)");
/// ```
#[derive(Clone, Eq, PartialEq)]
pub(super) struct BearerToken(
    /// Validated nonempty secret value.
    String,
);

impl BearerToken {
    /// Return the raw token value for constant-time-equivalent header comparison.
    ///
    /// ```
    /// use eliza::cli::BearerToken;
    ///
    /// let token: BearerToken = "local-dev".parse().unwrap();
    /// assert_eq!(token.as_str(), "local-dev");
    /// ```
    #[must_use]
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BearerToken {
    type Error = AppError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            Err(AppError::EmptyBearerToken)
        } else {
            Ok(Self(value))
        }
    }
}

impl FromStr for BearerToken {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl AsRef<str> for BearerToken {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Debug for BearerToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BearerToken(<redacted>)")
    }
}

// -----------------------------------------------------------------------------
// ServeArgs: Captures CLI-visible server options.
// -----------------------------------------------------------------------------

/// CLI-visible server options.
#[derive(Debug, Clone, Parser)]
#[command(after_long_help = "\
Provider paths:
  OpenAI:    GET /v1/models, POST /v1/chat/completions, POST /v1/responses
  Gemini OA: POST /v1beta/openai/chat/completions
  Anthropic: POST /v1/messages
  Gemini:    GET /v1beta/models, POST /v1beta/models/{model}:generateContent, POST /v1beta/models/{model}:streamGenerateContent
  Docs:      GET /docs, GET /openapi.json
")]
pub(super) struct ServeArgs {
    /// Bind address.
    #[arg(long, default_value = "127.0.0.1")]
    pub(super) host: IpAddr,

    /// Bind port.
    #[arg(long, default_value = "8787")]
    pub(super) port: u16,

    /// Provider-visible model id.
    #[arg(long, default_value = "eliza-doctor")]
    pub(super) model: ModelId,

    /// Authentication mode for provider endpoints.
    #[arg(long, value_enum, default_value = "none")]
    pub(super) auth: AuthMode,

    /// Required token when --auth bearer is used.
    #[arg(long)]
    pub(super) bearer_token: Option<BearerToken>,

    /// CORS behavior.
    #[arg(long, value_enum, default_value = "none")]
    pub(super) cors: CorsMode,

    /// Optional delay between streaming chunks.
    #[arg(long, default_value = "0")]
    pub(super) stream_delay_ms: u64,

    /// Reject requests whose text content exceeds this character count.
    #[arg(long, default_value = "8000")]
    pub(super) max_input_chars: usize,

    /// Bound replay work by limiting the number of user turns accepted.
    #[arg(long, default_value = "200")]
    pub(super) max_history_messages: usize,

    /// Log rendering mode.
    #[arg(long, value_enum, default_value = "text")]
    pub(super) log: LogFormat,
}

// -----------------------------------------------------------------------------
// Commands: Selects the standalone binary operation.
// -----------------------------------------------------------------------------

/// Available binary commands.
#[derive(Debug, Subcommand)]
pub(super) enum Commands {
    /// Run the local provider-compatible ELIZA HTTP server.
    Serve(
        /// Validated server command arguments.
        ServeArgs,
    ),
}

// -----------------------------------------------------------------------------
// Cli: Owns top-level command parsing.
// -----------------------------------------------------------------------------

/// Top-level ELIZA CLI.
///
/// ```
/// use clap::Parser;
/// use eliza::cli::{Cli, Commands};
///
/// let cli = Cli::parse_from(["eliza", "serve", "--port", "8787"]);
/// assert!(matches!(cli.command, Commands::Serve(_)));
/// ```
#[derive(Debug, Parser)]
#[command(name = "eliza")]
#[command(
    about = "Serve classic ELIZA through OpenAI, Anthropic, and Gemini-compatible HTTP APIs."
)]
pub(super) struct Cli {
    /// Selected command.
    #[command(subcommand)]
    pub(super) command: Commands,
}
