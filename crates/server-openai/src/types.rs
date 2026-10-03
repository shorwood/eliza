//! `OpenAI` wire contracts shared by its route adapters.
//! Empty metadata on documented newtype fields avoids a `schemars` 0.9 derive collision.
use eliza_http::model::ModelId;
use eliza_http::response::unix_timestamp;
use eliza_modality_chat as chat;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::errors::{OpenAiError, OpenAiFailureBody};

// -----------------------------------------------------------------------------
// Tool: Defines shared function and selection primitives.
// -----------------------------------------------------------------------------

/// Tool kind understood by the `OpenAI` compatibility adapter.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum ToolFunctionKind {
    /// Function tool or function call.
    Function,
    /// Any non-function extension kind.
    #[serde(other)]
    Unsupported,
}

/// Named tool-selection modes accepted by `OpenAI` APIs.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum ToolChoiceMode {
    /// Let the model decide whether to call a tool.
    Auto,
    /// Prevent tool calls.
    None,
    /// Require a tool call.
    Required,
    /// Any mode outside the supported subset.
    #[serde(other)]
    Unsupported,
}

/// Encoded or structured forms accepted for function arguments.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ToolFunctionArguments {
    /// JSON encoded as a string.
    Encoded(
        /// Encoded JSON object.
        #[schemars(title = "", description = "")]
        String,
    ),
    /// Already-decoded JSON object.
    Object(
        /// Structured arguments.
        #[schemars(title = "", description = "")]
        chat::json::JsonObject,
    ),
}

impl ToolFunctionArguments {
    /// Convert encoded or structured arguments into a validated JSON object.
    ///
    /// # Errors
    ///
    /// Returns [`OpenAiError`] when encoded arguments are not a JSON object.
    pub(super) fn into_object(
        self,
        param: &'static str,
    ) -> Result<chat::json::JsonObject, OpenAiError> {
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

/// Role emitted for assistant responses.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum AssistantRole {
    /// Model-authored assistant output.
    Assistant,
}

// -----------------------------------------------------------------------------
// ImageDetail: Accepts documented OpenAI image-detail hints.
// -----------------------------------------------------------------------------

/// Documented `OpenAI` image-detail hint accepted as a deterministic no-op.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum ImageDetail {
    /// Let the provider choose detail automatically.
    Auto,
    /// Request low image detail.
    Low,
    /// Request high image detail.
    High,
    /// Request original image detail.
    Original,
}

// -----------------------------------------------------------------------------
// Chat: Defines Chat Completions content shapes.
// -----------------------------------------------------------------------------

/// URL object nested inside a Chat Completions image part.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatImageUrl {
    /// Data or HTTP(S) image URL.
    pub(super) url: Option<String>,
    /// Accepted detail hint with no effect on deterministic sampling.
    pub(super) detail: Option<ImageDetail>,
}

/// One structured Chat Completions content part.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ChatContentPart {
    /// Plain text content.
    Text {
        /// Text carried by the part.
        text: String,
    },
    /// User-provided image URL.
    ImageUrl {
        /// URL and optional detail hint.
        image_url: Option<ChatImageUrl>,
    },
    /// Any content-part kind outside the supported subset.
    #[serde(other)]
    Unsupported,
}

/// Accepted wire shapes for Chat Completions message content.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ChatContent {
    /// Shorthand plain-text content.
    Text(
        /// Message text.
        #[schemars(title = "", description = "")]
        String,
    ),
    /// Structured content parts.
    Parts(
        /// Ordered content parts.
        #[schemars(title = "", description = "")]
        Vec<ChatContentPart>,
    ),
    /// Unsupported structured content retained for classification.
    Object(
        /// Structured content value.
        #[schemars(title = "", description = "")]
        chat::json::JsonObject,
    ),
}

// -----------------------------------------------------------------------------
// ChatTool: Defines Chat Completions functions and selection policy.
// -----------------------------------------------------------------------------

/// Function invocation nested inside a Chat Completions tool call.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatToolFunction {
    /// Function name requested by the model.
    name: Option<String>,
    /// Arguments supplied to the function.
    arguments: Option<ToolFunctionArguments>,
}

impl ChatToolFunction {
    /// Lower required function fields into the neutral function contract.
    ///
    /// # Errors
    ///
    /// Returns [`OpenAiError`] when the name or arguments are absent or the
    /// arguments are not a JSON object.
    fn lower(self, param: &'static str) -> Result<chat::turn::FunctionCall, OpenAiError> {
        let name = self
            .name
            .filter(|name| !name.is_empty())
            .ok_or(OpenAiError::MissingFunctionName { param })?;
        let arguments = self
            .arguments
            .ok_or(OpenAiError::MissingToolCallArguments { param })?;
        Ok(chat::turn::FunctionCall {
            name,
            arguments: arguments.into_object(param)?,
        })
    }
}

/// One Chat Completions tool call.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatToolCall {
    /// Tool-call discriminator.
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    /// Function invocation payload.
    function: Option<ChatToolFunction>,
}

impl ChatToolCall {
    /// Lower one chat tool call into the neutral function contract.
    ///
    /// # Errors
    ///
    /// Returns [`OpenAiError`] for a non-function call or missing function data.
    pub(super) fn lower(
        self,
        param: &'static str,
    ) -> Result<chat::turn::FunctionCall, OpenAiError> {
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

/// Function definition nested inside a Chat Completions tool.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(super) struct ChatToolDefinition {
    /// Function name exposed to the model.
    name: Option<String>,
    /// Optional human-readable function description.
    description: Option<String>,
    /// JSON Schema describing accepted parameters.
    parameters: Option<chat::json::JsonObject>,
}

impl ChatToolDefinition {
    /// Count definition characters for deterministic token accounting.
    fn char_count(&self) -> usize {
        self.name.as_ref().map_or(0, String::len)
            + self.description.as_ref().map_or(0, String::len)
            + self
                .parameters
                .as_ref()
                .map_or(0, |parameters| parameters.serialized().chars().count())
    }
}

/// Tool declaration accepted by Chat Completions.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(super) struct ChatTool {
    /// Tool-kind discriminator.
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    /// Function definition for function tools.
    function: Option<ChatToolDefinition>,
}

impl ChatTool {
    /// Count tool-definition characters for deterministic token accounting.
    fn char_count(&self) -> usize {
        let function = self.function.as_ref();
        function.map_or(0, ChatToolDefinition::char_count) + 8
    }
}

/// List of tools offered with a Chat Completions request.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(transparent)]
pub(super) struct ChatToolList(
    /// Tool declarations in request order.
    Vec<ChatTool>,
);

impl TryFrom<ChatToolList> for Vec<chat::turn::FunctionTool> {
    type Error = OpenAiError;

    fn try_from(tools: ChatToolList) -> Result<Self, Self::Error> {
        tools
            .0
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
                Ok(chat::turn::FunctionTool::new(name, definition_chars))
            })
            .collect()
    }
}

/// Function selector nested inside a named Chat Completions tool choice.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatToolNamedFunction {
    /// Selected function name.
    name: String,
}

/// Named Chat Completions tool-choice payload.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatToolNamedChoice {
    /// Tool-kind discriminator.
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    /// Selected function.
    function: ChatToolNamedFunction,
}

/// Named or mode-based Chat Completions tool-selection policy.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ChatToolChoice {
    /// General selection mode.
    Mode(
        /// Requested mode.
        #[schemars(title = "", description = "")]
        ToolChoiceMode,
    ),
    /// One required named function.
    Named(
        /// Named selection payload.
        #[schemars(title = "", description = "")]
        ChatToolNamedChoice,
    ),
}

impl TryFrom<ChatToolChoice> for chat::turn::ToolChoice {
    type Error = OpenAiError;

    fn try_from(choice: ChatToolChoice) -> Result<Self, Self::Error> {
        match choice {
            ChatToolChoice::Mode(ToolChoiceMode::Auto) => Ok(Self::Auto),
            ChatToolChoice::Mode(ToolChoiceMode::None) => Ok(Self::None),
            ChatToolChoice::Mode(ToolChoiceMode::Required) => Ok(Self::Required),
            ChatToolChoice::Mode(ToolChoiceMode::Unsupported) => {
                Err(OpenAiError::UnsupportedToolChoiceMode)
            }
            ChatToolChoice::Named(choice) if matches!(choice.kind, ToolFunctionKind::Function) => {
                Ok(chat::turn::ToolChoice::Named(choice.function.name))
            }
            ChatToolChoice::Named(_) => Err(OpenAiError::UnsupportedNamedToolChoice),
        }
    }
}

// -----------------------------------------------------------------------------
// ChatRequest: Defines the Chat Completions request envelope.
// -----------------------------------------------------------------------------

/// Named JSON Schema payload nested inside Chat Completions formatting.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct ChatRequestJsonSchema {
    /// Nonblank schema identity required by the `OpenAI` contract.
    name: Option<String>,
    /// Optional schema description accepted without changing local output.
    _description: Option<String>,
    /// JSON Schema compiled into the local deterministic witness.
    schema: Option<chat::json::JsonObject>,
    /// Strictness hint accepted as a no-op because local output is always exact.
    _strict: Option<bool>,
}

/// Role attached to an inbound Chat Completions message.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum ChatRequestRole {
    /// System instruction.
    System,
    /// Developer instruction.
    Developer,
    /// Human-authored input.
    User,
    /// Model-authored output.
    Assistant,
    /// Result from a prior tool call.
    Tool,
    /// Any role outside the supported subset.
    #[serde(other)]
    Unsupported,
}

/// One inbound Chat Completions message.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatRequestMessage {
    /// Message author role.
    pub(super) role: ChatRequestRole,
    /// Optional message payload.
    pub(super) content: Option<ChatContent>,
    /// Tool calls requested by an assistant message.
    pub(super) tool_calls: Option<Vec<ChatToolCall>>,
}

/// Optional controls for Chat Completions streaming.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
pub(super) struct ChatRequestStreamOptions {
    /// Whether the terminal stream chunk includes usage.
    #[serde(rename = "include_usage")]
    pub(super) should_include_usage: Option<bool>,
}

/// Text-output format requested from Chat Completions.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ChatRequestResponseFormat {
    /// Preserve normal plain-text output.
    Text,
    /// Wrap final text in the deterministic JSON object.
    JsonObject,
    /// Conform final text to a supplied JSON Schema.
    JsonSchema {
        /// Nested schema metadata and definition.
        json_schema: Option<ChatRequestJsonSchema>,
    },
    /// Any future or unknown output format.
    #[serde(other)]
    Unsupported,
}

/// Chat Completions request accepted by the adapter.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ChatRequest {
    /// Requested model identifier.
    pub(super) model: ModelId,
    /// Conversation history in request order.
    pub(super) messages: Vec<ChatRequestMessage>,
    /// Whether to use the event-stream response shape.
    #[serde(rename = "stream")]
    pub(super) should_stream: Option<bool>,
    /// Optional stream-specific controls.
    pub(super) stream_options: Option<ChatRequestStreamOptions>,
    /// Functions offered to the model.
    pub(super) tools: Option<ChatToolList>,
    /// Tool-selection policy.
    pub(super) tool_choice: Option<ChatToolChoice>,
    /// Optional plain-text, JSON-object, or JSON-Schema output control.
    pub(super) response_format: Option<ChatRequestResponseFormat>,
    /// Reasoning effort accepted for native compatibility without trace output.
    pub(super) reasoning_effort: Option<ReasoningEffort>,
}

/// Validate and compile Chat Completions' nested schema payload.
///
/// # Errors
///
/// Returns a typed `OpenAI` error for missing metadata or an invalid schema.
fn compile_chat_schema(
    definition: Option<ChatRequestJsonSchema>,
) -> Result<chat::structured_output::StructuredOutput, OpenAiError> {
    let definition = definition.ok_or(OpenAiError::InvalidResponseFormat {
        param: "response_format",
        reason: "json_schema is required",
    })?;
    validate_schema_name(definition.name, "response_format")?;
    let schema = definition
        .schema
        .ok_or(OpenAiError::InvalidResponseFormat {
            param: "response_format",
            reason: "json_schema.schema is required",
        })?;
    compile_schema(schema, "response_format")
}

impl TryFrom<ChatRequestResponseFormat> for chat::structured_output::StructuredOutput {
    type Error = OpenAiError;

    fn try_from(format: ChatRequestResponseFormat) -> Result<Self, Self::Error> {
        match format {
            ChatRequestResponseFormat::Text => Ok(Self::default()),
            ChatRequestResponseFormat::JsonObject => Ok(Self::json_object()),
            ChatRequestResponseFormat::JsonSchema { json_schema } => {
                compile_chat_schema(json_schema)
            }
            ChatRequestResponseFormat::Unsupported => Err(OpenAiError::UnsupportedResponseFormat {
                param: "response_format",
            }),
        }
    }
}

// -----------------------------------------------------------------------------
// ResponsesRequestContent: Defines Responses input content shapes.
// -----------------------------------------------------------------------------

/// One structured content part accepted by the Responses API.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ResponsesRequestContentPart {
    /// User-provided input text.
    InputText {
        /// Text carried by the part.
        text: String,
    },
    /// Prior assistant output text.
    OutputText {
        /// Text carried by the part.
        text: String,
    },
    /// Generic text compatibility shape.
    Text {
        /// Text carried by the part.
        text: String,
    },
    /// User-provided image URL or uploaded-file reference.
    InputImage {
        /// Data or HTTP(S) image URL.
        image_url: Option<String>,
        /// Provider-owned uploaded file identifier.
        file_id: Option<String>,
        /// Accepted detail hint with no effect on deterministic sampling.
        detail: Option<ImageDetail>,
    },
    /// Any content-part kind outside the supported subset.
    #[serde(other)]
    Unsupported,
}

/// Text or structured parts accepted as Responses message content.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ResponsesRequestContent {
    /// Shorthand plain-text content.
    Text(
        /// Message text.
        #[schemars(title = "", description = "")]
        String,
    ),
    /// Structured content parts.
    Parts(
        /// Ordered content parts.
        #[schemars(title = "", description = "")]
        Vec<ResponsesRequestContentPart>,
    ),
}

// -----------------------------------------------------------------------------
// ResponsesRequestToolOutput: Defines returned function values.
// -----------------------------------------------------------------------------

/// Text or structured JSON accepted as a Responses function result.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ResponsesRequestToolOutput {
    /// Plain-text function result.
    Text(
        /// Function-result text.
        #[schemars(title = "", description = "")]
        String,
    ),
    /// Structured JSON function result.
    Object(
        /// Function-result object.
        #[schemars(title = "", description = "")]
        chat::json::JsonObject,
    ),
}

// -----------------------------------------------------------------------------
// ResponsesRequestRole: Defines inbound message authors.
// -----------------------------------------------------------------------------

/// Role attached to an inbound Responses message item.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum ResponsesRequestRole {
    /// Human-authored input.
    User,
    /// System instruction.
    System,
    /// Developer instruction.
    Developer,
    /// Prior model-authored output.
    Assistant,
    /// Any role outside the supported subset.
    #[serde(other)]
    Unsupported,
}

// -----------------------------------------------------------------------------
// ResponsesRequestInput: Defines structured and shorthand input.
// -----------------------------------------------------------------------------

/// One prior reasoning summary part accepted in Responses history.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ResponsesRequestInputReasoningSummary {
    /// Readable summary text.
    SummaryText {
        /// Summary payload.
        text: String,
    },
    /// Any summary part outside the current contract.
    #[serde(other)]
    Unsupported,
}

/// One structured item accepted by the Responses API.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ResponsesRequestInputItem {
    /// Conversation message.
    Message {
        /// Message author role.
        role: ResponsesRequestRole,
        /// Message payload.
        content: ResponsesRequestContent,
    },
    /// Function invocation from a prior model response.
    FunctionCall {
        /// Requested function name.
        name: Option<String>,
        /// Arguments supplied to the function.
        arguments: Option<ToolFunctionArguments>,
    },
    /// Client-provided result from a prior function call.
    FunctionCallOutput {
        /// Returned function value.
        output: Option<ResponsesRequestToolOutput>,
    },
    /// Prior reasoning output returned unchanged by a client.
    Reasoning {
        /// Prior readable summary parts.
        summary: Option<Vec<ResponsesRequestInputReasoningSummary>>,
        /// Prior opaque reasoning token.
        encrypted_content: Option<String>,
    },
    /// Any input-item kind outside the supported subset.
    #[serde(other)]
    Unsupported,
}

/// Shorthand text or structured items accepted as Responses input.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ResponsesRequestInput {
    /// Shorthand plain-text input.
    Text(
        /// Input text.
        #[schemars(title = "", description = "")]
        String,
    ),
    /// Structured input items.
    Items(
        /// Ordered input items.
        #[schemars(title = "", description = "")]
        Vec<ResponsesRequestInputItem>,
    ),
}

// -----------------------------------------------------------------------------
// ResponsesRequestTool: Defines offered functions.
// -----------------------------------------------------------------------------

/// Function tool offered through the Responses API.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub(super) struct ResponsesRequestTool {
    /// Tool-kind discriminator.
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    /// Function name exposed to the model.
    name: Option<String>,
    /// Optional human-readable function description.
    description: Option<String>,
    /// JSON Schema describing accepted parameters.
    parameters: Option<chat::json::JsonObject>,
}

impl ResponsesRequestTool {
    /// Count tool-definition characters for deterministic token accounting.
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

/// List of tools offered with a Responses request.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(transparent)]
pub(super) struct ResponsesRequestToolList(
    /// Tool declarations in request order.
    Vec<ResponsesRequestTool>,
);

impl TryFrom<ResponsesRequestToolList> for Vec<chat::turn::FunctionTool> {
    type Error = OpenAiError;

    fn try_from(tools: ResponsesRequestToolList) -> Result<Self, Self::Error> {
        tools
            .0
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
                Ok(chat::turn::FunctionTool::new(name, definition_chars))
            })
            .collect()
    }
}

// -----------------------------------------------------------------------------
// ResponsesRequestNamedToolChoice: Defines named function selection.
// -----------------------------------------------------------------------------

/// Named function selected by a Responses request.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ResponsesRequestNamedToolChoice {
    /// Tool-kind discriminator.
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    /// Selected function name.
    name: Option<String>,
}

// -----------------------------------------------------------------------------
// ResponsesRequestToolChoice: Defines general function selection.
// -----------------------------------------------------------------------------

/// Named or mode-based Responses tool-selection policy.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ResponsesRequestToolChoice {
    /// General selection mode.
    Mode(
        /// Requested mode.
        #[schemars(title = "", description = "")]
        ToolChoiceMode,
    ),
    /// One required named function.
    Named(
        /// Named selection payload.
        #[schemars(title = "", description = "")]
        ResponsesRequestNamedToolChoice,
    ),
}

impl TryFrom<ResponsesRequestToolChoice> for chat::turn::ToolChoice {
    type Error = OpenAiError;

    fn try_from(choice: ResponsesRequestToolChoice) -> Result<Self, Self::Error> {
        match choice {
            ResponsesRequestToolChoice::Mode(ToolChoiceMode::Auto) => Ok(Self::Auto),
            ResponsesRequestToolChoice::Mode(ToolChoiceMode::None) => Ok(Self::None),
            ResponsesRequestToolChoice::Mode(ToolChoiceMode::Required) => Ok(Self::Required),
            ResponsesRequestToolChoice::Mode(ToolChoiceMode::Unsupported) => {
                Err(OpenAiError::UnsupportedToolChoiceMode)
            }
            ResponsesRequestToolChoice::Named(choice)
                if matches!(choice.kind, ToolFunctionKind::Function) =>
            {
                let name = choice.name.filter(|name| !name.is_empty()).ok_or(
                    OpenAiError::MissingFunctionName {
                        param: "tool_choice",
                    },
                )?;
                Ok(chat::turn::ToolChoice::Named(name))
            }
            ResponsesRequestToolChoice::Named(_) => Err(OpenAiError::UnsupportedNamedToolChoice),
        }
    }
}

/// Require the nonblank schema name used by both `OpenAI` generation APIs.
///
/// # Errors
///
/// Returns an invalid-format error when the name is absent or blank.
fn validate_schema_name(name: Option<String>, param: &'static str) -> Result<(), OpenAiError> {
    if name.is_some_and(|name| !name.trim().is_empty()) {
        Ok(())
    } else {
        Err(OpenAiError::InvalidResponseFormat {
            param,
            reason: "a nonblank schema name is required",
        })
    }
}

/// Map shared compiler categories to stable `OpenAI` diagnostics.
///
/// # Errors
///
/// Returns the provider-specific form of a shared schema compiler error.
fn compile_schema(
    schema: chat::json::JsonObject,
    param: &'static str,
) -> Result<chat::structured_output::StructuredOutput, OpenAiError> {
    chat::structured_output::StructuredOutput::try_from(schema).map_err(|source| {
        match source.kind() {
            chat::structured_output::StructuredOutputErrorKind::Invalid => {
                OpenAiError::InvalidResponseSchema { param, source }
            }
            chat::structured_output::StructuredOutputErrorKind::Unsupported => {
                OpenAiError::UnsupportedResponseSchema { param, source }
            }
        }
    })
}

// -----------------------------------------------------------------------------
// ResponsesRequestText: Defines text-output controls.
// -----------------------------------------------------------------------------

/// Text-output format requested from the Responses API.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(super) enum ResponsesRequestTextFormat {
    /// Plain text output.
    Text,
    /// Wrap final text in the deterministic JSON object.
    JsonObject,
    /// Conform final text to a supplied JSON Schema.
    JsonSchema {
        /// Nonblank schema identity required by the `OpenAI` contract.
        name: Option<String>,
        /// Optional schema description accepted without changing local output.
        _description: Option<String>,
        /// JSON Schema compiled into the local deterministic witness.
        schema: Option<chat::json::JsonObject>,
        /// Strictness hint accepted as a no-op because local output is always exact.
        _strict: Option<bool>,
    },
    /// Any text format outside the supported subset.
    #[serde(other)]
    Unsupported,
}

impl TryFrom<ResponsesRequestTextFormat> for chat::structured_output::StructuredOutput {
    type Error = OpenAiError;

    fn try_from(format: ResponsesRequestTextFormat) -> Result<Self, Self::Error> {
        match format {
            ResponsesRequestTextFormat::Text => Ok(Self::default()),
            ResponsesRequestTextFormat::JsonObject => Ok(Self::json_object()),
            ResponsesRequestTextFormat::JsonSchema { name, schema, .. } => {
                validate_schema_name(name, "text.format")?;
                let schema = schema.ok_or(OpenAiError::InvalidResponseFormat {
                    param: "text.format",
                    reason: "schema is required",
                })?;
                compile_schema(schema, "text.format")
            }
            ResponsesRequestTextFormat::Unsupported => {
                Err(OpenAiError::UnsupportedResponseFormat {
                    param: "text.format",
                })
            }
        }
    }
}

/// Text-output configuration for a Responses request.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ResponsesRequestTextConfig {
    /// Requested text format.
    pub(super) format: Option<ResponsesRequestTextFormat>,
}

// -----------------------------------------------------------------------------
// Reasoning: Defines native reasoning request values.
// -----------------------------------------------------------------------------

/// `OpenAI` reasoning effort values accepted by current native APIs.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum ReasoningEffort {
    /// Disable reasoning effort.
    None,
    /// Minimal effort.
    Minimal,
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

impl ReasoningEffort {
    /// Reject values outside the current native enum.
    ///
    /// # Errors
    ///
    /// Returns a typed compatibility error for unknown effort values.
    pub(super) fn validate(self) -> Result<(), OpenAiError> {
        if matches!(self, Self::Unsupported) {
            Err(OpenAiError::UnsupportedReasoningEffort)
        } else {
            Ok(())
        }
    }
}

/// `OpenAI` readable-summary modes.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum ReasoningSummary {
    /// Provider-selected summary detail.
    Auto,
    /// Concise summary.
    Concise,
    /// Detailed summary.
    Detailed,
    /// Any summary mode outside the current contract.
    #[serde(other)]
    Unsupported,
}

// -----------------------------------------------------------------------------
// ResponsesReasoningConfig: Defines Responses API reasoning controls.
// -----------------------------------------------------------------------------

/// Responses reasoning configuration.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ResponsesReasoningConfig {
    /// Requested effort, accepted without changing ELIZA.
    effort: Option<ReasoningEffort>,
    /// Current readable-summary selector.
    summary: Option<ReasoningSummary>,
    /// Deprecated readable-summary alias.
    generate_summary: Option<ReasoningSummary>,
}

impl ResponsesReasoningConfig {
    /// Validate controls and report whether a summary is readable.
    ///
    /// # Errors
    ///
    /// Returns a typed error for unknown effort or summary values.
    fn should_include_reasoning(&self) -> Result<bool, OpenAiError> {
        if let Some(effort) = self.effort {
            effort.validate()?;
        }

        let unsupported_summary = matches!(self.summary, Some(ReasoningSummary::Unsupported))
            || matches!(self.generate_summary, Some(ReasoningSummary::Unsupported));
        if unsupported_summary {
            Err(OpenAiError::UnsupportedReasoningSummary)
        } else {
            Ok(self.summary.is_some() || self.generate_summary.is_some())
        }
    }
}

// -----------------------------------------------------------------------------
// ResponsesRequest: Defines the request envelope.
// -----------------------------------------------------------------------------

/// Responses API request accepted by the adapter.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ResponsesRequest {
    /// Requested model identifier.
    pub(super) model: ModelId,
    /// Input text or structured items.
    pub(super) input: ResponsesRequestInput,
    /// Optional system-like instructions.
    pub(super) instructions: Option<ResponsesRequestContent>,
    /// Whether to use the event-stream response shape.
    #[serde(rename = "stream")]
    pub(super) should_stream: Option<bool>,
    /// Functions offered to the model.
    pub(super) tools: Option<ResponsesRequestToolList>,
    /// Tool-selection policy.
    pub(super) tool_choice: Option<ResponsesRequestToolChoice>,
    /// Requested text-output controls.
    pub(super) text: Option<ResponsesRequestTextConfig>,
    /// Native reasoning controls.
    reasoning: Option<ResponsesReasoningConfig>,
    /// Maximum output-token allowance accepted for compatibility.
    max_output_tokens: Option<usize>,
}

impl ResponsesRequest {
    /// Validate generation limits and return summary visibility.
    ///
    /// # Errors
    ///
    /// Returns a typed error for invalid reasoning or output-token controls.
    pub(super) fn should_include_reasoning(&self) -> Result<bool, OpenAiError> {
        if self.max_output_tokens == Some(0) {
            Err(OpenAiError::InvalidMaxOutputTokens)
        } else if let Some(reasoning) = &self.reasoning {
            reasoning.should_include_reasoning()
        } else {
            Ok(false)
        }
    }
}

// -----------------------------------------------------------------------------
// ChatResponse: Defines complete Chat Completions responses.
// -----------------------------------------------------------------------------

/// Reason a Chat Completions choice stopped generating.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum ChatResponseFinishReason {
    /// Text generation completed normally.
    Stop,
    /// The assistant requested one or more tool calls.
    ToolCalls,
}

/// Function payload emitted by a Chat Completions tool call.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(super) struct ChatResponseFunctionCall {
    /// Requested function name.
    name: String,
    /// Arguments encoded as JSON text.
    arguments: String,
}

/// Tool call emitted by a Chat Completions response.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(super) struct ChatResponseToolCall {
    /// Provider-shaped tool-call identifier.
    id: String,
    /// Tool-kind discriminator.
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    /// Function invocation payload.
    function: ChatResponseFunctionCall,
}

impl ChatResponseToolCall {
    /// Build a provider-shaped tool call from its canonical representation.
    fn from_call(call: &chat::turn::FunctionCall, id: String) -> Self {
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

/// Assistant message emitted by Chat Completions.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatResponseMessage {
    /// Fixed assistant role.
    role: AssistantRole,
    /// Generated text, absent for tool calls.
    content: Option<String>,
    /// Generated tool calls.
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

impl From<chat::turn::FunctionCall> for ChatResponseMessage {
    fn from(call: chat::turn::FunctionCall) -> Self {
        let id = format!("call_{}", Uuid::now_v7().simple());
        Self {
            role: AssistantRole::Assistant,
            content: None,
            tool_calls: vec![ChatResponseToolCall::from_call(&call, id)],
        }
    }
}

/// One choice in a Chat Completions response.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatResponseChoice {
    /// Zero-based choice position.
    index: usize,
    /// Generated assistant message.
    message: ChatResponseMessage,
    /// Reason this choice stopped generating.
    finish_reason: ChatResponseFinishReason,
}

/// Token counts serialized by Chat Completions.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[expect(
    clippy::struct_field_names,
    reason = "field names must match the OpenAI usage contract"
)]
pub(super) struct ChatResponseUsage {
    /// Estimated tokens consumed by request input.
    prompt_tokens: usize,
    /// Estimated tokens emitted by the response.
    completion_tokens: usize,
    /// Combined prompt and completion token count.
    total_tokens: usize,
}

impl From<chat::turn::Usage> for ChatResponseUsage {
    fn from(usage: chat::turn::Usage) -> Self {
        Self {
            prompt_tokens: usage.prompt,
            completion_tokens: usage.completion,
            total_tokens: usage.total,
        }
    }
}

/// Complete Chat Completions response.
#[derive(Debug, Serialize, JsonSchema)]
pub(crate) struct ChatResponse {
    /// Provider-shaped completion identifier.
    id: String,
    /// Wire object discriminator.
    object: &'static str,
    /// Unix timestamp when the response was created.
    created: u64,
    /// Model that produced the response.
    model: ModelId,
    /// Generated choices.
    choices: Vec<ChatResponseChoice>,
    /// Request and response token counts.
    usage: ChatResponseUsage,
}

impl ChatResponse {
    /// Attach the provider-owned model identity to one canonical result.
    pub(super) fn from_compat(model: ModelId, response: chat::turn::Response) -> Self {
        let (message, finish_reason) = match response.output {
            chat::turn::Output::Text(text) => (
                ChatResponseMessage::from(text),
                ChatResponseFinishReason::Stop,
            ),
            chat::turn::Output::ToolCall(call) => (
                ChatResponseMessage::from(call),
                ChatResponseFinishReason::ToolCalls,
            ),
        };
        Self {
            id: format!("chatcmpl-{}", Uuid::now_v7().simple()),
            object: "chat.completion",
            created: unix_timestamp(),
            model,
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

/// Incremental function-call fields in a Chat Completions stream.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub(super) struct ChatStreamFunctionDelta {
    /// Function name when first announced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) name: Option<String>,
    /// Newly emitted JSON argument fragment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) arguments: Option<String>,
}

/// Incremental tool call in a Chat Completions stream.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatStreamToolCallDelta {
    /// Zero-based tool-call position.
    pub(super) index: usize,
    /// Tool-call identifier when first announced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) id: Option<String>,
    /// Tool kind when first announced.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub(super) kind: Option<ToolFunctionKind>,
    /// Incremental function payload.
    pub(super) function: ChatStreamFunctionDelta,
}

/// Incremental assistant message in a Chat Completions stream.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub(super) struct ChatStreamDelta {
    /// Assistant role when first announced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) role: Option<AssistantRole>,
    /// Newly emitted text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) content: Option<String>,
    /// Incremental tool calls.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) tool_calls: Vec<ChatStreamToolCallDelta>,
}

/// One choice in a Chat Completions stream chunk.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatStreamChoice {
    /// Zero-based choice position.
    pub(super) index: usize,
    /// Newly emitted assistant fields.
    pub(super) delta: ChatStreamDelta,
    /// Stop reason, present only on the terminal choice chunk.
    pub(super) finish_reason: Option<ChatResponseFinishReason>,
}

/// One Chat Completions server-sent event payload.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatStreamChunk<'a> {
    /// Completion identifier shared by every chunk.
    pub(super) id: &'a str,
    /// Wire object discriminator.
    pub(super) object: &'static str,
    /// Unix timestamp when the stream was created.
    pub(super) created: u64,
    /// Model producing the stream.
    pub(super) model: &'a str,
    /// Incremental choices in this chunk.
    pub(super) choices: Vec<ChatStreamChoice>,
    /// Usage included only when requested on the terminal chunk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) usage: Option<ChatResponseUsage>,
}

// -----------------------------------------------------------------------------
// Responses: Defines complete and streaming Responses API output.
// -----------------------------------------------------------------------------

/// Lifecycle state of a Responses API response or output item.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResponsesResponseStatus {
    /// Generation remains active.
    InProgress,
    /// Generation completed successfully.
    Completed,
}

/// Stable identifiers shared by one Responses output item and function call.
pub(super) struct ResponsesResponseIds {
    /// Output-item identifier.
    pub(super) item: String,
    /// Function-call identifier.
    pub(super) call: String,
}

/// Content kind emitted inside a Responses message output.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResponsesResponseTextKind {
    /// Generated output text.
    OutputText,
}

/// Generated text content in a Responses message output.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(super) struct ResponsesResponseText {
    /// Content-kind discriminator.
    #[serde(rename = "type")]
    kind: ResponsesResponseTextKind,
    /// Generated text.
    text: String,
    /// Text annotations, empty in this emulator.
    annotations: Vec<String>,
    /// Token log probabilities, empty in this emulator.
    logprobs: Vec<String>,
}

impl From<&str> for ResponsesResponseText {
    fn from(text: &str) -> Self {
        Self {
            kind: ResponsesResponseTextKind::OutputText,
            text: text.to_owned(),
            annotations: Vec::new(),
            logprobs: Vec::new(),
        }
    }
}

/// One readable summary part inside a reasoning output item.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ResponsesReasoningSummary {
    /// Readable mechanical summary text.
    SummaryText {
        /// Summary payload.
        text: String,
    },
}

/// One output item emitted by the Responses API.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ResponsesResponseOutput {
    /// Readable reasoning summary requested by the client.
    Reasoning {
        /// Provider-shaped output-item identifier.
        id: String,
        /// Current item lifecycle state.
        status: ResponsesResponseStatus,
        /// Ordered readable summary parts.
        summary: Vec<ResponsesReasoningSummary>,
        /// Stable opaque fixture token over the trace.
        encrypted_content: String,
    },
    /// Assistant text message.
    Message {
        /// Provider-shaped output-item identifier.
        id: String,
        /// Current item lifecycle state.
        status: ResponsesResponseStatus,
        /// Fixed assistant role.
        role: AssistantRole,
        /// Generated message content.
        content: Vec<ResponsesResponseText>,
    },
    /// Assistant request to invoke a function.
    FunctionCall {
        /// Provider-shaped output-item identifier.
        id: String,
        /// Provider-shaped function-call identifier.
        call_id: String,
        /// Requested function name.
        name: String,
        /// Arguments encoded as JSON text.
        arguments: String,
        /// Current item lifecycle state.
        status: ResponsesResponseStatus,
    },
}

impl ResponsesResponseOutput {
    /// Build one provider-shaped output item from canonical model output.
    pub(super) fn from_compat(
        output: &chat::turn::Output,
        ids: ResponsesResponseIds,
        status: ResponsesResponseStatus,
    ) -> Self {
        match output {
            chat::turn::Output::Text(text) => {
                let content = vec![ResponsesResponseText::from(text.as_str())];
                Self::Message {
                    id: ids.item,
                    status,
                    role: AssistantRole::Assistant,
                    content,
                }
            }
            chat::turn::Output::ToolCall(call) => Self::FunctionCall {
                id: ids.item,
                call_id: ids.call,
                name: call.name.clone(),
                arguments: call.arguments.serialized(),
                status,
            },
        }
    }
}

/// Additional accounting for Responses input tokens.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub(super) struct ResponsesResponseInputTokenDetails {
    /// Input tokens served from a cache.
    cached_tokens: usize,
}

/// Additional accounting for Responses output tokens.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub(super) struct ResponsesResponseOutputTokenDetails {
    /// Output tokens attributed to hidden reasoning.
    reasoning_tokens: usize,
}

/// Token counts serialized by the Responses API.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub(super) struct ResponsesResponseUsage {
    /// Estimated tokens consumed by request input.
    input_tokens: usize,
    /// Additional input-token accounting.
    input_tokens_details: ResponsesResponseInputTokenDetails,
    /// Estimated tokens emitted by the response.
    output_tokens: usize,
    /// Additional output-token accounting.
    output_tokens_details: ResponsesResponseOutputTokenDetails,
    /// Combined input and output token count.
    total_tokens: usize,
}

impl From<chat::turn::Usage> for ResponsesResponseUsage {
    fn from(usage: chat::turn::Usage) -> Self {
        Self {
            input_tokens: usage.prompt,
            input_tokens_details: ResponsesResponseInputTokenDetails { cached_tokens: 0 },
            output_tokens: usage.completion + usage.reasoning,
            output_tokens_details: ResponsesResponseOutputTokenDetails {
                reasoning_tokens: usage.reasoning,
            },
            total_tokens: usage.total,
        }
    }
}

/// Complete Responses API response.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub(crate) struct ResponsesResponse {
    /// Provider-shaped response identifier.
    pub(super) id: String,
    /// Wire object discriminator.
    pub(super) object: &'static str,
    /// Unix timestamp when the response was created.
    pub(super) created_at: u64,
    /// Final response lifecycle state.
    pub(super) status: ResponsesResponseStatus,
    /// Provider error, absent for successful responses.
    pub(super) error: Option<OpenAiFailureBody>,
    /// Incompletion detail, absent for successful responses.
    pub(super) incomplete_details: Option<String>,
    /// Model that produced the response.
    pub(super) model: ModelId,
    /// Generated output items.
    pub(super) output: Vec<ResponsesResponseOutput>,
    /// Convenience concatenation of generated text.
    pub(super) output_text: String,
    /// Request and response token counts.
    pub(super) usage: ResponsesResponseUsage,
}

/// Non-terminal response snapshot carried by streaming events.
#[derive(Debug, Clone, Serialize)]
pub(super) struct ResponsesResponseProgress {
    /// Provider-shaped response identifier.
    pub(super) id: String,
    /// Wire object discriminator.
    pub(super) object: &'static str,
    /// Unix timestamp when the response was created.
    pub(super) created_at: u64,
    /// Current response lifecycle state.
    pub(super) status: ResponsesResponseStatus,
    /// Model producing the response.
    pub(super) model: ModelId,
    /// Output items announced so far.
    pub(super) output: Vec<ResponsesResponseOutput>,
    /// Provider error, absent while generation succeeds.
    pub(super) error: Option<OpenAiFailureBody>,
    /// Incompletion detail, absent while generation succeeds.
    pub(super) incomplete_details: Option<String>,
}

/// Responses API server-sent event payload.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub(super) enum ResponsesResponseStreamEvent {
    /// Opens a response stream.
    #[serde(rename = "response.created")]
    Created {
        /// Monotonic position in the event stream.
        sequence_number: usize,
        /// Initial response snapshot.
        response: ResponsesResponseProgress,
    },
    /// Announces a new output item.
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded {
        /// Monotonic position in the event stream.
        sequence_number: usize,
        /// Zero-based output-item position.
        output_index: usize,
        /// Initial output item.
        item: ResponsesResponseOutput,
    },
    /// Opens one reasoning summary part.
    #[serde(rename = "response.reasoning_summary_part.added")]
    ReasoningSummaryPartAdded {
        /// Monotonic position in the event stream.
        sequence_number: usize,
        /// Reasoning output-item identifier.
        item_id: String,
        /// Zero-based output-item position.
        output_index: usize,
        /// Zero-based summary-part position.
        summary_index: usize,
        /// Empty summary part opened by this event.
        part: ResponsesReasoningSummary,
    },
    /// Appends text to one reasoning summary.
    #[serde(rename = "response.reasoning_summary_text.delta")]
    ReasoningSummaryTextDelta {
        /// Monotonic position in the event stream.
        sequence_number: usize,
        /// Reasoning output-item identifier.
        item_id: String,
        /// Zero-based output-item position.
        output_index: usize,
        /// Zero-based summary-part position.
        summary_index: usize,
        /// Newly emitted summary text.
        delta: String,
    },
    /// Completes the text of one reasoning summary.
    #[serde(rename = "response.reasoning_summary_text.done")]
    ReasoningSummaryTextDone {
        /// Monotonic position in the event stream.
        sequence_number: usize,
        /// Reasoning output-item identifier.
        item_id: String,
        /// Zero-based output-item position.
        output_index: usize,
        /// Zero-based summary-part position.
        summary_index: usize,
        /// Complete summary text.
        text: String,
    },
    /// Completes one reasoning summary part.
    #[serde(rename = "response.reasoning_summary_part.done")]
    ReasoningSummaryPartDone {
        /// Monotonic position in the event stream.
        sequence_number: usize,
        /// Reasoning output-item identifier.
        item_id: String,
        /// Zero-based output-item position.
        output_index: usize,
        /// Zero-based summary-part position.
        summary_index: usize,
        /// Completed summary part.
        part: ResponsesReasoningSummary,
    },
    /// Appends generated text to a message item.
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta {
        /// Monotonic position in the event stream.
        sequence_number: usize,
        /// Output-item identifier.
        item_id: String,
        /// Zero-based output-item position.
        output_index: usize,
        /// Zero-based content-part position.
        content_index: usize,
        /// Newly emitted text.
        delta: String,
    },
    /// Completes generated text for a message item.
    #[serde(rename = "response.output_text.done")]
    OutputTextDone {
        /// Monotonic position in the event stream.
        sequence_number: usize,
        /// Output-item identifier.
        item_id: String,
        /// Zero-based output-item position.
        output_index: usize,
        /// Zero-based content-part position.
        content_index: usize,
        /// Complete generated text.
        text: String,
    },
    /// Appends JSON arguments to a function-call item.
    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionArgumentsDelta {
        /// Monotonic position in the event stream.
        sequence_number: usize,
        /// Output-item identifier.
        item_id: String,
        /// Zero-based output-item position.
        output_index: usize,
        /// Function-call identifier.
        call_id: String,
        /// Newly emitted JSON fragment.
        delta: String,
    },
    /// Completes JSON arguments for a function-call item.
    #[serde(rename = "response.function_call_arguments.done")]
    FunctionArgumentsDone {
        /// Monotonic position in the event stream.
        sequence_number: usize,
        /// Output-item identifier.
        item_id: String,
        /// Zero-based output-item position.
        output_index: usize,
        /// Function-call identifier.
        call_id: String,
        /// Requested function name.
        name: String,
        /// Complete arguments encoded as JSON text.
        arguments: String,
    },
    /// Completes one output item.
    #[serde(rename = "response.output_item.done")]
    OutputItemDone {
        /// Monotonic position in the event stream.
        sequence_number: usize,
        /// Zero-based output-item position.
        output_index: usize,
        /// Completed output item.
        item: ResponsesResponseOutput,
    },
    /// Completes the response stream.
    #[serde(rename = "response.completed")]
    Completed {
        /// Monotonic position in the event stream.
        sequence_number: usize,
        /// Final response snapshot.
        response: ResponsesResponse,
    },
}

// -----------------------------------------------------------------------------
// Model: Defines the OpenAI model catalog.
// -----------------------------------------------------------------------------

/// `OpenAI` model-catalog entry.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ModelDescriptor {
    /// Model identifier.
    pub(super) id: ModelId,
    /// Wire object discriminator.
    pub(super) object: &'static str,
    /// Unix timestamp associated with the model.
    pub(super) created: u64,
    /// Provider-compatible owner label.
    pub(super) owned_by: &'static str,
}

/// `OpenAI` model-list response.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ModelListResponse {
    /// Wire object discriminator.
    pub(super) object: &'static str,
    /// Available models.
    pub(super) data: Vec<ModelDescriptor>,
}
