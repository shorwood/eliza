//! Ollama wire contracts shared by its route adapters.
#![expect(
    clippy::missing_errors_doc,
    rlib::missing_section_dividers,
    rlib::undocumented_items,
    reason = "private Serde fields mirror Ollama's published wire names"
)]

use axum::Json;
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::types::http::ProviderRejection;
use crate::types::json::JsonObject;
use crate::types::model::ModelId;
use crate::types::turn::{CompatOutput, CompatTurnResponse, FunctionTool, ToolChoice};

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

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum FunctionKind {
    Function,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct CalledFunction {
    name: Option<String>,
    arguments: Option<JsonObject>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ToolCall {
    #[serde(rename = "type")]
    kind: FunctionKind,
    function: Option<CalledFunction>,
}

impl ToolCall {
    pub(super) fn lower(
        self,
        param: &'static str,
    ) -> Result<crate::types::turn::FunctionCall, ProviderRejection> {
        // Reject extension calls before reading function-only fields.
        if matches!(self.kind, FunctionKind::Unsupported) {
            return Err(ProviderRejection::unsupported(
                param,
                "only function tool calls are supported",
            ));
        }
        let function = self
            .function
            .ok_or_else(|| ProviderRejection::invalid(param, "tool call is missing function"))?;
        let name = function
            .name
            .filter(|name| !name.is_empty())
            .ok_or_else(|| ProviderRejection::invalid(param, "tool call is missing name"))?;
        Ok(crate::types::turn::FunctionCall {
            name,
            arguments: function.arguments.unwrap_or_default(),
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct Message {
    pub(super) role: MessageRole,
    pub(super) content: Option<MessageContent>,
    pub(super) tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct Tool {
    #[serde(rename = "type")]
    kind: FunctionKind,
    function: Option<FunctionDefinition>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct FunctionDefinition {
    name: Option<String>,
    description: Option<String>,
    parameters: Option<JsonObject>,
}

impl FunctionDefinition {
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
pub(super) struct ToolList(Vec<Tool>);

impl ToolList {
    pub(super) fn into_domain(self) -> Result<Vec<FunctionTool>, ProviderRejection> {
        self.0
            .into_iter()
            .map(|tool| {
                // A function list cannot normalize provider extension tools.
                if matches!(tool.kind, FunctionKind::Unsupported) {
                    return Err(ProviderRejection::unsupported(
                        "tools",
                        "only client function tools are supported",
                    ));
                }
                let function = tool.function.ok_or_else(|| {
                    ProviderRejection::invalid("tools", "function tool is missing function")
                })?;
                let definition_chars = function.char_count();
                let name = function
                    .name
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| {
                        ProviderRejection::invalid("tools", "function tool is missing name")
                    })?;
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
pub(super) struct NamedToolChoice {
    function: NamedFunction,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct NamedFunction {
    name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum OllamaToolChoice {
    Mode(ToolChoiceMode),
    Named(NamedToolChoice),
}

impl TryFrom<Option<OllamaToolChoice>> for ToolChoice {
    type Error = ProviderRejection;

    fn try_from(choice: Option<OllamaToolChoice>) -> Result<Self, Self::Error> {
        match choice {
            None | Some(OllamaToolChoice::Mode(ToolChoiceMode::Auto)) => Ok(Self::Auto),
            Some(OllamaToolChoice::Mode(ToolChoiceMode::None)) => Ok(Self::None),
            Some(OllamaToolChoice::Mode(ToolChoiceMode::Required)) => Ok(Self::Required),
            Some(OllamaToolChoice::Mode(ToolChoiceMode::Unsupported)) => Err(
                ProviderRejection::invalid("tool_choice", "unsupported tool_choice mode"),
            ),
            Some(OllamaToolChoice::Named(choice)) => choice
                .function
                .name
                .filter(|name| !name.is_empty())
                .map(Self::Named)
                .ok_or_else(|| {
                    ProviderRejection::invalid("tool_choice", "named tool_choice is missing name")
                }),
        }
    }
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

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum OutputFormatName {
    Json,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatRequest {
    pub(super) model: ModelId,
    pub(super) messages: Vec<Message>,
    #[serde(rename = "stream")]
    pub(super) should_stream: Option<bool>,
    pub(super) tools: Option<ToolList>,
    pub(super) tool_choice: Option<OllamaToolChoice>,
    pub(super) format: Option<OutputFormat>,
    pub(super) think: Option<bool>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct OutputFunction {
    name: String,
    arguments: JsonObject,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct OutputToolCall {
    #[serde(rename = "type")]
    kind: FunctionKind,
    function: OutputFunction,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct OutputMessage {
    pub(super) role: &'static str,
    pub(super) content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) tool_calls: Vec<OutputToolCall>,
}

impl From<&CompatOutput> for OutputMessage {
    fn from(output: &CompatOutput) -> Self {
        match output {
            CompatOutput::Text(text) => Self {
                role: "assistant",
                content: text.clone(),
                tool_calls: Vec::new(),
            },
            CompatOutput::ToolCall(call) => Self {
                role: "assistant",
                content: String::new(),
                tool_calls: vec![OutputToolCall {
                    kind: FunctionKind::Function,
                    function: OutputFunction {
                        name: call.name.clone(),
                        arguments: call.arguments.clone(),
                    },
                }],
            },
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatResponse {
    pub(super) model: ModelId,
    pub(super) created_at: &'static str,
    pub(super) message: OutputMessage,
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
            message: OutputMessage::from(&response.output),
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
pub(super) struct TagsResponse {
    pub(super) models: Vec<ModelDescriptor>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct OllamaFailureResponse {
    error: String,
}

#[derive(derive_more::From)]
pub(super) struct OllamaRejection(ProviderRejection);

impl IntoResponse for OllamaRejection {
    fn into_response(self) -> Response {
        let rejection = self.0;
        (
            rejection.status,
            Json(OllamaFailureResponse {
                error: rejection.message,
            }),
        )
            .into_response()
    }
}
