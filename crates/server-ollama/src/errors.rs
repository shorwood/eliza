//! Typed Ollama adapter failures and wire rendering.
use axum::Json;
use axum::response::{IntoResponse, Response};
use eliza_http::problem::{ApiError, NativeError, ProblemClass};
use eliza_modality_chat as chat;
use eliza_modality_embedding as embedding;
use eliza_modality_image as image;
use miette::Diagnostic;
use schemars::JsonSchema;
use serde::Serialize;
use thiserror::Error;

// -----------------------------------------------------------------------------
// OllamaError: Classifies failures found while lowering requests.
// -----------------------------------------------------------------------------

/// Failure detected while lowering an Ollama request.
#[derive(Debug, Diagnostic, Error)]
pub(super) enum OllamaError {
    /// Shared image source validation failed during provider lowering.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Image(
        /// Provider-neutral image failure.
        #[from]
        image::errors::Error,
    ),

    /// An embedding request selected a model other than the fixed fixture.
    #[error("embeddings require model `fnv-embed`")]
    #[diagnostic(code(eliza::embedding::model_required))]
    EmbeddingModelRequired,
    /// An embedding request used an input shape outside the text subset.
    #[error("embedding input must be a string or an array of strings")]
    #[diagnostic(code(eliza::ollama::unsupported_embedding_input))]
    UnsupportedEmbeddingInput,
    /// The requested schema is malformed or unsatisfiable.
    #[error("invalid response schema: {source}")]
    #[diagnostic(code(eliza::ollama::invalid_response_schema))]
    InvalidResponseSchema {
        /// Shared compiler failure with an RFC 6901 path.
        #[source]
        source: chat::structured_output::StructuredOutputError,
    },
    /// The request selected an unknown structured-output format.
    #[error("unsupported response format")]
    #[diagnostic(code(eliza::ollama::unsupported_response_format))]
    UnsupportedResponseFormat,
    /// The requested schema uses unsupported JSON Schema behavior.
    #[error("unsupported response schema: {source}")]
    #[diagnostic(code(eliza::ollama::unsupported_response_schema))]
    UnsupportedResponseSchema {
        /// Shared compiler failure with an RFC 6901 path.
        #[source]
        source: chat::structured_output::StructuredOutputError,
    },
    /// The request enabled unsupported reasoning output.
    #[error("reasoning output is not supported")]
    #[diagnostic(code(eliza::ollama::reasoning_unsupported))]
    ReasoningUnsupported,
    /// A message used a role outside Ollama's supported subset.
    #[error("unsupported Ollama role")]
    #[diagnostic(code(eliza::ollama::unsupported_role))]
    UnsupportedRole,
    /// An image list was attached to a non-user message.
    #[error("images are supported only on user messages")]
    #[diagnostic(code(eliza::ollama::images_require_user_role))]
    ImagesRequireUserRole,
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

impl ApiError for OllamaError {
    fn class(&self) -> ProblemClass {
        match self {
            Self::Image(source) => match source.kind() {
                image::errors::ErrorKind::InvalidInput => ProblemClass::InvalidRequest,
                image::errors::ErrorKind::Limit => ProblemClass::RequestTooLarge,
            },
            Self::UnsupportedEmbeddingInput
            | Self::UnsupportedResponseFormat
            | Self::UnsupportedResponseSchema { .. }
            | Self::ReasoningUnsupported
            | Self::UnsupportedRole
            | Self::UnsupportedToolCallKind { .. }
            | Self::UnsupportedToolDefinition => ProblemClass::UnsupportedRequest,
            _ => ProblemClass::InvalidRequest,
        }
    }

    fn param(&self) -> Option<&'static str> {
        match self {
            Self::Image(_) | Self::ImagesRequireUserRole => Some("messages.images"),
            Self::EmbeddingModelRequired => Some("model"),
            Self::UnsupportedEmbeddingInput => Some("input"),
            Self::InvalidResponseSchema { .. }
            | Self::UnsupportedResponseFormat
            | Self::UnsupportedResponseSchema { .. } => Some("format"),
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
#[derive(derive_more::From)]
pub struct OllamaRejection(
    /// Neutral problem awaiting Ollama wire rendering.
    NativeError,
);

impl OllamaRejection {
    /// Capture a typed diagnostic for Ollama rendering.
    pub(super) fn from_error(error: &impl ApiError) -> Self {
        Self(NativeError::from_error(error))
    }
}

impl From<&chat::errors::Error> for OllamaRejection {
    fn from(error: &chat::errors::Error) -> Self {
        let class = match error.kind() {
            chat::errors::ErrorKind::InvalidInput => ProblemClass::InvalidRequest,
            chat::errors::ErrorKind::Limit => ProblemClass::RequestTooLarge,
            chat::errors::ErrorKind::Internal => ProblemClass::Internal,
        };
        let param = match error.field() {
            Some(chat::errors::ErrorField::Input) => Some("messages"),
            Some(chat::errors::ErrorField::Tools) => Some("tools"),
            Some(chat::errors::ErrorField::ToolChoice) => Some("tool_choice"),
            None => None,
        };
        Self(NativeError::from_diagnostic(error, class, param))
    }
}

impl From<&embedding::engine::Error> for OllamaRejection {
    fn from(error: &embedding::engine::Error) -> Self {
        let class = match error.kind() {
            embedding::engine::ErrorKind::InvalidInput => ProblemClass::InvalidRequest,
            embedding::engine::ErrorKind::Limit => ProblemClass::RequestTooLarge,
        };
        let param = match error.field() {
            embedding::engine::ErrorField::Input => Some("input"),
            embedding::engine::ErrorField::Dimensions => Some("dimensions"),
        };
        Self(NativeError::from_diagnostic(error, class, param))
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
