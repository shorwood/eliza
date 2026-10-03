//! Typed `OpenAI` adapter failures and wire rendering.
use axum::Json;
use axum::response::{IntoResponse, Response};
use eliza_http::errors::EncodingError;
use eliza_http::problem::{Problem, ProblemClass, ProblemDetails};
use eliza_modality_chat as chat;
use eliza_modality_embedding as embedding;
use eliza_modality_image as image;
use eliza_modality_speech as speech;
use miette::Diagnostic;
use schemars::JsonSchema;
use serde::Serialize;
use thiserror::Error;

// -----------------------------------------------------------------------------
// OpenAiError: Classifies failures found while lowering requests.
// -----------------------------------------------------------------------------

/// Failure detected while lowering an `OpenAI` request.
#[derive(Debug, Diagnostic, Error)]
pub(super) enum OpenAiError {
    /// Shared image source validation failed during provider lowering.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Image(
        /// Provider-neutral image failure.
        image::errors::Error,
    ),

    /// The requested reasoning effort was outside the current contract.
    #[error("unsupported reasoning effort")]
    #[diagnostic(code(eliza::openai::unsupported_reasoning_effort))]
    UnsupportedReasoningEffort {
        /// Request field containing the unsupported effort.
        param: &'static str,
    },
    /// The requested reasoning summary mode was outside the current contract.
    #[error("unsupported reasoning summary")]
    #[diagnostic(code(eliza::openai::unsupported_reasoning_summary))]
    UnsupportedReasoningSummary {
        /// Request field containing the unsupported summary mode.
        param: &'static str,
    },
    /// The requested output-token limit was zero.
    #[error("max_output_tokens must be positive")]
    #[diagnostic(code(eliza::openai::invalid_max_output_tokens))]
    InvalidMaxOutputTokens,

    /// Image generation selected a model other than the fixed fixture.
    #[error("image generation requires model `eliza-retro-image`")]
    #[diagnostic(code(eliza::openai::image_generation_model_required))]
    ImageGenerationModelRequired,
    /// Image generation selected an output representation outside PNG base64.
    #[error("unsupported image output format")]
    #[diagnostic(code(eliza::openai::unsupported_image_output_format))]
    UnsupportedImageOutputFormat {
        /// Request field selecting unsupported output.
        param: &'static str,
    },
    /// Image generation selected dimensions outside the bounded fixture.
    #[error("unsupported image size")]
    #[diagnostic(code(eliza::openai::unsupported_image_size))]
    UnsupportedImageSize,
    /// A known image-generation control cannot be honored locally.
    #[error("unsupported image generation control `{param}`")]
    #[diagnostic(code(eliza::openai::unsupported_image_control))]
    UnsupportedImageControl {
        /// Request field selecting unsupported behavior.
        param: &'static str,
    },

    /// An embedding request selected a model other than the fixed fixture.
    #[error("embeddings require model `fnv-embed`")]
    #[diagnostic(code(eliza::embedding::model_required))]
    EmbeddingModelRequired,
    /// An embedding request selected an unsupported output encoding.
    #[error("embedding encoding_format must be `float`")]
    #[diagnostic(code(eliza::openai::unsupported_embedding_encoding))]
    UnsupportedEmbeddingEncoding,
    /// An embedding request supplied token identifiers instead of text.
    #[error("embedding token-id input is not supported")]
    #[diagnostic(code(eliza::openai::embedding_token_input_unsupported))]
    EmbeddingTokenInputUnsupported,
    /// An embedding request used an input shape outside the text subset.
    #[error("embedding input must be a string or an array of strings")]
    #[diagnostic(code(eliza::openai::unsupported_embedding_input))]
    UnsupportedEmbeddingInput,
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
    /// A structured-output control omitted required provider metadata.
    #[error("invalid response format: {reason}")]
    #[diagnostic(code(eliza::openai::invalid_response_format))]
    InvalidResponseFormat {
        /// Request field containing the invalid format.
        param: &'static str,
        /// Provider-contract requirement that was not satisfied.
        reason: &'static str,
    },
    /// The request selected an unknown text-output format.
    #[error("unsupported response format")]
    #[diagnostic(code(eliza::openai::unsupported_response_format))]
    UnsupportedResponseFormat {
        /// Request field selecting the unsupported format.
        param: &'static str,
    },
    /// The requested schema is malformed or unsatisfiable.
    #[error("invalid response schema: {source}")]
    #[diagnostic(code(eliza::openai::invalid_response_schema))]
    InvalidResponseSchema {
        /// Request field containing the schema.
        param: &'static str,
        /// Shared compiler failure with an RFC 6901 path.
        #[source]
        source: chat::structured_output::StructuredOutputError,
    },
    /// The requested schema uses unsupported JSON Schema behavior.
    #[error("unsupported response schema: {source}")]
    #[diagnostic(code(eliza::openai::unsupported_response_schema))]
    UnsupportedResponseSchema {
        /// Request field containing the schema.
        param: &'static str,
        /// Shared compiler failure with an RFC 6901 path.
        #[source]
        source: chat::structured_output::StructuredOutputError,
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
            Self::Image(source) => match source.kind() {
                image::errors::ErrorKind::InvalidInput => ProblemClass::InvalidRequest,
                image::errors::ErrorKind::Limit => ProblemClass::RequestTooLarge,
            },
            Self::UnsupportedEmbeddingEncoding
            | Self::UnsupportedReasoningEffort { .. }
            | Self::UnsupportedReasoningSummary { .. }
            | Self::UnsupportedImageOutputFormat { .. }
            | Self::UnsupportedImageSize
            | Self::UnsupportedImageControl { .. }
            | Self::EmbeddingTokenInputUnsupported
            | Self::UnsupportedEmbeddingInput
            | Self::UnsupportedToolCallKind { .. }
            | Self::UnsupportedToolDefinition
            | Self::UnsupportedNamedToolChoice
            | Self::UnsupportedResponseFormat { .. }
            | Self::UnsupportedResponseSchema { .. }
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
            Self::InvalidMaxOutputTokens => Some("max_output_tokens"),
            Self::EmbeddingModelRequired
            | Self::ImageGenerationModelRequired
            | Self::SpeechModelRequired => Some("model"),
            Self::UnsupportedEmbeddingEncoding => Some("encoding_format"),
            Self::Image(_)
            | Self::EmbeddingTokenInputUnsupported
            | Self::UnsupportedEmbeddingInput => Some("input"),
            Self::UnsupportedReasoningEffort { param }
            | Self::UnsupportedReasoningSummary { param }
            | Self::InvalidFunctionArguments { param, .. }
            | Self::UnsupportedToolCallKind { param }
            | Self::MissingToolCallFunction { param }
            | Self::MissingFunctionName { param }
            | Self::MissingToolCallArguments { param }
            | Self::InvalidResponseFormat { param, .. }
            | Self::UnsupportedResponseFormat { param }
            | Self::InvalidResponseSchema { param, .. }
            | Self::UnsupportedResponseSchema { param, .. }
            | Self::UnsupportedImageOutputFormat { param }
            | Self::UnsupportedImageControl { param }
            | Self::UnsupportedContentShape { param }
            | Self::UnsupportedContentPart { param } => Some(param),
            Self::UnsupportedImageSize => Some("size"),
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
        speech::errors::Error,
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
    /// Capture a chat diagnostic with the calling endpoint's input field.
    pub(super) fn chat(error: &chat::errors::Error, input: &'static str) -> Self {
        let class = match error.kind() {
            chat::errors::ErrorKind::InvalidInput => ProblemClass::InvalidRequest,
            chat::errors::ErrorKind::Limit => ProblemClass::RequestTooLarge,
            chat::errors::ErrorKind::Internal => ProblemClass::Internal,
        };
        let param = match error.field() {
            Some(chat::errors::ErrorField::Input) => Some(input),
            Some(chat::errors::ErrorField::Tools) => Some("tools"),
            Some(chat::errors::ErrorField::ToolChoice) => Some("tool_choice"),
            None => None,
        };
        Self(Problem::from_diagnostic(error, class, param))
    }

    /// Capture provider-lowering failures with an endpoint-specific input field.
    pub(super) fn request(error: &OpenAiError, input: &'static str) -> Self {
        // Non-image lowering errors already carry their provider field.
        let OpenAiError::Image(source) = error else {
            return Self::from_error(error);
        };
        let class = match source.kind() {
            image::errors::ErrorKind::InvalidInput => ProblemClass::InvalidRequest,
            image::errors::ErrorKind::Limit => ProblemClass::RequestTooLarge,
        };
        Self(Problem::from_diagnostic(source, class, Some(input)))
    }

    /// Capture a typed diagnostic for `OpenAI` rendering.
    pub(super) fn from_error(error: &impl ProblemDetails) -> Self {
        Self(Problem::from_error(error))
    }
}

impl From<&embedding::engine::Error> for OpenAiRejection {
    fn from(error: &embedding::engine::Error) -> Self {
        let class = match error.kind() {
            embedding::engine::ErrorKind::InvalidInput => ProblemClass::InvalidRequest,
            embedding::engine::ErrorKind::Limit => ProblemClass::RequestTooLarge,
        };
        let param = match error.field() {
            embedding::engine::ErrorField::Input => Some("input"),
            embedding::engine::ErrorField::Dimensions => Some("dimensions"),
        };
        Self(Problem::from_diagnostic(error, class, param))
    }
}

impl From<&image::generation::Error> for OpenAiRejection {
    fn from(error: &image::generation::Error) -> Self {
        let class = match error.kind() {
            image::generation::ErrorKind::InvalidInput => ProblemClass::InvalidRequest,
            image::generation::ErrorKind::Limit => ProblemClass::RequestTooLarge,
            image::generation::ErrorKind::Internal => ProblemClass::Internal,
        };
        let param = match error.field() {
            image::generation::ErrorField::Prompt => Some("prompt"),
            image::generation::ErrorField::Count => Some("n"),
            image::generation::ErrorField::None => None,
        };
        Self(Problem::from_diagnostic(error, class, param))
    }
}

impl From<&speech::errors::Error> for OpenAiRejection {
    fn from(error: &speech::errors::Error) -> Self {
        let class = match error.kind() {
            speech::errors::ErrorKind::InvalidInput => ProblemClass::InvalidRequest,
            speech::errors::ErrorKind::Limit => ProblemClass::RequestTooLarge,
            speech::errors::ErrorKind::Internal => ProblemClass::Internal,
        };
        let param = match error.field() {
            Some(speech::errors::ErrorField::Input) => Some("input"),
            Some(speech::errors::ErrorField::SampleRate) => Some("response_format"),
            Some(speech::errors::ErrorField::Speed) => Some("speed"),
            None => None,
        };
        Self(Problem::from_diagnostic(error, class, param))
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
