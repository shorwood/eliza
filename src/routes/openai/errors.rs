//! Typed `OpenAI` adapter failures and wire rendering.
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
// OpenAiError: Classifies failures found while lowering requests.
// -----------------------------------------------------------------------------

/// Failure detected while lowering an `OpenAI` request.
#[derive(Debug, Diagnostic, Error)]
pub(super) enum OpenAiError {
    #[error("invalid function arguments: {source}")]
    #[diagnostic(code(eliza::openai::invalid_function_arguments))]
    InvalidFunctionArguments {
        param: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("only function tool calls are supported")]
    #[diagnostic(code(eliza::openai::unsupported_tool_call_kind))]
    UnsupportedToolCallKind { param: &'static str },
    #[error("function tool call is missing function")]
    #[diagnostic(code(eliza::openai::missing_tool_call_function))]
    MissingToolCallFunction { param: &'static str },
    #[error("function is missing its name")]
    #[diagnostic(code(eliza::openai::missing_function_name))]
    MissingFunctionName { param: &'static str },
    #[error("function tool call is missing arguments")]
    #[diagnostic(code(eliza::openai::missing_tool_call_arguments))]
    MissingToolCallArguments { param: &'static str },
    #[error("only client function tools are supported")]
    #[diagnostic(code(eliza::openai::unsupported_tool_definition))]
    UnsupportedToolDefinition,
    #[error("function tool is missing function")]
    #[diagnostic(code(eliza::openai::missing_tool_definition))]
    MissingToolDefinition,
    #[error("unsupported tool_choice mode")]
    #[diagnostic(code(eliza::openai::unsupported_tool_choice_mode))]
    UnsupportedToolChoiceMode,
    #[error("only named function choices are supported")]
    #[diagnostic(code(eliza::openai::unsupported_named_tool_choice))]
    UnsupportedNamedToolChoice,
    #[error("structured output is not supported")]
    #[diagnostic(code(eliza::openai::structured_output_unsupported))]
    StructuredOutputUnsupported { param: &'static str },
    #[error("user message content is required")]
    #[diagnostic(code(eliza::openai::missing_user_content))]
    MissingUserContent,
    #[error("tool result content is required")]
    #[diagnostic(code(eliza::openai::missing_tool_result_content))]
    MissingToolResultContent,
    #[error("unsupported message role")]
    #[diagnostic(code(eliza::openai::unsupported_message_role))]
    UnsupportedMessageRole,
    #[error("content must be a string or text parts")]
    #[diagnostic(code(eliza::openai::unsupported_content_shape))]
    UnsupportedContentShape { param: &'static str },
    #[error("only text content parts are supported")]
    #[diagnostic(code(eliza::openai::unsupported_content_part))]
    UnsupportedContentPart { param: &'static str },
    #[error("unsupported Responses role")]
    #[diagnostic(code(eliza::openai::unsupported_responses_role))]
    UnsupportedResponsesRole,
    #[error("function_call is missing its arguments")]
    #[diagnostic(code(eliza::openai::missing_function_call_arguments))]
    MissingFunctionCallArguments,
    #[error("function_call_output is missing output")]
    #[diagnostic(code(eliza::openai::missing_function_output))]
    MissingFunctionOutput,
    #[error("unsupported Responses input item")]
    #[diagnostic(code(eliza::openai::unsupported_input_item))]
    UnsupportedInputItem,
}

impl ProblemDetails for OpenAiError {
    fn class(&self) -> ProblemClass {
        match self {
            Self::UnsupportedToolCallKind { .. }
            | Self::UnsupportedToolDefinition
            | Self::UnsupportedNamedToolChoice
            | Self::StructuredOutputUnsupported { .. }
            | Self::UnsupportedMessageRole
            | Self::UnsupportedContentShape { .. }
            | Self::UnsupportedContentPart { .. }
            | Self::UnsupportedResponsesRole
            | Self::UnsupportedInputItem => ProblemClass::UnsupportedRequest,
            _ => ProblemClass::InvalidRequest,
        }
    }

    fn param(&self) -> Option<&'static str> {
        match self {
            Self::InvalidFunctionArguments { param, .. }
            | Self::UnsupportedToolCallKind { param }
            | Self::MissingToolCallFunction { param }
            | Self::MissingFunctionName { param }
            | Self::MissingToolCallArguments { param }
            | Self::StructuredOutputUnsupported { param }
            | Self::UnsupportedContentShape { param }
            | Self::UnsupportedContentPart { param } => Some(param),
            Self::UnsupportedToolDefinition | Self::MissingToolDefinition => Some("tools"),
            Self::UnsupportedToolChoiceMode | Self::UnsupportedNamedToolChoice => {
                Some("tool_choice")
            }
            Self::MissingUserContent => Some("messages"),
            Self::MissingToolResultContent => Some("messages.content"),
            Self::UnsupportedMessageRole => Some("messages.role"),
            Self::UnsupportedResponsesRole => Some("input.role"),
            Self::MissingFunctionCallArguments => Some("input.arguments"),
            Self::MissingFunctionOutput => Some("input.output"),
            Self::UnsupportedInputItem => Some("input.type"),
        }
    }
}

// -----------------------------------------------------------------------------
// OpenAiErrorKind: Maps neutral failure classes to wire kinds.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum OpenAiErrorKind {
    #[serde(rename = "invalid_request_error")]
    InvalidRequest,
    #[serde(rename = "authentication_error")]
    Authentication,
    #[serde(rename = "server_error")]
    Server,
}

impl From<ProblemClass> for OpenAiErrorKind {
    fn from(class: ProblemClass) -> Self {
        match class {
            ProblemClass::InvalidRequest
            | ProblemClass::UnsupportedRequest
            | ProblemClass::RequestTooLarge => Self::InvalidRequest,
            ProblemClass::Authentication => Self::Authentication,
            ProblemClass::Internal => Self::Server,
        }
    }
}

// -----------------------------------------------------------------------------
// OpenAiFailureBody: Carries the nested wire error object.
// -----------------------------------------------------------------------------

/// `OpenAI` error object, also referenced by Responses API progress envelopes.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(super) struct OpenAiFailureBody {
    message: String,
    #[serde(rename = "type")]
    kind: OpenAiErrorKind,
    param: Option<&'static str>,
    code: Option<Box<str>>,
}

// -----------------------------------------------------------------------------
// OpenAiFailureResponse: Carries the top-level wire envelope.
// -----------------------------------------------------------------------------

/// `OpenAI` top-level failure envelope.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct OpenAiFailureResponse {
    error: OpenAiFailureBody,
}

// -----------------------------------------------------------------------------
// OpenAiRejection: Renders typed problems in the native wire format.
// -----------------------------------------------------------------------------

/// Provider-native renderer for one typed problem.
pub(super) struct OpenAiRejection(Problem);

impl OpenAiRejection {
    /// Capture a typed diagnostic for `OpenAI` rendering.
    pub(super) fn from_error(error: &impl ProblemDetails) -> Self {
        Self(Problem::from_error(error))
    }
}

impl IntoResponse for OpenAiRejection {
    fn into_response(self) -> Response {
        let problem = self.0;
        let response = (
            problem.status(),
            Json(OpenAiFailureResponse {
                error: OpenAiFailureBody {
                    message: problem.message().to_owned(),
                    kind: problem.class().into(),
                    param: problem.param(),
                    code: Some(problem.code().into()),
                },
            }),
        )
            .into_response();
        problem.finish_response(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify every neutral class maps to the intended `OpenAI` error type.
    ///
    /// # Panics
    ///
    /// Panics when a class regresses to a non-native wire type.
    #[test]
    fn maps_problem_classes_to_native_kinds() {
        assert_eq!(
            OpenAiErrorKind::from(ProblemClass::InvalidRequest),
            OpenAiErrorKind::InvalidRequest
        );
        assert_eq!(
            OpenAiErrorKind::from(ProblemClass::UnsupportedRequest),
            OpenAiErrorKind::InvalidRequest
        );
        assert_eq!(
            OpenAiErrorKind::from(ProblemClass::RequestTooLarge),
            OpenAiErrorKind::InvalidRequest
        );
        assert_eq!(
            OpenAiErrorKind::from(ProblemClass::Authentication),
            OpenAiErrorKind::Authentication
        );
        assert_eq!(
            OpenAiErrorKind::from(ProblemClass::Internal),
            OpenAiErrorKind::Server
        );
    }
}
