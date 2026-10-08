//! Universal request bounds, independent of caller identity or deployment.

// These two narrow exceptions keep the middleware's Axum entry point and
// provider-specific error envelopes together without extra wrapper types.
#![allow(
    rlib::axum_ad_hoc_response_wrappers,
    rlib::foreign_type_method_like_free_functions,
    rlib::misordered_module_declarations
)]

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use eliza_http::problem::NativeError;
use http_body::{Body as HttpBody, Frame, SizeHint};
use http_body_util::{BodyExt as _, Limited};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::{Instant, Sleep};

// -----------------------------------------------------------------------------
// Limits: Bound requests and active responses for every caller.
// -----------------------------------------------------------------------------

/// Maximum bytes buffered from one request.
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

/// Maximum bytes emitted by one response.
const MAX_OUTPUT_BYTES: usize = 32 * 1024 * 1024;

/// Maximum simultaneous response bodies held by this process.
const MAX_RESPONSES: usize = 64;

/// Time allowed to receive the complete upload.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(15);

/// Time allowed to obtain headers or stream a response.
const RESPONSE_TIMEOUT: Duration = Duration::from_mins(3);

/// Time allowed between response body frames.
const IDLE_TIMEOUT: Duration = Duration::from_secs(15);

/// One process's bounded response capacity.
pub(crate) struct Limits {
    /// Slots held until each response body completes or is dropped.
    responses: Arc<Semaphore>,
}

impl Limits {
    /// Construct the universal process bounds.
    pub(crate) fn new() -> Self {
        Self {
            responses: Arc::new(Semaphore::new(MAX_RESPONSES)),
        }
    }
}

/// Render a safety rejection using the requested provider's error envelope.
fn reject(path: &str, error: NativeError) -> Response {
    if path.starts_with("/openai/") || path.starts_with("/gemini/v1beta/openai/") {
        eliza_server_openai::errors::OpenAiRejection::from(error).into_response()
    } else if path.starts_with("/anthropic/") {
        eliza_server_anthropic::errors::AnthropicRejection::from(error).into_response()
    } else if path.starts_with("/gemini/") {
        eliza_server_gemini::errors::GeminiRejection::from(error).into_response()
    } else if path.starts_with("/ollama/") {
        eliza_server_ollama::errors::OllamaRejection::from(error).into_response()
    } else {
        error.into_response()
    }
}

/// Bound upload storage and response ownership around provider routes.
pub(crate) async fn handle(
    State(limits): State<Arc<Limits>>,
    request: Request,
    next: Next,
) -> Response {
    // Read-only and preflight routes have no upload or generated body to bound.
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        return next.run(request).await;
    }

    let path = request.uri().path().to_owned();

    // Reject known oversized bodies before reserving a response slot.
    if request.body().size_hint().lower() > MAX_BODY_BYTES as u64 {
        return reject(&path, NativeError::too_large());
    }

    // Each active response, including streamed output, owns one process slot.
    let Ok(permit) = Arc::clone(&limits.responses).try_acquire_owned() else {
        return reject(&path, NativeError::unavailable());
    };

    // Buffer the bounded upload so route handlers never see an unbounded stream.
    let (parts, body) = request.into_parts();
    let upload =
        tokio::time::timeout(UPLOAD_TIMEOUT, Limited::new(body, MAX_BODY_BYTES).collect()).await;

    // Convert upload failures into provider-compatible errors.
    let body = match upload {
        Ok(Ok(body)) => body.to_bytes(),
        // Limited maps both body read failure and size overflow to rejection.
        Ok(Err(_)) => return reject(&path, NativeError::too_large()),
        // A stalled sender must not retain a process slot indefinitely.
        Err(_) => return reject(&path, NativeError::request_timeout()),
    };

    // Restore the request with its bounded body for the provider router.
    let request = Request::from_parts(parts, Body::from(body));

    // Header generation and streaming share one response deadline.
    let deadline = Instant::now() + RESPONSE_TIMEOUT;

    // An expired deadline must release the reserved response slot.
    let Ok(response) = tokio::time::timeout_at(deadline, next.run(request)).await else {
        return reject(&path, NativeError::unavailable());
    };

    // Stream response data while retaining the capacity slot.
    let (mut parts, body) = response.into_parts();
    parts.headers.insert(
        "cache-control",
        axum::http::HeaderValue::from_static("no-store"),
    );
    Response::from_parts(parts, Body::new(BoundedBody::new(body, permit, deadline)))
}

// -----------------------------------------------------------------------------
// BoundedBody: Retain capacity and bound streaming output.
// -----------------------------------------------------------------------------

/// Keeps the response slot until streaming ends or the client disconnects.
struct BoundedBody {
    /// Provider response body being streamed to the caller.
    inner: Body,
    /// Capacity held until this body is dropped.
    _permit: OwnedSemaphorePermit,
    /// Timer restarted after each body frame.
    idle: Pin<Box<Sleep>>,
    /// Absolute timer for the body stream.
    deadline: Pin<Box<Sleep>>,
    /// Byte budget remaining for the body.
    remaining: usize,
    /// Whether the stream has ended or failed.
    is_finished: bool,
}

impl BoundedBody {
    /// Wrap a provider response with output and time bounds.
    fn new(inner: Body, permit: OwnedSemaphorePermit, deadline: Instant) -> Self {
        Self {
            inner,
            _permit: permit,
            idle: Box::pin(tokio::time::sleep(IDLE_TIMEOUT)),
            deadline: Box::pin(tokio::time::sleep_until(deadline)),
            remaining: MAX_OUTPUT_BYTES,
            is_finished: false,
        }
    }

    /// Stop streaming and surface an I/O failure to the transport.
    fn fail(
        &mut self,
        kind: std::io::ErrorKind,
    ) -> Poll<Option<Result<Frame<Bytes>, axum::Error>>> {
        self.inner = Body::empty();
        self.is_finished = true;
        Poll::Ready(Some(Err(axum::Error::new(std::io::Error::from(kind)))))
    }
}

impl HttpBody for BoundedBody {
    type Data = Bytes;

    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        // Once complete, further polls cannot produce another frame.
        if self.is_finished {
            return Poll::Ready(None);
        }

        // Neither a stalled nor a long-lived stream may hold a slot forever.
        if self.idle.as_mut().poll(cx).is_ready() || self.deadline.as_mut().poll(cx).is_ready() {
            return self.fail(std::io::ErrorKind::TimedOut);
        }
        let frame = Pin::new(&mut self.inner).poll_frame(cx);
        if let Poll::Ready(Some(Ok(frame))) = &frame {
            let bytes = frame.data_ref().map_or(0, Bytes::len);

            // Stop before a frame would exceed the response byte budget.
            if bytes > self.remaining {
                return self.fail(std::io::ErrorKind::InvalidData);
            }
            self.remaining -= bytes;
            self.idle
                .as_mut()
                .reset(tokio::time::Instant::now() + IDLE_TIMEOUT);
        }
        if matches!(frame, Poll::Ready(None | Some(Err(_)))) {
            self.is_finished = true;
        }
        frame
    }

    fn is_end_stream(&self) -> bool {
        self.is_finished || self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}
