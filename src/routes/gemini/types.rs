//! Gemini wire contracts shared by its route adapters.
#![expect(
    rlib::undocumented_items,
    reason = "this module contains only private Serde wire declarations whose field names are the provider contract"
)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::errors::GeminiError;
use crate::types::json::JsonObject;
use crate::types::model::ModelId;
use crate::types::turn::{
    CompatOutput, CompatTurn, CompatTurnResponse, FunctionCall, FunctionTool, TokenUsage,
    ToolChoice as CompatToolChoice,
};

// -----------------------------------------------------------------------------
// Content: Defines inbound conversation content and function records.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum ContentRole {
    User,
    Model,
    Function,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ContentFunctionCall {
    name: Option<String>,
    args: Option<JsonObject>,
}

impl TryFrom<ContentFunctionCall> for CompatTurn {
    type Error = GeminiError;

    fn try_from(function_call: ContentFunctionCall) -> Result<Self, Self::Error> {
        let name = function_call
            .name
            .filter(|name| !name.is_empty())
            .ok_or(GeminiError::MissingFunctionCallName)?;
        Ok(Self::ToolCall(FunctionCall {
            name,
            arguments: function_call.args.unwrap_or_default(),
        }))
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ContentFunctionResponseValue {
    Text(String),
    Object(JsonObject),
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ContentFunctionResponse {
    pub(super) response: Option<ContentFunctionResponseValue>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ContentPart {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: ContentFunctionCall,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: ContentFunctionResponse,
    },
    Unsupported {
        #[serde(flatten)]
        _ignored: JsonObject,
    },
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct Content {
    pub(super) role: Option<ContentRole>,
    pub(super) parts: Option<Vec<ContentPart>>,
}

// -----------------------------------------------------------------------------
// Tool: Defines offered functions and selection policy.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ToolFunctionDeclaration {
    name: Option<String>,
    description: Option<String>,
    parameters: Option<JsonObject>,
}

impl ToolFunctionDeclaration {
    fn char_count(&self) -> usize {
        self.name.as_ref().map_or(0, String::len)
            + self.description.as_ref().map_or(0, String::len)
            + self
                .parameters
                .as_ref()
                .map_or(0, |parameters| parameters.serialized().chars().count())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ToolGroup {
    function_declarations: Option<Vec<ToolFunctionDeclaration>>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(transparent)]
pub(super) struct ToolGroupList(Vec<ToolGroup>);

impl ToolGroupList {
    /// Lower provider tool groups into the neutral function contract.
    ///
    /// # Errors
    ///
    /// Returns [`GeminiError`] when a group has no function declarations or a
    /// declaration has no name.
    pub(super) fn into_domain(self) -> Result<Vec<FunctionTool>, GeminiError> {
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

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum ToolFunctionCallingMode {
    Auto,
    Validated,
    None,
    Any,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ToolFunctionCallingConfig {
    mode: Option<ToolFunctionCallingMode>,
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ToolConfig {
    function_calling_config: Option<ToolFunctionCallingConfig>,
}

impl ToolConfig {
    /// Lower optional Gemini tool configuration into a neutral tool choice.
    ///
    /// # Errors
    ///
    /// Returns [`GeminiError`] when the configured calling mode is unsupported.
    pub(super) fn into_domain(
        config: Option<Self>,
        param: &'static str,
    ) -> Result<CompatToolChoice, GeminiError> {
        match config.and_then(|config| config.function_calling_config) {
            Some(config) => config.lower(param),
            None => Ok(CompatToolChoice::Auto),
        }
    }
}

// -----------------------------------------------------------------------------
// Generate: Defines unary and streaming generation contracts.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GenerateContentRequest {
    pub(super) contents: Option<Vec<Content>>,
    pub(super) system_instruction: Option<Content>,
    pub(super) tools: Option<ToolGroupList>,
    pub(super) tool_config: Option<ToolConfig>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum GenerateStreamFormat {
    Sse,
    #[serde(other)]
    Json,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct GenerateQuery {
    pub(super) alt: Option<GenerateStreamFormat>,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum GenerateFinishReason {
    Stop,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct GenerateFunctionCall {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) args: JsonObject,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum GenerateOutputPart {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GenerateFunctionCall,
    },
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct GenerateOutputContent {
    pub(super) role: &'static str,
    pub(super) parts: Vec<GenerateOutputPart>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GenerateCandidate {
    pub(super) content: GenerateOutputContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) finish_reason: Option<GenerateFinishReason>,
    pub(super) index: usize,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_field_names,
    reason = "field names must match Gemini usage metadata"
)]
pub(super) struct GenerateUsage {
    prompt_token_count: usize,
    candidates_token_count: usize,
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

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GenerateContentResponse {
    pub(super) candidates: Vec<GenerateCandidate>,
    pub(super) model_version: ModelId,
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

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiModelIdentity {
    pub(super) name: String,
    pub(super) version: &'static str,
    pub(super) display_name: &'static str,
    pub(super) description: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiModelCapabilities {
    pub(super) supported_generation_methods: Vec<&'static str>,
    pub(super) input_token_limit: usize,
    pub(super) output_token_limit: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiModel {
    #[serde(flatten)]
    pub(super) identity: GeminiModelIdentity,
    #[serde(flatten)]
    pub(super) capabilities: GeminiModelCapabilities,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct GeminiModelListResponse {
    pub(super) models: Vec<GeminiModel>,
}
