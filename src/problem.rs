//! Provider-neutral HTTP problem details.

use aide::OperationOutput;
use aide::generate::GenContext;
use aide::openapi::{MediaType, Operation, Response as ApiResponse, SchemaObject};
use axum::Json;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use miette::Diagnostic;
use schemars::JsonSchema;
use serde::Serialize;

/// Response header carrying the stable diagnostic identifier.
const ERROR_CODE_HEADER: &str = "x-eliza-error-code";

// -----------------------------------------------------------------------------
// ProblemClass: Groups failures by transport semantics.
// -----------------------------------------------------------------------------

/// Provider-neutral failure category.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ProblemClass {
    /// Input does not satisfy the request contract.
    InvalidRequest,
    /// Input requests behavior the server does not implement.
    UnsupportedRequest,
    /// Provider credentials are missing or invalid.
    Authentication,
    /// Input exceeds a configured or transport bound.
    RequestTooLarge,
    /// The server failed while processing a valid request.
    Internal,
}

impl ProblemClass {
    /// Default HTTP status for this failure category.
    const fn status(self) -> StatusCode {
        match self {
            Self::InvalidRequest | Self::UnsupportedRequest => StatusCode::BAD_REQUEST,
            Self::Authentication => StatusCode::UNAUTHORIZED,
            Self::RequestTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Stable RFC 9457 title for this failure category.
    const fn title(self) -> &'static str {
        match self {
            Self::InvalidRequest => "Invalid request",
            Self::UnsupportedRequest => "Unsupported request",
            Self::Authentication => "Authentication failed",
            Self::RequestTooLarge => "Request too large",
            Self::Internal => "Internal server error",
        }
    }
}

// -----------------------------------------------------------------------------
// ProblemDetails: Describes errors at the HTTP boundary.
// -----------------------------------------------------------------------------

/// Typed error that can become provider-neutral problem details.
pub(crate) trait ProblemDetails: Diagnostic + std::error::Error {
    /// Classify the failure for HTTP and provider rendering.
    fn class(&self) -> ProblemClass;

    /// Select the HTTP status returned to the client.
    fn status(&self) -> StatusCode {
        self.class().status()
    }

    /// Identify the invalid provider parameter, when applicable.
    fn param(&self) -> Option<&'static str> {
        None
    }
}

// -----------------------------------------------------------------------------
// Problem: Carries one RFC 9457 response.
// -----------------------------------------------------------------------------

/// RFC 9457 problem details with an Eliza diagnostic code.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct Problem {
    /// Stable URN identifying the concrete diagnostic.
    #[serde(rename = "type")]
    kind: Box<str>,
    /// Short summary shared by failures in the same class.
    title: &'static str,
    /// HTTP status repeated in the response body.
    status: u16,
    /// Safe human-readable explanation, omitted for internal failures.
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    /// Request-specific problem occurrence URI, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    instance: Option<Box<str>>,
    /// Exact Miette diagnostic code.
    code: Box<str>,
    /// Invalid provider parameter, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    param: Option<&'static str>,
    /// Provider-neutral class used only while rendering a wire response.
    #[serde(skip)]
    #[schemars(skip)]
    class: ProblemClass,
}

impl Problem {
    /// Capture public problem facts from a typed diagnostic.
    pub(crate) fn from_error(error: &impl ProblemDetails) -> Self {
        // Classify public transport facts before rendering provider shapes.
        let class = error.class();
        let status = error.status();
        let param = error.param();

        // Keep the exact diagnostic identity while hiding internal details.
        let code = error.code().map_or_else(
            || "eliza::internal::missing_diagnostic_code".to_owned(),
            |code| code.to_string(),
        );
        let detail = if class == ProblemClass::Internal {
            tracing::error!(error = ?error, diagnostic_code = %code, "request failed");
            None
        } else {
            Some(error.to_string())
        };

        // Expose a stable problem type derived from the diagnostic namespace.
        let kind = format!(
            "urn:eliza:problem:{}",
            code.strip_prefix("eliza::")
                .unwrap_or(&code)
                .replace("::", ":")
        );

        Self {
            kind: kind.into_boxed_str(),
            title: class.title(),
            status: status.as_u16(),
            detail,
            instance: None,
            code: code.into_boxed_str(),
            param,
            class,
        }
    }

    /// Return the provider-neutral failure category.
    pub(crate) const fn class(&self) -> ProblemClass {
        self.class
    }

    /// Return the HTTP status selected for this problem.
    pub(crate) fn status(&self) -> StatusCode {
        StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    /// Return the safe provider-facing message.
    pub(crate) fn message(&self) -> &str {
        self.detail.as_deref().unwrap_or(self.title)
    }

    /// Return the exact Miette diagnostic code.
    pub(crate) fn code(&self) -> &str {
        &self.code
    }

    /// Return the invalid provider parameter, when applicable.
    pub(crate) const fn param(&self) -> Option<&'static str> {
        self.param
    }

    /// Attach the diagnostic identifier to response headers.
    pub(crate) fn write_error_code(&self, headers: &mut HeaderMap) {
        // An invalid diagnostic code cannot safely become an HTTP header.
        let Ok(value) = HeaderValue::from_str(self.code()) else {
            return;
        };
        headers.insert(ERROR_CODE_HEADER, value);
    }
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let status = self.status();
        let code = self.code.clone();
        let mut response = (status, Json(self)).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        if let Ok(code) = HeaderValue::from_str(&code) {
            response.headers_mut().insert(ERROR_CODE_HEADER, code);
        }
        response
    }
}

impl OperationOutput for Problem {
    type Inner = Self;

    fn operation_response(ctx: &mut GenContext, _operation: &mut Operation) -> Option<ApiResponse> {
        let mut response = ApiResponse {
            description: "RFC 9457 problem details".to_owned(),
            ..ApiResponse::default()
        };
        response.content.insert(
            "application/problem+json".to_owned(),
            MediaType {
                schema: Some(SchemaObject {
                    json_schema: ctx.schema.subschema_for::<Self>(),
                    external_docs: None,
                    example: None,
                }),
                ..MediaType::default()
            },
        );
        Some(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::speech::errors::SpeechError;
    use crate::types::errors::ModelError;

    /// Verify the serialized RFC 9457 and diagnostic fields.
    ///
    /// # Panics
    ///
    /// Panics when serialization fails or a field differs from its contract.
    #[test]
    fn problem_exposes_the_diagnostic_contract() {
        let problem = Problem::from_error(&ModelError::Empty);
        let value = serde_json::to_value(&problem).unwrap();

        assert_eq!(value["type"], "urn:eliza:problem:model:empty");
        assert_eq!(value["status"], 400);
        assert_eq!(value["detail"], "model id must not be empty");
        assert_eq!(value["code"], "eliza::model::empty");
        assert_eq!(value["param"], "model");
    }

    /// Verify the response status, content type, and diagnostic header.
    ///
    /// # Panics
    ///
    /// Panics when an expected response header is absent or differs.
    #[test]
    fn problem_response_uses_the_problem_media_type() {
        let response = Problem::from_error(&ModelError::Empty).into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/problem+json"
        );
        assert_eq!(
            response.headers().get(ERROR_CODE_HEADER).unwrap(),
            "eliza::model::empty"
        );
    }

    /// Verify that internal diagnostics keep implementation details private.
    ///
    /// # Panics
    ///
    /// Panics when the internal detail is exposed through the public message.
    #[test]
    fn internal_problem_redacts_its_detail() {
        let problem = Problem::from_error(&SpeechError::Unavailable);

        assert!(problem.detail.is_none());
        assert_eq!(problem.message(), "Internal server error");
    }
}
