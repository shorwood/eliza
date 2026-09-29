//! `OpenAI` wire contracts shared by its route adapters.
#![expect(
    rlib::undocumented_items,
    reason = "this module contains only private Serde wire declarations whose field names are the provider contract"
)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::errors::{OpenAiError, OpenAiFailureBody};
use crate::types::http::unix_timestamp;
use crate::types::json::JsonObject;
use crate::types::lower::Lower;
use crate::types::model::ModelId;
use crate::types::turn::{
    CompatOutput, CompatTurnResponse, FunctionCall, FunctionTool, TokenUsage,
    ToolChoice as CompatToolChoice,
};

// -----------------------------------------------------------------------------
// Tool: Defines shared function and selection primitives.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum ToolFunctionKind {
    Function,
    #[serde(other)]
    Unsupported,
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
#[serde(untagged)]
pub(super) enum ToolFunctionArguments {
    Encoded(String),
    Object(JsonObject),
}

impl ToolFunctionArguments {
    /// Convert encoded or structured arguments into a validated JSON object.
    ///
    /// # Errors
    ///
    /// Returns [`OpenAiError`] when encoded arguments are not a JSON object.
    pub(super) fn into_object(self, param: &'static str) -> Result<JsonObject, OpenAiError> {
        match self {
            Self::Object(arguments) => Ok(arguments),
            Self::Encoded(arguments) => serde_json::from_str(&arguments)
                .map_err(|source| OpenAiError::InvalidFunctionArguments { param, source }),
        }
    }
}

// -----------------------------------------------------------------------------
// AssistantRole: Defines the sole assistant response role.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum AssistantRole {
    Assistant,
}

// -----------------------------------------------------------------------------
// ChatContent: Defines Chat Completions content shapes.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ChatContentPart {
    Text {
        text: String,
    },
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ChatContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
    Object(JsonObject),
}

// -----------------------------------------------------------------------------
// ChatTool: Defines Chat Completions functions and selection policy.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatToolFunction {
    name: Option<String>,
    arguments: Option<ToolFunctionArguments>,
}

impl ChatToolFunction {
    /// Lower required function fields into the neutral function contract.
    ///
    /// # Errors
    ///
    /// Returns [`OpenAiError`] when the name or arguments are absent or the
    /// arguments are not a JSON object.
    fn lower(self, param: &'static str) -> Result<FunctionCall, OpenAiError> {
        let name = self
            .name
            .filter(|name| !name.is_empty())
            .ok_or(OpenAiError::MissingFunctionName { param })?;
        let arguments = self
            .arguments
            .ok_or(OpenAiError::MissingToolCallArguments { param })?;
        Ok(FunctionCall {
            name,
            arguments: arguments.into_object(param)?,
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatToolCall {
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    function: Option<ChatToolFunction>,
}

impl ChatToolCall {
    /// Lower one chat tool call into the neutral function contract.
    ///
    /// # Errors
    ///
    /// Returns [`OpenAiError`] for a non-function call or missing function data.
    pub(super) fn lower(self, param: &'static str) -> Result<FunctionCall, OpenAiError> {
        // Reject non-function calls before interpreting function-only fields.
        if matches!(self.kind, ToolFunctionKind::Unsupported) {
            return Err(OpenAiError::UnsupportedToolCallKind { param });
        }
        let function = self
            .function
            .ok_or(OpenAiError::MissingToolCallFunction { param })?;
        function.lower(param)
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(super) struct ChatToolDefinition {
    name: Option<String>,
    description: Option<String>,
    parameters: Option<JsonObject>,
}

impl ChatToolDefinition {
    fn char_count(&self) -> usize {
        self.name.as_ref().map_or(0, String::len)
            + self.description.as_ref().map_or(0, String::len)
            + self
                .parameters
                .as_ref()
                .map_or(0, |parameters| parameters.serialized().chars().count())
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(super) struct ChatTool {
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    function: Option<ChatToolDefinition>,
}

impl ChatTool {
    fn char_count(&self) -> usize {
        let function = self.function.as_ref();
        function.map_or(0, ChatToolDefinition::char_count) + 8
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(transparent)]
pub(super) struct ChatToolList(Vec<ChatTool>);

impl Lower for ChatToolList {
    type Canonical = Vec<FunctionTool>;

    type Error = OpenAiError;

    fn lower(self) -> Result<Self::Canonical, Self::Error> {
        self.0
            .into_iter()
            .map(|tool| {
                // A function list cannot normalize provider extension tools.
                if matches!(tool.kind, ToolFunctionKind::Unsupported) {
                    return Err(OpenAiError::UnsupportedToolDefinition);
                }
                let definition_chars = tool.char_count();
                let function = tool.function.ok_or(OpenAiError::MissingToolDefinition)?;
                let name = function
                    .name
                    .filter(|name| !name.is_empty())
                    .ok_or(OpenAiError::MissingFunctionName { param: "tools" })?;
                Ok(FunctionTool::new(name, definition_chars))
            })
            .collect()
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatToolNamedFunction {
    name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatToolNamedChoice {
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    function: ChatToolNamedFunction,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ChatToolChoice {
    Mode(ToolChoiceMode),
    Named(ChatToolNamedChoice),
}

impl Lower for ChatToolChoice {
    type Canonical = CompatToolChoice;

    type Error = OpenAiError;

    fn lower(self) -> Result<Self::Canonical, Self::Error> {
        match self {
            Self::Mode(ToolChoiceMode::Auto) => Ok(CompatToolChoice::Auto),
            Self::Mode(ToolChoiceMode::None) => Ok(CompatToolChoice::None),
            Self::Mode(ToolChoiceMode::Required) => Ok(CompatToolChoice::Required),
            Self::Mode(ToolChoiceMode::Unsupported) => Err(OpenAiError::UnsupportedToolChoiceMode),
            Self::Named(choice) if matches!(choice.kind, ToolFunctionKind::Function) => {
                Ok(CompatToolChoice::Named(choice.function.name))
            }
            Self::Named(_) => Err(OpenAiError::UnsupportedNamedToolChoice),
        }
    }
}

// -----------------------------------------------------------------------------
// ChatRequest: Defines the Chat Completions request envelope.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum ChatRequestRole {
    System,
    Developer,
    User,
    Assistant,
    Tool,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatRequestMessage {
    pub(super) role: ChatRequestRole,
    pub(super) content: Option<ChatContent>,
    pub(super) tool_calls: Option<Vec<ChatToolCall>>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
pub(super) struct ChatRequestStreamOptions {
    #[serde(rename = "include_usage")]
    pub(super) should_include_usage: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ChatRequest {
    pub(super) model: ModelId,
    pub(super) messages: Vec<ChatRequestMessage>,
    #[serde(rename = "stream")]
    pub(super) should_stream: Option<bool>,
    pub(super) stream_options: Option<ChatRequestStreamOptions>,
    pub(super) tools: Option<ChatToolList>,
    pub(super) tool_choice: Option<ChatToolChoice>,
    pub(super) response_format: Option<JsonObject>,
}

// -----------------------------------------------------------------------------
// ResponsesRequestContent: Defines Responses input content shapes.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ResponsesRequestContentPart {
    InputText {
        text: String,
    },
    OutputText {
        text: String,
    },
    Text {
        text: String,
    },
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ResponsesRequestContent {
    Text(String),
    Parts(Vec<ResponsesRequestContentPart>),
}

// -----------------------------------------------------------------------------
// ResponsesRequest: Defines the Responses API request contract.
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ResponsesRequestToolOutput {
    Text(String),
    Object(JsonObject),
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum ResponsesRequestRole {
    User,
    System,
    Developer,
    Assistant,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ResponsesRequestInputItem {
    Message {
        role: ResponsesRequestRole,
        content: ResponsesRequestContent,
    },
    FunctionCall {
        name: Option<String>,
        arguments: Option<ToolFunctionArguments>,
    },
    FunctionCallOutput {
        output: Option<ResponsesRequestToolOutput>,
    },
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ResponsesRequestInput {
    Text(String),
    Items(Vec<ResponsesRequestInputItem>),
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(super) struct ResponsesRequestTool {
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    name: Option<String>,
    description: Option<String>,
    parameters: Option<JsonObject>,
}

impl ResponsesRequestTool {
    fn char_count(&self) -> usize {
        self.name.as_ref().map_or(0, String::len)
            + self.description.as_ref().map_or(0, String::len)
            + self
                .parameters
                .as_ref()
                .map_or(0, |parameters| parameters.serialized().chars().count())
            + 8
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(transparent)]
pub(super) struct ResponsesRequestToolList(Vec<ResponsesRequestTool>);

impl Lower for ResponsesRequestToolList {
    type Canonical = Vec<FunctionTool>;

    type Error = OpenAiError;

    fn lower(self) -> Result<Self::Canonical, Self::Error> {
        self.0
            .into_iter()
            .map(|tool| {
                // A function list cannot normalize provider extension tools.
                if matches!(tool.kind, ToolFunctionKind::Unsupported) {
                    return Err(OpenAiError::UnsupportedToolDefinition);
                }
                let definition_chars = tool.char_count();
                let name = tool
                    .name
                    .filter(|name| !name.is_empty())
                    .ok_or(OpenAiError::MissingFunctionName { param: "tools" })?;
                Ok(FunctionTool::new(name, definition_chars))
            })
            .collect()
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ResponsesRequestNamedToolChoice {
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ResponsesRequestToolChoice {
    Mode(ToolChoiceMode),
    Named(ResponsesRequestNamedToolChoice),
}

impl Lower for ResponsesRequestToolChoice {
    type Canonical = CompatToolChoice;

    type Error = OpenAiError;

    fn lower(self) -> Result<Self::Canonical, Self::Error> {
        match self {
            Self::Mode(ToolChoiceMode::Auto) => Ok(CompatToolChoice::Auto),
            Self::Mode(ToolChoiceMode::None) => Ok(CompatToolChoice::None),
            Self::Mode(ToolChoiceMode::Required) => Ok(CompatToolChoice::Required),
            Self::Mode(ToolChoiceMode::Unsupported) => Err(OpenAiError::UnsupportedToolChoiceMode),
            Self::Named(choice) if matches!(choice.kind, ToolFunctionKind::Function) => {
                let name = choice.name.filter(|name| !name.is_empty()).ok_or(
                    OpenAiError::MissingFunctionName {
                        param: "tool_choice",
                    },
                )?;
                Ok(CompatToolChoice::Named(name))
            }
            Self::Named(_) => Err(OpenAiError::UnsupportedNamedToolChoice),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ResponsesRequestTextFormat {
    Text,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ResponsesRequestTextConfig {
    pub(super) format: Option<ResponsesRequestTextFormat>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ResponsesRequest {
    pub(super) model: ModelId,
    pub(super) input: ResponsesRequestInput,
    pub(super) instructions: Option<ResponsesRequestContent>,
    #[serde(rename = "stream")]
    pub(super) should_stream: Option<bool>,
    pub(super) tools: Option<ResponsesRequestToolList>,
    pub(super) tool_choice: Option<ResponsesRequestToolChoice>,
    pub(super) text: Option<ResponsesRequestTextConfig>,
}

// -----------------------------------------------------------------------------
// ChatResponse: Defines complete Chat Completions responses.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum ChatResponseFinishReason {
    Stop,
    ToolCalls,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(super) struct ChatResponseFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(super) struct ChatResponseToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    function: ChatResponseFunctionCall,
}

impl ChatResponseToolCall {
    fn from_call(call: &FunctionCall, id: String) -> Self {
        Self {
            id,
            kind: ToolFunctionKind::Function,
            function: ChatResponseFunctionCall {
                name: call.name.clone(),
                arguments: call.arguments.serialized(),
            },
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatResponseMessage {
    role: AssistantRole,
    content: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<ChatResponseToolCall>,
}

impl From<String> for ChatResponseMessage {
    fn from(text: String) -> Self {
        Self {
            role: AssistantRole::Assistant,
            content: Some(text),
            tool_calls: Vec::new(),
        }
    }
}

impl From<FunctionCall> for ChatResponseMessage {
    fn from(call: FunctionCall) -> Self {
        let id = format!("call_{}", Uuid::now_v7().simple());
        Self {
            role: AssistantRole::Assistant,
            content: None,
            tool_calls: vec![ChatResponseToolCall::from_call(&call, id)],
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatResponseChoice {
    index: usize,
    message: ChatResponseMessage,
    finish_reason: ChatResponseFinishReason,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[expect(
    clippy::struct_field_names,
    reason = "field names must match the OpenAI usage contract"
)]
pub(super) struct ChatResponseUsage {
    prompt_tokens: usize,
    completion_tokens: usize,
    total_tokens: usize,
}

impl From<TokenUsage> for ChatResponseUsage {
    fn from(usage: TokenUsage) -> Self {
        Self {
            prompt_tokens: usage.prompt,
            completion_tokens: usage.completion,
            total_tokens: usage.total,
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct ChatResponse {
    id: String,
    object: &'static str,
    created: u64,
    model: ModelId,
    choices: Vec<ChatResponseChoice>,
    usage: ChatResponseUsage,
}

impl From<CompatTurnResponse> for ChatResponse {
    fn from(response: CompatTurnResponse) -> Self {
        let (message, finish_reason) = match response.output {
            CompatOutput::Text(text) => (
                ChatResponseMessage::from(text),
                ChatResponseFinishReason::Stop,
            ),
            CompatOutput::ToolCall(call) => (
                ChatResponseMessage::from(call),
                ChatResponseFinishReason::ToolCalls,
            ),
        };
        Self {
            id: format!("chatcmpl-{}", Uuid::now_v7().simple()),
            object: "chat.completion",
            created: unix_timestamp(),
            model: response.model,
            choices: vec![ChatResponseChoice {
                index: 0,
                message,
                finish_reason,
            }],
            usage: response.usage.into(),
        }
    }
}

// -----------------------------------------------------------------------------
// ChatStream: Defines incremental Chat Completions events.
// -----------------------------------------------------------------------------

#[derive(Debug, Default, Serialize, JsonSchema)]
pub(super) struct ChatStreamFunctionDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) arguments: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatStreamToolCallDelta {
    pub(super) index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub(super) kind: Option<ToolFunctionKind>,
    pub(super) function: ChatStreamFunctionDelta,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub(super) struct ChatStreamDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) role: Option<AssistantRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) content: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) tool_calls: Vec<ChatStreamToolCallDelta>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatStreamChoice {
    pub(super) index: usize,
    pub(super) delta: ChatStreamDelta,
    pub(super) finish_reason: Option<ChatResponseFinishReason>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatStreamChunk<'a> {
    pub(super) id: &'a str,
    pub(super) object: &'static str,
    pub(super) created: u64,
    pub(super) model: &'a str,
    pub(super) choices: Vec<ChatStreamChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) usage: Option<ChatResponseUsage>,
}

// -----------------------------------------------------------------------------
// ResponsesResponse: Defines complete and streaming Responses API output.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResponsesResponseStatus {
    InProgress,
    Completed,
}

pub(super) struct ResponsesResponseIds {
    pub(super) item: String,
    pub(super) call: String,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResponsesResponseTextKind {
    OutputText,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(super) struct ResponsesResponseText {
    #[serde(rename = "type")]
    kind: ResponsesResponseTextKind,
    text: String,
    annotations: Vec<String>,
    logprobs: Vec<String>,
}

impl From<&String> for ResponsesResponseText {
    fn from(text: &String) -> Self {
        Self {
            kind: ResponsesResponseTextKind::OutputText,
            text: text.clone(),
            annotations: Vec::new(),
            logprobs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ResponsesResponseOutput {
    Message {
        id: String,
        status: ResponsesResponseStatus,
        role: AssistantRole,
        content: Vec<ResponsesResponseText>,
    },
    FunctionCall {
        id: String,
        call_id: String,
        name: String,
        arguments: String,
        status: ResponsesResponseStatus,
    },
}

impl ResponsesResponseOutput {
    pub(super) fn from_compat(
        output: &CompatOutput,
        ids: ResponsesResponseIds,
        status: ResponsesResponseStatus,
    ) -> Self {
        match output {
            CompatOutput::Text(text) => {
                let content = vec![ResponsesResponseText::from(text)];
                Self::Message {
                    id: ids.item,
                    status,
                    role: AssistantRole::Assistant,
                    content,
                }
            }
            CompatOutput::ToolCall(call) => Self::FunctionCall {
                id: ids.item,
                call_id: ids.call,
                name: call.name.clone(),
                arguments: call.arguments.serialized(),
                status,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub(super) struct ResponsesResponseInputTokenDetails {
    cached_tokens: usize,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub(super) struct ResponsesResponseOutputTokenDetails {
    reasoning_tokens: usize,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub(super) struct ResponsesResponseUsage {
    input_tokens: usize,
    input_tokens_details: ResponsesResponseInputTokenDetails,
    output_tokens: usize,
    output_tokens_details: ResponsesResponseOutputTokenDetails,
    total_tokens: usize,
}

impl From<TokenUsage> for ResponsesResponseUsage {
    fn from(usage: TokenUsage) -> Self {
        Self {
            input_tokens: usage.prompt,
            input_tokens_details: ResponsesResponseInputTokenDetails { cached_tokens: 0 },
            output_tokens: usage.completion,
            output_tokens_details: ResponsesResponseOutputTokenDetails {
                reasoning_tokens: 0,
            },
            total_tokens: usage.total,
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(crate) struct ResponsesResponse {
    pub(super) id: String,
    pub(super) object: &'static str,
    pub(super) created_at: u64,
    pub(super) status: ResponsesResponseStatus,
    pub(super) error: Option<OpenAiFailureBody>,
    pub(super) incomplete_details: Option<String>,
    pub(super) model: ModelId,
    pub(super) output: Vec<ResponsesResponseOutput>,
    pub(super) output_text: String,
    pub(super) usage: ResponsesResponseUsage,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ResponsesResponseProgress {
    pub(super) id: String,
    pub(super) object: &'static str,
    pub(super) created_at: u64,
    pub(super) status: ResponsesResponseStatus,
    pub(super) model: ModelId,
    pub(super) output: Vec<ResponsesResponseOutput>,
    pub(super) error: Option<OpenAiFailureBody>,
    pub(super) incomplete_details: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(super) enum ResponsesResponseStreamEvent {
    #[serde(rename = "response.created")]
    Created {
        sequence_number: usize,
        response: ResponsesResponseProgress,
    },
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded {
        sequence_number: usize,
        output_index: usize,
        item: ResponsesResponseOutput,
    },
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta {
        sequence_number: usize,
        item_id: String,
        output_index: usize,
        content_index: usize,
        delta: String,
    },
    #[serde(rename = "response.output_text.done")]
    OutputTextDone {
        sequence_number: usize,
        item_id: String,
        output_index: usize,
        content_index: usize,
        text: String,
    },
    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionArgumentsDelta {
        sequence_number: usize,
        item_id: String,
        output_index: usize,
        call_id: String,
        delta: String,
    },
    #[serde(rename = "response.function_call_arguments.done")]
    FunctionArgumentsDone {
        sequence_number: usize,
        item_id: String,
        output_index: usize,
        call_id: String,
        name: String,
        arguments: String,
    },
    #[serde(rename = "response.output_item.done")]
    OutputItemDone {
        sequence_number: usize,
        output_index: usize,
        item: ResponsesResponseOutput,
    },
    #[serde(rename = "response.completed")]
    Completed {
        sequence_number: usize,
        response: ResponsesResponse,
    },
}

pub(super) fn responses_response_output_text(response: &CompatTurnResponse) -> String {
    match &response.output {
        CompatOutput::Text(text) => text.clone(),
        CompatOutput::ToolCall(_) => String::new(),
    }
}

// -----------------------------------------------------------------------------
// Model: Defines the OpenAI model catalog.
// -----------------------------------------------------------------------------

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ModelDescriptor {
    pub(super) id: ModelId,
    pub(super) object: &'static str,
    pub(super) created: u64,
    pub(super) owned_by: &'static str,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ModelListResponse {
    pub(super) object: &'static str,
    pub(super) data: Vec<ModelDescriptor>,
}
