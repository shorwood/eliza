//! CLI contract and conversion into server configuration.

use std::net::SocketAddr;
use std::num::NonZeroUsize;

use clap::{Args, Parser, Subcommand};
use eliza_http::context::{ApiKey, RequestLimits, RouteConfig};
use eliza_http::model::{ModelAliases, ModelId, ModelNames};
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
  Chat:       eliza-1966
  Speech:     flite
  Embeddings: fnv-embed
  Images:     eliza-retro-image

Repeat --model-* options to advertise multiple names for a local engine.
Supplied names replace that modality's defaults.

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

    /// Chat model name advertised by provider catalogs (repeatable).
    #[arg(
        long = "model-chat",
        visible_alias = "chat-model",
        value_name = "ID",
        default_value = "eliza-1966",
        help_heading = "Models"
    )]
    chat_models: Vec<ModelId>,

    /// Embedding model name accepted by embedding routes (repeatable).
    #[arg(
        long = "model-embeddings",
        value_name = "ID",
        default_value = "fnv-embed",
        help_heading = "Models"
    )]
    embedding_models: Vec<ModelId>,

    /// Image model name accepted by image generation routes (repeatable).
    #[arg(
        long = "model-images",
        value_name = "ID",
        default_value = "eliza-retro-image",
        help_heading = "Models"
    )]
    image_models: Vec<ModelId>,

    /// Speech model name accepted by speech routes (repeatable).
    #[arg(
        long = "model-speech",
        value_name = "ID",
        default_value = "flite",
        help_heading = "Models"
    )]
    speech_models: Vec<ModelId>,

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
    ///
    /// # Panics
    /// Panics if constructed without a chat name; Clap supplies its default.
    #[must_use]
    pub(super) fn into_server_config(self) -> ServerConfig {
        let limits = RequestLimits::new(self.max_input_chars, self.max_history_messages);

        // Retain the first chat name for existing Rust configuration callers.
        let chat_model = self
            .chat_models
            .first()
            .expect("Clap supplies a chat name")
            .clone();

        // Collect independently configured names for each local engine.
        let models = ModelAliases {
            chat: ModelNames::new(self.chat_models),
            embeddings: ModelNames::new(self.embedding_models),
            images: ModelNames::new(self.image_models),
            speech: ModelNames::new(self.speech_models),
        };

        // Assemble route-wide behavior from parsed domain values.
        let routes = RouteConfig::new(chat_model, self.api_key, self.stream_delay_ms, limits)
            .with_model_aliases(models);

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
