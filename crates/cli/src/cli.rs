//! CLI contract and conversion into server configuration.

use std::net::SocketAddr;
use std::num::NonZeroUsize;

use clap::{Args, Parser, Subcommand};
use eliza_http::context::{ApiKey, RequestLimits, RouteConfig};
use eliza_http::model::ModelId;
use eliza_server::serve::{CorsMode, LogFormat, ServerConfig};

// -----------------------------------------------------------------------------
// ParsePositiveUsize: Parses request bounds without Rust type terminology.
// -----------------------------------------------------------------------------

/// Parse one positive CLI integer into its domain type.
///
/// # Errors
///
/// Returns a readable parser error when the value is zero or not an integer.
fn parse_positive_usize(value: &str) -> Result<NonZeroUsize, String> {
    let value = value.parse::<usize>().map_err(|error| error.to_string())?;
    NonZeroUsize::new(value).ok_or_else(|| "must be greater than zero".to_owned())
}

// -----------------------------------------------------------------------------
// ServeArgs: Captures CLI-visible server options.
// -----------------------------------------------------------------------------

/// CLI-visible server options.
#[derive(Debug, Clone, Args)]
#[command(after_long_help = "\
Built-in modality models:
  Chat:       eliza-1966 (configurable with --chat-model)
  Speech:     flite (fixed)
  Embeddings: fnv-embed (fixed)

Explore every provider route at /docs or /openapi.json.
")]
pub(super) struct ServeArgs {
    /// IP address and port on which to listen.
    #[arg(
        long,
        value_name = "IP:PORT",
        default_value = "127.0.0.1:8787",
        help_heading = "Listener"
    )]
    bind: SocketAddr,

    /// Chat model ID advertised by provider catalogs.
    #[arg(
        long,
        value_name = "ID",
        default_value = "eliza-1966",
        help_heading = "Chat"
    )]
    chat_model: ModelId,

    /// Maximum chat transcript entries accepted per request.
    #[arg(
        long,
        value_name = "MESSAGES",
        default_value = "200",
        help_heading = "Chat",
        value_parser = parse_positive_usize
    )]
    max_history_messages: NonZeroUsize,

    /// Require this key through each provider's native authentication scheme.
    #[arg(long, value_name = "KEY", help_heading = "Security")]
    api_key: Option<ApiKey>,

    /// Allow browser requests from any origin using permissive CORS.
    #[arg(long = "allow-any-origin", help_heading = "Security")]
    should_allow_any_origin: bool,

    /// Maximum combined text accepted per request, across every modality.
    #[arg(
        long,
        value_name = "CHARS",
        default_value = "8000",
        help_heading = "Requests",
        value_parser = parse_positive_usize
    )]
    max_input_chars: NonZeroUsize,

    /// Delay between chunks for every streaming response.
    #[arg(
        long,
        value_name = "MS",
        default_value = "0",
        help_heading = "Streaming"
    )]
    stream_delay_ms: u64,

    /// Render tracing events as JSON instead of human-readable text.
    #[arg(long = "json-logs", help_heading = "Logging")]
    should_use_json_logs: bool,
}

impl ServeArgs {
    /// Lower parsed arguments into runtime configuration.
    #[must_use]
    pub(super) fn into_server_config(self) -> ServerConfig {
        let limits = RequestLimits::new(self.max_input_chars, self.max_history_messages);

        // Assemble route-wide behavior from parsed domain values.
        let routes = RouteConfig::new(self.chat_model, self.api_key, self.stream_delay_ms, limits);

        // Translate simple CLI switches into explicit server modes.
        let cors = if self.should_allow_any_origin {
            CorsMode::Permissive
        } else {
            CorsMode::None
        };
        let log = if self.should_use_json_logs {
            LogFormat::Json
        } else {
            LogFormat::Text
        };

        ServerConfig::new(self.bind, routes, cors, log)
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
