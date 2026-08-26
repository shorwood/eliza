#![allow(
    clippy::items_after_test_module,
    reason = "rlib requires dependency-first declaration order for test modules"
)]

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

use crate::cli::{AuthMode, BearerToken, CorsMode, LogFormat, ServeArgs};
use crate::errors::AppError;
use crate::provider;
use crate::provider::contracts::{ModelId, ProviderRejection};

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
    pub(super) model: ModelId,
    /// Provider endpoint authentication mode.
    auth: AuthMode,
    /// Shared bearer/API-key token when bearer auth is enabled.
    bearer_token: Option<BearerToken>,
    /// CORS policy for browser clients.
    cors: CorsMode,
    /// Optional delay between SSE chunks for local demos.
    pub(super) stream_delay_ms: u64,
    /// Maximum accepted text character count across one provider request.
    pub(super) max_input_chars: NonZeroUsize,
    /// Maximum accepted user-turn count replayed from one provider request.
    pub(super) max_history_messages: NonZeroUsize,
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
    /// Build the complete provider-compatible router.
    fn into_router(self) -> Router {
        build_server_router(self.cors, Arc::new(self))
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
// AppState: Shares immutable server state with provider routes.
// -----------------------------------------------------------------------------

/// Shared immutable state cloned into every provider route.
#[derive(Debug, Clone)]
pub(super) struct AppState {
    /// Validated server behavior and request limits.
    pub(super) config: Arc<ServerConfig>,
}

// -----------------------------------------------------------------------------
// Provider: Validates shared provider credentials.
// -----------------------------------------------------------------------------

/// Reads an optional textual header without erasing malformed-value failures.
///
/// # Errors
///
/// Returns [`axum::http::header::ToStrError`] when a present header is not valid text.
fn provider_auth_header_text(
    headers: &HeaderMap,
    name: impl axum::http::header::AsHeaderName,
) -> Result<Option<&str>, axum::http::header::ToStrError> {
    // Missing optional headers are distinct from malformed present values.
    let Some(value) = headers.get(name) else {
        return Ok(None);
    };
    value.to_str().map(Some)
}

/// Checks the two provider-supported token headers against bearer configuration.
///
/// # Errors
///
/// Returns [`ProviderRejection`] when configuration is incomplete, a supplied
/// header is malformed, or neither supported header matches the expected token.
fn provider_authenticate_bearer(
    headers: &HeaderMap,
    config: &ServerConfig,
) -> Result<(), ProviderRejection> {
    // A validated config should always carry this token; preserve defense in depth.
    let Some(expected) = config.bearer_token.as_ref() else {
        return Err(ProviderRejection::unauthorized());
    };

    // Accept either the standard bearer header or provider API-key spelling.
    let bearer = provider_auth_header_text(headers, axum::http::header::AUTHORIZATION)
        .map_err(|_| ProviderRejection::unauthorized())?;
    let bearer_matches = bearer
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == expected.as_str());
    let api_key = provider_auth_header_text(headers, "x-api-key")
        .map_err(|_| ProviderRejection::unauthorized())?;
    let api_key_matches = api_key.is_some_and(|token| token == expected.as_str());

    if bearer_matches || api_key_matches {
        Ok(())
    } else {
        Err(ProviderRejection::unauthorized())
    }
}

/// Check provider authentication headers against the configured server auth.
///
/// # Errors
///
/// Returns [`ProviderRejection`] when bearer auth is enabled and neither
/// `Authorization: Bearer` nor `x-api-key` matches the configured token.
pub(super) fn provider_authenticate(
    headers: &HeaderMap,
    config: &ServerConfig,
) -> Result<(), ProviderRejection> {
    match config.auth {
        AuthMode::None => Ok(()),
        AuthMode::Bearer => provider_authenticate_bearer(headers, config),
    }
}

// -----------------------------------------------------------------------------
// HealthResponse: Models the liveness response.
// -----------------------------------------------------------------------------

/// Liveness response returned by the system health route.
#[derive(Debug, Serialize, JsonSchema)]
struct HealthResponse {
    /// Stable health state for machine consumers.
    status: &'static str,
    /// Service identifier used by local orchestration.
    service: &'static str,
}

/// Reports that the in-process HTTP service is alive.
async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "eliza",
    })
}

/// Returns the generated `OpenAPI` document mounted into the router.
async fn openapi_json(Extension(api): Extension<OpenApi>) -> Json<OpenApi> {
    Json(api)
}

/// Builds metadata shared by the `OpenAPI` JSON and Scalar documentation routes.
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

// -----------------------------------------------------------------------------
// EndpointMetadata: Builds documentation for mounted endpoints.
// -----------------------------------------------------------------------------

/// Human-facing metadata for one documented endpoint.
struct EndpointMetadata {
    /// Lowercase HTTP method name.
    method: &'static str,
    /// Short operation summary.
    summary: &'static str,
    /// `OpenAPI` grouping tag.
    tag: &'static str,
}

impl EndpointMetadata {
    /// Convert this metadata into Aide's method documentation.
    fn into_docs(self) -> ApiMethodDocs {
        // Describe uniform success and provider-specific failure envelopes.
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

        // Attach route identity and responses to Aide's method metadata.
        ApiMethodDocs::new(
            self.method,
            Operation {
                tags: vec![self.tag.to_owned()],
                summary: Some(self.summary.to_owned()),
                responses: Some(responses),
                ..Operation::default()
            },
        )
    }
}

// -----------------------------------------------------------------------------
// Tests: Verify provider compatibility routes and authentication.
// -----------------------------------------------------------------------------

/// Checks ELIZA provider compatibility behavior at its source owner.
#[cfg(test)]
#[allow(
    clippy::missing_panics_doc,
    reason = "test rationales replace public panic contracts"
)]
mod tests {
    /// Checks the anthropic contract.
    mod anthropic {
        use axum::http::StatusCode;
        use serde_json::{Value, json};

        use crate::serve::ServerConfig;
        use crate::test_support as support;

        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_anthropic_message_shape() {
            let support::TestResponse { status, body } = support::post_json_with(
                ServerConfig::default(),
                "/v1/messages",
                json!({
                    "model": "eliza-doctor",
                    "max_tokens": 128,
                    "messages": [{ "role": "user", "content": "I am sad" }]
                }),
                &[support::TestHeader {
                    name: "anthropic-version",
                    value: "2023-06-01",
                }],
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
            let support::TestResponse { status, body } = support::post_json_with(
                ServerConfig::default(),
                "/v1/messages",
                json!({
                    "model": "eliza-doctor",
                    "max_tokens": 128,
                    "stream": true,
                    "messages": [{ "role": "user", "content": "Hello" }]
                }),
                &[support::TestHeader {
                    name: "anthropic-version",
                    value: "2023-06-01",
                }],
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert!(body.contains("event: message_start"));
            assert!(body.contains("event: content_block_delta"));
            let has_message_stop = body.contains("event: message_stop");
            assert!(has_message_stop);
        }
    }
    /// Checks the gemini contract.
    mod gemini {
        use axum::http::StatusCode;
        use serde_json::{Value, json};

        use crate::serve::ServerConfig;
        use crate::test_support as support;

        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_gemini_generate_content_shape() {
            let support::TestResponse { status, body } = support::post_json_with(
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
            let support::TestResponse { status, body } = support::post_json_with(
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
            let has_finish_reason = body.contains("finishReason");
            assert!(has_finish_reason);
        }

        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_gemini_rejects_multimodal_parts_with_gemini_error() {
            let support::TestResponse { status, body } = support::post_json_with(
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
        use axum::http::StatusCode;
        use serde_json::{Value, json};

        use crate::cli::{AuthMode, BearerToken};
        use crate::serve::ServerConfig;
        use crate::test_support as support;

        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_openai_chat_completion_shape() {
            let support::TestResponse { status, body } = support::post_json_with(
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
            let support::TestResponse { status, body } = support::post_json_with(
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
            let has_done_event = body.contains("data: [DONE]");
            assert!(has_done_event);
        }

        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_gemini_openai_alias_uses_openai_shape() {
            let support::TestResponse { status, body } = support::post_json_with(
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
            let object = &body["object"];
            assert_eq!(object, "chat.completion");
        }

        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_openai_responses_shape() {
            let support::TestResponse { status, body } = support::post_json_with(
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
            let support::TestResponse { status, body } = support::post_json_with(
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
// SystemRoutes: Mounts system endpoints.
// -----------------------------------------------------------------------------

/// Mount process-health routes.
fn system_routes(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
    let router = router.route("/healthz", get(healthz));
    router.api_route_docs(
        "/healthz",
        EndpointMetadata {
            method: "get",
            summary: "Health check",
            tag: "system",
        }
        .into_docs(),
    )
}

// -----------------------------------------------------------------------------
// OpenaiRoutes: Mounts OpenAI-compatible endpoints.
// -----------------------------------------------------------------------------

/// Mount the `OpenAI`-compatible routes.
fn openai_routes(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
    let router = router.route("/v1/models", get(provider::openai::open_ai_route_models));
    let router = router.api_route_docs(
        "/v1/models",
        EndpointMetadata {
            method: "get",
            summary: "OpenAI models",
            tag: "openai",
        }
        .into_docs(),
    );
    let router = router.route(
        "/v1/chat/completions",
        post(provider::openai::open_ai_route_chat_completions),
    );
    let router = router.api_route_docs(
        "/v1/chat/completions",
        EndpointMetadata {
            method: "post",
            summary: "OpenAI chat completion",
            tag: "openai",
        }
        .into_docs(),
    );
    let router = router.route(
        "/v1/responses",
        post(provider::openai::open_ai_route_responses),
    );
    router.api_route_docs(
        "/v1/responses",
        EndpointMetadata {
            method: "post",
            summary: "OpenAI response",
            tag: "openai",
        }
        .into_docs(),
    )
}

// -----------------------------------------------------------------------------
// AlternateProviderRoutes: Mounts alternate provider endpoints.
// -----------------------------------------------------------------------------

/// Mount Anthropic, Gemini, and Gemini's `OpenAI` alias routes.
fn alternate_provider_routes(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
    let router = router.route(
        "/v1beta/openai/chat/completions",
        post(provider::openai::open_ai_route_chat_completions),
    );
    let router = router.api_route_docs(
        "/v1beta/openai/chat/completions",
        EndpointMetadata {
            method: "post",
            summary: "Gemini OpenAI chat",
            tag: "gemini-openai",
        }
        .into_docs(),
    );
    let router = router.route("/v1/messages", post(provider::anthropic::messages));
    let router = router.api_route_docs(
        "/v1/messages",
        EndpointMetadata {
            method: "post",
            summary: "Anthropic message",
            tag: "anthropic",
        }
        .into_docs(),
    );
    let router = router.route("/v1beta/models", get(provider::gemini::models));
    let router = router.api_route_docs(
        "/v1beta/models",
        EndpointMetadata {
            method: "get",
            summary: "Gemini models",
            tag: "gemini",
        }
        .into_docs(),
    );
    let router = router.route(
        "/v1beta/models/{model_action}",
        post(provider::gemini::GeminiActionHandler::handle),
    );
    router.api_route_docs(
        "/v1beta/models/{model_action}",
        EndpointMetadata {
            method: "post",
            summary: "Gemini content",
            tag: "gemini",
        }
        .into_docs(),
    )
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
fn build_server_router(cors: CorsMode, config: Arc<ServerConfig>) -> Router {
    let mut api = openapi_document();

    let router = system_routes(ApiRouter::new());
    let router = openai_routes(router);
    let router = alternate_provider_routes(router);

    // Mount generated documentation and attach shared application state.
    let router = router.route("/openapi.json", get(openapi_json));
    let docs = Scalar::new("/openapi.json")
        .with_title("ELIZA API")
        .axum_route();
    let router = router.route("/docs", docs).finish_api(&mut api);
    let router = router.with_state(AppState { config });
    let router = router.layer(Extension(api));

    match cors {
        CorsMode::None => router,
        CorsMode::Permissive => router.layer(CorsLayer::permissive()),
    }
}
