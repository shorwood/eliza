//! Ollama wire contracts shared by its route adapters.
//! Empty metadata on documented newtype fields avoids a `schemars` 0.9 derive collision.
use eliza_http::model::ModelId;
use eliza_modality_chat as chat;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::errors::OllamaError;

// -----------------------------------------------------------------------------
// Message: Defines inbound message roles and content.
// -----------------------------------------------------------------------------

/// Role attached to an inbound Ollama message.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum MessageRole {
    /// System instruction.
    System,
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

/// Text or structured JSON accepted as Ollama message content.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum MessageContent {
    /// Plain-text content.
    Text(
        /// Message text.
        #[schemars(title = "", description = "")]
        String,
    ),
    /// Structured content serialized as JSON text during lowering.
    Object(
        /// Structured message value.
        #[schemars(title = "", description = "")]
        chat::json::JsonObject,
    ),
}

impl MessageContent {
    /// Convert optional wire content to its canonical text representation.
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

/// Tool kind understood by the Ollama compatibility adapter.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum ToolFunctionKind {
    /// Function tool or function call.
    Function,
    /// Any non-function extension kind.
    #[serde(other)]
    Unsupported,
}

/// Function payload nested inside an Ollama tool call.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ToolCalledFunction {
    /// Function name requested by the model.
    name: Option<String>,
    /// Arguments supplied to the function.
    arguments: Option<chat::json::JsonObject>,
}

impl ToolCalledFunction {
    /// Lower required call fields into the neutral function contract.
    ///
    /// # Errors
    ///
    /// Returns [`OllamaError`] when the function name is absent or empty.
    fn lower(self, param: &'static str) -> Result<chat::turn::FunctionCall, OllamaError> {
        let name = self
            .name
            .filter(|name| !name.is_empty())
            .ok_or(OllamaError::MissingToolCallName { param })?;

        // Missing arguments mean an empty object in Ollama's request shape.
        Ok(chat::turn::FunctionCall {
            name,
            arguments: self.arguments.unwrap_or_default(),
        })
    }
}

/// One Ollama tool call from an assistant message.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ToolCall {
    /// Tool-call discriminator.
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    /// Function invocation payload.
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
    ) -> Result<chat::turn::FunctionCall, OllamaError> {
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

/// Ollama function-tool declaration.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ToolFunctionDefinition {
    /// Function name exposed to the model.
    name: Option<String>,
    /// Optional human-readable function description.
    description: Option<String>,
    /// JSON Schema describing accepted parameters.
    parameters: Option<chat::json::JsonObject>,
}

impl ToolFunctionDefinition {
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

/// Ollama tool declaration.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct Tool {
    /// Tool-kind discriminator.
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    /// Function definition for function tools.
    function: Option<ToolFunctionDefinition>,
}

/// List of tools offered with an Ollama request.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(transparent)]
pub(super) struct ToolList(
    /// Tool declarations in request order.
    Vec<Tool>,
);

impl TryFrom<ToolList> for Vec<chat::turn::FunctionTool> {
    type Error = OllamaError;

    fn try_from(tools: ToolList) -> Result<Self, Self::Error> {
        tools
            .0
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
                Ok(chat::turn::FunctionTool::new(name, definition_chars))
            })
            .collect()
    }
}

/// Named Ollama tool-selection mode.
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

/// Function selector nested inside a named tool choice.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ToolNamedFunction {
    /// Selected function name.
    name: Option<String>,
}

/// Ollama named tool-choice payload.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ToolNamedChoice {
    /// Selected function.
    function: ToolNamedFunction,
}

/// Named or mode-based Ollama tool-selection policy.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum ToolChoice {
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
        ToolNamedChoice,
    ),
}

impl TryFrom<ToolChoice> for chat::turn::ToolChoice {
    type Error = OllamaError;

    fn try_from(choice: ToolChoice) -> Result<Self, Self::Error> {
        match choice {
            ToolChoice::Mode(ToolChoiceMode::Auto) => Ok(Self::Auto),
            ToolChoice::Mode(ToolChoiceMode::None) => Ok(Self::None),
            ToolChoice::Mode(ToolChoiceMode::Required) => Ok(Self::Required),
            ToolChoice::Mode(ToolChoiceMode::Unsupported) => {
                Err(OllamaError::UnsupportedToolChoiceMode)
            }
            ToolChoice::Named(choice) => choice
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

/// Named structured-output format accepted by Ollama.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum OutputFormatName {
    /// JSON object output.
    Json,
    /// Any named format outside the supported subset.
    #[serde(other)]
    Unsupported,
}

/// Named or schema-driven structured-output request.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum OutputFormat {
    /// Named output format.
    Name(
        /// Requested format name.
        #[schemars(title = "", description = "")]
        OutputFormatName,
    ),
    /// Explicit JSON Schema output contract.
    Schema(
        /// Requested output schema.
        #[schemars(title = "", description = "")]
        chat::json::JsonObject,
    ),
}

/// Compile an Ollama schema and retain its invalid-versus-unsupported class.
///
/// # Errors
///
/// Returns the provider-specific form of a shared schema compiler error.
fn compile_output_schema(
    schema: chat::json::JsonObject,
) -> Result<chat::structured_output::StructuredOutput, OllamaError> {
    chat::structured_output::StructuredOutput::try_from(schema).map_err(|source| {
        match source.kind() {
            chat::structured_output::StructuredOutputErrorKind::Invalid => {
                OllamaError::InvalidResponseSchema { source }
            }
            chat::structured_output::StructuredOutputErrorKind::Unsupported => {
                OllamaError::UnsupportedResponseSchema { source }
            }
        }
    })
}

impl TryFrom<OutputFormat> for chat::structured_output::StructuredOutput {
    type Error = OllamaError;

    fn try_from(format: OutputFormat) -> Result<Self, Self::Error> {
        match format {
            OutputFormat::Name(OutputFormatName::Json) => Ok(Self::json_object()),
            OutputFormat::Name(OutputFormatName::Unsupported) => {
                Err(OllamaError::UnsupportedResponseFormat)
            }
            OutputFormat::Schema(schema) => compile_output_schema(schema),
        }
    }
}

// -----------------------------------------------------------------------------
// Chat: Defines unary and streaming chat contracts.
// -----------------------------------------------------------------------------

/// One inbound Ollama chat message.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatMessage {
    /// Message author role.
    pub(super) role: MessageRole,
    /// Optional message payload.
    pub(super) content: Option<MessageContent>,
    /// Inline standard-base64 images attached to a user message.
    pub(super) images: Option<Vec<String>>,
    /// Tool calls requested by an assistant message.
    pub(super) tool_calls: Option<Vec<ToolCall>>,
}

/// Ollama chat request accepted by the adapter.
#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct ChatRequest {
    /// Requested model identifier.
    pub(super) model: ModelId,
    /// Conversation history in request order.
    pub(super) messages: Vec<ChatMessage>,
    /// Whether to use newline-delimited streaming responses.
    #[serde(rename = "stream")]
    pub(super) should_stream: Option<bool>,
    /// Functions offered to the model.
    pub(super) tools: Option<ToolList>,
    /// Tool-selection policy.
    pub(super) tool_choice: Option<ToolChoice>,
    /// Optional structured-output request.
    pub(super) format: Option<OutputFormat>,
    /// Whether reasoning output was requested.
    pub(super) think: Option<bool>,
}

/// Function payload emitted inside an Ollama tool call.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatOutputFunction {
    /// Requested function name.
    name: String,
    /// Arguments supplied to the function.
    arguments: chat::json::JsonObject,
}

/// Tool call emitted by an Ollama assistant message.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatOutputToolCall {
    /// Tool-kind discriminator.
    #[serde(rename = "type")]
    kind: ToolFunctionKind,
    /// Function invocation payload.
    function: ChatOutputFunction,
}

/// Assistant message emitted by the Ollama chat API.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatOutputMessage {
    /// Fixed assistant role.
    pub(super) role: &'static str,
    /// Generated text, empty for tool calls.
    pub(super) content: String,
    /// Generated tool calls.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) tool_calls: Vec<ChatOutputToolCall>,
}

impl From<&chat::turn::Output> for ChatOutputMessage {
    fn from(output: &chat::turn::Output) -> Self {
        match output {
            chat::turn::Output::Text(text) => Self {
                role: "assistant",
                content: text.clone(),
                tool_calls: Vec::new(),
            },
            chat::turn::Output::ToolCall(call) => call.into(),
        }
    }
}

impl From<&chat::turn::FunctionCall> for ChatOutputMessage {
    fn from(call: &chat::turn::FunctionCall) -> Self {
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

/// Complete or incremental Ollama chat response.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ChatResponse {
    /// Model that produced the response.
    pub(super) model: ModelId,
    /// Provider-compatible creation timestamp.
    pub(super) created_at: &'static str,
    /// Assistant output for this chunk.
    pub(super) message: ChatOutputMessage,
    /// Whether this is the terminal response chunk.
    #[serde(rename = "done")]
    pub(super) is_done: bool,
    /// Reason generation completed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) done_reason: Option<&'static str>,
    /// Total request duration in nanoseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) total_duration: Option<usize>,
    /// Model-load duration in nanoseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) load_duration: Option<usize>,
    /// Estimated prompt token count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) prompt_eval_count: Option<usize>,
    /// Prompt evaluation duration in nanoseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) prompt_eval_duration: Option<usize>,
    /// Estimated generated token count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) eval_count: Option<usize>,
    /// Generation duration in nanoseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) eval_duration: Option<usize>,
}

impl ChatResponse {
    /// Attach the provider-owned model identity to one canonical result.
    pub(super) fn from_compat(model: ModelId, response: &chat::turn::Response) -> Self {
        Self {
            model,
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

/// Ollama-compatible model metadata.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ModelDetails {
    /// Parent model identifier, empty for the built-in model.
    pub(super) parent_model: &'static str,
    /// Model container format.
    pub(super) format: &'static str,
    /// Primary model family.
    pub(super) family: &'static str,
    /// All model families.
    pub(super) families: Vec<&'static str>,
    /// Human-readable parameter count.
    pub(super) parameter_size: &'static str,
    /// Human-readable quantization level.
    pub(super) quantization_level: &'static str,
}

/// Ollama model-catalog entry.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ModelDescriptor {
    /// Display name used by Ollama clients.
    pub(super) name: ModelId,
    /// Model identifier used for requests.
    pub(super) model: ModelId,
    /// Provider-compatible modification timestamp.
    pub(super) modified_at: &'static str,
    /// Reported model size in bytes.
    pub(super) size: usize,
    /// Provider-compatible content digest.
    pub(super) digest: &'static str,
    /// Ollama-compatible model metadata.
    pub(super) details: ModelDetails,
}

/// Ollama model-list response.
#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct ModelListResponse {
    /// Available models.
    pub(super) models: Vec<ModelDescriptor>,
}
