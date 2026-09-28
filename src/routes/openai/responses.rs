//! `OpenAI` Responses API adapter.
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
    AssistantRole, ResponseIds, ResponseProgress, ResponseStatus, ResponsesContent,
    ResponsesContentPart, ResponsesEnvelope, ResponsesInput, ResponsesInputItem, ResponsesOutput,
    ResponsesRequest, ResponsesRole, ResponsesStreamEvent, ResponsesTextConfig,
    ResponsesTextFormat, ToolOutput, response_output_text,
};
use crate::routes::errors::ExtractionError;
use crate::types::errors::EncodingError;
use crate::types::http::{SseEvents, json_event, stream_chunks, unix_timestamp};
use crate::types::turn::{
    CompatOutput, CompatTurn, CompatTurnRequest, CompatTurnResponse, FunctionCall, RequestLimits,
};

struct OpenAiResponsesTurn(CompatTurnRequest);

impl TryFrom<ResponsesRequest> for OpenAiResponsesTurn {
    type Error = OpenAiError;

    fn try_from(payload: ResponsesRequest) -> Result<Self, Self::Error> {
        validate_text_config(payload.text)?;
        let mut system = Vec::new();
        if let Some(instructions) = payload.instructions {
            system.push(text_content(instructions, "instructions")?);
        }
        let turns = lower_input(payload.input, &mut system)?;
        Ok(Self(CompatTurnRequest::new(
            payload.model,
            system,
            turns,
            payload.tools.unwrap_or_default().into_domain()?,
            payload.tool_choice.try_into()?,
        )))
    }
}

fn validate_text_config(config: Option<ResponsesTextConfig>) -> Result<(), OpenAiError> {
    match config.and_then(|config| config.format) {
        None | Some(ResponsesTextFormat::Text) => Ok(()),
        Some(ResponsesTextFormat::Unsupported) => Err(OpenAiError::StructuredOutputUnsupported {
            param: "text.format",
        }),
    }
}

fn lower_input(
    input: ResponsesInput,
    system: &mut Vec<String>,
) -> Result<Vec<CompatTurn>, OpenAiError> {
    match input {
        ResponsesInput::Text(text) => Ok(vec![CompatTurn::User(text)]),
        ResponsesInput::Items(items) => {
            let mut turns = Vec::new();
            for item in items {
                lower_item(item, system, &mut turns)?;
            }
            Ok(turns)
        }
    }
}

fn lower_item(
    item: ResponsesInputItem,
    system: &mut Vec<String>,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), OpenAiError> {
    match item {
        ResponsesInputItem::Message { role, content } => {
            let text = text_content(content, "input.content")?;
            match role {
                ResponsesRole::User => turns.push(CompatTurn::User(text)),
                ResponsesRole::System | ResponsesRole::Developer => system.push(text),
                ResponsesRole::Assistant => turns.push(CompatTurn::Assistant(text)),
                ResponsesRole::Unsupported => {
                    return Err(OpenAiError::UnsupportedResponsesRole);
                }
            }
        }
        ResponsesInputItem::FunctionCall { name, arguments } => {
            let name = required_name(name, "input.name")?;
            let arguments = arguments.ok_or(OpenAiError::MissingFunctionCallArguments)?;
            turns.push(CompatTurn::ToolCall(FunctionCall {
                name,
                arguments: arguments.into_object("input.arguments")?,
            }));
        }
        ResponsesInputItem::FunctionCallOutput { output } => {
            let output = output.ok_or(OpenAiError::MissingFunctionOutput)?;
            turns.push(CompatTurn::ToolResult(match output {
                ToolOutput::Text(text) => text,
                ToolOutput::Object(object) => object.serialized(),
            }));
        }
        ResponsesInputItem::Unsupported => {
            return Err(OpenAiError::UnsupportedInputItem);
        }
    }
    Ok(())
}

fn text_content(content: ResponsesContent, param: &'static str) -> Result<String, OpenAiError> {
    match content {
        ResponsesContent::Text(text) => Ok(text),
        ResponsesContent::Parts(parts) => {
            let mut text = Vec::with_capacity(parts.len());
            for part in parts {
                match part {
                    ResponsesContentPart::InputText { text: part }
                    | ResponsesContentPart::OutputText { text: part }
                    | ResponsesContentPart::Text { text: part } => text.push(part),
                    ResponsesContentPart::Unsupported => {
                        return Err(OpenAiError::UnsupportedContentPart { param });
                    }
                }
            }
            Ok(text.join("\n"))
        }
    }
}

fn required_name(name: Option<String>, param: &'static str) -> Result<String, OpenAiError> {
    name.filter(|name| !name.is_empty())
        .ok_or(OpenAiError::MissingFunctionName { param })
}

struct ResponseContext {
    item_id: String,
    call_id: String,
    final_output: ResponsesOutput,
    envelope: ResponsesEnvelope,
}

impl From<&CompatTurnResponse> for ResponseContext {
    fn from(response: &CompatTurnResponse) -> Self {
        let item_id = match response.output {
            CompatOutput::Text(_) => format!("msg_{}", Uuid::now_v7().simple()),
            CompatOutput::ToolCall(_) => format!("fc_{}", Uuid::now_v7().simple()),
        };
        let call_id = format!("call_{}", Uuid::now_v7().simple());
        let output = ResponsesOutput::from_compat(
            &response.output,
            ResponseIds {
                item: item_id.clone(),
                call: call_id.clone(),
            },
            ResponseStatus::Completed,
        );
        Self {
            item_id,
            call_id,
            final_output: output.clone(),
            envelope: ResponsesEnvelope {
                id: format!("resp_{}", Uuid::now_v7().simple()),
                object: "response",
                created_at: unix_timestamp(),
                status: ResponseStatus::Completed,
                error: None,
                incomplete_details: None,
                model: response.model.clone(),
                output: vec![output],
                output_text: response_output_text(response),
                usage: response.usage.into(),
            },
        }
    }
}

#[derive(Default)]
struct Sequence(usize);

impl Sequence {
    fn next(&mut self) -> usize {
        let current = self.0;
        self.0 += 1;
        current
    }
}

fn response_stream(response: &CompatTurnResponse) -> Result<SseEvents, EncodingError> {
    let context = ResponseContext::from(response);
    let mut sequence = Sequence::default();
    let mut events = Vec::new();
    push_event(
        &mut events,
        &ResponsesStreamEvent::Created {
            sequence_number: sequence.next(),
            response: ResponseProgress {
                id: context.envelope.id.clone(),
                object: "response",
                created_at: context.envelope.created_at,
                status: ResponseStatus::InProgress,
                model: response.model.clone(),
                output: Vec::new(),
                error: None,
                incomplete_details: None,
            },
        },
    )?;
    push_output_events(&mut events, &mut sequence, &context, &response.output)?;
    push_event(
        &mut events,
        &ResponsesStreamEvent::OutputItemDone {
            sequence_number: sequence.next(),
            output_index: 0,
            item: context.final_output,
        },
    )?;
    push_event(
        &mut events,
        &ResponsesStreamEvent::Completed {
            sequence_number: sequence.next(),
            response: context.envelope,
        },
    )?;
    Ok(SseEvents::from(events))
}

fn push_output_events(
    events: &mut Vec<Event>,
    sequence: &mut Sequence,
    context: &ResponseContext,
    output: &CompatOutput,
) -> Result<(), EncodingError> {
    match output {
        CompatOutput::Text(text) => push_text_events(events, sequence, context, text),
        CompatOutput::ToolCall(call) => push_tool_events(events, sequence, context, call),
    }
}

fn push_text_events(
    events: &mut Vec<Event>,
    sequence: &mut Sequence,
    context: &ResponseContext,
    text: &str,
) -> Result<(), EncodingError> {
    push_event(
        events,
        &ResponsesStreamEvent::OutputItemAdded {
            sequence_number: sequence.next(),
            output_index: 0,
            item: ResponsesOutput::Message {
                id: context.item_id.clone(),
                status: ResponseStatus::InProgress,
                role: AssistantRole::Assistant,
                content: Vec::new(),
            },
        },
    )?;
    for chunk in stream_chunks(text) {
        push_event(
            events,
            &ResponsesStreamEvent::OutputTextDelta {
                sequence_number: sequence.next(),
                item_id: context.item_id.clone(),
                output_index: 0,
                content_index: 0,
                delta: chunk,
            },
        )?;
    }
    push_event(
        events,
        &ResponsesStreamEvent::OutputTextDone {
            sequence_number: sequence.next(),
            item_id: context.item_id.clone(),
            output_index: 0,
            content_index: 0,
            text: text.to_owned(),
        },
    )
}

fn push_tool_events(
    events: &mut Vec<Event>,
    sequence: &mut Sequence,
    context: &ResponseContext,
    call: &FunctionCall,
) -> Result<(), EncodingError> {
    push_event(
        events,
        &ResponsesStreamEvent::OutputItemAdded {
            sequence_number: sequence.next(),
            output_index: 0,
            item: ResponsesOutput::FunctionCall {
                id: context.item_id.clone(),
                call_id: context.call_id.clone(),
                name: call.name.clone(),
                arguments: String::new(),
                status: ResponseStatus::InProgress,
            },
        },
    )?;
    let arguments = call.arguments.serialized();
    push_event(
        events,
        &ResponsesStreamEvent::FunctionArgumentsDelta {
            sequence_number: sequence.next(),
            item_id: context.item_id.clone(),
            output_index: 0,
            call_id: context.call_id.clone(),
            delta: arguments.clone(),
        },
    )?;
    push_event(
        events,
        &ResponsesStreamEvent::FunctionArgumentsDone {
            sequence_number: sequence.next(),
            item_id: context.item_id.clone(),
            output_index: 0,
            call_id: context.call_id.clone(),
            name: call.name.clone(),
            arguments,
        },
    )
}

fn push_event(events: &mut Vec<Event>, event: &ResponsesStreamEvent) -> Result<(), EncodingError> {
    events.push(json_event(event)?);
    Ok(())
}

async fn responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<ResponsesRequest>, JsonRejection>,
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

    let should_stream = payload.should_stream.unwrap_or(false);
    let request = match OpenAiResponsesTurn::try_from(payload) {
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
        match response_stream(&response) {
            Ok(events) => events
                .with_delay(state.config.stream_delay_ms)
                .into_response(),
            Err(error) => OpenAiRejection::from_error(&error).into_response(),
        }
    } else {
        Json(ResponseContext::from(&response).envelope).into_response()
    }
}

/// `OpenAI` Responses endpoint.
pub(super) struct OpenAiResponses;

impl OpenAiResponses {
    /// Mount the Responses route.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/v1/responses",
            post_with(responses, |operation| {
                operation
                    .summary("OpenAI response")
                    .tag("openai")
                    .response::<200, Json<ResponsesEnvelope>>()
                    .default_response::<Json<OpenAiFailureResponse>>()
            }),
        )
    }
}
