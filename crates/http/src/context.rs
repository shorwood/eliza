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

impl RouteConfig {
    /// Check provider authentication headers against this route configuration.
    ///
    /// # Errors
    ///
    /// Returns a provider rejection when configured credentials do not match.
    pub fn authenticate(
        &self,
        headers: &HeaderMap,
        provider: ProviderAuth,
    ) -> Result<(), AuthenticationError> {
        // Endpoints are public when authentication is disabled.
        if self.auth == AuthMode::None {
            return Ok(());
        }

        // Bearer mode without a configured token is always unauthorized.
        let Some(expected) = self.bearer_token.as_ref() else {
            return Err(AuthenticationError::Failed);
        };

        // Compare only successfully decoded provider credentials.
        let supplied = provider
            .credential(headers)
            .map_err(|_| AuthenticationError::Failed)?;
        if supplied == Some(expected.as_str()) {
            Ok(())
        } else {
            Err(AuthenticationError::Failed)
        }
    }
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

impl ProviderAuth {
    /// Read an optional textual header without hiding malformed values.
    ///
    /// # Errors
    ///
    /// Returns an error when a present header is not valid text.
    fn header_text(
        headers: &HeaderMap,
        name: impl axum::http::header::AsHeaderName,
    ) -> Result<Option<&str>, axum::http::header::ToStrError> {
        // Absence is valid and distinct from a malformed present value.
        let Some(value) = headers.get(name) else {
            return Ok(None);
        };
        value.to_str().map(Some)
    }

    /// Read the credential represented by this authentication scheme.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected header is present but not valid text.
    fn credential(
        self,
        headers: &HeaderMap,
    ) -> Result<Option<&str>, axum::http::header::ToStrError> {
        match self {
            Self::Bearer => Self::header_text(headers, axum::http::header::AUTHORIZATION)
                .map(|value| value.and_then(|text| text.strip_prefix("Bearer "))),
            Self::ApiKey(name) => Self::header_text(headers, name),
        }
    }
}

// -----------------------------------------------------------------------------
// Tests: Verify public and authenticated route policies.
// -----------------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::missing_panics_doc,
    reason = "test assertions are the intended panic contract"
)]
#[expect(
    rlib::missing_section_dividers,
    reason = "compact authentication scenarios share one responsibility"
)]
mod tests {
    use std::num::NonZeroUsize;

    use axum::http::{HeaderMap, HeaderValue, header};

    use super::*;

    /// Build the smallest route configuration needed by authentication tests.
    fn route_config(auth: AuthMode, bearer_token: Option<&str>) -> RouteConfig {
        RouteConfig {
            model: ModelId::default(),
            auth,
            bearer_token: bearer_token.map(|token| BearerToken(token.to_owned())),
            stream_delay_ms: 0,
            limits: RequestLimits::builder()
                .max_input_chars(NonZeroUsize::MIN)
                .max_history_messages(NonZeroUsize::MIN)
                .build(),
        }
    }

    /// Authentication-disabled routes ignore provider headers.
    #[test]
    fn disabled_authentication_accepts_missing_credentials() {
        let config = route_config(AuthMode::None, None);

        assert!(
            config
                .authenticate(&HeaderMap::new(), ProviderAuth::Bearer)
                .is_ok()
        );
    }

    /// Both supported provider schemes compare their decoded credential.
    #[test]
    fn authentication_accepts_matching_credentials() {
        let config = route_config(AuthMode::Bearer, Some("secret"));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret"),
        );
        headers.insert("x-api-key", HeaderValue::from_static("secret"));

        assert!(config.authenticate(&headers, ProviderAuth::Bearer).is_ok());
        assert!(
            config
                .authenticate(&headers, ProviderAuth::ApiKey("x-api-key"))
                .is_ok()
        );
    }

    /// Missing, malformed, and mismatched credentials are rejected alike.
    #[test]
    fn authentication_rejects_invalid_credentials() {
        let config = route_config(AuthMode::Bearer, Some("secret"));
        let mut headers = HeaderMap::new();

        // A required credential cannot be omitted.
        assert!(config.authenticate(&headers, ProviderAuth::Bearer).is_err());

        // A decoded credential must match exactly.
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer wrong"),
        );
        assert!(config.authenticate(&headers, ProviderAuth::Bearer).is_err());

        // A present but non-text header remains an authentication failure.
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_bytes(b"Bearer \xff").unwrap(),
        );
        assert!(config.authenticate(&headers, ProviderAuth::Bearer).is_err());
    }
}
