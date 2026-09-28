//! Typed failures shared by provider route boundaries.

use axum::extract::rejection::{
    BytesRejection, FailedToDeserializePathParams, FailedToDeserializeQueryString, JsonDataError,
    JsonRejection, JsonSyntaxError, PathRejection, QueryRejection,
};
use axum::http::StatusCode;
use miette::Diagnostic;
use thiserror::Error;

use crate::problem::{ProblemClass, ProblemDetails};

// -----------------------------------------------------------------------------
// AuthenticationError: Rejects invalid provider credentials.
// -----------------------------------------------------------------------------

/// Provider authentication failure.
#[derive(Debug, Diagnostic, Error)]
pub(super) enum AuthenticationError {
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
// ExtractionError: Normalizes Axum request extraction failures.
// -----------------------------------------------------------------------------

/// Request extraction failure normalized across provider surfaces.
#[derive(Debug, Diagnostic, Error)]
pub(super) enum ExtractionError {
    /// JSON syntax was valid but did not match the request contract.
    #[error("invalid JSON body: {source}")]
    #[diagnostic(code(eliza::extract::json_data))]
    InvalidJsonData {
        /// Axum JSON data rejection.
        #[source]
        source: JsonDataError,
    },

    /// Request body was not syntactically valid JSON.
    #[error("invalid JSON syntax: {source}")]
    #[diagnostic(code(eliza::extract::json_syntax))]
    InvalidJsonSyntax {
        /// Axum JSON syntax rejection.
        #[source]
        source: JsonSyntaxError,
    },

    /// JSON request omitted its required content type.
    #[error("expected request with `Content-Type: application/json`")]
    #[diagnostic(code(eliza::extract::missing_json_content_type))]
    MissingJsonContentType,

    /// Axum could not read the complete request body.
    #[error("failed to read request body: {source}")]
    #[diagnostic(code(eliza::extract::body))]
    BodyRead {
        /// Lower-level buffered-body rejection.
        #[source]
        source: BytesRejection,
    },

    /// Axum returned a JSON rejection unknown to this adapter version.
    #[error("unexpected JSON extraction failure: {source}")]
    #[diagnostic(code(eliza::extract::unexpected_json))]
    UnexpectedJson {
        /// Unrecognized Axum JSON rejection.
        #[source]
        source: JsonRejection,
    },

    /// Query parameters did not match the route contract.
    #[error("invalid query parameters: {source}")]
    #[diagnostic(code(eliza::extract::query))]
    InvalidQuery {
        /// Axum query deserialization rejection.
        #[source]
        source: FailedToDeserializeQueryString,
    },

    /// Axum returned a query rejection unknown to this adapter version.
    #[error("unexpected query extraction failure: {message}")]
    #[diagnostic(code(eliza::extract::unexpected_query))]
    UnexpectedQuery {
        /// Axum's description of the unrecognized rejection.
        message: String,
    },

    /// Path parameters did not match the route contract.
    #[error("invalid path parameters: {source}")]
    #[diagnostic(code(eliza::extract::path))]
    InvalidPath {
        /// Axum path deserialization rejection.
        #[source]
        source: FailedToDeserializePathParams,
    },

    /// Axum did not provide path parameters for a matched route.
    #[error("matched route is missing path parameters")]
    #[diagnostic(code(eliza::extract::missing_path_parameters))]
    MissingPathParameters,

    /// Axum returned a path rejection unknown to this adapter version.
    #[error("unexpected path extraction failure: {source}")]
    #[diagnostic(code(eliza::extract::unexpected_path))]
    UnexpectedPath {
        /// Unrecognized Axum path rejection.
        #[source]
        source: PathRejection,
    },
}

impl From<JsonRejection> for ExtractionError {
    fn from(rejection: JsonRejection) -> Self {
        match rejection {
            JsonRejection::JsonDataError(source) => Self::InvalidJsonData { source },
            JsonRejection::JsonSyntaxError(source) => Self::InvalidJsonSyntax { source },
            JsonRejection::MissingJsonContentType(_) => Self::MissingJsonContentType,
            JsonRejection::BytesRejection(source) => Self::BodyRead { source },
            source => Self::UnexpectedJson { source },
        }
    }
}

impl From<QueryRejection> for ExtractionError {
    fn from(rejection: QueryRejection) -> Self {
        match rejection {
            QueryRejection::FailedToDeserializeQueryString(source) => Self::InvalidQuery { source },
            source => Self::UnexpectedQuery {
                message: source.to_string(),
            },
        }
    }
}

impl From<PathRejection> for ExtractionError {
    fn from(rejection: PathRejection) -> Self {
        match rejection {
            PathRejection::FailedToDeserializePathParams(source) => Self::InvalidPath { source },
            PathRejection::MissingPathParams(_) => Self::MissingPathParameters,
            source => Self::UnexpectedPath { source },
        }
    }
}

impl ProblemDetails for ExtractionError {
    fn class(&self) -> ProblemClass {
        match self {
            Self::BodyRead { source } if source.status() == StatusCode::PAYLOAD_TOO_LARGE => {
                ProblemClass::RequestTooLarge
            }
            Self::BodyRead { source } if source.status().is_server_error() => {
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
            Self::BodyRead { source } => source.status(),
            Self::MissingPathParameters
            | Self::UnexpectedJson { .. }
            | Self::UnexpectedQuery { .. }
            | Self::UnexpectedPath { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
