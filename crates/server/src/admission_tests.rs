//! Tests actual worker ownership independently of CPU timing and native voice speed.

use std::sync::{Condvar, Mutex};

use axum::Extension;
use eliza_http::hosted::{AccessTier, HostedConfig, HostedSecrets};
use futures_util::StreamExt as _;
use tower::ServiceExt as _;

use super::*;

// -----------------------------------------------------------------------------
// AdmissionTestGate: Holds a started native handler until explicit release.
// -----------------------------------------------------------------------------

/// Deterministic worker barrier; no test result depends on synthesis timing.
#[derive(Default)]
struct AdmissionTestGate {
    /// Reports that the actual blocking worker began native handler execution.
    started: tokio::sync::Notify,
    /// Whether the worker can finish.
    is_released: Mutex<bool>,
    /// Wakes the actual worker after cancellation assertions finish.
    changed: Condvar,
}

impl AdmissionTestGate {
    /// Release actual work after verifying canceled HTTP ownership.
    fn release(&self) {
        *self.is_released.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

// -----------------------------------------------------------------------------
// AdmissionTestRelease: Releases blocked work during panic unwinding too.
// -----------------------------------------------------------------------------

/// Teardown ownership prevents failed assertions leaving a native worker blocked.
struct AdmissionTestRelease(
    /// Actual worker gate released on every test exit path.
    Arc<AdmissionTestGate>,
);

impl Drop for AdmissionTestRelease {
    fn drop(&mut self) {
        self.0.release();
    }
}

// -----------------------------------------------------------------------------
// AdmissionTest: Proves hard capacity and separate worker/delivery lifetimes.
// -----------------------------------------------------------------------------

/// Generous scheduling deadline without performance assertions.
const ADMISSION_TEST_SECONDS: u64 = 5;

/// Valid local ingress fixture secret.
const ADMISSION_TEST_INGRESS: &str = "admission-fixture-ingress-at-least-32-bytes";

/// Independent local entitlement fixture value; never a production credential.
const ADMISSION_TEST_ENTITLEMENT: &str = "admission-fixture-billing-at-least-32-bytes";

/// Build explicit staged policy with no remote billing dependency for public callers.
fn admission_test_policy() -> Arc<Hosted> {
    let config: HostedConfig =
        serde_json::from_str(include_str!("../../../config/hosted-staging.json")).unwrap();
    Arc::new(
        Hosted::new(
            config,
            HostedSecrets {
                ingress_secret: ADMISSION_TEST_INGRESS.into(),
                entitlement_secret: ADMISSION_TEST_ENTITLEMENT.into(),
            },
        )
        .unwrap(),
    )
}

/// Block only the actual generation worker, never the Tokio executor.
fn admission_test_handler(
    State(gate): State<Arc<AdmissionTestGate>>,
) -> std::future::Ready<&'static str> {
    gate.started.notify_one();
    let mut released = gate.is_released.lock().unwrap();
    while !*released {
        released = gate.changed.wait(released).unwrap();
    }
    std::future::ready("finished")
}

/// Construct a valid public request through authenticated ingress.
fn admission_test_request(path: &str, method: axum::http::Method) -> Request {
    let builder = Request::builder().uri(path).method(method);
    let builder = builder
        .header("x-eliza-ingress-token", ADMISSION_TEST_INGRESS)
        .header("x-eliza-client-ip", "192.0.2.1")
        .header("content-type", "application/json");
    builder.body(Body::from("{}")).unwrap()
}

/// Cancellation frees delivery immediately, while a started actual job keeps CPU and buffer admission.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admission_test_cancellation_keeps_actual_cpu() {
    let hosted = admission_test_policy();
    let gate = Arc::new(AdmissionTestGate::default());
    let _release = AdmissionTestRelease(Arc::clone(&gate));
    let router = axum::Router::new()
        .route(
            "/openai/v1/chat/completions",
            axum::routing::post(admission_test_handler),
        )
        .route(
            "/openai/v1/models",
            axum::routing::get(|| async { "catalog" }),
        )
        .with_state(Arc::clone(&gate))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&hosted),
            Admission::handle,
        ))
        .layer(Extension(ConnectInfo(crate::connection::Peer::from(
            "127.0.0.1:1".parse::<std::net::SocketAddr>().unwrap(),
        ))));
    let request = admission_test_request("/openai/v1/chat/completions", axum::http::Method::POST);
    let pending = tokio::spawn(router.clone().oneshot(request));
    tokio::time::timeout(
        Duration::from_secs(ADMISSION_TEST_SECONDS),
        gate.started.notified(),
    )
    .await
    .unwrap();
    let other_jobs = hosted.cpu.acquire(AccessTier::Public, 2).unwrap();
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    assert!(hosted.cpu.acquire(AccessTier::Public, 1).is_err());
    let buffer = hosted.config().buffer_bytes - hosted.config().reserved_buffer_bytes;
    let buffer = u32::try_from(buffer).unwrap();
    assert!(hosted.buffers.acquire(AccessTier::Public, buffer).is_err());
    let protected = hosted.cpu.acquire(AccessTier::Supporter, 1).unwrap();
    let responses = hosted.responses.acquire(AccessTier::Public, 48).unwrap();
    drop(responses);
    let discovery = router.oneshot(admission_test_request(
        "/openai/v1/models",
        axum::http::Method::GET,
    ));
    let discovery = tokio::time::timeout(Duration::from_secs(ADMISSION_TEST_SECONDS), discovery)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(discovery.status(), axum::http::StatusCode::OK);
    let body = discovery.into_body().collect().await.unwrap();
    assert_eq!(body.to_bytes(), "catalog");
    gate.release();
    tokio::time::timeout(Duration::from_secs(ADMISSION_TEST_SECONDS), async {
        loop {
            if let Ok(permit) = hosted.cpu.acquire(AccessTier::Public, 1) {
                drop(permit);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    drop(protected);
    drop(other_jobs);
}

/// Upload size admission rejects before allocating or starting a worker.
#[tokio::test]
async fn admission_test_size_failure_preserves_native_envelope() {
    let hosted = admission_test_policy();
    let body = Body::from(vec![b'x'; hosted.config().body_bytes + 1]);
    let mut request =
        admission_test_request("/openai/v1/chat/completions", axum::http::Method::POST);
    *request.body_mut() = body;
    let router = axum::Router::new()
        .route(
            "/openai/v1/chat/completions",
            axum::routing::post(|| async { "unexpected" }),
        )
        .layer(axum::middleware::from_fn_with_state(
            hosted,
            Admission::handle,
        ))
        .layer(Extension(ConnectInfo(crate::connection::Peer::from(
            "127.0.0.1:1".parse::<std::net::SocketAddr>().unwrap(),
        ))));
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::PAYLOAD_TOO_LARGE);
    let body = response.into_body().collect().await.unwrap();
    let bytes = body.to_bytes();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"]["type"], "invalid_request_error");
    assert_eq!(
        value["error"]["code"],
        "eliza::admission::request_too_large"
    );
}

/// Stalled uploads have a native HTTP 408 and release admission without a payment offer.
#[tokio::test]
async fn admission_test_upload_timeout_releases_delivery() {
    let mut config: HostedConfig =
        serde_json::from_str(include_str!("../../../config/hosted-staging.json")).unwrap();
    config.upload_seconds = 1;
    let hosted = Arc::new(
        Hosted::new(
            config,
            HostedSecrets {
                ingress_secret: ADMISSION_TEST_INGRESS.into(),
                entitlement_secret: ADMISSION_TEST_ENTITLEMENT.into(),
            },
        )
        .unwrap(),
    );
    let stream = futures_util::stream::pending::<Result<Bytes, std::io::Error>>();
    let mut request =
        admission_test_request("/openai/v1/chat/completions", axum::http::Method::POST);
    *request.body_mut() = Body::from_stream(stream);
    let router = axum::Router::new()
        .route(
            "/openai/v1/chat/completions",
            axum::routing::post(|| async { "unexpected" }),
        )
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&hosted),
            Admission::handle,
        ))
        .layer(Extension(ConnectInfo(crate::connection::Peer::from(
            "127.0.0.1:1".parse::<std::net::SocketAddr>().unwrap(),
        ))));
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::REQUEST_TIMEOUT);
    let body = response.into_body().collect().await.unwrap();
    let bytes = body.to_bytes();
    assert!(!String::from_utf8_lossy(&bytes).contains("$5"));
    let released = hosted.responses.acquire(AccessTier::Public, 48).unwrap();
    drop(released);
}

/// Produce headers and one frame, then remain idle so only the absolute socket deadline ends it.
fn admission_test_stalled_stream() -> std::future::Ready<Body> {
    let initial = futures_util::stream::once(std::future::ready(Ok::<_, std::io::Error>(
        Bytes::from_static(b"started"),
    )));
    let stream = initial.chain(futures_util::stream::pending());
    std::future::ready(Body::from_stream(stream))
}

/// Absolute response deadlines interrupt actual TCP IO without waiting for body idle expiry.
#[tokio::test]
async fn admission_test_deadline_interrupts_real_socket() {
    let mut config: HostedConfig =
        serde_json::from_str(include_str!("../../../config/hosted-staging.json")).unwrap();
    config.response_seconds = 1;
    config.idle_seconds = ADMISSION_TEST_SECONDS * 2;
    let hosted = Arc::new(
        Hosted::new(
            config,
            HostedSecrets {
                ingress_secret: ADMISSION_TEST_INGRESS.into(),
                entitlement_secret: ADMISSION_TEST_ENTITLEMENT.into(),
            },
        )
        .unwrap(),
    );
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = socket.local_addr().unwrap();
    let listener = crate::connection::OriginListener::new(socket, Some(hosted.config()));
    let router = axum::Router::new()
        .route(
            "/openai/v1/chat/completions",
            axum::routing::post(admission_test_stalled_stream),
        )
        .layer(axum::middleware::from_fn_with_state(
            hosted,
            Admission::handle,
        ));
    let server = tokio::spawn(async move {
        let service = router.into_make_service_with_connect_info::<crate::connection::Peer>();
        let served = axum::serve(listener, service).await;
        served.unwrap();
    });
    let builder = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(ADMISSION_TEST_SECONDS));
    let client = builder.build().unwrap();
    let request = client
        .post(format!("http://{address}/openai/v1/chat/completions"))
        .header("x-eliza-ingress-token", ADMISSION_TEST_INGRESS)
        .header("x-eliza-client-ip", "192.0.2.2");
    let request = request.json(&serde_json::json!({}));
    let sent = request.send().await;
    let response = sent.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let error = response.bytes().await.unwrap_err();
    server.abort();
    let is_client_timeout = error.is_timeout();
    assert!(!is_client_timeout);
}
