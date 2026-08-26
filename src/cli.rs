//! CLI contract for the standalone binary.
//!
//! `clap` is the only runtime configuration surface. The parsed `ServeArgs`
//! lower into `ServerConfig`; there are no Nano config files, hidden env-only
//! switches, or route-surface toggles.

use std::net::IpAddr;
use std::str::FromStr;

use clap::{Parser, Subcommand, ValueEnum};

use crate::ModelId;
use crate::errors::AppError;

// -----------------------------------------------------------------------------
// CLI value enums: stable operator vocabulary for server behavior.
// -----------------------------------------------------------------------------

/// Authentication mode for provider-compatible endpoints.
#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
pub enum AuthMode {
    /// Do not require provider authentication.
    None,
    /// Accept `Authorization: Bearer` or `x-api-key`.
    Bearer,
}

/// CORS policy for browser clients.
#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
pub enum CorsMode {
    /// Do not install a CORS layer.
    None,
    /// Install Axum's permissive CORS layer.
    Permissive,
}

/// Log rendering mode.
#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
pub enum LogFormat {
    /// Human-readable tracing output.
    Text,
    /// JSON tracing output.
    Json,
}

// -----------------------------------------------------------------------------
// Bearer token: semantic CLI value that validates once and redacts debug output.
// -----------------------------------------------------------------------------

/// Shared bearer/API-key token used when bearer auth is enabled.
///
/// ```
/// use eliza::serve::BearerToken;
///
/// let token: BearerToken = "secret".parse().unwrap();
/// assert_eq!(token.as_str(), "secret");
/// assert_eq!(format!("{token:?}"), "BearerToken(<redacted>)");
/// ```
#[derive(Clone, Eq, PartialEq)]
pub struct BearerToken(String);

impl BearerToken {
    /// Return the raw token value for constant-time-equivalent header comparison.
    ///
    /// ```
    /// use eliza::serve::BearerToken;
    ///
    /// let token: BearerToken = "local-dev".parse().unwrap();
    /// assert_eq!(token.as_str(), "local-dev");
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
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
// Command graph: one binary command exposes all provider-compatible routes.
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
pub struct ServeArgs {
    /// Bind address.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: IpAddr,

    /// Bind port.
    #[arg(long, default_value_t = 8787)]
    pub port: u16,

    /// Provider-visible model id.
    #[arg(long, default_value = "eliza-doctor")]
    pub model: ModelId,

    /// Authentication mode for provider endpoints.
    #[arg(long, value_enum, default_value_t = AuthMode::None)]
    pub auth: AuthMode,

    /// Required token when --auth bearer is used.
    #[arg(long)]
    pub bearer_token: Option<BearerToken>,

    /// CORS behavior.
    #[arg(long, value_enum, default_value_t = CorsMode::None)]
    pub cors: CorsMode,

    /// Optional delay between streaming chunks.
    #[arg(long, default_value_t = 0)]
    pub stream_delay_ms: u64,

    /// Reject requests whose text content exceeds this character count.
    #[arg(long, default_value_t = 8000)]
    pub max_input_chars: usize,

    /// Bound replay work by limiting the number of user turns accepted.
    #[arg(long, default_value_t = 200)]
    pub max_history_messages: usize,

    /// Log rendering mode.
    #[arg(long, value_enum, default_value_t = LogFormat::Text)]
    pub log: LogFormat,
}

/// Available binary commands.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Run the local provider-compatible ELIZA HTTP server.
    Serve(ServeArgs),
}

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
pub struct Cli {
    /// Selected command.
    #[command(subcommand)]
    pub command: Commands,
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

/// Checks ELIZA command parsing behavior at its source owner.
#[cfg(test)]
#[allow(
    clippy::missing_panics_doc,
    reason = "test rationales replace public panic contracts"
)]
mod tests {
    /// Holds request drivers and setup without copying production logic.
    mod support {
        /// Supplies setup for the commands checks.
        pub(super) mod commands {
            #![allow(
                clippy::missing_panics_doc,
                reason = "test assertions panic to report failures"
            )]
            pub(crate) use crate::ModelId;
            pub(crate) use crate::cli::{Cli, Commands};
            pub(crate) use crate::serve::ServerConfig;
            pub(crate) use clap::Parser;
        }
    }

    /// Checks the commands contract.
    mod commands {
        use super::support::commands::*;

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
