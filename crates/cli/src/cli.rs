//! CLI contract and conversion into server configuration.

use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;

use clap::parser::ValueSource;
use clap::{ArgMatches, Args, Parser, Subcommand, ValueEnum};
use eliza_http::context::{ApiKey, RequestLimits, RouteConfig};
use eliza_http::hosted::{Hosted, HostedConfig, HostedSecrets};
use eliza_http::model::{ModelAliases, ModelId, ModelNames};
use eliza_server::serve::{CorsMode, LogFormat, ServerConfig};
use figment::Figment;
use figment::providers::{Format as _, Serialized, Toml};
use figment::value::Dict;
use http::HeaderValue;
use serde::{Deserialize, Serialize};
use url::Url;

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
// CliCorsMode: Selects browser cross-origin access.
// -----------------------------------------------------------------------------

/// Browser cross-origin access policy.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
enum CliCorsMode {
    /// Disable cross-origin browser access.
    Off,
    /// Allow every browser origin.
    Any,
    /// Allow only configured browser origins.
    Origins,
}

/// Parse an exact HTTP origin into a safe response-header value.
///
/// # Errors
/// Returns a diagnostic for a URL with an unsupported scheme or extra parts.
fn parse_cors_origin(value: &str) -> Result<HeaderValue, String> {
    let url = Url::parse(value).map_err(|_| format!("invalid CORS origin `{value}`"))?;

    // Browser origins contain only an HTTP(S) scheme, host, and optional port.
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(format!("CORS origin must be an HTTP(S) origin: `{value}`"));
    }

    // Compare canonical values against the browser's serialized Origin header.
    HeaderValue::from_str(&url.origin().ascii_serialization())
        .map_err(|_| format!("invalid CORS origin `{value}`"))
}

// -----------------------------------------------------------------------------
// CliLogFormat: Parses the CLI and TOML logging format.
// -----------------------------------------------------------------------------

/// Supported process log formats.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
enum CliLogFormat {
    /// Human-readable tracing events.
    Text,
    /// Structured JSON tracing events.
    Json,
}

impl From<CliLogFormat> for LogFormat {
    fn from(format: CliLogFormat) -> Self {
        match format {
            CliLogFormat::Text => Self::Text,
            CliLogFormat::Json => Self::Json,
        }
    }
}

// -----------------------------------------------------------------------------
// ServeArgs: Captures CLI-visible server options.
// -----------------------------------------------------------------------------

/// CLI-visible server options.
#[derive(Debug, Clone, Args, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
    /// TOML file supplying server options below explicit CLI flags.
    #[arg(long = "config", value_name = "PATH", help_heading = "Configuration")]
    #[serde(skip)]
    config: Option<PathBuf>,

    /// IP address and port on which to listen.
    #[arg(
        long,
        value_name = "IP:PORT",
        default_value = "127.0.0.1:8787",
        help_heading = "Listener"
    )]
    bind: SocketAddr,

    /// Number of async runtime threads (default: Tokio automatic sizing).
    #[arg(long, value_name = "THREADS", help_heading = "Workers", value_parser = parse_positive_usize)]
    pub(super) runtime_workers: Option<NonZeroUsize>,

    /// Maximum simultaneous speech jobs across all providers.
    #[arg(long, value_name = "JOBS", default_value = "2", help_heading = "Workers", value_parser = parse_speech_workers)]
    speech_workers: NonZeroUsize,

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

    /// Hosted admission policy supplied by the TOML configuration file.
    #[arg(skip)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hosted: Option<HostedConfig>,

    /// Browser cross-origin access policy.
    #[arg(
        long = "cors-mode",
        value_name = "MODE",
        value_enum,
        default_value = "off",
        help_heading = "Security"
    )]
    cors_mode: CliCorsMode,

    /// Browser origin allowed when CORS mode is origins (repeatable).
    #[arg(long = "cors-origin", value_name = "ORIGIN", help_heading = "Security")]
    cors_origins: Vec<String>,

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

    /// Format tracing events as text or JSON.
    #[arg(
        long = "log-format",
        value_name = "FORMAT",
        value_enum,
        default_value = "text",
        help_heading = "Logging"
    )]
    log_format: CliLogFormat,
}

impl ServeArgs {
    /// Parse configured browser origins into response-header values.
    ///
    /// # Errors
    /// Returns a diagnostic for an invalid origin value.
    fn listed_cors_origins(&self) -> miette::Result<Vec<HeaderValue>> {
        self.cors_origins
            .iter()
            .map(|value| parse_cors_origin(value).map_err(|message| miette::miette!("{message}")))
            .collect()
    }

    /// Build the validated browser CORS policy from CLI or TOML values.
    ///
    /// # Errors
    /// Returns a diagnostic when a listed origin is not an HTTP(S) origin.
    fn cors_policy(&self) -> miette::Result<CorsMode> {
        match self.cors_mode {
            CliCorsMode::Off => Ok(CorsMode::Off),
            CliCorsMode::Any => Ok(CorsMode::Any),
            CliCorsMode::Origins => self.listed_cors_origins().map(CorsMode::Origins),
        }
    }

    /// Lower parsed arguments into runtime configuration.
    ///
    /// # Errors
    /// Rejects missing secrets and invalid hosted deployment policy.
    ///
    /// # Panics
    /// Panics if constructed without a chat name; Clap supplies its default.
    pub(super) fn into_server_config(self) -> miette::Result<ServerConfig> {
        let cors = self.cors_policy()?;
        let hosted = if let Some(config) = self.hosted {
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
            .with_model_aliases(models)
            .with_speech_workers(self.speech_workers);

        // Apply the process-wide logging policy.
        let log = self.log_format.into();

        let server = ServerConfig::new(self.bind, routes, cors, log);
        Ok(if let Some(hosted) = hosted {
            server.with_hosted(hosted)
        } else {
            server
        })
    }

    /// Check constraints that Clap cannot see in file-sourced values.
    ///
    /// # Errors
    /// Rejects conflicting security settings and invalid model or worker counts.
    fn validate_config(&self) -> miette::Result<()> {
        // An allowlist without origins cannot admit browser clients.
        if matches!(self.cors_mode, CliCorsMode::Origins) && self.cors_origins.is_empty() {
            return Err(miette::miette!(
                "cors_mode = 'origins' requires cors_origins"
            ));
        }

        // Other modes cannot silently ignore supplied origins.
        if !matches!(self.cors_mode, CliCorsMode::Origins) && !self.cors_origins.is_empty() {
            return Err(miette::miette!(
                "cors_origins require cors_mode = 'origins'"
            ));
        }

        // Clap sees only flags, so enforce this conflict after merging the file.
        if self.api_key.is_some() && self.hosted.is_some() {
            return Err(miette::miette!("api_key and hosted cannot both be set"));
        }

        // File values must fit the same semaphore bound as CLI values.
        if self.speech_workers.get() > tokio::sync::Semaphore::MAX_PERMITS {
            return Err(miette::miette!("exceeds maximum speech worker count"));
        }

        // RouteConfig needs a primary chat name for Rust callers.
        if self.chat_models.is_empty() {
            return Err(miette::miette!("chat_models must not be empty"));
        }
        Ok(())
    }

    /// Merge built-in defaults, an optional TOML file, and explicit CLI flags.
    ///
    /// # Errors
    /// Returns a diagnostic for unreadable or invalid configuration.
    ///
    /// # Panics
    /// Panics only if the built-in Clap declaration cannot parse its own defaults.
    pub(super) fn with_config(self, matches: &ArgMatches) -> miette::Result<Self> {
        // Keep the existing CLI-only path when no file is requested.
        let Some(path) = &self.config else {
            self.validate_config()?;
            return Ok(self);
        };

        let Commands::Serve(defaults) = Cli::try_parse_from(["eliza", "serve"])
            .expect("built-in serve defaults parse")
            .command;
        let mut explicit: Dict = Figment::from(Serialized::defaults(&self))
            .extract()
            .map_err(|error| miette::miette!("cannot read CLI options: {error}"))?;
        explicit.retain(|field, _| matches.value_source(field) == Some(ValueSource::CommandLine));

        // Apply file values over defaults, then apply only flags the caller set.
        let figment = Figment::from(Serialized::defaults(defaults))
            .merge(Toml::file_exact(path))
            .merge(Serialized::defaults(explicit));
        let args: Self = figment
            .extract()
            .map_err(|error| miette::miette!("invalid server configuration: {error}"))?;

        // Enforce cross-field limits after every source is combined.
        args.validate_config()?;
        Ok(args)
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
