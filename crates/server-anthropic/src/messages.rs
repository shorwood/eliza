//! Anthropic Messages route adapter.
use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::HeaderMap;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use eliza_http::context::ProviderAuth;
use eliza_http::errors::EncodingError;
use eliza_http::extraction::ExtractionError;
use eliza_http::model::ModelId;
use eliza_http::response::{SseEvents, json_event, stream_chunks};
use eliza_modality_chat as chat;
use eliza_modality_image as image;
use uuid::Uuid;

use super::errors::{AnthropicError, AnthropicFailureResponse, AnthropicRejection};
use super::types::{
    MessageContent, MessageContentBlock, MessageImageSource, MessageRole, MessagesOutputBlock,
    MessagesRequest, MessagesResponse, MessagesStopReason, MessagesUsage, StreamDelta, StreamEvent,
    StreamMessage, StreamMessageDelta, StreamOutputUsage, ToolResultContent, ToolResultTextBlock,
};
use crate::context::AppState;

impl TryFrom<MessagesRequest> for chat::turn::Request {
    type Error = AnthropicError;

    fn try_from(payload: MessagesRequest) -> Result<Self, Self::Error> {
        // Compile output controls independently from transcript content.
        let output_format = payload
            .output_config
            .map(TryInto::try_into)
            .transpose()?
            .unwrap_or_default();

        // Separate the optional system prompt before replayable turns.
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
        let tools = payload.tools.unwrap_or_default().try_into()?;
        let tool_choice = payload
            .tool_choice
            .map(TryInto::try_into)
            .transpose()?
            .unwrap_or_default();

        Ok(chat::turn::Request::new(
            system,
            turns,
            tools,
            tool_choice,
            output_format,
        ))
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
        MessageContentBlock::Image { .. }
        | MessageContentBlock::ToolUse { .. }
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

/// Flush accumulated user text and images before a tool-result boundary.
fn user_content_flush(
    text: &mut Vec<String>,
    images: &mut Vec<image::source::Source>,
    turns: &mut Vec<chat::turn::Turn>,
) {
    // Empty buffers do not represent a conversation turn.
    if text.is_empty() && images.is_empty() {
        return;
    }
    turns.push(chat::turn::Turn::user_with_images(
        std::mem::take(text).join("\n"),
        std::mem::take(images),
    ));
}

/// Normalize one typed Anthropic image source.
///
/// # Errors
///
/// Returns a typed failure for missing, malformed, or unsupported source data.
fn user_image_source(
    source: Option<MessageImageSource>,
) -> Result<image::source::Source, AnthropicError> {
    let missing = || AnthropicError::Image(image::errors::Error::MissingSource);
    match source.ok_or_else(missing)? {
        MessageImageSource::Base64 { media_type, data } => {
            let media_type = media_type.ok_or_else(missing)?;
            let data = data.ok_or_else(missing)?;
            image::source::Source::typed_inline_base64(&media_type, data)
                .map_err(AnthropicError::Image)
        }
        MessageImageSource::Url { url } => {
            let url = url.ok_or_else(missing)?;
            image::source::Source::url(&url).map_err(AnthropicError::Image)
        }
        MessageImageSource::File { file_id } => {
            let file_id = file_id.ok_or_else(missing)?;
            image::source::Source::provider_reference(&file_id, None).map_err(AnthropicError::Image)
        }
        MessageImageSource::Unsupported => Err(AnthropicError::UnsupportedUserBlock),
    }
}

/// Lower one user message into neutral user and tool-result turns.
///
/// # Errors
///
/// Returns [`AnthropicError`] when content is empty, includes an unsupported
/// block, or carries an invalid tool result.
fn user_content_lower(
    content: MessageContent,
    turns: &mut Vec<chat::turn::Turn>,
) -> Result<(), AnthropicError> {
    let blocks = match content {
        // A string message is already a complete user turn.
        MessageContent::Text(text) => {
            turns.push(chat::turn::Turn::from(text));
            return Ok(());
        }
        MessageContent::Blocks(blocks) => blocks,
    };

    // Block content must carry at least one user-facing item.
    if blocks.is_empty() {
        return Err(AnthropicError::MissingUserContent);
    }

    let mut text = Vec::new();
    let mut images = Vec::new();
    for block in blocks {
        match block {
            MessageContentBlock::Text { text: part } => text.push(part),
            MessageContentBlock::Image { source } => images.push(user_image_source(source)?),
            MessageContentBlock::ToolResult { content } => {
                user_content_flush(&mut text, &mut images, turns);
                turns.push(chat::turn::Turn::ToolResult(tool_result_text(content)?));
            }
            // User messages cannot originate assistant tool calls.
            MessageContentBlock::ToolUse { .. } | MessageContentBlock::Unsupported => {
                return Err(AnthropicError::UnsupportedUserBlock);
            }
        }
    }
    user_content_flush(&mut text, &mut images, turns);
    Ok(())
}

// -----------------------------------------------------------------------------
// Assistant: Lowers assistant text and tool calls in source order.
// -----------------------------------------------------------------------------

/// Lower one assistant tool-use block.
///
/// # Errors
///
/// Returns [`AnthropicError`] when the block omits its name or input.
fn assistant_tool_call(
    name: Option<String>,
    input: Option<chat::json::JsonObject>,
) -> Result<chat::turn::FunctionCall, AnthropicError> {
    let name = name
        .filter(|name| !name.is_empty())
        .ok_or(AnthropicError::MissingToolUseName)?;
    let arguments = input.ok_or(AnthropicError::MissingToolUseInput)?;
    Ok(chat::turn::FunctionCall { name, arguments })
}

/// Lower each typed assistant content block.
///
/// # Errors
///
/// Returns [`AnthropicError`] when a tool call omits required data or an
/// assistant block has an unsupported shape.
fn assistant_blocks_lower(
    blocks: Vec<MessageContentBlock>,
    turns: &mut Vec<chat::turn::Turn>,
) -> Result<(), AnthropicError> {
    for block in blocks {
        match block {
            MessageContentBlock::Text { text } => turns.push(chat::turn::Turn::Assistant(text)),
            MessageContentBlock::ToolUse { name, input } => {
                turns.push(chat::turn::Turn::ToolCall(assistant_tool_call(
                    name, input,
                )?));
            }
            // Assistant messages cannot contain client tool results.
            MessageContentBlock::Image { .. }
            | MessageContentBlock::ToolResult { .. }
            | MessageContentBlock::Unsupported => {
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
    turns: &mut Vec<chat::turn::Turn>,
) -> Result<(), AnthropicError> {
    match content {
        MessageContent::Text(text) => turns.push(chat::turn::Turn::Assistant(text)),
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
// MessageEvents: Owns Anthropic's ordered streaming event sequence.
// -----------------------------------------------------------------------------

/// Encoded events for one Anthropic Messages response.
#[derive(Default)]
struct MessageEvents(
    /// Events encoded in delivery order.
    Vec<Event>,
);

#[expect(
    clippy::missing_errors_doc,
    reason = "private helpers propagate the builder's documented encoding failure"
)]
impl MessageEvents {
    /// Serialize and append one named event.
    fn push(&mut self, name: &'static str, event: &StreamEvent) -> Result<(), EncodingError> {
        self.0.push(json_event(event)?.event(name));
        Ok(())
    }

    /// Append all text chunks for one streaming response.
    fn push_text(&mut self, text: &str) -> Result<(), EncodingError> {
        for chunk in stream_chunks(text) {
            let event = StreamEvent::ContentBlockDelta {
                index: 0,
                delta: StreamDelta::TextDelta { text: chunk },
            };
            self.push("content_block_delta", &event)?;
        }
        Ok(())
    }

    /// Append the JSON delta for one tool call.
    fn push_tool_call(&mut self, call: &chat::turn::FunctionCall) -> Result<(), EncodingError> {
        let event = StreamEvent::ContentBlockDelta {
            index: 0,
            delta: StreamDelta::InputJsonDelta {
                partial_json: call.arguments.serialized(),
            },
        };
        self.push("content_block_delta", &event)
    }

    /// Append output-specific deltas and return the terminal stop reason.
    fn push_output(
        &mut self,
        output: &chat::turn::Output,
    ) -> Result<MessagesStopReason, EncodingError> {
        match output {
            chat::turn::Output::Text(text) => {
                self.push_text(text)?;
                Ok(MessagesStopReason::EndTurn)
            }
            chat::turn::Output::ToolCall(call) => {
                self.push_tool_call(call)?;
                Ok(MessagesStopReason::ToolUse)
            }
        }
    }

    /// Render the complete streaming sequence for one response.
    ///
    /// # Errors
    ///
    /// Returns [`EncodingError`] when any stream event cannot be serialized.
    fn for_response(
        model: &ModelId,
        response: &chat::turn::Response,
    ) -> Result<Self, EncodingError> {
        let mut events = Self::default();
        events.push(
            "message_start",
            &StreamEvent::MessageStart {
                message: StreamMessage {
                    id: format!("msg_{}", Uuid::now_v7().simple()),
                    kind: "message",
                    role: "assistant",
                    model: model.clone(),
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
        events.push(
            "content_block_start",
            &StreamEvent::ContentBlockStart {
                index: 0,
                content_block: MessagesOutputBlock::empty_for(&response.output),
            },
        )?;
        let stop_reason = events.push_output(&response.output)?;
        events.push(
            "content_block_stop",
            &StreamEvent::ContentBlockStop { index: 0 },
        )?;
        events.push(
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
        events.push("message_stop", &StreamEvent::MessageStop)?;
        Ok(events)
    }

    /// Finish the provider-neutral SSE wrapper.
    fn into_sse(self) -> SseEvents {
        SseEvents::from(self.0)
    }
}

// -----------------------------------------------------------------------------
// AnthropicMessages: Authenticates and executes the endpoint.
// -----------------------------------------------------------------------------

/// Handle one Anthropic Messages request.
async fn anthropic_messages(
    headers: HeaderMap,
    State(state): State<AppState>,
    payload: Result<Json<MessagesRequest>, JsonRejection>,
) -> Response {
    // Authentication failures use Anthropic's native error envelope.
    if let Err(error) = state
        .config
        .authenticate(&headers, ProviderAuth::ApiKey("x-api-key"))
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
    let model = payload.model.clone();
    let should_stream = payload.should_stream.unwrap_or(false);
    let request: chat::turn::Request = match payload.try_into() {
        Ok(request) => request,
        // Provider validation failures use Anthropic's native envelope.
        Err(error) => {
            return AnthropicRejection::from_error(&error).into_response();
        }
    };

    // Enforce shared request bounds after provider-specific lowering.
    // Complete the validated neutral request before choosing a wire response.
    let response = match request.complete(
        state.config.limits.max_input_chars(),
        state.config.limits.max_history_messages(),
    ) {
        Ok(response) => response,
        // Shared execution failures still render as Anthropic errors.
        Err(error) => {
            return AnthropicRejection::from(&error).into_response();
        }
    };

    if should_stream {
        match MessageEvents::for_response(&model, &response) {
            Ok(events) => events
                .into_sse()
                .with_delay(state.config.stream_delay_ms)
                .into_response(),
            Err(error) => AnthropicRejection::from_error(&error).into_response(),
        }
    } else {
        Json(MessagesResponse::from_compat(model, &response)).into_response()
    }
}

// -----------------------------------------------------------------------------
// Router: Publishes the Messages endpoint.
// -----------------------------------------------------------------------------

/// Build the Anthropic Messages route.
pub(super) fn router() -> ApiRouter<AppState> {
    ApiRouter::new().api_route(
        "/v1/messages",
        post_with(anthropic_messages, |operation| {
            operation
                .summary("Anthropic message")
                .tag("anthropic")
                .response::<200, Json<MessagesResponse>>()
                .response::<429, Json<AnthropicFailureResponse>>()
                .response::<503, Json<AnthropicFailureResponse>>()
                .default_response::<Json<AnthropicFailureResponse>>()
        })
        .layer(DefaultBodyLimit::max(image::limits::LIMIT_JSON_BODY)),
    )
}
