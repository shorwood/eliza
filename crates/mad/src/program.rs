//! MAD expressions and executable program linking.

use std::collections::HashMap;

use thiserror::Error;

use crate::source::{SourceModule, SourceSpan, SourceStatement};
use crate::word::Word;

// -----------------------------------------------------------------------------
// Program: Owns linked instructions, labels, and shared grammar constants.
// -----------------------------------------------------------------------------

/// A linked program executable by [`crate::machine::Machine`].
#[derive(Clone, Debug)]
pub struct Program {
    /// Declared array bounds keyed by canonical name.
    pub(super) arrays: HashMap<String, usize>,
    /// Executable statements in linked order.
    pub(super) instructions: Vec<Instruction>,
    /// Statement labels mapped to program counters.
    pub(super) labels: HashMap<String, usize>,
}

impl Program {
    /// Number of linked source statements.
    #[must_use]
    pub fn instruction_count(&self) -> usize {
        self.instructions.len()
    }

    /// Add one label and compiled instruction.
    ///
    /// # Errors
    ///
    /// Returns a positioned compile or duplicate-label error.
    fn link_statement(&mut self, source: &SourceStatement) -> Result<(), LinkError> {
        if let Some(label) = source.label() {
            let label = canonical(label);
            let previous = self.labels.insert(label.clone(), self.instructions.len());

            // A label must resolve to exactly one instruction.
            if previous.is_some() {
                return Err(LinkError::at(
                    source.span(),
                    LinkErrorKind::DuplicateLabel { label },
                ));
            }
        }
        let instruction = Instruction::compile(source, &mut self.arrays)?;
        self.instructions.push(instruction);
        Ok(())
    }

    /// Add all statements in one source module.
    ///
    /// # Errors
    ///
    /// Returns a positioned compile or duplicate-label error.
    fn link_module(&mut self, module: &SourceModule) -> Result<(), LinkError> {
        for source in module.statements() {
            self.link_statement(source)?;
        }
        Ok(())
    }

    /// Validate one transfer target against linked labels.
    ///
    /// # Errors
    ///
    /// Returns a positioned error for an unknown direct label.
    fn validate_target(
        &self,
        instruction: &Instruction,
        target: &JumpTarget,
    ) -> Result<(), LinkError> {
        match target {
            JumpTarget::Label(label) if !self.labels.contains_key(label) => Err(LinkError::at(
                instruction.span(),
                LinkErrorKind::UnknownLabel {
                    label: label.clone(),
                },
            )),
            JumpTarget::Computed { .. } | JumpTarget::Label(_) => Ok(()),
        }
    }

    /// Validate all direct transfer targets.
    ///
    /// # Errors
    ///
    /// Returns the first unknown-label error.
    fn validate_targets(&self) -> Result<(), LinkError> {
        for instruction in &self.instructions {
            // Instructions without transfers have no target to validate.
            let Some(target) = instruction.target() else {
                continue;
            };
            self.validate_target(instruction, target)?;
        }
        Ok(())
    }

    /// Compile and link source modules in order.
    ///
    /// # Errors
    ///
    /// Returns a positioned error for duplicate labels, unsupported executable
    /// statements, malformed expressions, or unresolved transfers.
    pub fn link(modules: &[SourceModule]) -> Result<Self, LinkError> {
        let mut program = Self {
            arrays: HashMap::new(),
            instructions: Vec::new(),
            labels: HashMap::new(),
        };
        for module in modules {
            program.link_module(module)?;
        }
        program.validate_targets()?;
        Ok(program)
    }
}

/// IBM 7094 BCD code points in six-bit order.
const HOLLERITH_BCD_CODES: &str =
    "0123456789\0='\0\0\0+ABCDEFGHI\0.)\0\0\0-JKLMNOPQR\0$*\0\0\0 /STUVWXYZ\0,(\0\0\0";

/// Number of delimiter bytes surrounding a PRINT COMMENT payload.
const STATEMENT_COMMENT_DELIMITER_BYTES: usize = 2;

/// Characters stored in one 36-bit BCD word.
const HOLLERITH_WORD_CHARACTERS: usize = 6;

// -----------------------------------------------------------------------------
// Instruction: Represents the executable MAD subset.
// -----------------------------------------------------------------------------

/// One linked executable MAD statement.
#[derive(Clone, Debug)]
pub(super) enum Instruction {
    /// Store an expression in a scalar or array slot.
    Assign {
        /// Assignment destination.
        place: Place,
        /// Value expression.
        value: Expr,
        /// Source position.
        span: SourceSpan,
    },
    /// Emit source text from PRINT COMMENT.
    Comment {
        /// Comment payload.
        text: String,
        /// Source position.
        span: SourceSpan,
    },
    /// Retain a DIMENSION declaration in instruction order.
    Dimension {
        /// Source position.
        span: SourceSpan,
    },
    /// Evaluate a native function call for its side effects.
    Evaluate {
        /// Call expression.
        expression: Expr,
        /// Source position.
        span: SourceSpan,
    },
    /// Halt the program.
    Exit {
        /// Source position.
        span: SourceSpan,
    },
    /// Preserve a declaration or CONTINUE statement.
    Nop {
        /// Source position.
        span: SourceSpan,
    },
    /// Emit one machine word.
    Print {
        /// Value to emit.
        value: Expr,
        /// Source position.
        span: SourceSpan,
    },
    /// Suspend until one machine word is supplied.
    Read {
        /// Input destination.
        place: Place,
        /// Source position.
        span: SourceSpan,
    },
    /// Halt after assigning the conventional return value.
    Return {
        /// Function result.
        value: Expr,
        /// Source position.
        span: SourceSpan,
    },
    /// Execute an indexed THROUGH loop.
    Through {
        /// Initial induction value.
        start: Expr,
        /// Increment expression.
        step: Expr,
        /// Last statement in the loop body.
        target: JumpTarget,
        /// Termination condition.
        until: Expr,
        /// Induction variable name.
        variable: String,
        /// Source position.
        span: SourceSpan,
    },
    /// Jump to a label or computed label.
    Transfer {
        /// Jump destination.
        target: JumpTarget,
        /// Source position.
        span: SourceSpan,
    },
    /// Execute one statement when a condition is true.
    Whenever {
        /// Inline action.
        action: Box<Instruction>,
        /// Boolean condition.
        condition: Expr,
        /// Source position.
        span: SourceSpan,
    },
}

impl Instruction {
    /// Source position for diagnostics.
    pub(super) const fn span(&self) -> &SourceSpan {
        match self {
            Self::Assign { span, .. }
            | Self::Comment { span, .. }
            | Self::Dimension { span }
            | Self::Evaluate { span, .. }
            | Self::Exit { span }
            | Self::Nop { span }
            | Self::Print { span, .. }
            | Self::Read { span, .. }
            | Self::Return { span, .. }
            | Self::Through { span, .. }
            | Self::Transfer { span, .. }
            | Self::Whenever { span, .. } => span,
        }
    }

    /// Transfer target nested in this instruction, when present.
    fn target(&self) -> Option<&JumpTarget> {
        match self {
            Self::Through { target, .. } | Self::Transfer { target, .. } => Some(target),
            Self::Whenever { action, .. } => action.target(),
            Self::Assign { .. }
            | Self::Comment { .. }
            | Self::Dimension { .. }
            | Self::Evaluate { .. }
            | Self::Exit { .. }
            | Self::Nop { .. }
            | Self::Print { .. }
            | Self::Read { .. }
            | Self::Return { .. } => None,
        }
    }
}

// -----------------------------------------------------------------------------
// Expr: Models values, places, calls, and operators.
// -----------------------------------------------------------------------------

/// One MAD expression.
#[derive(Clone, Debug)]
pub(super) enum Expr {
    /// Binary arithmetic, comparison, or Boolean operation.
    Binary {
        /// Left operand.
        left: Box<Self>,
        /// Operator.
        operator: BinaryOperator,
        /// Right operand.
        right: Box<Self>,
    },
    /// Native function call.
    Call {
        /// Arguments evaluated from left to right.
        arguments: Vec<Self>,
        /// Canonical function name.
        name: String,
    },
    /// Literal machine word.
    Literal(
        /// Encoded literal.
        Word,
    ),
    /// Scalar or array reference.
    Place(
        /// Referenced storage.
        Place,
    ),
    /// Arithmetic negation.
    UnaryMinus(
        /// Negated expression.
        Box<Self>,
    ),
}

impl Expr {
    /// Borrow this expression as an assignable place.
    pub(super) fn as_place(&self) -> Option<&Place> {
        if let Self::Place(place) = self {
            Some(place)
        } else {
            None
        }
    }
}

// -----------------------------------------------------------------------------
// BinaryOperator: Defines evaluation and parser precedence.
// -----------------------------------------------------------------------------

/// Supported MAD binary operators.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BinaryOperator {
    /// Addition.
    Add,
    /// Short-circuit conjunction.
    And,
    /// Integer division.
    Divide,
    /// Raw-word equality.
    Equal,
    /// Signed greater-than comparison.
    Greater,
    /// Signed greater-than-or-equal comparison.
    GreaterEqual,
    /// Signed less-than comparison.
    Less,
    /// Signed less-than-or-equal comparison.
    LessEqual,
    /// Integer multiplication.
    Multiply,
    /// Raw-word inequality.
    NotEqual,
    /// Short-circuit disjunction.
    Or,
    /// Subtraction.
    Subtract,
}

impl BinaryOperator {
    /// Decode a canonical dotted operator.
    fn from_dotted(spelling: &str) -> Option<Self> {
        match spelling {
            ".AND." => Some(Self::And),
            ".E." => Some(Self::Equal),
            ".G." => Some(Self::Greater),
            ".GE." => Some(Self::GreaterEqual),
            ".L." => Some(Self::Less),
            ".LE." => Some(Self::LessEqual),
            ".NE." => Some(Self::NotEqual),
            ".OR." => Some(Self::Or),
            _ => None,
        }
    }

    /// Binding precedence used by the expression parser.
    const fn precedence(self) -> u8 {
        match self {
            Self::Or => 1,
            Self::And => 2,
            Self::Equal
            | Self::Greater
            | Self::GreaterEqual
            | Self::Less
            | Self::LessEqual
            | Self::NotEqual => 3,
            Self::Add | Self::Subtract => 4,
            Self::Divide | Self::Multiply => 5,
        }
    }
}

// -----------------------------------------------------------------------------
// JumpTarget: Names direct and computed transfer destinations.
// -----------------------------------------------------------------------------

/// A direct or indexed statement-label destination.
#[derive(Clone, Debug)]
pub(super) enum JumpTarget {
    /// Label formed from a base and evaluated numeric suffix.
    Computed {
        /// Label prefix.
        base: String,
        /// Numeric suffix expression.
        index: Expr,
    },
    /// One canonical statement label.
    Label(
        /// Canonical label.
        String,
    ),
}

// -----------------------------------------------------------------------------
// Place: Names scalar and indexed storage.
// -----------------------------------------------------------------------------

/// Assignable MAD storage.
#[derive(Clone, Debug)]
pub(super) enum Place {
    /// Indexed array element.
    Array {
        /// One-based array index.
        index: Box<Expr>,
        /// Canonical array name.
        name: String,
    },
    /// Scalar variable.
    Scalar(
        /// Canonical variable name.
        String,
    ),
}

// -----------------------------------------------------------------------------
// Statement: Lowers source text into executable instructions.
// -----------------------------------------------------------------------------

/// Two statement slices separated outside parentheses and Hollerith text.
struct StatementSplit<'source> {
    /// Text following the separator.
    after: &'source str,
    /// Text preceding the separator.
    before: &'source str,
}

impl StatementSplit<'_> {
    /// Split once on a delimiter outside parentheses and Hollerith text.
    fn at_top_level(text: &str, delimiter: char) -> Option<StatementSplit<'_>> {
        let mut depth = 0_usize;
        let mut found = None;
        let mut hollerith = false;
        for (index, character) in text.char_indices() {
            match character {
                '$' => hollerith = !hollerith,
                '(' if !hollerith => depth += 1,
                ')' if !hollerith => depth = depth.saturating_sub(1),
                value if value == delimiter && !hollerith && depth == 0 => {
                    found = Some(StatementSplit {
                        after: &text[index + value.len_utf8()..],
                        before: &text[..index],
                    });
                    break;
                }
                _ => {}
            }
        }
        found
    }
}

/// Trim and uppercase one MAD identifier or grammar fragment.
fn canonical(text: &str) -> String {
    text.trim().to_ascii_uppercase()
}

/// Original statement text paired with its canonical comparison form.
struct StatementText<'source> {
    /// Source spelling preserved for expression slices.
    original: &'source str,
    /// Trimmed uppercase spelling used for grammar decisions.
    upper: String,
}

impl<'source> From<&'source str> for StatementText<'source> {
    fn from(original: &'source str) -> Self {
        Self {
            original,
            upper: canonical(original),
        }
    }
}

impl<'source> StatementText<'source> {
    /// Test one canonical prefix.
    fn starts_with(&self, prefix: &str) -> bool {
        self.upper.starts_with(prefix)
    }

    /// Remove one canonical prefix from the source spelling.
    fn strip_prefix(&self, prefix: &str) -> Option<&'source str> {
        self.upper
            .starts_with(prefix)
            .then(|| &self.original[prefix.len()..])
    }

    /// Remove the first matching canonical prefix from the source spelling.
    fn strip_any_prefix(&self, prefixes: &[&str]) -> Option<&'source str> {
        prefixes.iter().find_map(|prefix| self.strip_prefix(prefix))
    }
}

/// Locate an assignment operator outside parentheses and Hollerith text.
fn find_assignment(text: &str) -> Option<usize> {
    let mut depth = 0_usize;
    let mut hollerith = false;
    for (index, character) in text.char_indices() {
        match character {
            '$' => hollerith = !hollerith,
            '(' if !hollerith => depth += 1,
            ')' if !hollerith => depth = depth.saturating_sub(1),
            // Only a top-level equals sign assigns storage.
            '=' if !hollerith && depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

/// Recognize declarations that affect neither this subset nor execution.
fn is_declaration(upper: &str) -> bool {
    [
        "ENTRY TO ",
        "EQUIVALENCE ",
        "EXTERNAL FUNCTION",
        "FLOATING POINT ",
        "NORMAL MODE ",
        "PROGRAM COMMON ",
        "VECTOR VALUES ",
    ]
    .iter()
    .any(|prefix| upper.starts_with(prefix))
}

/// Remove PRINT COMMENT Hollerith delimiters.
///
/// # Errors
///
/// Returns a positioned error when either delimiter is absent.
fn parse_comment(text: &str, span: &SourceSpan) -> Result<String, LinkError> {
    let trimmed = text.trim();

    // Comment payloads require a complete pair of Hollerith markers.
    if !trimmed.starts_with('$')
        || !trimmed.ends_with('$')
        || trimmed.len() < STATEMENT_COMMENT_DELIMITER_BYTES
    {
        return Err(LinkError::at(span, LinkErrorKind::PrintCommentDelimiters));
    }
    Ok(trimmed[1..trimmed.len() - 1].to_owned())
}

/// Parse one complete MAD expression.
///
/// # Errors
///
/// Returns a positioned lexical or syntax error.
fn parse_expression(text: &str, span: &SourceSpan) -> Result<Expr, LinkError> {
    ExpressionParser::new(text, span)?.parse()
}

impl JumpTarget {
    /// Parse a direct or computed transfer destination.
    ///
    /// # Errors
    ///
    /// Returns a positioned error when a computed suffix is malformed.
    fn parse(text: &str, span: &SourceSpan) -> Result<Self, LinkError> {
        let trimmed = text.trim();

        // Parenthesized suffixes form computed label destinations.
        if let Some((base, index)) = trimmed
            .strip_suffix(')')
            .and_then(|value| value.split_once('('))
        {
            return Ok(Self::Computed {
                base: canonical(base),
                index: parse_expression(index, span)?,
            });
        }
        Ok(Self::Label(canonical(trimmed)))
    }
}

impl Instruction {
    /// Compile a THROUGH loop statement.
    ///
    /// # Errors
    ///
    /// Returns a positioned error when its FOR clauses are malformed.
    fn compile_through(text: &str, span: &SourceSpan) -> Result<Self, LinkError> {
        let target_and_for = StatementSplit::at_top_level(text, ',')
            .ok_or_else(|| LinkError::at(span, LinkErrorKind::ThroughForClause))?;
        let clauses = target_and_for.after.trim();
        let clauses = clauses
            .strip_prefix("FOR ")
            .or_else(|| clauses.strip_prefix("for "))
            .ok_or_else(|| LinkError::at(span, LinkErrorKind::ThroughForKeyword))?;
        let assignment_and_rest = StatementSplit::at_top_level(clauses, ',')
            .ok_or_else(|| LinkError::at(span, LinkErrorKind::ForClauses))?;
        let step_and_until = StatementSplit::at_top_level(assignment_and_rest.after, ',')
            .ok_or_else(|| LinkError::at(span, LinkErrorKind::ForClauses))?;
        let assignment = assignment_and_rest.before;
        let assignment_index = find_assignment(assignment)
            .ok_or_else(|| LinkError::at(span, LinkErrorKind::ForAssignment))?;
        let variable = canonical(&assignment[..assignment_index]);
        Ok(Self::Through {
            start: parse_expression(&assignment[assignment_index + 1..], span)?,
            step: parse_expression(step_and_until.before, span)?,
            target: JumpTarget::parse(target_and_for.before, span)?,
            until: parse_expression(step_and_until.after, span)?,
            variable,
            span: span.clone(),
        })
    }
}

/// Parse an assignable scalar or array place.
///
/// # Errors
///
/// Returns a positioned error when the expression is not assignable.
fn parse_place(text: &str, span: &SourceSpan) -> Result<Place, LinkError> {
    let expression = parse_expression(text.trim(), span)?;
    match expression {
        Expr::Place(place) => Ok(place),
        _ => Err(LinkError::at(span, LinkErrorKind::AssignmentDestination)),
    }
}

/// Split comma-separated text outside parentheses and Hollerith text.
///
/// # Errors
///
/// Returns an error when parentheses or Hollerith markers remain open.
fn split_arguments<'source>(
    text: &'source str,
    span: &SourceSpan,
) -> Result<Vec<&'source str>, LinkError> {
    let mut arguments = Vec::new();
    let mut start = 0;
    let mut depth = 0_usize;
    let mut hollerith = false;
    for (index, character) in text.char_indices() {
        match character {
            '$' => hollerith = !hollerith,
            '(' if !hollerith => depth += 1,
            ')' if !hollerith => depth = depth.saturating_sub(1),
            ',' if !hollerith && depth == 0 => {
                arguments.push(&text[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }

    // Open delimiters make the complete argument list ambiguous.
    if hollerith || depth != 0 {
        return Err(LinkError::at(span, LinkErrorKind::MalformedArgumentList));
    }
    arguments.push(&text[start..]);
    Ok(arguments)
}

/// Record all array declarations from one DIMENSION statement.
///
/// # Errors
///
/// Returns a positioned error for malformed names or bounds.
fn parse_dimensions(
    text: &str,
    arrays: &mut HashMap<String, usize>,
    span: &SourceSpan,
) -> Result<(), LinkError> {
    let declarations = text
        .split_once(char::is_whitespace)
        .map(|(_, rest)| rest)
        .unwrap_or_default();
    for declaration in split_arguments(declarations, span)? {
        let declaration = declaration.trim();
        let declaration = declaration
            .strip_suffix(')')
            .ok_or_else(|| LinkError::at(span, LinkErrorKind::MalformedDimension))?;
        let (name, size) = declaration
            .split_once('(')
            .ok_or_else(|| LinkError::at(span, LinkErrorKind::MalformedDimension))?;
        let size = size
            .trim()
            .parse::<usize>()
            .map_err(|_| LinkError::at(span, LinkErrorKind::InvalidArraySize))?;
        arrays.insert(canonical(name), size);
    }
    Ok(())
}

impl Instruction {
    /// Compile a PRINT FORMAT statement.
    ///
    /// # Errors
    ///
    /// Returns an error when the value expression is absent or malformed.
    fn compile_print(text: &str, span: &SourceSpan) -> Result<Self, LinkError> {
        let rest = text
            .split_once(',')
            .map(|(_, value)| value)
            .ok_or_else(|| LinkError::at(span, LinkErrorKind::MissingPrintValue))?;
        Ok(Self::Print {
            value: parse_expression(rest, span)?,
            span: span.clone(),
        })
    }

    /// Compile an inline WHENEVER statement.
    ///
    /// # Errors
    ///
    /// Returns an error when its condition or action is malformed.
    fn compile_whenever(
        rest: &str,
        source: &SourceStatement,
        arrays: &mut HashMap<String, usize>,
    ) -> Result<Self, LinkError> {
        let split = StatementSplit::at_top_level(rest, ',')
            .ok_or_else(|| LinkError::at(source.span(), LinkErrorKind::BlockWhenever))?;
        let action_source =
            SourceStatement::synthetic(split.after.trim().to_owned(), source.span().clone());
        Ok(Self::Whenever {
            action: Box::new(Self::compile(&action_source, arrays)?),
            condition: parse_expression(split.before, source.span())?,
            span: source.span().clone(),
        })
    }

    /// Compile one source statement.
    ///
    /// # Errors
    ///
    /// Returns a positioned error for unsupported syntax or malformed expressions.
    fn compile(
        source: &SourceStatement,
        arrays: &mut HashMap<String, usize>,
    ) -> Result<Self, LinkError> {
        let text = source.text().trim();
        let statement = StatementText::from(text);
        let span = source.span().clone();

        // Declarations allocate storage but do not execute.
        if statement.starts_with("DIMENSION ") {
            parse_dimensions(text, arrays, source.span())?;
            return Ok(Self::Dimension { span });
        }

        // Other declarations and CONTINUE preserve label positions as no-ops.
        if is_declaration(&statement.upper) || statement.upper == "CONTINUE" {
            return Ok(Self::Nop { span });
        }

        // Every accepted exit spelling halts the bounded machine.
        if matches!(
            statement.upper.as_str(),
            "EXIT." | "END OF FUNCTION" | "E'M"
        ) {
            return Ok(Self::Exit { span });
        }

        // Both function-return spellings carry one value expression.
        if let Some(rest) = statement.strip_any_prefix(&["FUNCTION RETURN ", "F'N "]) {
            return Ok(Self::Return {
                value: parse_expression(rest, source.span())?,
                span,
            });
        }

        // The comment form emits its Hollerith payload directly.
        if let Some(rest) = statement.strip_prefix("PRINT COMMENT ") {
            let text = parse_comment(rest, source.span())?;
            return Ok(Self::Comment { text, span });
        }

        // Both print forms place their output value after the first comma.
        if statement.starts_with("PRINT FORMAT ") || statement.starts_with("PRINT ON LINE FORMAT ")
        {
            return Self::compile_print(text, source.span());
        }

        // READ FORMAT stores one supplied word in its comma-separated place.
        if statement.starts_with("READ FORMAT ") {
            let rest = text
                .split_once(',')
                .map(|(_, place)| place)
                .ok_or_else(|| {
                    LinkError::at(source.span(), LinkErrorKind::MissingReadDestination)
                })?;
            return Ok(Self::Read {
                place: parse_place(rest, source.span())?,
                span,
            });
        }

        // Direct and computed transfers share the same target parser.
        if let Some(rest) = statement.strip_any_prefix(&["T'O ", "TRANSFER TO "]) {
            return Ok(Self::Transfer {
                target: JumpTarget::parse(rest, source.span())?,
                span,
            });
        }

        // THROUGH owns its full FOR-clause grammar.
        if let Some(rest) = statement.strip_any_prefix(&["T'H ", "THROUGH "]) {
            return Self::compile_through(rest, source.span());
        }

        // This slice executes the inline form and rejects block conditionals below.
        if let Some(rest) = statement.strip_any_prefix(&["W'R ", "WHENEVER "]) {
            return Self::compile_whenever(rest, source, arrays);
        }

        // Parsing block syntax without executing it would silently change semantics.
        if matches!(
            statement.upper.as_str(),
            "O'E" | "OTHERWISE" | "E'L" | "END OF CONDITIONAL"
        ) {
            return Err(LinkError::at(
                source.span(),
                LinkErrorKind::BlockConditional,
            ));
        }

        // A top-level equals sign distinguishes assignment from a call statement.
        if let Some(index) = find_assignment(text) {
            return Ok(Self::Assign {
                place: parse_place(&text[..index], source.span())?,
                value: parse_expression(&text[index + 1..], source.span())?,
                span,
            });
        }

        let expression = parse_expression(text.trim_start_matches("EXECUTE "), source.span())?;

        // Bare expression statements are valid only for native side effects.
        if !matches!(expression, Expr::Call { .. }) {
            return Err(LinkError::at(
                source.span(),
                LinkErrorKind::ExpressionStatement,
            ));
        }
        Ok(Self::Evaluate { expression, span })
    }
}

// -----------------------------------------------------------------------------
// Token: Represents the bounded MAD expression vocabulary.
// -----------------------------------------------------------------------------

/// One lexical expression token.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Token {
    /// Argument separator.
    Comma,
    /// Canonical identifier.
    Identifier(
        /// Identifier spelling.
        String,
    ),
    /// Opening parenthesis.
    LeftParen,
    /// Encoded literal.
    Literal(
        /// Literal value.
        Word,
    ),
    /// Binary operator.
    Operator(
        /// Operator kind.
        BinaryOperator,
    ),
    /// Function-call period.
    Period,
    /// Closing parenthesis.
    RightParen,
    /// Prefix negation.
    UnaryMinus,
}

// -----------------------------------------------------------------------------
// ExpressionParser: Applies precedence and call/place syntax.
// -----------------------------------------------------------------------------

/// Cursor over one tokenized expression.
struct ExpressionParser<'a> {
    /// Index of the next token.
    position: usize,
    /// Source position used by all expression diagnostics.
    span: &'a SourceSpan,
    /// Complete token stream.
    tokens: Vec<Token>,
}

impl<'a> ExpressionParser<'a> {
    /// Tokenize one expression into a fresh parser.
    ///
    /// # Errors
    ///
    /// Returns a positioned lexical error.
    fn new(text: &str, span: &'a SourceSpan) -> Result<Self, LinkError> {
        Ok(Self {
            position: 0,
            span,
            tokens: Lexer::new(text, span).tokenize()?,
        })
    }

    /// Borrow the next token without consuming it.
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    /// Consume one required token.
    ///
    /// # Errors
    ///
    /// Returns a positioned error when the next token differs.
    fn require(&mut self, expected: Token, kind: LinkErrorKind) -> Result<(), LinkError> {
        if self.next() == Some(expected) {
            Ok(())
        } else {
            Err(LinkError::at(self.span, kind))
        }
    }

    /// Parse a comma-separated call argument list.
    ///
    /// # Errors
    ///
    /// Returns a positioned error when separators or closing syntax are absent.
    fn parse_arguments(&mut self) -> Result<Vec<Expr>, LinkError> {
        let mut arguments = Vec::new();

        // A close parenthesis immediately after the open marker means no arguments.
        if self.peek() == Some(&Token::RightParen) {
            self.position += 1;
            return Ok(arguments);
        }
        loop {
            arguments.push(self.parse_precedence(0)?);
            match self.next() {
                Some(Token::Comma) => {}
                Some(Token::RightParen) => return Ok(arguments),
                _ => {
                    return Err(LinkError::at(
                        self.span,
                        LinkErrorKind::ExpectedArgumentSeparator,
                    ));
                }
            }
        }
    }

    /// Parse operators at or above one binding precedence.
    ///
    /// # Errors
    ///
    /// Returns a positioned operand or operator error.
    fn parse_precedence(&mut self, minimum: u8) -> Result<Expr, LinkError> {
        let first = self.next();
        let mut left = self.parse_primary_token(first)?;
        while let Some(Token::Operator(operator)) = self.peek() {
            let precedence = operator.precedence();
            if precedence < minimum {
                break;
            }
            let operator = *operator;
            self.position += 1;
            let right = self.parse_precedence(precedence + 1)?;
            left = Expr::Binary {
                left: Box::new(left),
                operator,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    /// Parse one literal, place, call, parenthesized value, or negation.
    ///
    /// # Errors
    ///
    /// Returns a positioned error for malformed primary-expression syntax.
    fn parse_primary_token(&mut self, token: Option<Token>) -> Result<Expr, LinkError> {
        match token {
            Some(Token::Literal(value)) => Ok(Expr::Literal(value)),
            Some(Token::UnaryMinus) => {
                let operand = self.next();
                Ok(Expr::UnaryMinus(Box::new(
                    self.parse_primary_token(operand)?,
                )))
            }
            Some(Token::LeftParen) => self.parse_parenthesized_expression(),
            Some(Token::Identifier(name)) => self.parse_identifier_expression(name),
            _ => Err(LinkError::at(self.span, LinkErrorKind::ExpectedExpression)),
        }
    }

    /// Parse a scalar, array element, or native call after its identifier.
    ///
    /// # Errors
    ///
    /// Returns a positioned error for malformed array or call syntax.
    fn parse_identifier_expression(&mut self, name: String) -> Result<Expr, LinkError> {
        match self.peek() {
            Some(Token::Period) => self.parse_call(name),
            Some(Token::LeftParen) => self.parse_array_place(name),
            _ => Ok(Expr::Place(Place::Scalar(name))),
        }
    }

    /// Parse an indexed array place after its name.
    ///
    /// # Errors
    ///
    /// Returns a positioned error when the closing parenthesis is absent.
    fn parse_array_place(&mut self, name: String) -> Result<Expr, LinkError> {
        self.position += 1;
        let index = self.parse_precedence(0)?;
        self.require(Token::RightParen, LinkErrorKind::ExpectedArrayClose)?;
        Ok(Expr::Place(Place::Array {
            index: Box::new(index),
            name,
        }))
    }

    /// Parse a native call after its function name.
    ///
    /// # Errors
    ///
    /// Returns a positioned error for malformed opening or argument syntax.
    fn parse_call(&mut self, name: String) -> Result<Expr, LinkError> {
        self.position += 1;
        self.require(Token::LeftParen, LinkErrorKind::ExpectedCallOpen)?;
        Ok(Expr::Call {
            arguments: self.parse_arguments()?,
            name,
        })
    }

    /// Parse an expression enclosed in parentheses.
    ///
    /// # Errors
    ///
    /// Returns a positioned error when the closing parenthesis is absent.
    fn parse_parenthesized_expression(&mut self) -> Result<Expr, LinkError> {
        let expression = self.parse_precedence(0)?;
        self.require(Token::RightParen, LinkErrorKind::ExpectedClose)?;
        Ok(expression)
    }

    /// Parse one complete expression.
    ///
    /// # Errors
    ///
    /// Returns a positioned syntax error or trailing-token error.
    fn parse(mut self) -> Result<Expr, LinkError> {
        let expression = self.parse_precedence(0)?;

        // A complete expression must consume the entire token stream.
        if self.position != self.tokens.len() {
            return Err(LinkError::at(self.span, LinkErrorKind::TrailingToken));
        }
        Ok(expression)
    }
}

impl Iterator for ExpressionParser<'_> {
    type Item = Token;

    /// Consume the next token.
    fn next(&mut self) -> Option<Self::Item> {
        let token = self.tokens.get(self.position).cloned();
        self.position += usize::from(token.is_some());
        token
    }
}

// -----------------------------------------------------------------------------
// Lexer: Converts expression text into positioned tokens.
// -----------------------------------------------------------------------------

/// Radix selected by a trailing K integer marker.
const LEXER_OCTAL_RADIX: u32 = 8;

/// Default integer radix.
const LEXER_DECIMAL_RADIX: u32 = 10;

/// Cursor that tokenizes one ASCII expression.
struct Lexer<'source> {
    /// Current byte offset.
    position: usize,
    /// Source position shared by lexical errors.
    span: &'source SourceSpan,
    /// Expression text.
    text: &'source str,
    /// Tokens emitted so far.
    tokens: Vec<Token>,
}

impl<'source> Lexer<'source> {
    /// Start at the first byte of one expression.
    fn new(text: &'source str, span: &'source SourceSpan) -> Self {
        Self {
            position: 0,
            span,
            text,
            tokens: Vec::new(),
        }
    }

    /// Emit one single-byte token.
    fn push(&mut self, token: Token) {
        self.tokens.push(token);
        self.position += 1;
    }

    /// Tokenize a Hollerith literal.
    ///
    /// # Errors
    ///
    /// Returns a positioned error when its closing marker is absent.
    fn tokenize_hollerith(&mut self) -> Result<(), LinkError> {
        let content_start = self.position + 1;
        let end = self.text[content_start..]
            .find('$')
            .map(|offset| content_start + offset)
            .ok_or_else(|| LinkError::at(self.span, LinkErrorKind::UnclosedHollerith))?;
        let value = hollerith_encode(&self.text[content_start..end], self.span)?;
        self.tokens.push(Token::Literal(value));
        self.position = end + 1;
        Ok(())
    }

    /// Tokenize an identifier.
    fn tokenize_identifier(&mut self) {
        let start = self.position;
        while self
            .text
            .as_bytes()
            .get(self.position)
            .is_some_and(|value| value.is_ascii_alphanumeric() || *value == b'_')
        {
            self.position += 1;
        }
        self.tokens.push(Token::Identifier(canonical(
            &self.text[start..self.position],
        )));
    }

    /// Distinguish prefix negation from binary subtraction.
    fn tokenize_minus(&mut self) {
        let is_unary = self.tokens.last().is_none_or(|token| {
            matches!(token, Token::Comma | Token::LeftParen | Token::Operator(_))
        });
        let token = if is_unary {
            Token::UnaryMinus
        } else {
            Token::Operator(BinaryOperator::Subtract)
        };
        self.push(token);
    }

    /// Tokenize a decimal or K-suffixed octal integer.
    ///
    /// # Errors
    ///
    /// Returns a positioned error when the integer exceeds the host range.
    fn tokenize_number(&mut self) -> Result<(), LinkError> {
        let start = self.position;
        while self
            .text
            .as_bytes()
            .get(self.position)
            .is_some_and(u8::is_ascii_digit)
        {
            self.position += 1;
        }
        let is_octal = self
            .text
            .as_bytes()
            .get(self.position)
            .is_some_and(|value| matches!(value, b'K' | b'k'));
        self.position += usize::from(is_octal);
        let digits = &self.text[start..self.position - usize::from(is_octal)];
        let radix = if is_octal {
            LEXER_OCTAL_RADIX
        } else {
            LEXER_DECIMAL_RADIX
        };
        let value = i64::from_str_radix(digits, radix)
            .map_err(|_| LinkError::at(self.span, LinkErrorKind::InvalidInteger))?;
        self.tokens.push(Token::Literal(Word::from_i64(value)));
        Ok(())
    }

    /// Tokenize a dotted Boolean or comparison operator.
    ///
    /// # Errors
    ///
    /// Returns an error for an unclosed or unknown operator.
    fn tokenize_dotted_operator(&mut self) -> Result<(), LinkError> {
        let end = self.text[self.position + 1..]
            .find('.')
            .map(|offset| self.position + 1 + offset)
            .ok_or_else(|| LinkError::at(self.span, LinkErrorKind::UnclosedDottedOperator))?;
        let spelling = canonical(&self.text[self.position..=end]);
        let operator = BinaryOperator::from_dotted(&spelling)
            .ok_or_else(|| LinkError::at(self.span, LinkErrorKind::UnknownDottedOperator))?;
        self.tokens.push(Token::Operator(operator));
        self.position = end + 1;
        Ok(())
    }

    /// Tokenize a call period or dotted operator.
    ///
    /// # Errors
    ///
    /// Returns a positioned error for unclosed or unknown dotted operators.
    fn tokenize_period(&mut self) -> Result<(), LinkError> {
        // A period before an opening parenthesis introduces a call.
        if self.text.as_bytes().get(self.position + 1) == Some(&b'(') {
            self.push(Token::Period);
            return Ok(());
        }
        self.tokenize_dotted_operator()
    }

    /// Tokenize one non-whitespace byte at the current position.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed or unsupported syntax.
    fn tokenize_non_whitespace(&mut self, byte: u8) -> Result<(), LinkError> {
        match byte {
            b',' => self.push(Token::Comma),
            b'(' => self.push(Token::LeftParen),
            b')' => self.push(Token::RightParen),
            b'+' => self.push(Token::Operator(BinaryOperator::Add)),
            b'-' => self.tokenize_minus(),
            b'*' => self.push(Token::Operator(BinaryOperator::Multiply)),
            b'/' => self.push(Token::Operator(BinaryOperator::Divide)),
            b'$' => self.tokenize_hollerith()?,
            b'.' => self.tokenize_period()?,
            value if value.is_ascii_digit() => self.tokenize_number()?,
            value if value.is_ascii_alphabetic() || value == b'_' => self.tokenize_identifier(),
            // All remaining bytes are unsupported expression syntax.
            _ => {
                return Err(LinkError::at(
                    self.span,
                    LinkErrorKind::UnexpectedCharacter { byte },
                ));
            }
        }
        Ok(())
    }

    /// Tokenize the item at the current byte.
    ///
    /// # Errors
    ///
    /// Returns a positioned error for malformed or unsupported syntax.
    fn tokenize_one(&mut self) -> Result<(), LinkError> {
        let byte = self.text.as_bytes()[self.position];

        // Whitespace separates tokens without producing one.
        if byte.is_ascii_whitespace() {
            self.position += 1;
            return Ok(());
        }
        self.tokenize_non_whitespace(byte)
    }

    /// Tokenize the complete expression.
    ///
    /// # Errors
    ///
    /// Returns a positioned error for malformed or unsupported tokens.
    fn tokenize(mut self) -> Result<Vec<Token>, LinkError> {
        while self.position < self.text.len() {
            self.tokenize_one()?;
        }
        Ok(self.tokens)
    }
}

// -----------------------------------------------------------------------------
// HollerithEncode: Converts source literals to 7094 BCD words.
// -----------------------------------------------------------------------------

/// Encode one six-character Hollerith word.
///
/// # Errors
///
/// Returns a positioned error for excess length or unsupported BCD characters.
fn hollerith_encode(text: &str, span: &SourceSpan) -> Result<Word, LinkError> {
    // One machine word cannot preserve additional characters.
    if text.chars().count() > HOLLERITH_WORD_CHARACTERS {
        return Err(LinkError::at(span, LinkErrorKind::HollerithTooLong));
    }
    let mut raw = 0_u64;
    let padded = text.chars().chain(std::iter::repeat(' '));
    for character in padded.take(HOLLERITH_WORD_CHARACTERS) {
        let character = character.to_ascii_uppercase();
        let code = HOLLERITH_BCD_CODES
            .chars()
            .position(|candidate| candidate == character)
            .ok_or_else(|| LinkError::at(span, LinkErrorKind::InvalidBcdCharacter { character }))?;
        let code = u8::try_from(code)
            .map_err(|_| LinkError::at(span, LinkErrorKind::BcdCodeOutOfRange))?;
        raw = (raw << 6) | u64::from(code);
    }
    Ok(Word::from_raw(raw))
}

// -----------------------------------------------------------------------------
// LinkError: Carries a declared failure and its source position.
// -----------------------------------------------------------------------------

/// Executable syntax and linking failures recognized by this MAD subset.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
enum LinkErrorKind {
    /// An assignment target is not writable storage.
    #[error("assignment destination is not a variable")]
    AssignmentDestination,
    /// A BCD character index did not fit its machine representation.
    #[error("BCD character code is out of range")]
    BcdCodeOutOfRange,
    /// A block conditional reached the executable subset.
    #[error("block conditionals are not executable; use inline WHENEVER")]
    BlockConditional,
    /// A block `WHENEVER` reached the executable subset.
    #[error("block WHENEVER is parsed as source but is not executable")]
    BlockWhenever,
    /// One label was declared more than once.
    #[error("duplicate label {label}")]
    DuplicateLabel {
        /// Canonical duplicate label.
        label: String,
    },
    /// A bare executable expression was not a native call.
    #[error("an executable expression statement must be a function call")]
    ExpressionStatement,
    /// An array reference has no closing parenthesis.
    #[error("expected closing array parenthesis")]
    ExpectedArrayClose,
    /// A call argument was followed by invalid syntax.
    #[error("expected comma or closing parenthesis")]
    ExpectedArgumentSeparator,
    /// A native call has no opening parenthesis.
    #[error("expected opening parenthesis after call")]
    ExpectedCallOpen,
    /// A parenthesized expression has no closing parenthesis.
    #[error("expected closing parenthesis")]
    ExpectedClose,
    /// A primary expression was absent.
    #[error("expected expression")]
    ExpectedExpression,
    /// A `FOR` clause has no induction assignment.
    #[error("FOR requires an induction assignment")]
    ForAssignment,
    /// A `FOR` clause does not contain its three required parts.
    #[error("FOR requires start, step, and condition")]
    ForClauses,
    /// A Hollerith word exceeds one machine word.
    #[error("a Hollerith word contains at most six characters")]
    HollerithTooLong,
    /// An array declaration has an invalid bound.
    #[error("array size must be a non-negative integer")]
    InvalidArraySize,
    /// A Hollerith character is outside IBM 7094 BCD.
    #[error("{character:?} is not IBM 7094 BCD")]
    InvalidBcdCharacter {
        /// Unsupported character.
        character: char,
    },
    /// An integer literal could not be represented.
    #[error("invalid integer constant")]
    InvalidInteger,
    /// An argument list contains unbalanced delimiters.
    #[error("malformed argument list")]
    MalformedArgumentList,
    /// An array declaration has invalid syntax.
    #[error("malformed DIMENSION declaration")]
    MalformedDimension,
    /// A `PRINT FORMAT` statement has no value.
    #[error("PRINT FORMAT needs a value")]
    MissingPrintValue,
    /// A `READ FORMAT` statement has no destination.
    #[error("READ FORMAT needs a destination")]
    MissingReadDestination,
    /// A `PRINT COMMENT` payload lacks Hollerith delimiters.
    #[error("PRINT COMMENT must use $ delimiters")]
    PrintCommentDelimiters,
    /// A complete expression left tokens unread.
    #[error("unexpected token after expression")]
    TrailingToken,
    /// A `THROUGH` statement has no `FOR` clause.
    #[error("THROUGH requires a FOR clause")]
    ThroughForClause,
    /// A `THROUGH` clause omits the `FOR` keyword.
    #[error("THROUGH requires FOR")]
    ThroughForKeyword,
    /// A dotted operator has no closing period.
    #[error("unclosed dotted operator")]
    UnclosedDottedOperator,
    /// A Hollerith constant has no closing marker.
    #[error("unclosed Hollerith constant")]
    UnclosedHollerith,
    /// The lexer encountered unsupported syntax.
    #[error("unexpected character {byte:?}")]
    UnexpectedCharacter {
        /// Unsupported source byte.
        byte: u8,
    },
    /// A dotted operator is not part of the supported vocabulary.
    #[error("unknown dotted operator")]
    UnknownDottedOperator,
    /// A transfer references an undeclared label.
    #[error("unknown label {label}")]
    UnknownLabel {
        /// Canonical target label.
        label: String,
    },
}

/// MAD link or executable-syntax error.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{}:{}:{}: {kind}", .span.module(), .span.line(), .span.column())]
pub struct LinkError {
    /// Declared link failure.
    kind: LinkErrorKind,
    /// Source position of the failing statement.
    span: SourceSpan,
}

impl LinkError {
    /// Build a positioned link error.
    fn at(span: &SourceSpan, kind: LinkErrorKind) -> Self {
        Self {
            kind,
            span: span.clone(),
        }
    }

    /// Source position of the failing statement.
    #[must_use]
    pub const fn span(&self) -> &SourceSpan {
        &self.span
    }
}
