//! Hosted request admission before provider extractors or generation run.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use bytes::BytesMut;
use eliza_http::hosted::{Caller, CapacityLease, Hosted, IdentityLease, Modality, Provider};
use eliza_http::problem::NativeError;
use http_body::{Body as HttpBody, Frame, SizeHint};
use http_body_util::BodyExt as _;
use tokio::time::{Instant, Sleep};

use crate::work::{DeliveryLease, Work, WorkResponse};

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

/// Retains delivery admission and checks output, idle and absolute response bounds.
pub(super) struct GuardedBody {
    /// Provider-produced frames, already generated off the async runtime.
    inner: Body,
    /// Delivery admission is independent of CPU-worker completion.
    lease: Option<DeliveryLease>,
    /// Idle frame deadline; socket deadlines also cover blocked downstream writes.
    idle: Pin<Box<Sleep>>,
    /// Duration between actual frame deliveries.
    idle_duration: Duration,
    /// Remaining encoded bytes accepted by the HTTP transport.
    remaining: usize,
}

impl GuardedBody {
    /// Build delivery timers from the explicit nonsecret deployment policy.
    pub(super) fn new(inner: Body, lease: DeliveryLease, hosted: &Hosted) -> Self {
        let idle_duration = Duration::from_secs(hosted.config().idle_seconds);
        Self {
            inner,
            lease: Some(lease),
            idle: Box::pin(tokio::time::sleep(idle_duration)),

            idle_duration,
            remaining: hosted.config().output_bytes,
        }
    }

    /// Drop buffered frames and release delivery while actual workers retain their own permits.
    fn fail(
        &mut self,
        kind: std::io::ErrorKind,
    ) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        self.inner = Body::empty();
        self.lease = None;
        Poll::Ready(Some(Err(axum::Error::new(std::io::Error::from(kind)))))
    }
}

impl HttpBody for GuardedBody {
    type Data = Bytes;

    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        // Timeouts release delivery and close the receiver; nested workers keep actual work admission.
        if self.idle.as_mut().poll(cx).is_ready() {
            return self.fail(std::io::ErrorKind::TimedOut);
        }
        let frame = Pin::new(&mut self.inner).poll_frame(cx);
        if let Poll::Ready(Some(Ok(frame))) = &frame {
            let bytes = frame.data_ref().map_or(0, Bytes::len);

            // Never deliver bytes beyond the explicit encoded output cap.
            if bytes > self.remaining {
                return self.fail(std::io::ErrorKind::InvalidData);
            }
            self.remaining -= bytes;

            // Shared response admission precedes any personal allowance failure.
            let until = Instant::now() + self.idle_duration;
            self.idle.as_mut().reset(until);
        }
        if matches!(frame, Poll::Ready(None | Some(Err(_)))) {
            self.lease = None;
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
// AdmittedUpload: Owns bounded input and HTTP concurrency through upload.
// -----------------------------------------------------------------------------

/// Upload ownership stays with the HTTP future until actual worker handoff.
struct AdmittedUpload {
    /// Request metadata ready for provider extraction on the worker.
    request: Request,
    /// Exactly one admitted raw input allocation.
    bytes: Bytes,
    /// Actual input ownership transferred to the worker.
    input: CapacityLease,
    /// HTTP concurrency stays with the transport, including cancellation before headers.
    identity: IdentityLease,
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
    ) -> Result<WorkResponse, AdmissionFailure> {
        let selected = provider(request.uri().path());
        let rejection = |error| AdmissionFailure {
            provider: selected,
            error,
        };

        // Verify only immediate connection metadata and the authenticated ingress header.
        let peer = request
            .extensions()
            .get::<ConnectInfo<crate::connection::Peer>>()
            .map(|value| value.0.clone());
        let peer = peer
            .ok_or_else(NativeError::unauthorized)
            .map_err(rejection)?;

        // Authenticate ingress before reading any asserted public client identity.
        let ip = hosted
            .client_ip(peer.address.ip(), request.headers())
            .map_err(rejection)?;

        // Health and documentation still require ingress but have no generation allowance.
        let Some(selected) = selected else {
            return Ok(WorkResponse::discovery(next.run(request).await, hosted));
        };

        // Resolve a real account or normalized public identity before admission.
        let caller = hosted
            .caller(ip, request.headers(), selected)
            .await
            .map_err(rejection)?;

        // Shared response admission precedes any personal allowance failure.
        let until = Instant::now() + Duration::from_secs(hosted.config().response_seconds);
        let capacity = hosted
            .responses
            .acquire(caller.tier(), 1)
            .map_err(rejection)?;

        let deadline = peer.deadline(until);

        // Cheap discovery does not occupy generation CPU or input-buffer admission.
        if modality(request.uri().path(), request.method()) == Modality::Discovery {
            let identity = hosted
                .admit(&caller, Modality::Discovery)
                .map_err(rejection)?;
            let response = next.run(request).await;
            return Ok(
                WorkResponse::discovery(response, hosted).admit(capacity, identity, deadline)
            );
        }

        let uploaded = Self::upload(&hosted, &caller, request)
            .await
            .map_err(rejection)?;

        // CPU admission rejects immediately; no personal allowance is charged for shared overload.
        let cpu = hosted.cpu.acquire(caller.tier(), 1).map_err(rejection)?;
        let work = Work {
            hosted: Arc::clone(&hosted),
            caller,
            cpu,
            input: uploaded.input,
        };

        // Await only metadata; actual work and permits remain inside the producer.
        let headers = work.start(uploaded.request, uploaded.bytes, next);
        let result = tokio::time::timeout_at(until, headers)
            .await
            .map_err(|_| rejection(NativeError::unavailable()))?;

        // Transfer response ownership only after a successful worker header result.
        let result = result
            .map_err(|_| rejection(NativeError::unavailable()))?
            .map_err(rejection)?;
        Ok(result.admit(capacity, uploaded.identity, deadline))
    }

    /// Admit bounded upload storage and personal HTTP concurrency before reading input.
    ///
    /// # Errors
    /// Returns native upload timeout, size, concurrency or shared-overload failures.
    async fn upload(
        hosted: &Hosted,
        caller: &Caller,
        request: Request,
    ) -> Result<AdmittedUpload, NativeError> {
        // Derive the exact raw allocation limit from the transport size hint.
        let maximum = hosted.config().body_bytes;
        let hint = request.body().size_hint();
        let length = hint.upper().unwrap_or(maximum as u64);
        let length = usize::try_from(length).map_err(|_| NativeError::too_large())?;

        // Bound allocation before reading any unauthenticated-size upload.
        if length > maximum {
            return Err(NativeError::too_large());
        }
        let units = u32::try_from(length).map_err(|_| NativeError::too_large())?;
        let input = hosted.buffers.acquire(caller.tier(), units)?;

        // Probe current shared CPU overload before reporting a personal concurrency failure.
        drop(hosted.cpu.acquire(caller.tier(), 1)?);
        let identity = hosted.begin(caller, modality(request.uri().path(), request.method()))?;

        // Read into one admitted allocation under a finite upload deadline.
        let (parts, body) = request.into_parts();
        let upload = tokio::time::timeout(
            Duration::from_secs(hosted.config().upload_seconds),
            Self::read_body(body, length),
        )
        .await;

        // Preserve explicit request timeout and size failures before spawning generation.
        let bytes = upload.map_err(|_| NativeError::request_timeout())??;

        Ok(AdmittedUpload {
            request: Request::from_parts(parts, Body::empty()),
            bytes,
            input,
            identity,
        })
    }

    /// Read directly into one admitted allocation, without a second collected-body copy.
    ///
    /// # Errors
    /// Returns native invalid-body or explicit input-size failures.
    async fn read_body(mut body: Body, maximum: usize) -> Result<Bytes, NativeError> {
        let mut bytes = BytesMut::with_capacity(maximum);
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|_| {
                NativeError::admission(
                    eliza_http::problem::ProblemClass::InvalidRequest,
                    "Unable to read request body.".into(),
                    None,
                )
            })?;

            // Request trailers do not contribute JSON input bytes.
            let Ok(data) = frame.into_data() else {
                continue;
            };

            // Refuse growth beyond the exact allocation already admitted.
            if bytes.len().saturating_add(data.len()) > maximum {
                return Err(NativeError::too_large());
            }
            bytes.extend_from_slice(&data);
        }
        Ok(bytes.freeze())
    }
}
