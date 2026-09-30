//! CLI contract and conversion into server configuration.

use std::net::{IpAddr, SocketAddr};

use clap::{Parser, Subcommand, ValueEnum};
use eliza_http::context::{AuthMode as RouteAuthMode, BearerToken, RouteConfig};
use eliza_http::model::ModelId;
use eliza_http::response::RequestLimits;
use eliza_server::serve::{CorsMode as ServerCorsMode, LogFormat as ServerLogFormat, ServerConfig};

use crate::errors::{ConfigError, RequestLimit};

// -----------------------------------------------------------------------------
// AuthMode: Controls provider endpoint authentication.
// -----------------------------------------------------------------------------

/// Authentication mode for provider-compatible endpoints.
#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
pub(super) enum AuthMode {
    /// Do not require provider authentication.
    None,
    /// Require each provider's native bearer or API-key header.
    Bearer,
}

impl From<AuthMode> for RouteAuthMode {
    fn from(mode: AuthMode) -> Self {
        match mode {
            AuthMode::None => Self::None,
            AuthMode::Bearer => Self::Bearer,
        }
    }
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

impl From<CorsMode> for ServerCorsMode {
    fn from(mode: CorsMode) -> Self {
        match mode {
            CorsMode::None => Self::None,
            CorsMode::Permissive => Self::Permissive,
        }
    }
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

impl From<LogFormat> for ServerLogFormat {
    fn from(format: LogFormat) -> Self {
        match format {
            LogFormat::Text => Self::Text,
            LogFormat::Json => Self::Json,
        }
    }
}

// -----------------------------------------------------------------------------
// ServeArgs: Captures CLI-visible server options.
// -----------------------------------------------------------------------------

/// CLI-visible server options.
#[derive(Debug, Clone, Parser)]
#[command(after_long_help = "\
Provider paths:
  OpenAI:    GET /openai/v1/models, POST /openai/v1/chat/completions, POST /openai/v1/responses, POST /openai/v1/audio/speech
  Gemini OA: POST /gemini/v1beta/openai/chat/completions
  Anthropic: GET /anthropic/v1/models, POST /anthropic/v1/messages
  Gemini:    GET /gemini/v1beta/models, POST /gemini/v1beta/models/{model}:generateContent, POST /gemini/v1beta/models/{model}:streamGenerateContent
  Ollama:    GET /ollama/api/tags, POST /ollama/api/chat
  Docs:      GET /docs, GET /openapi.json
")]
pub(super) struct ServeArgs {
    /// Bind address.
    #[arg(long, default_value = "127.0.0.1")]
    host: IpAddr,

    /// Bind port.
    #[arg(long, default_value = "8787")]
    port: u16,

    /// Provider-visible model id.
    #[arg(long, default_value = "eliza-1966")]
    model: ModelId,

    /// Authentication mode for provider endpoints.
    #[arg(long, value_enum, default_value = "none")]
    auth: AuthMode,

    /// Required token when --auth bearer is used.
    #[arg(long)]
    bearer_token: Option<BearerToken>,

    /// CORS behavior.
    #[arg(long, value_enum, default_value = "none")]
    cors: CorsMode,

    /// Optional delay between streaming chunks.
    #[arg(long, default_value = "0")]
    stream_delay_ms: u64,

    /// Reject requests whose combined input content exceeds this character count.
    #[arg(long, default_value = "8000")]
    max_input_chars: usize,

    /// Bound replay work by limiting the number of user turns accepted.
    #[arg(long, default_value = "200")]
    max_history_messages: usize,

    /// Log rendering mode.
    #[arg(long, value_enum, default_value = "text")]
    log: LogFormat,
}

impl ServeArgs {
    /// Validate cross-option invariants and lower into runtime configuration.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic when bearer authentication has no token or a
    /// request bound is zero.
    #[expect(
        rlib::long_method_chains,
        reason = "the Bon builder is one declarative route configuration"
    )]
    pub(super) fn into_server_config(self) -> Result<ServerConfig, ConfigError> {
        // Reject bearer mode before constructing unusable shared route state.
        if self.auth == AuthMode::Bearer && self.bearer_token.is_none() {
            return Err(ConfigError::MissingBearerToken);
        }

        let max_input_chars = RequestLimit::InputChars.validate(self.max_input_chars)?;
        let max_history_messages =
            RequestLimit::HistoryMessages.validate(self.max_history_messages)?;
        let limits = RequestLimits::builder()
            .max_input_chars(max_input_chars)
            .max_history_messages(max_history_messages)
            .build();

        let routes = RouteConfig::builder()
            .model(self.model)
            .auth(self.auth.into())
            .bearer_token(self.bearer_token)
            .stream_delay_ms(self.stream_delay_ms)
            .limits(limits)
            .build();

        Ok(ServerConfig::new(
            SocketAddr::new(self.host, self.port),
            routes,
            self.cors.into(),
            self.log.into(),
        ))
    }
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
#[derive(Debug, Parser)]
#[command(name = "eliza")]
#[command(
    about = "Serve classic ELIZA through OpenAI, Anthropic, Gemini, and Ollama-compatible HTTP APIs.",
    long_about = None
)]
pub(super) struct Cli {
    /// Selected command.
    #[command(subcommand)]
    pub(super) command: Commands,
}
