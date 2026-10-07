//! Classifies bounded input and pumps provider generation on owned CPU workers.

use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use eliza_http::execution::Execution;
use eliza_http::hosted::{Caller, CapacityLease, Hosted, IdentityLease, Modality};
use eliza_http::problem::NativeError;
use http_body::{Body as _, Frame};
use http_body_util::BodyExt as _;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

// -----------------------------------------------------------------------------
// WorkKinds: Classifies media before native base64 decode and generation.
// -----------------------------------------------------------------------------

/// Shared resource and allowance classification before expensive provider lowering.
struct WorkKinds {
    /// Main request allowance.
    primary: Modality,
    /// Whether image generation or inline-image decoding needs an image slot.
    has_image_work: bool,
    /// Conservative embedding output estimate, absent for other or invalid requests.
    estimated_output_bytes: Option<usize>,
}

impl WorkKinds {
    /// Classify native Gemini media selectors and other provider media endpoints.
    fn new(path: &str, method: &Method, body: &Bytes) -> Self {
        let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
        let modalities = value
            .pointer("/generationConfig/responseModalities")
            .and_then(Value::as_array);
        let model = path
            .split_once("/models/")
            .and_then(|(_, action)| action.split(':').next());
        let audio = model == Some(eliza_modality_speech::core::MODEL_ID)
            || modalities.is_some_and(|values| values.iter().any(|value| value == "AUDIO"));
        let image = model == Some(eliza_modality_image::generation::MODEL_ID)
            || modalities.is_some_and(|values| values.iter().any(|value| value == "IMAGE"));
        let primary = if *method == Method::GET {
            Modality::Discovery
        } else if path.ends_with("/audio/speech") || audio {
            Modality::Speech
        } else if path.ends_with("/images/generations") || image {
            Modality::Image
        } else {
            Modality::Text
        };
        let has_image_work = primary == Modality::Image || Self::has_inline_image(&value);
        let estimated_output_bytes = Self::embedding_output(path, &value);
        Self {
            primary,
            has_image_work,
            estimated_output_bytes,
        }
    }

    /// Recognize image-bearing content objects without decoding their bytes.
    fn has_image_fields(fields: &serde_json::Map<String, Value>) -> bool {
        let images = fields.get("images").and_then(Value::as_array);
        let kind = fields.get("type").and_then(Value::as_str);
        fields.contains_key("inlineData")
            || fields.contains_key("inline_data")
            || images.is_some_and(|images| !images.is_empty())
            || kind.is_some_and(|kind| matches!(kind, "image" | "image_url" | "input_image"))
    }

    /// Recognize actual media shapes without decoding their binary payloads.
    fn has_inline_image(value: &Value) -> bool {
        match value {
            Value::Array(values) => values.iter().any(Self::has_inline_image),
            Value::Object(fields) => {
                Self::has_image_fields(fields) || fields.values().any(Self::has_inline_image)
            }
            _ => false,
        }
    }

    /// Estimate only valid embedding dimensions; native DTOs retain their own invalid-input errors.
    fn embedding_bytes(path: &str, value: &Value) -> Option<usize> {
        let default = eliza_modality_embedding::engine::MODEL_DEFAULT_DIMENSIONS;
        let is_native_gemini =
            path.ends_with("embedContent") || path.ends_with("batchEmbedContents");
        let requested = if is_native_gemini {
            value
                .get("outputDimensionality")
                .filter(|value| !value.is_null())
                .or_else(|| value.pointer("/embedContentConfig/outputDimensionality"))
        } else {
            value.get("dimensions")
        };

        // Interpret dimensions without overriding native validation errors.
        let dimensions = requested.and_then(Value::as_i64).unwrap_or(default);

        // Invalid dimensions must preserve their native 400 contract.
        if !(1..=eliza_modality_embedding::engine::MODEL_MAX_DIMENSIONS).contains(&dimensions) {
            return None;
        }

        // A failed architecture conversion leaves validation to the native adapter.
        let Ok(dimensions) = usize::try_from(dimensions) else {
            return None;
        };

        // Bound the requested vector batch independently of input string sizes.
        let count = if is_native_gemini {
            1
        } else {
            value
                .get("input")
                .and_then(Value::as_array)
                .map_or(1, Vec::len)
        };
        let vectors = count.saturating_mul(dimensions);
        Some(vectors.saturating_mul(WORK_POLICY_FLOAT_BYTES))
    }

    /// Estimate native batch vectors before any actual embedding allocation.
    fn embedding_output(path: &str, value: &Value) -> Option<usize> {
        // Other endpoints retain engine-specific bounded output generation.
        if !(path.ends_with("/embeddings")
            || path.ends_with("/embed")
            || path.ends_with("embedContent")
            || path.ends_with("batchEmbedContents"))
        {
            return None;
        }

        // Unknown request fields cannot replace another provider's native input contract.
        if !path.ends_with("batchEmbedContents") {
            return Self::embedding_bytes(path, value);
        }

        // Unary requests and native batch elements share the same estimate.
        let Some(requests) = value.get("requests").and_then(Value::as_array) else {
            return Self::embedding_bytes(path, value);
        };
        let mut total = 0usize;
        for request in requests {
            total = total.saturating_add(Self::embedding_bytes(path, request)?);
        }
        Some(total)
    }

    /// Refuse oversized embedding output before allocating native vectors.
    ///
    /// # Errors
    /// Returns the hosted encoded-output size rejection.
    fn validate_output(&self, limit: usize) -> Result<(), NativeError> {
        // Refuse expensive output allocation before generating native vectors.
        if self
            .estimated_output_bytes
            .is_some_and(|estimate| estimate > limit)
        {
            return Err(NativeError::too_large());
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// WorkPolicy: Bounds frame buffering and embedding output estimates.
// -----------------------------------------------------------------------------

/// Maximum encoded float width plus separators for conservative embedding estimates.
const WORK_POLICY_FLOAT_BYTES: usize = 32;

/// Encoded delivery frame bound independent of a provider's original chunk size.
const WORK_POLICY_FRAME_BYTES: usize = 64 * 1024;

/// Maximum retained delivery frames between worker and HTTP consumer.
const WORK_POLICY_FRAMES: usize = 1;

// -----------------------------------------------------------------------------
// WorkLease: Retains actual CPU, buffer and media admission.
// -----------------------------------------------------------------------------

/// Permits transferred into every actual nested worker through the request extension.
struct WorkLease {
    /// Retains CPU admission until actual generation and serialization end.
    _cpu: CapacityLease,
    /// Retains the raw input buffer budget while its allocation is live.
    _input: CapacityLease,
    /// Retains speech admission through both native work and response delivery.
    _speech: Option<Arc<CapacityLease>>,
    /// Retains image admission through both native work and response delivery.
    _image: Option<Arc<CapacityLease>>,
}

// -----------------------------------------------------------------------------
// DeliveryLease: Retains response admission through body completion.
// -----------------------------------------------------------------------------

/// Delivery admission remains separate from actual CPU-worker ownership.
pub(super) struct DeliveryLease {
    /// Retains global response admission until actual body completion.
    _capacity: CapacityLease,
    /// Retains the caller's concurrent-response accounting.
    _identity: IdentityLease,
    /// Retains speech response admission after CPU work completes.
    _speech: Option<Arc<CapacityLease>>,
    /// Retains image response admission after CPU work completes.
    _image: Option<Arc<CapacityLease>>,
    /// Watchdog ends exactly with response ownership.
    _deadline: crate::connection::ResponseDeadline,
}

// -----------------------------------------------------------------------------
// WorkResponse: Projects admitted provider frames into HTTP delivery.
// -----------------------------------------------------------------------------

/// Worker response metadata, bounded body frames and separate delivery admission.
pub(super) struct WorkResponse {
    /// Provider-native response metadata, with a bounded channel-backed body.
    response: Response,
    /// Actual delivery ownership, independent of CPU-worker completion.
    lease: Option<DeliveryLease>,
    /// Explicit nonsecret delivery policy.
    hosted: Arc<Hosted>,
    /// Media admission remains owned by the actual worker and delivery together.
    speech: Option<Arc<CapacityLease>>,
    /// Image admission, including inline image decoding.
    image: Option<Arc<CapacityLease>>,
}

impl WorkResponse {
    /// Construct a cheap origin or discovery response without generation media admission.
    pub(super) fn discovery(response: Response, hosted: Arc<Hosted>) -> Self {
        Self {
            response,
            hosted,
            lease: None,
            speech: None,
            image: None,
        }
    }

    /// Attach transport-owned admission only after successful native header production.
    pub(super) fn admit(
        mut self,
        capacity: CapacityLease,
        identity: IdentityLease,
        deadline: crate::connection::ResponseDeadline,
    ) -> Self {
        self.lease = Some(DeliveryLease {
            _capacity: capacity,
            _identity: identity,
            _speech: self.speech.take(),
            _image: self.image.take(),
            _deadline: deadline,
        });
        self
    }
}

impl IntoResponse for WorkResponse {
    fn into_response(self) -> Response {
        // Origin health and docs do not consume generation delivery admission.
        let Some(lease) = self.lease else {
            return self.response;
        };
        let (mut parts, body) = self.response.into_parts();
        parts.headers.insert(
            "cache-control",
            axum::http::HeaderValue::from_static("no-store"),
        );

        // Apply only transport-owned response limits after actual header production.
        let guarded = crate::admission::GuardedBody::new(body, lease, &self.hosted);
        Response::from_parts(parts, Body::new(guarded))
    }
}

// -----------------------------------------------------------------------------
// PreparedWork: Retains actual execution while native frames are produced.
// -----------------------------------------------------------------------------

/// Prepared response and actual execution retained until all provider frames are produced.
struct PreparedWork {
    /// Native response awaiting publication.
    result: WorkResponse,
    /// Provider body polled only on the actual CPU worker.
    body: Body,
    /// Bounded encoded delivery channel.
    frames: mpsc::Sender<Result<Frame<Bytes>, axum::Error>>,
    /// Actual work admission retained through the final produced frame.
    _execution: Execution,
    /// Encoded output byte bound.
    output: usize,
}

// -----------------------------------------------------------------------------
// Work: Executes provider generation off the async runtime.
// -----------------------------------------------------------------------------

/// Origin execution boundary, shared by every provider and modality.
pub(super) struct Work {
    /// Explicit immutable admission policy.
    pub(super) hosted: Arc<Hosted>,
    /// Confirmed caller and capacity class.
    pub(super) caller: Caller,
    /// CPU admission transferred into the actual worker.
    pub(super) cpu: CapacityLease,
    /// Raw input allocation admission.
    pub(super) input: CapacityLease,
}

impl Work {
    /// Send one frame or return a private disconnected-consumer error.
    ///
    /// # Errors
    /// Returns a broken-pipe marker when HTTP delivery has been canceled.
    fn send(
        sender: &mpsc::Sender<Result<Frame<Bytes>, axum::Error>>,
        frame: Result<Frame<Bytes>, axum::Error>,
    ) -> Result<(), axum::Error> {
        sender
            .blocking_send(frame)
            .map_err(|_| axum::Error::new(std::io::Error::from(std::io::ErrorKind::BrokenPipe)))
    }

    /// Split provider data into independently owned bounded delivery frames.
    ///
    /// # Errors
    /// Returns a disconnected-consumer error.
    fn send_data(
        mut data: Bytes,
        sender: &mpsc::Sender<Result<Frame<Bytes>, axum::Error>>,
    ) -> Result<(), axum::Error> {
        while !data.is_empty() {
            let length = data.len().min(WORK_POLICY_FRAME_BYTES);
            let chunk = Bytes::copy_from_slice(&data.split_to(length));
            Self::send(sender, Ok(Frame::data(chunk)))?;
        }
        Ok(())
    }

    /// Stop asynchronous body waits immediately when HTTP delivery disconnects.
    async fn next_frame(
        body: &mut Body,
        sender: &mpsc::Sender<Result<Frame<Bytes>, axum::Error>>,
    ) -> Option<Result<Frame<Bytes>, axum::Error>> {
        let canceled = std::pin::pin!(sender.closed());
        let frame = std::pin::pin!(body.frame());
        match futures_util::future::select(canceled, frame).await {
            futures_util::future::Either::Left(_) => None,
            futures_util::future::Either::Right((frame, _)) => frame,
        }
    }

    /// Pump provider frames entirely on the admitted blocking worker.
    ///
    /// # Errors
    /// Returns output-limit, provider-frame or disconnected-consumer failures.
    fn pump(
        mut body: Body,
        sender: &mpsc::Sender<Result<Frame<Bytes>, axum::Error>>,
        limit: usize,
    ) -> Result<(), axum::Error> {
        let runtime = tokio::runtime::Handle::current();
        let mut written = 0usize;
        while let Some(frame) = runtime.block_on(Self::next_frame(&mut body, sender)) {
            let frame = frame?;
            let data = match frame.into_data() {
                Ok(data) => data,

                // Trailers preserve their native protocol meaning.
                Err(trailers) => {
                    Self::send(sender, Ok(trailers))?;
                    continue;
                }
            };
            written = written.saturating_add(data.len());

            // Stop before delivering encoded bytes beyond the explicit output bound.
            if written > limit {
                return Err(axum::Error::new(std::io::Error::from(
                    std::io::ErrorKind::InvalidData,
                )));
            }
            Self::send_data(data, sender)?;
        }
        Ok(())
    }

    /// Lower and execute native handlers while retaining actual work ownership.
    ///
    /// # Errors
    /// Returns preflight size, capacity, allowance or native-worker failures.
    fn prepare(
        self,
        mut request: Request,
        bytes: Bytes,
        next: Next,
    ) -> Result<PreparedWork, NativeError> {
        let kinds = WorkKinds::new(request.uri().path(), request.method(), &bytes);
        let output = self.hosted.config().output_bytes;
        kinds.validate_output(output)?;

        // Claim every required shared modality gate before spending personal allowance.
        let speech = if kinds.primary == Modality::Speech {
            Some(Arc::new(self.hosted.speech.acquire(self.caller.tier(), 1)?))
        } else {
            None
        };
        let image = if kinds.has_image_work {
            Some(Arc::new(self.hosted.image.acquire(self.caller.tier(), 1)?))
        } else {
            None
        };

        // Charge personal allowances only after every required shared media gate succeeds.
        self.hosted.charge(&self.caller, kinds.primary)?;
        if kinds.has_image_work && kinds.primary != Modality::Image {
            self.hosted.charge(&self.caller, Modality::Image)?;
        }

        // Move actual work ownership into the extension used by nested native workers.
        let lease = WorkLease {
            _cpu: self.cpu,
            _input: self.input,
            _speech: speech.clone(),
            _image: image.clone(),
        };

        // Make worker ownership available to actual nested native speech jobs.
        let execution = Execution::new(lease);
        request.extensions_mut().insert(execution.clone());
        *request.body_mut() = Body::from(bytes);

        // Native handlers and lazy serializers are polled only on this admitted worker.
        let runtime = tokio::runtime::Handle::current();
        let response = runtime.block_on(next.run(request));

        // Finite unary bodies can reject oversized output before successful headers are sent.
        let output_length = response.body().size_hint().exact();

        // Reject an oversized unary response before publishing successful native headers.
        if output_length.is_some_and(|length| length > output as u64) {
            return Err(NativeError::too_large());
        }

        // Prepare one bounded delivery frame channel without retaining CPU in the HTTP body.
        let (parts, body) = response.into_parts();
        let (frames, receiver) = mpsc::channel(WORK_POLICY_FRAMES);
        let response = Response::from_parts(parts, Body::new(WorkBody { receiver }));

        // Response and caller concurrency stay with the HTTP future until headers are ready.
        let result = WorkResponse {
            response,
            lease: None,
            hosted: Arc::clone(&self.hosted),
            speech,
            image,
        };

        // Keep actual admission alive through the final produced frame.
        Ok(PreparedWork {
            result,
            body,
            frames,
            _execution: execution,
            output,
        })
    }

    /// Publish native headers once and keep pumping until actual completion or disconnect.
    fn run(
        self,
        request: Request,
        bytes: Bytes,
        next: Next,
        sender: oneshot::Sender<Result<WorkResponse, NativeError>>,
    ) {
        // Canceled requests that have not started native work need no generation.
        if sender.is_closed() {
            return;
        }

        // Finish native preparation before publishing response ownership.
        let prepared = match self.prepare(request, bytes, next) {
            Ok(prepared) => prepared,

            // Failed admission ends before any successful HTTP headers can be published.
            Err(error) => {
                match sender.send(Err(error)) {
                    Ok(()) | Err(_) => {}
                }
                return;
            }
        };

        // The execution guard remains in this scope until actual pumping completes.
        let PreparedWork {
            result,
            body,
            frames,
            _execution,
            output,
        } = prepared;

        // Header-waiter cancellation drops delivery; nested workers still own their work admission.
        if sender.send(Ok(result)).is_err() {
            return;
        }

        // Successful completion needs no terminal transport error.
        let Err(error) = Self::pump(body, &frames, output) else {
            return;
        };
        match Self::send(&frames, Err(error)) {
            Ok(()) | Err(_) => {}
        }
    }

    /// Spawn bounded actual work and expose only its header waiter to the async runtime.
    pub(super) fn start(
        self,
        request: Request,
        bytes: Bytes,
        next: Next,
    ) -> oneshot::Receiver<Result<WorkResponse, NativeError>> {
        let (sender, receiver) = oneshot::channel();
        tokio::task::spawn_blocking(move || self.run(request, bytes, next, sender));
        receiver
    }
}

// -----------------------------------------------------------------------------
// WorkBody: Delivers frames without executing generation on the async runtime.
// -----------------------------------------------------------------------------

/// Bounded channel-backed body; receiver drop cancels a still-running producer.
struct WorkBody {
    /// At most one bounded frame awaiting HTTP delivery.
    receiver: mpsc::Receiver<Result<Frame<Bytes>, axum::Error>>,
}

impl http_body::Body for WorkBody {
    type Data = Bytes;

    type Error = axum::Error;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        self.receiver.poll_recv(cx)
    }
}

// -----------------------------------------------------------------------------
// Tests: Exercises provider-specific preflight before allocation.
// -----------------------------------------------------------------------------

#[cfg(test)]
#[path = "work_tests.rs"]
mod tests;
