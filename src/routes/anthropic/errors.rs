//! Typed Anthropic adapter failures and wire rendering.
use axum::Json;
use axum::response::{IntoResponse, Response};
use miette::Diagnostic;
use schemars::JsonSchema;
use serde::Serialize;
use thiserror::Error;

use crate::problem::{Problem, ProblemClass, ProblemDetails};

// -----------------------------------------------------------------------------
// AnthropicError: Classifies failures found while lowering requests.
// -----------------------------------------------------------------------------

/// Failure detected while lowering an Anthropic request.
#[expect(
    rlib::undocumented_items,
    reason = "thiserror messages and Miette codes are the canonical contracts for private adapter failures"
)]
#[derive(Debug, Diagnostic, Error)]
pub(super) enum AnthropicError {
    #[error("tool is missing its name")]
    #[diagnostic(code(eliza::anthropic::missing_tool_name))]
    MissingToolName,
    #[error("function tool is missing input_schema")]
    #[diagnostic(code(eliza::anthropic::missing_tool_input_schema))]
    MissingToolInputSchema,
    #[error("named tool_choice is missing name")]
    #[diagnostic(code(eliza::anthropic::missing_named_tool_choice))]
    MissingNamedToolChoice,
    #[error("unsupported tool_choice type")]
    #[diagnostic(code(eliza::anthropic::unsupported_tool_choice))]
    UnsupportedToolChoice,
    #[error("unsupported Anthropic role")]
    #[diagnostic(code(eliza::anthropic::unsupported_role))]
    UnsupportedRole,
    #[error("only text system blocks are supported")]
    #[diagnostic(code(eliza::anthropic::unsupported_system_block))]
    UnsupportedSystemBlock,
    #[error("user content blocks are required")]
    #[diagnostic(code(eliza::anthropic::missing_user_content))]
    MissingUserContent,
    #[error("only text and tool_result blocks are supported")]
    #[diagnostic(code(eliza::anthropic::unsupported_user_block))]
    UnsupportedUserBlock,
    #[error("tool_use block is missing name")]
    #[diagnostic(code(eliza::anthropic::missing_tool_use_name))]
    MissingToolUseName,
    #[error("tool_use block is missing input")]
    #[diagnostic(code(eliza::anthropic::missing_tool_use_input))]
    MissingToolUseInput,
    #[error("only text and tool_use blocks are supported")]
    #[diagnostic(code(eliza::anthropic::unsupported_assistant_block))]
    UnsupportedAssistantBlock,
    #[error("only text tool-result blocks are supported")]
    #[diagnostic(code(eliza::anthropic::unsupported_tool_result_block))]
    UnsupportedToolResultBlock,
    #[error("tool result content is required")]
    #[diagnostic(code(eliza::anthropic::missing_tool_result_content))]
    MissingToolResultContent,
}

impl ProblemDetails for AnthropicError {
    fn class(&self) -> ProblemClass {
        match self {
            Self::UnsupportedToolChoice
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
            Self::MissingToolName | Self::MissingToolInputSchema => Some("tools"),
            Self::MissingNamedToolChoice | Self::UnsupportedToolChoice => Some("tool_choice"),
            Self::UnsupportedRole => Some("messages.role"),
            Self::UnsupportedSystemBlock => Some("system"),
            Self::MissingUserContent
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
pub(super) struct AnthropicRejection(
    /// Neutral problem awaiting Anthropic wire rendering.
    Problem,
);

impl AnthropicRejection {
    /// Capture a typed diagnostic for Anthropic rendering.
    pub(super) fn from_error(error: &impl ProblemDetails) -> Self {
        Self(Problem::from_error(error))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify every neutral class maps to the intended Anthropic error type.
    ///
    /// # Panics
    ///
    /// Panics when a class regresses to a non-native wire type.
    #[test]
    fn maps_problem_classes_to_native_kinds() {
        assert_eq!(
            AnthropicErrorKind::from(ProblemClass::InvalidRequest),
            AnthropicErrorKind::InvalidRequestError
        );
        assert_eq!(
            AnthropicErrorKind::from(ProblemClass::UnsupportedRequest),
            AnthropicErrorKind::InvalidRequestError
        );
        assert_eq!(
            AnthropicErrorKind::from(ProblemClass::Authentication),
            AnthropicErrorKind::AuthenticationError
        );
        assert_eq!(
            AnthropicErrorKind::from(ProblemClass::RequestTooLarge),
            AnthropicErrorKind::RequestTooLarge
        );
        assert_eq!(
            AnthropicErrorKind::from(ProblemClass::Internal),
            AnthropicErrorKind::ApiError
        );
    }
}
