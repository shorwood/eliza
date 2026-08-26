#![allow(
    clippy::items_after_test_module,
    reason = "rlib requires dependency-first declaration order for test modules"
)]

//! HTTP serving boundary.
//!
//! `ServeArgs` are the only runtime configuration input. They lower into
//! `ServerConfig`, which mounts every provider-compatible route and hands each
//! request to the route module that owns that wire shape.
//!
//! ```text
//! eliza serve CLI
//!   -> ServerConfig
//!   -> router + AppState
//!   -> routes::{openai_chat_completions, anthropic_messages, gemini_content}
//!   -> OpenAPI / Scalar from the same mounted routes
//! ```

use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroUsize;

use axum::Router;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

use crate::cli::{AuthMode, BearerToken, CorsMode, LogFormat, ServeArgs};
use crate::errors::AppError;
use crate::provider::contracts::ModelId;
use crate::routes;

/// Runtime operations owned by a validated server configuration.
pub(super) trait ServerRuntime {
    /// Build the complete provider-compatible router.
    fn router(self) -> Router;

    /// Bind and serve until the listener fails or the task is cancelled.
    async fn serve(self) -> Result<(), AppError>;
}

// -----------------------------------------------------------------------------
// ServerConfig: CLI-visible options converted into typed runtime facts.
// -----------------------------------------------------------------------------

/// Runtime configuration for one ELIZA HTTP server process.
///
/// ```
/// use eliza::serve::ServerConfig;
///
/// let config = ServerConfig::default();
/// assert_eq!(config.port, 8787);
/// assert_eq!(config.model.as_str(), "eliza-doctor");
/// ```
#[derive(Debug, Clone)]
pub(super) struct ServerConfig {
    /// Bind address.
    host: IpAddr,
    /// Bind port.
    port: u16,
    /// Provider-visible model id returned by model-list endpoints.
    model: ModelId,
    /// Provider endpoint authentication mode.
    auth: AuthMode,
    /// Shared bearer/API-key token when bearer auth is enabled.
    bearer_token: Option<BearerToken>,
    /// CORS policy for browser clients.
    cors: CorsMode,
    /// Optional delay between SSE chunks for local demos.
    stream_delay_ms: u64,
    /// Maximum accepted text character count across one provider request.
    max_input_chars: NonZeroUsize,
    /// Maximum accepted user-turn count replayed from one provider request.
    max_history_messages: NonZeroUsize,
    /// Log rendering mode.
    log: LogFormat,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: IpAddr::from([127, 0, 0, 1]),
            port: 8787,
            model: ModelId::default(),
            auth: AuthMode::None,
            bearer_token: None,
            cors: CorsMode::None,
            stream_delay_ms: 0,
            max_input_chars: NonZeroUsize::new(8000)
                .expect("default max input chars should be nonzero"),
            max_history_messages: NonZeroUsize::new(200)
                .expect("default max history messages should be nonzero"),
            log: LogFormat::Text,
        }
    }
}

impl TryFrom<ServeArgs> for ServerConfig {
    type Error = AppError;

    fn try_from(args: ServeArgs) -> Result<Self, Self::Error> {
        // Reject bearer mode before constructing an unusable server config.
        if args.auth == AuthMode::Bearer && args.bearer_token.is_none() {
            return Err(AppError::MissingBearerToken);
        }

        // Convert raw CLI counts into positive request-bound invariants.
        let max_input_chars =
            NonZeroUsize::new(args.max_input_chars).ok_or(AppError::ZeroLimit {
                flag: "max-input-chars",
            })?;
        let max_history_messages =
            NonZeroUsize::new(args.max_history_messages).ok_or(AppError::ZeroLimit {
                flag: "max-history-messages",
            })?;

        // Preserve the validated CLI values as the complete runtime contract.
        Ok(Self {
            host: args.host,
            port: args.port,
            model: args.model,
            auth: args.auth,
            bearer_token: args.bearer_token,
            cors: args.cors,
            stream_delay_ms: args.stream_delay_ms,
            max_input_chars,
            max_history_messages,
            log: args.log,
        })
    }
}

impl ServerConfig {
    /// Enable bearer authentication for an in-process server test.
    #[cfg(test)]
    pub(crate) fn with_bearer_token(mut self, token: BearerToken) -> Self {
        self.auth = AuthMode::Bearer;
        self.bearer_token = Some(token);
        self
    }

    /// Build the complete provider-compatible router.
    fn into_router(self) -> Router {
        let route_config = routes::context::RouteConfig::new(
            self.model,
            self.auth,
            self.bearer_token,
            self.stream_delay_ms,
            self.max_input_chars,
            self.max_history_messages,
        );
        build_server_router(self.cors, route_config)
    }

    /// Run the HTTP server until its listener fails.
    /// Serve HTTP requests until the server exits or fails.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] when binding or serving fails.
    async fn serve_http(self) -> Result<(), AppError> {
        init_tracing(self.log);
        let addr = SocketAddr::new(self.host, self.port);
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|source| AppError::Bind { addr, source })?;
        tracing::info!(%addr, "serving ELIZA compatibility server");
        axum::serve(listener, ServerRuntime::router(self))
            .await
            .map_err(AppError::Serve)
    }
}

impl ServerRuntime for ServerConfig {
    fn router(self) -> Router {
        self.into_router()
    }

    async fn serve(self) -> Result<(), AppError> {
        self.serve_http().await
    }
}

// -----------------------------------------------------------------------------
// RunServer: Runs any validated server runtime.
// -----------------------------------------------------------------------------

/// Run any validated server runtime.
///
/// # Errors
///
/// Returns an application error when binding or serving fails.
pub(super) async fn run_server(runtime: impl ServerRuntime) -> Result<(), AppError> {
    runtime.serve().await
}

/// Build a router from any testable server runtime.
#[cfg(test)]
pub(super) fn run_server_test_router(runtime: impl ServerRuntime) -> Router {
    runtime.router()
}

// -----------------------------------------------------------------------------
// InitTracing: Installs process-wide tracing.
// -----------------------------------------------------------------------------

/// Installs the requested tracing formatter once for the process.
fn init_tracing(format: LogFormat) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let result = match format {
        LogFormat::Text => tracing_subscriber::fmt().with_env_filter(filter).try_init(),
        LogFormat::Json => tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .try_init(),
    };
    match result {
        Ok(()) | Err(_) => {}
    }
}

// -----------------------------------------------------------------------------
// BuildServerRouter: Mounts every provider surface.
// -----------------------------------------------------------------------------

/// Build the full provider-compatible router.
///
/// ```
/// use eliza::serve::{ServerConfig, router};
///
/// let _router = router(ServerConfig::default());
/// ```
fn build_server_router(cors: CorsMode, config: routes::context::RouteConfig) -> Router {
    let router = routes::router::Routes::build(config);

    match cors {
        CorsMode::None => router,
        CorsMode::Permissive => router.layer(CorsLayer::permissive()),
    }
}
