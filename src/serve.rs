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
//!   -> provider route adapters
//!   -> OpenAPI / Scalar from the same mounted routes
//! ```

use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroUsize;

use axum::Router;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

use crate::cli::{AuthMode, CorsMode, LogFormat, ServeArgs};
use crate::errors::AppError;
use crate::routes;
use crate::types::model::ModelId;
use crate::types::turn::RequestLimits;

// -----------------------------------------------------------------------------
// PositiveLimit: Converts raw CLI counts into runtime invariants.
// -----------------------------------------------------------------------------

/// Convert one CLI request bound into its positive runtime representation.
///
/// # Errors
///
/// Returns [`AppError`] when `value` is zero.
fn positive_limit(value: usize, flag: &'static str) -> Result<NonZeroUsize, AppError> {
    NonZeroUsize::new(value).ok_or(AppError::ZeroLimit { flag })
}

// -----------------------------------------------------------------------------
// ServerConfig: CLI-visible options converted into typed runtime facts.
// -----------------------------------------------------------------------------

/// Runtime configuration for one ELIZA HTTP server process.
///
/// ```
/// use eliza::serve::ServerConfig;
///
/// let _config = ServerConfig::default();
/// ```
#[derive(Debug, Clone)]
pub(super) struct ServerConfig {
    /// Validated listener address.
    address: SocketAddr,
    /// Behavior shared by all provider routes.
    routes: routes::context::RouteConfig,
    /// CORS policy for browser clients.
    cors: CorsMode,
    /// Log rendering mode.
    log: LogFormat,
}

impl Default for ServerConfig {
    fn default() -> Self {
        let limits = RequestLimits::builder()
            .max_input_chars(
                NonZeroUsize::new(8000).expect("default max input chars should be nonzero"),
            )
            .max_history_messages(
                NonZeroUsize::new(200).expect("default max history messages should be nonzero"),
            )
            .build();
        let routes = routes::context::RouteConfig::builder()
            .model(ModelId::default())
            .auth(AuthMode::None)
            .bearer_token(None)
            .stream_delay_ms(0)
            .limits(limits)
            .build();
        Self {
            address: SocketAddr::new(IpAddr::from([127, 0, 0, 1]), 8787),
            routes,
            cors: CorsMode::None,
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
        let limits = RequestLimits::builder()
            .max_input_chars(positive_limit(args.max_input_chars, "max-input-chars")?)
            .max_history_messages(positive_limit(
                args.max_history_messages,
                "max-history-messages",
            )?)
            .build();

        // Move provider behavior into the configuration shared by route state.
        let routes = routes::context::RouteConfig::builder()
            .model(args.model)
            .auth(args.auth)
            .bearer_token(args.bearer_token)
            .stream_delay_ms(args.stream_delay_ms)
            .limits(limits)
            .build();

        // Retain only process-level behavior at the serving boundary.
        Ok(Self {
            address: SocketAddr::new(args.host, args.port),
            routes,
            cors: args.cors,
            log: args.log,
        })
    }
}

impl ServerConfig {
    /// Build the complete provider-compatible router.
    fn into_router(self) -> Router {
        build_server_router(self.cors, self.routes)
    }

    /// Run the HTTP server until its listener fails.
    /// Serve HTTP requests until the server exits or fails.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] when binding or serving fails.
    pub(super) async fn run(self) -> Result<(), AppError> {
        init_tracing(self.log);
        let listener = TcpListener::bind(self.address)
            .await
            .map_err(|source| AppError::Bind {
                addr: self.address,
                source,
            })?;
        tracing::info!(address = %self.address, "serving ELIZA compatibility server");
        axum::serve(listener, self.into_router())
            .await
            .map_err(AppError::Serve)
    }
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
