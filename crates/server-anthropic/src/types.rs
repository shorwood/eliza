//! Anthropic wire contracts shared by the route adapters.
//! Empty metadata on documented newtype fields avoids a `schemars` 0.9 derive collision.
use eliza_http::model::ModelId;
use eliza_modality_chat as chat;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::errors::AnthropicError;

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

/// One source accepted by an Anthropic image block.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum MessageImageSource {
    /// Inline standard base64 with a declared media type.
    Base64 {
        /// Declared PNG or JPEG media type.
        media_type: Option<String>,
        /// Standard base64 payload.
        data: Option<String>,
    },
    /// Data or HTTP(S) URL.
    Url {
        /// Image location.
        url: Option<String>,
    },
    /// Provider-owned uploaded file.
    File {
        /// Provider file identifier.
        file_id: Option<String>,
    },
    /// Any image-source kind outside the supported subset.
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
    /// User-provided image content.
    Image {
        /// Inline, URL, or provider-file source.
        source: Option<MessageImageSource>,
    },
    /// Model request to invoke a tool.
    ToolUse {
        /// Requested tool name.
        name: Option<String>,
        /// Arguments supplied to the tool.
        input: Option<chat::json::JsonObject>,
    },
    /// Client-provided result from a prior tool call.
    ToolResult {
        /// Returned tool content.
        content: Option<ToolResultContent>,
    },
    /// Prior readable thinking returned by the model.
    Thinking {
        /// Readable thinking summary.
        thinking: String,
        /// Opaque signature returned with the summary.
        signature: Option<String>,
    },
    /// Prior opaque thinking retained for replay compatibility.
    RedactedThinking {
        /// Opaque provider payload.
        data: String,
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
        chat::json::JsonObject,
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
    input_schema: Option<chat::json::JsonObject>,
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

impl TryFrom<ToolList> for Vec<chat::turn::FunctionTool> {
    type Error = AnthropicError;

    fn try_from(tools: ToolList) -> Result<Self, Self::Error> {
        tools
            .0
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
                Ok(chat::turn::FunctionTool::new(name, definition_chars))
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

impl TryFrom<ToolChoice> for chat::turn::ToolChoice {
    type Error = AnthropicError;

    fn try_from(choice: ToolChoice) -> Result<Self, Self::Error> {
        match choice {
            ToolChoice::Auto => Ok(Self::Auto),
            ToolChoice::None => Ok(Self::None),
            ToolChoice::Any => Ok(Self::Required),
            ToolChoice::Tool { name } => name
                .filter(|name| !name.is_empty())
                .map(Self::Named)
                .ok_or(AnthropicError::MissingNamedToolChoice),
            ToolChoice::Unsupported => Err(AnthropicError::UnsupportedToolChoice),
        }
    }
}

// -----------------------------------------------------------------------------
// Output: Defines direct assistant-output controls.
// -----------------------------------------------------------------------------

/// Structured-output format nested inside Anthropic output configuration.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum OutputFormat {
    /// Conform final text to a supplied JSON Schema.
    JsonSchema {
        /// JSON Schema compiled into the local deterministic witness.
        schema: Option<chat::json::JsonObject>,
    },
    /// Any future or unknown output format.
    #[serde(other)]
    Unsupported,
}

impl TryFrom<OutputFormat> for chat::structured_output::StructuredOutput {
    type Error = AnthropicError;

    fn try_from(format: OutputFormat) -> Result<Self, Self::Error> {
        // Anthropic exposes only schema mode under this control.
        let OutputFormat::JsonSchema { schema } = format else {
            return Err(AnthropicError::UnsupportedOutputFormat);
        };
        let schema = schema.ok_or(AnthropicError::InvalidOutputFormat)?;
        chat::structured_output::StructuredOutput::try_from(schema).map_err(|source| {
            match source.kind() {
                chat::structured_output::StructuredOutputErrorKind::Invalid => {
                    AnthropicError::InvalidOutputSchema { source }
                }
                chat::structured_output::StructuredOutputErrorKind::Unsupported => {
                    AnthropicError::UnsupportedOutputSchema { source }
                }
            }
        })
    }
}

/// Current Anthropic effort levels.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum OutputEffort {
    /// Low effort.
    Low,
    /// Medium effort.
    Medium,
    /// High effort.
    High,
    /// Extra-high effort.
    Xhigh,
    /// Maximum effort.
    Max,
    /// Any effort outside the current contract.
    #[serde(other)]
    Unsupported,
}

/// Anthropic response-output controls.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct OutputConfig {
    /// Optional schema format for direct assistant text.
    format: Option<OutputFormat>,
    /// Requested response effort, accepted without changing ELIZA.
    effort: Option<OutputEffort>,
}

impl TryFrom<OutputConfig> for chat::structured_output::StructuredOutput {
    type Error = AnthropicError;

    fn try_from(config: OutputConfig) -> Result<Self, Self::Error> {
        if matches!(config.effort, Some(OutputEffort::Unsupported)) {
            Err(AnthropicError::UnsupportedEffort)
        } else {
            Ok(config
                .format
                .map(TryInto::try_into)
                .transpose()?
                .unwrap_or_default())
        }
    }
}

// -----------------------------------------------------------------------------
// Thinking: Defines native Anthropic reasoning controls.
// -----------------------------------------------------------------------------

/// Visibility requested for returned thinking.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum ThinkingDisplay {
    /// Return a readable summary.
    Summarized,
    /// Omit thinking from the response.
    Omitted,
    /// Any display mode outside the current contract.
    #[serde(other)]
    Unsupported,
}

/// Smallest explicit thinking budget accepted by Anthropic.
const MIN_THINKING_BUDGET_TOKENS: usize = 1024;

/// Resolve readable visibility for a supported thinking mode.
///
/// # Errors
///
/// Returns a typed error when the display mode is unsupported.
fn is_thinking_display_visible(display: Option<ThinkingDisplay>) -> Result<bool, AnthropicError> {
    match display {
        None | Some(ThinkingDisplay::Summarized) => Ok(true),
        Some(ThinkingDisplay::Omitted) => Ok(false),
        Some(ThinkingDisplay::Unsupported) => Err(AnthropicError::UnsupportedThinkingDisplay),
    }
}

/// Validate an explicit thinking budget against Anthropic's bounds.
///
/// # Errors
///
/// Returns a typed error when the budget is too small or reaches the output limit.
fn validate_thinking_budget(
    budget_tokens: usize,
    max_tokens: Option<usize>,
) -> Result<(), AnthropicError> {
    if budget_tokens < MIN_THINKING_BUDGET_TOKENS {
        Err(AnthropicError::InvalidThinkingBudget)
    } else if max_tokens.is_some_and(|maximum| budget_tokens >= maximum) {
        Err(AnthropicError::ThinkingBudgetTooLarge)
    } else {
        Ok(())
    }
}

/// Validate explicit thinking and resolve its response visibility.
///
/// # Errors
///
/// Returns a typed error for an invalid budget or display mode.
fn should_include_enabled_thinking(
    budget_tokens: usize,
    max_tokens: Option<usize>,
    display: Option<ThinkingDisplay>,
) -> Result<bool, AnthropicError> {
    validate_thinking_budget(budget_tokens, max_tokens)?;
    is_thinking_display_visible(display)
}

/// Anthropic thinking mode.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Thinking {
    /// Explicit thinking with a required budget.
    Enabled {
        /// Requested thinking budget.
        budget_tokens: usize,
        /// Returned thinking visibility.
        display: Option<ThinkingDisplay>,
    },
    /// Provider-selected adaptive thinking.
    Adaptive {
        /// Returned thinking visibility.
        display: Option<ThinkingDisplay>,
    },
    /// Disable thinking.
    Disabled,
    /// Limit thinking to tool boundaries; no readable trace is returned here.
    BetweenTools,
    /// Any thinking mode outside the current contract.
    #[serde(other)]
    Unsupported,
}

impl Thinking {
    /// Validate thinking controls and report readable visibility.
    ///
    /// # Errors
    ///
    /// Returns a typed error for invalid budgets or modes.
    fn should_include_reasoning(&self, max_tokens: Option<usize>) -> Result<bool, AnthropicError> {
        match *self {
            Self::Enabled {
                budget_tokens,
                display,
            } => should_include_enabled_thinking(budget_tokens, max_tokens, display),
            Self::Adaptive { display } => is_thinking_display_visible(display),
            Self::Disabled | Self::BetweenTools => Ok(false),
            Self::Unsupported => Err(AnthropicError::UnsupportedThinkingMode),
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
    /// Optional direct-output schema controls.
    pub(super) output_config: Option<OutputConfig>,
    /// Native thinking controls.
    thinking: Option<Thinking>,
    /// Maximum output tokens accepted as a compatibility limit.
    max_tokens: Option<usize>,
}

impl MessagesRequest {
    /// Validate output limits and return readable-thinking visibility.
    ///
    /// # Errors
    ///
    /// Returns a typed error for invalid thinking or token controls.
    pub(super) fn should_include_reasoning(&self) -> Result<bool, AnthropicError> {
        if self.max_tokens == Some(0) {
            Err(AnthropicError::InvalidMaxTokens)
        } else if let Some(thinking) = &self.thinking {
            thinking.should_include_reasoning(self.max_tokens)
        } else {
            Ok(false)
        }
    }
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
        input: chat::json::JsonObject,
    },
    /// Readable thinking returned before the answer.
    Thinking {
        /// Mechanical trace summary.
        thinking: String,
        /// Stable fixture signature over the trace.
        #[serde(skip_serializing_if = "String::is_empty")]
        signature: String,
    },
}

impl MessagesOutputBlock {
    /// Build the empty block that starts streaming one canonical output.
    pub(super) fn empty_for(output: &chat::turn::Output) -> Self {
        match output {
            chat::turn::Output::Text(_) => Self::Text {
                text: String::new(),
            },
            chat::turn::Output::ToolCall(call) => Self::ToolUse {
                id: format!("toolu_{}", Uuid::now_v7().simple()),
                name: call.name.clone(),
                input: chat::json::JsonObject::default(),
            },
        }
    }
}

impl From<&chat::turn::Output> for MessagesOutputBlock {
    fn from(output: &chat::turn::Output) -> Self {
        match output {
            chat::turn::Output::Text(text) => Self::Text { text: text.clone() },
            chat::turn::Output::ToolCall(call) => Self::ToolUse {
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

impl From<chat::turn::Usage> for MessagesUsage {
    fn from(usage: chat::turn::Usage) -> Self {
        Self {
            input_tokens: usage.prompt,
            output_tokens: usage.completion + usage.reasoning,
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

impl MessagesResponse {
    /// Attach the provider-owned model identity to one canonical result.
    pub(super) fn from_compat(model: ModelId, response: &chat::turn::Response) -> Self {
        let stop_reason = match &response.output {
            chat::turn::Output::Text(_) => MessagesStopReason::EndTurn,
            chat::turn::Output::ToolCall(_) => MessagesStopReason::ToolUse,
        };
        let mut content = response
            .reasoning
            .as_ref()
            .map_or_else(Vec::new, |reasoning| {
                vec![MessagesOutputBlock::Thinking {
                    thinking: reasoning.text.clone(),
                    signature: reasoning.signature.clone(),
                }]
            });
        content.push(MessagesOutputBlock::from(&response.output));
        Self {
            id: format!("msg_{}", Uuid::now_v7().simple()),
            kind: "message",
            role: "assistant",
            model,
            content,
            stop_reason,
            stop_sequence: None,
            usage: MessagesUsage {
                input_tokens: response.usage.prompt,
                output_tokens: response.usage.completion + response.usage.reasoning,
            },
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
    #[serde(rename = "text_delta")]
    Text {
        /// Newly emitted text.
        text: String,
    },
    /// JSON appended to a tool-use input block.
    #[serde(rename = "input_json_delta")]
    InputJson {
        /// Newly emitted JSON fragment.
        partial_json: String,
    },
    /// Text appended to a thinking block.
    #[serde(rename = "thinking_delta")]
    Thinking {
        /// Newly emitted thinking text.
        thinking: String,
    },
    /// Signature appended after thinking text.
    #[serde(rename = "signature_delta")]
    Signature {
        /// Stable fixture signature.
        signature: String,
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
