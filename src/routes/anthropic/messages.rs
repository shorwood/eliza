//! Anthropic Messages adapter.
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
use super::errors::{AnthropicError, AnthropicFailureResponse, AnthropicRejection};
use super::types::{
    MessageContent, MessageContentBlock, MessageRole, MessagesOutputBlock, MessagesRequest,
    MessagesResponse, MessagesStopReason, MessagesUsage, StreamDelta, StreamEvent, StreamMessage,
    StreamMessageDelta, StreamOutputUsage, ToolResultContent, ToolResultTextBlock,
};
use crate::routes::errors::ExtractionError;
use crate::types::errors::EncodingError;
use crate::types::http::{SseEvents, json_event, stream_chunks};
use crate::types::turn::{
    CompatOutput, CompatTurn, CompatTurnRequest, CompatTurnResponse, FunctionCall, RequestLimits,
};

// -----------------------------------------------------------------------------
// AnthropicTurn: Lowers one Messages request into the neutral contract.
// -----------------------------------------------------------------------------

/// Provider-neutral request lowered from Anthropic Messages input.
struct AnthropicTurn(
    /// Validated conversation consumed by the shared executor.
    CompatTurnRequest,
);

impl TryFrom<MessagesRequest> for AnthropicTurn {
    type Error = AnthropicError;

    fn try_from(payload: MessagesRequest) -> Result<Self, Self::Error> {
        let mut system = Vec::new();
        if let Some(content) = payload.system {
            system.push(system_text(content)?);
        }

        let mut turns = Vec::new();
        for message in payload.messages {
            match message.role {
                MessageRole::User => user_content_lower(message.content, &mut turns)?,
                MessageRole::Assistant => assistant_content_lower(message.content, &mut turns)?,
                // Unknown provider roles cannot be replayed safely.
                MessageRole::Unsupported => return Err(AnthropicError::UnsupportedRole),
            }
        }

        Ok(Self(CompatTurnRequest::new(
            payload.model,
            system,
            turns,
            payload.tools.unwrap_or_default().into_domain()?,
            payload.tool_choice.try_into()?,
        )))
    }
}

// -----------------------------------------------------------------------------
// System: Lowers system content into instruction text.
// -----------------------------------------------------------------------------

/// Lower one system block into its text payload.
///
/// # Errors
///
/// Returns [`AnthropicError`] when the block is not text.
fn system_block_text(block: MessageContentBlock) -> Result<String, AnthropicError> {
    match block {
        MessageContentBlock::Text { text } => Ok(text),
        MessageContentBlock::ToolUse { .. }
        | MessageContentBlock::ToolResult { .. }
        | MessageContentBlock::Unsupported => Err(AnthropicError::UnsupportedSystemBlock),
    }
}

/// Join validated text-only system blocks.
///
/// # Errors
///
/// Returns [`AnthropicError`] when any block is not text.
fn system_blocks_text(blocks: Vec<MessageContentBlock>) -> Result<String, AnthropicError> {
    let text = blocks
        .into_iter()
        .map(system_block_text)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(text.join("\n"))
}

/// Lower string or block system content into one instruction.
///
/// # Errors
///
/// Returns [`AnthropicError`] when block content contains a non-text item.
fn system_text(content: MessageContent) -> Result<String, AnthropicError> {
    match content {
        MessageContent::Text(text) => Ok(text),
        MessageContent::Blocks(blocks) => system_blocks_text(blocks),
    }
}

// -----------------------------------------------------------------------------
// User: Lowers user content and tool results in source order.
// -----------------------------------------------------------------------------

/// Flush accumulated user text before a tool-result boundary.
fn user_text_flush(text: &mut Vec<String>, turns: &mut Vec<CompatTurn>) {
    // Empty buffers do not represent a conversation turn.
    if text.is_empty() {
        return;
    }
    turns.push(CompatTurn::User(std::mem::take(text).join("\n")));
}

/// Lower one user message into neutral user and tool-result turns.
///
/// # Errors
///
/// Returns [`AnthropicError`] when content is empty, includes an unsupported
/// block, or carries an invalid tool result.
fn user_content_lower(
    content: MessageContent,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), AnthropicError> {
    let blocks = match content {
        // A string message is already a complete user turn.
        MessageContent::Text(text) => {
            turns.push(CompatTurn::User(text));
            return Ok(());
        }
        MessageContent::Blocks(blocks) => blocks,
    };

    // Block content must carry at least one user-facing item.
    if blocks.is_empty() {
        return Err(AnthropicError::MissingUserContent);
    }

    let mut text = Vec::new();
    for block in blocks {
        match block {
            MessageContentBlock::Text { text: part } => text.push(part),
            MessageContentBlock::ToolResult { content } => {
                user_text_flush(&mut text, turns);
                turns.push(CompatTurn::ToolResult(tool_result_text(content)?));
            }
            // User messages cannot originate assistant tool calls.
            MessageContentBlock::ToolUse { .. } | MessageContentBlock::Unsupported => {
                return Err(AnthropicError::UnsupportedUserBlock);
            }
        }
    }
    user_text_flush(&mut text, turns);
    Ok(())
}

// -----------------------------------------------------------------------------
// Assistant: Lowers assistant text and tool calls in source order.
// -----------------------------------------------------------------------------

/// Lower each typed assistant content block.
///
/// # Errors
///
/// Returns [`AnthropicError`] when a tool call omits required data or an
/// assistant block has an unsupported shape.
fn assistant_blocks_lower(
    blocks: Vec<MessageContentBlock>,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), AnthropicError> {
    for block in blocks {
        match block {
            MessageContentBlock::Text { text } => turns.push(CompatTurn::Assistant(text)),
            MessageContentBlock::ToolUse { name, input } => {
                let name = name
                    .filter(|name| !name.is_empty())
                    .ok_or(AnthropicError::MissingToolUseName)?;
                let arguments = input.ok_or(AnthropicError::MissingToolUseInput)?;
                turns.push(CompatTurn::ToolCall(FunctionCall { name, arguments }));
            }
            // Assistant messages cannot contain client tool results.
            MessageContentBlock::ToolResult { .. } | MessageContentBlock::Unsupported => {
                return Err(AnthropicError::UnsupportedAssistantBlock);
            }
        }
    }
    Ok(())
}

/// Lower one assistant message into neutral assistant and tool-call turns.
///
/// # Errors
///
/// Returns [`AnthropicError`] when block content cannot be represented by the
/// neutral transcript.
fn assistant_content_lower(
    content: MessageContent,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), AnthropicError> {
    match content {
        MessageContent::Text(text) => turns.push(CompatTurn::Assistant(text)),
        MessageContent::Blocks(blocks) => assistant_blocks_lower(blocks, turns)?,
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// ToolResult: Normalizes provider tool-result content.
// -----------------------------------------------------------------------------

/// Lower one tool-result block into its text payload.
///
/// # Errors
///
/// Returns [`AnthropicError`] when the block is not text.
fn tool_result_block_text(block: ToolResultTextBlock) -> Result<String, AnthropicError> {
    match block {
        ToolResultTextBlock::Text { text } => Ok(text),
        ToolResultTextBlock::Unsupported => Err(AnthropicError::UnsupportedToolResultBlock),
    }
}

/// Join validated text-only tool-result blocks.
///
/// # Errors
///
/// Returns [`AnthropicError`] when any block is not text.
fn tool_result_blocks_text(blocks: Vec<ToolResultTextBlock>) -> Result<String, AnthropicError> {
    let text = blocks
        .into_iter()
        .map(tool_result_block_text)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(text.join("\n"))
}

/// Lower optional tool-result content into replayable text.
///
/// # Errors
///
/// Returns [`AnthropicError`] when content is absent or includes a non-text block.
fn tool_result_text(content: Option<ToolResultContent>) -> Result<String, AnthropicError> {
    match content {
        Some(ToolResultContent::Text(text)) => Ok(text),
        Some(ToolResultContent::Object(object)) => Ok(object.serialized()),
        Some(ToolResultContent::Blocks(blocks)) => tool_result_blocks_text(blocks),
        None => Err(AnthropicError::MissingToolResultContent),
    }
}

// -----------------------------------------------------------------------------
// Events: Renders Anthropic's streaming event sequence.
// -----------------------------------------------------------------------------

/// Serialize and append one named Anthropic event.
///
/// # Errors
///
/// Returns [`EncodingError`] when the event cannot be serialized.
fn events_push(
    events: &mut Vec<Event>,
    name: &'static str,
    event: &StreamEvent,
) -> Result<(), EncodingError> {
    events.push(json_event(event)?.event(name));
    Ok(())
}

/// Append all text chunks for one streaming response.
///
/// # Errors
///
/// Returns [`EncodingError`] when a text delta cannot be serialized.
fn events_push_text(events: &mut Vec<Event>, text: &str) -> Result<(), EncodingError> {
    for chunk in stream_chunks(text) {
        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::TextDelta { text: chunk },
        };
        events_push(events, "content_block_delta", &event)?;
    }
    Ok(())
}

/// Append the JSON delta for one tool call.
///
/// # Errors
///
/// Returns [`EncodingError`] when the tool-call delta cannot be serialized.
fn events_push_tool_call(
    events: &mut Vec<Event>,
    call: &FunctionCall,
) -> Result<(), EncodingError> {
    let event = StreamEvent::ContentBlockDelta {
        index: 0,
        delta: StreamDelta::InputJsonDelta {
            partial_json: call.arguments.serialized(),
        },
    };
    events_push(events, "content_block_delta", &event)
}

/// Append content deltas for one completed output and return its stop reason.
///
/// # Errors
///
/// Returns [`EncodingError`] when an output delta cannot be serialized.
fn events_push_output(
    events: &mut Vec<Event>,
    output: &CompatOutput,
) -> Result<MessagesStopReason, EncodingError> {
    match output {
        CompatOutput::Text(text) => {
            events_push_text(events, text)?;
            Ok(MessagesStopReason::EndTurn)
        }
        CompatOutput::ToolCall(call) => {
            events_push_tool_call(events, call)?;
            Ok(MessagesStopReason::ToolUse)
        }
    }
}

/// Render the complete streaming event sequence for one response.
///
/// # Errors
///
/// Returns [`EncodingError`] when any stream event cannot be serialized.
fn events_response(response: &CompatTurnResponse) -> Result<SseEvents, EncodingError> {
    let mut events = Vec::new();
    events_push(
        &mut events,
        "message_start",
        &StreamEvent::MessageStart {
            message: StreamMessage {
                id: format!("msg_{}", Uuid::now_v7().simple()),
                kind: "message",
                role: "assistant",
                model: response.model.clone(),
                content: Vec::new(),
                stop_reason: None,
                stop_sequence: None,
                usage: MessagesUsage {
                    input_tokens: response.usage.prompt,
                    output_tokens: 0,
                },
            },
        },
    )?;
    events_push(
        &mut events,
        "content_block_start",
        &StreamEvent::ContentBlockStart {
            index: 0,
            content_block: MessagesOutputBlock::empty_for(&response.output),
        },
    )?;
    let stop_reason = events_push_output(&mut events, &response.output)?;
    events_push(
        &mut events,
        "content_block_stop",
        &StreamEvent::ContentBlockStop { index: 0 },
    )?;
    events_push(
        &mut events,
        "message_delta",
        &StreamEvent::MessageDelta {
            delta: StreamMessageDelta {
                stop_reason,
                stop_sequence: None,
            },
            usage: StreamOutputUsage {
                output_tokens: response.usage.completion,
            },
        },
    )?;
    events_push(&mut events, "message_stop", &StreamEvent::MessageStop)?;
    Ok(SseEvents::from(events))
}

// -----------------------------------------------------------------------------
// AnthropicMessages: Authenticates, executes, and mounts the endpoint.
// -----------------------------------------------------------------------------

/// Handle one Anthropic Messages request.
async fn anthropic_messages(
    headers: HeaderMap,
    State(state): State<AppState>,
    payload: Result<Json<MessagesRequest>, JsonRejection>,
) -> Response {
    // Authentication failures use Anthropic's native error envelope.
    if let Err(error) =
        provider_authenticate(&headers, &state.config, ProviderAuth::ApiKey("x-api-key"))
    {
        return AnthropicRejection::from_error(&error).into_response();
    }
    let Json(payload) = match payload {
        Ok(payload) => payload,
        // Extraction failures must retain their typed diagnostic code.
        Err(error) => {
            return AnthropicRejection::from_error(&ExtractionError::from(error)).into_response();
        }
    };

    // Capture transport policy before lowering consumes the request body.
    let should_stream = payload.should_stream.unwrap_or(false);
    let request = match AnthropicTurn::try_from(payload) {
        Ok(request) => request.0,
        // Provider validation failures use Anthropic's native envelope.
        Err(error) => {
            return AnthropicRejection::from_error(&error).into_response();
        }
    };

    // Enforce shared request bounds after provider-specific lowering.
    let limits = RequestLimits::new(
        state.config.max_input_chars,
        state.config.max_history_messages,
    );

    // Complete the validated neutral request before choosing a wire response.
    let response = match request.complete(limits) {
        Ok(response) => response,
        // Shared execution failures still render as Anthropic errors.
        Err(error) => {
            return AnthropicRejection::from_error(&error).into_response();
        }
    };

    if should_stream {
        match events_response(&response) {
            Ok(events) => events
                .with_delay(state.config.stream_delay_ms)
                .into_response(),
            Err(error) => AnthropicRejection::from_error(&error).into_response(),
        }
    } else {
        Json(MessagesResponse::from(response)).into_response()
    }
}

/// Anthropic Messages endpoint.
pub(super) struct AnthropicMessages;

impl AnthropicMessages {
    /// Mount the Anthropic Messages route.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/v1/messages",
            post_with(anthropic_messages, |operation| {
                operation
                    .summary("Anthropic message")
                    .tag("anthropic")
                    .response::<200, Json<MessagesResponse>>()
                    .default_response::<Json<AnthropicFailureResponse>>()
            }),
        )
    }
}
