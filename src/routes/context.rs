//! Shared route configuration and provider authentication.

use std::num::NonZeroUsize;
use std::sync::Arc;

use axum::http::HeaderMap;

use crate::cli::{AuthMode, BearerToken};
use crate::provider::contracts::{ModelId, ProviderRejection};

// -----------------------------------------------------------------------------
// RouteConfig: Stores behavior consumed by HTTP routes.
// -----------------------------------------------------------------------------

/// Route-visible server behavior without listener or tracing concerns.
#[derive(Debug, Clone)]
pub(crate) struct RouteConfig {
    /// Provider-visible model identifier.
    pub(super) model: ModelId,
    /// Authentication policy for provider endpoints.
    auth: AuthMode,
    /// Expected credential when bearer authentication is enabled.
    bearer_token: Option<BearerToken>,
    /// Optional delay between streamed chunks.
    pub(super) stream_delay_ms: u64,
    /// Maximum accepted request text size.
    pub(super) max_input_chars: NonZeroUsize,
    /// Maximum accepted replay history length.
    pub(super) max_history_messages: NonZeroUsize,
}

impl RouteConfig {
    /// Collect the configuration consumed by HTTP routes.
    pub(crate) fn new(
        model: ModelId,
        auth: AuthMode,
        bearer_token: Option<BearerToken>,
        stream_delay_ms: u64,
        max_input_chars: NonZeroUsize,
        max_history_messages: NonZeroUsize,
    ) -> Self {
        Self {
            model,
            auth,
            bearer_token,
            stream_delay_ms,
            max_input_chars,
            max_history_messages,
        }
    }
}

// -----------------------------------------------------------------------------
// AppState: Shares immutable route configuration.
// -----------------------------------------------------------------------------

/// Shared immutable state cloned into every provider route.
#[derive(Debug, Clone)]
pub(super) struct AppState {
    /// Validated behavior shared by handlers.
    pub(super) config: Arc<RouteConfig>,
}

// -----------------------------------------------------------------------------
// ProviderAuthenticate: Validates shared provider credentials.
// -----------------------------------------------------------------------------

/// Read an optional textual header without hiding malformed values.
///
/// # Errors
///
/// Returns an error when a present header is not valid text.
fn provider_authenticate_header_text(
    headers: &HeaderMap,
    name: impl axum::http::header::AsHeaderName,
) -> Result<Option<&str>, axum::http::header::ToStrError> {
    // Absence is valid and distinct from a malformed present value.
    let Some(value) = headers.get(name) else {
        return Ok(None);
    };
    value.to_str().map(Some)
}

/// Check provider authentication headers against route configuration.
///
/// # Errors
///
/// Returns a provider rejection when configured credentials do not match.
pub(super) fn provider_authenticate(
    headers: &HeaderMap,
    config: &RouteConfig,
) -> Result<(), ProviderRejection> {
    // Endpoints are public when authentication is disabled.
    if config.auth == AuthMode::None {
        return Ok(());
    }

    // Bearer mode without a configured token is always unauthorized.
    let Some(expected) = config.bearer_token.as_ref() else {
        return Err(ProviderRejection::unauthorized());
    };
    let bearer = provider_authenticate_header_text(headers, axum::http::header::AUTHORIZATION)
        .map_err(|_| ProviderRejection::unauthorized())?;
    let bearer_matches = bearer
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == expected.as_str());
    let api_key = provider_authenticate_header_text(headers, "x-api-key")
        .map_err(|_| ProviderRejection::unauthorized())?;
    if bearer_matches || api_key.is_some_and(|token| token == expected.as_str()) {
        Ok(())
    } else {
        Err(ProviderRejection::unauthorized())
    }
}
