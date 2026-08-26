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

use crate::serve::{ServerConfig, router};

/// Sends one JSON request through the in-process server router.
pub(crate) async fn post_json_with(
    config: ServerConfig,
    uri: &str,
    body: Value,
    headers: &[(&str, &str)],
) -> (StatusCode, String) {
    let mut request = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = router(config)
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
    (
        status,
        String::from_utf8(body.to_vec()).expect("body should be utf8"),
    )
}
