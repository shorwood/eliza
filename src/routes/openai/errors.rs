//! Typed `OpenAI` adapter failures and wire rendering.
use axum::Json;
use axum::response::{IntoResponse, Response};
use miette::Diagnostic;
use schemars::JsonSchema;
use serde::Serialize;
use thiserror::Error;

use crate::problem::{Problem, ProblemClass, ProblemDetails};
use crate::speech::errors::SpeechError;
use crate::types::errors::EncodingError;

// -----------------------------------------------------------------------------
// OpenAiError: Classifies failures found while lowering requests.
// -----------------------------------------------------------------------------

/// Failure detected while lowering an `OpenAI` request.
#[derive(Debug, Diagnostic, Error)]
pub(super) enum OpenAiError {
    /// Encoded function arguments were not a JSON object.
    #[error("invalid function arguments: {source}")]
    #[diagnostic(code(eliza::openai::invalid_function_arguments))]
    InvalidFunctionArguments {
        /// Request field containing the invalid arguments.
        param: &'static str,
        /// JSON decoding failure.
        #[source]
        source: serde_json::Error,
    },
    /// A tool call selected a non-function kind.
    #[error("only function tool calls are supported")]
    #[diagnostic(code(eliza::openai::unsupported_tool_call_kind))]
    UnsupportedToolCallKind {
        /// Request field containing the unsupported kind.
        param: &'static str,
    },
    /// A tool call omitted its function payload.
    #[error("function tool call is missing function")]
    #[diagnostic(code(eliza::openai::missing_tool_call_function))]
    MissingToolCallFunction {
        /// Request field containing the incomplete call.
        param: &'static str,
    },
    /// A function call or declaration omitted its name.
    #[error("function is missing its name")]
    #[diagnostic(code(eliza::openai::missing_function_name))]
    MissingFunctionName {
        /// Request field containing the missing name.
        param: &'static str,
    },
    /// A function call omitted its arguments.
    #[error("function tool call is missing arguments")]
    #[diagnostic(code(eliza::openai::missing_tool_call_arguments))]
    MissingToolCallArguments {
        /// Request field containing the incomplete call.
        param: &'static str,
    },
    /// A tool declaration selected a non-function kind.
    #[error("only client function tools are supported")]
    #[diagnostic(code(eliza::openai::unsupported_tool_definition))]
    UnsupportedToolDefinition,
    /// A function tool omitted its definition payload.
    #[error("function tool is missing function")]
    #[diagnostic(code(eliza::openai::missing_tool_definition))]
    MissingToolDefinition,
    /// The requested tool-choice mode cannot be represented.
    #[error("unsupported tool_choice mode")]
    #[diagnostic(code(eliza::openai::unsupported_tool_choice_mode))]
    UnsupportedToolChoiceMode,
    /// A named tool choice selected a non-function kind.
    #[error("only named function choices are supported")]
    #[diagnostic(code(eliza::openai::unsupported_named_tool_choice))]
    UnsupportedNamedToolChoice,
    /// The request selected unsupported structured output.
    #[error("structured output is not supported")]
    #[diagnostic(code(eliza::openai::structured_output_unsupported))]
    StructuredOutputUnsupported {
        /// Request field selecting structured output.
        param: &'static str,
    },
    /// A user message omitted its content.
    #[error("user message content is required")]
    #[diagnostic(code(eliza::openai::missing_user_content))]
    MissingUserContent,
    /// A tool-result message omitted its content.
    #[error("tool result content is required")]
    #[diagnostic(code(eliza::openai::missing_tool_result_content))]
    MissingToolResultContent,
    /// A message used a role outside the supported subset.
    #[error("unsupported message role")]
    #[diagnostic(code(eliza::openai::unsupported_message_role))]
    UnsupportedMessageRole,
    /// A content field used an unsupported container shape.
    #[error("content must be a string or text parts")]
    #[diagnostic(code(eliza::openai::unsupported_content_shape))]
    UnsupportedContentShape {
        /// Request field containing the unsupported shape.
        param: &'static str,
    },
    /// A content list contained a non-text part.
    #[error("only text content parts are supported")]
    #[diagnostic(code(eliza::openai::unsupported_content_part))]
    UnsupportedContentPart {
        /// Request field containing the unsupported part.
        param: &'static str,
    },
    /// A Responses input used an unsupported message role.
    #[error("unsupported Responses role")]
    #[diagnostic(code(eliza::openai::unsupported_responses_role))]
    UnsupportedResponsesRole,
    /// A Responses function call omitted its arguments.
    #[error("function_call is missing its arguments")]
    #[diagnostic(code(eliza::openai::missing_function_call_arguments))]
    MissingFunctionCallArguments,
    /// A Responses function result omitted its output.
    #[error("function_call_output is missing output")]
    #[diagnostic(code(eliza::openai::missing_function_output))]
    MissingFunctionOutput,
    /// A Responses input item used an unsupported kind.
    #[error("unsupported Responses input item")]
    #[diagnostic(code(eliza::openai::unsupported_input_item))]
    UnsupportedInputItem,
    /// Speech synthesis used a model other than the local speech model.
    #[error("speech requires model `flite`")]
    #[diagnostic(code(eliza::openai::speech_model_required))]
    SpeechModelRequired,
    /// Speech synthesis received an empty voice name.
    #[error("speech voice must not be empty")]
    #[diagnostic(code(eliza::openai::empty_voice))]
    EmptyVoice,
    /// The requested speech encoding is not implemented.
    #[error("unsupported speech response format")]
    #[diagnostic(code(eliza::openai::unsupported_speech_format))]
    UnsupportedSpeechFormat,
    /// The requested speech stream envelope is not implemented.
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
// OpenAiSpeechStreamError: Preserves failures after response streaming begins.
// -----------------------------------------------------------------------------

/// Failure that can terminate an `OpenAI` speech stream after headers are sent.
#[derive(Debug, Error)]
pub(super) enum OpenAiSpeechStreamError {
    /// A typed server-sent event could not be encoded.
    #[error(transparent)]
    Encoding(
        /// Provider-neutral response encoding failure.
        EncodingError,
    ),
    /// Speech synthesis or audio encoding failed.
    #[error(transparent)]
    Speech(
        /// Provider-neutral speech failure.
        SpeechError,
    ),
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
