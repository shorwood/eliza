//! Typed Gemini adapter failures and wire rendering.
use axum::Json;
use axum::response::{IntoResponse, Response};
use eliza_http::errors::EncodingError;
use eliza_http::model::ModelId;
use eliza_http::problem::{ApiError, NativeError, ProblemClass};
use eliza_modality_chat as chat;
use eliza_modality_embedding as embedding;
use eliza_modality_image as image;
use eliza_modality_speech as speech;
use miette::Diagnostic;
use schemars::JsonSchema;
use serde::Serialize;
use thiserror::Error;

// -----------------------------------------------------------------------------
// GeminiError: Classifies failures found while lowering requests.
// -----------------------------------------------------------------------------

/// Failure detected while lowering a Gemini request.
#[derive(Debug, Diagnostic, Error)]
pub(super) enum GeminiError {
    /// Shared image source validation failed during provider lowering.
    #[error(transparent)]
    #[diagnostic(transparent)]
    Image(
        /// Provider-neutral image failure.
        image::errors::Error,
    ),

    /// An embedding body selected a model other than the fixed resource.
    #[error("embeddings require model `models/{expected}`")]
    #[diagnostic(code(eliza::gemini::embedding_model_required))]
    EmbeddingModelRequired {
        /// Model selected by the embedding URL.
        expected: ModelId,
    },
    /// A native embedding request omitted its content.
    #[error("embedding content and text parts are required")]
    #[diagnostic(code(eliza::gemini::missing_embedding_content))]
    MissingEmbeddingContent,
    /// A native batch embedding request omitted its request list.
    #[error("embedding requests are required")]
    #[diagnostic(code(eliza::gemini::missing_embedding_requests))]
    MissingEmbeddingRequests,
    /// Native embedding content contained a non-text part.
    #[error("embeddings support only text content parts")]
    #[diagnostic(code(eliza::gemini::embedding_text_only))]
    EmbeddingTextOnly,
    /// Legacy and current Gemini dimension fields disagreed.
    #[error("embedding dimensionality fields must agree")]
    #[diagnostic(code(eliza::gemini::conflicting_embedding_dimensions))]
    ConflictingEmbeddingDimensions,
    /// Current and legacy text-output controls were combined.
    #[error(
        "generationConfig.responseFormat.text cannot be combined with legacy response format fields"
    )]
    #[diagnostic(code(eliza::gemini::conflicting_response_formats))]
    ConflictingResponseFormats,
    /// Numeric and named thinking controls were combined.
    #[error("thinkingBudget and thinkingLevel cannot be combined")]
    #[diagnostic(code(eliza::gemini::conflicting_thinking_controls))]
    ConflictingThinkingControls,
    /// A thinking budget was below Gemini's dynamic-budget sentinel.
    #[error("thinkingBudget must be -1 or nonnegative")]
    #[diagnostic(code(eliza::gemini::invalid_thinking_budget))]
    InvalidThinkingBudget,
    /// A named thinking level was outside the current contract.
    #[error("unsupported thinkingLevel")]
    #[diagnostic(code(eliza::gemini::unsupported_thinking_level))]
    UnsupportedThinkingLevel,
    /// The requested output-token limit was zero.
    #[error("maxOutputTokens must be positive")]
    #[diagnostic(code(eliza::gemini::invalid_max_output_tokens))]
    InvalidMaxOutputTokens,
    /// A current Gemini embedding control cannot be honored locally.
    #[error("unsupported embedding control `{param}`")]
    #[diagnostic(code(eliza::gemini::unsupported_embedding_control))]
    UnsupportedEmbeddingControl {
        /// Request field selecting unsupported behavior.
        param: &'static str,
    },
    /// A text-output control has an incompatible or incomplete shape.
    #[error("invalid response format: {reason}")]
    #[diagnostic(code(eliza::gemini::invalid_response_format))]
    InvalidResponseFormat {
        /// Request field containing the invalid format.
        param: &'static str,
        /// Provider-contract requirement that was not satisfied.
        reason: &'static str,
    },
    /// The requested schema is malformed or unsatisfiable.
    #[error("invalid response schema: {source}")]
    #[diagnostic(code(eliza::gemini::invalid_response_schema))]
    InvalidResponseSchema {
        /// Request field containing the schema.
        param: &'static str,
        /// Shared compiler failure with an RFC 6901 path.
        #[source]
        source: chat::structured_output::StructuredOutputError,
    },
    /// A tool group omitted its function declarations.
    #[error("only client function declarations are supported")]
    #[diagnostic(code(eliza::gemini::missing_function_declarations))]
    MissingFunctionDeclarations,
    /// A function declaration omitted its name.
    #[error("function declaration is missing name")]
    #[diagnostic(code(eliza::gemini::missing_function_name))]
    MissingFunctionName,
    /// The requested function-calling mode cannot be represented.
    #[error("unsupported function calling mode")]
    #[diagnostic(code(eliza::gemini::unsupported_calling_mode))]
    UnsupportedCallingMode {
        /// Request field containing the unsupported mode.
        param: &'static str,
    },
    /// The model path names an unsupported generation action.
    #[error("unsupported Gemini model action `{action}`")]
    #[diagnostic(code(eliza::gemini::unsupported_model_action))]
    UnsupportedModelAction {
        /// Unsupported action suffix from the model path.
        action: String,
    },
    /// The model path omitted its generation action.
    #[error("Gemini model path must include a generation action")]
    #[diagnostic(code(eliza::gemini::missing_model_action))]
    MissingModelAction,
    /// The model path contained an empty model identifier.
    #[error("Gemini model id must not be empty")]
    #[diagnostic(code(eliza::gemini::empty_model))]
    EmptyModel,
    /// A content record used a role outside Gemini's supported subset.
    #[error("unsupported Gemini role")]
    #[diagnostic(code(eliza::gemini::unsupported_role))]
    UnsupportedRole,
    /// A content record omitted its parts.
    #[error("content parts are required")]
    #[diagnostic(code(eliza::gemini::missing_content_parts))]
    MissingContentParts,
    /// A function response omitted its response value.
    #[error("functionResponse is missing response")]
    #[diagnostic(code(eliza::gemini::missing_function_response))]
    MissingFunctionResponse,
    /// A user content record contained an unsupported part kind.
    #[error("only text and functionResponse parts are supported")]
    #[diagnostic(code(eliza::gemini::unsupported_user_part))]
    UnsupportedUserPart,
    /// A function-call part omitted its function name.
    #[error("functionCall is missing name")]
    #[diagnostic(code(eliza::gemini::missing_function_call_name))]
    MissingFunctionCallName,
    /// A model content record contained an unsupported part kind.
    #[error("only text and functionCall parts are supported")]
    #[diagnostic(code(eliza::gemini::unsupported_model_part))]
    UnsupportedModelPart,
    /// A field requiring text did not contain any text parts.
    #[error("text parts are required")]
    #[diagnostic(code(eliza::gemini::missing_text_parts))]
    MissingTextParts {
        /// Request field that requires text parts.
        param: &'static str,
    },
    /// A text-only field contained another part kind.
    #[error("only text parts are supported")]
    #[diagnostic(code(eliza::gemini::unsupported_text_part))]
    UnsupportedTextPart {
        /// Request field containing the unsupported part.
        param: &'static str,
    },
    /// The request selected an unknown text-output MIME type.
    #[error("unsupported text response format")]
    #[diagnostic(code(eliza::gemini::unsupported_response_format))]
    UnsupportedResponseFormat {
        /// Request field selecting the unsupported format.
        param: &'static str,
    },
    /// The requested schema uses unsupported JSON Schema behavior.
    #[error("unsupported response schema: {source}")]
    #[diagnostic(code(eliza::gemini::unsupported_response_schema))]
    UnsupportedResponseSchema {
        /// Request field containing the schema.
        param: &'static str,
        /// Shared compiler failure with an RFC 6901 path.
        #[source]
        source: chat::structured_output::StructuredOutputError,
    },
    /// Text generation included image-specific response controls.
    #[error("image response formats are not valid for TEXT responses")]
    #[diagnostic(code(eliza::gemini::image_format_for_text))]
    ImageFormatForText,
    /// Audio generation included image-specific response controls.
    #[error("image response formats are not valid for AUDIO responses")]
    #[diagnostic(code(eliza::gemini::image_format_for_audio))]
    ImageFormatForAudio,
    /// Audio generation included text-output formatting controls.
    #[error("text response formats are not valid for AUDIO responses")]
    #[diagnostic(code(eliza::gemini::text_format_for_audio))]
    TextFormatForAudio,
    /// Audio generation used a model other than the local speech model.
    #[error("audio generation requires model {expected}")]
    #[diagnostic(code(eliza::gemini::speech_model_required))]
    SpeechModelRequired {
        /// Configured names accepted for audio generation.
        expected: String,
    },
    /// The local speech model was asked for a non-audio response.
    #[error("model `{model}` only supports AUDIO responses")]
    #[diagnostic(code(eliza::gemini::speech_model_audio_only))]
    SpeechModelAudioOnly {
        /// Requested dedicated speech alias.
        model: ModelId,
    },
    /// Image generation used a model other than the local image fixture.
    #[error("image generation requires model {expected}")]
    #[diagnostic(code(eliza::gemini::image_generation_model_required))]
    ImageGenerationModelRequired {
        /// Configured names accepted for image generation.
        expected: String,
    },
    /// The local image model was asked for a response without an image.
    #[error("model `{model}` requires IMAGE output")]
    #[diagnostic(code(eliza::gemini::image_model_requires_image))]
    ImageModelRequiresImage {
        /// Requested dedicated image alias.
        model: ModelId,
    },
    /// Response modalities did not select one supported output combination.
    #[error("responseModalities must select TEXT, AUDIO, IMAGE, or TEXT with IMAGE")]
    #[diagnostic(code(eliza::gemini::invalid_response_modalities))]
    InvalidResponseModalities,
    /// Audio generation omitted its speech configuration or voice.
    #[error("speechConfig and a nonempty voice are required")]
    #[diagnostic(code(eliza::gemini::missing_speech_config))]
    MissingSpeechConfig,
    /// The requested speech encoding is not implemented.
    #[error("unsupported speech response format")]
    #[diagnostic(code(eliza::gemini::unsupported_speech_format))]
    UnsupportedSpeechFormat,
    /// Audio generation received non-text user content.
    #[error("audio generation supports only user text parts")]
    #[diagnostic(code(eliza::gemini::speech_text_only))]
    SpeechTextOnly,
    /// Audio generation included tools or a system instruction.
    #[error("tools and system instructions are not supported for audio generation")]
    #[diagnostic(code(eliza::gemini::speech_controls_unsupported))]
    SpeechControlsUnsupported,
    /// Image generation received anything other than one user text prompt.
    #[error("image generation supports one user content record with text parts only")]
    #[diagnostic(code(eliza::gemini::image_text_only))]
    ImageTextOnly,
    /// Image generation included controls outside the initial compatibility subset.
    #[error("unsupported image generation control `{param}`")]
    #[diagnostic(code(eliza::gemini::unsupported_image_control))]
    UnsupportedImageControl {
        /// Request field selecting unsupported behavior.
        param: &'static str,
    },
    /// Image generation selected a ratio outside the fixed subset.
    #[error("unsupported image aspect ratio")]
    #[diagnostic(code(eliza::gemini::unsupported_image_aspect_ratio))]
    UnsupportedImageAspectRatio,
    /// Image generation selected a size class outside the fixed subset.
    #[error("unsupported image size")]
    #[diagnostic(code(eliza::gemini::unsupported_image_size))]
    UnsupportedImageSize,
    /// A multi-speaker request did not define two distinct speakers.
    #[error("multi-speaker speech requires exactly two distinct speaker mappings")]
    #[diagnostic(code(eliza::gemini::invalid_speaker_config))]
    InvalidSpeakerConfig,
    /// A speech segment omitted its speaker or named an unknown one.
    #[error("speech part has a missing or unknown speaker")]
    #[diagnostic(code(eliza::gemini::unknown_speaker))]
    UnknownSpeaker,
}

impl ApiError for GeminiError {
    fn class(&self) -> ProblemClass {
        match self {
            Self::Image(source) => match source.kind() {
                image::errors::ErrorKind::InvalidInput => ProblemClass::InvalidRequest,
                image::errors::ErrorKind::Limit => ProblemClass::RequestTooLarge,
            },
            Self::EmbeddingTextOnly
            | Self::UnsupportedEmbeddingControl { .. }
            | Self::UnsupportedResponseFormat { .. }
            | Self::UnsupportedResponseSchema { .. }
            | Self::ImageFormatForText
            | Self::ImageFormatForAudio
            | Self::MissingFunctionDeclarations
            | Self::UnsupportedCallingMode { .. }
            | Self::UnsupportedModelAction { .. }
            | Self::UnsupportedRole
            | Self::UnsupportedUserPart
            | Self::UnsupportedModelPart
            | Self::UnsupportedTextPart { .. }
            | Self::UnsupportedSpeechFormat
            | Self::SpeechTextOnly
            | Self::SpeechControlsUnsupported
            | Self::ImageTextOnly
            | Self::UnsupportedImageControl { .. }
            | Self::UnsupportedImageAspectRatio
            | Self::UnsupportedImageSize => ProblemClass::UnsupportedRequest,
            _ => ProblemClass::InvalidRequest,
        }
    }

    fn param(&self) -> Option<&'static str> {
        match self {
            Self::MissingEmbeddingContent | Self::EmbeddingTextOnly => Some("content.parts"),
            Self::MissingEmbeddingRequests => Some("requests"),
            Self::ConflictingEmbeddingDimensions => Some("outputDimensionality"),
            Self::ConflictingResponseFormats
            | Self::ConflictingThinkingControls
            | Self::InvalidThinkingBudget
            | Self::UnsupportedThinkingLevel
            | Self::InvalidMaxOutputTokens
            | Self::TextFormatForAudio
            | Self::SpeechControlsUnsupported => Some("generationConfig"),
            Self::ImageFormatForText | Self::ImageFormatForAudio => {
                Some("generationConfig.responseFormat.image")
            }
            Self::MissingFunctionDeclarations | Self::MissingFunctionName => Some("tools"),
            Self::UnsupportedEmbeddingControl { param }
            | Self::InvalidResponseFormat { param, .. }
            | Self::InvalidResponseSchema { param, .. }
            | Self::UnsupportedResponseFormat { param }
            | Self::UnsupportedResponseSchema { param, .. }
            | Self::UnsupportedCallingMode { param }
            | Self::MissingTextParts { param }
            | Self::UnsupportedTextPart { param }
            | Self::UnsupportedImageControl { param } => Some(param),
            Self::UnsupportedModelAction { .. } | Self::MissingModelAction | Self::EmptyModel => {
                Some("model")
            }
            Self::UnsupportedRole => Some("contents.role"),
            Self::Image(_)
            | Self::MissingContentParts
            | Self::MissingFunctionResponse
            | Self::UnsupportedUserPart
            | Self::MissingFunctionCallName
            | Self::UnsupportedModelPart
            | Self::SpeechTextOnly
            | Self::ImageTextOnly
            | Self::UnknownSpeaker => Some("contents.parts"),
            Self::EmbeddingModelRequired { .. }
            | Self::SpeechModelRequired { .. }
            | Self::SpeechModelAudioOnly { .. }
            | Self::ImageGenerationModelRequired { .. }
            | Self::ImageModelRequiresImage { .. } => Some("model"),
            Self::InvalidResponseModalities => Some("generationConfig.responseModalities"),
            Self::MissingSpeechConfig | Self::InvalidSpeakerConfig => {
                Some("generationConfig.speechConfig")
            }
            Self::UnsupportedSpeechFormat => Some("generationConfig.responseFormat.audio.mimeType"),
            Self::UnsupportedImageAspectRatio => {
                Some("generationConfig.responseFormat.image.aspectRatio")
            }
            Self::UnsupportedImageSize => Some("generationConfig.responseFormat.image.imageSize"),
        }
    }
}

// -----------------------------------------------------------------------------
// GeminiSpeechStreamError: Preserves failures after response streaming begins.
// -----------------------------------------------------------------------------

/// Failure that can terminate a Gemini speech stream after headers are sent.
#[derive(Debug, Error)]
pub(super) enum GeminiSpeechStreamError {
    /// A typed server-sent event could not be encoded.
    #[error(transparent)]
    Encoding(
        /// Provider-neutral response encoding failure.
        EncodingError,
    ),
    /// A Gemini JSON response record could not be encoded.
    #[error("failed to encode Gemini speech record: {source}")]
    Json {
        /// JSON serialization failure.
        #[source]
        source: serde_json::Error,
    },
    /// Speech synthesis or audio encoding failed.
    #[error(transparent)]
    Speech(
        /// Provider-neutral speech failure.
        speech::errors::Error,
    ),
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
    /// Shared service capacity is unavailable.
    Unavailable,
}

impl From<ProblemClass> for GeminiErrorStatus {
    fn from(class: ProblemClass) -> Self {
        match class {
            ProblemClass::InvalidRequest | ProblemClass::UnsupportedRequest => {
                Self::InvalidArgument
            }
            ProblemClass::Authentication => Self::Unauthenticated,
            ProblemClass::RequestTooLarge | ProblemClass::RateLimit => Self::ResourceExhausted,
            ProblemClass::Internal => Self::Internal,
            ProblemClass::Unavailable => Self::Unavailable,
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
#[derive(derive_more::From)]
pub struct GeminiRejection(
    /// Neutral problem awaiting Gemini wire rendering.
    NativeError,
);

impl GeminiRejection {
    /// Capture a typed diagnostic for Gemini rendering.
    pub(super) fn from_error(error: &impl ApiError) -> Self {
        Self(NativeError::from_error(error))
    }
}

impl From<&chat::errors::Error> for GeminiRejection {
    fn from(error: &chat::errors::Error) -> Self {
        let class = match error.kind() {
            chat::errors::ErrorKind::InvalidInput => ProblemClass::InvalidRequest,
            chat::errors::ErrorKind::Limit => ProblemClass::RequestTooLarge,
            chat::errors::ErrorKind::Internal => ProblemClass::Internal,
        };
        let param = match error.field() {
            Some(chat::errors::ErrorField::Input) => Some("contents"),
            Some(chat::errors::ErrorField::Tools) => Some("tools"),
            Some(chat::errors::ErrorField::ToolChoice) => Some("toolConfig"),
            None => None,
        };
        Self(NativeError::from_diagnostic(error, class, param))
    }
}

impl From<&embedding::engine::Error> for GeminiRejection {
    fn from(error: &embedding::engine::Error) -> Self {
        let class = match error.kind() {
            embedding::engine::ErrorKind::InvalidInput => ProblemClass::InvalidRequest,
            embedding::engine::ErrorKind::Limit => ProblemClass::RequestTooLarge,
        };
        let param = match error.field() {
            embedding::engine::ErrorField::Input => Some("content.parts"),
            embedding::engine::ErrorField::Dimensions => Some("outputDimensionality"),
        };
        Self(NativeError::from_diagnostic(error, class, param))
    }
}

impl From<&image::generation::Error> for GeminiRejection {
    fn from(error: &image::generation::Error) -> Self {
        let class = match error.kind() {
            image::generation::ErrorKind::InvalidInput => ProblemClass::InvalidRequest,
            image::generation::ErrorKind::Limit => ProblemClass::RequestTooLarge,
            image::generation::ErrorKind::Internal => ProblemClass::Internal,
        };
        let param = match error.field() {
            image::generation::ErrorField::Prompt => Some("contents.parts"),
            image::generation::ErrorField::Count | image::generation::ErrorField::None => None,
        };
        Self(NativeError::from_diagnostic(error, class, param))
    }
}

impl From<&speech::errors::Error> for GeminiRejection {
    fn from(error: &speech::errors::Error) -> Self {
        let class = match error.kind() {
            speech::errors::ErrorKind::InvalidInput => ProblemClass::InvalidRequest,
            speech::errors::ErrorKind::Limit => ProblemClass::RequestTooLarge,
            speech::errors::ErrorKind::Internal => ProblemClass::Internal,
            speech::errors::ErrorKind::Overloaded => ProblemClass::Unavailable,
        };
        let param = match error.field() {
            Some(speech::errors::ErrorField::Input) => Some("contents.parts"),
            Some(speech::errors::ErrorField::SampleRate) => {
                Some("generationConfig.responseFormat.audio.mimeType")
            }
            Some(speech::errors::ErrorField::Speed) => Some("generationConfig.speechConfig"),
            None => None,
        };
        Self(NativeError::from_diagnostic(error, class, param))
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
