//! Typed Gemini adapter failures and wire rendering.
use axum::Json;
use axum::response::{IntoResponse, Response};
use miette::Diagnostic;
use schemars::JsonSchema;
use serde::Serialize;
use thiserror::Error;

use crate::problem::{Problem, ProblemClass, ProblemDetails};

// -----------------------------------------------------------------------------
// GeminiError: Classifies failures found while lowering requests.
// -----------------------------------------------------------------------------

/// Failure detected while lowering a Gemini request.
#[expect(
    rlib::undocumented_items,
    reason = "thiserror messages and Miette codes are the canonical contracts for private adapter failures"
)]
#[derive(Debug, Diagnostic, Error)]
pub(super) enum GeminiError {
    #[error("only client function declarations are supported")]
    #[diagnostic(code(eliza::gemini::missing_function_declarations))]
    MissingFunctionDeclarations,
    #[error("function declaration is missing name")]
    #[diagnostic(code(eliza::gemini::missing_function_name))]
    MissingFunctionName,
    #[error("unsupported function calling mode")]
    #[diagnostic(code(eliza::gemini::unsupported_calling_mode))]
    UnsupportedCallingMode { param: &'static str },
    #[error("unsupported Gemini model action `{action}`")]
    #[diagnostic(code(eliza::gemini::unsupported_model_action))]
    UnsupportedModelAction { action: String },
    #[error("Gemini model path must include a generation action")]
    #[diagnostic(code(eliza::gemini::missing_model_action))]
    MissingModelAction,
    #[error("Gemini model id must not be empty")]
    #[diagnostic(code(eliza::gemini::empty_model))]
    EmptyModel,
    #[error("unsupported Gemini role")]
    #[diagnostic(code(eliza::gemini::unsupported_role))]
    UnsupportedRole,
    #[error("content parts are required")]
    #[diagnostic(code(eliza::gemini::missing_content_parts))]
    MissingContentParts,
    #[error("functionResponse is missing response")]
    #[diagnostic(code(eliza::gemini::missing_function_response))]
    MissingFunctionResponse,
    #[error("only text and functionResponse parts are supported")]
    #[diagnostic(code(eliza::gemini::unsupported_user_part))]
    UnsupportedUserPart,
    #[error("functionCall is missing name")]
    #[diagnostic(code(eliza::gemini::missing_function_call_name))]
    MissingFunctionCallName,
    #[error("only text and functionCall parts are supported")]
    #[diagnostic(code(eliza::gemini::unsupported_model_part))]
    UnsupportedModelPart,
    #[error("text parts are required")]
    #[diagnostic(code(eliza::gemini::missing_text_parts))]
    MissingTextParts { param: &'static str },
    #[error("only text parts are supported")]
    #[diagnostic(code(eliza::gemini::unsupported_text_part))]
    UnsupportedTextPart { param: &'static str },
    #[error("audio generation requires model `eliza-retro-tts`")]
    #[diagnostic(code(eliza::gemini::speech_model_required))]
    SpeechModelRequired,
    #[error("model `eliza-retro-tts` only supports AUDIO responses")]
    #[diagnostic(code(eliza::gemini::speech_model_audio_only))]
    SpeechModelAudioOnly,
    #[error("responseModalities must contain exactly TEXT or AUDIO")]
    #[diagnostic(code(eliza::gemini::invalid_response_modalities))]
    InvalidResponseModalities,
    #[error("speechConfig and a nonempty voice are required")]
    #[diagnostic(code(eliza::gemini::missing_speech_config))]
    MissingSpeechConfig,
    #[error("unsupported speech response format")]
    #[diagnostic(code(eliza::gemini::unsupported_speech_format))]
    UnsupportedSpeechFormat,
    #[error("audio generation supports only user text parts")]
    #[diagnostic(code(eliza::gemini::speech_text_only))]
    SpeechTextOnly,
    #[error("tools and system instructions are not supported for audio generation")]
    #[diagnostic(code(eliza::gemini::speech_controls_unsupported))]
    SpeechControlsUnsupported,
    #[error("multi-speaker speech requires exactly two distinct speaker mappings")]
    #[diagnostic(code(eliza::gemini::invalid_speaker_config))]
    InvalidSpeakerConfig,
    #[error("speech part has a missing or unknown speaker")]
    #[diagnostic(code(eliza::gemini::unknown_speaker))]
    UnknownSpeaker,
}

impl ProblemDetails for GeminiError {
    fn class(&self) -> ProblemClass {
        match self {
            Self::MissingFunctionDeclarations
            | Self::UnsupportedCallingMode { .. }
            | Self::UnsupportedModelAction { .. }
            | Self::UnsupportedRole
            | Self::UnsupportedUserPart
            | Self::UnsupportedModelPart
            | Self::UnsupportedTextPart { .. }
            | Self::UnsupportedSpeechFormat
            | Self::SpeechTextOnly
            | Self::SpeechControlsUnsupported => ProblemClass::UnsupportedRequest,
            _ => ProblemClass::InvalidRequest,
        }
    }

    fn param(&self) -> Option<&'static str> {
        match self {
            Self::MissingFunctionDeclarations | Self::MissingFunctionName => Some("tools"),
            Self::UnsupportedCallingMode { param }
            | Self::MissingTextParts { param }
            | Self::UnsupportedTextPart { param } => Some(param),
            Self::UnsupportedModelAction { .. } | Self::MissingModelAction | Self::EmptyModel => {
                Some("model")
            }
            Self::UnsupportedRole => Some("contents.role"),
            Self::MissingContentParts
            | Self::MissingFunctionResponse
            | Self::UnsupportedUserPart
            | Self::MissingFunctionCallName
            | Self::UnsupportedModelPart
            | Self::SpeechTextOnly
            | Self::UnknownSpeaker => Some("contents.parts"),
            Self::SpeechModelRequired | Self::SpeechModelAudioOnly => Some("model"),
            Self::InvalidResponseModalities => Some("generationConfig.responseModalities"),
            Self::MissingSpeechConfig | Self::InvalidSpeakerConfig => {
                Some("generationConfig.speechConfig")
            }
            Self::UnsupportedSpeechFormat => Some("generationConfig.responseFormat.audio.mimeType"),
            Self::SpeechControlsUnsupported => Some("generationConfig"),
        }
    }
}

// -----------------------------------------------------------------------------
// GeminiErrorStatus: Maps neutral failure classes to wire statuses.
// -----------------------------------------------------------------------------

/// Gemini's serialized error classification.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum GeminiErrorStatus {
    /// Invalid or unsupported request input.
    InvalidArgument,
    /// Missing or invalid authentication.
    Unauthenticated,
    /// Request body exceeding the accepted limit.
    ResourceExhausted,
    /// Internal server failure.
    Internal,
}

impl From<ProblemClass> for GeminiErrorStatus {
    fn from(class: ProblemClass) -> Self {
        match class {
            ProblemClass::InvalidRequest | ProblemClass::UnsupportedRequest => {
                Self::InvalidArgument
            }
            ProblemClass::Authentication => Self::Unauthenticated,
            ProblemClass::RequestTooLarge => Self::ResourceExhausted,
            ProblemClass::Internal => Self::Internal,
        }
    }
}

// -----------------------------------------------------------------------------
// GeminiFailureBody: Carries the nested wire error object.
// -----------------------------------------------------------------------------

/// Nested Gemini error payload.
#[derive(Debug, Serialize, JsonSchema)]
struct GeminiFailureBody {
    /// Numeric HTTP status.
    code: u16,
    /// Human-readable failure detail.
    message: String,
    /// Provider error classification.
    status: GeminiErrorStatus,
}

// -----------------------------------------------------------------------------
// GeminiFailureResponse: Carries the top-level wire envelope.
// -----------------------------------------------------------------------------

/// Gemini top-level failure envelope.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct GeminiFailureResponse {
    /// Provider-native error payload.
    error: GeminiFailureBody,
}

// -----------------------------------------------------------------------------
// GeminiRejection: Renders typed problems in the native wire format.
// -----------------------------------------------------------------------------

/// Provider-native renderer for one typed problem.
pub(super) struct GeminiRejection(
    /// Neutral problem awaiting Gemini wire rendering.
    Problem,
);

impl GeminiRejection {
    /// Capture a typed diagnostic for Gemini rendering.
    pub(super) fn from_error(error: &impl ProblemDetails) -> Self {
        Self(Problem::from_error(error))
    }
}

impl IntoResponse for GeminiRejection {
    fn into_response(self) -> Response {
        let problem = self.0;
        let mut response = (
            problem.status(),
            Json(GeminiFailureResponse {
                error: GeminiFailureBody {
                    code: problem.status().as_u16(),
                    message: problem.message().to_owned(),
                    status: problem.class().into(),
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

    /// Verify every neutral class maps to the intended Gemini status.
    ///
    /// # Panics
    ///
    /// Panics when a class regresses to the wrong wire status.
    #[test]
    fn maps_problem_classes_to_native_statuses() {
        assert_eq!(
            GeminiErrorStatus::from(ProblemClass::InvalidRequest),
            GeminiErrorStatus::InvalidArgument
        );
        assert_eq!(
            GeminiErrorStatus::from(ProblemClass::UnsupportedRequest),
            GeminiErrorStatus::InvalidArgument
        );
        assert_eq!(
            GeminiErrorStatus::from(ProblemClass::Authentication),
            GeminiErrorStatus::Unauthenticated
        );
        assert_eq!(
            GeminiErrorStatus::from(ProblemClass::RequestTooLarge),
            GeminiErrorStatus::ResourceExhausted
        );
        assert_eq!(
            GeminiErrorStatus::from(ProblemClass::Internal),
            GeminiErrorStatus::Internal
        );
    }
}
