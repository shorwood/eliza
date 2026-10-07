//! Bounded execution for linked MAD programs.

use std::borrow::Cow;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

use thiserror::Error;

use crate::program::{BinaryOperator, Expr, Instruction, JumpTarget, Place, Program};
use crate::source::SourceSpan;
use crate::word::Word;

// -----------------------------------------------------------------------------
// Host: Defines native functions available to interpreted MAD code.
// -----------------------------------------------------------------------------

/// Native functions supplied to a MAD machine.
pub trait Host {
    /// Declared native-call failure.
    type Error: StdError + Send + Sync + 'static;

    /// Invoke one external function. MAD arguments are passed by reference:
    /// changes made to `arguments` are written back when an argument is a
    /// variable or array element.
    ///
    /// # Errors
    ///
    /// Return a typed error when the function or its arguments are invalid.
    fn call(&mut self, name: &str, arguments: &mut [Word]) -> Result<Word, Self::Error>;
}

// -----------------------------------------------------------------------------
// RunState: Describes each externally observable execution boundary.
// -----------------------------------------------------------------------------

/// Observable cooperative-machine state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunState {
    /// Program terminated.
    Halted,
    /// `READ FORMAT` is waiting for one word.
    NeedsInput,
    /// `PRINT COMMENT` emitted source text.
    OutputComment(
        /// Emitted source text.
        String,
    ),
    /// `PRINT FORMAT` emitted a machine word.
    OutputWord(
        /// Emitted machine word.
        Word,
    ),
}

// -----------------------------------------------------------------------------
// LoopFrame: Retains one active THROUGH loop.
// -----------------------------------------------------------------------------

/// Execution state for an active THROUGH loop.
#[derive(Clone)]
struct LoopFrame {
    /// Program counter of the last loop-body instruction.
    end_pc: usize,
    /// Program counter of the first loop-body instruction.
    start_pc: usize,
    /// Increment evaluated after each iteration.
    step: Expr,
    /// Condition evaluated after incrementing the induction variable.
    until: Expr,
    /// Canonical induction-variable name.
    variable: String,
}

// -----------------------------------------------------------------------------
// Machine: Executes one linked program cooperatively within explicit bounds.
// -----------------------------------------------------------------------------

/// A linked MAD program and its mutable execution state.
pub struct Machine<H> {
    /// One-based storage for each declared array.
    arrays: HashMap<String, Vec<Word>>,
    /// Whether execution has terminated.
    is_halted: bool,
    /// Native function implementation.
    host: H,
    /// Active THROUGH loops from outermost to innermost.
    loops: Vec<LoopFrame>,
    /// Destination awaiting a supplied input word.
    pending_input: Option<Place>,
    /// Immutable linked instructions and labels.
    program: Arc<Program>,
    /// Index of the next instruction.
    program_counter: usize,
    /// Scalar storage keyed by canonical name.
    scalars: HashMap<String, Word>,
}

impl<H: Host> Machine<H> {
    /// Create a machine at the first linked statement from an owned or shared
    /// program. Each machine initializes independent variable and array storage.
    #[must_use]
    pub fn new(program: impl Into<Arc<Program>>, host: H) -> Self {
        let program = program.into();
        let arrays = program
            .arrays
            .iter()
            .map(|(name, size)| (name.clone(), vec![Word::ZERO; size + 1]))
            .collect();
        Self {
            arrays,
            is_halted: false,
            host,
            loops: Vec::new(),
            pending_input: None,
            program,
            program_counter: 0,
            scalars: HashMap::new(),
        }
    }

    /// Apply an eager arithmetic or comparison operator.
    ///
    /// # Errors
    ///
    /// Returns an error for division by zero or a misplaced logical operator.
    fn apply_binary(
        operator: BinaryOperator,
        left: Word,
        right: Word,
        span: Option<&SourceSpan>,
    ) -> Result<Word, RuntimeError> {
        let left_integer = left.to_i64();
        let right_integer = right.to_i64();
        let result = match operator {
            BinaryOperator::Add => left_integer.wrapping_add(right_integer),
            // Logical operators must take the short-circuit evaluation path.
            BinaryOperator::And | BinaryOperator::Or => {
                return Err(RuntimeError::at_optional(
                    span,
                    RuntimeErrorKind::EagerLogicalOperator,
                ));
            }
            BinaryOperator::Divide => {
                // Integer division is undefined when the divisor is zero.
                if right_integer == 0 {
                    return Err(RuntimeError::at_optional(
                        span,
                        RuntimeErrorKind::DivisionByZero,
                    ));
                }
                left_integer.wrapping_div(right_integer)
            }
            BinaryOperator::Equal => i64::from(left.raw() == right.raw()),
            BinaryOperator::Greater => i64::from(left_integer > right_integer),
            BinaryOperator::GreaterEqual => i64::from(left_integer >= right_integer),
            BinaryOperator::Less => i64::from(left_integer < right_integer),
            BinaryOperator::LessEqual => i64::from(left_integer <= right_integer),
            BinaryOperator::Multiply => left_integer.wrapping_mul(right_integer),
            BinaryOperator::NotEqual => i64::from(left.raw() != right.raw()),
            BinaryOperator::Subtract => left_integer.wrapping_sub(right_integer),
        };
        Ok(Word::from_i64(result))
    }

    /// Convert a machine word into a non-negative host array index.
    ///
    /// # Errors
    ///
    /// Returns an error when the signed word is negative or too large.
    fn index_from_word(value: Word, span: Option<&SourceSpan>) -> Result<usize, RuntimeError> {
        usize::try_from(value.to_i64())
            .map_err(|_| RuntimeError::at_optional(span, RuntimeErrorKind::NegativeArrayIndex))
    }

    /// Borrow the native host.
    #[must_use]
    pub const fn host(&self) -> &H {
        &self.host
    }

    /// Mutably borrow the native host.
    #[must_use]
    pub const fn host_mut(&mut self) -> &mut H {
        &mut self.host
    }

    /// Invoke the native host and attach the function name to failures.
    ///
    /// # Errors
    ///
    /// Returns the diagnostic reported by the host.
    fn call_host(&mut self, name: &str, values: &mut [Word]) -> Result<Word, RuntimeError> {
        self.host
            .call(name, values)
            .map_err(|source| RuntimeError::native(name, source))
    }

    /// Evaluate one expression against current storage and the native host.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid storage, arithmetic, or native calls.
    fn evaluate(&mut self, expression: &Expr) -> Result<Word, RuntimeError> {
        match expression {
            Expr::Binary {
                left,
                operator,
                right,
            } => self.evaluate_binary_expression(*operator, left, right),
            Expr::Call { arguments, name } => self.evaluate_call(name, arguments),
            Expr::Literal(value) => Ok(*value),
            Expr::Place(place) => self.read(place, None),
            Expr::UnaryMinus(value) => Ok(Word::from_i64(-self.evaluate(value)?.to_i64())),
        }
    }

    /// Evaluate a binary expression, including Boolean short-circuiting.
    ///
    /// # Errors
    ///
    /// Returns an error when either operand or the selected operation fails.
    fn evaluate_binary_expression(
        &mut self,
        operator: BinaryOperator,
        left: &Expr,
        right: &Expr,
    ) -> Result<Word, RuntimeError> {
        match operator {
            BinaryOperator::And => self.evaluate_conjunction(left, right),
            BinaryOperator::Or => self.evaluate_disjunction(left, right),
            _ => {
                let left = self.evaluate(left)?;
                let right = self.evaluate(right)?;
                Self::apply_binary(operator, left, right, None)
            }
        }
    }

    /// Evaluate a short-circuit conjunction.
    ///
    /// # Errors
    ///
    /// Returns an error when evaluating either required operand fails.
    fn evaluate_conjunction(&mut self, left: &Expr, right: &Expr) -> Result<Word, RuntimeError> {
        let left = self.evaluate(left)?.is_true();

        // A false left operand determines the conjunction.
        if !left {
            return Ok(Word::ZERO);
        }
        let right = self.evaluate(right)?.is_true();
        Ok(Word::from_i64(i64::from(right)))
    }

    /// Evaluate a short-circuit disjunction.
    ///
    /// # Errors
    ///
    /// Returns an error when evaluating either required operand fails.
    fn evaluate_disjunction(&mut self, left: &Expr, right: &Expr) -> Result<Word, RuntimeError> {
        let left = self.evaluate(left)?.is_true();

        // A true left operand determines the disjunction.
        if left {
            return Ok(Word::from_i64(1));
        }
        let right = self.evaluate(right)?.is_true();
        Ok(Word::from_i64(i64::from(right)))
    }

    /// Evaluate a native call and write back arguments passed by reference.
    ///
    /// # Errors
    ///
    /// Returns an error when arguments, the native call, or writeback fails.
    fn evaluate_call(&mut self, name: &str, arguments: &[Expr]) -> Result<Word, RuntimeError> {
        // Evaluate every input before crossing the native boundary.
        let mut values = arguments
            .iter()
            .map(|argument| self.evaluate(argument))
            .collect::<Result<Vec<_>, _>>()?;
        let result = self.call_host(name, &mut values)?;

        // Native mutations flow back only through assignable arguments.
        for (argument, value) in arguments.iter().zip(values) {
            let Some(place) = argument.as_place() else {
                continue;
            };
            self.write(place, value, None)?;
        }
        Ok(result)
    }

    /// Read a scalar or array element.
    ///
    /// # Errors
    ///
    /// Returns an error for a negative, undeclared, or out-of-bounds array place.
    fn read(&mut self, place: &Place, span: Option<&SourceSpan>) -> Result<Word, RuntimeError> {
        match place {
            Place::Array { index, name } => self.read_array(index, name, span),
            Place::Scalar(name) => Ok(self.scalars.get(name).copied().unwrap_or(Word::ZERO)),
        }
    }

    /// Read one indexed array element.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid index or array name.
    fn read_array(
        &mut self,
        expression: &Expr,
        name: &str,
        span: Option<&SourceSpan>,
    ) -> Result<Word, RuntimeError> {
        let index = Self::index_from_word(self.evaluate(expression)?, span)?;
        let array = self.arrays.get(name).ok_or_else(|| {
            RuntimeError::at_optional(
                span,
                RuntimeErrorKind::UnknownArray {
                    name: name.to_owned(),
                },
            )
        })?;
        array.get(index).copied().ok_or_else(|| {
            RuntimeError::at_optional(
                span,
                RuntimeErrorKind::InvalidArrayIndex {
                    index,
                    name: name.to_owned(),
                },
            )
        })
    }

    /// Write a scalar or array element.
    ///
    /// # Errors
    ///
    /// Returns an error for a negative, undeclared, or out-of-bounds array place.
    fn write(
        &mut self,
        place: &Place,
        value: Word,
        span: Option<&SourceSpan>,
    ) -> Result<(), RuntimeError> {
        match place {
            Place::Array { index, name } => self.write_array(index, name, value, span)?,
            Place::Scalar(name) => {
                if let Some(slot) = self.scalars.get_mut(name.as_str()) {
                    *slot = value;
                } else {
                    self.scalars.insert(name.clone(), value);
                }
            }
        }
        Ok(())
    }

    /// Write one indexed array element.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid index or array name.
    fn write_array(
        &mut self,
        expression: &Expr,
        name: &str,
        value: Word,
        span: Option<&SourceSpan>,
    ) -> Result<(), RuntimeError> {
        let index = Self::index_from_word(self.evaluate(expression)?, span)?;
        let array = self.arrays.get_mut(name).ok_or_else(|| {
            RuntimeError::at_optional(
                span,
                RuntimeErrorKind::UnknownArray {
                    name: name.to_owned(),
                },
            )
        })?;
        let slot = array.get_mut(index).ok_or_else(|| {
            RuntimeError::at_optional(
                span,
                RuntimeErrorKind::InvalidArrayIndex {
                    index,
                    name: name.to_owned(),
                },
            )
        })?;
        *slot = value;
        Ok(())
    }

    /// Supply the word requested by [`RunState::NeedsInput`].
    ///
    /// # Errors
    ///
    /// Returns an error when no read is pending or its destination is invalid.
    pub fn provide_input(&mut self, value: Word) -> Result<(), RuntimeError> {
        let place = self
            .pending_input
            .take()
            .ok_or_else(|| RuntimeError::at_optional(None, RuntimeErrorKind::NotWaitingForInput))?;
        self.write(&place, value, None)
    }

    /// Advance an active THROUGH loop after its final body instruction.
    ///
    /// # Errors
    ///
    /// Returns an error when evaluating the step or termination expression fails.
    fn advance_loops(&mut self, executed_pc: usize) -> Result<(), RuntimeError> {
        // Programs without an active loop need no bookkeeping.
        let Some(frame) = self.loops.last().cloned() else {
            return Ok(());
        };

        // Transfers out of the loop body suppress its normal back edge.
        if frame.end_pc != executed_pc || self.program_counter != executed_pc + 1 {
            return Ok(());
        }

        // Increment the induction variable before testing the condition.
        let current = self
            .scalars
            .get(&frame.variable)
            .copied()
            .unwrap_or(Word::ZERO);
        let current = current.to_i64();
        let step = self.evaluate(&frame.step)?.to_i64();

        // Store the value consumed by the termination expression.
        let next = Word::from_i64(current.wrapping_add(step));
        self.scalars.insert(frame.variable, next);

        // A true condition exits; otherwise execution returns to the body.
        let is_complete = self.evaluate(&frame.until)?.is_true();
        if is_complete {
            self.loops.pop();
        } else {
            self.program_counter = frame.start_pc;
        }
        Ok(())
    }

    /// Resolve a direct or computed transfer target.
    ///
    /// # Errors
    ///
    /// Returns an error when the computed expression or resulting label is invalid.
    fn resolve_target(
        &mut self,
        target: &JumpTarget,
        span: Option<&SourceSpan>,
    ) -> Result<usize, RuntimeError> {
        let label = match target {
            JumpTarget::Computed { base, index } => {
                Cow::Owned(format!("{base}({})", self.evaluate(index)?.to_i64()))
            }
            JumpTarget::Label(label) => Cow::Borrowed(label.as_str()),
        };
        self.program
            .labels
            .get(label.as_ref())
            .copied()
            .ok_or_else(|| {
                RuntimeError::at_optional(
                    span,
                    RuntimeErrorKind::UnknownLabel {
                        label: label.into_owned(),
                    },
                )
            })
    }

    /// Retain a THROUGH loop for its eventual back edge.
    fn push_loop(&mut self, end_pc: usize, step: &Expr, until: &Expr, variable: &str) {
        self.loops.push(LoopFrame {
            end_pc,
            start_pc: self.program_counter,
            step: step.clone(),
            until: until.clone(),
            variable: variable.to_owned(),
        });
    }

    /// Initialize a THROUGH loop or skip a body whose condition is already true.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid expressions, labels, or loop direction.
    fn begin_loop(
        &mut self,
        start: &Expr,
        step: &Expr,
        target: &JumpTarget,
        until: &Expr,
        variable: &str,
        span: &SourceSpan,
    ) -> Result<Option<RunState>, RuntimeError> {
        let start = self.evaluate(start)?;
        self.scalars.insert(variable.to_owned(), start);
        let end_pc = self.resolve_target(target, Some(span))?;

        // A loop body cannot precede its THROUGH statement.
        if end_pc < self.program_counter {
            return Err(RuntimeError::at(
                span,
                RuntimeErrorKind::BackwardThroughTarget,
            ));
        }

        // A true initial condition skips the complete loop body.
        if self.evaluate(until)?.is_true() {
            self.program_counter = end_pc + 1;
            return Ok(None);
        }

        self.push_loop(end_pc, step, until, variable);
        Ok(None)
    }

    /// Execute one linked instruction.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid storage, control flow, or expressions.
    fn execute(&mut self, instruction: &Instruction) -> Result<Option<RunState>, RuntimeError> {
        let span = Some(instruction.span());
        match instruction {
            Instruction::Assign { place, value, .. } => {
                let value = self.evaluate(value)?;
                self.write(place, value, span)?;
                Ok(None)
            }
            Instruction::Comment { text, .. } => Ok(Some(RunState::OutputComment(text.clone()))),
            Instruction::Dimension { .. } | Instruction::Nop { .. } => Ok(None),
            Instruction::Evaluate { expression, .. } => {
                self.evaluate(expression)?;
                Ok(None)
            }
            Instruction::Exit { .. } => {
                self.is_halted = true;
                Ok(Some(RunState::Halted))
            }
            Instruction::Print { value, .. } => {
                let value = self.evaluate(value)?;
                Ok(Some(RunState::OutputWord(value)))
            }
            Instruction::Read { place, .. } => {
                self.pending_input = Some(place.clone());
                Ok(Some(RunState::NeedsInput))
            }
            Instruction::Return { value, .. } => {
                let value = self.evaluate(value)?;
                self.scalars.insert("RETURN".to_owned(), value);
                self.is_halted = true;
                Ok(Some(RunState::Halted))
            }
            Instruction::Through {
                start,
                step,
                target,
                until,
                variable,
                ..
            } => self.begin_loop(start, step, target, until, variable, instruction.span()),
            Instruction::Transfer { target, .. } => {
                self.program_counter = self.resolve_target(target, span)?;
                Ok(None)
            }
            Instruction::Whenever {
                action, condition, ..
            } => {
                let should_execute = self.evaluate(condition)?.is_true();
                if should_execute {
                    self.execute(action)
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// Execute until input, output, termination, or the instruction bound.
    ///
    /// # Errors
    ///
    /// Returns a positioned runtime error for an invalid operation, native
    /// call failure, bad transfer, or exhausted instruction budget.
    pub fn run(&mut self, instruction_limit: usize) -> Result<RunState, RuntimeError> {
        // A terminated machine remains observably halted.
        if self.is_halted {
            return Ok(RunState::Halted);
        }

        // An unsatisfied read must be completed before execution resumes.
        if self.pending_input.is_some() {
            return Ok(RunState::NeedsInput);
        }

        // Keep instructions alive independently of the mutable machine state.
        let program = Arc::clone(&self.program);
        for _ in 0..instruction_limit {
            // Falling off the linked program terminates it.
            let Some(instruction) = program.instructions.get(self.program_counter) else {
                self.is_halted = true;
                return Ok(RunState::Halted);
            };
            let executed_pc = self.program_counter;
            self.program_counter += 1;

            let output = self.execute(instruction)?;
            self.advance_loops(executed_pc)?;

            // Cooperative events return control to the caller immediately.
            if let Some(state) = output {
                return Ok(state);
            }
        }

        // Attribute budget exhaustion to the next instruction when possible.
        let span = program
            .instructions
            .get(self.program_counter)
            .map(Instruction::span);
        Err(RuntimeError::instruction_limit(span, instruction_limit))
    }
}

// -----------------------------------------------------------------------------
// Runtime: Declares positioned execution failures.
// -----------------------------------------------------------------------------

/// Optional source prefix rendered before a runtime failure.
#[derive(Debug)]
struct RuntimeLocation(
    /// Failing instruction location, when execution has one.
    Option<SourceSpan>,
);

impl fmt::Display for RuntimeLocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Unpositioned host and protocol failures need no prefix.
        let Some(span) = &self.0 else {
            return Ok(());
        };
        write!(
            formatter,
            "{}:{}:{}: ",
            span.module(),
            span.line(),
            span.column()
        )
    }
}

/// Failures recognized while executing the linked MAD subset.
#[derive(Debug, Error)]
enum RuntimeErrorKind {
    /// A `THROUGH` loop points behind its declaration.
    #[error("THROUGH target must follow the loop statement")]
    BackwardThroughTarget,
    /// Integer division received a zero divisor.
    #[error("division by zero")]
    DivisionByZero,
    /// A logical operator reached eager evaluation.
    #[error("logical operator was evaluated without short-circuiting")]
    EagerLogicalOperator,
    /// Cooperative execution consumed its instruction budget.
    #[error("instruction limit of {limit} exhausted")]
    InstructionLimit {
        /// Maximum instructions allowed for the run.
        limit: usize,
    },
    /// An array index falls outside allocated storage.
    #[error("invalid {name}({index})")]
    InvalidArrayIndex {
        /// Invalid zero-based machine index.
        index: usize,
        /// Canonical array name.
        name: String,
    },
    /// The caller supplied input without a pending read.
    #[error("the machine is not waiting for input")]
    NotWaitingForInput,
    /// An array index is negative.
    #[error("array index must be non-negative")]
    NegativeArrayIndex,
    /// A native function rejected its call.
    #[error("native function {function} failed: {source}")]
    NativeCall {
        /// Canonical native function name.
        function: String,
        /// Typed host failure.
        #[source]
        source: Box<dyn StdError + Send + Sync>,
    },
    /// An array place references no declaration.
    #[error("unknown array {name}")]
    UnknownArray {
        /// Canonical array name.
        name: String,
    },
    /// A transfer references no linked label.
    #[error("unknown label {label}")]
    UnknownLabel {
        /// Canonical target label.
        label: String,
    },
}

/// MAD execution failure.
#[derive(Debug, Error)]
#[error("{location}{kind}")]
pub struct RuntimeError {
    /// Declared execution failure.
    #[source]
    kind: RuntimeErrorKind,
    /// Optional failing-instruction prefix.
    location: RuntimeLocation,
}

impl RuntimeError {
    /// Build an error with an optional source position.
    fn at_optional(span: Option<&SourceSpan>, kind: RuntimeErrorKind) -> Self {
        Self {
            kind,
            location: RuntimeLocation(span.cloned()),
        }
    }

    /// Build an error at one exact source position.
    fn at(span: &SourceSpan, kind: RuntimeErrorKind) -> Self {
        Self::at_optional(Some(span), kind)
    }

    /// Build an exhausted instruction-budget error.
    fn instruction_limit(span: Option<&SourceSpan>, limit: usize) -> Self {
        Self::at_optional(span, RuntimeErrorKind::InstructionLimit { limit })
    }

    /// Wrap one typed failure from a native function.
    fn native<E>(function: &str, source: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self::at_optional(
            None,
            RuntimeErrorKind::NativeCall {
                function: function.to_owned(),
                source: Box::new(source),
            },
        )
    }

    /// Source position of the failing instruction, when available.
    #[must_use]
    pub const fn span(&self) -> Option<&SourceSpan> {
        self.location.0.as_ref()
    }
}

// -----------------------------------------------------------------------------
// Tests: Exercise cooperative I/O, transfers, native calls, and THROUGH loops.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use thiserror::Error;

    use super::{Host, Machine, RunState, RuntimeErrorKind, Word};
    use crate::program::{JumpTarget, Program};
    use crate::source::SourceModule;

    /// Native failure exposed by [`TestHost`].
    #[derive(Debug, Error)]
    enum TestHostError {
        /// The test program called an unsupported function.
        #[error("unknown native function {name}")]
        UnknownFunction {
            /// Unsupported canonical function name.
            name: String,
        },
    }

    /// Deterministic native host used by machine tests.
    struct TestHost;

    impl Host for TestHost {
        type Error = TestHostError;

        fn call(&mut self, name: &str, arguments: &mut [Word]) -> Result<Word, Self::Error> {
            match name {
                "DOUBLE" => Ok(Word::from_i64(arguments[0].to_i64() * 2)),
                "INCREMENT" => {
                    arguments[0] = Word::from_i64(arguments[0].to_i64() + 1);
                    Ok(arguments[0])
                }
                _ => Err(TestHostError::UnknownFunction {
                    name: name.to_owned(),
                }),
            }
        }
    }

    /// # Panics
    ///
    /// Panics when parsing, linking, execution, or an assertion fails.
    #[test]
    fn executes_control_flow_and_cooperative_io() {
        let source = concat!(
            "            DIMENSION A(2)\n",
            "START       READ FORMAT TEXT,A(1)\n",
            "            A(2)=DOUBLE.(A(1))\n",
            "            W'R A(2) .L. 10, T'O START\n",
            "            PRINT FORMAT TEXT,A(2)\n",
            "            EXIT.\n",
        );
        let module = SourceModule::parse("machine", source).unwrap();
        let program = Program::link(&[module]).unwrap();
        let mut machine = Machine::new(program, TestHost);

        assert_eq!(machine.run(100).unwrap(), RunState::NeedsInput);
        machine.provide_input(Word::from_i64(6)).unwrap();
        assert_eq!(
            machine.run(100).unwrap(),
            RunState::OutputWord(Word::from_i64(12))
        );
        assert_eq!(machine.run(100).unwrap(), RunState::Halted);
    }

    /// # Panics
    ///
    /// Panics when parsing, linking, execution, or the assertion fails.
    #[test]
    fn executes_through_loops() {
        let source = concat!(
            "            SUM=0\n",
            "            T'H ADD, FOR I=1,1, I .G. 4\n",
            "ADD         SUM=SUM+I\n",
            "            PRINT FORMAT NUMBER,SUM\n",
        );
        let module = SourceModule::parse("loop", source).unwrap();
        let program = Program::link(&[module]).unwrap();
        let mut machine = Machine::new(program, TestHost);
        assert_eq!(
            machine.run(100).unwrap(),
            RunState::OutputWord(Word::from_i64(10))
        );
    }

    /// Sharing a program leaves machine storage and suspension independent.
    ///
    /// # Panics
    /// Panics if linking, execution, or machine isolation changes.
    #[test]
    fn shared_program_keeps_machine_state_independent() {
        let source = concat!(
            "            DIMENSION A(1)\n",
            "            READ FORMAT TEXT,A(1)\n",
            "            RESULT=DOUBLE.(A(1))\n",
            "            PRINT FORMAT TEXT,RESULT\n",
            "            EXIT.\n",
        );
        let module = SourceModule::parse("shared", source).unwrap();
        let program = Arc::new(Program::link(&[module]).unwrap());
        let mut first = Machine::new(Arc::clone(&program), TestHost);
        let mut second = Machine::new(program, TestHost);
        assert!(Arc::ptr_eq(&first.program, &second.program));
        assert_eq!(first.run(100).unwrap(), RunState::NeedsInput);
        assert_eq!(second.run(100).unwrap(), RunState::NeedsInput);
        first.provide_input(Word::from_i64(6)).unwrap();
        second.provide_input(Word::from_i64(9)).unwrap();
        assert_eq!(
            first.run(100).unwrap(),
            RunState::OutputWord(Word::from_i64(12))
        );
        assert_eq!(
            second.run(100).unwrap(),
            RunState::OutputWord(Word::from_i64(18))
        );
        assert_eq!(first.run(100).unwrap(), RunState::Halted);
        assert_eq!(second.run(100).unwrap(), RunState::Halted);
    }

    /// Emitted comments own their text beyond the program's lifetime.
    ///
    /// # Panics
    /// Panics if linking, execution, or output ownership changes.
    #[test]
    fn emitted_comment_outlives_machine_and_program() {
        let module = SourceModule::parse("comment", "            PRINT COMMENT $HELLO$\n").unwrap();
        let program = Program::link(&[module]).unwrap();
        let mut machine = Machine::new(program, TestHost);
        let output = machine.run(1).unwrap();
        drop(machine);
        assert_eq!(output, RunState::OutputComment("HELLO".to_owned()));
    }

    /// Exhaustion identifies the next statement and leaves execution resumable.
    ///
    /// # Panics
    /// Panics if budget handling, source ownership, or resumption changes.
    #[test]
    fn instruction_budget_preserves_position_and_resumption() {
        let source = concat!(
            "            VALUE=7\n",
            "            PRINT FORMAT NUMBER,VALUE\n",
            "            EXIT.\n",
        );
        let module = SourceModule::parse("budget", source).unwrap();
        let program = Program::link(&[module]).unwrap();
        let mut machine = Machine::new(program, TestHost);
        let error = machine.run(1).unwrap_err();
        assert_eq!(error.span().unwrap().line(), 2);
        assert!(matches!(
            &error.kind,
            RuntimeErrorKind::InstructionLimit { limit: 1 }
        ));
        assert_eq!(
            machine.run(1).unwrap(),
            RunState::OutputWord(Word::from_i64(7))
        );
        assert_eq!(machine.run(1).unwrap(), RunState::Halted);
        assert_eq!(machine.run(0).unwrap(), RunState::Halted);
        drop(machine);
        assert_eq!(error.span().unwrap().module(), "budget");
        assert_eq!(error.span().unwrap().line(), 2);
    }

    /// Native argument mutations reach both scalar and array storage.
    ///
    /// # Panics
    /// Panics if linking, execution, or by-reference writeback changes.
    #[test]
    fn native_arguments_write_back_to_scalar_and_array() {
        let source = concat!(
            "            DIMENSION A(1)\n",
            "            VALUE=1\n",
            "            A(1)=2\n",
            "            RESULT=INCREMENT.(VALUE)\n",
            "            RESULT=INCREMENT.(A(1))\n",
            "            PRINT FORMAT NUMBER,VALUE\n",
            "            PRINT FORMAT NUMBER,A(1)\n",
        );
        let module = SourceModule::parse("writeback", source).unwrap();
        let program = Program::link(&[module]).unwrap();
        let mut machine = Machine::new(program, TestHost);
        assert_eq!(
            machine.run(100).unwrap(),
            RunState::OutputWord(Word::from_i64(2))
        );
        assert_eq!(
            machine.run(100).unwrap(),
            RunState::OutputWord(Word::from_i64(3))
        );
        assert_eq!(machine.run(100).unwrap(), RunState::Halted);
    }

    /// Scalars retain default-zero reads and updates across cooperative input.
    ///
    /// # Panics
    /// Panics if first insertion, overwrite, or input writeback changes.
    #[test]
    fn scalar_writes_preserve_values_across_input() {
        let source = concat!(
            "            PRINT FORMAT NUMBER,VALUE\n",
            "            VALUE=7\n",
            "            PRINT FORMAT NUMBER,VALUE\n",
            "            VALUE=VALUE+2\n",
            "            PRINT FORMAT NUMBER,VALUE\n",
            "            READ FORMAT NUMBER,VALUE\n",
            "            PRINT FORMAT NUMBER,VALUE\n",
        );
        let module = SourceModule::parse("scalar", source).unwrap();
        let program = Program::link(&[module]).unwrap();
        let mut machine = Machine::new(program, TestHost);
        for value in [0, 7, 9] {
            assert_eq!(
                machine.run(100).unwrap(),
                RunState::OutputWord(Word::from_i64(value))
            );
        }
        assert_eq!(machine.run(100).unwrap(), RunState::NeedsInput);
        machine.provide_input(Word::from_i64(11)).unwrap();
        assert_eq!(
            machine.run(100).unwrap(),
            RunState::OutputWord(Word::from_i64(11))
        );
        assert_eq!(machine.run(100).unwrap(), RunState::Halted);
    }

    /// Native writeback can create a scalar that previously read as zero.
    ///
    /// # Panics
    /// Panics if native scalar insertion or return-value assignment changes.
    #[test]
    fn native_arguments_create_absent_scalar() {
        let source = concat!(
            "            RESULT=INCREMENT.(VALUE)\n",
            "            PRINT FORMAT NUMBER,VALUE\n",
            "            PRINT FORMAT NUMBER,RESULT\n",
        );
        let module = SourceModule::parse("native_scalar", source).unwrap();
        let program = Program::link(&[module]).unwrap();
        let mut machine = Machine::new(program, TestHost);
        assert_eq!(
            machine.run(100).unwrap(),
            RunState::OutputWord(Word::from_i64(1))
        );
        assert_eq!(
            machine.run(100).unwrap(),
            RunState::OutputWord(Word::from_i64(1))
        );
        assert_eq!(machine.run(100).unwrap(), RunState::Halted);
    }

    /// Computed transfers evaluate a mutating index exactly once.
    ///
    /// # Panics
    /// Panics if index evaluation or computed label resolution changes.
    #[test]
    fn computed_transfer_evaluates_index_once() {
        let source = concat!(
            "            INDEX=0\n",
            "            T'O TARGET(INCREMENT.(INDEX))\n",
            "TARGET(1)   PRINT FORMAT NUMBER,INDEX\n",
            "            EXIT.\n",
        );
        let module = SourceModule::parse("computed", source).unwrap();
        let program = Program::link(&[module]).unwrap();
        let mut machine = Machine::new(program, TestHost);
        assert_eq!(
            machine.run(100).unwrap(),
            RunState::OutputWord(Word::from_i64(1))
        );
        assert_eq!(machine.run(100).unwrap(), RunState::Halted);
    }

    /// Missing computed targets retain their exact label and source location.
    ///
    /// # Panics
    /// Panics if linking or owned error diagnostics change.
    #[test]
    fn computed_transfer_error_outlives_machine() {
        let source = concat!(
            "            INDEX=0\n",
            "            T'O MISSING(INCREMENT.(INDEX))\n",
        );
        let module = SourceModule::parse("computed_missing", source).unwrap();
        let program = Program::link(&[module]).unwrap();
        let mut machine = Machine::new(program, TestHost);
        let error = machine.run(100).unwrap_err();
        assert_eq!(machine.scalars.get("INDEX"), Some(&Word::from_i64(1)));
        drop(machine);
        assert!(
            matches!(&error.kind, RuntimeErrorKind::UnknownLabel { label } if label == "MISSING(1)")
        );
        assert_eq!(error.span().unwrap().module(), "computed_missing");
        assert_eq!(error.span().unwrap().line(), 2);
    }

    /// Direct lookup failures own their label beyond the borrowed target.
    ///
    /// # Panics
    /// Panics if linking or owned direct-target diagnostics change.
    #[test]
    fn direct_target_error_outlives_target_and_machine() {
        let module = SourceModule::parse("direct_missing", "            EXIT.\n").unwrap();
        let program = Program::link(&[module]).unwrap();
        let mut machine = Machine::new(program, TestHost);
        let target = JumpTarget::Label("MISSING".to_owned());
        let error = machine.resolve_target(&target, None).unwrap_err();
        drop(target);
        drop(machine);
        assert!(
            matches!(&error.kind, RuntimeErrorKind::UnknownLabel { label } if label == "MISSING")
        );
        assert!(error.span().is_none());
    }
}
