//! Anthropic Messages adapter.
#![expect(
    clippy::missing_errors_doc,
    rlib::missing_section_dividers,
    rlib::undocumented_early_returns,
    rlib::undocumented_items,
    reason = "private adapter stages stay in request flow; validation errors describe each guard"
)]

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::sse::Event;
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use super::super::context::{AppState, ProviderAuth, provider_authenticate};
use super::types::{
    AnthropicFailureResponse, AnthropicRejection, Content, ContentBlock, MessageDelta, MessageRole,
    MessagesRequest, MessagesResponse, OutputBlock, OutputUsage, StopReason, StreamDelta,
    StreamEvent, StreamMessage, ToolResultContent, ToolResultTextBlock, Usage,
};
use crate::types::http::{ProviderRejection, SseEvents, json_event, stream_chunks};
use crate::types::turn::{
    CompatOutput, CompatTurn, CompatTurnRequest, CompatTurnResponse, FunctionCall, RequestLimits,
};

struct AnthropicTurn(CompatTurnRequest);

impl TryFrom<MessagesRequest> for AnthropicTurn {
    type Error = ProviderRejection;

    fn try_from(payload: MessagesRequest) -> Result<Self, Self::Error> {
        let mut system = Vec::new();
        if let Some(content) = payload.system {
            system.push(system_text(content)?);
        }

        let mut turns = Vec::new();
        for message in payload.messages {
            match message.role {
                MessageRole::User => lower_user_content(message.content, &mut turns)?,
                MessageRole::Assistant => lower_assistant_content(message.content, &mut turns)?,
                MessageRole::Unsupported => {
                    return Err(ProviderRejection::unsupported(
                        "messages.role",
                        "unsupported Anthropic role",
                    ));
                }
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

fn system_text(content: Content) -> Result<String, ProviderRejection> {
    match content {
        Content::Text(text) => Ok(text),
        Content::Blocks(blocks) => {
            let mut text = Vec::with_capacity(blocks.len());
            for block in blocks {
                match block {
                    ContentBlock::Text { text: part } => text.push(part),
                    ContentBlock::ToolUse { .. }
                    | ContentBlock::ToolResult { .. }
                    | ContentBlock::Unsupported => {
                        return Err(ProviderRejection::unsupported(
                            "system",
                            "only text system blocks are supported",
                        ));
                    }
                }
            }
            Ok(text.join("\n"))
        }
    }
}

fn lower_user_content(
    content: Content,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), ProviderRejection> {
    let blocks = match content {
        Content::Text(text) => {
            turns.push(CompatTurn::User(text));
            return Ok(());
        }
        Content::Blocks(blocks) => blocks,
    };
    if blocks.is_empty() {
        return Err(ProviderRejection::invalid(
            "messages.content",
            "user content blocks are required",
        ));
    }

    let mut text = Vec::new();
    for block in blocks {
        match block {
            ContentBlock::Text { text: part } => text.push(part),
            ContentBlock::ToolResult { content } => {
                flush_user_text(&mut text, turns);
                turns.push(CompatTurn::ToolResult(tool_result_text(content)?));
            }
            ContentBlock::ToolUse { .. } | ContentBlock::Unsupported => {
                return Err(ProviderRejection::unsupported(
                    "messages.content",
                    "only text and tool_result blocks are supported",
                ));
            }
        }
    }
    flush_user_text(&mut text, turns);
    Ok(())
}

fn flush_user_text(text: &mut Vec<String>, turns: &mut Vec<CompatTurn>) {
    if text.is_empty() {
        return;
    }
    turns.push(CompatTurn::User(std::mem::take(text).join("\n")));
}

fn lower_assistant_content(
    content: Content,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), ProviderRejection> {
    match content {
        Content::Text(text) => turns.push(CompatTurn::Assistant(text)),
        Content::Blocks(blocks) => lower_assistant_blocks(blocks, turns)?,
    }
    Ok(())
}

fn lower_assistant_blocks(
    blocks: Vec<ContentBlock>,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), ProviderRejection> {
    for block in blocks {
        match block {
            ContentBlock::Text { text } => turns.push(CompatTurn::Assistant(text)),
            ContentBlock::ToolUse { name, input } => {
                let name = name.filter(|name| !name.is_empty()).ok_or_else(|| {
                    ProviderRejection::invalid("messages.content", "tool_use block is missing name")
                })?;
                let arguments = input.ok_or_else(|| {
                    ProviderRejection::invalid(
                        "messages.content",
                        "tool_use block is missing input",
                    )
                })?;
                turns.push(CompatTurn::ToolCall(FunctionCall { name, arguments }));
            }
            ContentBlock::ToolResult { .. } | ContentBlock::Unsupported => {
                return Err(ProviderRejection::unsupported(
                    "messages.content",
                    "only text and tool_use blocks are supported",
                ));
            }
        }
    }
    Ok(())
}

fn tool_result_text(content: Option<ToolResultContent>) -> Result<String, ProviderRejection> {
    match content {
        Some(ToolResultContent::Text(text)) => Ok(text),
        Some(ToolResultContent::Object(object)) => Ok(object.serialized()),
        Some(ToolResultContent::Blocks(blocks)) => {
            let mut text = Vec::with_capacity(blocks.len());
            for block in blocks {
                match block {
                    ToolResultTextBlock::Text { text: part } => text.push(part),
                    ToolResultTextBlock::Unsupported => {
                        return Err(ProviderRejection::unsupported(
                            "messages.content",
                            "only text tool-result blocks are supported",
                        ));
                    }
                }
            }
            Ok(text.join("\n"))
        }
        None => Err(ProviderRejection::invalid(
            "messages.content",
            "tool result content is required",
        )),
    }
}

fn response_events(response: &CompatTurnResponse) -> Result<SseEvents, ProviderRejection> {
    let mut events = Vec::new();
    push_event(
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
                usage: Usage {
                    input_tokens: response.usage.prompt,
                    output_tokens: 0,
                },
            },
        },
    )?;
    push_event(
        &mut events,
        "content_block_start",
        &StreamEvent::ContentBlockStart {
            index: 0,
            content_block: OutputBlock::empty_for(&response.output),
        },
    )?;
    let stop_reason = match &response.output {
        CompatOutput::Text(text) => {
            for chunk in stream_chunks(text) {
                push_event(
                    &mut events,
                    "content_block_delta",
                    &StreamEvent::ContentBlockDelta {
                        index: 0,
                        delta: StreamDelta::TextDelta { text: chunk },
                    },
                )?;
            }
            StopReason::EndTurn
        }
        CompatOutput::ToolCall(call) => {
            push_event(
                &mut events,
                "content_block_delta",
                &StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: StreamDelta::InputJsonDelta {
                        partial_json: call.arguments.serialized(),
                    },
                },
            )?;
            StopReason::ToolUse
        }
    };
    push_event(
        &mut events,
        "content_block_stop",
        &StreamEvent::ContentBlockStop { index: 0 },
    )?;
    push_event(
        &mut events,
        "message_delta",
        &StreamEvent::MessageDelta {
            delta: MessageDelta {
                stop_reason,
                stop_sequence: None,
            },
            usage: OutputUsage {
                output_tokens: response.usage.completion,
            },
        },
    )?;
    push_event(&mut events, "message_stop", &StreamEvent::MessageStop)?;
    Ok(SseEvents::from(events))
}

fn push_event(
    events: &mut Vec<Event>,
    name: &'static str,
    event: &StreamEvent,
) -> Result<(), ProviderRejection> {
    events.push(json_event(event)?.event(name));
    Ok(())
}

async fn messages(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<MessagesRequest>,
) -> Response {
    if let Err(error) =
        provider_authenticate(&headers, &state.config, ProviderAuth::ApiKey("x-api-key"))
    {
        return AnthropicRejection::from(error).into_response();
    }

    // Capture transport policy before lowering consumes the request body.
    let should_stream = payload.should_stream.unwrap_or(false);
    let request = match AnthropicTurn::try_from(payload) {
        Ok(request) => request.0,
        Err(error) => return AnthropicRejection::from(error).into_response(),
    };
    let limits = RequestLimits::new(
        state.config.max_input_chars,
        state.config.max_history_messages,
    );
    let response = match request.complete(limits) {
        Ok(response) => response,
        Err(error) => return AnthropicRejection::from(error).into_response(),
    };

    if should_stream {
        match response_events(&response) {
            Ok(events) => events
                .with_delay(state.config.stream_delay_ms)
                .into_response(),
            Err(error) => AnthropicRejection::from(error).into_response(),
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
            post_with(messages, |operation| {
                operation
                    .summary("Anthropic message")
                    .tag("anthropic")
                    .response::<200, Json<MessagesResponse>>()
                    .default_response::<Json<AnthropicFailureResponse>>()
            }),
        )
    }
}
