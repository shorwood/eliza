//! `OpenAI` Responses API adapter.
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
use eliza_http::response::{SseEvents, json_event, stream_chunks, unix_timestamp};
use eliza_modality_chat as chat;
use eliza_modality_image as image;
use uuid::Uuid;

use super::errors::{OpenAiError, OpenAiFailureResponse, OpenAiRejection};
use super::types::{
    AssistantRole, ResponsesReasoningSummary, ResponsesRequest, ResponsesRequestContent,
    ResponsesRequestContentPart, ResponsesRequestInput, ResponsesRequestInputItem,
    ResponsesRequestInputReasoningSummary, ResponsesRequestInputTypedItem, ResponsesRequestRole,
    ResponsesRequestToolOutput, ResponsesResponse, ResponsesResponseIds, ResponsesResponseOutput,
    ResponsesResponseProgress, ResponsesResponseStatus, ResponsesResponseStreamEvent,
};
use crate::context::AppState;

impl TryFrom<ResponsesRequest> for chat::turn::Request {
    type Error = OpenAiError;

    fn try_from(payload: ResponsesRequest) -> Result<Self, Self::Error> {
        let include_reasoning = payload.should_include_reasoning()?;
        let output_format = payload
            .text
            .and_then(|config| config.format)
            .map(TryInto::try_into)
            .transpose()?
            .unwrap_or_default();

        // Separate request-level instructions from replayable input turns.
        let mut system = Vec::new();
        if let Some(instructions) = payload.instructions {
            system.push(text_content(instructions, "instructions")?);
        }
        let turns = lower_input(payload.input, &mut system)?;

        // Lower tool declarations and selection policy independently.
        let tools = payload.tools.unwrap_or_default().try_into()?;
        let tool_choice = payload
            .tool_choice
            .map(TryInto::try_into)
            .transpose()?
            .unwrap_or_default();

        let request = chat::turn::Request::new(system, turns, tools, tool_choice, output_format);
        Ok(if include_reasoning {
            request.with_reasoning()
        } else {
            request
        })
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
    turns: &mut Vec<chat::turn::Turn>,
) -> Result<(), OpenAiError> {
    match role {
        ResponsesRequestRole::User => {
            let content: UserContent = content.try_into()?;
            turns.push(chat::turn::Turn::user_with_images(
                content.text,
                content.images,
            ));
        }
        ResponsesRequestRole::System | ResponsesRequestRole::Developer => {
            system.push(text_content(content, "input.content")?);
        }
        ResponsesRequestRole::Assistant => turns.push(chat::turn::Turn::Assistant(text_content(
            content,
            "input.content",
        )?)),
        // Unknown provider roles cannot be replayed safely.
        ResponsesRequestRole::Unsupported => return Err(OpenAiError::UnsupportedResponsesRole),
    }
    Ok(())
}

/// Extract readable text from a supported prior summary part.
fn lower_reasoning_summary_text(part: ResponsesRequestInputReasoningSummary) -> Option<String> {
    match part {
        ResponsesRequestInputReasoningSummary::SummaryText { text } => Some(text),
        ResponsesRequestInputReasoningSummary::Unsupported => None,
    }
}

/// Preserve readable and opaque reasoning material in replay history.
fn lower_reasoning_item(
    summary: Option<Vec<ResponsesRequestInputReasoningSummary>>,
    encrypted_content: Option<String>,
    turns: &mut Vec<chat::turn::Turn>,
) {
    let parts = summary.unwrap_or_default();
    let mut history = parts
        .into_iter()
        .filter_map(lower_reasoning_summary_text)
        .collect::<Vec<_>>();
    history.extend(encrypted_content);

    // Empty reasoning items do not add a transcript turn.
    if history.is_empty() {
        return;
    }
    turns.push(chat::turn::Turn::Assistant(history.join(" ")));
}

/// Lower one typed non-message Responses input item.
///
/// # Errors
///
/// Returns [`OpenAiError`] when the item is unsupported or incomplete.
fn lower_typed_item(
    item: ResponsesRequestInputTypedItem,
    turns: &mut Vec<chat::turn::Turn>,
) -> Result<(), OpenAiError> {
    match item {
        ResponsesRequestInputTypedItem::FunctionCall { name, arguments } => {
            let name = required_name(name, "input.name")?;
            let arguments = arguments.ok_or(OpenAiError::MissingFunctionCallArguments)?;
            turns.push(chat::turn::Turn::ToolCall(chat::turn::FunctionCall {
                name,
                arguments: arguments.into_object("input.arguments")?,
            }));
        }
        ResponsesRequestInputTypedItem::FunctionCallOutput { output } => {
            let output = output.ok_or(OpenAiError::MissingFunctionOutput)?;
            turns.push(chat::turn::Turn::ToolResult(match output {
                ResponsesRequestToolOutput::Text(text) => text,
                ResponsesRequestToolOutput::Object(object) => object.serialized(),
            }));
        }
        ResponsesRequestInputTypedItem::Reasoning {
            summary,
            encrypted_content,
        } => lower_reasoning_item(summary, encrypted_content, turns),
        ResponsesRequestInputTypedItem::Unsupported => Err(OpenAiError::UnsupportedInputItem)?,
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
    turns: &mut Vec<chat::turn::Turn>,
) -> Result<(), OpenAiError> {
    match item {
        ResponsesRequestInputItem::Message { role, content } => {
            lower_message_item(role, content, system, turns)?;
        }
        ResponsesRequestInputItem::Typed { item } => lower_typed_item(item, turns)?,
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
) -> Result<Vec<chat::turn::Turn>, OpenAiError> {
    match input {
        ResponsesRequestInput::Text(text) => Ok(vec![chat::turn::Turn::from(text)]),
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
// UserContent: Normalizes Responses user text and images.
// -----------------------------------------------------------------------------

/// Text and images lowered from one Responses user message.
struct UserContent {
    /// Joined user text parts.
    text: String,
    /// Images retained in content-part order.
    images: Vec<image::source::Source>,
}

impl UserContent {
    /// Join user text while retaining ordered image sources.
    ///
    /// # Errors
    ///
    /// Returns a typed failure when any content part cannot be normalized.
    fn from_parts(parts: Vec<ResponsesRequestContentPart>) -> Result<Self, OpenAiError> {
        let mut text = Vec::new();
        let mut images = Vec::new();
        for part in parts {
            match part {
                ResponsesRequestContentPart::InputText { text: part }
                | ResponsesRequestContentPart::OutputText { text: part }
                | ResponsesRequestContentPart::Text { text: part } => text.push(part),
                ResponsesRequestContentPart::InputImage {
                    image_url,
                    file_id,
                    detail,
                } => {
                    let _ = detail;
                    images.push(Self::image_source(image_url, file_id)?);
                }
                ResponsesRequestContentPart::Unsupported => {
                    Err(OpenAiError::UnsupportedContentPart {
                        param: "input.content",
                    })?;
                }
            }
        }
        Ok(Self {
            text: text.join("\n"),
            images,
        })
    }

    /// Normalize exactly one Responses image source.
    ///
    /// # Errors
    ///
    /// Returns a typed failure when the source is missing, conflicting, or invalid.
    fn image_source(
        image_url: Option<String>,
        file_id: Option<String>,
    ) -> Result<image::source::Source, OpenAiError> {
        image::source::Source::url_or_reference(image_url, file_id).map_err(OpenAiError::Image)
    }
}

impl TryFrom<ResponsesRequestContent> for UserContent {
    type Error = OpenAiError;

    fn try_from(content: ResponsesRequestContent) -> Result<Self, Self::Error> {
        match content {
            ResponsesRequestContent::Text(text) => Ok(Self {
                text,
                images: Vec::new(),
            }),
            ResponsesRequestContent::Parts(parts) => Self::from_parts(parts),
        }
    }
}

// -----------------------------------------------------------------------------
// Text: Normalizes non-user Responses content into replayable text.
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
        ResponsesRequestContentPart::InputImage { .. }
        | ResponsesRequestContentPart::Unsupported => {
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
    /// Reasoning output-item identifier when a summary was requested.
    reasoning_id: Option<String>,
    /// Final typed output item.
    final_output: ResponsesResponseOutput,
    /// Complete terminal response envelope.
    envelope: ResponsesResponse,
}

impl ResponseContext {
    /// Attach provider-owned response facts to one canonical result.
    fn new(model: ModelId, response: &chat::turn::Response) -> Self {
        let item_id = match response.output {
            chat::turn::Output::Text(_) => format!("msg_{}", Uuid::now_v7().simple()),
            chat::turn::Output::ToolCall(_) => format!("fc_{}", Uuid::now_v7().simple()),
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
        let output_text = match &response.output {
            chat::turn::Output::Text(text) => text.clone(),
            chat::turn::Output::ToolCall(_) => String::new(),
        };
        let reasoning_id = response
            .reasoning
            .as_ref()
            .map(|_| format!("rs_{}", Uuid::now_v7().simple()));
        let mut outputs = response
            .reasoning
            .as_ref()
            .map_or_else(Vec::new, |reasoning| {
                vec![ResponsesResponseOutput::Reasoning {
                    id: reasoning_id.clone().unwrap_or_default(),
                    status: ResponsesResponseStatus::Completed,
                    summary: vec![ResponsesReasoningSummary::SummaryText {
                        text: reasoning.text.clone(),
                    }],
                    encrypted_content: reasoning.signature.clone(),
                }]
            });
        outputs.push(output.clone());
        Self {
            item_id,
            call_id,
            reasoning_id,
            final_output: output.clone(),
            envelope: ResponsesResponse {
                id: format!("resp_{}", Uuid::now_v7().simple()),
                object: "response",
                created_at: unix_timestamp(),
                status: ResponsesResponseStatus::Completed,
                error: None,
                incomplete_details: None,
                model,
                output: outputs,
                output_text,
                usage: response.usage.into(),
            },
        }
    }
}

// -----------------------------------------------------------------------------
// ResponseStream: Owns OpenAI's ordered, monotonically numbered event sequence.
// -----------------------------------------------------------------------------

/// Event builder for one completed Responses API result.
struct ResponseStream {
    /// Stable identifiers and final response values shared by all events.
    context: ResponseContext,
    /// Encoded events in delivery order.
    events: Vec<Event>,
    /// Sequence number assigned to the next event.
    next_sequence: usize,
}

#[expect(
    clippy::missing_errors_doc,
    reason = "private helpers propagate the builder's documented encoding failure"
)]
impl ResponseStream {
    /// Return the current sequence number and advance the counter.
    fn next_sequence(&mut self) -> usize {
        let current = self.next_sequence;
        self.next_sequence += 1;
        current
    }

    /// Serialize and append one event.
    fn push(&mut self, event: &ResponsesResponseStreamEvent) -> Result<(), EncodingError> {
        self.events.push(json_event(event)?);
        Ok(())
    }

    /// Append text item, delta, and completion events.
    fn push_text(&mut self, output_index: usize, text: &str) -> Result<(), EncodingError> {
        let sequence_number = self.next_sequence();
        self.push(&ResponsesResponseStreamEvent::OutputItemAdded {
            sequence_number,
            output_index,
            item: ResponsesResponseOutput::Message {
                id: self.context.item_id.clone(),
                status: ResponsesResponseStatus::InProgress,
                role: AssistantRole::Assistant,
                content: Vec::new(),
            },
        })?;
        for chunk in stream_chunks(text) {
            let sequence_number = self.next_sequence();
            self.push(&ResponsesResponseStreamEvent::OutputTextDelta {
                sequence_number,
                item_id: self.context.item_id.clone(),
                output_index,
                content_index: 0,
                delta: chunk,
            })?;
        }
        let sequence_number = self.next_sequence();
        self.push(&ResponsesResponseStreamEvent::OutputTextDone {
            sequence_number,
            item_id: self.context.item_id.clone(),
            output_index,
            content_index: 0,
            text: text.to_owned(),
        })
    }

    /// Append function item, argument delta, and completion events.
    fn push_tool(
        &mut self,
        output_index: usize,
        call: &chat::turn::FunctionCall,
    ) -> Result<(), EncodingError> {
        let sequence_number = self.next_sequence();
        self.push(&ResponsesResponseStreamEvent::OutputItemAdded {
            sequence_number,
            output_index,
            item: ResponsesResponseOutput::FunctionCall {
                id: self.context.item_id.clone(),
                call_id: self.context.call_id.clone(),
                name: call.name.clone(),
                arguments: String::new(),
                status: ResponsesResponseStatus::InProgress,
            },
        })?;
        let arguments = call.arguments.serialized();
        let sequence_number = self.next_sequence();
        self.push(&ResponsesResponseStreamEvent::FunctionArgumentsDelta {
            sequence_number,
            item_id: self.context.item_id.clone(),
            output_index,
            call_id: self.context.call_id.clone(),
            delta: arguments.clone(),
        })?;
        let sequence_number = self.next_sequence();
        self.push(&ResponsesResponseStreamEvent::FunctionArgumentsDone {
            sequence_number,
            item_id: self.context.item_id.clone(),
            output_index,
            call_id: self.context.call_id.clone(),
            name: call.name.clone(),
            arguments,
        })
    }

    /// Append output-specific events.
    fn push_output(
        &mut self,
        output_index: usize,
        output: &chat::turn::Output,
    ) -> Result<(), EncodingError> {
        match output {
            chat::turn::Output::Text(text) => self.push_text(output_index, text),
            chat::turn::Output::ToolCall(call) => self.push_tool(output_index, call),
        }
    }

    /// Append a complete native reasoning-summary lifecycle.
    fn push_reasoning(&mut self, reasoning: &chat::turn::Reasoning) -> Result<(), EncodingError> {
        let item_id = self.context.reasoning_id.clone().unwrap_or_default();
        let sequence_number = self.next_sequence();
        self.push(&ResponsesResponseStreamEvent::OutputItemAdded {
            sequence_number,
            output_index: 0,
            item: ResponsesResponseOutput::Reasoning {
                id: item_id.clone(),
                status: ResponsesResponseStatus::InProgress,
                summary: Vec::new(),
                encrypted_content: reasoning.signature.clone(),
            },
        })?;
        let sequence_number = self.next_sequence();
        self.push(&ResponsesResponseStreamEvent::ReasoningSummaryPartAdded {
            sequence_number,
            item_id: item_id.clone(),
            output_index: 0,
            summary_index: 0,
            part: ResponsesReasoningSummary::SummaryText {
                text: String::new(),
            },
        })?;
        for chunk in stream_chunks(&reasoning.text) {
            let sequence_number = self.next_sequence();
            self.push(&ResponsesResponseStreamEvent::ReasoningSummaryTextDelta {
                sequence_number,
                item_id: item_id.clone(),
                output_index: 0,
                summary_index: 0,
                delta: chunk,
            })?;
        }
        let summary = ResponsesReasoningSummary::SummaryText {
            text: reasoning.text.clone(),
        };
        let sequence_number = self.next_sequence();
        self.push(&ResponsesResponseStreamEvent::ReasoningSummaryTextDone {
            sequence_number,
            item_id: item_id.clone(),
            output_index: 0,
            summary_index: 0,
            text: reasoning.text.clone(),
        })?;
        let sequence_number = self.next_sequence();
        self.push(&ResponsesResponseStreamEvent::ReasoningSummaryPartDone {
            sequence_number,
            item_id: item_id.clone(),
            output_index: 0,
            summary_index: 0,
            part: summary.clone(),
        })?;
        let sequence_number = self.next_sequence();
        self.push(&ResponsesResponseStreamEvent::OutputItemDone {
            sequence_number,
            output_index: 0,
            item: ResponsesResponseOutput::Reasoning {
                id: item_id,
                status: ResponsesResponseStatus::Completed,
                summary: vec![summary],
                encrypted_content: reasoning.signature.clone(),
            },
        })
    }

    /// Render the complete SSE sequence for one neutral response.
    ///
    /// # Errors
    ///
    /// Returns [`EncodingError`] when any response event cannot be serialized.
    fn render(
        model: ModelId,
        response: &chat::turn::Response,
    ) -> Result<Vec<Event>, EncodingError> {
        let mut stream = Self {
            context: ResponseContext::new(model, response),
            events: Vec::new(),
            next_sequence: 0,
        };
        let sequence_number = stream.next_sequence();
        stream.push(&ResponsesResponseStreamEvent::Created {
            sequence_number,
            response: ResponsesResponseProgress {
                id: stream.context.envelope.id.clone(),
                object: "response",
                created_at: stream.context.envelope.created_at,
                status: ResponsesResponseStatus::InProgress,
                model: stream.context.envelope.model.clone(),
                output: Vec::new(),
                error: None,
                incomplete_details: None,
            },
        })?;
        if let Some(reasoning) = &response.reasoning {
            stream.push_reasoning(reasoning)?;
        }
        let output_index = usize::from(response.reasoning.is_some());
        stream.push_output(output_index, &response.output)?;
        let output_done_sequence = stream.next_sequence();
        let completed_sequence = stream.next_sequence();
        let Self {
            context,
            mut events,
            ..
        } = stream;
        events.push(json_event(&ResponsesResponseStreamEvent::OutputItemDone {
            sequence_number: output_done_sequence,
            output_index,
            item: context.final_output,
        })?);
        events.push(json_event(&ResponsesResponseStreamEvent::Completed {
            sequence_number: completed_sequence,
            response: context.envelope,
        })?);
        Ok(events)
    }
}

// -----------------------------------------------------------------------------
// OpenAiResponses: Handles the endpoint.
// -----------------------------------------------------------------------------

/// Handle one unary or streaming Responses API request.
async fn open_ai_responses(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<ResponsesRequest>, JsonRejection>,
) -> Response {
    // Authentication failures use OpenAI's native error envelope.
    if let Err(error) = state.config.authenticate(&headers, ProviderAuth::Bearer) {
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
    let model = payload.model.clone();
    let should_stream = payload.should_stream.unwrap_or(false);
    let request: chat::turn::Request = match payload.try_into() {
        Ok(request) => request,
        // Provider validation failures use OpenAI's native envelope.
        Err(error) => {
            return OpenAiRejection::request(&error, "input").into_response();
        }
    };

    // Apply shared resource bounds to the lowered request.
    // Complete one neutral turn before rendering Responses output.
    let response = match request.complete(
        state.config.limits.max_input_chars(),
        state.config.limits.max_history_messages(),
    ) {
        Ok(response) => response,
        // Shared execution failures still render as OpenAI errors.
        Err(error) => {
            return OpenAiRejection::chat(&error, "input").into_response();
        }
    };

    if should_stream {
        match ResponseStream::render(model, &response) {
            Ok(events) => SseEvents::from(events)
                .with_delay(state.config.stream_delay_ms)
                .into_response(),
            Err(error) => OpenAiRejection::from_error(&error).into_response(),
        }
    } else {
        Json(ResponseContext::new(model, &response).envelope).into_response()
    }
}

// -----------------------------------------------------------------------------
// Router: Publishes the Responses endpoint.
// -----------------------------------------------------------------------------

/// Build the Responses route.
pub(super) fn router() -> ApiRouter<AppState> {
    ApiRouter::new().api_route(
        "/v1/responses",
        post_with(open_ai_responses, |operation| {
            operation
                .summary("OpenAI response")
                .tag("openai")
                .response::<200, Json<ResponsesResponse>>()
                .response::<429, Json<OpenAiFailureResponse>>()
                .response::<503, Json<OpenAiFailureResponse>>()
                .default_response::<Json<OpenAiFailureResponse>>()
        })
        .layer(DefaultBodyLimit::max(image::limits::LIMIT_JSON_BODY)),
    )
}
