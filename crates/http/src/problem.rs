//! Typed public problem contracts with provider-native projections.
#![expect(
    rlib::undocumented_items,
    reason = "problems 0.1.1 generates undocumented definition constants in a sibling impl; handwritten items below are documented"
)]

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use miette::Diagnostic;
use problems::{IntoReport as _, Problem as _, Report};
use thiserror::Error;

// -----------------------------------------------------------------------------
// ApiProblem: Declares public status, detail and safe diagnostic metadata.
// -----------------------------------------------------------------------------

/// Shared public API contracts. Provider adapters preserve their native envelopes.
#[derive(Debug, Error, problems::Problem)]
pub enum ApiProblem {
    /// Invalid or unsupported provider input.
    #[error("{message}")]
    #[problem(
        type_uri = "urn:eliza:problem:invalid-request",
        status = 400,
        detail = "{message}"
    )]
    InvalidRequest {
        /// Safe client recovery message.
        message: String,
    },
    /// Provider authentication failed.
    #[error("{message}")]
    #[problem(
        type_uri = "urn:eliza:problem:authentication",
        status = 401,
        detail = "{message}"
    )]
    Authentication {
        /// Safe authentication message.
        message: String,
    },
    /// Input exceeds an explicit request safety bound.
    #[error("{message}")]
    #[problem(
        type_uri = "urn:eliza:problem:request-too-large",
        status = 413,
        detail = "{message}"
    )]
    RequestTooLarge {
        /// Safe bound explanation.
        message: String,
    },
    /// Preserve a framework rejection's validated status.
    #[error("{message}")]
    #[problem(type_uri = "urn:eliza:problem:extraction", detail = "{message}")]
    Extraction {
        /// Original framework status.
        #[problem(status)]
        status: u16,
        /// Safe rejection message.
        message: String,
    },
    /// Private implementation failures expose no diagnostic detail.
    #[error("internal server error")]
    #[problem(
        type_uri = "urn:eliza:problem:internal",
        status = 500,
        title = "Internal server error"
    )]
    Internal,
    /// A caller has exhausted its own allowance.
    #[error("{message}")]
    #[problem(
        type_uri = "urn:eliza:problem:rate-limit",
        status = 429,
        detail = "{message}"
    )]
    RateLimit {
        /// Safe retry guidance, with an offer only for anonymous allowances.
        message: String,
    },
    /// Shared capacity or a required service is unavailable.
    #[error("service temporarily unavailable")]
    #[problem(
        type_uri = "urn:eliza:problem:unavailable",
        status = 503,
        detail = "Service temporarily unavailable; retry shortly."
    )]
    Unavailable,
}

// -----------------------------------------------------------------------------
// ProblemClass: Translates application failures to provider-native categories.
// -----------------------------------------------------------------------------

/// Provider adapter classification; HTTP contracts are declared by `ApiProblem`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ProblemClass {
    /// Invalid provider input.
    InvalidRequest,
    /// Unsupported provider input.
    UnsupportedRequest,
    /// Authentication failed.
    Authentication,
    /// Request safety limit exceeded.
    RequestTooLarge,
    /// Internal implementation failure.
    Internal,
    /// Personal allowance exceeded.
    RateLimit,
    /// Shared capacity or entitlement lookup unavailable.
    Unavailable,
}

impl ProblemClass {
    /// Project a category into a declared typed contract.
    fn problem(self, message: String) -> ApiProblem {
        match self {
            Self::InvalidRequest | Self::UnsupportedRequest => {
                ApiProblem::InvalidRequest { message }
            }
            Self::Authentication => ApiProblem::Authentication { message },
            Self::RequestTooLarge => ApiProblem::RequestTooLarge { message },
            Self::Internal => ApiProblem::Internal,
            Self::RateLimit => ApiProblem::RateLimit { message },
            Self::Unavailable => ApiProblem::Unavailable,
        }
    }
}

// -----------------------------------------------------------------------------
// ApiError: Maps existing diagnostic sources into public contracts.
// -----------------------------------------------------------------------------

/// Application-owned source mapping into the shared typed API contract.
///
/// Engine errors remain transport-neutral; adapters supply their parameter names.
pub trait ApiError: Diagnostic + std::error::Error {
    /// Classify the source for the provider boundary.
    fn class(&self) -> ProblemClass;

    /// Preserve a specific framework status when it differs from its category.
    fn status(&self) -> StatusCode {
        self.class().problem(String::new()).status()
    }

    /// Identify the provider parameter.
    fn param(&self) -> Option<&'static str> {
        None
    }
}

// -----------------------------------------------------------------------------
// NativeError: Projects a report into existing SDK-compatible envelopes.
// -----------------------------------------------------------------------------

/// Native error metadata around the crate's authoritative typed report.
#[derive(Debug)]
pub struct NativeError {
    /// Public contract and retained public diagnostic.
    report: Box<Report<ApiProblem>>,
    /// Stable application diagnostic code, retained for SDK compatibility.
    code: Box<str>,
    /// Provider-specific input field.
    param: Option<&'static str>,
    /// Provider category, independent of its native name.
    class: ProblemClass,
    /// Retry hint, when the failure can be retried.
    retry_after: Option<u64>,
    /// Public detail materialized once for native rendering.
    message: String,
}

impl NativeError {
    /// Explicit hosted encoded input or output size rejection.
    #[must_use]
    pub fn too_large() -> Self {
        Self::admission(
            ProblemClass::RequestTooLarge,
            "Request exceeds hosted size limits.".into(),
            None,
        )
    }

    /// Upload deadline exhaustion retains a native invalid-request envelope with HTTP 408.
    #[must_use]
    pub fn request_timeout() -> Self {
        let message = "Request upload deadline exhausted.".to_owned();
        let mut error = Self::admission(ProblemClass::InvalidRequest, message.clone(), None);
        error.report = Box::new(
            ApiProblem::Extraction {
                status: StatusCode::REQUEST_TIMEOUT.as_u16(),
                message,
            }
            .into_report(),
        );
        error.code = "eliza::admission::request_timeout".into();
        error
    }

    /// Shared overload, without a payment offer or provider diagnostic.
    #[must_use]
    pub fn unavailable() -> Self {
        Self::admission(ProblemClass::Unavailable, String::new(), Some(1))
    }

    /// Invalid credentials, without revealing their value or lookup result.
    #[must_use]
    pub fn unauthorized() -> Self {
        Self::admission(
            ProblemClass::Authentication,
            "authentication failed".into(),
            None,
        )
    }

    /// Map transport-neutral modality diagnostics at their existing API boundary.
    pub fn from_diagnostic(
        error: &impl Diagnostic,
        class: ProblemClass,
        param: Option<&'static str>,
    ) -> Self {
        let code = error.code().map_or_else(
            || "eliza::internal::missing_diagnostic_code".into(),
            |value| value.to_string().into_boxed_str(),
        );
        let report = class
            .problem(if class == ProblemClass::Internal {
                String::new()
            } else {
                error.to_string()
            })
            .into_report();
        let message = report
            .problem()
            .detail()
            .unwrap_or_else(|| report.problem().definition().title.into());
        Self {
            report: Box::new(report),
            code,
            param,
            class,
            retry_after: (class == ProblemClass::Unavailable).then_some(1),
            message,
        }
    }

    /// Construct a hosted policy rejection without exposing credentials or provider causes.
    #[must_use]
    pub fn admission(class: ProblemClass, message: String, retry_after: Option<u64>) -> Self {
        let report = class.problem(message).into_report();
        let message = report
            .problem()
            .detail()
            .unwrap_or_else(|| report.problem().definition().title.into());
        Self {
            report: Box::new(report),
            code: match class {
                ProblemClass::RateLimit => "eliza::admission::rate_limit",
                ProblemClass::Authentication => "eliza::auth::failed",
                ProblemClass::RequestTooLarge => "eliza::admission::request_too_large",
                ProblemClass::InvalidRequest | ProblemClass::UnsupportedRequest => {
                    "eliza::admission::invalid_request"
                }
                _ => "eliza::admission::unavailable",
            }
            .into(),
            param: None,
            class,
            retry_after,
            message,
        }
    }

    /// Native category.
    #[must_use]
    pub const fn class(&self) -> ProblemClass {
        self.class
    }

    /// Status declared by `problems`.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.report.problem().status()
    }

    /// Map an existing application failure to a typed report.
    pub fn from_error(error: &impl ApiError) -> Self {
        let mut projected = Self::from_diagnostic(error, error.class(), error.param());

        // Preserve a more specific framework status than the default category.
        if error.status() != projected.status() {
            projected.report = Box::new(
                ApiProblem::Extraction {
                    status: error.status().as_u16(),
                    message: projected.message.clone(),
                }
                .into_report(),
            );
        }
        projected
    }

    /// Explicit public recovery detail.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Stable diagnostic identity.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Provider parameter.
    #[must_use]
    pub const fn param(&self) -> Option<&'static str> {
        self.param
    }

    /// Attach metadata while leaving report/native body status unchanged.
    pub fn write_error_code(&self, headers: &mut HeaderMap) {
        // Diagnostic metadata is omitted only when it cannot form a header.
        if let Ok(value) = HeaderValue::from_str(self.code()) {
            headers.insert("x-eliza-error-code", value);
        }
        if let Some(seconds) = self.retry_after {
            headers.insert("retry-after", HeaderValue::from(seconds));
        }
        headers.insert("cache-control", HeaderValue::from_static("no-store"));
    }
}

impl IntoResponse for NativeError {
    fn into_response(self) -> Response {
        let mut headers = HeaderMap::new();
        self.write_error_code(&mut headers);
        let mut response = (*self.report).into_response();
        response.headers_mut().extend(headers);
        response
    }
}

// -----------------------------------------------------------------------------
// Tests: Preserve SDK metadata and prevent private cause disclosure.
// -----------------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::missing_panics_doc,
    reason = "test assertions define the panic contract"
)]
#[path = "../tests/unit/problem.rs"]
mod tests;
