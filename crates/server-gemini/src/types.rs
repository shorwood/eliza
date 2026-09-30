//! Gemini wire contracts shared by its route adapters.
//! Empty metadata on documented newtype fields avoids a `schemars` 0.9 derive collision.
use eliza_http::lower::Lower;
use eliza_http::model::ModelId;
use eliza_modality_chat::json::JsonObject;
use eliza_modality_chat::structured_output::{StructuredOutput, StructuredOutputErrorKind};
use eliza_modality_chat::turn::{
    CompatOutput, CompatTurn, CompatTurnResponse, FunctionCall, FunctionTool, TokenUsage,
    ToolChoice as CompatToolChoice,
};
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
    args: Option<JsonObject>,
}

impl Lower for ContentFunctionCall {
    type Canonical = CompatTurn;

    type Error = GeminiError;

    fn lower(self) -> Result<Self::Canonical, Self::Error> {
        let name = self
            .name
            .filter(|name| !name.is_empty())
            .ok_or(GeminiError::MissingFunctionCallName)?;
        Ok(CompatTurn::ToolCall(FunctionCall {
            name,
            arguments: self.args.unwrap_or_default(),
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
        JsonObject,
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
        _ignored: JsonObject,
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
    parameters: Option<JsonObject>,
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

impl Lower for ToolGroupList {
    type Canonical = Vec<FunctionTool>;

    type Error = GeminiError;

    fn lower(self) -> Result<Self::Canonical, Self::Error> {
        let mut functions = Vec::new();
        for group in self.0 {
            let declarations = group
                .function_declarations
                .ok_or(GeminiError::MissingFunctionDeclarations)?;
            for declaration in declarations {
                let definition_chars = declaration.char_count();
                let name = declaration
                    .name
                    .filter(|name| !name.is_empty())
                    .ok_or(GeminiError::MissingFunctionName)?;
                functions.push(FunctionTool::new(name, definition_chars));
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
    fn lower(self, param: &'static str) -> Result<CompatToolChoice, GeminiError> {
        match self.mode.unwrap_or(ToolFunctionCallingMode::Auto) {
            ToolFunctionCallingMode::Auto | ToolFunctionCallingMode::Validated => {
                Ok(CompatToolChoice::Auto)
            }
            ToolFunctionCallingMode::None => Ok(CompatToolChoice::None),
            ToolFunctionCallingMode::Any => {
                let names = self.allowed_function_names.unwrap_or_default();
                Ok(match names.as_slice() {
                    [] => CompatToolChoice::Required,
                    [name] => CompatToolChoice::Named(name.clone()),
                    _ => CompatToolChoice::Allowed(names),
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

impl Lower for ToolConfig {
    type Canonical = CompatToolChoice;

    type Error = GeminiError;

    fn lower(self) -> Result<Self::Canonical, Self::Error> {
        match self.function_calling_config {
            Some(config) => config.lower("toolConfig"),
            None => Ok(CompatToolChoice::Auto),
        }
    }
}

// -----------------------------------------------------------------------------
// Speech: Defines voice selection and audio response contracts.
// -----------------------------------------------------------------------------

/// Response modality requested from Gemini generation.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum SpeechResponseModality {
    /// Text generation.
    Text,
    /// Audio generation through the local speech model.
    Audio,
    /// Any modality outside the supported subset.
    #[serde(other)]
    Unsupported,
}

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
    pub(super) args: JsonObject,
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
    prompt_token_count: usize,
    /// Estimated tokens emitted by candidates.
    candidates_token_count: usize,
    /// Combined input and candidate token count.
    total_token_count: usize,
}

impl From<TokenUsage> for GenerateUsage {
    fn from(usage: TokenUsage) -> Self {
        Self {
            prompt_token_count: usage.prompt,
            candidates_token_count: usage.completion,
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
    pub(super) fn from_compat(model: ModelId, response: CompatTurnResponse) -> Self {
        let part = match response.output {
            CompatOutput::Text(text) => GenerateOutputPart::Text { text },
            CompatOutput::ToolCall(call) => GenerateOutputPart::FunctionCall {
                function_call: GenerateFunctionCall {
                    id: format!("call_{}", Uuid::now_v7().simple()),
                    name: call.name,
                    args: call.arguments,
                },
            },
        };
        Self {
            candidates: vec![GenerateCandidate {
                content: GenerateOutputContent {
                    role: "model",
                    parts: vec![part],
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
    schema: JsonObject,
    param: &'static str,
) -> Result<StructuredOutput, GeminiError> {
    StructuredOutput::try_from(schema).map_err(|source| match source.kind() {
        StructuredOutputErrorKind::Invalid => GeminiError::InvalidResponseSchema { param, source },
        StructuredOutputErrorKind::Unsupported => {
            GeminiError::UnsupportedResponseSchema { param, source }
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
    schema: Option<JsonObject>,
}

impl TextResponseFormat {
    /// Compile Gemini's current `responseFormat.text` control.
    ///
    /// # Errors
    ///
    /// Returns a typed Gemini error for an unknown MIME type or invalid schema.
    fn compile(self) -> Result<StructuredOutput, GeminiError> {
        match (self.mime_type, self.schema) {
            (None | Some(TextResponseMimeType::TextPlain), None) => Ok(StructuredOutput::default()),
            (Some(TextResponseMimeType::ApplicationJson), None) => {
                Ok(StructuredOutput::json_object())
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
// ResponseFormat: Combines current text and audio response controls.
// -----------------------------------------------------------------------------

/// Gemini response-format envelope for generated text or audio.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ResponseFormat {
    /// Text-specific response properties.
    text: Option<TextResponseFormat>,
    /// Audio-specific response properties.
    pub(super) audio: Option<SpeechAudioConfig>,
}

// -----------------------------------------------------------------------------
// GenerateConfig: Compiles request-wide generation controls.
// -----------------------------------------------------------------------------

/// Lower Gemini's legacy MIME-type and JSON-Schema controls.
///
/// # Errors
///
/// Returns a typed Gemini error for incompatible legacy values or schemas.
fn lower_legacy_text_format(
    mime_type: Option<LegacyTextResponseMimeType>,
    schema: Option<JsonObject>,
) -> Result<StructuredOutput, GeminiError> {
    match (mime_type, schema) {
        (None | Some(LegacyTextResponseMimeType::TextPlain), None) => {
            Ok(StructuredOutput::default())
        }
        (Some(LegacyTextResponseMimeType::ApplicationJson), None) => {
            Ok(StructuredOutput::json_object())
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

/// Gemini generation controls used by text and speech requests.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GenerateConfig {
    /// Requested response modalities.
    pub(super) response_modalities: Option<Vec<SpeechResponseModality>>,
    /// Requested response wire format.
    pub(super) response_format: Option<ResponseFormat>,
    /// Legacy text response MIME type.
    response_mime_type: Option<LegacyTextResponseMimeType>,
    /// Legacy JSON Schema paired with `responseMimeType`.
    response_json_schema: Option<JsonObject>,
    /// Voice and speaker controls for audio output.
    pub(super) speech_config: Option<SpeechConfig>,
}

impl GenerateConfig {
    /// Report whether an audio request contains any text-format controls.
    pub(super) fn has_text_format_controls(&self) -> bool {
        self.response_format
            .as_ref()
            .is_some_and(|format| format.text.is_some())
            || self.response_mime_type.is_some()
            || self.response_json_schema.is_some()
    }
}

impl Lower for GenerateConfig {
    type Canonical = StructuredOutput;

    type Error = GeminiError;

    fn lower(self) -> Result<Self::Canonical, Self::Error> {
        let has_current = self.response_format.is_some();
        let has_legacy = self.response_mime_type.is_some() || self.response_json_schema.is_some();

        // Current and deprecated controls describe one mutually exclusive format.
        if has_current && has_legacy {
            return Err(GeminiError::ConflictingResponseFormats);
        }

        // A current text block owns formatting whenever it is present.
        if let Some(text) = self.response_format.and_then(|format| format.text) {
            return text.compile();
        }
        lower_legacy_text_format(self.response_mime_type, self.response_json_schema)
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
