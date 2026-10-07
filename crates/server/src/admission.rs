//! Hosted request admission before provider extractors or generation run.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use eliza_http::hosted::{CapacityLease, Hosted, IdentityLease, Modality, Provider};
use eliza_http::problem::NativeError;
use http_body::{Body as HttpBody, Frame, SizeHint};

// -----------------------------------------------------------------------------
// Provider: Selects native policy responses without duplicating wire DTOs.
// -----------------------------------------------------------------------------

/// Select native response and credential contracts from the route prefix.
fn provider(path: &str) -> Option<Provider> {
    if path.starts_with("/openai/") || path.starts_with("/gemini/v1beta/openai/") {
        Some(Provider::OpenAi)
    } else if path.starts_with("/anthropic/") {
        Some(Provider::Anthropic)
    } else if path.starts_with("/gemini/") {
        Some(Provider::Gemini)
    } else if path.starts_with("/ollama/") {
        Some(Provider::Ollama)
    } else {
        None
    }
}

// -----------------------------------------------------------------------------
// Modality: Selects generation allowances before provider extraction.
// -----------------------------------------------------------------------------

/// Identify discovery and native media endpoints without decoding their bodies.
fn modality(path: &str, method: &axum::http::Method) -> Modality {
    if *method == axum::http::Method::GET {
        Modality::Discovery
    } else if path.ends_with("/audio/speech") {
        Modality::Speech
    } else if path.ends_with("/images/generations") {
        Modality::Image
    } else {
        Modality::Text
    }
}

// -----------------------------------------------------------------------------
// AdmissionFailure: Owns the policy rejection's provider-native projection.
// -----------------------------------------------------------------------------

/// A typed policy failure paired with the SDK surface that requested it.
pub(super) struct AdmissionFailure {
    /// Native response family, absent for origin health and documentation routes.
    provider: Option<Provider>,
    /// Public typed problem, without private ingress or billing causes.
    error: NativeError,
}

impl IntoResponse for AdmissionFailure {
    fn into_response(self) -> Response {
        match self.provider {
            Some(Provider::OpenAi) => {
                eliza_server_openai::errors::OpenAiRejection::from(self.error).into_response()
            }
            Some(Provider::Anthropic) => {
                eliza_server_anthropic::errors::AnthropicRejection::from(self.error).into_response()
            }
            Some(Provider::Gemini) => {
                eliza_server_gemini::errors::GeminiRejection::from(self.error).into_response()
            }
            Some(Provider::Ollama) => {
                eliza_server_ollama::errors::OllamaRejection::from(self.error).into_response()
            }
            None => self.error.into_response(),
        }
    }
}

// -----------------------------------------------------------------------------
// GuardedBody: Keeps admission until the response is consumed or disconnected.
// -----------------------------------------------------------------------------

/// Retains response and identity admission while the body is being delivered.
struct GuardedBody {
    /// Provider-produced body frames.
    inner: Body,
    /// Response capacity released on completion or disconnect.
    capacity: Option<CapacityLease>,
    /// Per-caller concurrency released on completion or disconnect.
    identity: Option<IdentityLease>,
}

impl HttpBody for GuardedBody {
    type Data = Bytes;

    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let frame = Pin::new(&mut self.inner).poll_frame(cx);
        if matches!(frame, Poll::Ready(None | Some(Err(_)))) {
            self.capacity = None;
            self.identity = None;
        }
        frame
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

// -----------------------------------------------------------------------------
// Admission: Applies hosted policy only when explicitly enabled.
// -----------------------------------------------------------------------------

/// Owns the hosted middleware boundary and its failure projection.
pub(super) struct Admission;

impl Admission {
    /// Authenticate ingress and account, then admit responses without queuing.
    ///
    /// # Errors
    /// Returns the selected provider's native ingress, lookup or allowance failure.
    pub(super) async fn handle(
        State(hosted): State<Arc<Hosted>>,
        request: Request,
        next: Next,
    ) -> Result<Response, AdmissionFailure> {
        let selected = provider(request.uri().path());
        let rejection = |error| AdmissionFailure {
            provider: selected,
            error,
        };

        // Verify only immediate connection metadata and the authenticated ingress header.
        let peer = request
            .extensions()
            .get::<ConnectInfo<std::net::SocketAddr>>()
            .map(|value| value.0.ip());
        let peer = peer
            .ok_or_else(NativeError::unauthorized)
            .map_err(rejection)?;

        // Authenticate ingress before reading any asserted public client identity.
        let ip = hosted
            .client_ip(peer, request.headers())
            .map_err(rejection)?;

        // Health and documentation still require ingress but have no generation allowance.
        let Some(selected) = selected else {
            return Ok(next.run(request).await);
        };
        let kind = modality(request.uri().path(), request.method());
        let caller = hosted
            .caller(ip, request.headers(), selected)
            .await
            .map_err(rejection)?;

        // Shared overload takes priority over personal allowances and never offers support.
        let capacity = hosted
            .responses
            .acquire(caller.tier(), 1)
            .map_err(rejection)?;
        let identity = hosted.admit(&caller, kind).map_err(rejection)?;

        // Preserve native handlers after both shared and personal admission.
        let response = next.run(request).await;
        let (mut parts, body) = response.into_parts();
        parts.headers.insert(
            "cache-control",
            axum::http::HeaderValue::from_static("no-store"),
        );

        // Keep admission until actual body completion, even after the handler returns.
        let guarded = GuardedBody {
            inner: body,
            capacity: Some(capacity),
            identity: Some(identity),
        };
        Ok(Response::from_parts(parts, Body::new(guarded)))
    }
}
