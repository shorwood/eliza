//! Provider-neutral conversation execution.
#![expect(
    rlib::missing_section_dividers,
    rlib::undocumented_early_returns,
    reason = "the compact execution pipeline needs no per-type dividers, and validation errors explain their guards"
)]

use std::num::NonZeroUsize;

use schemars::JsonSchema;
use serde::Serialize;

use super::http::ProviderRejection;
use super::json::JsonObject;
use super::model::ModelId;
use crate::eliza::engine::{ElizaSession, doctor_script};

/// Text returned after a client submits a tool result.
const TOOL_COMPLETE_TEXT: &str = "TOOL CALL COMPLETE";

/// One client-defined function offered to ELIZA.
#[derive(Debug, Clone)]
pub(crate) struct FunctionTool {
    /// Function name used by the wire adapter.
    name: String,
    /// Serialized definition size retained for limits and accounting.
    definition_chars: usize,
}

impl FunctionTool {
    /// Retain a validated function name and its provider definition.
    pub(crate) fn new(name: String, definition_chars: usize) -> Self {
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

/// One normalized function call.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FunctionCall {
    /// Function selected by the explicit fixture directive.
    pub(crate) name: String,
    /// Caller-supplied JSON object passed through unchanged.
    pub(crate) arguments: JsonObject,
}

impl FunctionCall {
    /// Parse the opt-in fixture directive from a complete user message.
    ///
    /// # Errors
    ///
    /// Returns a rejection when a message begins with `@tool` but does not
    /// contain a function name followed by a JSON object.
    fn parse_directive(text: &str) -> Result<Option<Self>, ProviderRejection> {
        if !text.starts_with("@tool") {
            return Ok(None);
        }
        let Some(rest) = text.strip_prefix("@tool ") else {
            return Err(ProviderRejection::invalid(
                "messages",
                "tool directive must be `@tool <name> <json-object>`",
            ));
        };
        let Some((name, arguments)) = rest.split_once(char::is_whitespace) else {
            return Err(ProviderRejection::invalid(
                "messages",
                "tool directive must include a JSON object",
            ));
        };
        if name.is_empty() {
            return Err(ProviderRejection::invalid(
                "messages",
                "tool directive must include a function name",
            ));
        }
        let arguments =
            serde_json::from_str::<JsonObject>(arguments.trim_start()).map_err(|error| {
                ProviderRejection::invalid("messages", format!("invalid tool arguments: {error}"))
            })?;
        Ok(Some(Self {
            name: name.to_owned(),
            arguments,
        }))
    }

    /// Count the provider-visible name and serialized arguments.
    fn char_count(&self) -> usize {
        self.name.chars().count() + self.arguments.serialized().chars().count()
    }
}

/// Tool selection requested by a provider client.
#[derive(Debug, Clone, Default)]
pub(crate) enum ToolChoice {
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
    fn validate_call(&self, name: &str) -> Result<(), ProviderRejection> {
        match self {
            Self::Auto | Self::Required => Ok(()),
            Self::None => Err(ProviderRejection::invalid(
                "tool_choice",
                "tool_choice forbids the explicit @tool directive",
            )),
            Self::Named(expected) if expected == name => Ok(()),
            Self::Allowed(names) if names.iter().any(|allowed| allowed == name) => Ok(()),
            Self::Named(_) | Self::Allowed(_) => Err(ProviderRejection::invalid(
                "tool_choice",
                format!("tool_choice does not allow function `{name}`"),
            )),
        }
    }
}

/// One provider message lowered into conversation history.
#[derive(Debug, Clone)]
pub(crate) enum CompatTurn {
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

impl CompatTurn {
    /// Count the provider-visible content represented by this history turn.
    fn char_count(&self) -> usize {
        match self {
            Self::User(text) | Self::Assistant(text) | Self::ToolResult(text) => {
                text.chars().count()
            }
            Self::ToolCall(call) => call.char_count(),
        }
    }
}

/// Provider-neutral successful output.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CompatOutput {
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

/// One provider-neutral request.
#[derive(Debug)]
pub(crate) struct CompatTurnRequest {
    /// Model identifier echoed in the provider response.
    model: ModelId,
    /// System instructions retained for limits and token accounting.
    system_text: Vec<String>,
    /// Provider transcript normalized into execution order.
    turns: Vec<CompatTurn>,
    /// Client function definitions available for a fixture call.
    tools: Vec<FunctionTool>,
    /// Client policy governing function selection.
    tool_choice: ToolChoice,
}

impl CompatTurnRequest {
    /// Build a request from provider-lowered conversation state.
    pub(crate) fn new(
        model: ModelId,
        system_text: Vec<String>,
        turns: Vec<CompatTurn>,
        tools: Vec<FunctionTool>,
        tool_choice: ToolChoice,
    ) -> Self {
        Self {
            model,
            system_text,
            turns,
            tools,
            tool_choice,
        }
    }

    /// Approximate tokens across instructions, history, and tool definitions.
    fn prompt_tokens(&self) -> usize {
        let mut texts = self.system_text.clone();
        texts.extend(self.turns.iter().map(|turn| match turn {
            CompatTurn::User(text) | CompatTurn::Assistant(text) | CompatTurn::ToolResult(text) => {
                text.clone()
            }
            CompatTurn::ToolCall(call) => format!("{} {}", call.name, call.arguments.serialized()),
        }));
        let definition_chars = self
            .tools
            .iter()
            .map(FunctionTool::char_count)
            .sum::<usize>();
        approximate_tokens(texts.iter().map(String::as_str)) + definition_chars.div_ceil(4)
    }

    /// Replay ordinary user turns through a fresh deterministic ELIZA session.
    ///
    /// # Errors
    ///
    /// Returns a rejection when the history has no ordinary user message.
    fn replay_eliza(&self) -> Result<String, ProviderRejection> {
        let mut session = ElizaSession::from(doctor_script());
        let mut output = None;
        for turn in &self.turns {
            let CompatTurn::User(text) = turn else {
                continue;
            };
            if text.starts_with("@tool") {
                continue;
            }
            output = Some(session.respond(text).output);
        }
        output.ok_or_else(|| {
            ProviderRejection::invalid("messages", "at least one ordinary user turn is required")
        })
    }

    /// Enforce configured history and serialized-input bounds.
    ///
    /// # Errors
    ///
    /// Returns a rejection when either configured bound is exceeded.
    fn validate_limits(&self, limits: RequestLimits) -> Result<(), ProviderRejection> {
        if self.turns.len() > limits.max_history_messages.get() {
            return Err(ProviderRejection::too_large(format!(
                "too many conversation turns: {} > {}",
                self.turns.len(),
                limits.max_history_messages
            )));
        }

        let input_chars = self
            .system_text
            .iter()
            .map(|text| text.chars().count())
            .chain(self.turns.iter().map(CompatTurn::char_count))
            .chain(self.tools.iter().map(FunctionTool::char_count))
            .sum::<usize>();
        if input_chars > limits.max_input_chars.get() {
            return Err(ProviderRejection::too_large(format!(
                "input text is too large: {input_chars} > {}",
                limits.max_input_chars
            )));
        }
        Ok(())
    }

    /// Verify that a directive selected an offered and permitted function.
    ///
    /// # Errors
    ///
    /// Returns a rejection for an unknown or disallowed function name.
    fn validate_tool_call(&self, call: &FunctionCall) -> Result<(), ProviderRejection> {
        if !self.tools.iter().any(|tool| tool.name == call.name) {
            return Err(ProviderRejection::invalid(
                "tools",
                format!("function `{}` was not offered", call.name),
            ));
        }
        self.tool_choice.validate_call(&call.name)
    }

    /// Match a result to the preceding deterministic directive and function call.
    ///
    /// # Errors
    ///
    /// Returns a rejection when the history lacks a matching directive and
    /// provider-native call before the result.
    fn validate_tool_result(&self, result_index: usize) -> Result<(), ProviderRejection> {
        let Some((call_index, CompatTurn::ToolCall(call))) = self.turns[..result_index]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, turn)| matches!(turn, CompatTurn::ToolCall(_)))
        else {
            return Err(ProviderRejection::invalid(
                "messages",
                "tool result is missing its preceding tool call",
            ));
        };
        let Some(marker) = self.turns[..call_index].iter().rev().find_map(|turn| {
            if let CompatTurn::User(text) = turn {
                Some(text)
            } else {
                None
            }
        }) else {
            return Err(ProviderRejection::invalid(
                "messages",
                "tool call is missing its @tool directive",
            ));
        };
        let Some(expected) = FunctionCall::parse_directive(marker)? else {
            return Err(ProviderRejection::invalid(
                "messages",
                "tool call is missing its @tool directive",
            ));
        };
        if expected != *call {
            return Err(ProviderRejection::invalid(
                "messages",
                "tool call does not match its @tool directive",
            ));
        }
        Ok(())
    }

    /// Execute ordinary text or the deterministic tool fixture.
    ///
    /// # Errors
    ///
    /// Returns a provider rejection for invalid history, directives, choices,
    /// or configured request limits.
    pub(crate) fn complete(
        self,
        limits: RequestLimits,
    ) -> Result<CompatTurnResponse, ProviderRejection> {
        self.validate_limits(limits)?;

        let Some((index, latest)) = self
            .turns
            .iter()
            .enumerate()
            .rev()
            .find(|(_, turn)| matches!(turn, CompatTurn::User(_) | CompatTurn::ToolResult(_)))
        else {
            return Err(ProviderRejection::invalid(
                "messages",
                "at least one user text turn is required",
            ));
        };

        let output = match latest {
            CompatTurn::ToolResult(_) => {
                self.validate_tool_result(index)?;
                CompatOutput::Text(TOOL_COMPLETE_TEXT.to_owned())
            }
            CompatTurn::User(text) => match FunctionCall::parse_directive(text)? {
                Some(call) => {
                    self.validate_tool_call(&call)?;
                    CompatOutput::ToolCall(call)
                }
                None if self.tool_choice.should_require_call() => {
                    return Err(ProviderRejection::invalid(
                        "tool_choice",
                        "an explicit @tool directive is required by tool_choice",
                    ));
                }
                None => CompatOutput::Text(self.replay_eliza()?),
            },
            CompatTurn::Assistant(_) | CompatTurn::ToolCall(_) => unreachable!(),
        };

        let prompt = self.prompt_tokens();
        let completion = match &output {
            CompatOutput::Text(text) => approximate_tokens(std::iter::once(text.as_str())),
            CompatOutput::ToolCall(call) => {
                let arguments = call.arguments.serialized();
                approximate_tokens([call.name.as_str(), arguments.as_str()].into_iter())
            }
        };

        Ok(CompatTurnResponse {
            model: self.model,
            output,
            usage: TokenUsage {
                prompt,
                completion,
                total: prompt + completion,
            },
        })
    }
}

/// Approximate provider token counts.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, JsonSchema)]
pub(crate) struct TokenUsage {
    /// Approximate input token count.
    #[serde(rename = "prompt_tokens")]
    pub(crate) prompt: usize,
    /// Approximate output token count.
    #[serde(rename = "completion_tokens")]
    pub(crate) completion: usize,
    /// Prompt plus completion tokens.
    #[serde(rename = "total_tokens")]
    pub(crate) total: usize,
}

/// One completed provider-neutral turn.
#[derive(Debug)]
pub(crate) struct CompatTurnResponse {
    /// Model id echoed by the provider adapter.
    pub(crate) model: ModelId,
    /// Text or function call selected by the shared executor.
    pub(crate) output: CompatOutput,
    /// Approximate token usage.
    pub(crate) usage: TokenUsage,
}

/// Bounds transcript size and replay work.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RequestLimits {
    /// Maximum serialized input size across all request components.
    max_input_chars: NonZeroUsize,
    /// Maximum number of normalized history turns.
    max_history_messages: NonZeroUsize,
}

impl RequestLimits {
    /// Build limits from validated positive bounds.
    pub(crate) const fn new(
        max_input_chars: NonZeroUsize,
        max_history_messages: NonZeroUsize,
    ) -> Self {
        Self {
            max_input_chars,
            max_history_messages,
        }
    }
}

/// Approximate token usage by counting whitespace-delimited words.
fn approximate_tokens<'a>(texts: impl Iterator<Item = &'a str>) -> usize {
    texts.flat_map(str::split_whitespace).count()
}

#[cfg(test)]
#[expect(
    clippy::missing_panics_doc,
    reason = "test assertions are the intended panic contract"
)]
mod tests {
    use super::*;

    /// Return permissive bounds for unit-sized transcripts.
    fn limits() -> RequestLimits {
        RequestLimits::new(NonZeroUsize::MAX, NonZeroUsize::MAX)
    }

    /// Return the function definition offered by fixture tests.
    fn tool() -> FunctionTool {
        FunctionTool::new(
            "echo".to_owned(),
            serde_json::json!({"type":"function","name":"echo"})
                .to_string()
                .chars()
                .count(),
        )
    }

    #[test]
    fn explicit_directive_calls_an_offered_tool() {
        let response = CompatTurnRequest::new(
            ModelId::default(),
            Vec::new(),
            vec![CompatTurn::User(
                "@tool echo {\"value\":\"hello\"}".to_owned(),
            )],
            vec![tool()],
            ToolChoice::Auto,
        )
        .complete(limits())
        .unwrap();

        assert_eq!(
            response.output,
            CompatOutput::ToolCall(FunctionCall {
                name: "echo".to_owned(),
                arguments: serde_json::from_value(serde_json::json!({"value":"hello"})).unwrap(),
            })
        );
    }

    #[test]
    fn tools_do_not_change_an_ordinary_turn() {
        let response = CompatTurnRequest::new(
            ModelId::default(),
            Vec::new(),
            vec![CompatTurn::User("Hello".to_owned())],
            vec![tool()],
            ToolChoice::Auto,
        )
        .complete(limits())
        .unwrap();

        assert!(matches!(response.output, CompatOutput::Text(_)));
    }

    #[test]
    fn matching_tool_result_finishes_the_fixture() {
        let call = FunctionCall {
            name: "echo".to_owned(),
            arguments: serde_json::from_value(serde_json::json!({"value":"hello"})).unwrap(),
        };
        let response = CompatTurnRequest::new(
            ModelId::default(),
            Vec::new(),
            vec![
                CompatTurn::User("@tool echo {\"value\":\"hello\"}".to_owned()),
                CompatTurn::ToolCall(call),
                CompatTurn::ToolResult("hello".to_owned()),
            ],
            vec![tool()],
            ToolChoice::Auto,
        )
        .complete(limits())
        .unwrap();

        assert_eq!(
            response.output,
            CompatOutput::Text(TOOL_COMPLETE_TEXT.to_owned())
        );
    }

    #[test]
    fn malformed_directive_is_rejected() {
        let error = CompatTurnRequest::new(
            ModelId::default(),
            Vec::new(),
            vec![CompatTurn::User("@tool echo []".to_owned())],
            vec![tool()],
            ToolChoice::Auto,
        )
        .complete(limits())
        .unwrap_err();

        assert_eq!(error.param, Some("messages"));
    }

    #[test]
    fn required_choice_needs_an_explicit_directive() {
        let error = CompatTurnRequest::new(
            ModelId::default(),
            Vec::new(),
            vec![CompatTurn::User("Hello".to_owned())],
            vec![tool()],
            ToolChoice::Required,
        )
        .complete(limits())
        .unwrap_err();

        assert_eq!(error.param, Some("tool_choice"));
    }

    #[test]
    fn named_choice_must_match_the_directive() {
        let error = CompatTurnRequest::new(
            ModelId::default(),
            Vec::new(),
            vec![CompatTurn::User("@tool echo {}".to_owned())],
            vec![tool()],
            ToolChoice::Named("other".to_owned()),
        )
        .complete(limits())
        .unwrap_err();

        assert_eq!(error.param, Some("tool_choice"));
    }

    #[test]
    fn ordinary_turn_after_tool_history_resumes_eliza() {
        let call = FunctionCall {
            name: "echo".to_owned(),
            arguments: serde_json::from_value(serde_json::json!({"value":"hello"})).unwrap(),
        };
        let response = CompatTurnRequest::new(
            ModelId::default(),
            Vec::new(),
            vec![
                CompatTurn::User("@tool echo {\"value\":\"hello\"}".to_owned()),
                CompatTurn::ToolCall(call),
                CompatTurn::ToolResult("hello".to_owned()),
                CompatTurn::User("I am sad".to_owned()),
            ],
            vec![tool()],
            ToolChoice::Auto,
        )
        .complete(limits())
        .unwrap();

        assert_eq!(
            response.output,
            CompatOutput::Text("I AM SORRY TO HEAR YOU ARE SAD".to_owned())
        );
    }
}
