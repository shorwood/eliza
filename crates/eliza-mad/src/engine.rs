//! Public conversation API driven by the MAD machine.

use std::fmt;

use mad::machine::{Machine, RunState};
use mad::program::Program;
use mad::source::SourceModule;
use thiserror::Error as ThisError;

use crate::programs::MAD_1965;
use crate::runtime::{ElizaHost, NativeError, TraceSource as RuntimeTraceSource, TraceState};
use crate::script::Script;

// -----------------------------------------------------------------------------
// Doctor: Compiles the archival program inventory and 1966 conversation data.
// -----------------------------------------------------------------------------

/// Uppercase characters accepted by the historical terminal contract.
const BCD_REPERTOIRE: &str = "0123456789='+ABCDEFGHI.)-JKLMNOPQR$* /STUVWXYZ,(";

/// Corrected 1966 DOCTOR script.
const DOCTOR_SCRIPT: &str = include_str!("../programs/1966/doctor.script");

/// Reconstructed MAD driver for the 1966 algorithm.
const DRIVER_SOURCE: &str = include_str!("../programs/1966/eliza.mad");

/// Compiled 1966 DOCTOR program and script.
#[derive(Clone)]
pub struct Doctor {
    /// Linked reconstruction driver.
    program: Program,
    /// Parsed 1966 keyword and memory rules.
    script: Script,
}

impl Doctor {
    /// Parse the archival source inventory, compile the reconstruction driver,
    /// and validate the corrected 1966 script.
    ///
    /// # Errors
    ///
    /// Returns a positioned source, link, or script error.
    pub fn compile() -> Result<Self, Error> {
        for module in MAD_1965 {
            SourceModule::parse(module.name, module.text)?;
        }
        let driver = SourceModule::parse("eliza-1966.mad", DRIVER_SOURCE)?;
        let program = Program::link(&[driver])?;
        let script = DOCTOR_SCRIPT.parse::<Script>()?;
        Ok(Self { program, script })
    }

    /// Start an independent conversation with fresh response counters and
    /// memory.
    ///
    /// # Errors
    ///
    /// Returns an error if startup does not emit a greeting and request input.
    pub fn session(&self) -> Result<Session, Error> {
        self.try_into()
    }
}

/// Maximum instructions allowed while starting a session.
const STARTUP_LIMIT: usize = 10_000_000;

/// Maximum instructions allowed for one conversation turn.
const TURN_LIMIT: usize = 1_000_000;

/// Maximum characters in one historical input record.
const INPUT_RECORD_COLUMNS: usize = 72;

// -----------------------------------------------------------------------------
// Trace: Describes the mechanical rule-selection path for one response.
// -----------------------------------------------------------------------------

/// Final rule coordinates selected during one turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceRule {
    /// Canonical keyword whose rule produced the response.
    pub keyword: String,
    /// Zero-based decomposition position.
    pub decomposition: usize,
    /// Zero-based reassembly position.
    pub reassembly: usize,
}

/// Branch that produced one response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceSource {
    /// A ranked keyword rule produced the response.
    Keyword,
    /// A queued memory produced the response.
    Memory,
    /// The script's `NONE` rule or historical fallback produced the response.
    None,
    /// A provider-neutral fixture tool produced the response.
    Tool,
}

impl fmt::Display for TraceSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Keyword => "KEYWORD",
            Self::Memory => "MEMORY",
            Self::None => "NONE",
            Self::Tool => "TOOL",
        })
    }
}

/// Join one word list or render the trace's explicit empty marker.
fn words_or_dash(words: &[String]) -> String {
    if words.is_empty() {
        "-".to_owned()
    } else {
        words.join(" ")
    }
}

/// Honest mechanical facts recorded while ELIZA selected one response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Trace {
    /// Canonical input words after substitutions and delimiter handling.
    pub normalized_input: Vec<String>,
    /// Keywords in the order ELIZA inspected them.
    pub ranked_keywords: Vec<String>,
    /// Final rule that assembled response text, when one did.
    pub selected_rule: Option<TraceRule>,
    /// Branch that produced the response.
    pub source: TraceSource,
}

impl fmt::Display for Trace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let normalized = words_or_dash(&self.normalized_input);
        let keywords = words_or_dash(&self.ranked_keywords);
        let selected = self.selected_rule.as_ref().map_or_else(
            || "-".to_owned(),
            |rule| {
                format!(
                    "{}/{}/{}",
                    rule.keyword, rule.decomposition, rule.reassembly
                )
            },
        );
        write!(
            formatter,
            "NORMALIZED INPUT: {normalized}\nRANKED KEYWORDS: {keywords}\nSELECTED RULE: {selected}\nRESPONSE SOURCE: {}",
            self.source
        )
    }
}

impl From<TraceState> for Trace {
    fn from(trace: TraceState) -> Self {
        Self {
            normalized_input: trace.normalized_input,
            ranked_keywords: trace.ranked_keywords,
            selected_rule: trace.selected_rule.map(|rule| TraceRule {
                keyword: rule.keyword,
                decomposition: rule.decomposition,
                reassembly: rule.reassembly,
            }),
            source: match trace.source {
                RuntimeTraceSource::Keyword => TraceSource::Keyword,
                RuntimeTraceSource::Memory => TraceSource::Memory,
                RuntimeTraceSource::None => TraceSource::None,
            },
        }
    }
}

// -----------------------------------------------------------------------------
// Response: Carries historical output together with its trace.
// -----------------------------------------------------------------------------

/// One completed historical response and its selection trace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Response {
    /// Historical response text.
    pub text: String,
    /// Mechanical rule-selection facts.
    pub trace: Trace,
}

// -----------------------------------------------------------------------------
// Session: Owns the mutable state of one DOCTOR conversation.
// -----------------------------------------------------------------------------

/// One stateful DOCTOR conversation.
pub struct Session {
    /// Opening remark emitted during startup.
    greeting: String,
    /// MAD interpreter and native SLIP bridge.
    machine: Machine<ElizaHost>,
    /// Whether the machine has suspended at READ FORMAT.
    is_waiting_for_input: bool,
}

impl TryFrom<&Doctor> for Session {
    type Error = Error;

    fn try_from(doctor: &Doctor) -> Result<Self, Self::Error> {
        let host = ElizaHost::new(doctor.script.clone()).map_err(Error::from)?;
        let mut machine = Machine::new(doctor.program.clone(), host);
        let greeting_handle = match machine.run(STARTUP_LIMIT)? {
            RunState::OutputWord(handle) => Ok(handle),
            state => Err(Error::new(ErrorDetail::StartupGreeting { state })),
        }?;
        let greeting = machine
            .host()
            .output(greeting_handle)
            .map_err(Error::from)?;
        match machine.run(STARTUP_LIMIT)? {
            RunState::NeedsInput => Ok(Self {
                greeting,
                machine,
                is_waiting_for_input: true,
            }),
            state => Err(Error::new(ErrorDetail::StartupInput { state })),
        }
    }
}

impl Session {
    /// Opening remark loaded from the script.
    #[must_use]
    pub fn greeting(&self) -> &str {
        &self.greeting
    }

    /// Resume execution until the machine requests the next input record.
    ///
    /// # Errors
    ///
    /// Returns an error when execution emits another event or fails.
    fn prepare_input(&mut self) -> Result<(), Error> {
        // A machine already suspended at READ FORMAT needs no preparation.
        if self.is_waiting_for_input {
            return Ok(());
        }
        match self.machine.run(TURN_LIMIT)? {
            RunState::NeedsInput => {
                self.is_waiting_for_input = true;
                Ok(())
            }
            state => Err(Error::new(ErrorDetail::PrepareInput { state })),
        }
    }

    /// Convert the next cooperative output event into response text.
    ///
    /// # Errors
    ///
    /// Returns an error when execution does not emit response output.
    fn read_response(&mut self) -> Result<String, Error> {
        match self.machine.run(TURN_LIMIT)? {
            RunState::OutputComment(text) => Ok(text),
            RunState::OutputWord(handle) => self.machine.host().output(handle).map_err(Error::from),
            state => Err(Error::new(ErrorDetail::Response { state })),
        }
    }

    /// Execute one historical 72-column input record.
    ///
    /// # Errors
    ///
    /// Returns an input or bounded-machine error.
    pub fn respond(&mut self, input: &str) -> Result<Response, Error> {
        validate_input(input)?;
        self.prepare_input()?;

        // Pass the record through the native bridge into READ FORMAT.
        let input_handle = self
            .machine
            .host_mut()
            .store_input(input)
            .map_err(Error::from)?;

        // Resume READ FORMAT with the stored resource handle.
        self.machine.provide_input(input_handle)?;
        self.is_waiting_for_input = false;
        let text = self.read_response()?;
        let trace_state = self.machine.host().trace_state().clone();
        let trace = trace_state.into();
        Ok(Response { text, trace })
    }
}

// -----------------------------------------------------------------------------
// Error: Categorizes compilation, input, script, and runtime failures.
// -----------------------------------------------------------------------------

/// Broad category for a standalone engine failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    /// Input is outside the historical record contract.
    Input,
    /// Executable MAD could not be linked.
    Link,
    /// Runtime execution or native SLIP bridge failed.
    Runtime,
    /// DOCTOR script could not be parsed or validated.
    Script,
    /// Historical or reconstructed MAD source could not be parsed.
    Source,
}

/// Complete standalone engine failures.
#[derive(Debug, ThisError)]
enum ErrorDetail {
    /// The input record contains no non-whitespace characters.
    #[error("input record is blank")]
    BlankInput,
    /// Standalone mode received an interactive editing command.
    #[error("interactive script editing and dumping are not available")]
    InputCommand,
    /// The input record exceeds the historical terminal width.
    #[error("input exceeds the historical 72-column record")]
    InputTooLong,
    /// The input record contains a character outside IBM 7094 BCD.
    #[error("{character:?} is not uppercase IBM 7094 BCD")]
    InvalidInputCharacter {
        /// Unsupported input character.
        character: char,
    },
    /// Executable MAD could not be linked.
    #[error(transparent)]
    Link {
        /// Linker failure.
        source: mad::program::LinkError,
    },
    /// The native SLIP bridge failed.
    #[error(transparent)]
    Native {
        /// Native bridge failure.
        source: NativeError,
    },
    /// The machine emitted the wrong event while preparing a turn.
    #[error("MAD emitted {state:?} while preparing the next turn")]
    PrepareInput {
        /// Unexpected cooperative state.
        state: RunState,
    },
    /// The machine emitted the wrong event for a response.
    #[error("MAD emitted {state:?} instead of a response")]
    Response {
        /// Unexpected cooperative state.
        state: RunState,
    },
    /// MAD execution failed.
    #[error(transparent)]
    Runtime {
        /// Interpreter failure.
        source: mad::machine::RuntimeError,
    },
    /// The DOCTOR script could not be parsed or validated.
    #[error(transparent)]
    Script {
        /// Script failure.
        source: crate::script::ScriptError,
    },
    /// Historical startup did not emit a greeting.
    #[error("MAD startup emitted {state:?} instead of the greeting")]
    StartupGreeting {
        /// Unexpected cooperative state.
        state: RunState,
    },
    /// Historical startup did not request the first input.
    #[error("MAD startup emitted {state:?} instead of requesting input")]
    StartupInput {
        /// Unexpected cooperative state.
        state: RunState,
    },
    /// MAD source cards could not be parsed.
    #[error(transparent)]
    Source {
        /// Fixed-form source failure.
        source: mad::source::ParseError,
    },
}

/// Standalone compilation, input, or execution failure.
#[derive(Debug, ThisError)]
#[error("{detail}")]
pub struct Error {
    /// Fully declared failure detail.
    #[source]
    detail: ErrorDetail,
}

impl Error {
    /// Wrap one declared failure.
    const fn new(detail: ErrorDetail) -> Self {
        Self { detail }
    }

    /// Stable error category.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self.detail {
            ErrorDetail::BlankInput
            | ErrorDetail::InputCommand
            | ErrorDetail::InputTooLong
            | ErrorDetail::InvalidInputCharacter { .. } => ErrorKind::Input,
            ErrorDetail::Link { .. } => ErrorKind::Link,
            ErrorDetail::Native { .. }
            | ErrorDetail::PrepareInput { .. }
            | ErrorDetail::Response { .. }
            | ErrorDetail::Runtime { .. }
            | ErrorDetail::StartupGreeting { .. }
            | ErrorDetail::StartupInput { .. } => ErrorKind::Runtime,
            ErrorDetail::Script { .. } => ErrorKind::Script,
            ErrorDetail::Source { .. } => ErrorKind::Source,
        }
    }
}

impl From<NativeError> for Error {
    fn from(error: NativeError) -> Self {
        Self::new(ErrorDetail::Native { source: error })
    }
}

impl From<crate::script::ScriptError> for Error {
    fn from(error: crate::script::ScriptError) -> Self {
        Self::new(ErrorDetail::Script { source: error })
    }
}

impl From<mad::program::LinkError> for Error {
    fn from(error: mad::program::LinkError) -> Self {
        Self::new(ErrorDetail::Link { source: error })
    }
}

impl From<mad::source::ParseError> for Error {
    fn from(error: mad::source::ParseError) -> Self {
        Self::new(ErrorDetail::Source { source: error })
    }
}

impl From<mad::machine::RuntimeError> for Error {
    fn from(error: mad::machine::RuntimeError) -> Self {
        Self::new(ErrorDetail::Runtime { source: error })
    }
}

// -----------------------------------------------------------------------------
// ValidateInput: Enforces the uppercase 72-column terminal contract.
// -----------------------------------------------------------------------------

/// Validate one historical input record.
///
/// # Errors
///
/// Returns an input error for blank, long, non-BCD, or command records.
fn validate_input(input: &str) -> Result<(), Error> {
    // Historical terminal records occupy at most 72 columns.
    if input.chars().count() > INPUT_RECORD_COLUMNS {
        return Err(Error::new(ErrorDetail::InputTooLong));
    }

    // Blank records do not enter the original input loop.
    if input.trim().is_empty() {
        return Err(Error::new(ErrorDetail::BlankInput));
    }

    // The IBM 7094 script accepts only its uppercase BCD repertoire.
    if let Some(character) = input
        .chars()
        .find(|character| !BCD_REPERTOIRE.contains(*character))
    {
        return Err(Error::new(ErrorDetail::InvalidInputCharacter { character }));
    }

    // Standalone mode intentionally excludes interactive script editing.
    if matches!(input.split_whitespace().next(), Some("+" | "*")) {
        return Err(Error::new(ErrorDetail::InputCommand));
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// Tests: Compile the historical assets and verify the opening remark.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::Doctor;

    /// # Panics
    ///
    /// Panics when compilation, startup, or the assertion fails.
    #[test]
    fn compiles_and_starts_doctor() {
        let doctor = Doctor::compile().unwrap();
        let session = doctor.session().unwrap();
        assert_eq!(
            session.greeting(),
            "HOW DO YOU DO. PLEASE TELL ME YOUR PROBLEM"
        );
    }
}
