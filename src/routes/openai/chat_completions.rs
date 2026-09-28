//! `OpenAI` Chat Completions and Gemini's OpenAI-compatible alias.
use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::HeaderMap;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use super::super::context::{AppState, ProviderAuth, provider_authenticate};
use super::errors::{OpenAiError, OpenAiFailureResponse, OpenAiRejection};
use super::types::{
    AssistantRole, ChatContent, ChatContentPart, ChatRequest, ChatRequestMessage, ChatRequestRole,
    ChatRequestStreamOptions, ChatResponse, ChatResponseFinishReason, ChatResponseUsage,
    ChatStreamChoice, ChatStreamChunk, ChatStreamDelta, ChatStreamFunctionDelta,
    ChatStreamToolCallDelta, ChatToolCall, ToolFunctionKind,
};
use crate::routes::errors::ExtractionError;
use crate::types::errors::EncodingError;
use crate::types::http::{SseEvents, json_event, stream_chunks, unix_timestamp};
use crate::types::turn::{
    CompatOutput, CompatTurn, CompatTurnRequest, CompatTurnResponse, FunctionCall, RequestLimits,
};

// -----------------------------------------------------------------------------
// OpenAiChatTurn: Lowers one chat request into the neutral contract.
// -----------------------------------------------------------------------------

/// Provider-neutral request lowered from Chat Completions input.
struct OpenAiChatTurn(
    /// Validated conversation consumed by the shared executor.
    CompatTurnRequest,
);

impl TryFrom<ChatRequest> for OpenAiChatTurn {
    type Error = OpenAiError;

    fn try_from(payload: ChatRequest) -> Result<Self, Self::Error> {
        // ELIZA cannot honor OpenAI's structured-output modes.
        if payload.response_format.is_some() {
            return Err(OpenAiError::StructuredOutputUnsupported {
                param: "response_format",
            });
        }

        let chat = ChatTranscript(payload.messages).lower()?;

        // Assemble the neutral request from lowered transcript components.
        Ok(Self(CompatTurnRequest::new(
            payload.model,
            chat.system,
            chat.turns,
            payload.tools.unwrap_or_default().into_domain()?,
            payload.tool_choice.try_into()?,
        )))
    }
}

// -----------------------------------------------------------------------------
// ChatTranscript: Separates instructions from replayable chat turns.
// -----------------------------------------------------------------------------

/// Provider-neutral conversation pieces lowered from one chat transcript.
struct ChatTranscriptOutput {
    /// Instructions separated from replayable turns.
    system: Vec<String>,
    /// Ordered user, assistant, and tool turns.
    turns: Vec<CompatTurn>,
}

/// Ordered chat messages awaiting provider-neutral lowering.
struct ChatTranscript(
    /// Messages preserved in provider order.
    Vec<ChatRequestMessage>,
);

impl ChatTranscript {
    /// Lower the complete transcript into system text and neutral turns.
    ///
    /// # Errors
    ///
    /// Returns [`OpenAiError`] when a message has an unsupported role or
    /// content shape, or carries an invalid tool call.
    fn lower(self) -> Result<ChatTranscriptOutput, OpenAiError> {
        let mut system = Vec::new();
        let mut turns = Vec::new();
        for message in self.0 {
            let calls = message.tool_calls.unwrap_or_default();
            lower_message(
                message.role,
                message.content,
                calls,
                &mut system,
                &mut turns,
            )?;
        }
        Ok(ChatTranscriptOutput { system, turns })
    }
}

// -----------------------------------------------------------------------------
// Lower: Converts typed chat roles into neutral conversation turns.
// -----------------------------------------------------------------------------

/// Lower optional assistant text followed by its ordered tool calls.
///
/// # Errors
///
/// Returns [`OpenAiError`] when assistant content or a tool call is invalid.
fn lower_assistant_message(
    content: Option<ChatContent>,
    tool_calls: Vec<ChatToolCall>,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), OpenAiError> {
    if let Some(content) = content {
        turns.push(CompatTurn::Assistant(text_content(
            content,
            "messages.content",
        )?));
    }
    for call in tool_calls {
        turns.push(CompatTurn::ToolCall(call.lower("messages.tool_calls")?));
    }
    Ok(())
}

/// Lower one typed chat message into system text or replayable turns.
///
/// # Errors
///
/// Returns [`OpenAiError`] when required content is absent or the message role,
/// content, or tool calls cannot be represented by the neutral transcript.
fn lower_message(
    role: ChatRequestRole,
    content: Option<ChatContent>,
    tool_calls: Vec<ChatToolCall>,
    system: &mut Vec<String>,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), OpenAiError> {
    match role {
        ChatRequestRole::System | ChatRequestRole::Developer => {
            if let Some(content) = content {
                system.push(text_content(content, "messages.content")?);
            }
        }
        ChatRequestRole::User => {
            let content = content.ok_or(OpenAiError::MissingUserContent)?;
            turns.push(CompatTurn::User(text_content(content, "messages.content")?));
        }
        ChatRequestRole::Assistant => lower_assistant_message(content, tool_calls, turns)?,
        ChatRequestRole::Tool => {
            let content = content.ok_or(OpenAiError::MissingToolResultContent)?;
            turns.push(CompatTurn::ToolResult(text_tool_result(content)?));
        }
        // Unknown provider roles cannot be replayed safely.
        ChatRequestRole::Unsupported => return Err(OpenAiError::UnsupportedMessageRole),
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// Text: Normalizes OpenAI content shapes into replayable text.
// -----------------------------------------------------------------------------

/// Join a sequence of text-only content parts.
///
/// # Errors
///
/// Returns [`OpenAiError`] when any content part is not text.
fn text_parts_join(
    parts: Vec<ChatContentPart>,
    param: &'static str,
) -> Result<String, OpenAiError> {
    let mut text = Vec::with_capacity(parts.len());
    for part in parts {
        match part {
            ChatContentPart::Text { text: part } => text.push(part),
            // Non-text content cannot be represented by ELIZA.
            ChatContentPart::Unsupported => {
                return Err(OpenAiError::UnsupportedContentPart { param });
            }
        }
    }
    Ok(text.join("\n"))
}

/// Lower supported Chat Completions content into text.
///
/// # Errors
///
/// Returns [`OpenAiError`] for object content or a non-text content part.
fn text_content(content: ChatContent, param: &'static str) -> Result<String, OpenAiError> {
    match content {
        ChatContent::Text(text) => Ok(text),
        ChatContent::Parts(parts) => text_parts_join(parts, param),
        ChatContent::Object(_) => Err(OpenAiError::UnsupportedContentShape { param }),
    }
}

/// Lower tool-result content, including JSON objects, into text.
///
/// # Errors
///
/// Returns [`OpenAiError`] when part-list content contains a non-text part.
fn text_tool_result(content: ChatContent) -> Result<String, OpenAiError> {
    match content {
        ChatContent::Text(text) => Ok(text),
        ChatContent::Parts(parts) => text_parts_join(parts, "messages.content"),
        ChatContent::Object(object) => Ok(object.serialized()),
    }
}

// -----------------------------------------------------------------------------
// StreamContext: Carries stable fields shared by every chat chunk.
// -----------------------------------------------------------------------------

/// Stable response identity shared by a Chat Completions stream.
struct StreamContext<'a> {
    /// Completion identifier.
    id: String,
    /// Unix creation timestamp.
    created: u64,
    /// Provider-visible model identifier.
    model: &'a str,
}

// -----------------------------------------------------------------------------
// UsageStream: Selects whether a stream includes final token usage.
// -----------------------------------------------------------------------------

/// Token-usage emission policy for streaming responses.
#[derive(Clone, Copy)]
enum UsageStream {
    /// Emit a final usage-only chunk.
    Include,
    /// End after content and the sentinel event.
    Omit,
}

impl UsageStream {
    /// Normalize optional wire options into one emission policy.
    fn for_options(options: Option<ChatRequestStreamOptions>) -> Self {
        match options.and_then(|options| options.should_include_usage) {
            Some(true) => Self::Include,
            Some(false) | None => Self::Omit,
        }
    }
}

// -----------------------------------------------------------------------------
// Stream: Renders OpenAI's Chat Completions event sequence.
// -----------------------------------------------------------------------------

/// Serialize and append one Chat Completions chunk.
///
/// # Errors
///
/// Returns [`EncodingError`] when the chunk cannot be serialized.
fn stream_push_chunk(
    events: &mut Vec<Event>,
    context: &StreamContext<'_>,
    delta: ChatStreamDelta,
    finish_reason: Option<ChatResponseFinishReason>,
    usage: Option<ChatResponseUsage>,
) -> Result<(), EncodingError> {
    events.push(json_event(&ChatStreamChunk {
        id: &context.id,
        object: "chat.completion.chunk",
        created: context.created,
        model: context.model,
        choices: vec![ChatStreamChoice {
            index: 0,
            delta,
            finish_reason,
        }],
        usage,
    })?);
    Ok(())
}

/// Append incremental text chunks and their terminal choice.
///
/// # Errors
///
/// Returns [`EncodingError`] when a stream chunk cannot be serialized.
fn stream_push_text(
    events: &mut Vec<Event>,
    context: &StreamContext<'_>,
    text: &str,
) -> Result<(), EncodingError> {
    for chunk in stream_chunks(text) {
        stream_push_chunk(
            events,
            context,
            ChatStreamDelta {
                content: Some(chunk),
                ..ChatStreamDelta::default()
            },
            None,
            None,
        )?;
    }
    stream_push_chunk(
        events,
        context,
        ChatStreamDelta::default(),
        Some(ChatResponseFinishReason::Stop),
        None,
    )
}

/// Append a typed function call and its serialized arguments.
///
/// # Errors
///
/// Returns [`EncodingError`] when a tool-call chunk cannot be serialized.
fn stream_push_tool(
    events: &mut Vec<Event>,
    context: &StreamContext<'_>,
    call: &FunctionCall,
) -> Result<(), EncodingError> {
    stream_push_chunk(
        events,
        context,
        ChatStreamDelta {
            tool_calls: vec![ChatStreamToolCallDelta {
                index: 0,
                id: Some(format!("call_{}", Uuid::now_v7().simple())),
                kind: Some(ToolFunctionKind::Function),
                function: ChatStreamFunctionDelta {
                    name: Some(call.name.clone()),
                    arguments: Some(String::new()),
                },
            }],
            ..ChatStreamDelta::default()
        },
        None,
        None,
    )?;
    stream_push_chunk(
        events,
        context,
        ChatStreamDelta {
            tool_calls: vec![ChatStreamToolCallDelta {
                index: 0,
                id: None,
                kind: None,
                function: ChatStreamFunctionDelta {
                    name: None,
                    arguments: Some(call.arguments.serialized()),
                },
            }],
            ..ChatStreamDelta::default()
        },
        None,
        None,
    )?;
    stream_push_chunk(
        events,
        context,
        ChatStreamDelta::default(),
        Some(ChatResponseFinishReason::ToolCalls),
        None,
    )
}

/// Render the complete SSE sequence for one neutral response.
///
/// # Errors
///
/// Returns [`EncodingError`] when any response chunk cannot be serialized.
fn stream_render(
    response: &CompatTurnResponse,
    usage_stream: UsageStream,
) -> Result<SseEvents, EncodingError> {
    let context = StreamContext {
        id: format!("chatcmpl-{}", Uuid::now_v7().simple()),
        created: unix_timestamp(),
        model: response.model.as_str(),
    };
    let mut events = Vec::new();
    stream_push_chunk(
        &mut events,
        &context,
        ChatStreamDelta {
            role: Some(AssistantRole::Assistant),
            ..ChatStreamDelta::default()
        },
        None,
        None,
    )?;
    match &response.output {
        CompatOutput::Text(text) => stream_push_text(&mut events, &context, text)?,
        CompatOutput::ToolCall(call) => stream_push_tool(&mut events, &context, call)?,
    }
    if matches!(usage_stream, UsageStream::Include) {
        events.push(json_event(&ChatStreamChunk {
            id: &context.id,
            object: "chat.completion.chunk",
            created: context.created,
            model: context.model,
            choices: Vec::new(),
            usage: Some(response.usage.into()),
        })?);
    }
    events.push(Event::default().data("[DONE]"));
    Ok(SseEvents::from(events))
}

// -----------------------------------------------------------------------------
// OpenAiChatCompletions: Handles and mounts the endpoint.
// -----------------------------------------------------------------------------

/// `OpenAI` Chat Completions endpoint.
pub(crate) struct OpenAiChatCompletions;

impl OpenAiChatCompletions {
    /// Handle one OpenAI-compatible Chat Completions request.
    #[expect(
        clippy::unused_async,
        reason = "Axum handlers must return a future even when their work is synchronous"
    )]
    pub(crate) async fn handle(
        State(state): State<AppState>,
        headers: HeaderMap,
        payload: Result<Json<ChatRequest>, JsonRejection>,
    ) -> Response {
        // Authentication failures use OpenAI's native error envelope.
        if let Err(error) = provider_authenticate(&headers, &state.config, ProviderAuth::Bearer) {
            return OpenAiRejection::from_error(&error).into_response();
        }
        let Json(payload) = match payload {
            Ok(payload) => payload,
            // Extraction failures retain their typed diagnostic code.
            Err(error) => {
                return OpenAiRejection::from_error(&ExtractionError::from(error)).into_response();
            }
        };

        // Capture transport options before lowering consumes the request body.
        let should_stream = payload.should_stream.unwrap_or(false);
        let usage_stream = UsageStream::for_options(payload.stream_options);

        // Lower and execute the provider request under shared resource limits.
        let request = match OpenAiChatTurn::try_from(payload) {
            Ok(request) => request.0,
            // Provider validation failures use OpenAI's native envelope.
            Err(error) => {
                return OpenAiRejection::from_error(&error).into_response();
            }
        };

        // Apply shared resource bounds to the lowered request.
        let limits = RequestLimits::new(
            state.config.max_input_chars,
            state.config.max_history_messages,
        );

        // Complete one neutral turn before rendering OpenAI output.
        let response = match request.complete(limits) {
            Ok(response) => response,
            // Shared execution failures still render as OpenAI errors.
            Err(error) => {
                return OpenAiRejection::from_error(&error).into_response();
            }
        };

        if should_stream {
            match stream_render(&response, usage_stream) {
                Ok(events) => events
                    .with_delay(state.config.stream_delay_ms)
                    .into_response(),
                Err(error) => OpenAiRejection::from_error(&error).into_response(),
            }
        } else {
            Json(ChatResponse::from(response)).into_response()
        }
    }

    /// Mount the `OpenAI` Chat Completions route.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/v1/chat/completions",
            post_with(Self::handle, |operation| {
                operation
                    .summary("OpenAI chat completion")
                    .tag("openai")
                    .response::<200, Json<ChatResponse>>()
                    .default_response::<Json<OpenAiFailureResponse>>()
            }),
        )
    }
}
