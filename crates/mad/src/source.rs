//! Fixed-form MAD source cards.

use thiserror::Error;

// -----------------------------------------------------------------------------
// SourceSpan: Locates one statement in one source module.
// -----------------------------------------------------------------------------

/// A source position in a MAD module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    /// One-based source column.
    column: usize,
    /// One-based source line.
    line: usize,
    /// Module name supplied by the caller.
    module: String,
}

impl SourceSpan {
    /// One-based source column.
    #[must_use]
    pub const fn column(&self) -> usize {
        self.column
    }

    /// One-based source line.
    #[must_use]
    pub const fn line(&self) -> usize {
        self.line
    }

    /// Module name supplied to the module parser.
    #[must_use]
    pub fn module(&self) -> &str {
        &self.module
    }
}

// -----------------------------------------------------------------------------
// SourceStatement: Holds one joined logical MAD statement.
// -----------------------------------------------------------------------------

/// One logical MAD statement after continuation cards are joined.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceStatement {
    /// Optional fixed-field statement label.
    label: Option<String>,
    /// Location of the first source card.
    span: SourceSpan,
    /// Statement text with card padding removed.
    text: String,
}

impl SourceStatement {
    /// Build an unlabeled statement derived from another statement.
    #[must_use]
    pub(super) fn synthetic(text: String, span: SourceSpan) -> Self {
        Self {
            label: None,
            span,
            text,
        }
    }

    /// Build a statement from its first physical card.
    fn from_card(line: SourceLine<'_>, card: Card<'_>) -> Self {
        Self {
            label: card.label.map(str::to_owned),
            span: line.span(card.code_column),
            text: card.code.to_owned(),
        }
    }

    /// Optional fixed-field statement label.
    #[must_use]
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// Location of the first card in this logical statement.
    #[must_use]
    pub const fn span(&self) -> &SourceSpan {
        &self.span
    }

    /// Statement text with card padding removed.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Validate delimiters after all continuation cards have been joined.
    ///
    /// # Errors
    ///
    /// Returns an error for unbalanced parentheses or Hollerith markers.
    fn validate(&self) -> Result<(), ParseError> {
        let mut depth = 0_usize;
        let mut in_hollerith = false;
        for (offset, character) in self.text.char_indices() {
            match character {
                '$' => in_hollerith = !in_hollerith,
                '(' if !in_hollerith => depth += 1,
                ')' if !in_hollerith => {
                    depth = depth.checked_sub(1).ok_or_else(|| {
                        ParseError::at_span(&self.span, offset + 1, ParseErrorKind::UnexpectedClose)
                    })?;
                }
                _ => {}
            }
        }

        // A Hollerith marker must be paired within the logical statement.
        if in_hollerith {
            return Err(ParseError::at_span(
                &self.span,
                self.text.len(),
                ParseErrorKind::UnclosedHollerith,
            ));
        }

        // Parentheses must balance after continuations are joined.
        if depth != 0 {
            return Err(ParseError::at_span(
                &self.span,
                self.text.len(),
                ParseErrorKind::UnclosedParenthesis,
            ));
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// SourceModule: Parses physical cards into positioned logical statements.
// -----------------------------------------------------------------------------

/// A parsed fixed-form MAD source module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceModule {
    /// Caller-provided module name.
    name: String,
    /// Logical statements in source order.
    statements: Vec<SourceStatement>,
}

impl SourceModule {
    /// Parse fixed-form MAD cards.
    ///
    /// Columns 1-11 hold a label, column 12 is the conventional comment or
    /// continuation control, columns 13-80 hold source, and later columns form
    /// an ignored sequence field. Archival listings that place their control
    /// character in columns 12-16 are accepted as well.
    ///
    /// # Errors
    ///
    /// Returns a positioned error for non-ASCII input, tabs, orphan
    /// continuations, unclosed Hollerith strings, or unbalanced parentheses.
    pub fn parse(name: impl Into<String>, source: &str) -> Result<Self, ParseError> {
        let name = name.into();
        let mut statements = SourceStatementList::default();

        // Convert each physical card without losing its original line number.
        for (line_index, raw_line) in source.lines().enumerate() {
            let line = SourceLine {
                module: &name,
                number: line_index + 1,
                text: raw_line,
            };
            line.validate()?;
            let card = Card::read(raw_line);
            if card.is_comment || card.code.is_empty() {
                continue;
            }
            statements.append(line, card)?;
        }

        Ok(Self {
            name,
            statements: statements.into_validated()?,
        })
    }

    /// Module name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Logical statements in source order.
    #[must_use]
    pub fn statements(&self) -> &[SourceStatement] {
        &self.statements
    }
}

// -----------------------------------------------------------------------------
// Card: Interprets one physical fixed-form record.
// -----------------------------------------------------------------------------

/// Number of source columns before an optional sequence field.
const CARD_WIDTH: usize = 80;

/// Zero-based column containing the conventional control character.
const CARD_CONTROL_INDEX: usize = 11;

/// Zero-based first column of the conventional statement field.
const CARD_CODE_START: usize = 12;

/// Number of columns scanned for control characters in archival listings.
const CARD_FLEXIBLE_CONTROL_WIDTH: usize = 16;

/// Number of columns reserved for a statement label.
const CARD_LABEL_WIDTH: usize = 11;

/// Borrowed fields decoded from one physical card.
#[derive(Clone, Copy)]
struct Card<'source> {
    /// Trimmed source statement field.
    code: &'source str,
    /// One-based column where the trimmed statement begins.
    code_column: usize,
    /// Whether the card is a comment.
    is_comment: bool,
    /// Whether the card continues its predecessor.
    is_continuation: bool,
    /// Optional fixed-field label.
    label: Option<&'source str>,
}

impl<'source> Card<'source> {
    /// Decode one ASCII physical card.
    fn read(line: &'source str) -> Self {
        let source_end = line.len().min(CARD_WIDTH);
        let source = &line[..source_end];

        // Short records have no fixed control column.
        if source.len() < CARD_CODE_START {
            return Self::read_short(source);
        }

        let label = source[..CARD_LABEL_WIDTH].trim();

        // Archival listings sometimes shift an otherwise empty control field.
        if let Some(card) = Self::read_flexible_control(source, label) {
            return card;
        }

        let control = source.as_bytes()[CARD_CONTROL_INDEX];
        let field = source[CARD_CODE_START..].trim();
        let code_offset = source[CARD_CODE_START..].find(field).unwrap_or_default();
        Self {
            code: field,
            code_column: CARD_CODE_START + code_offset + 1,
            is_comment: matches!(control, b'R' | b'r'),
            is_continuation: control.is_ascii_digit() && control != b'0',
            label: (!label.is_empty()).then_some(label),
        }
    }

    /// Decode a short free-form record.
    fn read_short(source: &'source str) -> Self {
        let code = source.trim();
        let code_column = source.find(code).map_or(1, |column| column + 1);
        let next = code.get(1..2);
        Self {
            code,
            code_column,
            is_comment: code.starts_with('R')
                && next.is_none_or(|value| matches!(value, " " | "*")),
            is_continuation: false,
            label: None,
        }
    }

    /// Decode control characters shifted right in archival listings.
    fn read_flexible_control(source: &'source str, label: &str) -> Option<Self> {
        // A populated label field rules out a shifted control marker.
        if !label.is_empty() {
            return None;
        }
        let leading = &source[..source.len().min(CARD_FLEXIBLE_CONTROL_WIDTH)];
        let (index, control) = leading
            .bytes()
            .enumerate()
            .find(|(_, byte)| !byte.is_ascii_whitespace())?;

        // Only columns at or beyond the conventional control field qualify.
        if index < CARD_CONTROL_INDEX {
            return None;
        }

        let next = source.as_bytes().get(index + 1).copied();

        // A nonzero digit marks a continuation card.
        if control.is_ascii_digit() && control != b'0' {
            let field = source[index + 1..].trim();
            let code_offset = source[index + 1..].find(field).unwrap_or_default();
            return Some(Self {
                code: field,
                code_column: index + code_offset + 2,
                is_comment: false,
                is_continuation: true,
                label: None,
            });
        }

        // An isolated R marks a comment card.
        if matches!(control, b'R' | b'r')
            && next.is_none_or(|value| value.is_ascii_whitespace() || value == b'*')
        {
            return Some(Self {
                code: "",
                code_column: index + 1,
                is_comment: true,
                is_continuation: false,
                label: None,
            });
        }
        None
    }
}

// -----------------------------------------------------------------------------
// SourceLine: Carries one physical card and its source identity.
// -----------------------------------------------------------------------------

/// One physical card with the location supplied by its module parser.
#[derive(Clone, Copy)]
struct SourceLine<'source> {
    /// Module containing the card.
    module: &'source str,
    /// One-based physical line number.
    number: usize,
    /// Unmodified physical card text.
    text: &'source str,
}

impl SourceLine<'_> {
    /// Locate one column on this physical card.
    fn span(&self, column: usize) -> SourceSpan {
        SourceSpan {
            column,
            line: self.number,
            module: self.module.to_owned(),
        }
    }

    /// Reject text that cannot be indexed as a historical ASCII card.
    ///
    /// # Errors
    ///
    /// Returns an error for the first non-ASCII character or tab.
    fn validate(&self) -> Result<(), ParseError> {
        // Byte-oriented card columns require ASCII input.
        if let Some((column, _)) = self
            .text
            .char_indices()
            .find(|(_, value)| !value.is_ascii())
        {
            return Err(ParseError::new(
                self.span(column + 1),
                ParseErrorKind::NonAscii,
            ));
        }

        // Tabs do not have a stable width in fixed-form source.
        if let Some(column) = self.text.find('\t') {
            return Err(ParseError::new(self.span(column + 1), ParseErrorKind::Tab));
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// SourceStatementList: Joins cards and validates logical statements.
// -----------------------------------------------------------------------------

/// Logical statements accumulated from physical cards.
#[derive(Default)]
struct SourceStatementList {
    /// Statements in source order.
    items: Vec<SourceStatement>,
}

impl SourceStatementList {
    /// Append one code card or join it to its predecessor.
    ///
    /// # Errors
    ///
    /// Returns an error when a continuation has no predecessor.
    fn append(&mut self, line: SourceLine<'_>, card: Card<'_>) -> Result<(), ParseError> {
        // A continuation extends the preceding logical statement.
        if card.is_continuation {
            let previous = self.items.last_mut().ok_or_else(|| {
                ParseError::new(
                    line.span(CARD_CONTROL_INDEX + 1),
                    ParseErrorKind::OrphanContinuation,
                )
            })?;
            previous.text.push_str(card.code);
            return Ok(());
        }

        self.items.push(SourceStatement::from_card(line, card));
        Ok(())
    }

    /// Validate joined syntax and release the accumulated statements.
    ///
    /// # Errors
    ///
    /// Returns the first malformed logical statement.
    fn into_validated(self) -> Result<Vec<SourceStatement>, ParseError> {
        for statement in &self.items {
            statement.validate()?;
        }
        Ok(self.items)
    }
}

// -----------------------------------------------------------------------------
// ParseError: Reports exact source-card failures.
// -----------------------------------------------------------------------------

/// Fixed-form source failures recognized by the MAD parser.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
enum ParseErrorKind {
    /// A continuation card appeared before any statement.
    #[error("continuation card has no preceding statement")]
    OrphanContinuation,
    /// A card contains a non-ASCII character.
    #[error("fixed-form MAD accepts ASCII cards only")]
    NonAscii,
    /// A tab made the fixed columns ambiguous.
    #[error("tabs are not valid in fixed-form MAD")]
    Tab,
    /// A close parenthesis has no matching open parenthesis.
    #[error("unexpected closing parenthesis")]
    UnexpectedClose,
    /// A Hollerith marker has no closing pair.
    #[error("unclosed Hollerith string")]
    UnclosedHollerith,
    /// An open parenthesis has no closing pair.
    #[error("unclosed parenthesis")]
    UnclosedParenthesis,
}

/// Positioned MAD source error.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error("{}:{}:{}: {kind}", .span.module, .span.line, .span.column)]
pub struct ParseError {
    /// Declared parse failure.
    kind: ParseErrorKind,
    /// Exact source position.
    span: SourceSpan,
}

impl ParseError {
    /// Build an error at an exact source position.
    fn new(span: SourceSpan, kind: ParseErrorKind) -> Self {
        Self { kind, span }
    }

    /// Build an error relative to a logical statement.
    fn at_span(span: &SourceSpan, offset: usize, kind: ParseErrorKind) -> Self {
        let mut location = span.clone();
        location.column += offset;
        Self::new(location, kind)
    }

    /// Error location.
    #[must_use]
    pub const fn span(&self) -> &SourceSpan {
        &self.span
    }
}
