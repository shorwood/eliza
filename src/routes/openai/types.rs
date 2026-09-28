//! `OpenAI` wire contracts shared by its route adapters.
#![expect(
    clippy::missing_errors_doc,
    rlib::missing_section_dividers,
    rlib::undocumented_items,
    reason = "wire types stay grouped by endpoint; private Serde names are self-describing"
)]

use axum::Json;
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::http::{ProviderRejection, ProviderRejectionKind, unix_timestamp};
use crate::types::json::JsonObject;
use crate::types::model::ModelId;
use crate::types::turn::{
    CompatOutput, CompatTurnResponse, FunctionCall, FunctionTool, TokenUsage, ToolChoice,
};

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum MessageRole {
    System,
    Developer,
    User,
    Assistant,
    Tool,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum FunctionKind {
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

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum FinishReason {
    Stop,
    ToolCalls,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum AssistantRole {
    Assistant,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ChatContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
    Object(JsonObject),
}

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
pub(super) enum ResponsesContent {
    Text(String),
    Parts(Vec<ResponsesContentPart>),
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ResponsesContentPart {
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
pub(super) enum FunctionArguments {
    Encoded(String),
    Object(JsonObject),
}

impl FunctionArguments {
    pub(super) fn into_object(self, param: &'static str) -> Result<JsonObject, ProviderRejection> {
        match self {
            Self::Object(arguments) => Ok(arguments),
            Self::Encoded(arguments) => serde_json::from_str(&arguments).map_err(|error| {
                ProviderRejection::invalid(param, format!("invalid function arguments: {error}"))
            }),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ToolOutput {
    Text(String),
    Object(JsonObject),
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatMessage {
    pub(super) role: MessageRole,
    pub(super) content: Option<ChatContent>,
    pub(super) tool_calls: Option<Vec<ChatToolCall>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatToolCall {
    #[serde(rename = "type")]
    kind: FunctionKind,
    function: Option<CalledFunction>,
}

impl ChatToolCall {
    pub(super) fn lower(self, param: &'static str) -> Result<FunctionCall, ProviderRejection> {
        // Reject non-function calls before interpreting function-only fields.
        if matches!(self.kind, FunctionKind::Unsupported) {
            return Err(ProviderRejection::unsupported(
                param,
                "only function tool calls are supported",
            ));
        }
        let function = self.function.ok_or_else(|| {
            ProviderRejection::invalid(param, "function tool call is missing function")
        })?;
        let name = function
            .name
            .filter(|name| !name.is_empty())
            .ok_or_else(|| ProviderRejection::invalid(param, "function is missing its name"))?;
        let arguments = function.arguments.ok_or_else(|| {
            ProviderRejection::invalid(param, "function tool call is missing arguments")
        })?;
        Ok(FunctionCall {
            name,
            arguments: arguments.into_object(param)?,
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct CalledFunction {
    name: Option<String>,
    arguments: Option<FunctionArguments>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(super) struct ChatTool {
    #[serde(rename = "type")]
    kind: FunctionKind,
    function: Option<ChatFunctionDefinition>,
}

impl ChatTool {
    fn char_count(&self) -> usize {
        let function = self.function.as_ref();
        function.map_or(0, ChatFunctionDefinition::char_count) + 8
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(super) struct ChatFunctionDefinition {
    name: Option<String>,
    description: Option<String>,
    parameters: Option<JsonObject>,
}

impl ChatFunctionDefinition {
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
pub(super) struct ChatToolList(Vec<ChatTool>);

impl ChatToolList {
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
                let definition_chars = tool.char_count();
                let function = tool.function.ok_or_else(|| {
                    ProviderRejection::invalid("tools", "function tool is missing function")
                })?;
                let name = function
                    .name
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| {
                        ProviderRejection::invalid("tools", "function is missing its name")
                    })?;
                Ok(FunctionTool::new(name, definition_chars))
            })
            .collect()
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ChatToolChoice {
    Mode(ToolChoiceMode),
    Named(NamedChatToolChoice),
}

impl TryFrom<Option<ChatToolChoice>> for ToolChoice {
    type Error = ProviderRejection;

    fn try_from(choice: Option<ChatToolChoice>) -> Result<Self, Self::Error> {
        match choice {
            None | Some(ChatToolChoice::Mode(ToolChoiceMode::Auto)) => Ok(Self::Auto),
            Some(ChatToolChoice::Mode(ToolChoiceMode::None)) => Ok(Self::None),
            Some(ChatToolChoice::Mode(ToolChoiceMode::Required)) => Ok(Self::Required),
            Some(ChatToolChoice::Mode(ToolChoiceMode::Unsupported)) => Err(
                ProviderRejection::invalid("tool_choice", "unsupported tool_choice mode"),
            ),
            Some(ChatToolChoice::Named(choice))
                if matches!(choice.kind, FunctionKind::Function) =>
            {
                Ok(Self::Named(choice.function.name))
            }
            Some(ChatToolChoice::Named(_)) => Err(ProviderRejection::unsupported(
                "tool_choice",
                "only named function choices are supported",
            )),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct NamedChatToolChoice {
    #[serde(rename = "type")]
    kind: FunctionKind,
    function: NamedFunction,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct NamedFunction {
    name: String,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
pub(super) struct StreamOptions {
    #[serde(rename = "include_usage")]
    pub(super) should_include_usage: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ChatCompletionRequest {
    pub(super) model: ModelId,
    pub(super) messages: Vec<ChatMessage>,
    #[serde(rename = "stream")]
    pub(super) should_stream: Option<bool>,
    pub(super) stream_options: Option<StreamOptions>,
    pub(super) tools: Option<ChatToolList>,
    pub(super) tool_choice: Option<ChatToolChoice>,
    pub(super) response_format: Option<JsonObject>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ResponsesRequest {
    pub(super) model: ModelId,
    pub(super) input: ResponsesInput,
    pub(super) instructions: Option<ResponsesContent>,
    #[serde(rename = "stream")]
    pub(super) should_stream: Option<bool>,
    pub(super) tools: Option<ResponsesToolList>,
    pub(super) tool_choice: Option<ResponsesToolChoice>,
    pub(super) text: Option<ResponsesTextConfig>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ResponsesInput {
    Text(String),
    Items(Vec<ResponsesInputItem>),
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ResponsesInputItem {
    Message {
        role: ResponsesRole,
        content: ResponsesContent,
    },
    FunctionCall {
        name: Option<String>,
        arguments: Option<FunctionArguments>,
    },
    FunctionCallOutput {
        output: Option<ToolOutput>,
    },
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum ResponsesRole {
    User,
    System,
    Developer,
    Assistant,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(super) struct ResponsesTool {
    #[serde(rename = "type")]
    kind: FunctionKind,
    name: Option<String>,
    description: Option<String>,
    parameters: Option<JsonObject>,
}

impl ResponsesTool {
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
pub(super) struct ResponsesToolList(Vec<ResponsesTool>);

impl ResponsesToolList {
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
                let definition_chars = tool.char_count();
                let name = tool.name.filter(|name| !name.is_empty()).ok_or_else(|| {
                    ProviderRejection::invalid("tools", "function is missing its name")
                })?;
                Ok(FunctionTool::new(name, definition_chars))
            })
            .collect()
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ResponsesToolChoice {
    Mode(ToolChoiceMode),
    Named(NamedResponsesToolChoice),
}

impl TryFrom<Option<ResponsesToolChoice>> for ToolChoice {
    type Error = ProviderRejection;

    fn try_from(choice: Option<ResponsesToolChoice>) -> Result<Self, Self::Error> {
        match choice {
            None | Some(ResponsesToolChoice::Mode(ToolChoiceMode::Auto)) => Ok(Self::Auto),
            Some(ResponsesToolChoice::Mode(ToolChoiceMode::None)) => Ok(Self::None),
            Some(ResponsesToolChoice::Mode(ToolChoiceMode::Required)) => Ok(Self::Required),
            Some(ResponsesToolChoice::Mode(ToolChoiceMode::Unsupported)) => Err(
                ProviderRejection::invalid("tool_choice", "unsupported tool_choice mode"),
            ),
            Some(ResponsesToolChoice::Named(choice))
                if matches!(choice.kind, FunctionKind::Function) =>
            {
                let name = choice.name.filter(|name| !name.is_empty()).ok_or_else(|| {
                    ProviderRejection::invalid("tool_choice", "function is missing its name")
                })?;
                Ok(Self::Named(name))
            }
            Some(ResponsesToolChoice::Named(_)) => Err(ProviderRejection::unsupported(
                "tool_choice",
                "only named function choices are supported",
            )),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct NamedResponsesToolChoice {
    #[serde(rename = "type")]
    kind: FunctionKind,
    name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ResponsesTextConfig {
    pub(super) format: Option<ResponsesTextFormat>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ResponsesTextFormat {
    Text,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct ChatCompletionResponse {
    id: String,
    object: &'static str,
    created: u64,
    model: ModelId,
    choices: Vec<ChatChoice>,
    usage: ChatUsage,
}

impl From<CompatTurnResponse> for ChatCompletionResponse {
    fn from(response: CompatTurnResponse) -> Self {
        let (message, finish_reason) = match response.output {
            CompatOutput::Text(text) => (
                AssistantMessage {
                    role: AssistantRole::Assistant,
                    content: Some(text),
                    tool_calls: Vec::new(),
                },
                FinishReason::Stop,
            ),
            CompatOutput::ToolCall(call) => (
                AssistantMessage {
                    role: AssistantRole::Assistant,
                    content: None,
                    tool_calls: vec![AssistantToolCall::from_call(
                        &call,
                        format!("call_{}", Uuid::now_v7().simple()),
                    )],
                },
                FinishReason::ToolCalls,
            ),
        };
        Self {
            id: format!("chatcmpl-{}", Uuid::now_v7().simple()),
            object: "chat.completion",
            created: unix_timestamp(),
            model: response.model,
            choices: vec![ChatChoice {
                index: 0,
                message,
                finish_reason,
            }],
            usage: response.usage.into(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatChoice {
    index: usize,
    message: AssistantMessage,
    finish_reason: FinishReason,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct AssistantMessage {
    role: AssistantRole,
    content: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<AssistantToolCall>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(super) struct AssistantToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: FunctionKind,
    function: AssistantFunctionCall,
}

impl AssistantToolCall {
    fn from_call(call: &FunctionCall, id: String) -> Self {
        Self {
            id,
            kind: FunctionKind::Function,
            function: AssistantFunctionCall {
                name: call.name.clone(),
                arguments: call.arguments.serialized(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(super) struct AssistantFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[expect(
    clippy::struct_field_names,
    reason = "field names must match the OpenAI usage contract"
)]
pub(super) struct ChatUsage {
    prompt_tokens: usize,
    completion_tokens: usize,
    total_tokens: usize,
}

impl From<TokenUsage> for ChatUsage {
    fn from(usage: TokenUsage) -> Self {
        Self {
            prompt_tokens: usage.prompt,
            completion_tokens: usage.completion,
            total_tokens: usage.total,
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatChunk<'a> {
    pub(super) id: &'a str,
    pub(super) object: &'static str,
    pub(super) created: u64,
    pub(super) model: &'a str,
    pub(super) choices: Vec<ChatChunkChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) usage: Option<ChatUsage>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatChunkChoice {
    pub(super) index: usize,
    pub(super) delta: ChatDelta,
    pub(super) finish_reason: Option<FinishReason>,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub(super) struct ChatDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) role: Option<AssistantRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) content: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) tool_calls: Vec<ChatToolCallDelta>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatToolCallDelta {
    pub(super) index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) id: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub(super) kind: Option<FunctionKind>,
    pub(super) function: FunctionCallDelta,
}

#[derive(Debug, Default, Serialize, JsonSchema)]
pub(super) struct FunctionCallDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) arguments: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(crate) struct ResponsesEnvelope {
    pub(super) id: String,
    pub(super) object: &'static str,
    pub(super) created_at: u64,
    pub(super) status: ResponseStatus,
    pub(super) error: Option<OpenAiFailureBody>,
    pub(super) incomplete_details: Option<String>,
    pub(super) model: ModelId,
    pub(super) output: Vec<ResponsesOutput>,
    pub(super) output_text: String,
    pub(super) usage: ResponsesUsage,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResponseStatus {
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ResponsesOutput {
    Message {
        id: String,
        status: ResponseStatus,
        role: AssistantRole,
        content: Vec<OutputText>,
    },
    FunctionCall {
        id: String,
        call_id: String,
        name: String,
        arguments: String,
        status: ResponseStatus,
    },
}

pub(super) struct ResponseIds {
    pub(super) item: String,
    pub(super) call: String,
}

impl ResponsesOutput {
    pub(super) fn from_compat(
        output: &CompatOutput,
        ids: ResponseIds,
        status: ResponseStatus,
    ) -> Self {
        match output {
            CompatOutput::Text(text) => Self::Message {
                id: ids.item,
                status,
                role: AssistantRole::Assistant,
                content: vec![OutputText {
                    kind: OutputTextKind::OutputText,
                    text: text.clone(),
                    annotations: Vec::new(),
                    logprobs: Vec::new(),
                }],
            },
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

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(super) struct OutputText {
    #[serde(rename = "type")]
    kind: OutputTextKind,
    text: String,
    annotations: Vec<String>,
    logprobs: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum OutputTextKind {
    OutputText,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub(super) struct InputTokenDetails {
    cached_tokens: usize,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub(super) struct OutputTokenDetails {
    reasoning_tokens: usize,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub(super) struct ResponsesUsage {
    input_tokens: usize,
    input_tokens_details: InputTokenDetails,
    output_tokens: usize,
    output_tokens_details: OutputTokenDetails,
    total_tokens: usize,
}

impl From<TokenUsage> for ResponsesUsage {
    fn from(usage: TokenUsage) -> Self {
        Self {
            input_tokens: usage.prompt,
            input_tokens_details: InputTokenDetails { cached_tokens: 0 },
            output_tokens: usage.completion,
            output_tokens_details: OutputTokenDetails {
                reasoning_tokens: 0,
            },
            total_tokens: usage.total,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ResponseProgress {
    pub(super) id: String,
    pub(super) object: &'static str,
    pub(super) created_at: u64,
    pub(super) status: ResponseStatus,
    pub(super) model: ModelId,
    pub(super) output: Vec<ResponsesOutput>,
    pub(super) error: Option<OpenAiFailureBody>,
    pub(super) incomplete_details: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(super) enum ResponsesStreamEvent {
    #[serde(rename = "response.created")]
    Created {
        sequence_number: usize,
        response: ResponseProgress,
    },
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded {
        sequence_number: usize,
        output_index: usize,
        item: ResponsesOutput,
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
        item: ResponsesOutput,
    },
    #[serde(rename = "response.completed")]
    Completed {
        sequence_number: usize,
        response: ResponsesEnvelope,
    },
}

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

#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum OpenAiErrorKind {
    InvalidRequestError,
    UnsupportedRequestError,
    AuthenticationError,
    RequestTooLarge,
    ServerError,
}

impl From<ProviderRejectionKind> for OpenAiErrorKind {
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

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(super) struct OpenAiFailureBody {
    message: String,
    #[serde(rename = "type")]
    kind: OpenAiErrorKind,
    param: Option<&'static str>,
    code: Option<&'static str>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct OpenAiFailureResponse {
    error: OpenAiFailureBody,
}

#[derive(derive_more::From)]
pub(super) struct OpenAiRejection(ProviderRejection);

impl IntoResponse for OpenAiRejection {
    fn into_response(self) -> Response {
        let rejection = self.0;
        (
            rejection.status,
            Json(OpenAiFailureResponse {
                error: OpenAiFailureBody {
                    message: rejection.message,
                    kind: rejection.kind.into(),
                    param: rejection.param,
                    code: None,
                },
            }),
        )
            .into_response()
    }
}

pub(super) fn response_output_text(response: &CompatTurnResponse) -> String {
    match &response.output {
        CompatOutput::Text(text) => text.clone(),
        CompatOutput::ToolCall(_) => String::new(),
    }
}
