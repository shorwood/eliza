//! CLI contract and conversion into server configuration.

use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
use eliza_http::context::{ApiKey, RequestLimits, RouteConfig};
use eliza_http::hosted::{Hosted, HostedConfig, HostedSecrets};
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
// ParseSpeechWorkers: Rejects limits the shared semaphore cannot represent.
// -----------------------------------------------------------------------------

/// Parse a positive speech job limit within Tokio's semaphore capacity.
///
/// # Errors
/// Returns an invalid integer or an unsupported semaphore capacity.
fn parse_speech_workers(value: &str) -> Result<NonZeroUsize, String> {
    let workers = parse_positive_usize(value)?;

    // Reject unsupported capacity before semaphore construction can panic.
    if workers.get() > tokio::sync::Semaphore::MAX_PERMITS {
        return Err("exceeds maximum speech worker count".to_owned());
    }
    Ok(workers)
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

    /// Number of async runtime threads (default: Tokio automatic sizing).
    #[arg(long, value_name = "THREADS", help_heading = "Workers", value_parser = parse_positive_usize)]
    pub(super) runtime_workers: Option<NonZeroUsize>,

    /// Maximum simultaneous speech jobs across all providers.
    #[arg(long, value_name = "JOBS", default_value = "2", help_heading = "Workers", value_parser = parse_speech_workers)]
    speech_workers: NonZeroUsize,

    /// Require this key through each provider's native authentication scheme.
    #[arg(long, value_name = "KEY", help_heading = "Security")]
    api_key: Option<ApiKey>,

    /// Enable explicit hosted admission using a JSON policy file.
    #[arg(
        long,
        value_name = "PATH",
        conflicts_with = "api_key",
        help_heading = "Security"
    )]
    hosted_config: Option<PathBuf>,

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
    ///
    /// # Errors
    /// Rejects missing secrets and invalid hosted deployment policy.
    pub(super) fn into_server_config(self) -> miette::Result<ServerConfig> {
        let hosted = if let Some(path) = self.hosted_config {
            let bytes = std::fs::read(path)
                .map_err(|_| miette::miette!("cannot read hosted configuration"))?;
            let config: HostedConfig = serde_json::from_slice(&bytes)
                .map_err(|_| miette::miette!("invalid hosted JSON configuration"))?;
            let ingress = std::env::var("ELIZA_INGRESS_SECRET")
                .map_err(|_| miette::miette!("ELIZA_INGRESS_SECRET is required"))?;
            let entitlement = std::env::var("ELIZA_ENTITLEMENT_SECRET")
                .map_err(|_| miette::miette!("ELIZA_ENTITLEMENT_SECRET is required"))?;
            Some(Arc::new(
                Hosted::new(
                    config,
                    HostedSecrets {
                        ingress_secret: ingress,
                        entitlement_secret: entitlement,
                    },
                )
                .map_err(|message| miette::miette!("{message}"))?,
            ))
        } else {
            None
        };
        let limits = RequestLimits::new(self.max_input_chars, self.max_history_messages);

        // Assemble route-wide behavior from parsed domain values.
        let routes = RouteConfig::new(self.chat_model, self.api_key, self.stream_delay_ms, limits)
            .with_speech_workers(self.speech_workers);

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

        let server = ServerConfig::new(self.bind, routes, cors, log);
        Ok(if let Some(hosted) = hosted {
            server.with_hosted(hosted)
        } else {
            server
        })
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
