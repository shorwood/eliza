//! Anthropic adapter failures and wire rendering.
use axum::Json;
use axum::response::{IntoResponse, Response};
use eliza_http::problem::{ApiError, NativeError, ProblemClass};
use eliza_modality_chat as chat;
use eliza_modality_image as image;
use miette::Diagnostic;
use schemars::JsonSchema;
use serde::Serialize;
use thiserror::Error;

// -----------------------------------------------------------------------------
// AnthropicError: Classifies failures found while lowering requests.
// -----------------------------------------------------------------------------

/// Failure detected while lowering an Anthropic request.
#[derive(Debug, Diagnostic, Error)]
pub(super) enum AnthropicError {
    /// Shared image source validation failed during provider lowering.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Image(
        /// Provider-neutral image failure.
        image::errors::Error,
    ),

    /// Chat selected a name outside the configured local aliases.
    #[error("chat requires model {expected}")]
    #[diagnostic(code(eliza::chat::model_required))]
    ChatModelRequired {
        /// Configured names accepted by the Messages route.
        expected: String,
    },

    /// The requested output-token limit was zero.
    #[error("max_tokens must be positive")]
    #[diagnostic(code(eliza::anthropic::invalid_max_tokens))]
    InvalidMaxTokens,
    /// Explicit thinking used less than Anthropic's minimum budget.
    #[error("thinking budget_tokens must be at least 1024")]
    #[diagnostic(code(eliza::anthropic::invalid_thinking_budget))]
    InvalidThinkingBudget,
    /// Explicit thinking consumed the entire output-token allowance.
    #[error("thinking budget_tokens must be less than max_tokens")]
    #[diagnostic(code(eliza::anthropic::thinking_budget_too_large))]
    ThinkingBudgetTooLarge,
    /// The requested thinking mode was outside the current contract.
    #[error("unsupported thinking type")]
    #[diagnostic(code(eliza::anthropic::unsupported_thinking_mode))]
    UnsupportedThinkingMode,
    /// The requested thinking visibility was outside the current contract.
    #[error("unsupported thinking display")]
    #[diagnostic(code(eliza::anthropic::unsupported_thinking_display))]
    UnsupportedThinkingDisplay,
    /// The requested effort was outside the current contract.
    #[error("unsupported effort")]
    #[diagnostic(code(eliza::anthropic::unsupported_effort))]
    UnsupportedEffort,

    /// A structured-output control omitted its schema.
    #[error("invalid output format: schema is required")]
    #[diagnostic(code(eliza::anthropic::invalid_output_format))]
    InvalidOutputFormat,
    /// The requested schema is malformed or unsatisfiable.
    #[error("invalid output schema: {source}")]
    #[diagnostic(code(eliza::anthropic::invalid_output_schema))]
    InvalidOutputSchema {
        /// Shared compiler failure with an RFC 6901 path.
        #[source]
        source: chat::structured_output::StructuredOutputError,
    },
    /// A tool declaration omitted its name.
    #[error("tool is missing its name")]
    #[diagnostic(code(eliza::anthropic::missing_tool_name))]
    MissingToolName,
    /// A function tool omitted its input schema.
    #[error("function tool is missing input_schema")]
    #[diagnostic(code(eliza::anthropic::missing_tool_input_schema))]
    MissingToolInputSchema,
    /// A named tool choice omitted the selected tool name.
    #[error("named tool_choice is missing name")]
    #[diagnostic(code(eliza::anthropic::missing_named_tool_choice))]
    MissingNamedToolChoice,
    /// The requested tool-choice mode cannot be represented.
    #[error("unsupported tool_choice type")]
    #[diagnostic(code(eliza::anthropic::unsupported_tool_choice))]
    UnsupportedToolChoice,
    /// A message used a role outside Anthropic's supported subset.
    #[error("unsupported Anthropic role")]
    #[diagnostic(code(eliza::anthropic::unsupported_role))]
    UnsupportedRole,
    /// A system prompt contained a non-text block.
    #[error("only text system blocks are supported")]
    #[diagnostic(code(eliza::anthropic::unsupported_system_block))]
    UnsupportedSystemBlock,
    /// A user message omitted its content blocks.
    #[error("user content blocks are required")]
    #[diagnostic(code(eliza::anthropic::missing_user_content))]
    MissingUserContent,
    /// A user message contained an unsupported block kind.
    #[error("only text and tool_result blocks are supported")]
    #[diagnostic(code(eliza::anthropic::unsupported_user_block))]
    UnsupportedUserBlock,
    /// A tool-use block omitted its function name.
    #[error("tool_use block is missing name")]
    #[diagnostic(code(eliza::anthropic::missing_tool_use_name))]
    MissingToolUseName,
    /// A tool-use block omitted its input object.
    #[error("tool_use block is missing input")]
    #[diagnostic(code(eliza::anthropic::missing_tool_use_input))]
    MissingToolUseInput,
    /// An assistant message contained an unsupported block kind.
    #[error("only text and tool_use blocks are supported")]
    #[diagnostic(code(eliza::anthropic::unsupported_assistant_block))]
    UnsupportedAssistantBlock,
    /// A tool result contained a non-text block.
    #[error("only text tool-result blocks are supported")]
    #[diagnostic(code(eliza::anthropic::unsupported_tool_result_block))]
    UnsupportedToolResultBlock,
    /// A tool result omitted its content.
    #[error("tool result content is required")]
    #[diagnostic(code(eliza::anthropic::missing_tool_result_content))]
    MissingToolResultContent,
    /// The request selected an unknown output format.
    #[error("unsupported output format")]
    #[diagnostic(code(eliza::anthropic::unsupported_output_format))]
    UnsupportedOutputFormat,
    /// The requested schema uses unsupported JSON Schema behavior.
    #[error("unsupported output schema: {source}")]
    #[diagnostic(code(eliza::anthropic::unsupported_output_schema))]
    UnsupportedOutputSchema {
        /// Shared compiler failure with an RFC 6901 path.
        #[source]
        source: chat::structured_output::StructuredOutputError,
    },
}

impl ApiError for AnthropicError {
    fn class(&self) -> ProblemClass {
        match self {
            Self::Image(source) => match source.kind() {
                image::errors::ErrorKind::InvalidInput => ProblemClass::InvalidRequest,
                image::errors::ErrorKind::Limit => ProblemClass::RequestTooLarge,
            },
            Self::UnsupportedOutputFormat
            | Self::UnsupportedOutputSchema { .. }
            | Self::UnsupportedThinkingMode
            | Self::UnsupportedThinkingDisplay
            | Self::UnsupportedEffort
            | Self::UnsupportedToolChoice
            | Self::UnsupportedRole
            | Self::UnsupportedSystemBlock
            | Self::UnsupportedUserBlock
            | Self::UnsupportedAssistantBlock
            | Self::UnsupportedToolResultBlock => ProblemClass::UnsupportedRequest,
            _ => ProblemClass::InvalidRequest,
        }
    }

    fn param(&self) -> Option<&'static str> {
        match self {
            Self::ChatModelRequired { .. } => Some("model"),
            Self::InvalidMaxTokens => Some("max_tokens"),
            Self::InvalidThinkingBudget
            | Self::ThinkingBudgetTooLarge
            | Self::UnsupportedThinkingMode
            | Self::UnsupportedThinkingDisplay => Some("thinking"),
            Self::UnsupportedEffort => Some("output_config.effort"),
            Self::InvalidOutputFormat
            | Self::InvalidOutputSchema { .. }
            | Self::UnsupportedOutputFormat
            | Self::UnsupportedOutputSchema { .. } => Some("output_config.format"),
            Self::MissingToolName | Self::MissingToolInputSchema => Some("tools"),
            Self::MissingNamedToolChoice | Self::UnsupportedToolChoice => Some("tool_choice"),
            Self::UnsupportedRole => Some("messages.role"),
            Self::UnsupportedSystemBlock => Some("system"),
            Self::Image(_)
            | Self::MissingUserContent
            | Self::UnsupportedUserBlock
            | Self::MissingToolUseName
            | Self::MissingToolUseInput
            | Self::UnsupportedAssistantBlock
            | Self::UnsupportedToolResultBlock
            | Self::MissingToolResultContent => Some("messages.content"),
        }
    }
}

// -----------------------------------------------------------------------------
// AnthropicErrorKind: Maps neutral failure classes to wire kinds.
// -----------------------------------------------------------------------------

/// Anthropic's serialized error classification.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum AnthropicErrorKind {
    /// Invalid or unsupported request input.
    InvalidRequestError,
    /// Missing or invalid authentication.
    AuthenticationError,
    /// Request body exceeding the accepted limit.
    RequestTooLarge,
    /// Internal server failure.
    ApiError,
    /// Personal request allowance exhausted.
    RateLimitError,
    /// Shared service capacity is unavailable.
    OverloadedError,
}

impl From<ProblemClass> for AnthropicErrorKind {
    fn from(class: ProblemClass) -> Self {
        match class {
            ProblemClass::InvalidRequest | ProblemClass::UnsupportedRequest => {
                Self::InvalidRequestError
            }
            ProblemClass::Authentication => Self::AuthenticationError,
            ProblemClass::RequestTooLarge => Self::RequestTooLarge,
            ProblemClass::Internal => Self::ApiError,
            ProblemClass::RateLimit => Self::RateLimitError,
            ProblemClass::Unavailable => Self::OverloadedError,
        }
    }
}

// -----------------------------------------------------------------------------
// AnthropicFailureBody: Carries the nested wire error object.
// -----------------------------------------------------------------------------

/// Nested Anthropic error payload.
#[derive(Debug, Serialize, JsonSchema)]
struct AnthropicFailureBody {
    /// Provider error classification.
    #[serde(rename = "type")]
    kind: AnthropicErrorKind,
    /// Human-readable failure detail.
    message: String,
}

// -----------------------------------------------------------------------------
// AnthropicFailureResponse: Carries the top-level wire envelope.
// -----------------------------------------------------------------------------

/// Anthropic top-level failure envelope.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct AnthropicFailureResponse {
    /// Envelope discriminator required by Anthropic clients.
    #[serde(rename = "type")]
    kind: &'static str,
    /// Provider-native error payload.
    error: AnthropicFailureBody,
}

// -----------------------------------------------------------------------------
// AnthropicRejection: Renders typed problems in the native wire format.
// -----------------------------------------------------------------------------

/// Provider-native renderer for one typed problem.
#[derive(derive_more::From)]
pub struct AnthropicRejection(
    /// Neutral problem awaiting Anthropic wire rendering.
    NativeError,
);

impl AnthropicRejection {
    /// Capture a typed diagnostic for Anthropic rendering.
    pub(super) fn from_error(error: &impl ApiError) -> Self {
        Self(NativeError::from_error(error))
    }
}

impl From<&chat::errors::Error> for AnthropicRejection {
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

impl IntoResponse for AnthropicRejection {
    fn into_response(self) -> Response {
        let problem = self.0;
        let mut response = (
            problem.status(),
            Json(AnthropicFailureResponse {
                kind: "error",
                error: AnthropicFailureBody {
                    kind: problem.class().into(),
                    message: problem.message().to_owned(),
                },
            }),
        )
            .into_response();
        problem.write_error_code(response.headers_mut());
        response
    }
}
