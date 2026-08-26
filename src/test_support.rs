//! Test-only driver for the Axum HTTP boundary.
//!
//! Provider checks send real requests through the public router. This module
//! builds those requests and collects replies; it does not copy route logic.

#![allow(
    clippy::missing_panics_doc,
    reason = "test support uses expect to fail tests clearly"
)]

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt;

use crate::serve::{ServerConfig, run_server_test_router};

/// One additional HTTP header supplied by a provider compatibility test.
pub(crate) struct TestHeader<'a> {
    /// Case-insensitive HTTP field name.
    pub(crate) name: &'a str,
    /// Textual field value.
    pub(crate) value: &'a str,
}

/// Collected status and UTF-8 response body from an in-process request.
pub(crate) struct TestResponse {
    /// HTTP status returned by the router.
    pub(crate) status: StatusCode,
    /// Fully collected UTF-8 response body.
    pub(crate) body: String,
}

/// Sends one JSON request through the in-process server router.
pub(crate) async fn post_json_with(
    config: ServerConfig,
    uri: &str,
    body: Value,
    headers: &[TestHeader<'_>],
) -> TestResponse {
    let mut request = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    for header in headers {
        request = request.header(header.name, header.value);
    }
    let response = run_server_test_router(config)
        .oneshot(
            request
                .body(Body::from(body.to_string()))
                .expect("request should build"),
        )
        .await
        .expect("router should respond");
    let status = response.status();
    let body = to_bytes(response.into_body(), 1_048_576)
        .await
        .expect("body should collect");
    TestResponse {
        status,
        body: String::from_utf8(body.to_vec()).expect("body should be utf8"),
    }
}
