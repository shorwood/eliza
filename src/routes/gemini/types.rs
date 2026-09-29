//! Gemini wire contracts shared by its route adapters.
//! Empty metadata on documented newtype fields avoids a `schemars` 0.9 derive collision.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::errors::GeminiError;
use crate::types::json::JsonObject;
use crate::types::lower::Lower;
use crate::types::model::ModelId;
use crate::types::turn::{
    CompatOutput, CompatTurn, CompatTurnResponse, FunctionCall, FunctionTool, TokenUsage,
    ToolChoice as CompatToolChoice,
};

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

/// Gemini response-format envelope for generated audio.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct SpeechResponseFormat {
    /// Audio-specific response properties.
    pub(super) audio: Option<SpeechAudioConfig>,
}

// -----------------------------------------------------------------------------
// Generate: Defines unary and streaming generation contracts.
// -----------------------------------------------------------------------------

/// Gemini generation controls used by text and speech requests.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GenerateConfig {
    /// Requested response modalities.
    pub(super) response_modalities: Option<Vec<SpeechResponseModality>>,
    /// Requested response wire format.
    pub(super) response_format: Option<SpeechResponseFormat>,
    /// Voice and speaker controls for audio output.
    pub(super) speech_config: Option<SpeechConfig>,
}

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

impl From<CompatTurnResponse> for GenerateContentResponse {
    fn from(response: CompatTurnResponse) -> Self {
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
            model_version: response.model,
            usage_metadata: Some(response.usage.into()),
        }
    }
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
