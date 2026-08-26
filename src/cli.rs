//! CLI contract for the standalone binary.
//!
//! `clap` is the only runtime configuration surface. The parsed `ServeArgs`
//! lower into `ServerConfig`; there are no Nano config files, hidden env-only
//! switches, or route-surface toggles.

use std::net::IpAddr;
use std::str::FromStr;

use clap::{Parser, Subcommand, ValueEnum};

use crate::errors::AppError;
use crate::provider::contracts::ModelId;

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

// -----------------------------------------------------------------------------
// Tests: Verify CLI parsing and configuration lowering.
// -----------------------------------------------------------------------------

/// Checks ELIZA command parsing behavior at its source owner.
#[cfg(test)]
#[allow(
    clippy::missing_panics_doc,
    reason = "test rationales replace public panic contracts"
)]
mod tests {
    // -------------------------------------------------------------------------
    // Commands: Verifies command parsing and configuration lowering.
    // -------------------------------------------------------------------------

    /// Checks the commands contract.
    mod commands {
        use clap::Parser;

        use crate::cli::{Cli, Commands};
        use crate::provider::contracts::ModelId;
        use crate::serve::ServerConfig;

        #[test]
        fn it_should_help_succeeds() {
            Cli::try_parse_from(["eliza", "--help"]).expect_err("help exits through clap display");
        }

        #[test]
        fn it_should_serve_help_lists_provider_paths() {
            let help = Cli::try_parse_from(["eliza", "serve", "--help"])
                .expect_err("help exits through clap display")
                .to_string();
            assert!(help.contains("/v1/chat/completions"));
            assert!(help.contains("/v1/messages"));
            assert!(help.contains(":generateContent"));
            assert!(!help.contains("--compat"));
            assert!(!help.contains("--state"));
        }

        #[test]
        fn it_should_bearer_auth_requires_token() {
            let cli = Cli::parse_from(["eliza", "serve", "--auth", "bearer"]);
            let Commands::Serve(args) = cli.command;
            let error = ServerConfig::try_from(args).expect_err("bearer without token should fail");
            assert_eq!(error.to_string(), "--auth bearer requires --bearer-token");
        }

        #[test]
        fn it_should_bearer_token_rejects_empty_cli_value() {
            let error = Cli::try_parse_from(["eliza", "serve", "--bearer-token", ""])
                .expect_err("empty token should fail");
            assert!(error.to_string().contains("bearer token must not be empty"));
        }

        #[test]
        fn it_should_model_id_rejects_empty_cli_value() {
            let error = Cli::try_parse_from(["eliza", "serve", "--model", ""])
                .expect_err("empty model id should fail");
            assert!(error.to_string().contains("model id must not be empty"));
        }

        #[test]
        fn it_should_model_id_deserializes_through_parser() {
            let error = serde_json::from_str::<ModelId>("\"\"")
                .expect_err("empty JSON model id should fail");
            assert!(error.to_string().contains("model id must not be empty"));
        }

        #[test]
        fn it_should_request_limits_must_be_non_zero() {
            let cli = Cli::parse_from(["eliza", "serve", "--max-input-chars", "0"]);
            let Commands::Serve(args) = cli.command;
            let error = ServerConfig::try_from(args).expect_err("zero max input chars should fail");
            assert_eq!(
                error.to_string(),
                "--max-input-chars must be greater than zero"
            );
            let cli = Cli::parse_from(["eliza", "serve", "--max-history-messages", "0"]);
            let Commands::Serve(args) = cli.command;
            let error =
                ServerConfig::try_from(args).expect_err("zero max history messages should fail");
            assert_eq!(
                error.to_string(),
                "--max-history-messages must be greater than zero"
            );
        }

        #[test]
        fn it_should_manifest_stays_dependency_isolated_from_nano() {
            let manifest = include_str!("../Cargo.toml");
            assert!(!manifest.contains("workspace = true"));
            assert!(!manifest.contains("path = \".."));
            assert!(!manifest.contains("nano-"));
        }
    }
}
