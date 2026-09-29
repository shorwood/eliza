//! Anthropic wire contracts shared by its route adapters.
//! Empty metadata on documented newtype fields avoids a `schemars` 0.9 derive collision.
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

/// Role attached to an inbound Anthropic message.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum MessageRole {
    /// Human-authored input.
    User,
    /// Model-authored output.
    Assistant,
    /// Any role outside the supported subset.
    #[serde(other)]
    Unsupported,
}

/// One structured block from an Anthropic message.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum MessageContentBlock {
    /// Plain text content.
    Text {
        /// Text carried by the block.
        text: String,
    },
    /// Model request to invoke a tool.
    ToolUse {
        /// Requested tool name.
        name: Option<String>,
        /// Arguments supplied to the tool.
        input: Option<JsonObject>,
    },
    /// Client-provided result from a prior tool call.
    ToolResult {
        /// Returned tool content.
        content: Option<ToolResultContent>,
    },
    /// Any block kind outside the supported subset.
    #[serde(other)]
    Unsupported,
}

/// Text or structured blocks accepted as message content.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum MessageContent {
    /// Shorthand plain-text content.
    Text(
        /// Message text.
        #[schemars(title = "", description = "")]
        String,
    ),
    /// Structured message blocks.
    Blocks(
        /// Ordered message blocks.
        #[schemars(title = "", description = "")]
        Vec<MessageContentBlock>,
    ),
}

// -----------------------------------------------------------------------------
// ToolResult: Defines client-submitted function output.
// -----------------------------------------------------------------------------

/// One structured block inside a tool result.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ToolResultTextBlock {
    /// Plain text returned by the tool.
    Text {
        /// Text carried by the block.
        text: String,
    },
    /// Any result block kind outside the supported subset.
    #[serde(other)]
    Unsupported,
}

/// Accepted wire shapes for tool-result content.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ToolResultContent {
    /// Shorthand plain-text result.
    Text(
        /// Tool-result text.
        #[schemars(title = "", description = "")]
        String,
    ),
    /// Structured result blocks.
    Blocks(
        /// Ordered tool-result blocks.
        #[schemars(title = "", description = "")]
        Vec<ToolResultTextBlock>,
    ),
    /// Structured JSON result.
    Object(
        /// Tool-result object.
        #[schemars(title = "", description = "")]
        JsonObject,
    ),
}

// -----------------------------------------------------------------------------
// Tool: Defines offered functions and selection policy.
// -----------------------------------------------------------------------------

/// Anthropic function-tool declaration.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct Tool {
    /// Tool name exposed to the model.
    name: Option<String>,
    /// Optional human-readable tool description.
    description: Option<String>,
    /// JSON Schema describing accepted input.
    input_schema: Option<JsonObject>,
}

impl Tool {
    /// Count tool-definition characters for deterministic token accounting.
    fn char_count(&self) -> usize {
        self.name.as_ref().map_or(0, String::len)
            + self.description.as_ref().map_or(0, String::len)
            + self
                .input_schema
                .as_ref()
                .map_or(0, |schema| schema.serialized().chars().count())
    }
}

/// List of tools offered with an Anthropic request.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(transparent)]
pub(super) struct ToolList(
    /// Tool declarations in request order.
    Vec<Tool>,
);

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

/// Anthropic tool-selection policy.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ToolChoice {
    /// Let the model decide whether to call a tool.
    Auto,
    /// Prevent tool calls.
    None,
    /// Require some tool call.
    Any,
    /// Require one named tool.
    Tool {
        /// Selected tool name.
        name: Option<String>,
    },
    /// Any policy outside the supported subset.
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

/// One inbound Anthropic conversation message.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct MessagesInputMessage {
    /// Message author role.
    pub(super) role: MessageRole,
    /// Message payload.
    pub(super) content: MessageContent,
}

/// Anthropic Messages request accepted by the adapter.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct MessagesRequest {
    /// Requested model identifier.
    pub(super) model: ModelId,
    /// Optional system instruction.
    pub(super) system: Option<MessageContent>,
    /// Conversation history in request order.
    pub(super) messages: Vec<MessagesInputMessage>,
    /// Whether to use the event-stream response shape.
    #[serde(rename = "stream")]
    pub(super) should_stream: Option<bool>,
    /// Functions offered to the model.
    pub(super) tools: Option<ToolList>,
    /// Tool-selection policy.
    pub(super) tool_choice: Option<ToolChoice>,
}

/// Reason the Anthropic response stopped generating.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum MessagesStopReason {
    /// The assistant completed its turn normally.
    EndTurn,
    /// The assistant requested a tool invocation.
    ToolUse,
}

/// One content block emitted by an Anthropic response.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum MessagesOutputBlock {
    /// Assistant text output.
    Text {
        /// Generated text.
        text: String,
    },
    /// Assistant request to invoke a tool.
    ToolUse {
        /// Provider-shaped tool-call identifier.
        id: String,
        /// Requested tool name.
        name: String,
        /// Arguments supplied to the tool.
        input: JsonObject,
    },
}

impl MessagesOutputBlock {
    /// Build the empty block that starts streaming one canonical output.
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

/// Token counts serialized by the Anthropic Messages API.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub(super) struct MessagesUsage {
    /// Estimated tokens consumed by request input.
    pub(super) input_tokens: usize,
    /// Estimated tokens emitted by the response.
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

/// Complete Anthropic Messages response.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct MessagesResponse {
    /// Provider-shaped message identifier.
    id: String,
    /// Wire object discriminator.
    #[serde(rename = "type")]
    kind: &'static str,
    /// Fixed assistant role.
    role: &'static str,
    /// Model that produced the response.
    model: ModelId,
    /// Generated response blocks.
    content: Vec<MessagesOutputBlock>,
    /// Reason generation stopped.
    stop_reason: MessagesStopReason,
    /// Matching stop sequence, absent in this emulator.
    stop_sequence: Option<String>,
    /// Request and response token counts.
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

/// Message snapshot carried by the first stream event.
#[derive(Debug, Serialize)]
pub(super) struct StreamMessage {
    /// Provider-shaped message identifier.
    pub(super) id: String,
    /// Wire object discriminator.
    #[serde(rename = "type")]
    pub(super) kind: &'static str,
    /// Fixed assistant role.
    pub(super) role: &'static str,
    /// Model producing the stream.
    pub(super) model: ModelId,
    /// Content blocks opened so far.
    pub(super) content: Vec<MessagesOutputBlock>,
    /// Stop reason, absent while generation is active.
    pub(super) stop_reason: Option<MessagesStopReason>,
    /// Matching stop sequence, absent in this emulator.
    pub(super) stop_sequence: Option<String>,
    /// Input usage known at stream start.
    pub(super) usage: MessagesUsage,
}

/// Incremental payload for one streamed content block.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum StreamDelta {
    /// Text appended to an assistant block.
    TextDelta {
        /// Newly emitted text.
        text: String,
    },
    /// JSON appended to a tool-use input block.
    InputJsonDelta {
        /// Newly emitted JSON fragment.
        partial_json: String,
    },
}

/// Completion metadata carried by a message-delta event.
#[derive(Debug, Serialize)]
pub(super) struct StreamMessageDelta {
    /// Reason generation stopped.
    pub(super) stop_reason: MessagesStopReason,
    /// Matching stop sequence, absent in this emulator.
    pub(super) stop_sequence: Option<String>,
}

/// Completion usage carried by a message-delta event.
#[derive(Debug, Serialize)]
pub(super) struct StreamOutputUsage {
    /// Estimated tokens emitted by the response.
    pub(super) output_tokens: usize,
}

/// Anthropic Messages server-sent event payload.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum StreamEvent {
    /// Opens a streamed message.
    MessageStart {
        /// Initial message snapshot.
        message: StreamMessage,
    },
    /// Opens the response's sole content block.
    ContentBlockStart {
        /// Zero-based block position.
        index: usize,
        /// Empty block matching the eventual output kind.
        content_block: MessagesOutputBlock,
    },
    /// Appends data to an open content block.
    ContentBlockDelta {
        /// Zero-based block position.
        index: usize,
        /// Newly emitted block data.
        delta: StreamDelta,
    },
    /// Closes a content block.
    ContentBlockStop {
        /// Zero-based block position.
        index: usize,
    },
    /// Completes the message with stop and usage metadata.
    MessageDelta {
        /// Final message state.
        delta: StreamMessageDelta,
        /// Final output token count.
        usage: StreamOutputUsage,
    },
    /// Closes the event stream.
    MessageStop,
}

// -----------------------------------------------------------------------------
// Model: Defines the Anthropic model catalog.
// -----------------------------------------------------------------------------

/// Anthropic model-catalog entry.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ModelDescriptor {
    /// Wire object discriminator.
    #[serde(rename = "type")]
    pub(super) kind: &'static str,
    /// Model identifier.
    pub(super) id: ModelId,
    /// Human-readable model label.
    pub(super) display_name: &'static str,
    /// Provider-compatible creation timestamp.
    pub(super) created_at: &'static str,
}

/// Anthropic model-list response.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ModelListResponse {
    /// Available models.
    pub(super) data: Vec<ModelDescriptor>,
    /// Whether another page exists.
    pub(super) has_more: bool,
    /// First model identifier in this page.
    pub(super) first_id: ModelId,
    /// Last model identifier in this page.
    pub(super) last_id: ModelId,
}
