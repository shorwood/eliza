//! Typed `OpenAI` adapter failures and wire rendering.
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
#[expect(
    rlib::undocumented_items,
    reason = "thiserror messages and Miette codes are the canonical contracts for private adapter failures"
)]
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
    #[error("speech requires model `eliza-retro-tts`")]
    #[diagnostic(code(eliza::openai::speech_model_required))]
    SpeechModelRequired,
    #[error("speech voice must not be empty")]
    #[diagnostic(code(eliza::openai::empty_voice))]
    EmptyVoice,
    #[error("unsupported speech response format")]
    #[diagnostic(code(eliza::openai::unsupported_speech_format))]
    UnsupportedSpeechFormat,
    #[error("unsupported speech stream format")]
    #[diagnostic(code(eliza::openai::unsupported_speech_stream_format))]
    UnsupportedSpeechStreamFormat,
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
            | Self::UnsupportedInputItem
            | Self::UnsupportedSpeechFormat
            | Self::UnsupportedSpeechStreamFormat => ProblemClass::UnsupportedRequest,
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
            Self::SpeechModelRequired => Some("model"),
            Self::EmptyVoice => Some("voice"),
            Self::UnsupportedSpeechFormat => Some("response_format"),
            Self::UnsupportedSpeechStreamFormat => Some("stream_format"),
        }
    }
}

// -----------------------------------------------------------------------------
// OpenAiErrorKind: Maps neutral failure classes to wire kinds.
// -----------------------------------------------------------------------------

/// `OpenAI`'s serialized error classification.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum OpenAiErrorKind {
    /// Invalid, unsupported, or oversized request input.
    #[serde(rename = "invalid_request_error")]
    InvalidRequest,
    /// Missing or invalid authentication.
    #[serde(rename = "authentication_error")]
    Authentication,
    /// Internal server failure.
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
    /// Human-readable failure detail.
    message: String,
    /// Provider error classification.
    #[serde(rename = "type")]
    kind: OpenAiErrorKind,
    /// Request field associated with the failure, when known.
    param: Option<&'static str>,
    /// Stable diagnostic code.
    code: Option<Box<str>>,
}

// -----------------------------------------------------------------------------
// OpenAiFailureResponse: Carries the top-level wire envelope.
// -----------------------------------------------------------------------------

/// `OpenAI` top-level failure envelope.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct OpenAiFailureResponse {
    /// Provider-native error payload.
    error: OpenAiFailureBody,
}

// -----------------------------------------------------------------------------
// OpenAiRejection: Renders typed problems in the native wire format.
// -----------------------------------------------------------------------------

/// Provider-native renderer for one typed problem.
pub(super) struct OpenAiRejection(
    /// Neutral problem awaiting `OpenAI` wire rendering.
    Problem,
);

impl OpenAiRejection {
    /// Capture a typed diagnostic for `OpenAI` rendering.
    pub(super) fn from_error(error: &impl ProblemDetails) -> Self {
        Self(Problem::from_error(error))
    }
}

impl IntoResponse for OpenAiRejection {
    fn into_response(self) -> Response {
        let problem = self.0;
        let mut response = (
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
        problem.write_error_code(response.headers_mut());
        response
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
