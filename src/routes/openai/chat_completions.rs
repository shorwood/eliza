//! `OpenAI` Chat Completions and Gemini's OpenAI-compatible alias.
#![expect(
    clippy::missing_errors_doc,
    rlib::missing_section_dividers,
    rlib::undocumented_items,
    rlib::undocumented_early_returns,
    reason = "private adapter stages stay in request flow; wire names and validation errors are self-describing"
)]

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
    AssistantRole, ChatChunk, ChatChunkChoice, ChatCompletionRequest, ChatCompletionResponse,
    ChatContent, ChatContentPart, ChatDelta, ChatToolCall, ChatToolCallDelta, FinishReason,
    FunctionCallDelta, FunctionKind, MessageRole, StreamOptions,
};
use crate::routes::errors::ExtractionError;
use crate::types::errors::EncodingError;
use crate::types::http::{SseEvents, json_event, stream_chunks, unix_timestamp};
use crate::types::turn::{
    CompatOutput, CompatTurn, CompatTurnRequest, CompatTurnResponse, FunctionCall, RequestLimits,
};

struct OpenAiChatTurn(CompatTurnRequest);

impl TryFrom<ChatCompletionRequest> for OpenAiChatTurn {
    type Error = OpenAiError;

    fn try_from(payload: ChatCompletionRequest) -> Result<Self, Self::Error> {
        if payload.response_format.is_some() {
            return Err(OpenAiError::StructuredOutputUnsupported {
                param: "response_format",
            });
        }

        let mut system = Vec::new();
        let mut turns = Vec::new();
        for message in payload.messages {
            lower_message(
                message.role,
                message.content,
                message.tool_calls.unwrap_or_default(),
                &mut system,
                &mut turns,
            )?;
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

fn lower_message(
    role: MessageRole,
    content: Option<ChatContent>,
    tool_calls: Vec<ChatToolCall>,
    system: &mut Vec<String>,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), OpenAiError> {
    match role {
        MessageRole::System | MessageRole::Developer => {
            if let Some(content) = content {
                system.push(text_content(content, "messages.content")?);
            }
        }
        MessageRole::User => {
            let content = content.ok_or(OpenAiError::MissingUserContent)?;
            turns.push(CompatTurn::User(text_content(content, "messages.content")?));
        }
        MessageRole::Assistant => {
            if let Some(content) = content {
                turns.push(CompatTurn::Assistant(text_content(
                    content,
                    "messages.content",
                )?));
            }
            for call in tool_calls {
                turns.push(CompatTurn::ToolCall(call.lower("messages.tool_calls")?));
            }
        }
        MessageRole::Tool => {
            let content = content.ok_or(OpenAiError::MissingToolResultContent)?;
            turns.push(CompatTurn::ToolResult(tool_result_text(content)?));
        }
        MessageRole::Unsupported => {
            return Err(OpenAiError::UnsupportedMessageRole);
        }
    }
    Ok(())
}

fn text_content(content: ChatContent, param: &'static str) -> Result<String, OpenAiError> {
    match content {
        ChatContent::Text(text) => Ok(text),
        ChatContent::Parts(parts) => join_text_parts(parts, param),
        ChatContent::Object(_) => Err(OpenAiError::UnsupportedContentShape { param }),
    }
}

fn join_text_parts(
    parts: Vec<ChatContentPart>,
    param: &'static str,
) -> Result<String, OpenAiError> {
    let mut text = Vec::with_capacity(parts.len());
    for part in parts {
        match part {
            ChatContentPart::Text { text: part } => text.push(part),
            ChatContentPart::Unsupported => {
                return Err(OpenAiError::UnsupportedContentPart { param });
            }
        }
    }
    Ok(text.join("\n"))
}

fn tool_result_text(content: ChatContent) -> Result<String, OpenAiError> {
    match content {
        ChatContent::Text(text) => Ok(text),
        ChatContent::Parts(parts) => join_text_parts(parts, "messages.content"),
        ChatContent::Object(object) => Ok(object.serialized()),
    }
}

struct StreamContext<'a> {
    id: String,
    created: u64,
    model: &'a str,
}

#[derive(Clone, Copy)]
enum UsageStream {
    Include,
    Omit,
}

impl UsageStream {
    fn for_options(options: Option<StreamOptions>) -> Self {
        match options.and_then(|options| options.should_include_usage) {
            Some(true) => Self::Include,
            Some(false) | None => Self::Omit,
        }
    }
}

fn stream(
    response: &CompatTurnResponse,
    usage_stream: UsageStream,
) -> Result<SseEvents, EncodingError> {
    let context = StreamContext {
        id: format!("chatcmpl-{}", Uuid::now_v7().simple()),
        created: unix_timestamp(),
        model: response.model.as_str(),
    };
    let mut events = Vec::new();
    push_chunk(
        &mut events,
        &context,
        ChatDelta {
            role: Some(AssistantRole::Assistant),
            ..ChatDelta::default()
        },
        None,
        None,
    )?;
    match &response.output {
        CompatOutput::Text(text) => push_text_events(&mut events, &context, text)?,
        CompatOutput::ToolCall(call) => push_tool_events(&mut events, &context, call)?,
    }
    if matches!(usage_stream, UsageStream::Include) {
        events.push(json_event(&ChatChunk {
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

fn push_text_events(
    events: &mut Vec<Event>,
    context: &StreamContext<'_>,
    text: &str,
) -> Result<(), EncodingError> {
    for chunk in stream_chunks(text) {
        push_chunk(
            events,
            context,
            ChatDelta {
                content: Some(chunk),
                ..ChatDelta::default()
            },
            None,
            None,
        )?;
    }
    push_chunk(
        events,
        context,
        ChatDelta::default(),
        Some(FinishReason::Stop),
        None,
    )
}

fn push_tool_events(
    events: &mut Vec<Event>,
    context: &StreamContext<'_>,
    call: &FunctionCall,
) -> Result<(), EncodingError> {
    push_chunk(
        events,
        context,
        ChatDelta {
            tool_calls: vec![ChatToolCallDelta {
                index: 0,
                id: Some(format!("call_{}", Uuid::now_v7().simple())),
                kind: Some(FunctionKind::Function),
                function: FunctionCallDelta {
                    name: Some(call.name.clone()),
                    arguments: Some(String::new()),
                },
            }],
            ..ChatDelta::default()
        },
        None,
        None,
    )?;
    push_chunk(
        events,
        context,
        ChatDelta {
            tool_calls: vec![ChatToolCallDelta {
                index: 0,
                id: None,
                kind: None,
                function: FunctionCallDelta {
                    name: None,
                    arguments: Some(call.arguments.serialized()),
                },
            }],
            ..ChatDelta::default()
        },
        None,
        None,
    )?;
    push_chunk(
        events,
        context,
        ChatDelta::default(),
        Some(FinishReason::ToolCalls),
        None,
    )
}

fn push_chunk(
    events: &mut Vec<Event>,
    context: &StreamContext<'_>,
    delta: ChatDelta,
    finish_reason: Option<FinishReason>,
    usage: Option<super::types::ChatUsage>,
) -> Result<(), EncodingError> {
    events.push(json_event(&ChatChunk {
        id: &context.id,
        object: "chat.completion.chunk",
        created: context.created,
        model: context.model,
        choices: vec![ChatChunkChoice {
            index: 0,
            delta,
            finish_reason,
        }],
        usage,
    })?);
    Ok(())
}

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
        payload: Result<Json<ChatCompletionRequest>, JsonRejection>,
    ) -> Response {
        if let Err(error) = provider_authenticate(&headers, &state.config, ProviderAuth::Bearer) {
            return OpenAiRejection::from_error(&error).into_response();
        }
        let Json(payload) = match payload {
            Ok(payload) => payload,
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
            Err(error) => return OpenAiRejection::from_error(&error).into_response(),
        };
        let limits = RequestLimits::new(
            state.config.max_input_chars,
            state.config.max_history_messages,
        );
        let response = match request.complete(limits) {
            Ok(response) => response,
            Err(error) => return OpenAiRejection::from_error(&error).into_response(),
        };

        if should_stream {
            match stream(&response, usage_stream) {
                Ok(events) => events
                    .with_delay(state.config.stream_delay_ms)
                    .into_response(),
                Err(error) => OpenAiRejection::from_error(&error).into_response(),
            }
        } else {
            Json(ChatCompletionResponse::from(response)).into_response()
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
                    .response::<200, Json<ChatCompletionResponse>>()
                    .default_response::<Json<OpenAiFailureResponse>>()
            }),
        )
    }
}
