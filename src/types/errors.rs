//! Typed failures raised by provider-neutral request handling.

use miette::Diagnostic;
use thiserror::Error;

use crate::problem::{ProblemClass, ProblemDetails};

// -----------------------------------------------------------------------------
// ModelError: Rejects invalid provider-visible model identifiers.
// -----------------------------------------------------------------------------

/// Model identifier validation failure.
#[derive(Debug, Diagnostic, Error)]
pub(crate) enum ModelError {
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
// TurnError: Rejects invalid provider-neutral conversation state.
// -----------------------------------------------------------------------------

/// Conversation validation or execution failure.
#[derive(Debug, Diagnostic, Error)]
pub(crate) enum TurnError {
    /// An explicit fixture directive has the wrong outer shape.
    #[error("tool directive must be `@tool <name> <json-object>`")]
    #[diagnostic(code(eliza::turn::malformed_tool_directive))]
    MalformedToolDirective,

    /// An explicit fixture directive omits its arguments object.
    #[error("tool directive must include a JSON object")]
    #[diagnostic(code(eliza::turn::missing_tool_arguments))]
    MissingToolArguments,

    /// An explicit fixture directive omits its function name.
    #[error("tool directive must include a function name")]
    #[diagnostic(code(eliza::turn::missing_tool_name))]
    MissingToolName,

    /// An explicit fixture directive contains invalid JSON arguments.
    #[error("invalid tool arguments: {source}")]
    #[diagnostic(code(eliza::turn::invalid_tool_arguments))]
    InvalidToolArguments {
        /// JSON object decoding failure.
        #[source]
        source: serde_json::Error,
    },

    /// The selected tool policy forbids explicit calls.
    #[error("tool_choice forbids the explicit @tool directive")]
    #[diagnostic(code(eliza::turn::tool_directive_forbidden))]
    ToolDirectiveForbidden,

    /// The selected tool policy excludes the requested function.
    #[error("tool_choice does not allow function `{name}`")]
    #[diagnostic(code(eliza::turn::tool_choice_disallows))]
    ToolChoiceDisallows {
        /// Function selected by the fixture directive.
        name: String,
    },

    /// Conversation history has no ordinary user turn to replay.
    #[error("at least one ordinary user turn is required")]
    #[diagnostic(code(eliza::turn::missing_ordinary_user_turn))]
    MissingOrdinaryUserTurn,

    /// Conversation history exceeds the configured turn limit.
    #[error("too many conversation turns: {actual} > {limit}")]
    #[diagnostic(code(eliza::turn::too_many_turns))]
    TooManyTurns {
        /// Number of submitted conversation turns.
        actual: usize,
        /// Maximum accepted conversation turns.
        limit: usize,
    },

    /// Provider-visible input exceeds the configured character limit.
    #[error("input text is too large: {actual} > {limit}")]
    #[diagnostic(code(eliza::turn::input_too_large))]
    InputTooLarge {
        /// Counted provider-visible input characters.
        actual: usize,
        /// Maximum accepted input characters.
        limit: usize,
    },

    /// A fixture directive selected a function absent from the request.
    #[error("function `{name}` was not offered")]
    #[diagnostic(code(eliza::turn::tool_not_offered))]
    ToolNotOffered {
        /// Function selected by the fixture directive.
        name: String,
    },

    /// A tool result has no preceding provider-native call.
    #[error("tool result is missing its preceding tool call")]
    #[diagnostic(code(eliza::turn::orphan_tool_result))]
    OrphanToolResult,

    /// A provider-native call has no preceding fixture directive.
    #[error("tool call is missing its @tool directive")]
    #[diagnostic(code(eliza::turn::missing_tool_directive))]
    MissingToolDirective,

    /// A provider-native call differs from its fixture directive.
    #[error("tool call does not match its @tool directive")]
    #[diagnostic(code(eliza::turn::mismatched_tool_directive))]
    MismatchedToolDirective,

    /// Conversation history has no actionable user turn.
    #[error("at least one user text turn is required")]
    #[diagnostic(code(eliza::turn::missing_user_text))]
    MissingUserText,

    /// Tool policy requires a fixture directive that was not supplied.
    #[error("an explicit @tool directive is required by tool_choice")]
    #[diagnostic(code(eliza::turn::required_tool_directive))]
    RequiredToolDirective,
}

impl ProblemDetails for TurnError {
    fn class(&self) -> ProblemClass {
        match self {
            Self::TooManyTurns { .. } | Self::InputTooLarge { .. } => ProblemClass::RequestTooLarge,
            _ => ProblemClass::InvalidRequest,
        }
    }

    fn param(&self) -> Option<&'static str> {
        match self {
            Self::ToolDirectiveForbidden
            | Self::ToolChoiceDisallows { .. }
            | Self::RequiredToolDirective => Some("tool_choice"),
            Self::ToolNotOffered { .. } => Some("tools"),
            Self::MalformedToolDirective
            | Self::MissingToolArguments
            | Self::MissingToolName
            | Self::InvalidToolArguments { .. }
            | Self::MissingOrdinaryUserTurn
            | Self::OrphanToolResult
            | Self::MissingToolDirective
            | Self::MismatchedToolDirective
            | Self::MissingUserText => Some("messages"),
            Self::TooManyTurns { .. } | Self::InputTooLarge { .. } => None,
        }
    }
}

// -----------------------------------------------------------------------------
// EncodingError: Reports response serialization failures.
// -----------------------------------------------------------------------------

/// Provider response serialization failure.
#[derive(Debug, Diagnostic, Error)]
pub(crate) enum EncodingError {
    /// A typed server-sent event could not be serialized.
    #[error("failed to encode event: {source}")]
    #[diagnostic(code(eliza::encoding::sse))]
    Sse {
        /// Axum event serialization failure.
        #[source]
        source: axum::Error,
    },

    /// A typed newline-delimited JSON record could not be serialized.
    #[error("failed to encode record: {source}")]
    #[diagnostic(code(eliza::encoding::ndjson))]
    Ndjson {
        /// JSON serialization failure.
        #[source]
        source: serde_json::Error,
    },
}

impl ProblemDetails for EncodingError {
    fn class(&self) -> ProblemClass {
        ProblemClass::Internal
    }
}
