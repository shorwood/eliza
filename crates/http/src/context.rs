//! Shared route configuration and provider authentication.

use std::str::FromStr;

use axum::http::HeaderMap;
use miette::Diagnostic;
use thiserror::Error;

use crate::model::ModelId;
use crate::problem::{ProblemClass, ProblemDetails};
use crate::response::RequestLimits;

// -----------------------------------------------------------------------------
// AuthMode: Controls provider endpoint authentication.
// -----------------------------------------------------------------------------

/// Authentication mode for provider-compatible endpoints.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AuthMode {
    /// Do not require provider authentication.
    None,
    /// Require each provider's native bearer or API-key header.
    Bearer,
}

// -----------------------------------------------------------------------------
// BearerToken: Validates and redacts the shared provider credential.
// -----------------------------------------------------------------------------

/// Invalid shared bearer/API-key token.
#[derive(Debug, Diagnostic, Error)]
pub enum BearerTokenError {
    /// Tokens cannot be empty.
    #[error("bearer token must not be empty")]
    #[diagnostic(code(eliza::config::empty_bearer_token))]
    Empty,
}

/// Shared bearer/API-key token used when authentication is enabled.
#[derive(Clone, Eq, PartialEq)]
pub struct BearerToken(
    /// Validated nonempty secret value.
    String,
);

impl BearerToken {
    /// Return the raw token value for header comparison.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BearerToken {
    type Error = BearerTokenError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            Err(BearerTokenError::Empty)
        } else {
            Ok(Self(value))
        }
    }
}

impl FromStr for BearerToken {
    type Err = BearerTokenError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl AsRef<str> for BearerToken {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Debug for BearerToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BearerToken(<redacted>)")
    }
}

// -----------------------------------------------------------------------------
// AuthenticationError: Rejects invalid provider credentials.
// -----------------------------------------------------------------------------

/// Provider authentication failure.
#[derive(Debug, Diagnostic, Error)]
pub enum AuthenticationError {
    /// Configured credentials were absent, malformed, or incorrect.
    #[error("authentication failed")]
    #[diagnostic(code(eliza::auth::failed))]
    Failed,
}

impl ProblemDetails for AuthenticationError {
    fn class(&self) -> ProblemClass {
        ProblemClass::Authentication
    }
}

// -----------------------------------------------------------------------------
// RouteConfig: Stores behavior consumed by HTTP routes.
// -----------------------------------------------------------------------------

/// Route-visible server behavior without listener or tracing concerns.
#[derive(Debug, Clone, bon::Builder)]
pub struct RouteConfig {
    /// Provider-visible model identifier.
    pub model: ModelId,
    /// Authentication policy for provider endpoints.
    auth: AuthMode,
    /// Expected credential when bearer authentication is enabled.
    #[builder(required)]
    bearer_token: Option<BearerToken>,
    /// Optional delay between streamed chunks.
    pub stream_delay_ms: u64,
    /// Shared request-size and replay bounds.
    pub limits: RequestLimits,
}

// -----------------------------------------------------------------------------
// ProviderAuth: Selects and validates provider-specific credentials.
// -----------------------------------------------------------------------------

/// Credential header used by one provider surface.
#[derive(Debug, Clone, Copy)]
pub enum ProviderAuth {
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
pub fn provider_authenticate(
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
