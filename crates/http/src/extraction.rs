//! Typed failures shared by provider route boundaries.

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::http::StatusCode;
use miette::Diagnostic;
use thiserror::Error;

use crate::problem::{ProblemClass, ProblemDetails};

// -----------------------------------------------------------------------------
// ExtractionError: Normalizes Axum request extraction failures.
// -----------------------------------------------------------------------------

/// Request extraction failure normalized across provider surfaces.
#[derive(Debug, Diagnostic, Error)]
pub enum ExtractionError {
    /// JSON syntax was valid but did not match the request contract.
    #[error("invalid JSON body: {detail}")]
    #[diagnostic(code(eliza::extract::json_data))]
    InvalidJsonData {
        /// Stable owned JSON data rejection detail.
        detail: String,
    },

    /// Request body was not syntactically valid JSON.
    #[error("invalid JSON syntax: {detail}")]
    #[diagnostic(code(eliza::extract::json_syntax))]
    InvalidJsonSyntax {
        /// Stable owned JSON syntax rejection detail.
        detail: String,
    },

    /// JSON request omitted its required content type.
    #[error("expected request with `Content-Type: application/json`")]
    #[diagnostic(code(eliza::extract::missing_json_content_type))]
    MissingJsonContentType,

    /// Axum could not read the complete request body.
    #[error("failed to read request body: {detail}")]
    #[diagnostic(code(eliza::extract::body))]
    BodyRead {
        /// Stable owned buffered-body rejection detail.
        detail: String,
        /// HTTP status selected by the original body rejection.
        status: u16,
    },

    /// Axum returned a JSON rejection unknown to this adapter version.
    #[error("unexpected JSON extraction failure: {detail}")]
    #[diagnostic(code(eliza::extract::unexpected_json))]
    UnexpectedJson {
        /// Stable owned unrecognized JSON rejection detail.
        detail: String,
    },

    /// Query parameters did not match the route contract.
    #[error("invalid query parameters: {detail}")]
    #[diagnostic(code(eliza::extract::query))]
    InvalidQuery {
        /// Stable owned query deserialization rejection detail.
        detail: String,
    },

    /// Axum returned a query rejection unknown to this adapter version.
    #[error("unexpected query extraction failure: {detail}")]
    #[diagnostic(code(eliza::extract::unexpected_query))]
    UnexpectedQuery {
        /// Stable owned unrecognized query rejection detail.
        detail: String,
    },

    /// Path parameters did not match the route contract.
    #[error("invalid path parameters: {detail}")]
    #[diagnostic(code(eliza::extract::path))]
    InvalidPath {
        /// Stable owned path deserialization rejection detail.
        detail: String,
    },

    /// Axum did not provide path parameters for a matched route.
    #[error("matched route is missing path parameters")]
    #[diagnostic(code(eliza::extract::missing_path_parameters))]
    MissingPathParameters,

    /// Axum returned a path rejection unknown to this adapter version.
    #[error("unexpected path extraction failure: {detail}")]
    #[diagnostic(code(eliza::extract::unexpected_path))]
    UnexpectedPath {
        /// Stable owned unrecognized path rejection detail.
        detail: String,
    },
}

impl From<JsonRejection> for ExtractionError {
    fn from(rejection: JsonRejection) -> Self {
        match rejection {
            JsonRejection::JsonDataError(source) => Self::InvalidJsonData {
                detail: source.to_string(),
            },
            JsonRejection::JsonSyntaxError(source) => Self::InvalidJsonSyntax {
                detail: source.to_string(),
            },
            JsonRejection::MissingJsonContentType(_) => Self::MissingJsonContentType,
            JsonRejection::BytesRejection(source) => Self::BodyRead {
                status: source.status().as_u16(),
                detail: source.to_string(),
            },
            source => Self::UnexpectedJson {
                detail: source.to_string(),
            },
        }
    }
}

impl From<QueryRejection> for ExtractionError {
    fn from(rejection: QueryRejection) -> Self {
        match rejection {
            QueryRejection::FailedToDeserializeQueryString(source) => Self::InvalidQuery {
                detail: source.to_string(),
            },
            source => Self::UnexpectedQuery {
                detail: source.to_string(),
            },
        }
    }
}

impl From<PathRejection> for ExtractionError {
    fn from(rejection: PathRejection) -> Self {
        match rejection {
            PathRejection::FailedToDeserializePathParams(source) => Self::InvalidPath {
                detail: source.to_string(),
            },
            PathRejection::MissingPathParams(_) => Self::MissingPathParameters,
            source => Self::UnexpectedPath {
                detail: source.to_string(),
            },
        }
    }
}

impl ProblemDetails for ExtractionError {
    fn class(&self) -> ProblemClass {
        match self {
            Self::BodyRead { status, .. } if *status == StatusCode::PAYLOAD_TOO_LARGE.as_u16() => {
                ProblemClass::RequestTooLarge
            }
            Self::BodyRead { status, .. }
                if StatusCode::from_u16(*status).is_ok_and(|status| status.is_server_error()) =>
            {
                ProblemClass::Internal
            }
            Self::MissingPathParameters
            | Self::UnexpectedJson { .. }
            | Self::UnexpectedQuery { .. }
            | Self::UnexpectedPath { .. } => ProblemClass::Internal,
            _ => ProblemClass::InvalidRequest,
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::InvalidJsonData { .. }
            | Self::InvalidJsonSyntax { .. }
            | Self::InvalidQuery { .. }
            | Self::InvalidPath { .. } => StatusCode::BAD_REQUEST,
            Self::MissingJsonContentType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::BodyRead { status, .. } => {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
            }
            Self::MissingPathParameters
            | Self::UnexpectedJson { .. }
            | Self::UnexpectedQuery { .. }
            | Self::UnexpectedPath { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
