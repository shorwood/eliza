//! Typed failures raised by provider-neutral request handling.

use miette::Diagnostic;
use thiserror::Error;

use crate::problem::{ProblemClass, ProblemDetails};

// -----------------------------------------------------------------------------
// ModelError: Rejects invalid provider-visible model identifiers.
// -----------------------------------------------------------------------------

/// Model identifier validation failure.
#[derive(Debug, Diagnostic, Error)]
pub enum ModelError {
    /// Model identifiers cannot be empty or whitespace-only.
    #[error("model id must not be empty")]
    #[diagnostic(code(eliza::model::empty))]
    Empty,
}

impl ProblemDetails for ModelError {
    fn class(&self) -> ProblemClass {
        ProblemClass::InvalidRequest
    }

    fn param(&self) -> Option<&'static str> {
        Some("model")
    }
}

// -----------------------------------------------------------------------------
// EncodingError: Reports response serialization failures.
// -----------------------------------------------------------------------------

/// Provider response serialization failure.
#[derive(Debug, Diagnostic, Error)]
pub enum EncodingError {
    /// A typed server-sent event could not be serialized.
    #[error("failed to encode event: {detail}")]
    #[diagnostic(code(eliza::encoding::sse))]
    Sse {
        /// Stable owned event serialization failure detail.
        detail: String,
    },

    /// A typed newline-delimited JSON record could not be serialized.
    #[error("failed to encode record: {detail}")]
    #[diagnostic(code(eliza::encoding::ndjson))]
    Ndjson {
        /// Stable owned record serialization failure detail.
        detail: String,
    },
}

impl ProblemDetails for EncodingError {
    fn class(&self) -> ProblemClass {
        ProblemClass::Internal
    }
}
