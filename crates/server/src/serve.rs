//! Server configuration, listener, CORS, and tracing.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use eliza_http::context::RouteConfig;
use eliza_http::hosted::Hosted;
use miette::Diagnostic;
use thiserror::Error;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

// -----------------------------------------------------------------------------
// CorsMode: Controls browser cross-origin access.
// -----------------------------------------------------------------------------

/// CORS policy installed around the complete HTTP surface.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CorsMode {
    /// Do not install a CORS layer.
    None,
    /// Permit requests from any browser origin.
    Permissive,
}

// -----------------------------------------------------------------------------
// LogFormat: Controls tracing event rendering.
// -----------------------------------------------------------------------------

/// Tracing event rendering mode.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LogFormat {
    /// Human-readable tracing output.
    Text,
    /// Structured JSON tracing output.
    Json,
}

// -----------------------------------------------------------------------------
// ServerError: Reports listener and serving failures.
// -----------------------------------------------------------------------------

/// Internal server startup and runtime failure vocabulary.
#[derive(Debug, Diagnostic, Error)]
enum ServerErrorKind {
    /// The configured TCP listener address could not be bound.
    #[error("failed to bind {addr}: {source}")]
    #[diagnostic(code(eliza::serve::bind))]
    Bind {
        /// Address the server attempted to bind.
        addr: SocketAddr,
        /// Operating-system error returned by the listener.
        #[source]
        source: std::io::Error,
    },

    /// Axum stopped because its serving loop failed.
    #[error("server error: {0}")]
    #[diagnostic(code(eliza::serve::runtime))]
    Serve(
        /// Runtime I/O failure returned by Axum.
        #[source]
        std::io::Error,
    ),
}

/// Opaque server startup or runtime failure.
#[derive(Debug, Diagnostic, Error)]
#[error(transparent)]
#[diagnostic(transparent)]
pub struct ServerError(
    /// Private diagnostic retaining the operating-system source chain.
    #[diagnostic_source]
    ServerErrorKind,
);

// -----------------------------------------------------------------------------
// ServerConfig: Owns validated route and process behavior.
// -----------------------------------------------------------------------------

/// Interval between bounded identity maintenance passes.
const SERVER_CONFIG_CLEANUP_SECONDS: u64 = 30;

/// Runtime configuration for one ELIZA HTTP server process.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Validated listener address.
    address: SocketAddr,
    /// Behavior shared by all provider routes.
    routes: RouteConfig,
    /// CORS policy for browser clients.
    cors: CorsMode,
    /// Log rendering mode.
    log: LogFormat,
    /// Optional explicit hosted policy.
    hosted: Option<Arc<Hosted>>,
}

impl ServerConfig {
    /// Construct a server from validated route and process configuration.
    #[must_use]
    pub const fn new(
        address: SocketAddr,
        routes: RouteConfig,
        cors: CorsMode,
        log: LogFormat,
    ) -> Self {
        Self {
            address,
            routes,
            cors,
            log,
            hosted: None,
        }
    }

    /// Enable an explicitly validated hosted deployment.
    #[must_use]
    pub fn with_hosted(mut self, hosted: Arc<Hosted>) -> Self {
        let workers = std::num::NonZeroUsize::new(hosted.config().speech_jobs)
            .unwrap_or(std::num::NonZeroUsize::MIN);
        self.routes = self.routes.with_speech_workers(workers);
        self.hosted = Some(hosted);
        self
    }

    /// Build the complete provider-compatible router.
    fn into_router(self) -> Router {
        let models = Arc::new(self.routes.models.clone());
        let router = crate::routes::Routes::for_config(self.routes).into_router();
        let router = if let Some(hosted) = self.hosted {
            let cleanup = Arc::downgrade(&hosted);
            tokio::spawn(async move {
                let mut timer = tokio::time::interval(std::time::Duration::from_secs(
                    SERVER_CONFIG_CLEANUP_SECONDS,
                ));
                loop {
                    timer.tick().await;
                    // Stop maintenance once the router and its active requests are gone.
                    let Some(hosted) = cleanup.upgrade() else {
                        break;
                    };
                    hosted.cleanup();
                }
            });
            router
                .layer(axum::middleware::from_fn_with_state(
                    hosted,
                    crate::admission::Admission::handle,
                ))
                .layer(axum::Extension(models))
        } else {
            router
        };
        match self.cors {
            CorsMode::None => router,
            CorsMode::Permissive => router.layer(CorsLayer::permissive()),
        }
    }

    /// Serve HTTP requests until the listener exits or fails.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic when the listener cannot bind or Axum fails.
    pub async fn run(self) -> Result<(), ServerError> {
        init_tracing(self.log);
        let listener = TcpListener::bind(self.address).await.map_err(|source| {
            ServerError(ServerErrorKind::Bind {
                addr: self.address,
                source,
            })
        })?;
        tracing::info!(address = %self.address, "serving ELIZA compatibility server");
        let config = self.hosted.as_ref().map(|hosted| hosted.config().clone());
        let router = self.into_router();
        let result = if let Some(config) = config {
            let listener = crate::connection::OriginListener::new(listener, Some(&config));
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<crate::connection::Peer>(),
            )
            .await
        } else {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
        };
        result.map_err(|source| ServerError(ServerErrorKind::Serve(source)))?;
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// InitTracing: Installs process-wide tracing.
// -----------------------------------------------------------------------------

/// Install the requested tracing formatter once for the process.
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
