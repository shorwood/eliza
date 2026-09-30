//! Provider-neutral conversation execution.
use std::num::NonZeroUsize;

use crate::eliza::{doctor, input_record};
use crate::errors::Error;
use crate::json::JsonObject;
use crate::structured_output::StructuredOutput;

// -----------------------------------------------------------------------------
// ToolCompleteText: Defines the deterministic tool-result acknowledgement.
// -----------------------------------------------------------------------------

/// Text returned after a client submits a tool result.
const TOOL_COMPLETE_TEXT: &str = "TOOL CALL COMPLETE";

// -----------------------------------------------------------------------------
// FunctionTool: Retains one offered function definition.
// -----------------------------------------------------------------------------

/// One client-defined function offered to ELIZA.
#[derive(Debug, Clone)]
pub struct FunctionTool {
    /// Function name used by the wire adapter.
    name: String,
    /// Serialized definition size retained for limits and accounting.
    definition_chars: usize,
}

impl FunctionTool {
    /// Retain a validated function name and its provider definition.
    #[must_use]
    pub fn new(name: String, definition_chars: usize) -> Self {
        Self {
            name,
            definition_chars,
        }
    }

    /// Count the serialized definition for request-size enforcement.
    fn char_count(&self) -> usize {
        self.definition_chars
    }
}

// -----------------------------------------------------------------------------
// FunctionCall: Normalizes one selected function invocation.
// -----------------------------------------------------------------------------

/// One normalized function call.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionCall {
    /// Function selected by the explicit fixture directive.
    pub name: String,
    /// Caller-supplied JSON object passed through unchanged.
    pub arguments: JsonObject,
}

impl FunctionCall {
    /// Parse the opt-in fixture directive from a complete user message.
    ///
    /// # Errors
    ///
    /// Returns a rejection when a message begins with `@tool` but does not
    /// contain a function name followed by a JSON object.
    fn parse_directive(text: &str) -> Result<Option<Self>, Error> {
        // Ordinary user text does not opt into the deterministic tool fixture.
        if !text.starts_with("@tool") {
            return Ok(None);
        }

        // The directive marker must be followed by call content.
        let Some(rest) = text.strip_prefix("@tool ") else {
            return Err(Error::MalformedToolDirective);
        };

        // A call must separate its function name from JSON arguments.
        let Some((name, arguments)) = rest.split_once(char::is_whitespace) else {
            return Err(Error::MissingToolArguments);
        };

        // Empty names cannot identify an offered function.
        if name.is_empty() {
            return Err(Error::MissingToolName);
        }
        let arguments = Self::parse_arguments(arguments.trim_start())?;
        Ok(Some(Self {
            name: name.to_owned(),
            arguments,
        }))
    }

    /// Decode the JSON object carried by one fixture directive.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection when the argument text is not a JSON object.
    fn parse_arguments(arguments: &str) -> Result<JsonObject, Error> {
        serde_json::from_str(arguments).map_err(|source| Error::InvalidToolArguments {
            detail: source.to_string(),
        })
    }

    /// Count the provider-visible name and serialized arguments.
    fn char_count(&self) -> usize {
        self.name.chars().count() + self.arguments.serialized().chars().count()
    }

    /// Approximate provider tokens in the function name and arguments.
    fn token_count(&self) -> usize {
        self.name.split_whitespace().count()
            + self.arguments.serialized().split_whitespace().count()
    }
}

// -----------------------------------------------------------------------------
// ToolChoice: Enforces the provider's function-selection policy.
// -----------------------------------------------------------------------------

/// Tool selection requested by a provider client.
#[derive(Debug, Clone, Default)]
pub enum ToolChoice {
    /// Call only when the prompt contains an explicit fixture directive.
    #[default]
    Auto,
    /// Do not permit tool calls.
    None,
    /// Require a fixture directive selecting any offered function.
    Required,
    /// Require a fixture directive selecting this function.
    Named(
        /// Sole function name permitted by the client.
        String,
    ),
    /// Require a fixture directive selecting one of these functions.
    Allowed(
        /// Function names permitted by the client.
        Vec<String>,
    ),
}

impl ToolChoice {
    /// Report whether the client requires an explicit function call.
    fn should_require_call(&self) -> bool {
        matches!(self, Self::Required | Self::Named(_) | Self::Allowed(_))
    }

    /// Check a directive-selected function against client policy.
    ///
    /// # Errors
    ///
    /// Returns a rejection when tool calls are disabled or the selected name
    /// is outside the client's named allowlist.
    fn validate_call(&self, name: &str) -> Result<(), Error> {
        match self {
            Self::Auto | Self::Required => Ok(()),
            Self::None => Err(Error::ToolDirectiveForbidden),
            Self::Named(expected) if expected == name => Ok(()),
            Self::Allowed(names) if names.iter().any(|allowed| allowed == name) => Ok(()),
            Self::Named(_) | Self::Allowed(_) => Err(Error::ToolChoiceDisallows {
                name: name.to_owned(),
            }),
        }
    }
}

// -----------------------------------------------------------------------------
// Turn: Represents normalized conversation history.
// -----------------------------------------------------------------------------

/// One provider message lowered into conversation history.
#[derive(Debug, Clone)]
pub enum Turn {
    /// Text replayed through the ELIZA engine.
    User(
        /// User-authored text.
        String,
    ),
    /// Prior model text retained only for request limits.
    Assistant(
        /// Model-authored text.
        String,
    ),
    /// Prior provider-native function call.
    ToolCall(
        /// Normalized function call.
        FunctionCall,
    ),
    /// Provider-native function result.
    ToolResult(
        /// Client-submitted function output.
        String,
    ),
}

impl Turn {
    /// Count the provider-visible content represented by this history turn.
    fn char_count(&self) -> usize {
        match self {
            Self::User(text) | Self::Assistant(text) | Self::ToolResult(text) => {
                text.chars().count()
            }
            Self::ToolCall(call) => call.char_count(),
        }
    }

    /// Approximate the provider tokens represented by this history turn.
    fn token_count(&self) -> usize {
        match self {
            Self::User(text) | Self::Assistant(text) | Self::ToolResult(text) => {
                text.split_whitespace().count()
            }
            Self::ToolCall(call) => call.token_count(),
        }
    }
}

// -----------------------------------------------------------------------------
// Positioned: Couples relevant transcript entries with their indexes.
// -----------------------------------------------------------------------------

/// Tool call paired with its transcript position.
struct PositionedToolCall<'turn> {
    /// Index used to inspect the preceding directive.
    index: usize,
    /// Normalized call submitted to the provider.
    call: &'turn FunctionCall,
}

/// Response-producing turn paired with its transcript position.
struct PositionedInput<'turn> {
    /// Index used to validate a tool result.
    index: usize,
    /// Latest user input or tool result.
    turn: &'turn Turn,
}

// -----------------------------------------------------------------------------
// Output: Represents one provider-neutral successful result.
// -----------------------------------------------------------------------------

/// Provider-neutral successful output.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// Ordinary assistant text.
    Text(
        /// ELIZA response text.
        String,
    ),
    /// One deterministic client function call.
    ToolCall(
        /// Normalized function call rendered by the provider adapter.
        FunctionCall,
    ),
}

// -----------------------------------------------------------------------------
// Request: Validates and executes normalized provider input.
// -----------------------------------------------------------------------------

/// One provider-neutral request.
#[derive(Debug)]
pub struct Request {
    /// System instructions retained for limits and token accounting.
    system_text: Vec<String>,
    /// Provider transcript normalized into execution order.
    turns: Vec<Turn>,
    /// Client function definitions available for a fixture call.
    tools: Vec<FunctionTool>,
    /// Client policy governing function selection.
    tool_choice: ToolChoice,
    /// Compiled text-response formatting applied after generation.
    output_format: StructuredOutput,
}

impl Request {
    /// Construct a request from provider-normalized conversation parts.
    #[must_use]
    pub fn new(
        system_text: Vec<String>,
        turns: Vec<Turn>,
        tools: Vec<FunctionTool>,
        tool_choice: ToolChoice,
        output_format: StructuredOutput,
    ) -> Self {
        Self {
            system_text,
            turns,
            tools,
            tool_choice,
            output_format,
        }
    }

    /// Approximate tokens across instructions, history, and tool definitions.
    fn prompt_tokens(&self) -> usize {
        let system_tokens = self
            .system_text
            .iter()
            .map(|text| text.split_whitespace().count())
            .sum::<usize>();
        let turn_tokens = self.turns.iter().map(Turn::token_count).sum::<usize>();
        let definition_chars = self
            .tools
            .iter()
            .map(FunctionTool::char_count)
            .sum::<usize>();
        let schema_chars = self.output_format.schema_chars();
        system_tokens + turn_tokens + (definition_chars + schema_chars).div_ceil(4)
    }

    /// Replay ordinary user turns through a fresh deterministic ELIZA session.
    ///
    /// # Errors
    ///
    /// Returns a rejection when the history has no ordinary user message.
    fn replay_eliza(&self) -> Result<String, Error> {
        let mut session = doctor()?.session()?;
        let mut output = None;
        for turn in &self.turns {
            let Turn::User(text) = turn else {
                continue;
            };
            if text.starts_with("@tool") {
                continue;
            }
            output = Some(session.respond(&input_record(text))?);
        }
        output.ok_or(Error::MissingOrdinaryUserTurn)
    }

    /// Count all provider-visible request content for size enforcement.
    fn input_char_count(&self) -> usize {
        let system = self
            .system_text
            .iter()
            .map(|text| text.chars().count())
            .sum::<usize>();
        let turns = self.turns.iter().map(Turn::char_count).sum::<usize>();
        let tools = self
            .tools
            .iter()
            .map(FunctionTool::char_count)
            .sum::<usize>();
        system + turns + tools + self.output_format.schema_chars()
    }

    /// Enforce configured history and serialized-input bounds.
    ///
    /// # Errors
    ///
    /// Returns a rejection when either configured bound is exceeded.
    fn validate_limits(
        &self,
        max_input_chars: NonZeroUsize,
        max_history_messages: NonZeroUsize,
    ) -> Result<(), Error> {
        // Reject histories that exceed the configured replay-work bound.
        if self.turns.len() > max_history_messages.get() {
            return Err(Error::TooManyTurns {
                actual: self.turns.len(),
                limit: max_history_messages.get(),
            });
        }

        // Enforce the combined provider-visible request size.
        let input_chars = self.input_char_count();

        // Reject serialized input beyond the configured character bound.
        if input_chars > max_input_chars.get() {
            return Err(Error::InputTooLarge {
                actual: input_chars,
                limit: max_input_chars.get(),
            });
        }
        Ok(())
    }

    /// Verify that a directive selected an offered and permitted function.
    ///
    /// # Errors
    ///
    /// Returns a rejection for an unknown or disallowed function name.
    fn validate_tool_call(&self, call: &FunctionCall) -> Result<(), Error> {
        // A directive cannot select a function absent from the request.
        if !self.tools.iter().any(|tool| tool.name == call.name) {
            return Err(Error::ToolNotOffered {
                name: call.name.clone(),
            });
        }
        self.tool_choice.validate_call(&call.name)
    }

    /// Find the nearest tool call preceding one result.
    fn preceding_tool_call(&self, result_index: usize) -> Option<PositionedToolCall<'_>> {
        let turns = &self.turns[..result_index];
        let indexed = turns.iter().enumerate();
        indexed.rev().find_map(|(index, turn)| match turn {
            Turn::ToolCall(call) => Some(PositionedToolCall { index, call }),
            Turn::User(_) | Turn::Assistant(_) | Turn::ToolResult(_) => None,
        })
    }

    /// Match a result to the preceding deterministic directive and function call.
    ///
    /// # Errors
    ///
    /// Returns a rejection when the history lacks a matching directive and
    /// provider-native call before the result.
    fn validate_tool_result(&self, result_index: usize) -> Result<(), Error> {
        // Every result must follow a provider-visible function call.
        let Some(positioned_call) = self.preceding_tool_call(result_index) else {
            return Err(Error::OrphanToolResult);
        };
        let prior_turns = &self.turns[..positioned_call.index];

        // The call must itself follow a user-authored fixture directive.
        let Some(marker) = prior_turns.iter().rev().find_map(|turn| match turn {
            Turn::User(text) => Some(text),
            Turn::Assistant(_) | Turn::ToolCall(_) | Turn::ToolResult(_) => None,
        }) else {
            return Err(Error::MissingToolDirective);
        };

        // The marker must parse as a complete function directive.
        let Some(expected) = FunctionCall::parse_directive(marker)? else {
            return Err(Error::MissingToolDirective);
        };

        // Provider history must preserve the exact requested call.
        if expected != *positioned_call.call {
            return Err(Error::MismatchedToolDirective);
        }
        Ok(())
    }

    /// Find the newest user input or tool result that can produce output.
    fn latest_input(&self) -> Option<PositionedInput<'_>> {
        let indexed = self.turns.iter().enumerate();
        indexed.rev().find_map(|(index, turn)| match turn {
            Turn::User(_) | Turn::ToolResult(_) => Some(PositionedInput { index, turn }),
            Turn::Assistant(_) | Turn::ToolCall(_) => None,
        })
    }

    /// Complete one latest user turn under the selected tool policy.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when a tool directive is malformed or conflicts
    /// with the available tools or selected tool policy.
    fn complete_user(&self, text: &str) -> Result<Output, Error> {
        match FunctionCall::parse_directive(text)? {
            Some(call) => {
                self.validate_tool_call(&call)?;
                Ok(Output::ToolCall(call))
            }
            None if self.tool_choice.should_require_call() => Err(Error::RequiredToolDirective),
            None => Ok(Output::Text(self.replay_eliza()?)),
        }
    }

    /// Execute ordinary text or the deterministic tool fixture.
    ///
    /// # Errors
    ///
    /// Returns a provider rejection for invalid history, directives, choices,
    /// or configured request limits.
    pub fn complete(
        self,
        max_input_chars: NonZeroUsize,
        max_history_messages: NonZeroUsize,
    ) -> Result<Response, Error> {
        self.validate_limits(max_input_chars, max_history_messages)?;

        // Select the newest turn that can produce a response.
        let Some(latest) = self.latest_input() else {
            return Err(Error::MissingUserText);
        };

        // Execute either a verified tool result or a new user request.
        let mut output = match latest.turn {
            Turn::ToolResult(_) => {
                self.validate_tool_result(latest.index)?;
                Output::Text(TOOL_COMPLETE_TEXT.to_owned())
            }
            Turn::User(text) => self.complete_user(text)?,
            Turn::Assistant(_) | Turn::ToolCall(_) => unreachable!(),
        };

        // Structured formatting applies only after final text is available.
        if let Output::Text(text) = &mut output {
            *text = self.output_format.render(std::mem::take(text));
        }

        let prompt = self.prompt_tokens();
        let completion = match &output {
            Output::Text(text) => text.split_whitespace().count(),
            Output::ToolCall(call) => call.token_count(),
        };

        // Combine prompt and completion accounting into wire-neutral usage.
        let usage = Usage {
            prompt,
            completion,
            total: prompt + completion,
        };

        Ok(Response { output, usage })
    }
}

// -----------------------------------------------------------------------------
// Usage: Reports provider-neutral token accounting.
// -----------------------------------------------------------------------------

/// Approximate provider token counts.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct Usage {
    /// Approximate input token count.
    pub prompt: usize,
    /// Approximate output token count.
    pub completion: usize,
    /// Prompt plus completion tokens.
    pub total: usize,
}

// -----------------------------------------------------------------------------
// Response: Carries one completed neutral turn.
// -----------------------------------------------------------------------------

/// One completed provider-neutral turn.
#[derive(Debug)]
pub struct Response {
    /// Text or function call selected by the shared executor.
    pub output: Output,
    /// Approximate token usage.
    pub usage: Usage,
}

// -----------------------------------------------------------------------------
// Tests: Verify neutral execution and deterministic tool fixtures.
// -----------------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::missing_panics_doc,
    reason = "test assertions are the intended panic contract"
)]
#[expect(
    rlib::missing_section_dividers,
    reason = "the shared it_should naming family already groups compact scenario tests"
)]
mod tests {
    use super::*;
    use crate::structured_output::StructuredOutput;

    /// Return the function definition offered by fixture tests.
    fn it_should_fixture_tool() -> FunctionTool {
        FunctionTool::new(
            "echo".to_owned(),
            serde_json::json!({"type":"function","name":"echo"})
                .to_string()
                .chars()
                .count(),
        )
    }

    /// Build one request from fixture-owned neutral parts.
    fn it_should_build_request(
        turns: Vec<Turn>,
        tools: Vec<FunctionTool>,
        tool_choice: ToolChoice,
        output_format: StructuredOutput,
    ) -> Request {
        Request::new(Vec::new(), turns, tools, tool_choice, output_format)
    }

    /// Build one fixture request with the shared model and tool definition.
    fn it_should_fixture_request(turns: Vec<Turn>, tool_choice: ToolChoice) -> Request {
        it_should_build_request(
            turns,
            vec![it_should_fixture_tool()],
            tool_choice,
            StructuredOutput::default(),
        )
    }

    /// Compile one structured-output schema for neutral execution fixtures.
    #[expect(
        rlib::ad_hoc_conversions,
        reason = "the test helper deliberately unwraps a JSON literal at the fixture boundary"
    )]
    fn it_should_compile_fixture_format(schema: serde_json::Value) -> StructuredOutput {
        let schema = serde_json::from_value::<JsonObject>(schema).unwrap();
        StructuredOutput::try_from(schema).unwrap()
    }

    #[test]
    fn it_should_call_an_offered_tool_for_an_explicit_directive() {
        let response = it_should_fixture_request(
            vec![Turn::User("@tool echo {\"value\":\"hello\"}".to_owned())],
            ToolChoice::Auto,
        )
        .complete(NonZeroUsize::MAX, NonZeroUsize::MAX)
        .unwrap();

        assert_eq!(
            response.output,
            Output::ToolCall(FunctionCall {
                name: "echo".to_owned(),
                arguments: serde_json::from_value(serde_json::json!({"value":"hello"})).unwrap(),
            })
        );
    }

    #[test]
    fn it_should_call_eliza_for_an_ordinary_turn() {
        let response =
            it_should_fixture_request(vec![Turn::User("Hello".to_owned())], ToolChoice::Auto)
                .complete(NonZeroUsize::MAX, NonZeroUsize::MAX)
                .unwrap();

        assert!(matches!(response.output, Output::Text(_)));
    }

    #[test]
    fn it_should_call_out_a_malformed_directive() {
        let error = it_should_fixture_request(
            vec![Turn::User("@tool echo []".to_owned())],
            ToolChoice::Auto,
        )
        .complete(NonZeroUsize::MAX, NonZeroUsize::MAX)
        .unwrap_err();

        assert!(matches!(error, Error::InvalidToolArguments { .. }));
    }

    #[test]
    fn it_should_call_for_a_required_choice() {
        let error =
            it_should_fixture_request(vec![Turn::User("Hello".to_owned())], ToolChoice::Required)
                .complete(NonZeroUsize::MAX, NonZeroUsize::MAX)
                .unwrap_err();

        assert!(matches!(error, Error::RequiredToolDirective));
    }

    #[test]
    fn it_should_call_only_the_named_choice() {
        let error = it_should_fixture_request(
            vec![Turn::User("@tool echo {}".to_owned())],
            ToolChoice::Named("other".to_owned()),
        )
        .complete(NonZeroUsize::MAX, NonZeroUsize::MAX)
        .unwrap_err();

        assert!(matches!(error, Error::ToolChoiceDisallows { .. }));
    }

    #[test]
    fn it_should_result_in_fixture_completion_for_a_matching_call() {
        let call = FunctionCall {
            name: "echo".to_owned(),
            arguments: serde_json::from_value(serde_json::json!({"value":"hello"})).unwrap(),
        };
        let response = it_should_fixture_request(
            vec![
                Turn::User("@tool echo {\"value\":\"hello\"}".to_owned()),
                Turn::ToolCall(call),
                Turn::ToolResult("hello".to_owned()),
            ],
            ToolChoice::Auto,
        )
        .complete(NonZeroUsize::MAX, NonZeroUsize::MAX)
        .unwrap();

        assert_eq!(response.output, Output::Text(TOOL_COMPLETE_TEXT.to_owned()));
    }

    #[test]
    fn it_should_result_in_eliza_resuming_after_tool_history() {
        let call = FunctionCall {
            name: "echo".to_owned(),
            arguments: serde_json::from_value(serde_json::json!({"value":"hello"})).unwrap(),
        };
        let response = it_should_fixture_request(
            vec![
                Turn::User("@tool echo {\"value\":\"hello\"}".to_owned()),
                Turn::ToolCall(call),
                Turn::ToolResult("hello".to_owned()),
                Turn::User("I am sad".to_owned()),
            ],
            ToolChoice::Auto,
        )
        .complete(NonZeroUsize::MAX, NonZeroUsize::MAX)
        .unwrap();

        assert_eq!(
            response.output,
            Output::Text("I AM SORRY TO HEAR YOU ARE SAD".to_owned())
        );
    }

    #[test]
    fn it_should_format_ordinary_text_before_completion_accounting() {
        let format = it_should_compile_fixture_format(serde_json::json!({
            "type":"object",
            "properties":{"ok":{"type":"boolean"}},
            "required":["ok"]
        }));
        let request = it_should_build_request(
            vec![Turn::User("I am sad".to_owned())],
            Vec::new(),
            ToolChoice::Auto,
            format,
        );
        let response = request
            .complete(NonZeroUsize::MAX, NonZeroUsize::MAX)
            .unwrap();

        assert_eq!(response.output, Output::Text(r#"{"ok":false}"#.to_owned()));
        assert_eq!(response.usage.completion, 1);
    }

    #[test]
    fn it_should_not_format_tool_calls() {
        // Build a formatted request whose output is a function call.
        let request = it_should_build_request(
            vec![Turn::User("@tool echo {}".to_owned())],
            vec![it_should_fixture_tool()],
            ToolChoice::Auto,
            StructuredOutput::json_object(),
        );

        // Complete the request under unbounded fixture limits.
        let response = request
            .complete(NonZeroUsize::MAX, NonZeroUsize::MAX)
            .unwrap();

        assert!(matches!(response.output, Output::ToolCall(_)));
    }

    #[test]
    fn it_should_format_tool_result_acknowledgements() {
        let call = FunctionCall {
            name: "echo".to_owned(),
            arguments: serde_json::from_value(serde_json::json!({})).unwrap(),
        };
        let request = it_should_build_request(
            vec![
                Turn::User("@tool echo {}".to_owned()),
                Turn::ToolCall(call),
                Turn::ToolResult("done".to_owned()),
            ],
            vec![it_should_fixture_tool()],
            ToolChoice::Auto,
            StructuredOutput::json_object(),
        );
        let response = request
            .complete(NonZeroUsize::MAX, NonZeroUsize::MAX)
            .unwrap();

        assert_eq!(
            response.output,
            Output::Text(r#"{"response":"TOOL CALL COMPLETE"}"#.to_owned())
        );
    }

    #[test]
    fn it_should_count_the_serialized_schema_toward_input_limits() {
        // Account for the exact serialized schema and user text.
        let schema = serde_json::json!({"type":"string"});
        let format = it_should_compile_fixture_format(schema.clone());
        let schema_chars = schema.to_string().chars().count();

        // Derive the failing limit from the request's complete character count.
        let user_input = "Hello";
        let input_chars = user_input.chars().count();
        let expected_actual = input_chars + schema_chars;
        let expected_limit = expected_actual - 1;
        let max_input_chars = NonZeroUsize::new(expected_limit).unwrap();

        // Submit the request with a limit one character below its full input.
        let request = it_should_build_request(
            vec![Turn::User(user_input.to_owned())],
            Vec::new(),
            ToolChoice::Auto,
            format,
        );

        // Capture the boundary rejection for exact accounting assertions.
        let error = request
            .complete(max_input_chars, NonZeroUsize::MAX)
            .unwrap_err();

        assert!(matches!(
            error,
            Error::InputTooLarge { actual, limit }
                if actual == expected_actual && limit == expected_limit
        ));
    }
}
