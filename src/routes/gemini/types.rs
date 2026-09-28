//! Gemini wire contracts shared by its route adapters.
#![expect(
    clippy::missing_errors_doc,
    rlib::missing_section_dividers,
    rlib::undocumented_items,
    reason = "private Serde fields mirror Gemini's published wire names"
)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::errors::GeminiError;
use crate::types::json::JsonObject;
use crate::types::model::ModelId;
use crate::types::turn::{CompatOutput, CompatTurnResponse, FunctionTool, TokenUsage, ToolChoice};

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
#[serde(untagged)]
pub(super) enum Part {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: FunctionCallInput,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: FunctionResponseInput,
    },
    Unsupported {
        #[serde(flatten)]
        _ignored: JsonObject,
    },
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct FunctionCallInput {
    pub(super) name: Option<String>,
    pub(super) args: Option<JsonObject>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct FunctionResponseInput {
    pub(super) response: Option<FunctionResponseValue>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum FunctionResponseValue {
    Text(String),
    Object(JsonObject),
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct GeminiContent {
    pub(super) role: Option<ContentRole>,
    pub(super) parts: Option<Vec<Part>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ToolGroup {
    function_declarations: Option<Vec<FunctionDeclaration>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct FunctionDeclaration {
    name: Option<String>,
    description: Option<String>,
    parameters: Option<JsonObject>,
}

impl FunctionDeclaration {
    fn char_count(&self) -> usize {
        self.name.as_ref().map_or(0, String::len)
            + self.description.as_ref().map_or(0, String::len)
            + self
                .parameters
                .as_ref()
                .map_or(0, |parameters| parameters.serialized().chars().count())
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(transparent)]
pub(super) struct ToolGroupList(Vec<ToolGroup>);

impl ToolGroupList {
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
pub(super) enum FunctionCallingMode {
    Auto,
    Validated,
    None,
    Any,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct FunctionCallingConfig {
    mode: Option<FunctionCallingMode>,
    allowed_function_names: Option<Vec<String>>,
}

impl FunctionCallingConfig {
    fn lower(self, param: &'static str) -> Result<ToolChoice, GeminiError> {
        match self.mode.unwrap_or(FunctionCallingMode::Auto) {
            FunctionCallingMode::Auto | FunctionCallingMode::Validated => Ok(ToolChoice::Auto),
            FunctionCallingMode::None => Ok(ToolChoice::None),
            FunctionCallingMode::Any => {
                let names = self.allowed_function_names.unwrap_or_default();
                Ok(match names.as_slice() {
                    [] => ToolChoice::Required,
                    [name] => ToolChoice::Named(name.clone()),
                    _ => ToolChoice::Allowed(names),
                })
            }
            FunctionCallingMode::Unsupported => Err(GeminiError::UnsupportedCallingMode { param }),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ToolConfig {
    function_calling_config: Option<FunctionCallingConfig>,
}

impl ToolConfig {
    pub(super) fn into_domain(
        config: Option<Self>,
        param: &'static str,
    ) -> Result<ToolChoice, GeminiError> {
        match config.and_then(|config| config.function_calling_config) {
            Some(config) => config.lower(param),
            None => Ok(ToolChoice::Auto),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GenerateContentRequest {
    pub(super) contents: Option<Vec<GeminiContent>>,
    pub(super) system_instruction: Option<GeminiContent>,
    pub(super) tools: Option<ToolGroupList>,
    pub(super) tool_config: Option<ToolConfig>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum StreamFormat {
    Sse,
    #[serde(other)]
    Json,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct GenerateQuery {
    pub(super) alt: Option<StreamFormat>,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum FinishReason {
    Stop,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum OutputPart {
    Text {
        text: String,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: FunctionCallOutput,
    },
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct FunctionCallOutput {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) args: JsonObject,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct OutputContent {
    pub(super) role: &'static str,
    pub(super) parts: Vec<OutputPart>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct Candidate {
    pub(super) content: OutputContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) finish_reason: Option<FinishReason>,
    pub(super) index: usize,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[expect(
    clippy::struct_field_names,
    reason = "field names must match Gemini usage metadata"
)]
pub(super) struct UsageMetadata {
    prompt_token_count: usize,
    candidates_token_count: usize,
    total_token_count: usize,
}

impl From<TokenUsage> for UsageMetadata {
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
    pub(super) candidates: Vec<Candidate>,
    pub(super) model_version: ModelId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) usage_metadata: Option<UsageMetadata>,
}

impl From<CompatTurnResponse> for GenerateContentResponse {
    fn from(response: CompatTurnResponse) -> Self {
        let part = match response.output {
            CompatOutput::Text(text) => OutputPart::Text { text },
            CompatOutput::ToolCall(call) => OutputPart::FunctionCall {
                function_call: FunctionCallOutput {
                    id: format!("call_{}", Uuid::now_v7().simple()),
                    name: call.name,
                    args: call.arguments,
                },
            },
        };
        Self {
            candidates: vec![Candidate {
                content: OutputContent {
                    role: "model",
                    parts: vec![part],
                },
                finish_reason: Some(FinishReason::Stop),
                index: 0,
            }],
            model_version: response.model,
            usage_metadata: Some(response.usage.into()),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiModel {
    pub(super) name: String,
    pub(super) version: &'static str,
    pub(super) display_name: &'static str,
    pub(super) description: &'static str,
    pub(super) supported_generation_methods: Vec<&'static str>,
    pub(super) input_token_limit: usize,
    pub(super) output_token_limit: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct GeminiModelsResponse {
    pub(super) models: Vec<GeminiModel>,
}
