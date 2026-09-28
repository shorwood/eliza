//! Ollama wire contracts shared by its route adapters.
#![expect(
    rlib::undocumented_items,
    reason = "this module contains only private Serde wire declarations whose field names are the provider contract"
)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::errors::OllamaError;
use crate::types::json::JsonObject;
use crate::types::model::ModelId;
use crate::types::turn::{
    CompatOutput, CompatTurnResponse, FunctionTool, ToolChoice as CompatToolChoice,
};

// -----------------------------------------------------------------------------
// Message: Defines inbound message roles and content.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum MessageContent {
    Text(String),
    Object(JsonObject),
}

impl MessageContent {
    pub(super) fn into_text(content: Option<Self>) -> String {
        match content {
            Some(Self::Text(text)) => text,
            Some(Self::Object(object)) => object.serialized(),
            None => String::new(),
        }
    }
}

// -----------------------------------------------------------------------------
// Tool: Defines function calls, declarations, and selection policy.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum ToolFunctionKind {
    Function,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ToolCalledFunction {
    name: Option<String>,
    arguments: Option<JsonObject>,
}

impl ToolCalledFunction {
    /// Lower required call fields into the neutral function contract.
    ///
    /// # Errors
    ///
    /// Returns [`OllamaError`] when the function name is absent or empty.
    fn lower(self, param: &'static str) -> Result<crate::types::turn::FunctionCall, OllamaError> {
        let name = self
            .name
            .filter(|name| !name.is_empty())
            .ok_or(OllamaError::MissingToolCallName { param })?;

        // Missing arguments mean an empty object in Ollama's request shape.
        Ok(crate::types::turn::FunctionCall {
            name,
            arguments: self.arguments.unwrap_or_default(),
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ToolCall {
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    function: Option<ToolCalledFunction>,
}

impl ToolCall {
    /// Lower one Ollama tool call into the neutral function contract.
    ///
    /// # Errors
    ///
    /// Returns [`OllamaError`] for a non-function call or a missing function.
    pub(super) fn lower(
        self,
        param: &'static str,
    ) -> Result<crate::types::turn::FunctionCall, OllamaError> {
        // Reject extension calls before reading function-only fields.
        if matches!(self.kind, ToolFunctionKind::Unsupported) {
            return Err(OllamaError::UnsupportedToolCallKind { param });
        }
        let function = self
            .function
            .ok_or(OllamaError::MissingToolCallFunction { param })?;
        function.lower(param)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ToolFunctionDefinition {
    name: Option<String>,
    description: Option<String>,
    parameters: Option<JsonObject>,
}

impl ToolFunctionDefinition {
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
pub(super) struct Tool {
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    function: Option<ToolFunctionDefinition>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(transparent)]
pub(super) struct ToolList(Vec<Tool>);

impl ToolList {
    /// Lower provider tool declarations into the neutral function contract.
    ///
    /// # Errors
    ///
    /// Returns [`OllamaError`] when a tool is not a function or omits its
    /// function definition or name.
    pub(super) fn into_domain(self) -> Result<Vec<FunctionTool>, OllamaError> {
        self.0
            .into_iter()
            .map(|tool| {
                // A function list cannot normalize provider extension tools.
                if matches!(tool.kind, ToolFunctionKind::Unsupported) {
                    return Err(OllamaError::UnsupportedToolDefinition);
                }
                let function = tool.function.ok_or(OllamaError::MissingToolDefinition)?;
                let definition_chars = function.char_count();
                let name = function
                    .name
                    .filter(|name| !name.is_empty())
                    .ok_or(OllamaError::MissingFunctionName)?;
                Ok(FunctionTool::new(name, definition_chars))
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum ToolChoiceMode {
    Auto,
    None,
    Required,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ToolNamedFunction {
    name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ToolNamedChoice {
    function: ToolNamedFunction,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ToolChoice {
    Mode(ToolChoiceMode),
    Named(ToolNamedChoice),
}

impl TryFrom<Option<ToolChoice>> for CompatToolChoice {
    type Error = OllamaError;

    fn try_from(choice: Option<ToolChoice>) -> Result<Self, Self::Error> {
        match choice {
            None | Some(ToolChoice::Mode(ToolChoiceMode::Auto)) => Ok(Self::Auto),
            Some(ToolChoice::Mode(ToolChoiceMode::None)) => Ok(Self::None),
            Some(ToolChoice::Mode(ToolChoiceMode::Required)) => Ok(Self::Required),
            Some(ToolChoice::Mode(ToolChoiceMode::Unsupported)) => {
                Err(OllamaError::UnsupportedToolChoiceMode)
            }
            Some(ToolChoice::Named(choice)) => choice
                .function
                .name
                .filter(|name| !name.is_empty())
                .map(Self::Named)
                .ok_or(OllamaError::MissingNamedToolChoice),
        }
    }
}

// -----------------------------------------------------------------------------
// OutputFormat: Defines structured-output request options.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum OutputFormatName {
    Json,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum OutputFormat {
    Name(OutputFormatName),
    Schema(JsonObject),
}

impl OutputFormat {
    pub(super) fn label(&self) -> &'static str {
        match self {
            Self::Name(OutputFormatName::Json) => "json",
            Self::Name(OutputFormatName::Unsupported) => "named",
            Self::Schema(_schema) => "schema",
        }
    }
}

// -----------------------------------------------------------------------------
// Chat: Defines unary and streaming chat contracts.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatMessage {
    pub(super) role: MessageRole,
    pub(super) content: Option<MessageContent>,
    pub(super) tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatRequest {
    pub(super) model: ModelId,
    pub(super) messages: Vec<ChatMessage>,
    #[serde(rename = "stream")]
    pub(super) should_stream: Option<bool>,
    pub(super) tools: Option<ToolList>,
    pub(super) tool_choice: Option<ToolChoice>,
    pub(super) format: Option<OutputFormat>,
    pub(super) think: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatOutputFunction {
    name: String,
    arguments: JsonObject,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatOutputToolCall {
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    function: ChatOutputFunction,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatOutputMessage {
    pub(super) role: &'static str,
    pub(super) content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) tool_calls: Vec<ChatOutputToolCall>,
}

impl From<&CompatOutput> for ChatOutputMessage {
    fn from(output: &CompatOutput) -> Self {
        match output {
            CompatOutput::Text(text) => Self {
                role: "assistant",
                content: text.clone(),
                tool_calls: Vec::new(),
            },
            CompatOutput::ToolCall(call) => call.into(),
        }
    }
}

impl From<&crate::types::turn::FunctionCall> for ChatOutputMessage {
    fn from(call: &crate::types::turn::FunctionCall) -> Self {
        let function = ChatOutputFunction {
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        };
        let tool_call = ChatOutputToolCall {
            kind: ToolFunctionKind::Function,
            function,
        };
        Self {
            role: "assistant",
            content: String::new(),
            tool_calls: vec![tool_call],
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatResponse {
    pub(super) model: ModelId,
    pub(super) created_at: &'static str,
    pub(super) message: ChatOutputMessage,
    #[serde(rename = "done")]
    pub(super) is_done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) done_reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) total_duration: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) load_duration: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) prompt_eval_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) prompt_eval_duration: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) eval_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) eval_duration: Option<usize>,
}

impl From<&CompatTurnResponse> for ChatResponse {
    fn from(response: &CompatTurnResponse) -> Self {
        Self {
            model: response.model.clone(),
            created_at: "1966-01-01T00:00:00Z",
            message: ChatOutputMessage::from(&response.output),
            is_done: true,
            done_reason: Some("stop"),
            total_duration: Some(0),
            load_duration: Some(0),
            prompt_eval_count: Some(response.usage.prompt),
            prompt_eval_duration: Some(0),
            eval_count: Some(response.usage.completion),
            eval_duration: Some(0),
        }
    }
}

// -----------------------------------------------------------------------------
// Model: Defines the Ollama model catalog.
// -----------------------------------------------------------------------------

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ModelDetails {
    pub(super) parent_model: &'static str,
    pub(super) format: &'static str,
    pub(super) family: &'static str,
    pub(super) families: Vec<&'static str>,
    pub(super) parameter_size: &'static str,
    pub(super) quantization_level: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ModelDescriptor {
    pub(super) name: ModelId,
    pub(super) model: ModelId,
    pub(super) modified_at: &'static str,
    pub(super) size: usize,
    pub(super) digest: &'static str,
    pub(super) details: ModelDetails,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ModelListResponse {
    pub(super) models: Vec<ModelDescriptor>,
}
