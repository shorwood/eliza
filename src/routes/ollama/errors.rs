//! Typed Ollama adapter failures and wire rendering.
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
    /// An embedding request used an input shape outside the text subset.
    #[error("embedding input must be a string or an array of strings")]
    #[diagnostic(code(eliza::ollama::unsupported_embedding_input))]
    UnsupportedEmbeddingInput,
    /// The request selected an unsupported structured-output shape.
    #[error("structured {kind} output is not supported")]
    #[diagnostic(code(eliza::ollama::structured_output_unsupported))]
    StructuredOutputUnsupported {
        /// Human-readable structured-output kind.
        kind: &'static str,
    },
    /// The request enabled unsupported reasoning output.
    #[error("reasoning output is not supported")]
    #[diagnostic(code(eliza::ollama::reasoning_unsupported))]
    ReasoningUnsupported,
    /// A message used a role outside Ollama's supported subset.
    #[error("unsupported Ollama role")]
    #[diagnostic(code(eliza::ollama::unsupported_role))]
    UnsupportedRole,
    /// A tool call selected a non-function kind.
    #[error("only function tool calls are supported")]
    #[diagnostic(code(eliza::ollama::unsupported_tool_call_kind))]
    UnsupportedToolCallKind {
        /// Request field containing the unsupported kind.
        param: &'static str,
    },
    /// A tool call omitted its function payload.
    #[error("tool call is missing function")]
    #[diagnostic(code(eliza::ollama::missing_tool_call_function))]
    MissingToolCallFunction {
        /// Request field containing the incomplete call.
        param: &'static str,
    },
    /// A tool call omitted its function name.
    #[error("tool call is missing name")]
    #[diagnostic(code(eliza::ollama::missing_tool_call_name))]
    MissingToolCallName {
        /// Request field containing the incomplete call.
        param: &'static str,
    },
    /// A tool declaration selected a non-function kind.
    #[error("only client function tools are supported")]
    #[diagnostic(code(eliza::ollama::unsupported_tool_definition))]
    UnsupportedToolDefinition,
    /// A function tool omitted its definition payload.
    #[error("function tool is missing function")]
    #[diagnostic(code(eliza::ollama::missing_tool_definition))]
    MissingToolDefinition,
    /// A function tool omitted its name.
    #[error("function tool is missing name")]
    #[diagnostic(code(eliza::ollama::missing_function_name))]
    MissingFunctionName,
    /// The requested tool-choice mode cannot be represented.
    #[error("unsupported tool_choice mode")]
    #[diagnostic(code(eliza::ollama::unsupported_tool_choice_mode))]
    UnsupportedToolChoiceMode,
    /// A named tool choice omitted its selected function name.
    #[error("named tool_choice is missing name")]
    #[diagnostic(code(eliza::ollama::missing_named_tool_choice))]
    MissingNamedToolChoice,
}

impl ProblemDetails for OllamaError {
    fn class(&self) -> ProblemClass {
        match self {
            Self::UnsupportedEmbeddingInput
            | Self::StructuredOutputUnsupported { .. }
            | Self::ReasoningUnsupported
            | Self::UnsupportedRole
            | Self::UnsupportedToolCallKind { .. }
            | Self::UnsupportedToolDefinition => ProblemClass::UnsupportedRequest,
            _ => ProblemClass::InvalidRequest,
        }
    }

    fn param(&self) -> Option<&'static str> {
        match self {
            Self::UnsupportedEmbeddingInput => Some("input"),
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
    /// Human-readable failure detail.
    error: String,
}

// -----------------------------------------------------------------------------
// OllamaRejection: Renders typed problems in the native wire format.
// -----------------------------------------------------------------------------

/// Provider-native renderer for one typed problem.
pub(super) struct OllamaRejection(
    /// Neutral problem awaiting Ollama wire rendering.
    Problem,
);

impl OllamaRejection {
    /// Capture a typed diagnostic for Ollama rendering.
    pub(super) fn from_error(error: &impl ProblemDetails) -> Self {
        Self(Problem::from_error(error))
    }
}

impl IntoResponse for OllamaRejection {
    fn into_response(self) -> Response {
        let problem = self.0;
        let mut response = (
            problem.status(),
            Json(OllamaFailureResponse {
                error: problem.message().to_owned(),
            }),
        )
            .into_response();
        problem.write_error_code(response.headers_mut());
        response
    }
}
