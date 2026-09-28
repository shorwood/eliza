//! Typed Ollama adapter failures and wire rendering.
#![expect(
    rlib::undocumented_items,
    reason = "private variants and wire fields repeat their diagnostic and Serde contracts"
)]

use axum::Json;
use axum::response::{IntoResponse, Response};
use miette::Diagnostic;
use schemars::JsonSchema;
use serde::Serialize;
use thiserror::Error;

use crate::problem::{Problem, ProblemClass, ProblemDetails};

// -----------------------------------------------------------------------------
// OllamaError: Classifies failures found while lowering requests.
// -----------------------------------------------------------------------------

/// Failure detected while lowering an Ollama request.
#[derive(Debug, Diagnostic, Error)]
pub(super) enum OllamaError {
    #[error("structured {kind} output is not supported")]
    #[diagnostic(code(eliza::ollama::structured_output_unsupported))]
    StructuredOutputUnsupported { kind: &'static str },
    #[error("reasoning output is not supported")]
    #[diagnostic(code(eliza::ollama::reasoning_unsupported))]
    ReasoningUnsupported,
    #[error("unsupported Ollama role")]
    #[diagnostic(code(eliza::ollama::unsupported_role))]
    UnsupportedRole,
    #[error("only function tool calls are supported")]
    #[diagnostic(code(eliza::ollama::unsupported_tool_call_kind))]
    UnsupportedToolCallKind { param: &'static str },
    #[error("tool call is missing function")]
    #[diagnostic(code(eliza::ollama::missing_tool_call_function))]
    MissingToolCallFunction { param: &'static str },
    #[error("tool call is missing name")]
    #[diagnostic(code(eliza::ollama::missing_tool_call_name))]
    MissingToolCallName { param: &'static str },
    #[error("only client function tools are supported")]
    #[diagnostic(code(eliza::ollama::unsupported_tool_definition))]
    UnsupportedToolDefinition,
    #[error("function tool is missing function")]
    #[diagnostic(code(eliza::ollama::missing_tool_definition))]
    MissingToolDefinition,
    #[error("function tool is missing name")]
    #[diagnostic(code(eliza::ollama::missing_function_name))]
    MissingFunctionName,
    #[error("unsupported tool_choice mode")]
    #[diagnostic(code(eliza::ollama::unsupported_tool_choice_mode))]
    UnsupportedToolChoiceMode,
    #[error("named tool_choice is missing name")]
    #[diagnostic(code(eliza::ollama::missing_named_tool_choice))]
    MissingNamedToolChoice,
}

impl ProblemDetails for OllamaError {
    fn class(&self) -> ProblemClass {
        match self {
            Self::StructuredOutputUnsupported { .. }
            | Self::ReasoningUnsupported
            | Self::UnsupportedRole
            | Self::UnsupportedToolCallKind { .. }
            | Self::UnsupportedToolDefinition => ProblemClass::UnsupportedRequest,
            _ => ProblemClass::InvalidRequest,
        }
    }

    fn param(&self) -> Option<&'static str> {
        match self {
            Self::StructuredOutputUnsupported { .. } => Some("format"),
            Self::ReasoningUnsupported => Some("think"),
            Self::UnsupportedRole => Some("messages.role"),
            Self::UnsupportedToolCallKind { param }
            | Self::MissingToolCallFunction { param }
            | Self::MissingToolCallName { param } => Some(param),
            Self::UnsupportedToolDefinition
            | Self::MissingToolDefinition
            | Self::MissingFunctionName => Some("tools"),
            Self::UnsupportedToolChoiceMode | Self::MissingNamedToolChoice => Some("tool_choice"),
        }
    }
}

// -----------------------------------------------------------------------------
// OllamaFailureResponse: Carries the native wire envelope.
// -----------------------------------------------------------------------------

/// Ollama top-level failure envelope.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct OllamaFailureResponse {
    error: String,
}

// -----------------------------------------------------------------------------
// OllamaRejection: Renders typed problems in the native wire format.
// -----------------------------------------------------------------------------

/// Provider-native renderer for one typed problem.
pub(super) struct OllamaRejection(Problem);

impl OllamaRejection {
    /// Capture a typed diagnostic for Ollama rendering.
    pub(super) fn from_error(error: &impl ProblemDetails) -> Self {
        Self(Problem::from_error(error))
    }
}

impl IntoResponse for OllamaRejection {
    fn into_response(self) -> Response {
        let problem = self.0;
        let response = (
            problem.status(),
            Json(OllamaFailureResponse {
                error: problem.message().to_owned(),
            }),
        )
            .into_response();
        problem.finish_response(response)
    }
}
