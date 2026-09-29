//! `OpenAI` Responses API adapter.
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
    AssistantRole, ResponsesRequest, ResponsesRequestContent, ResponsesRequestContentPart,
    ResponsesRequestInput, ResponsesRequestInputItem, ResponsesRequestRole,
    ResponsesRequestTextConfig, ResponsesRequestTextFormat, ResponsesRequestToolOutput,
    ResponsesResponse, ResponsesResponseIds, ResponsesResponseOutput, ResponsesResponseProgress,
    ResponsesResponseStatus, ResponsesResponseStreamEvent, responses_response_output_text,
};
use crate::routes::errors::ExtractionError;
use crate::types::errors::EncodingError;
use crate::types::http::{SseEvents, json_event, stream_chunks, unix_timestamp};
use crate::types::lower::Lower;
use crate::types::turn::{
    CompatOutput, CompatTurn, CompatTurnRequest, CompatTurnResponse, FunctionCall,
};

// -----------------------------------------------------------------------------
// ResponsesLowering: Lowers one request into the neutral contract.
// -----------------------------------------------------------------------------

/// Responses input awaiting provider-neutral conversion.
struct ResponsesLowering(
    /// Typed provider request.
    ResponsesRequest,
);

impl Lower for ResponsesLowering {
    type Canonical = CompatTurnRequest;

    type Error = OpenAiError;

    fn lower(self) -> Result<Self::Canonical, Self::Error> {
        let payload = self.0;
        validate_text_config(payload.text)?;

        // Separate request-level instructions from replayable input turns.
        let mut system = Vec::new();
        if let Some(instructions) = payload.instructions {
            system.push(text_content(instructions, "instructions")?);
        }
        let turns = lower_input(payload.input, &mut system)?;

        // Lower tool declarations and selection policy independently.
        let tools = payload.tools.unwrap_or_default().lower()?;
        let tool_choice = payload
            .tool_choice
            .map(Lower::lower)
            .transpose()?
            .unwrap_or_default();

        Ok(CompatTurnRequest::builder()
            .model(payload.model)
            .system_text(system)
            .turns(turns)
            .tools(tools)
            .tool_choice(tool_choice)
            .build())
    }
}

// -----------------------------------------------------------------------------
// ValidateTextConfig: Rejects unsupported structured-output requests.
// -----------------------------------------------------------------------------

/// Validate the optional Responses text format.
///
/// # Errors
///
/// Returns [`OpenAiError`] when structured output is requested.
fn validate_text_config(config: Option<ResponsesRequestTextConfig>) -> Result<(), OpenAiError> {
    match config.and_then(|config| config.format) {
        None | Some(ResponsesRequestTextFormat::Text) => Ok(()),
        Some(ResponsesRequestTextFormat::Unsupported) => {
            Err(OpenAiError::StructuredOutputUnsupported {
                param: "text.format",
            })
        }
    }
}

// -----------------------------------------------------------------------------
// Lower: Converts Responses input items into neutral conversation turns.
// -----------------------------------------------------------------------------

/// Lower one Responses message according to its typed role.
///
/// # Errors
///
/// Returns [`OpenAiError`] when content is unsupported or the role is unknown.
fn lower_message_item(
    role: ResponsesRequestRole,
    content: ResponsesRequestContent,
    system: &mut Vec<String>,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), OpenAiError> {
    let text = text_content(content, "input.content")?;
    match role {
        ResponsesRequestRole::User => turns.push(CompatTurn::User(text)),
        ResponsesRequestRole::System | ResponsesRequestRole::Developer => system.push(text),
        ResponsesRequestRole::Assistant => turns.push(CompatTurn::Assistant(text)),
        // Unknown provider roles cannot be replayed safely.
        ResponsesRequestRole::Unsupported => return Err(OpenAiError::UnsupportedResponsesRole),
    }
    Ok(())
}

/// Lower one typed Responses input item.
///
/// # Errors
///
/// Returns [`OpenAiError`] when the item is unsupported or omits required
/// function-call data.
fn lower_item(
    item: ResponsesRequestInputItem,
    system: &mut Vec<String>,
    turns: &mut Vec<CompatTurn>,
) -> Result<(), OpenAiError> {
    match item {
        ResponsesRequestInputItem::Message { role, content } => {
            lower_message_item(role, content, system, turns)?;
        }
        ResponsesRequestInputItem::FunctionCall { name, arguments } => {
            let name = required_name(name, "input.name")?;
            let arguments = arguments.ok_or(OpenAiError::MissingFunctionCallArguments)?;
            turns.push(CompatTurn::ToolCall(FunctionCall {
                name,
                arguments: arguments.into_object("input.arguments")?,
            }));
        }
        ResponsesRequestInputItem::FunctionCallOutput { output } => {
            let output = output.ok_or(OpenAiError::MissingFunctionOutput)?;
            turns.push(CompatTurn::ToolResult(match output {
                ResponsesRequestToolOutput::Text(text) => text,
                ResponsesRequestToolOutput::Object(object) => object.serialized(),
            }));
        }
        // Unknown item types cannot be represented by the neutral contract.
        ResponsesRequestInputItem::Unsupported => return Err(OpenAiError::UnsupportedInputItem),
    }
    Ok(())
}

/// Lower string or item-list input into an ordered neutral transcript.
///
/// # Errors
///
/// Returns [`OpenAiError`] when any input item cannot be represented by the
/// neutral transcript.
fn lower_input(
    input: ResponsesRequestInput,
    system: &mut Vec<String>,
) -> Result<Vec<CompatTurn>, OpenAiError> {
    match input {
        ResponsesRequestInput::Text(text) => Ok(vec![CompatTurn::User(text)]),
        ResponsesRequestInput::Items(items) => {
            let mut turns = Vec::new();
            for item in items {
                lower_item(item, system, &mut turns)?;
            }
            Ok(turns)
        }
    }
}

// -----------------------------------------------------------------------------
// Text: Normalizes Responses content into replayable text.
// -----------------------------------------------------------------------------

/// Lower one Responses content part into its text payload.
///
/// # Errors
///
/// Returns [`OpenAiError`] when the content part is not text.
fn text_part(
    part: ResponsesRequestContentPart,
    param: &'static str,
) -> Result<String, OpenAiError> {
    match part {
        ResponsesRequestContentPart::InputText { text }
        | ResponsesRequestContentPart::OutputText { text }
        | ResponsesRequestContentPart::Text { text } => Ok(text),
        ResponsesRequestContentPart::Unsupported => {
            Err(OpenAiError::UnsupportedContentPart { param })
        }
    }
}

/// Join validated text parts from one Responses content value.
///
/// # Errors
///
/// Returns [`OpenAiError`] when any content part is not text.
fn text_parts(
    parts: Vec<ResponsesRequestContentPart>,
    param: &'static str,
) -> Result<String, OpenAiError> {
    let text = parts
        .into_iter()
        .map(|part| text_part(part, param))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(text.join("\n"))
}

/// Lower string or part-list Responses content into text.
///
/// # Errors
///
/// Returns [`OpenAiError`] when part-list content contains a non-text part.
fn text_content(
    content: ResponsesRequestContent,
    param: &'static str,
) -> Result<String, OpenAiError> {
    match content {
        ResponsesRequestContent::Text(text) => Ok(text),
        ResponsesRequestContent::Parts(parts) => text_parts(parts, param),
    }
}

// -----------------------------------------------------------------------------
// RequiredName: Validates names carried by function-call items.
// -----------------------------------------------------------------------------

/// Require one non-empty function name.
///
/// # Errors
///
/// Returns [`OpenAiError`] when the name is absent or empty.
fn required_name(name: Option<String>, param: &'static str) -> Result<String, OpenAiError> {
    name.filter(|name| !name.is_empty())
        .ok_or(OpenAiError::MissingFunctionName { param })
}

// -----------------------------------------------------------------------------
// ResponseContext: Owns stable identifiers and the completed envelope.
// -----------------------------------------------------------------------------

/// Stable identifiers and completed values shared across stream events.
struct ResponseContext {
    /// Output item identifier.
    item_id: String,
    /// Function call identifier.
    call_id: String,
    /// Final typed output item.
    final_output: ResponsesResponseOutput,
    /// Complete terminal response envelope.
    envelope: ResponsesResponse,
}

impl From<&CompatTurnResponse> for ResponseContext {
    fn from(response: &CompatTurnResponse) -> Self {
        let item_id = match response.output {
            CompatOutput::Text(_) => format!("msg_{}", Uuid::now_v7().simple()),
            CompatOutput::ToolCall(_) => format!("fc_{}", Uuid::now_v7().simple()),
        };
        let call_id = format!("call_{}", Uuid::now_v7().simple());
        let output = ResponsesResponseOutput::from_compat(
            &response.output,
            ResponsesResponseIds {
                item: item_id.clone(),
                call: call_id.clone(),
            },
            ResponsesResponseStatus::Completed,
        );
        Self {
            item_id,
            call_id,
            final_output: output.clone(),
            envelope: ResponsesResponse {
                id: format!("resp_{}", Uuid::now_v7().simple()),
                object: "response",
                created_at: unix_timestamp(),
                status: ResponsesResponseStatus::Completed,
                error: None,
                incomplete_details: None,
                model: response.model.clone(),
                output: vec![output],
                output_text: responses_response_output_text(response),
                usage: response.usage.into(),
            },
        }
    }
}

// -----------------------------------------------------------------------------
// Sequence: Allocates monotonically increasing event positions.
// -----------------------------------------------------------------------------

/// Next sequence number assigned to a Responses stream event.
#[derive(Default)]
struct Sequence(
    /// Next available sequence number.
    usize,
);

impl Sequence {
    /// Return the current sequence number and advance the counter.
    fn next(&mut self) -> usize {
        let current = self.0;
        self.0 += 1;
        current
    }
}

// -----------------------------------------------------------------------------
// Stream: Renders OpenAI's Responses event sequence.
// -----------------------------------------------------------------------------

/// Serialize and append one Responses stream event.
///
/// # Errors
///
/// Returns [`EncodingError`] when the event cannot be serialized.
fn stream_push_event(
    events: &mut Vec<Event>,
    event: &ResponsesResponseStreamEvent,
) -> Result<(), EncodingError> {
    events.push(json_event(event)?);
    Ok(())
}

/// Append text item, delta, and completion events.
///
/// # Errors
///
/// Returns [`EncodingError`] when a text event cannot be serialized.
fn stream_push_text(
    events: &mut Vec<Event>,
    sequence: &mut Sequence,
    context: &ResponseContext,
    text: &str,
) -> Result<(), EncodingError> {
    stream_push_event(
        events,
        &ResponsesResponseStreamEvent::OutputItemAdded {
            sequence_number: sequence.next(),
            output_index: 0,
            item: ResponsesResponseOutput::Message {
                id: context.item_id.clone(),
                status: ResponsesResponseStatus::InProgress,
                role: AssistantRole::Assistant,
                content: Vec::new(),
            },
        },
    )?;
    for chunk in stream_chunks(text) {
        stream_push_event(
            events,
            &ResponsesResponseStreamEvent::OutputTextDelta {
                sequence_number: sequence.next(),
                item_id: context.item_id.clone(),
                output_index: 0,
                content_index: 0,
                delta: chunk,
            },
        )?;
    }
    stream_push_event(
        events,
        &ResponsesResponseStreamEvent::OutputTextDone {
            sequence_number: sequence.next(),
            item_id: context.item_id.clone(),
            output_index: 0,
            content_index: 0,
            text: text.to_owned(),
        },
    )
}

/// Append function item, argument delta, and completion events.
///
/// # Errors
///
/// Returns [`EncodingError`] when a function event cannot be serialized.
fn stream_push_tool(
    events: &mut Vec<Event>,
    sequence: &mut Sequence,
    context: &ResponseContext,
    call: &FunctionCall,
) -> Result<(), EncodingError> {
    stream_push_event(
        events,
        &ResponsesResponseStreamEvent::OutputItemAdded {
            sequence_number: sequence.next(),
            output_index: 0,
            item: ResponsesResponseOutput::FunctionCall {
                id: context.item_id.clone(),
                call_id: context.call_id.clone(),
                name: call.name.clone(),
                arguments: String::new(),
                status: ResponsesResponseStatus::InProgress,
            },
        },
    )?;
    let arguments = call.arguments.serialized();
    stream_push_event(
        events,
        &ResponsesResponseStreamEvent::FunctionArgumentsDelta {
            sequence_number: sequence.next(),
            item_id: context.item_id.clone(),
            output_index: 0,
            call_id: context.call_id.clone(),
            delta: arguments.clone(),
        },
    )?;
    stream_push_event(
        events,
        &ResponsesResponseStreamEvent::FunctionArgumentsDone {
            sequence_number: sequence.next(),
            item_id: context.item_id.clone(),
            output_index: 0,
            call_id: context.call_id.clone(),
            name: call.name.clone(),
            arguments,
        },
    )
}

/// Append output-specific events to one response stream.
///
/// # Errors
///
/// Returns [`EncodingError`] when an output event cannot be serialized.
fn stream_push_output(
    events: &mut Vec<Event>,
    sequence: &mut Sequence,
    context: &ResponseContext,
    output: &CompatOutput,
) -> Result<(), EncodingError> {
    match output {
        CompatOutput::Text(text) => stream_push_text(events, sequence, context, text),
        CompatOutput::ToolCall(call) => stream_push_tool(events, sequence, context, call),
    }
}

/// Render the complete SSE sequence for one neutral response.
///
/// # Errors
///
/// Returns [`EncodingError`] when any response event cannot be serialized.
fn stream_response(response: &CompatTurnResponse) -> Result<SseEvents, EncodingError> {
    let context = ResponseContext::from(response);
    let mut sequence = Sequence::default();
    let mut events = Vec::new();
    stream_push_event(
        &mut events,
        &ResponsesResponseStreamEvent::Created {
            sequence_number: sequence.next(),
            response: ResponsesResponseProgress {
                id: context.envelope.id.clone(),
                object: "response",
                created_at: context.envelope.created_at,
                status: ResponsesResponseStatus::InProgress,
                model: response.model.clone(),
                output: Vec::new(),
                error: None,
                incomplete_details: None,
            },
        },
    )?;
    stream_push_output(&mut events, &mut sequence, &context, &response.output)?;
    stream_push_event(
        &mut events,
        &ResponsesResponseStreamEvent::OutputItemDone {
            sequence_number: sequence.next(),
            output_index: 0,
            item: context.final_output,
        },
    )?;
    stream_push_event(
        &mut events,
        &ResponsesResponseStreamEvent::Completed {
            sequence_number: sequence.next(),
            response: context.envelope,
        },
    )?;
    Ok(SseEvents::from(events))
}

// -----------------------------------------------------------------------------
// OpenAiResponses: Handles and mounts the endpoint.
// -----------------------------------------------------------------------------

/// Handle one unary or streaming Responses API request.
async fn open_ai_responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<ResponsesRequest>, JsonRejection>,
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

    // Capture transport policy before lowering consumes the request body.
    let should_stream = payload.should_stream.unwrap_or(false);
    let request = match ResponsesLowering(payload).lower() {
        Ok(request) => request,
        // Provider validation failures use OpenAI's native envelope.
        Err(error) => {
            return OpenAiRejection::from_error(&error).into_response();
        }
    };

    // Apply shared resource bounds to the lowered request.
    // Complete one neutral turn before rendering Responses output.
    let response = match request.complete(state.config.limits) {
        Ok(response) => response,
        // Shared execution failures still render as OpenAI errors.
        Err(error) => {
            return OpenAiRejection::from_error(&error).into_response();
        }
    };

    if should_stream {
        match stream_response(&response) {
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
            post_with(open_ai_responses, |operation| {
                operation
                    .summary("OpenAI response")
                    .tag("openai")
                    .response::<200, Json<ResponsesResponse>>()
                    .default_response::<Json<OpenAiFailureResponse>>()
            }),
        )
    }
}
