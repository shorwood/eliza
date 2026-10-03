//! Gemini wire contracts shared by its route adapters.
//! Empty metadata on documented newtype fields avoids a `schemars` 0.9 derive collision.
use eliza_http::model::ModelId;
use eliza_modality_chat as chat;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::errors::GeminiError;

// -----------------------------------------------------------------------------
// Content: Defines inbound conversation content and function records.
// -----------------------------------------------------------------------------

/// Role attached to an inbound Gemini content record.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum ContentRole {
    /// Human-authored input.
    User,
    /// Model-authored output.
    Model,
    /// Legacy function-result input.
    Function,
    /// Any role outside the supported subset.
    #[serde(other)]
    Unsupported,
}

/// Function invocation embedded in a Gemini content part.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ContentFunctionCall {
    /// Function name requested by the model.
    name: Option<String>,
    /// Arguments supplied to the function.
    args: Option<chat::json::JsonObject>,
}

impl TryFrom<ContentFunctionCall> for chat::turn::Turn {
    type Error = GeminiError;

    fn try_from(call: ContentFunctionCall) -> Result<Self, Self::Error> {
        let name = call
            .name
            .filter(|name| !name.is_empty())
            .ok_or(GeminiError::MissingFunctionCallName)?;
        Ok(chat::turn::Turn::ToolCall(chat::turn::FunctionCall {
            name,
            arguments: call.args.unwrap_or_default(),
        }))
    }
}

/// Accepted wire shapes for a Gemini function result.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ContentFunctionResponseValue {
    /// Plain-text function result.
    Text(
        /// Function-result text.
        #[schemars(title = "", description = "")]
        String,
    ),
    /// Structured JSON function result.
    Object(
        /// Function-result object.
        #[schemars(title = "", description = "")]
        chat::json::JsonObject,
    ),
}

/// Function result embedded in a Gemini content part.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ContentFunctionResponse {
    /// Value returned by the function.
    pub(super) response: Option<ContentFunctionResponseValue>,
}

/// Per-part controls used by the local speech extension.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ContentSpeechMetadata {
    /// Speaker label assigned to this text segment.
    pub(super) speaker: Option<String>,
    /// Optional synthesis style assigned to this segment.
    pub(super) style: Option<String>,
}

/// Inline bytes nested inside a Gemini content part.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ContentInlineData {
    /// Declared PNG or JPEG media type.
    pub(super) mime_type: Option<String>,
    /// Standard base64 payload.
    pub(super) data: Option<String>,
}

/// Provider-owned file nested inside a Gemini content part.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ContentFileData {
    /// Declared PNG or JPEG media type.
    pub(super) mime_type: Option<String>,
    /// Provider file URI.
    pub(super) file_uri: Option<String>,
}

/// One content part accepted by the Gemini adapter.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub(super) enum ContentPart {
    /// Plain text, optionally annotated for speech synthesis.
    Text {
        /// Text carried by the part.
        text: String,
        /// Local speaker and style annotations.
        #[serde(alias = "speech_metadata")]
        speech_metadata: Option<ContentSpeechMetadata>,
        /// Whether this model-authored text is a returned thought.
        thought: Option<bool>,
        /// Opaque signature returned with prior thought content.
        thought_signature: Option<String>,
    },
    /// Inline image bytes.
    InlineData {
        /// Typed base64 payload.
        inline_data: ContentInlineData,
    },
    /// Provider-owned image file.
    FileData {
        /// Typed provider file reference.
        file_data: ContentFileData,
    },
    /// Model request to invoke a function.
    FunctionCall {
        /// Function invocation payload.
        function_call: ContentFunctionCall,
    },
    /// Client-provided result from a prior function call.
    FunctionResponse {
        /// Function result payload.
        function_response: ContentFunctionResponse,
    },
    /// Any content part outside the supported subset.
    Unsupported {
        /// Unrecognized fields retained only to classify the part.
        #[serde(flatten)]
        _ignored: chat::json::JsonObject,
    },
}

/// Gemini message-like content record.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct Content {
    /// Author role, inferred as user when omitted where permitted.
    pub(super) role: Option<ContentRole>,
    /// Ordered content parts.
    pub(super) parts: Option<Vec<ContentPart>>,
}

// -----------------------------------------------------------------------------
// Tool: Defines offered functions and selection policy.
// -----------------------------------------------------------------------------

/// Gemini function declaration offered to the model.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ToolFunctionDeclaration {
    /// Function name exposed to the model.
    name: Option<String>,
    /// Optional human-readable function description.
    description: Option<String>,
    /// JSON Schema describing accepted parameters.
    parameters: Option<chat::json::JsonObject>,
}

impl ToolFunctionDeclaration {
    /// Count definition characters for deterministic token accounting.
    fn char_count(&self) -> usize {
        self.name.as_ref().map_or(0, String::len)
            + self.description.as_ref().map_or(0, String::len)
            + self
                .parameters
                .as_ref()
                .map_or(0, |parameters| parameters.serialized().chars().count())
    }
}

/// Gemini tool group containing function declarations.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ToolGroup {
    /// Functions exposed by this group.
    function_declarations: Option<Vec<ToolFunctionDeclaration>>,
}

/// List of Gemini tool groups offered with a request.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(transparent)]
pub(super) struct ToolGroupList(
    /// Tool groups in request order.
    Vec<ToolGroup>,
);

impl TryFrom<ToolGroupList> for Vec<chat::turn::FunctionTool> {
    type Error = GeminiError;

    fn try_from(groups: ToolGroupList) -> Result<Self, Self::Error> {
        let mut functions = Vec::new();
        for group in groups.0 {
            let declarations = group
                .function_declarations
                .ok_or(GeminiError::MissingFunctionDeclarations)?;
            for declaration in declarations {
                let definition_chars = declaration.char_count();
                let name = declaration
                    .name
                    .filter(|name| !name.is_empty())
                    .ok_or(GeminiError::MissingFunctionName)?;
                functions.push(chat::turn::FunctionTool::new(name, definition_chars));
            }
        }
        Ok(functions)
    }
}

/// Gemini function-calling policy mode.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum ToolFunctionCallingMode {
    /// Let the model decide whether to call a function.
    Auto,
    /// Let the model choose while validating calls against declarations.
    Validated,
    /// Prevent function calls.
    None,
    /// Require a function call.
    Any,
    /// Any mode outside the supported subset.
    #[serde(other)]
    Unsupported,
}

/// Gemini controls for function selection.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ToolFunctionCallingConfig {
    /// Requested selection mode.
    mode: Option<ToolFunctionCallingMode>,
    /// Function names eligible when calls are required.
    allowed_function_names: Option<Vec<String>>,
}

impl ToolFunctionCallingConfig {
    /// Lower Gemini's function-calling policy into the neutral tool choice.
    ///
    /// # Errors
    ///
    /// Returns [`GeminiError`] for an unsupported calling mode.
    fn lower(self, param: &'static str) -> Result<chat::turn::ToolChoice, GeminiError> {
        match self.mode.unwrap_or(ToolFunctionCallingMode::Auto) {
            ToolFunctionCallingMode::Auto | ToolFunctionCallingMode::Validated => {
                Ok(chat::turn::ToolChoice::Auto)
            }
            ToolFunctionCallingMode::None => Ok(chat::turn::ToolChoice::None),
            ToolFunctionCallingMode::Any => {
                let names = self.allowed_function_names.unwrap_or_default();
                Ok(match names.as_slice() {
                    [] => chat::turn::ToolChoice::Required,
                    [name] => chat::turn::ToolChoice::Named(name.clone()),
                    _ => chat::turn::ToolChoice::Allowed(names),
                })
            }
            ToolFunctionCallingMode::Unsupported => {
                Err(GeminiError::UnsupportedCallingMode { param })
            }
        }
    }
}

/// Top-level Gemini tool configuration.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ToolConfig {
    /// Function-selection controls.
    function_calling_config: Option<ToolFunctionCallingConfig>,
}

impl TryFrom<ToolConfig> for chat::turn::ToolChoice {
    type Error = GeminiError;

    fn try_from(config: ToolConfig) -> Result<Self, Self::Error> {
        match config.function_calling_config {
            Some(config) => config.lower("toolConfig"),
            None => Ok(Self::Auto),
        }
    }
}

// -----------------------------------------------------------------------------
// GenerateDelivery: Selects unary and streaming response transport.
// -----------------------------------------------------------------------------

/// Provider-neutral delivery contract selected by the route and query.
#[derive(Debug, Clone, Copy)]
pub(super) enum GenerateDelivery {
    /// One complete JSON response.
    Unary,
    /// A JSON array containing response records.
    JsonStream,
    /// Response records encoded as server-sent events.
    Sse,
}

impl GenerateDelivery {
    /// Return whether this delivery mode uses streaming response records.
    pub(super) const fn is_streaming(self) -> bool {
        matches!(self, Self::JsonStream | Self::Sse)
    }
}

// -----------------------------------------------------------------------------
// GenerateResponseModality: Defines generation output selection.
// -----------------------------------------------------------------------------

/// Response modality requested from Gemini generation.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum GenerateResponseModality {
    /// Text generation.
    Text,
    /// Audio generation through the local speech model.
    Audio,
    /// PNG generation through the local image model.
    Image,
    /// Any modality outside the supported subset.
    #[serde(other)]
    Unsupported,
}

// -----------------------------------------------------------------------------
// Speech: Defines voice selection and audio response contracts.
// -----------------------------------------------------------------------------

/// Gemini's nested prebuilt-voice selector.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct SpeechPrebuiltVoiceConfig {
    /// Prebuilt voice name.
    pub(super) voice_name: Option<String>,
}

/// Voice selector accepted by the local Gemini speech extension.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct SpeechVoiceConfig {
    /// Direct voice name accepted as a compatibility shorthand.
    pub(super) voice: Option<String>,
    /// Gemini-compatible nested voice selector.
    pub(super) prebuilt_voice_config: Option<SpeechPrebuiltVoiceConfig>,
}

/// Voice assigned to one named speaker.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct SpeechSpeakerVoiceConfig {
    /// Speaker label referenced by content parts.
    pub(super) speaker: Option<String>,
    /// Voice used for this speaker.
    pub(super) voice_config: Option<SpeechVoiceConfig>,
}

/// Gemini multi-speaker voice map.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct SpeechMultiSpeakerVoiceConfig {
    /// Per-speaker voice assignments.
    pub(super) speaker_voice_configs: Option<Vec<SpeechSpeakerVoiceConfig>>,
}

/// Audio encoding requested from Gemini speech generation.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum SpeechAudioFormat {
    /// RIFF/WAVE-wrapped signed PCM.
    #[default]
    AudioWav,
    /// Headerless big-endian signed PCM.
    AudioL16,
    /// Headerless G.711 mu-law.
    AudioMulaw,
    /// Headerless G.711 A-law.
    AudioAlaw,
    /// Any audio encoding outside the supported subset.
    #[serde(other)]
    Unsupported,
}

/// Voice configuration for Gemini audio generation.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct SpeechConfig {
    /// Single-speaker voice selector.
    pub(super) voice_config: Option<SpeechVoiceConfig>,
    /// Multi-speaker voice assignments.
    pub(super) multi_speaker_voice_config: Option<SpeechMultiSpeakerVoiceConfig>,
}

/// Encoded-audio properties requested by Gemini clients.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct SpeechAudioConfig {
    /// Requested audio encoding.
    pub(super) mime_type: Option<SpeechAudioFormat>,
    /// Requested output sample rate.
    pub(super) sample_rate: Option<u32>,
}

// -----------------------------------------------------------------------------
// Image: Defines deterministic image response controls.
// -----------------------------------------------------------------------------

/// Aspect ratio accepted by the bounded image fixture.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
pub(super) enum ImageAspectRatio {
    /// Square output and default.
    #[default]
    #[serde(rename = "1:1")]
    Square,
    /// Landscape output.
    #[serde(rename = "3:2")]
    Landscape,
    /// Portrait output.
    #[serde(rename = "2:3")]
    Portrait,
    /// Any ratio outside the fixed subset.
    #[serde(other)]
    Unsupported,
}

/// Image-size class accepted by the bounded image fixture.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
pub(super) enum ImageSize {
    /// Gemini's default one-kilopixel size class.
    #[default]
    #[serde(rename = "1K")]
    OneK,
    /// Any size class outside the fixed subset.
    #[serde(other)]
    Unsupported,
}

/// Gemini image-generation format properties.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ImageResponseFormat {
    /// Fixed output aspect ratio.
    pub(super) aspect_ratio: Option<ImageAspectRatio>,
    /// Fixed one-kilopixel size class.
    pub(super) image_size: Option<ImageSize>,
}

// -----------------------------------------------------------------------------
// TextResponseMimeType: Defines current structured-text MIME values.
// -----------------------------------------------------------------------------

/// MIME type accepted by Gemini's current text response-format control.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum TextResponseMimeType {
    /// Deterministic JSON text.
    ApplicationJson,
    /// Ordinary ELIZA text.
    TextPlain,
    /// Any future or unknown MIME type.
    #[serde(other)]
    Unsupported,
}

// -----------------------------------------------------------------------------
// LegacyTextResponseMimeType: Defines legacy structured-text MIME values.
// -----------------------------------------------------------------------------

/// MIME type accepted by Gemini's legacy text response-format control.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
pub(super) enum LegacyTextResponseMimeType {
    /// Deterministic JSON text.
    #[serde(rename = "application/json")]
    ApplicationJson,
    /// Ordinary ELIZA text.
    #[serde(rename = "text/plain")]
    TextPlain,
    /// Any future or unknown MIME type.
    #[serde(other)]
    Unsupported,
}

// -----------------------------------------------------------------------------
// Generate: Defines unary and streaming response contracts.
// -----------------------------------------------------------------------------

/// Envelope used for streaming Gemini responses.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum GenerateStreamFormat {
    /// Server-sent events.
    Sse,
    /// Incrementally delivered JSON array.
    #[serde(other)]
    Json,
}

/// Query parameters accepted by the streaming generation action.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct GenerateQuery {
    /// Requested streaming envelope.
    pub(super) alt: Option<GenerateStreamFormat>,
}

/// Reason a Gemini candidate stopped generating.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum GenerateFinishReason {
    /// Generation completed normally.
    Stop,
}

/// Function call emitted by a Gemini candidate.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct GenerateFunctionCall {
    /// Provider-shaped call identifier.
    pub(super) id: String,
    /// Requested function name.
    pub(super) name: String,
    /// Arguments supplied to the function.
    pub(super) args: chat::json::JsonObject,
}

/// Base64-encoded binary data emitted by Gemini generation.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GenerateInlineData {
    /// Media type describing the encoded bytes.
    pub(super) mime_type: String,
    /// Base64-encoded payload.
    pub(super) data: String,
}

/// One output part emitted by a Gemini candidate.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub(super) enum GenerateOutputPart {
    /// Generated text.
    Text {
        /// Text carried by the part.
        text: String,
    },
    /// Generated reasoning summary.
    Thought {
        /// Mechanical reasoning trace.
        text: String,
        /// Marks this part as thought content.
        thought: bool,
        /// Stable fixture signature over the complete trace.
        thought_signature: String,
    },
    /// Requested function invocation.
    FunctionCall {
        /// Function-call payload.
        function_call: GenerateFunctionCall,
    },
    /// Generated binary data.
    InlineData {
        /// Encoded data payload.
        inline_data: GenerateInlineData,
    },
}

/// Model-authored content emitted by a Gemini candidate.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct GenerateOutputContent {
    /// Fixed model role.
    pub(super) role: &'static str,
    /// Generated content parts.
    pub(super) parts: Vec<GenerateOutputPart>,
}

/// One Gemini generation candidate.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GenerateCandidate {
    /// Generated candidate content.
    pub(super) content: GenerateOutputContent,
    /// Completion reason, omitted while streaming is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) finish_reason: Option<GenerateFinishReason>,
    /// Zero-based candidate position.
    pub(super) index: usize,
}

/// Token counts serialized by Gemini generation APIs.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_field_names,
    reason = "field names must match Gemini usage metadata"
)]
pub(super) struct GenerateUsage {
    /// Estimated tokens consumed by request input.
    pub(super) prompt_token_count: usize,
    /// Estimated tokens emitted by candidates.
    pub(super) candidates_token_count: usize,
    /// Estimated tokens emitted as readable thoughts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) thoughts_token_count: Option<usize>,
    /// Combined input and candidate token count.
    pub(super) total_token_count: usize,
}

impl From<chat::turn::Usage> for GenerateUsage {
    fn from(usage: chat::turn::Usage) -> Self {
        Self {
            prompt_token_count: usage.prompt,
            candidates_token_count: usage.completion,
            thoughts_token_count: (usage.reasoning > 0).then_some(usage.reasoning),
            total_token_count: usage.total,
        }
    }
}

/// Complete or incremental Gemini generation response.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GenerateContentResponse {
    /// Generated candidates.
    pub(super) candidates: Vec<GenerateCandidate>,
    /// Model that produced the response.
    pub(super) model_version: ModelId,
    /// Token usage, omitted from non-terminal stream chunks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) usage_metadata: Option<GenerateUsage>,
}

impl GenerateContentResponse {
    /// Attach the provider-owned model identity to one canonical result.
    pub(super) fn from_compat(model: ModelId, response: chat::turn::Response) -> Self {
        let part = match response.output {
            chat::turn::Output::Text(text) => GenerateOutputPart::Text { text },
            chat::turn::Output::ToolCall(call) => GenerateOutputPart::FunctionCall {
                function_call: GenerateFunctionCall {
                    id: format!("call_{}", Uuid::now_v7().simple()),
                    name: call.name,
                    args: call.arguments,
                },
            },
        };
        let mut parts = response.reasoning.map_or_else(Vec::new, |reasoning| {
            vec![GenerateOutputPart::Thought {
                text: reasoning.text,
                thought: true,
                thought_signature: reasoning.signature,
            }]
        });
        parts.push(part);
        Self {
            candidates: vec![GenerateCandidate {
                content: GenerateOutputContent {
                    role: "model",
                    parts,
                },
                finish_reason: Some(GenerateFinishReason::Stop),
                index: 0,
            }],
            model_version: model,
            usage_metadata: Some(response.usage.into()),
        }
    }
}

// -----------------------------------------------------------------------------
// TextResponseFormat: Compiles the current structured-text controls.
// -----------------------------------------------------------------------------

/// Provider path for Gemini's current text-format control.
const CURRENT_TEXT_FORMAT_PARAM: &str = "generationConfig.responseFormat.text";

/// Provider path for Gemini's legacy text-format control.
const LEGACY_TEXT_FORMAT_PARAM: &str = "generationConfig.responseMimeType";

/// Map shared compiler categories to stable Gemini diagnostics.
///
/// # Errors
///
/// Returns the provider-specific form of a shared schema compiler error.
fn compile_text_schema(
    schema: chat::json::JsonObject,
    param: &'static str,
) -> Result<chat::structured_output::StructuredOutput, GeminiError> {
    chat::structured_output::StructuredOutput::try_from(schema).map_err(|source| {
        match source.kind() {
            chat::structured_output::StructuredOutputErrorKind::Invalid => {
                GeminiError::InvalidResponseSchema { param, source }
            }
            chat::structured_output::StructuredOutputErrorKind::Unsupported => {
                GeminiError::UnsupportedResponseSchema { param, source }
            }
        }
    })
}

/// Current Gemini text response-format properties.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct TextResponseFormat {
    /// Requested response MIME type.
    mime_type: Option<TextResponseMimeType>,
    /// Optional JSON Schema for `APPLICATION_JSON` output.
    schema: Option<chat::json::JsonObject>,
}

impl TextResponseFormat {
    /// Compile Gemini's current `responseFormat.text` control.
    ///
    /// # Errors
    ///
    /// Returns a typed Gemini error for an unknown MIME type or invalid schema.
    fn compile(self) -> Result<chat::structured_output::StructuredOutput, GeminiError> {
        match (self.mime_type, self.schema) {
            (None | Some(TextResponseMimeType::TextPlain), None) => {
                Ok(chat::structured_output::StructuredOutput::default())
            }
            (Some(TextResponseMimeType::ApplicationJson), None) => {
                Ok(chat::structured_output::StructuredOutput::json_object())
            }
            (Some(TextResponseMimeType::ApplicationJson), Some(schema)) => {
                compile_text_schema(schema, CURRENT_TEXT_FORMAT_PARAM)
            }
            (None, Some(_)) => Err(GeminiError::InvalidResponseFormat {
                param: CURRENT_TEXT_FORMAT_PARAM,
                reason: "mimeType is required when schema is present",
            }),
            (Some(TextResponseMimeType::TextPlain), Some(_)) => {
                Err(GeminiError::InvalidResponseFormat {
                    param: CURRENT_TEXT_FORMAT_PARAM,
                    reason: "schema requires APPLICATION_JSON",
                })
            }
            (Some(TextResponseMimeType::Unsupported), _) => {
                Err(GeminiError::UnsupportedResponseFormat {
                    param: CURRENT_TEXT_FORMAT_PARAM,
                })
            }
        }
    }
}

// -----------------------------------------------------------------------------
// ResponseFormat: Combines current modality response controls.
// -----------------------------------------------------------------------------

/// Gemini response-format envelope for generated text, audio, or images.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ResponseFormat {
    /// Text-specific response properties.
    pub(super) text: Option<TextResponseFormat>,
    /// Audio-specific response properties.
    pub(super) audio: Option<SpeechAudioConfig>,
    /// Image-specific response properties.
    pub(super) image: Option<ImageResponseFormat>,
}

// -----------------------------------------------------------------------------
// Thinking: Defines native Gemini reasoning controls.
// -----------------------------------------------------------------------------

/// Named Gemini thinking level.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum ThinkingLevel {
    /// Provider-selected level.
    Unspecified,
    /// Smallest supported thinking level.
    Minimal,
    /// Low thinking level.
    Low,
    /// Medium thinking level.
    Medium,
    /// High thinking level.
    High,
    /// Any level outside the current contract.
    #[serde(other)]
    Unsupported,
}

/// Gemini thought visibility and budget controls.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ThinkingConfig {
    /// Whether returned thought summaries are readable.
    include_thoughts: Option<bool>,
    /// Dynamic (`-1`), disabled (`0`), or positive thinking budget.
    thinking_budget: Option<i32>,
    /// Named thinking level used instead of a numeric budget.
    thinking_level: Option<ThinkingLevel>,
}

impl ThinkingConfig {
    /// Validate syntax and report whether thoughts are readable.
    ///
    /// # Errors
    ///
    /// Returns a typed error for conflicting or invalid controls.
    fn should_include_reasoning(&self) -> Result<bool, GeminiError> {
        if self.thinking_budget.is_some() && self.thinking_level.is_some() {
            Err(GeminiError::ConflictingThinkingControls)
        } else if self.thinking_budget.is_some_and(|budget| budget < -1) {
            Err(GeminiError::InvalidThinkingBudget)
        } else if matches!(self.thinking_level, Some(ThinkingLevel::Unsupported)) {
            Err(GeminiError::UnsupportedThinkingLevel)
        } else {
            Ok(self.include_thoughts.unwrap_or(false))
        }
    }
}

// -----------------------------------------------------------------------------
// GenerateConfig: Compiles request-wide generation controls.
// -----------------------------------------------------------------------------

/// Gemini generation controls used by text, speech, and image requests.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GenerateConfig {
    /// Requested response modalities.
    pub(super) response_modalities: Option<Vec<GenerateResponseModality>>,
    /// Requested response wire format.
    pub(super) response_format: Option<ResponseFormat>,
    /// Legacy text response MIME type.
    response_mime_type: Option<LegacyTextResponseMimeType>,
    /// Legacy JSON Schema paired with `responseMimeType`.
    response_json_schema: Option<chat::json::JsonObject>,
    /// Voice and speaker controls for audio output.
    pub(super) speech_config: Option<SpeechConfig>,
    /// Native thought visibility and budget controls.
    thinking_config: Option<ThinkingConfig>,
    /// Maximum generated tokens accepted as a compatibility control.
    max_output_tokens: Option<usize>,
}

impl GenerateConfig {
    /// Validate generation limits and return thought visibility.
    ///
    /// # Errors
    ///
    /// Returns a typed error for zero output limits or invalid thought controls.
    pub(super) fn should_include_reasoning(&self) -> Result<bool, GeminiError> {
        if self.max_output_tokens == Some(0) {
            Err(GeminiError::InvalidMaxOutputTokens)
        } else if let Some(config) = &self.thinking_config {
            config.should_include_reasoning()
        } else {
            Ok(false)
        }
    }

    /// Report whether an audio request contains any text-format controls.
    pub(super) fn has_text_format_controls(&self) -> bool {
        self.response_format
            .as_ref()
            .is_some_and(|format| format.text.is_some())
            || self.response_mime_type.is_some()
            || self.response_json_schema.is_some()
    }

    /// Report whether an image request contains text-format controls.
    pub(super) fn has_legacy_text_format_controls(&self) -> bool {
        self.response_mime_type.is_some() || self.response_json_schema.is_some()
    }
}

/// Lower Gemini's legacy MIME-type and JSON-Schema controls.
///
/// # Errors
///
/// Returns a typed Gemini error for incompatible legacy values or schemas.
fn lower_legacy_text_format(
    mime_type: Option<LegacyTextResponseMimeType>,
    schema: Option<chat::json::JsonObject>,
) -> Result<chat::structured_output::StructuredOutput, GeminiError> {
    match (mime_type, schema) {
        (None | Some(LegacyTextResponseMimeType::TextPlain), None) => {
            Ok(chat::structured_output::StructuredOutput::default())
        }
        (Some(LegacyTextResponseMimeType::ApplicationJson), None) => {
            Ok(chat::structured_output::StructuredOutput::json_object())
        }
        (Some(LegacyTextResponseMimeType::ApplicationJson), Some(schema)) => {
            compile_text_schema(schema, "generationConfig.responseJsonSchema")
        }
        (None, Some(_)) => Err(GeminiError::InvalidResponseFormat {
            param: "generationConfig.responseJsonSchema",
            reason: "responseMimeType is required when responseJsonSchema is present",
        }),
        (Some(LegacyTextResponseMimeType::TextPlain), Some(_)) => {
            Err(GeminiError::InvalidResponseFormat {
                param: "generationConfig.responseJsonSchema",
                reason: "responseJsonSchema requires application/json",
            })
        }
        (Some(LegacyTextResponseMimeType::Unsupported), _) => {
            Err(GeminiError::UnsupportedResponseFormat {
                param: LEGACY_TEXT_FORMAT_PARAM,
            })
        }
    }
}

impl TryFrom<GenerateConfig> for chat::structured_output::StructuredOutput {
    type Error = GeminiError;

    fn try_from(config: GenerateConfig) -> Result<Self, Self::Error> {
        config.should_include_reasoning()?;

        // Image formatting belongs exclusively to the image-generation path.
        if config
            .response_format
            .as_ref()
            .is_some_and(|format| format.image.is_some())
        {
            return Err(GeminiError::ImageFormatForText);
        }
        let has_current = config.response_format.is_some();
        let has_legacy =
            config.response_mime_type.is_some() || config.response_json_schema.is_some();

        // Current and deprecated controls describe one mutually exclusive format.
        if has_current && has_legacy {
            return Err(GeminiError::ConflictingResponseFormats);
        }

        // A current text block owns formatting whenever it is present.
        if let Some(text) = config.response_format.and_then(|format| format.text) {
            return text.compile();
        }
        lower_legacy_text_format(config.response_mime_type, config.response_json_schema)
    }
}

// -----------------------------------------------------------------------------
// GenerateContentRequest: Defines the native generation request envelope.
// -----------------------------------------------------------------------------

/// Gemini generate-content request accepted by the adapter.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GenerateContentRequest {
    /// Conversation content in request order.
    pub(super) contents: Option<Vec<Content>>,
    /// Optional system instruction.
    pub(super) system_instruction: Option<Content>,
    /// Functions offered to the model.
    pub(super) tools: Option<ToolGroupList>,
    /// Function-selection policy.
    pub(super) tool_config: Option<ToolConfig>,
    /// Text or speech generation controls.
    pub(super) generation_config: Option<GenerateConfig>,
}

// -----------------------------------------------------------------------------
// GeminiModel: Defines the Gemini model catalog.
// -----------------------------------------------------------------------------

/// Stable identity fields for one Gemini model.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiModelIdentity {
    /// Resource name used by Gemini clients.
    pub(super) name: String,
    /// Provider-compatible model version.
    pub(super) version: &'static str,
    /// Human-readable model label.
    pub(super) display_name: &'static str,
    /// Human-readable model description.
    pub(super) description: &'static str,
}

/// Generation limits and actions supported by one Gemini model.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiModelCapabilities {
    /// Generation action names accepted for this model.
    pub(super) supported_generation_methods: Vec<&'static str>,
    /// Maximum accepted input tokens.
    pub(super) input_token_limit: usize,
    /// Maximum emitted output tokens.
    pub(super) output_token_limit: usize,
}

/// Gemini model-catalog entry.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiModel {
    /// Model identity flattened into the wire object.
    #[serde(flatten)]
    pub(super) identity: GeminiModelIdentity,
    /// Model capabilities flattened into the wire object.
    #[serde(flatten)]
    pub(super) capabilities: GeminiModelCapabilities,
}

/// Gemini model-list response.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct GeminiModelListResponse {
    /// Available models.
    pub(super) models: Vec<GeminiModel>,
}
