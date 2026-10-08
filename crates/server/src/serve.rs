//! Server configuration, listener, CORS, and tracing.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::http::HeaderValue;
use eliza_http::context::RouteConfig;
use miette::Diagnostic;
use thiserror::Error;
use tokio::net::TcpListener;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tracing_subscriber::EnvFilter;

// -----------------------------------------------------------------------------
// CorsMode: Controls browser cross-origin access.
// -----------------------------------------------------------------------------

/// CORS policy installed around the complete HTTP surface.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CorsMode {
    /// Do not install a CORS layer.
    Off,
    /// Permit requests from any browser origin.
    Any,
    /// Permit requests from the listed browser origins.
    Origins(
        /// Exact `Origin` header values allowed to read browser responses.
        Vec<HeaderValue>,
    ),
}

/// Build a CORS layer that admits only the supplied browser origins.
fn origin_cors_layer(origins: Vec<HeaderValue>) -> CorsLayer {
    let allowed = AllowOrigin::predicate(move |origin, _| origins.contains(origin));
    let layer = CorsLayer::new().allow_origin(allowed).allow_methods(Any);
    layer.allow_headers(Any).expose_headers(Any)
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
        }
    }

    /// Build the complete provider-compatible router.
    pub fn into_router(self) -> Router {
        let router = crate::routes::Routes::for_config(self.routes)
            .into_router()
            .layer(axum::middleware::from_fn_with_state(
                Arc::new(crate::limits::Limits::new()),
                crate::limits::handle,
            ));
        match self.cors {
            CorsMode::Off => router,
            CorsMode::Any => router.layer(CorsLayer::permissive()),
            CorsMode::Origins(origins) => router.layer(origin_cors_layer(origins)),
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
        let router = self.into_router();
        let result = axum::serve(listener, router.into_make_service()).await;
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
