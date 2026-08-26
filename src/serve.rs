//! HTTP serving boundary.
//!
//! `ServeArgs` are the only runtime configuration input. They lower into
//! `ServerConfig`, which mounts every provider-compatible route and hands each
//! request to the provider adapter that owns that wire shape.
//!
//! ```text
//! eliza serve CLI
//!   -> ServerConfig
//!   -> router + AppState
//!   -> provider::{openai, anthropic, gemini}
//!   -> OpenAPI / Scalar from the same mounted routes
//! ```

use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroUsize;
use std::sync::Arc;

use aide::axum::ApiRouter;
use aide::axum::routing::ApiMethodDocs;
use aide::openapi::{Info, OpenApi, Operation, ReferenceOr, Response, Responses, StatusCode};
use aide::scalar::Scalar;
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use schemars::JsonSchema;
use serde::Serialize;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

pub use crate::cli::BearerToken;
use crate::cli::{AuthMode, CorsMode, LogFormat, ServeArgs};
use crate::errors::AppError;
use crate::provider::{ProviderRejection, RequestLimits};
use crate::{ModelId, provider};

// -----------------------------------------------------------------------------
// Server configuration: CLI-visible options converted into typed runtime facts.
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
pub struct ServerConfig {
    /// Bind address.
    pub host: IpAddr,
    /// Bind port.
    pub port: u16,
    /// Provider-visible model id returned by model-list endpoints.
    pub model: ModelId,
    /// Provider endpoint authentication mode.
    pub auth: AuthMode,
    /// Shared bearer/API-key token when bearer auth is enabled.
    pub bearer_token: Option<BearerToken>,
    /// CORS policy for browser clients.
    pub cors: CorsMode,
    /// Optional delay between SSE chunks for local demos.
    pub stream_delay_ms: u64,
    /// Maximum accepted text character count across one provider request.
    pub max_input_chars: NonZeroUsize,
    /// Maximum accepted user-turn count replayed from one provider request.
    pub max_history_messages: NonZeroUsize,
    /// Log rendering mode.
    pub log: LogFormat,
}

// TODO: Delete in favor of From impl.
impl ServerConfig {
    pub(crate) fn limits(&self) -> RequestLimits {
        RequestLimits {
            max_input_chars: self.max_input_chars,
            max_history_messages: self.max_history_messages,
        }
    }
}

impl From<ServerConfig> for RequestLimits {
    fn from(config: ServerConfig) -> Self {
        Self {
            max_input_chars: config.max_input_chars,
            max_history_messages: config.max_history_messages,
        }
    }
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
        if args.auth == AuthMode::Bearer && args.bearer_token.is_none() {
            return Err(AppError::MissingBearerToken);
        }
        let max_input_chars =
            NonZeroUsize::new(args.max_input_chars).ok_or(AppError::ZeroLimit {
                flag: "max-input-chars",
            })?;
        let max_history_messages =
            NonZeroUsize::new(args.max_history_messages).ok_or(AppError::ZeroLimit {
                flag: "max-history-messages",
            })?;

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

#[derive(Debug, Clone)]
pub(crate) struct AppState {
    pub(crate) config: Arc<ServerConfig>,
}

// -----------------------------------------------------------------------------
// Provider authentication: routes share the check, but each provider renders the
// failure in its own public envelope.
// -----------------------------------------------------------------------------

/// Check provider authentication headers against the configured server auth.
///
/// # Errors
///
/// Returns [`ProviderRejection`] when bearer auth is enabled and neither
/// `Authorization: Bearer` nor `x-api-key` matches the configured token.
pub(crate) fn require_provider_auth(
    headers: &HeaderMap,
    config: &ServerConfig,
) -> Result<(), ProviderRejection> {
    match config.auth {
        AuthMode::None => Ok(()),
        AuthMode::Bearer => {
            let Some(expected) = config.bearer_token.as_ref() else {
                return Err(ProviderRejection::unauthorized());
            };

            let bearer_matches = headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
                .is_some_and(|token| token == expected.as_str());
            let api_key_matches = headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|token| token == expected.as_str());

            if bearer_matches || api_key_matches {
                Ok(())
            } else {
                Err(ProviderRejection::unauthorized())
            }
        }
    }
}

// -----------------------------------------------------------------------------
// OpenAPI and system routes: docs are generated from the same graph the server
// actually mounts.
// -----------------------------------------------------------------------------

#[derive(Debug, Serialize, JsonSchema)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
}

async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "eliza",
    })
}

async fn openapi_json(Extension(api): Extension<OpenApi>) -> Json<OpenApi> {
    Json(api)
}

fn openapi_document() -> OpenApi {
    OpenApi {
        info: Info {
            title: "ELIZA Compatibility Server".to_owned(),
            summary: Some("Classic ELIZA served through provider-compatible HTTP APIs.".to_owned()),
            description: Some(
                "A standalone ELIZA server exposing OpenAI, Anthropic, and Gemini-compatible endpoints.".to_owned(),
            ),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            ..Info::default()
        },
        ..OpenApi::default()
    }
}

fn endpoint_docs(method: &'static str, summary: &str, tag: &str) -> ApiMethodDocs {
    let mut responses = Responses::default();
    responses.responses.insert(
        StatusCode::Code(200),
        ReferenceOr::Item(Response {
            description: "Successful response.".to_owned(),
            ..Response::default()
        }),
    );
    responses.default = Some(ReferenceOr::Item(Response {
        description: "Provider-shaped error response.".to_owned(),
        ..Response::default()
    }));

    ApiMethodDocs::new(
        method,
        Operation {
            tags: vec![tag.to_owned()],
            summary: Some(summary.to_owned()),
            responses: Some(responses),
            ..Operation::default()
        },
    )
}

// -----------------------------------------------------------------------------
// Process lifecycle: graceful shutdown hooks for the standalone binary.
// -----------------------------------------------------------------------------

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

fn init_tracing(format: LogFormat) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    match format {
        LogFormat::Text => {
            let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
        }
        LogFormat::Json => {
            let _ = tracing_subscriber::fmt()
                .json()
                .with_env_filter(filter)
                .try_init();
        }
    }
}

// -----------------------------------------------------------------------------
// Runtime lifecycle and route graph: all provider surfaces are always mounted;
// SDK callers choose the path that matches their provider.
// -----------------------------------------------------------------------------

/// Build the full provider-compatible router.
///
/// ```
/// use eliza::serve::{ServerConfig, router};
///
/// let _router = router(ServerConfig::default());
/// ```
pub fn router(config: ServerConfig) -> Router {
    let mut api = openapi_document();
    let cors = config.cors;

    let router = ApiRouter::new()
        // --- Start with process liveness before provider-specific surfaces.
        .route("/healthz", get(healthz))
        .api_route_docs("/healthz", endpoint_docs("get", "Health check", "system"))
        // --- Publish the OpenAI model catalog shape.
        .route("/v1/models", get(provider::openai::models))
        .api_route_docs(
            "/v1/models",
            endpoint_docs("get", "OpenAI models", "openai"),
        )
        // --- Lower OpenAI Chat Completions requests into one ELIZA turn.
        .route(
            "/v1/chat/completions",
            post(provider::openai::chat_completions),
        )
        .api_route_docs(
            "/v1/chat/completions",
            endpoint_docs("post", "OpenAI chat completion", "openai"),
        )
        // --- Lower OpenAI Responses requests into the same conversation path.
        .route("/v1/responses", post(provider::openai::responses))
        .api_route_docs(
            "/v1/responses",
            endpoint_docs("post", "OpenAI response", "openai"),
        )
        // --- Serve Gemini clients that use the OpenAI-compatible chat route.
        .route(
            "/v1beta/openai/chat/completions",
            post(provider::openai::chat_completions),
        )
        .api_route_docs(
            "/v1beta/openai/chat/completions",
            endpoint_docs("post", "Gemini OpenAI chat", "gemini-openai"),
        )
        // --- Lower Anthropic Messages requests into one ELIZA turn.
        .route("/v1/messages", post(provider::anthropic::messages))
        .api_route_docs(
            "/v1/messages",
            endpoint_docs("post", "Anthropic message", "anthropic"),
        )
        // --- Publish the native Gemini model catalog shape.
        .route("/v1beta/models", get(provider::gemini::models))
        .api_route_docs(
            "/v1beta/models",
            endpoint_docs("get", "Gemini models", "gemini"),
        )
        // --- Serve Gemini generateContent and streamGenerateContent actions.
        .route(
            "/v1beta/models/{model_action}",
            post(provider::gemini::model_action),
        )
        .api_route_docs(
            "/v1beta/models/{model_action}",
            endpoint_docs("post", "Gemini content", "gemini"),
        )
        // --- Expose the generated OpenAPI document for raw tooling.
        .route("/openapi.json", get(openapi_json))
        // --- Mount Scalar against the same OpenAPI document.
        .route(
            "/docs",
            Scalar::new("/openapi.json")
                .with_title("ELIZA API")
                .axum_route(),
        )
        .finish_api(&mut api)
        .with_state(AppState {
            config: Arc::new(config),
        })
        .layer(Extension(api));

    match cors {
        CorsMode::None => router,
        CorsMode::Permissive => router.layer(CorsLayer::permissive()),
    }
}

/// Run the HTTP server until Ctrl-C or SIGTERM.
///
/// This binds a real TCP listener. Use [`router`] when embedding the app in
/// tests or another process.
///
/// # Errors
///
/// Returns [`AppError::Bind`] when the TCP listener cannot bind and
/// [`AppError::Serve`] when Axum returns a server IO error.
pub async fn serve(config: ServerConfig) -> Result<(), AppError> {
    init_tracing(config.log);
    let addr = SocketAddr::new(config.host, config.port);
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|source| AppError::Bind { addr, source })?;

    tracing::info!(%addr, "serving ELIZA compatibility server");
    axum::serve(listener, router(config))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(AppError::Serve)
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::missing_panics_doc,
    reason = "test rationales replace public panic contracts"
)]
/// Checks ELIZA provider compatibility behavior at its source owner.
mod tests {
    /// Holds request drivers and setup without copying production logic.
    mod support {
        /// Supplies setup for the anthropic checks.
        pub(super) mod anthropic {
            #![allow(
                clippy::missing_panics_doc,
                reason = "test assertions panic to report failures"
            )]
            pub(crate) use axum::http::StatusCode;
            pub(crate) use serde_json::{Value, json};

            pub(crate) use crate::serve::ServerConfig;
            pub(crate) use crate::test_support as support;
        }
        /// Supplies setup for the gemini checks.
        pub(super) mod gemini {
            #![allow(
                clippy::missing_panics_doc,
                reason = "test assertions panic to report failures"
            )]
            pub(crate) use axum::http::StatusCode;
            pub(crate) use serde_json::{Value, json};

            pub(crate) use crate::serve::ServerConfig;
            pub(crate) use crate::test_support as support;
        }
        /// Supplies setup for the openai checks.
        pub(super) mod openai {
            #![allow(
                clippy::missing_panics_doc,
                reason = "test assertions panic to report failures"
            )]
            pub(crate) use axum::http::StatusCode;
            pub(crate) use serde_json::{Value, json};

            pub(crate) use crate::cli::AuthMode;
            pub(crate) use crate::serve::{BearerToken, ServerConfig};
            pub(crate) use crate::test_support as support;
        }
    }
    /// Checks the anthropic contract.
    mod anthropic {
        use super::support::anthropic::*;
        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_anthropic_message_shape() {
            let (status, body) = support::post_json_with(
                ServerConfig::default(),
                "/v1/messages",
                json!({
                    "model": "eliza-doctor",
                    "max_tokens": 128,
                    "messages": [{ "role": "user", "content": "I am sad" }]
                }),
                &[("anthropic-version", "2023-06-01")],
            )
            .await;
            let body: Value = serde_json::from_str(&body).expect("response should be json");
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["type"], "message");
            assert_eq!(body["content"][0]["type"], "text");
            assert_eq!(body["content"][0]["text"], "I AM SORRY TO HEAR YOU ARE SAD");
            assert_eq!(body["stop_reason"], "end_turn");
        }
        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_anthropic_stream_uses_named_sse_events() {
            let (status, body) = support::post_json_with(
                ServerConfig::default(),
                "/v1/messages",
                json!({
                    "model": "eliza-doctor",
                    "max_tokens": 128,
                    "stream": true,
                    "messages": [{ "role": "user", "content": "Hello" }]
                }),
                &[("anthropic-version", "2023-06-01")],
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert!(body.contains("event: message_start"));
            assert!(body.contains("event: content_block_delta"));
            assert!(body.contains("event: message_stop"));
        }
    }
    /// Checks the gemini contract.
    mod gemini {
        use super::support::gemini::*;
        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_gemini_generate_content_shape() {
            let (status, body) = support::post_json_with(
                ServerConfig::default(),
                "/v1beta/models/eliza-doctor:generateContent",
                json!({
                    "contents": [{
                        "role": "user",
                        "parts": [{ "text": "I am sad" }]
                    }]
                }),
                &[],
            )
            .await;
            let body: Value = serde_json::from_str(&body).expect("response should be json");
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["candidates"][0]["content"]["role"], "model");
            assert_eq!(
                body["candidates"][0]["content"]["parts"][0]["text"],
                "I AM SORRY TO HEAR YOU ARE SAD"
            );
            assert_eq!(body["candidates"][0]["finishReason"], "STOP");
        }
        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_gemini_stream_generate_content_can_return_sse() {
            let (status, body) = support::post_json_with(
                ServerConfig::default(),
                "/v1beta/models/eliza-doctor:streamGenerateContent?alt=sse",
                json!({
                    "contents": [{
                        "role": "user",
                        "parts": [{ "text": "Hello" }]
                    }]
                }),
                &[],
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert!(body.contains("data:"));
            assert!(body.contains("finishReason"));
        }
        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_gemini_rejects_multimodal_parts_with_gemini_error() {
            let (status, body) = support::post_json_with(
                ServerConfig::default(),
                "/v1beta/models/eliza-doctor:generateContent",
                json!({
                    "contents": [{
                        "role": "user",
                        "parts": [{ "inlineData": { "mimeType": "image/png", "data": "AA==" } }]
                    }]
                }),
                &[],
            )
            .await;
            let body: Value = serde_json::from_str(&body).expect("response should be json");
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["error"]["status"], "INVALID_ARGUMENT");
        }
    }
    /// Checks the openai contract.
    mod openai {
        use super::support::openai::*;
        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_openai_chat_completion_shape() {
            let (status, body) = support::post_json_with(
                ServerConfig::default(),
                "/v1/chat/completions",
                json!({
                    "model": "eliza-doctor",
                    "messages": [{ "role": "user", "content": "I am sad" }]
                }),
                &[],
            )
            .await;
            let body: Value = serde_json::from_str(&body).expect("response should be json");
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["object"], "chat.completion");
            assert_eq!(
                body["choices"][0]["message"]["content"],
                "I AM SORRY TO HEAR YOU ARE SAD"
            );
        }
        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_openai_streaming_chat_completion_shape() {
            let (status, body) = support::post_json_with(
                ServerConfig::default(),
                "/v1/chat/completions",
                json!({
                    "model": "eliza-doctor",
                    "stream": true,
                    "messages": [{ "role": "user", "content": "Hello" }]
                }),
                &[],
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert!(body.contains("chat.completion.chunk"));
            assert!(body.contains("data: [DONE]"));
        }
        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_gemini_openai_alias_uses_openai_shape() {
            let (status, body) = support::post_json_with(
                ServerConfig::default(),
                "/v1beta/openai/chat/completions",
                json!({
                    "model": "eliza-doctor",
                    "messages": [{ "role": "user", "content": "Hello" }]
                }),
                &[],
            )
            .await;
            let body: Value = serde_json::from_str(&body).expect("response should be json");
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["object"], "chat.completion");
        }
        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_openai_responses_shape() {
            let (status, body) = support::post_json_with(
                ServerConfig::default(),
                "/v1/responses",
                json!({
                    "model": "eliza-doctor",
                    "input": "I need help"
                }),
                &[],
            )
            .await;
            let body: Value = serde_json::from_str(&body).expect("response should be json");
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["object"], "response");
            assert_eq!(body["output"][0]["content"][0]["type"], "output_text");
        }
        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_bearer_auth_rejects_missing_token() {
            let config = ServerConfig {
                auth: AuthMode::Bearer,
                bearer_token: Some("secret".parse::<BearerToken>().expect("token should parse")),
                ..ServerConfig::default()
            };
            let (status, body) = support::post_json_with(
                config,
                "/v1/chat/completions",
                json!({
                    "model": "eliza-doctor",
                    "messages": [{ "role": "user", "content": "Hello" }]
                }),
                &[],
            )
            .await;
            let body: Value = serde_json::from_str(&body).expect("response should be json");
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert_eq!(body["error"]["type"], "authentication_error");
        }
    }
}
