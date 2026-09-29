//! Anthropic wire contracts shared by its route adapters.
#![expect(
    rlib::undocumented_items,
    reason = "this module contains only private Serde wire declarations whose field names are the provider contract"
)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::errors::AnthropicError;
use crate::types::json::JsonObject;
use crate::types::lower::Lower;
use crate::types::model::ModelId;
use crate::types::turn::{
    CompatOutput, CompatTurnResponse, FunctionTool, TokenUsage, ToolChoice as CompatToolChoice,
};

// -----------------------------------------------------------------------------
// Message: Defines inbound message roles and content.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum MessageRole {
    User,
    Assistant,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum MessageContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        name: Option<String>,
        input: Option<JsonObject>,
    },
    ToolResult {
        content: Option<ToolResultContent>,
    },
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum MessageContent {
    Text(String),
    Blocks(Vec<MessageContentBlock>),
}

// -----------------------------------------------------------------------------
// ToolResult: Defines client-submitted function output.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ToolResultTextBlock {
    Text {
        text: String,
    },
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ToolResultContent {
    Text(String),
    Blocks(Vec<ToolResultTextBlock>),
    Object(JsonObject),
}

// -----------------------------------------------------------------------------
// Tool: Defines offered functions and selection policy.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct Tool {
    name: Option<String>,
    description: Option<String>,
    input_schema: Option<JsonObject>,
}

impl Tool {
    fn char_count(&self) -> usize {
        self.name.as_ref().map_or(0, String::len)
            + self.description.as_ref().map_or(0, String::len)
            + self
                .input_schema
                .as_ref()
                .map_or(0, |schema| schema.serialized().chars().count())
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(transparent)]
pub(super) struct ToolList(Vec<Tool>);

impl Lower for ToolList {
    type Canonical = Vec<FunctionTool>;

    type Error = AnthropicError;

    fn lower(self) -> Result<Self::Canonical, Self::Error> {
        self.0
            .into_iter()
            .map(|tool| {
                let definition_chars = tool.char_count();
                let name = tool
                    .name
                    .filter(|name| !name.is_empty())
                    .ok_or(AnthropicError::MissingToolName)?;
                // An Anthropic tool without a schema is not a callable contract.
                if tool.input_schema.is_none() {
                    return Err(AnthropicError::MissingToolInputSchema);
                }
                Ok(FunctionTool::new(name, definition_chars))
            })
            .collect()
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ToolChoice {
    Auto,
    None,
    Any,
    Tool {
        name: Option<String>,
    },
    #[serde(other)]
    Unsupported,
}

impl Lower for ToolChoice {
    type Canonical = CompatToolChoice;

    type Error = AnthropicError;

    fn lower(self) -> Result<Self::Canonical, Self::Error> {
        match self {
            Self::Auto => Ok(CompatToolChoice::Auto),
            Self::None => Ok(CompatToolChoice::None),
            Self::Any => Ok(CompatToolChoice::Required),
            Self::Tool { name } => name
                .filter(|name| !name.is_empty())
                .map(CompatToolChoice::Named)
                .ok_or(AnthropicError::MissingNamedToolChoice),
            Self::Unsupported => Err(AnthropicError::UnsupportedToolChoice),
        }
    }
}

// -----------------------------------------------------------------------------
// Messages: Defines unary request and response contracts.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct MessagesInputMessage {
    pub(super) role: MessageRole,
    pub(super) content: MessageContent,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct MessagesRequest {
    pub(super) model: ModelId,
    pub(super) system: Option<MessageContent>,
    pub(super) messages: Vec<MessagesInputMessage>,
    #[serde(rename = "stream")]
    pub(super) should_stream: Option<bool>,
    pub(super) tools: Option<ToolList>,
    pub(super) tool_choice: Option<ToolChoice>,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum MessagesStopReason {
    EndTurn,
    ToolUse,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum MessagesOutputBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: JsonObject,
    },
}

impl MessagesOutputBlock {
    pub(super) fn empty_for(output: &CompatOutput) -> Self {
        match output {
            CompatOutput::Text(_) => Self::Text {
                text: String::new(),
            },
            CompatOutput::ToolCall(call) => Self::ToolUse {
                id: format!("toolu_{}", Uuid::now_v7().simple()),
                name: call.name.clone(),
                input: JsonObject::default(),
            },
        }
    }
}

impl From<&CompatOutput> for MessagesOutputBlock {
    fn from(output: &CompatOutput) -> Self {
        match output {
            CompatOutput::Text(text) => Self::Text { text: text.clone() },
            CompatOutput::ToolCall(call) => Self::ToolUse {
                id: format!("toolu_{}", Uuid::now_v7().simple()),
                name: call.name.clone(),
                input: call.arguments.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub(super) struct MessagesUsage {
    pub(super) input_tokens: usize,
    pub(super) output_tokens: usize,
}

impl From<TokenUsage> for MessagesUsage {
    fn from(usage: TokenUsage) -> Self {
        Self {
            input_tokens: usage.prompt,
            output_tokens: usage.completion,
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct MessagesResponse {
    id: String,
    #[serde(rename = "type")]
    kind: &'static str,
    role: &'static str,
    model: ModelId,
    content: Vec<MessagesOutputBlock>,
    stop_reason: MessagesStopReason,
    stop_sequence: Option<String>,
    usage: MessagesUsage,
}

impl From<CompatTurnResponse> for MessagesResponse {
    fn from(response: CompatTurnResponse) -> Self {
        let stop_reason = match response.output {
            CompatOutput::Text(_) => MessagesStopReason::EndTurn,
            CompatOutput::ToolCall(_) => MessagesStopReason::ToolUse,
        };
        Self {
            id: format!("msg_{}", Uuid::now_v7().simple()),
            kind: "message",
            role: "assistant",
            model: response.model,
            content: vec![MessagesOutputBlock::from(&response.output)],
            stop_reason,
            stop_sequence: None,
            usage: response.usage.into(),
        }
    }
}

// -----------------------------------------------------------------------------
// Stream: Defines incremental Messages event contracts.
// -----------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub(super) struct StreamMessage {
    pub(super) id: String,
    #[serde(rename = "type")]
    pub(super) kind: &'static str,
    pub(super) role: &'static str,
    pub(super) model: ModelId,
    pub(super) content: Vec<MessagesOutputBlock>,
    pub(super) stop_reason: Option<MessagesStopReason>,
    pub(super) stop_sequence: Option<String>,
    pub(super) usage: MessagesUsage,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum StreamDelta {
    TextDelta { text: String },
    InputJsonDelta { partial_json: String },
}

#[derive(Debug, Serialize)]
pub(super) struct StreamMessageDelta {
    pub(super) stop_reason: MessagesStopReason,
    pub(super) stop_sequence: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct StreamOutputUsage {
    pub(super) output_tokens: usize,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum StreamEvent {
    MessageStart {
        message: StreamMessage,
    },
    ContentBlockStart {
        index: usize,
        content_block: MessagesOutputBlock,
    },
    ContentBlockDelta {
        index: usize,
        delta: StreamDelta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        delta: StreamMessageDelta,
        usage: StreamOutputUsage,
    },
    MessageStop,
}

// -----------------------------------------------------------------------------
// Model: Defines the Anthropic model catalog.
// -----------------------------------------------------------------------------

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ModelDescriptor {
    #[serde(rename = "type")]
    pub(super) kind: &'static str,
    pub(super) id: ModelId,
    pub(super) display_name: &'static str,
    pub(super) created_at: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ModelListResponse {
    pub(super) data: Vec<ModelDescriptor>,
    pub(super) has_more: bool,
    pub(super) first_id: ModelId,
    pub(super) last_id: ModelId,
}
