//! Shared route configuration and provider authentication.

use std::num::NonZeroUsize;
use std::str::FromStr;

use axum::http::HeaderMap;
use miette::Diagnostic;
use thiserror::Error;

use crate::model::ModelId;
use crate::problem::{ApiError, ProblemClass};

// -----------------------------------------------------------------------------
// ApiKey: Validates and redacts the shared provider credential.
// -----------------------------------------------------------------------------

/// Invalid shared bearer/API-key token.
#[derive(Debug, Diagnostic, Error)]
pub enum ApiKeyError {
    /// API keys cannot be empty.
    #[error("API key must not be empty")]
    #[diagnostic(code(eliza::config::empty_api_key))]
    Empty,
}

/// Shared bearer/API-key token used when authentication is enabled.
#[derive(Clone, Eq, PartialEq)]
pub struct ApiKey(
    /// Validated nonempty secret value.
    String,
);

impl ApiKey {
    /// Return the raw token value for header comparison.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ApiKey {
    type Error = ApiKeyError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            Err(ApiKeyError::Empty)
        } else {
            Ok(Self(value))
        }
    }
}

impl FromStr for ApiKey {
    type Err = ApiKeyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl AsRef<str> for ApiKey {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ApiKey(<redacted>)")
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

impl ApiError for AuthenticationError {
    fn class(&self) -> ProblemClass {
        ProblemClass::Authentication
    }
}

// -----------------------------------------------------------------------------
// RequestLimits: Bounds normalized request size and replay work.
// -----------------------------------------------------------------------------

/// Bounds transcript size and replay work.
#[derive(Debug, Clone, Copy)]
pub struct RequestLimits {
    /// Maximum serialized input size across all request components.
    max_input_chars: NonZeroUsize,
    /// Maximum number of normalized history turns.
    max_history_messages: NonZeroUsize,
}

impl RequestLimits {
    /// Construct validated request bounds.
    #[must_use]
    pub const fn new(max_input_chars: NonZeroUsize, max_history_messages: NonZeroUsize) -> Self {
        Self {
            max_input_chars,
            max_history_messages,
        }
    }

    /// Return the maximum serialized input size.
    #[must_use]
    pub const fn max_input_chars(self) -> NonZeroUsize {
        self.max_input_chars
    }

    /// Return the maximum normalized history length.
    #[must_use]
    pub const fn max_history_messages(self) -> NonZeroUsize {
        self.max_history_messages
    }
}

// -----------------------------------------------------------------------------
// RouteConfig: Stores behavior consumed by HTTP routes.
// -----------------------------------------------------------------------------

/// Route-visible server behavior without listener or tracing concerns.
#[derive(Debug, Clone)]
pub struct RouteConfig {
    /// Provider-visible chat model identifier.
    pub chat_model: ModelId,
    /// Expected credential when provider authentication is enabled.
    api_key: Option<ApiKey>,
    /// Optional delay between streamed chunks.
    pub stream_delay_ms: u64,
    /// Shared request-size and replay bounds.
    pub limits: RequestLimits,
    /// Maximum simultaneous speech jobs, shared across provider routes.
    pub speech_workers: NonZeroUsize,
}

impl RouteConfig {
    /// Construct behavior shared by every provider route.
    #[must_use]
    pub const fn new(
        chat_model: ModelId,
        api_key: Option<ApiKey>,
        stream_delay_ms: u64,
        limits: RequestLimits,
    ) -> Self {
        Self {
            chat_model,
            api_key,
            stream_delay_ms,
            limits,
            speech_workers: NonZeroUsize::MIN.saturating_add(1),
        }
    }

    /// Set the shared speech concurrency limit.
    #[must_use]
    pub const fn with_speech_workers(mut self, workers: NonZeroUsize) -> Self {
        self.speech_workers = workers;
        self
    }

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
        // Omitting a configured key leaves provider endpoints public.
        let Some(expected) = self.api_key.as_ref() else {
            return Ok(());
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
    fn route_config_with_api_key(api_key: Option<&str>) -> RouteConfig {
        RouteConfig::new(
            "test-chat"
                .parse()
                .expect("test chat model should be valid"),
            api_key.map(|key| ApiKey(key.to_owned())),
            0,
            RequestLimits::new(NonZeroUsize::MIN, NonZeroUsize::MIN),
        )
    }

    /// Authentication-disabled routes ignore provider headers.
    #[test]
    fn disabled_authentication_accepts_missing_credentials() {
        let config = route_config_with_api_key(None);

        assert!(
            config
                .authenticate(&HeaderMap::new(), ProviderAuth::Bearer)
                .is_ok()
        );
    }

    /// Both supported provider schemes compare their decoded credential.
    #[test]
    fn authentication_accepts_matching_credentials() {
        let config = route_config_with_api_key(Some("secret"));
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
        let config = route_config_with_api_key(Some("secret"));
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
