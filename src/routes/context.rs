//! Shared route configuration and provider authentication.

use std::sync::Arc;

use axum::http::HeaderMap;

use super::errors::AuthenticationError;
use crate::cli::{AuthMode, BearerToken};
use crate::types::model::ModelId;
use crate::types::turn::RequestLimits;

// -----------------------------------------------------------------------------
// RouteConfig: Stores behavior consumed by HTTP routes.
// -----------------------------------------------------------------------------

/// Route-visible server behavior without listener or tracing concerns.
#[derive(Debug, Clone, bon::Builder)]
pub(crate) struct RouteConfig {
    /// Provider-visible model identifier.
    pub(super) model: ModelId,
    /// Authentication policy for provider endpoints.
    auth: AuthMode,
    /// Expected credential when bearer authentication is enabled.
    #[builder(required)]
    bearer_token: Option<BearerToken>,
    /// Optional delay between streamed chunks.
    pub(super) stream_delay_ms: u64,
    /// Shared request-size and replay bounds.
    pub(super) limits: RequestLimits,
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
// ProviderAuth: Selects and validates provider-specific credentials.
// -----------------------------------------------------------------------------

/// Credential header used by one provider surface.
#[derive(Debug, Clone, Copy)]
pub(super) enum ProviderAuth {
    /// `Authorization: Bearer <token>`.
    Bearer,
    /// Raw API key in the named header.
    ApiKey(
        /// Header carrying the raw API key.
        &'static str,
    ),
}

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

/// Read the credential represented by one provider's authentication scheme.
///
/// # Errors
///
/// Returns an error when the selected header is present but not valid text.
fn provider_credential(
    headers: &HeaderMap,
    provider: ProviderAuth,
) -> Result<Option<&str>, axum::http::header::ToStrError> {
    match provider {
        ProviderAuth::Bearer => {
            provider_authenticate_header_text(headers, axum::http::header::AUTHORIZATION)
                .map(|value| value.and_then(|text| text.strip_prefix("Bearer ")))
        }
        ProviderAuth::ApiKey(name) => provider_authenticate_header_text(headers, name),
    }
}

/// Check provider authentication headers against route configuration.
///
/// # Errors
///
/// Returns a provider rejection when configured credentials do not match.
pub(super) fn provider_authenticate(
    headers: &HeaderMap,
    config: &RouteConfig,
    provider: ProviderAuth,
) -> Result<(), AuthenticationError> {
    // Endpoints are public when authentication is disabled.
    if config.auth == AuthMode::None {
        return Ok(());
    }

    // Bearer mode without a configured token is always unauthorized.
    let Some(expected) = config.bearer_token.as_ref() else {
        return Err(AuthenticationError::Failed);
    };

    // Compare only successfully decoded provider credentials.
    let supplied =
        provider_credential(headers, provider).map_err(|_| AuthenticationError::Failed)?;
    if supplied == Some(expected.as_str()) {
        Ok(())
    } else {
        Err(AuthenticationError::Failed)
    }
}
