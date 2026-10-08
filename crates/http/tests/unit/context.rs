use std::num::NonZeroUsize;

use axum::http::{HeaderMap, HeaderValue, header};

use super::*;

/// Non-text headers remain authentication failures.
#[test]
fn authentication_rejects_non_text_credentials() {
    let config = RouteConfig::new(
        "test-chat".parse().unwrap(),
        Some(ApiKey("secret".to_owned())),
        0,
        RequestLimits::new(NonZeroUsize::MIN, NonZeroUsize::MIN),
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_bytes(b"Bearer \xff").unwrap(),
    );
    assert!(config.authenticate(&headers, ProviderAuth::Bearer).is_err());
}
