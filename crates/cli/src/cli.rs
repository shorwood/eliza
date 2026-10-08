//! CLI contract and conversion into server configuration.

use std::net::SocketAddr;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;

use clap::{ArgMatches, Args, Parser, Subcommand, parser::ValueSource};
use eliza_http::context::{ApiKey, RequestLimits, RouteConfig};
use eliza_http::hosted::{Hosted, HostedConfig, HostedSecrets};
use eliza_http::model::{ModelAliases, ModelId, ModelNames};
use eliza_server::serve::{CorsMode, LogFormat, ServerConfig};
use figment::{
    Figment,
    providers::{Format as _, Serialized, Toml},
    value::Dict,
};
use serde::{Deserialize, Serialize};

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
    config_file: Option<PathBuf>,

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
    ///
    /// # Panics
    /// Panics if constructed without a chat name; Clap supplies its default.
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

    /// Check constraints that Clap cannot see in file-sourced values.
    ///
    /// # Errors
    /// Rejects conflicting security settings and invalid model or worker counts.
    fn validate_config(&self) -> miette::Result<()> {
        // Clap sees only flags, so enforce this conflict after merging the file.
        if self.api_key.is_some() && self.hosted_config.is_some() {
            return Err(miette::miette!(
                "api_key and hosted_config cannot both be set"
            ));
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
        let Some(path) = &self.config_file else {
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

#[cfg(test)]
mod tests {
    #![expect(
        clippy::missing_errors_doc,
        clippy::missing_panics_doc,
        reason = "configuration tests use assertions and temporary files"
    )]

    use std::num::NonZeroUsize;

    use clap::{CommandFactory as _, FromArgMatches as _};

    use super::{ApiKey, Cli, Commands, ServeArgs};

    /// Load a temporary TOML file through the same Clap and Figment path as main.
    fn load(file: &str, flags: &[&str]) -> miette::Result<ServeArgs> {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("serve.toml");
        std::fs::write(&path, file).unwrap();
        let args = ["eliza", "serve", "--config", path.to_str().unwrap()]
            .into_iter()
            .chain(flags.iter().copied());
        let matches = Cli::command().try_get_matches_from(args).unwrap();
        let Commands::Serve(parsed) = Cli::from_arg_matches(&matches).unwrap().command;
        parsed.with_config(matches.subcommand_matches("serve").unwrap())
    }

    /// File values override built-ins, while explicit flags replace file values.
    #[test]
    fn config_precedence() {
        let file = "bind = '127.0.0.1:0'\nruntime_workers = 1\nspeech_workers = 4\nchat_models = ['file-chat']\nshould_allow_any_origin = true\napi_key = 'secret'\n";
        let from_file = load(file, &[]).unwrap();
        assert_eq!(from_file.bind.port(), 0);
        assert_eq!(from_file.runtime_workers.map(NonZeroUsize::get), Some(1));
        assert_eq!(from_file.speech_workers.get(), 4);
        assert_eq!(from_file.chat_models[0].as_str(), "file-chat");
        assert!(from_file.should_allow_any_origin);
        assert_eq!(
            from_file.api_key.as_ref().map(ApiKey::as_str),
            Some("secret")
        );

        let overridden =
            load(file, &["--model-chat", "cli-chat", "--speech-workers", "2"]).unwrap();
        assert_eq!(overridden.chat_models[0].as_str(), "cli-chat");
        assert_eq!(overridden.speech_workers.get(), 2);
        assert!(
            load("should_allow_any_origin = false", &["--allow-any-origin"])
                .unwrap()
                .should_allow_any_origin
        );
    }

    /// File values must retain constraints that Clap applies to flags.
    #[test]
    fn config_validation() {
        assert!(load("chat_models = []", &[]).is_err());
        assert!(load("api_key = ''", &[]).is_err());
        assert!(load("speech_workers = 0", &[]).is_err());
        assert!(load("api_key = 'secret'\nhosted_config = 'hosted.json'", &[]).is_err());
        assert!(load("misspelled_option = true", &[]).is_err());
    }
}
