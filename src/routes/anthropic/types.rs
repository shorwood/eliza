//! Anthropic wire contracts shared by its route adapters.
#![expect(
    clippy::missing_errors_doc,
    rlib::missing_section_dividers,
    rlib::undocumented_items,
    reason = "private Serde fields mirror Anthropic's published wire names"
)]

use axum::Json;
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::http::{ProviderRejection, ProviderRejectionKind};
use crate::types::json::JsonObject;
use crate::types::model::ModelId;
use crate::types::turn::{CompatOutput, CompatTurnResponse, FunctionTool, TokenUsage, ToolChoice};

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
pub(super) enum ContentBlock {
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
pub(super) enum Content {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ToolResultContent {
    Text(String),
    Blocks(Vec<ToolResultTextBlock>),
    Object(JsonObject),
}

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
pub(super) struct Message {
    pub(super) role: MessageRole,
    pub(super) content: Content,
}

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

impl ToolList {
    pub(super) fn into_domain(self) -> Result<Vec<FunctionTool>, ProviderRejection> {
        self.0
            .into_iter()
            .map(|tool| {
                let definition_chars = tool.char_count();
                let name = tool.name.filter(|name| !name.is_empty()).ok_or_else(|| {
                    ProviderRejection::invalid("tools", "tool is missing its name")
                })?;
                // An Anthropic tool without a schema is not a callable contract.
                if tool.input_schema.is_none() {
                    return Err(ProviderRejection::invalid(
                        "tools",
                        "function tool is missing input_schema",
                    ));
                }
                Ok(FunctionTool::new(name, definition_chars))
            })
            .collect()
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum AnthropicToolChoice {
    Auto,
    None,
    Any,
    Tool {
        name: Option<String>,
    },
    #[serde(other)]
    Unsupported,
}

impl TryFrom<Option<AnthropicToolChoice>> for ToolChoice {
    type Error = ProviderRejection;

    fn try_from(choice: Option<AnthropicToolChoice>) -> Result<Self, Self::Error> {
        match choice {
            None | Some(AnthropicToolChoice::Auto) => Ok(Self::Auto),
            Some(AnthropicToolChoice::None) => Ok(Self::None),
            Some(AnthropicToolChoice::Any) => Ok(Self::Required),
            Some(AnthropicToolChoice::Tool { name }) => name
                .filter(|name| !name.is_empty())
                .map(Self::Named)
                .ok_or_else(|| {
                    ProviderRejection::invalid("tool_choice", "named tool_choice is missing name")
                }),
            Some(AnthropicToolChoice::Unsupported) => Err(ProviderRejection::invalid(
                "tool_choice",
                "unsupported tool_choice type",
            )),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct MessagesRequest {
    pub(super) model: ModelId,
    pub(super) system: Option<Content>,
    pub(super) messages: Vec<Message>,
    #[serde(rename = "stream")]
    pub(super) should_stream: Option<bool>,
    pub(super) tools: Option<ToolList>,
    pub(super) tool_choice: Option<AnthropicToolChoice>,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum StopReason {
    EndTurn,
    ToolUse,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum OutputBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: JsonObject,
    },
}

impl OutputBlock {
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

impl From<&CompatOutput> for OutputBlock {
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
pub(super) struct Usage {
    pub(super) input_tokens: usize,
    pub(super) output_tokens: usize,
}

impl From<TokenUsage> for Usage {
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
    content: Vec<OutputBlock>,
    stop_reason: StopReason,
    stop_sequence: Option<String>,
    usage: Usage,
}

impl From<CompatTurnResponse> for MessagesResponse {
    fn from(response: CompatTurnResponse) -> Self {
        let stop_reason = match response.output {
            CompatOutput::Text(_) => StopReason::EndTurn,
            CompatOutput::ToolCall(_) => StopReason::ToolUse,
        };
        Self {
            id: format!("msg_{}", Uuid::now_v7().simple()),
            kind: "message",
            role: "assistant",
            model: response.model,
            content: vec![OutputBlock::from(&response.output)],
            stop_reason,
            stop_sequence: None,
            usage: response.usage.into(),
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct StreamMessage {
    pub(super) id: String,
    #[serde(rename = "type")]
    pub(super) kind: &'static str,
    pub(super) role: &'static str,
    pub(super) model: ModelId,
    pub(super) content: Vec<OutputBlock>,
    pub(super) stop_reason: Option<StopReason>,
    pub(super) stop_sequence: Option<String>,
    pub(super) usage: Usage,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum StreamDelta {
    TextDelta { text: String },
    InputJsonDelta { partial_json: String },
}

#[derive(Debug, Serialize)]
pub(super) struct MessageDelta {
    pub(super) stop_reason: StopReason,
    pub(super) stop_sequence: Option<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct OutputUsage {
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
        content_block: OutputBlock,
    },
    ContentBlockDelta {
        index: usize,
        delta: StreamDelta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        delta: MessageDelta,
        usage: OutputUsage,
    },
    MessageStop,
}

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

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum AnthropicErrorKind {
    InvalidRequestError,
    UnsupportedRequestError,
    AuthenticationError,
    RequestTooLarge,
    ServerError,
}

impl From<ProviderRejectionKind> for AnthropicErrorKind {
    fn from(kind: ProviderRejectionKind) -> Self {
        match kind {
            ProviderRejectionKind::Invalid => Self::InvalidRequestError,
            ProviderRejectionKind::Unsupported => Self::UnsupportedRequestError,
            ProviderRejectionKind::Unauthorized => Self::AuthenticationError,
            ProviderRejectionKind::TooLarge => Self::RequestTooLarge,
            ProviderRejectionKind::Internal => Self::ServerError,
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct AnthropicFailureBody {
    #[serde(rename = "type")]
    kind: AnthropicErrorKind,
    message: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct AnthropicFailureResponse {
    #[serde(rename = "type")]
    kind: &'static str,
    error: AnthropicFailureBody,
}

#[derive(derive_more::From)]
pub(super) struct AnthropicRejection(ProviderRejection);

impl IntoResponse for AnthropicRejection {
    fn into_response(self) -> Response {
        let rejection = self.0;
        (
            rejection.status,
            Json(AnthropicFailureResponse {
                kind: "error",
                error: AnthropicFailureBody {
                    kind: rejection.kind.into(),
                    message: rejection.message,
                },
            }),
        )
            .into_response()
    }
}
