//! Parser and validated model for the 1966 DOCTOR script.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;

use slip::pattern::{AssemblyItem, PatternItem};
use thiserror::Error as ThisError;

// -----------------------------------------------------------------------------
// Span: Locates syntax in the DOCTOR transcription.
// -----------------------------------------------------------------------------

/// One-based position in a DOCTOR script.
#[derive(Clone, Copy, Debug)]
struct Span {
    /// One-based source column.
    column: usize,
    /// One-based source line.
    line: usize,
}

impl Span {
    /// Position at the start of a script.
    const START: Self = Self { column: 1, line: 1 };

    /// Locate a byte offset in UTF-8 source text.
    fn at_offset(source: &str, offset: usize) -> Self {
        let prefix = &source[..offset];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let line_start = prefix.rfind('\n').map_or(0, |position| position + 1);
        Self {
            column: source[line_start..offset].chars().count() + 1,
            line,
        }
    }
}

impl From<&Lexer<'_>> for Span {
    fn from(lexer: &Lexer<'_>) -> Self {
        Self {
            column: lexer.position - lexer.line_start + 1,
            line: lexer.line,
        }
    }
}

// -----------------------------------------------------------------------------
// ScriptError: Reports positioned syntax and validation failures.
// -----------------------------------------------------------------------------

/// Grammar element named by [`ScriptErrorKind::Expected`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScriptExpectation {
    /// Atom naming one DLIST tag.
    DlistTag,
    /// List containing DLIST tags.
    DlistTagList,
    /// List containing a decomposition pattern.
    DecompositionPattern,
    /// Complete decomposition rule.
    DecompositionRule,
    /// Keyword atom.
    Keyword,
    /// Target atom in a keyword link.
    LinkKeyword,
    /// Memory-rule keyword atom.
    MemoryKeyword,
    /// Complete memory transformation.
    MemoryTransformation,
    /// Structured pattern group.
    PatternGroup,
    /// First atom in a structured pattern group.
    PatternGroupMarker,
    /// Member atom in a structured pattern group.
    PatternGroupValue,
    /// Words rewritten by PRE.
    PreWordList,
    /// List containing a reassembly.
    Reassembly,
    /// One word or capture in a reassembly.
    ReassemblyWord,
    /// Keyword atom at the start of a rule.
    RuleKeyword,
    /// Required script header marker.
    Start,
    /// Keyword substitution atom.
    Substitution,
    /// Literal word atom.
    Word,
    /// List containing literal words.
    WordList,
}

impl fmt::Display for ScriptExpectation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::DlistTag => "DLIST tag",
            Self::DlistTagList => "DLIST tag list",
            Self::DecompositionPattern => "decomposition pattern",
            Self::DecompositionRule => "decomposition rule",
            Self::Keyword => "keyword",
            Self::LinkKeyword => "link keyword",
            Self::MemoryKeyword => "memory keyword",
            Self::MemoryTransformation => "memory transformation",
            Self::PatternGroup => "pattern group",
            Self::PatternGroupMarker => "pattern group marker",
            Self::PatternGroupValue => "pattern group value",
            Self::PreWordList => "PRE word list",
            Self::Reassembly => "reassembly",
            Self::ReassemblyWord => "reassembly word",
            Self::RuleKeyword => "rule keyword",
            Self::Start => "START",
            Self::Substitution => "substitution",
            Self::Word => "word",
            Self::WordList => "word list",
        })
    }
}

/// Syntax and validation failures recognized in a DOCTOR script.
#[derive(Clone, Debug, Eq, PartialEq, ThisError)]
enum ScriptErrorKind {
    /// A reassembly capture refers outside its decomposition pattern.
    #[error("capture {index} is outside 1..={maximum}")]
    CaptureOutsidePattern {
        /// Invalid one-based capture index.
        index: usize,
        /// Largest valid one-based capture index.
        maximum: usize,
    },
    /// The script declares the same keyword rule twice.
    #[error("duplicate keyword rule")]
    DuplicateKeywordRule,
    /// The script declares more than one memory rule.
    #[error("duplicate MEMORY rule")]
    DuplicateMemoryRule,
    /// A keyword rule declares more than one fallback.
    #[error("duplicate rule-level link")]
    DuplicateRuleLink,
    /// A decomposition has no pattern form.
    #[error("decomposition has no pattern")]
    EmptyDecomposition,
    /// A decomposition has no reassembly form.
    #[error("decomposition has no reassembly")]
    EmptyReassembly,
    /// A pattern group contains no marker or values.
    #[error("empty pattern group")]
    EmptyPatternGroup,
    /// A pattern marker atom is empty.
    #[error("empty pattern marker")]
    EmptyPatternMarker,
    /// A keyword rule declares no useful property.
    #[error("empty keyword rule")]
    EmptyKeywordRule,
    /// A memory rule has no content.
    #[error("empty MEMORY rule")]
    EmptyMemoryRule,
    /// A decomposition pattern contains no items.
    #[error("empty decomposition pattern")]
    EmptyPattern,
    /// A form has the wrong grammar shape.
    #[error("expected {expected}")]
    Expected {
        /// Typed grammar element required at this position.
        expected: ScriptExpectation,
    },
    /// A rule-level link is followed by more rule data.
    #[error("rule-level link must be last")]
    LinkNotLast,
    /// A DLIST marker has no tag list.
    #[error("DLIST has no tag list")]
    MissingDlist,
    /// The complete script has no greeting.
    #[error("script has no greeting")]
    MissingGreeting,
    /// A keyword parser reached the end before completing its rule.
    #[error("missing keyword-rule item")]
    MissingKeywordItem,
    /// A memory rule has no keyword.
    #[error("MEMORY has no keyword")]
    MissingMemoryKeyword,
    /// The complete script has no memory rule.
    #[error("script has no MEMORY rule")]
    MissingMemoryRule,
    /// The complete script has no default rule.
    #[error("script has no NONE rule")]
    MissingNoneRule,
    /// A PRE reassembly has no target link.
    #[error("PRE requires a link")]
    MissingPreLink,
    /// A substitution marker has no replacement.
    #[error("substitution has no replacement")]
    MissingSubstitution,
    /// The greeting is not followed by `START`.
    #[error("script greeting must be followed by START")]
    MissingStart,
    /// A memory transformation has no separator.
    #[error("memory transformation has no = separator")]
    MissingTransformationSeparator,
    /// A substitution is duplicated or follows decompositions.
    #[error("misplaced or duplicate substitution")]
    MisplacedSubstitution,
    /// A pattern group starts with an unsupported marker.
    #[error("pattern list must start with * or /")]
    PatternMarker,
    /// A PRE form does not contain its two operands.
    #[error("PRE requires words and one link")]
    PreShape,
    /// A memory table does not contain four transformations.
    #[error("MEMORY must have four transformations")]
    TransformationCount,
    /// A top-level rule is not parenthesized.
    #[error("top-level script rule must be a list")]
    TopLevelList,
    /// The script contains a non-ASCII character.
    #[error("script must contain only ASCII characters")]
    NonAscii,
    /// A close parenthesis has no matching open parenthesis.
    #[error("unexpected closing parenthesis")]
    UnexpectedClose,
    /// Parsing required another form after end of input.
    #[error("unexpected end of script")]
    UnexpectedEnd,
    /// A list has no closing parenthesis.
    #[error("unclosed list")]
    UnclosedList,
}

/// DOCTOR script parse or validation failure.
#[derive(Clone, Debug, Eq, PartialEq, ThisError)]
#[error("{line}:{column}: {kind}")]
pub struct ScriptError {
    /// One-based source column.
    column: usize,
    /// One-based source line.
    line: usize,
    /// Declared script failure.
    kind: ScriptErrorKind,
}

impl ScriptError {
    /// Build a diagnostic at one source position.
    fn at(span: Span, kind: ScriptErrorKind) -> Self {
        Self {
            column: span.column,
            kind,
            line: span.line,
        }
    }
}

// -----------------------------------------------------------------------------
// Atom: Retains one symbol and its source position.
// -----------------------------------------------------------------------------

/// Atomic script symbol.
#[derive(Clone, Debug)]
struct Atom {
    /// Source position of the first character.
    span: Span,
    /// Canonical uppercase spelling.
    text: String,
}

impl Atom {
    /// Build a diagnostic at this atom.
    fn error(&self, kind: ScriptErrorKind) -> ScriptError {
        ScriptError::at(self.span, kind)
    }
}

// -----------------------------------------------------------------------------
// Form: Represents the script's list syntax.
// -----------------------------------------------------------------------------

/// One atom or parenthesized list in the script.
#[derive(Clone, Debug)]
enum Form {
    /// Atomic symbol.
    Atom(
        /// Positioned symbol.
        Atom,
    ),
    /// Parenthesized sequence.
    List {
        /// Child forms in source order.
        items: Vec<Self>,
        /// Position of the opening parenthesis.
        span: Span,
    },
}

impl Form {
    /// Borrow the atom text when this form is atomic.
    fn atom_text(&self) -> Option<&str> {
        match self {
            Self::Atom(atom) => Some(&atom.text),
            Self::List { .. } => None,
        }
    }

    /// Build a diagnostic at this form.
    fn error(&self, kind: ScriptErrorKind) -> ScriptError {
        let span = match self {
            Self::Atom(atom) => atom.span,
            Self::List { span, .. } => *span,
        };
        ScriptError::at(span, kind)
    }

    /// Borrow this form as an atom.
    ///
    /// # Errors
    ///
    /// Returns an error when this form is a list.
    fn atom(&self, expected: ScriptExpectation) -> Result<&Atom, ScriptError> {
        match self {
            Self::Atom(atom) => Ok(atom),
            Self::List { .. } => Err(self.error(ScriptErrorKind::Expected { expected })),
        }
    }

    /// Borrow this form as a list.
    ///
    /// # Errors
    ///
    /// Returns an error when this form is an atom.
    fn list(&self, expected: ScriptExpectation) -> Result<&[Self], ScriptError> {
        match self {
            Self::List { items, .. } => Ok(items),
            Self::Atom(_) => Err(self.error(ScriptErrorKind::Expected { expected })),
        }
    }
}

// -----------------------------------------------------------------------------
// TokenKind: Defines the script's lexical vocabulary.
// -----------------------------------------------------------------------------

/// Kind of one script token.
#[derive(Clone, Debug, Eq, PartialEq)]
enum TokenKind {
    /// Canonical uppercase symbol.
    Atom(
        /// Symbol spelling.
        String,
    ),
    /// Opening parenthesis.
    Left,
    /// Closing parenthesis.
    Right,
}

// -----------------------------------------------------------------------------
// Token: Couples one lexical item to its source position.
// -----------------------------------------------------------------------------

/// Positioned script token.
#[derive(Clone, Debug)]
struct Token {
    /// Lexical token kind.
    kind: TokenKind,
    /// Position of the first token character.
    span: Span,
}

// -----------------------------------------------------------------------------
// Lexer: Converts ASCII script text into positioned tokens.
// -----------------------------------------------------------------------------

/// Cursor over one DOCTOR script.
struct Lexer<'source> {
    /// One-based source line.
    line: usize,
    /// Byte offset where the current line begins.
    line_start: usize,
    /// Current byte offset.
    position: usize,
    /// Complete source text.
    source: &'source str,
    /// Tokens emitted so far.
    tokens: Vec<Token>,
}

impl<'source> TryFrom<&'source str> for Lexer<'source> {
    type Error = ScriptError;

    fn try_from(source: &'source str) -> Result<Self, Self::Error> {
        // MAD-SLIP script symbols are defined over the terminal's ASCII subset.
        if let Some((offset, _)) = source.char_indices().find(|(_, value)| !value.is_ascii()) {
            return Err(ScriptError::at(
                Span::at_offset(source, offset),
                ScriptErrorKind::NonAscii,
            ));
        }

        // Begin tokenization at the first byte of the first line.
        Ok(Self {
            line: 1,
            line_start: 0,
            position: 0,
            source,
            tokens: Vec::new(),
        })
    }
}

impl Lexer<'_> {
    /// Read the byte at the cursor.
    fn current(&self) -> Option<u8> {
        self.source.as_bytes().get(self.position).copied()
    }

    /// Emit a one-byte token and advance.
    fn push(&mut self, kind: TokenKind) {
        self.tokens.push(Token {
            kind,
            span: Span::from(&*self),
        });
        self.position += 1;
    }

    /// Consume a comment, including its newline when present.
    fn skip_comment(&mut self) {
        while let Some(byte) = self.current() {
            self.position += 1;
            if byte == b'\n' {
                self.line += 1;
                self.line_start = self.position;
                break;
            }
        }
    }

    /// Consume and emit one atom.
    fn tokenize_atom(&mut self) {
        let span = Span::from(&*self);
        let start = self.position;
        while self.current().is_some_and(|byte| {
            !byte.is_ascii_whitespace() && !matches!(byte, b'(' | b')' | b'=' | b';')
        }) {
            self.position += 1;
        }
        self.tokens.push(Token {
            kind: TokenKind::Atom(self.source[start..self.position].to_ascii_uppercase()),
            span,
        });
    }

    /// Tokenize the complete source.
    fn tokenize(mut self) -> Vec<Token> {
        while let Some(byte) = self.current() {
            match byte {
                b'\n' => {
                    self.position += 1;
                    self.line += 1;
                    self.line_start = self.position;
                }
                b';' => self.skip_comment(),
                b'(' => self.push(TokenKind::Left),
                b')' => self.push(TokenKind::Right),
                b'=' => self.push(TokenKind::Atom("=".to_owned())),
                byte if byte.is_ascii_whitespace() => self.position += 1,
                _ => self.tokenize_atom(),
            }
        }
        self.tokens
    }
}

// -----------------------------------------------------------------------------
// FormParser: Builds nested forms from the token stream.
// -----------------------------------------------------------------------------

/// Cursor over positioned script tokens.
struct FormParser {
    /// Index of the next token.
    position: usize,
    /// Complete token stream.
    tokens: Vec<Token>,
}

impl FromStr for FormParser {
    type Err = ScriptError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            position: 0,
            tokens: Lexer::try_from(source)?.tokenize(),
        })
    }
}

impl FormParser {
    /// Parse a list after its opening parenthesis.
    ///
    /// # Errors
    ///
    /// Returns an error when the list is not closed.
    fn parse_list(&mut self, span: Span) -> Result<Form, ScriptError> {
        let mut items = Vec::new();
        loop {
            let next = self
                .tokens
                .get(self.position)
                .ok_or_else(|| ScriptError::at(span, ScriptErrorKind::UnclosedList))?;

            // A closing parenthesis completes the current list.
            if next.kind == TokenKind::Right {
                self.position += 1;
                return Ok(Form::List { items, span });
            }
            items.push(Form::try_from(&mut *self)?);
        }
    }

    /// Parse every top-level form.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed list structure.
    fn parse_all(mut self) -> Result<Vec<Form>, ScriptError> {
        let mut forms = Vec::new();
        while self.position < self.tokens.len() {
            forms.push(Form::try_from(&mut self)?);
        }
        Ok(forms)
    }
}

impl TryFrom<&mut FormParser> for Form {
    type Error = ScriptError;

    fn try_from(parser: &mut FormParser) -> Result<Self, Self::Error> {
        let token = parser
            .tokens
            .get(parser.position)
            .cloned()
            .ok_or_else(|| ScriptError::at(Span::START, ScriptErrorKind::UnexpectedEnd))?;
        parser.position += 1;
        match token.kind {
            TokenKind::Atom(text) => Ok(Self::Atom(Atom {
                span: token.span,
                text,
            })),
            TokenKind::Left => parser.parse_list(token.span),
            TokenKind::Right => Err(ScriptError::at(
                token.span,
                ScriptErrorKind::UnexpectedClose,
            )),
        }
    }
}

/// Parse all list forms in one script.
///
/// # Errors
///
/// Returns a positioned lexical or list-structure error.
fn parse_forms(source: &str) -> Result<Vec<Form>, ScriptError> {
    source.parse::<FormParser>()?.parse_all()
}

// -----------------------------------------------------------------------------
// Reassembly: Describes one response or control action.
// -----------------------------------------------------------------------------

/// One response template or control action.
#[derive(Clone, Debug)]
pub(crate) enum Reassembly {
    /// Delegate to another keyword.
    Link {
        /// Target keyword.
        keyword: String,
    },
    /// Resume selection with the next keyword.
    NewKey,
    /// Rewrite the input and delegate to another keyword.
    Pre {
        /// Target keyword.
        link: String,
        /// Rewritten input template.
        words: Vec<AssemblyItem>,
    },
    /// Assemble a response from words and captures.
    Words {
        /// Response template.
        items: Vec<AssemblyItem>,
    },
}

impl Reassembly {
    /// Parse a reassembly under one decomposition's capture count.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed control forms or capture references.
    fn parse_with_pattern_len(form: &Form, pattern_len: usize) -> Result<Self, ScriptError> {
        // A two-item equality form delegates directly to another keyword.
        if let Some(keyword) = parse_link(form)? {
            return Ok(Self::Link { keyword });
        }
        let items = form.list(ScriptExpectation::Reassembly)?;

        // NEWKEY is a complete control-form reassembly.
        if matches!(items, [item] if item.atom_text() == Some("NEWKEY")) {
            return Ok(Self::NewKey);
        }

        // PRE rewrites the input before following its required link.
        if items.first().and_then(Form::atom_text) == Some("PRE") {
            // PRE contains its marker, replacement words, and one link.
            let [_, words_form, link_form] = items else {
                return Err(form.error(ScriptErrorKind::PreShape));
            };
            let words = parse_reassembly_items(
                words_form.list(ScriptExpectation::PreWordList)?,
                pattern_len,
            )?;

            // Resolve the mandatory target after validating replacement words.
            let link = parse_link(link_form)?
                .ok_or_else(|| link_form.error(ScriptErrorKind::MissingPreLink))?;
            return Ok(Self::Pre { link, words });
        }
        Ok(Self::Words {
            items: parse_reassembly_items(items, pattern_len)?,
        })
    }
}

// -----------------------------------------------------------------------------
// Decomposition: Couples a pattern to rotating reassemblies.
// -----------------------------------------------------------------------------

/// One keyword decomposition and its response cycle.
#[derive(Clone, Debug)]
pub(crate) struct Decomposition {
    /// Index of this decomposition's session-local response counter.
    pub(crate) counter_index: usize,
    /// Pattern matched against the normalized input.
    pub(crate) pattern: Vec<PatternItem>,
    /// Response and control templates in rotation order.
    pub(crate) reassemblies: Vec<Reassembly>,
}

impl TryFrom<&Form> for Decomposition {
    type Error = ScriptError;

    fn try_from(form: &Form) -> Result<Self, Self::Error> {
        let items = form.list(ScriptExpectation::DecompositionRule)?;
        let pattern_form = items
            .first()
            .ok_or_else(|| form.error(ScriptErrorKind::EmptyDecomposition))?;
        let pattern = parse_pattern(pattern_form)?;

        // Compile each response under the pattern's capture count.
        let reassemblies = items[1..]
            .iter()
            .map(|item| Reassembly::parse_with_pattern_len(item, pattern.len()))
            .collect::<Result<Vec<_>, _>>()?;

        // Every successful decomposition needs at least one action.
        if reassemblies.is_empty() {
            return Err(form.error(ScriptErrorKind::EmptyReassembly));
        }
        Ok(Self {
            counter_index: 0,
            pattern,
            reassemblies,
        })
    }
}

// -----------------------------------------------------------------------------
// MemoryDecomposition: Holds one hashed memory transformation.
// -----------------------------------------------------------------------------

/// One pattern and response template in the memory table.
#[derive(Clone, Debug)]
pub(crate) struct MemoryDecomposition {
    /// Pattern matched against the normalized input.
    pub(crate) pattern: Vec<PatternItem>,
    /// Response assembled for a successful match.
    pub(crate) reassembly: Vec<AssemblyItem>,
}

impl TryFrom<&Form> for MemoryDecomposition {
    type Error = ScriptError;

    fn try_from(form: &Form) -> Result<Self, Self::Error> {
        let items = form.list(ScriptExpectation::MemoryTransformation)?;
        let separator = items
            .iter()
            .position(|item| item.atom_text() == Some("="))
            .ok_or_else(|| form.error(ScriptErrorKind::MissingTransformationSeparator))?;
        Ok(Self {
            pattern: parse_pattern_items(&items[..separator])?,
            reassembly: parse_reassembly_items(&items[separator + 1..], separator)?,
        })
    }
}

// -----------------------------------------------------------------------------
// MemoryRule: Selects four transformations from a keyword hash.
// -----------------------------------------------------------------------------

/// Number of transformations selected by the historical two-bit hash.
const MEMORY_TRANSFORMATION_COUNT: usize = 4;

/// Script memory rule.
#[derive(Clone, Debug)]
pub(crate) struct MemoryRule {
    /// Hash-indexed transformations.
    pub(crate) decompositions: Vec<MemoryDecomposition>,
    /// Keyword that triggers memory generation.
    pub(crate) keyword: String,
}

impl TryFrom<&[Form]> for MemoryRule {
    type Error = ScriptError;

    fn try_from(items: &[Form]) -> Result<Self, Self::Error> {
        let marker = items
            .first()
            .ok_or_else(|| ScriptError::at(Span::START, ScriptErrorKind::EmptyMemoryRule))?;

        // The atom after MEMORY selects the keyword whose inputs are retained.
        let keyword = items
            .get(1)
            .ok_or_else(|| marker.error(ScriptErrorKind::MissingMemoryKeyword))?
            .atom(ScriptExpectation::MemoryKeyword)?
            .text
            .clone();

        // Preserve the hash-table order of all four transformations.
        let decompositions = items[2..]
            .iter()
            .map(MemoryDecomposition::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        // The two-bit hash must address every transformation exactly once.
        if decompositions.len() != MEMORY_TRANSFORMATION_COUNT {
            return Err(marker.error(ScriptErrorKind::TransformationCount));
        }
        Ok(Self {
            decompositions,
            keyword,
        })
    }
}

// -----------------------------------------------------------------------------
// KeywordRule: Parses and stores one keyword entry.
// -----------------------------------------------------------------------------

/// Mutable cursor and fields used while parsing a keyword rule.
struct KeywordRuleParser<'form> {
    /// Parsed decompositions.
    decompositions: Vec<Decomposition>,
    /// Optional rule-level link.
    fallback: Option<String>,
    /// Complete rule form after its outer parenthesis.
    items: &'form [Form],
    /// Canonical keyword.
    keyword: String,
    /// Stack ordering precedence.
    precedence: i64,
    /// Index of the next rule item.
    position: usize,
    /// Position of the outer rule form.
    span: Span,
    /// Optional input substitution.
    substitution: Option<String>,
    /// DLIST memberships.
    tags: Vec<String>,
}

impl<'form> KeywordRuleParser<'form> {
    /// Start parsing after the required keyword.
    ///
    /// # Errors
    ///
    /// Returns an error when the keyword is absent or not atomic.
    fn new(items: &'form [Form], span: Span) -> Result<Self, ScriptError> {
        let keyword = items
            .first()
            .ok_or_else(|| ScriptError::at(span, ScriptErrorKind::EmptyKeywordRule))?
            .atom(ScriptExpectation::Keyword)?
            .text
            .clone();
        Ok(Self {
            decompositions: Vec::new(),
            fallback: None,
            items,
            keyword,
            precedence: 0,
            position: 1,
            span,
            substitution: None,
            tags: Vec::new(),
        })
    }

    /// Parse an input substitution at the cursor.
    ///
    /// # Errors
    ///
    /// Returns an error when the substitution is misplaced or incomplete.
    fn parse_substitution(&mut self, marker: &Form) -> Result<(), ScriptError> {
        // Substitution is unique and must precede decompositions.
        if self.substitution.is_some() || !self.decompositions.is_empty() {
            return Err(marker.error(ScriptErrorKind::MisplacedSubstitution));
        }

        // Consume the replacement immediately after its equals marker.
        let replacement = self
            .items
            .get(self.position + 1)
            .ok_or_else(|| marker.error(ScriptErrorKind::MissingSubstitution))?;

        // Retain the validated atom as this rule's substitution.
        self.substitution = Some(
            replacement
                .atom(ScriptExpectation::Substitution)?
                .text
                .clone(),
        );

        // Advance beyond the marker and replacement.
        self.position += 2;
        Ok(())
    }

    /// Parse DLIST memberships at the cursor.
    ///
    /// # Errors
    ///
    /// Returns an error when the tag list is absent or malformed.
    fn parse_tags(&mut self, marker: &Form) -> Result<(), ScriptError> {
        let list = self
            .items
            .get(self.position + 1)
            .ok_or_else(|| marker.error(ScriptErrorKind::MissingDlist))?;
        self.tags = parse_tags(list)?;
        self.position += 2;
        Ok(())
    }

    /// Store a rule-level fallback and require it to be last.
    ///
    /// # Errors
    ///
    /// Returns an error for a duplicate or non-final link.
    fn parse_fallback(&mut self, form: &Form, target: String) -> Result<(), ScriptError> {
        // A rule has at most one fallback target.
        if self.fallback.replace(target).is_some() {
            return Err(form.error(ScriptErrorKind::DuplicateRuleLink));
        }
        self.position += 1;

        // Nothing may follow a rule-level fallback.
        if self.position != self.items.len() {
            return Err(self.items[self.position].error(ScriptErrorKind::LinkNotLast));
        }
        Ok(())
    }

    /// Parse one item at the cursor.
    ///
    /// # Errors
    ///
    /// Returns an error when the item violates keyword-rule grammar.
    fn parse_next(&mut self) -> Result<(), ScriptError> {
        let form = self
            .items
            .get(self.position)
            .cloned()
            .ok_or_else(|| ScriptError::at(self.span, ScriptErrorKind::MissingKeywordItem))?;

        // An equals marker introduces input substitution.
        if form.atom_text() == Some("=") {
            return self.parse_substitution(&form);
        }

        // DLIST introduces tag memberships.
        if form.atom_text() == Some("DLIST") {
            return self.parse_tags(&form);
        }

        // A signed decimal atom sets keyword precedence.
        if let Some(text) = form.atom_text()
            && let Ok(precedence) = text.parse::<i64>()
        {
            self.precedence = precedence;
            self.position += 1;
            return Ok(());
        }

        // A link at rule level becomes the final fallback.
        if let Some(target) = parse_link(&form)? {
            return self.parse_fallback(&form, target);
        }
        self.decompositions.push(Decomposition::try_from(&form)?);
        self.position += 1;
        Ok(())
    }

    /// Parse the remaining items and register DLIST membership.
    ///
    /// # Errors
    ///
    /// Returns an error when the rule is empty or an item is malformed.
    fn parse(
        mut self,
        tag_index: &mut HashMap<String, HashSet<String>>,
    ) -> Result<KeywordRule, ScriptError> {
        while self.position < self.items.len() {
            self.parse_next()?;
        }

        // A keyword must declare at least one useful property.
        if self.decompositions.is_empty()
            && self.fallback.is_none()
            && self.substitution.is_none()
            && self.tags.is_empty()
        {
            return Err(ScriptError::at(
                self.span,
                ScriptErrorKind::EmptyKeywordRule,
            ));
        }

        // Register memberships before moving the keyword into its rule.
        for tag in self.tags {
            tag_index
                .entry(tag)
                .or_default()
                .insert(self.keyword.clone());
        }

        // Commit the validated parser fields to an immutable rule.
        Ok(KeywordRule {
            decompositions: self.decompositions,
            fallback: self.fallback,
            keyword: self.keyword,
            precedence: self.precedence,
            substitution: self.substitution,
        })
    }
}

/// Parsed transformation rule for one keyword.
#[derive(Clone, Debug)]
pub(crate) struct KeywordRule {
    /// Pattern and response alternatives.
    pub(crate) decompositions: Vec<Decomposition>,
    /// Rule-level link used when no decomposition matches.
    pub(crate) fallback: Option<String>,
    /// Canonical keyword.
    pub(crate) keyword: String,
    /// Stack ordering precedence.
    pub(crate) precedence: i64,
    /// Word substituted before transformation.
    pub(crate) substitution: Option<String>,
}

impl KeywordRule {
    /// Parse one outer keyword-rule list.
    ///
    /// # Errors
    ///
    /// Returns an error when the rule grammar or contents are invalid.
    fn parse(
        items: &[Form],
        span: Span,
        tag_index: &mut HashMap<String, HashSet<String>>,
    ) -> Result<Self, ScriptError> {
        KeywordRuleParser::new(items, span)?.parse(tag_index)
    }

    /// Whether this keyword can transform an input.
    pub(crate) fn has_transformation(&self) -> bool {
        !self.decompositions.is_empty() || self.fallback.is_some()
    }
}

// -----------------------------------------------------------------------------
// Insert: Adds validated top-level rules to their indexes.
// -----------------------------------------------------------------------------

/// Insert the unique MEMORY rule.
///
/// # Errors
///
/// Returns an error for a duplicate or malformed memory rule.
fn insert_memory(memory: &mut Option<MemoryRule>, items: &[Form]) -> Result<(), ScriptError> {
    let rule = MemoryRule::try_from(items)?;

    // The script format admits exactly one memory table.
    if memory.replace(rule).is_some() {
        return Err(items[0].error(ScriptErrorKind::DuplicateMemoryRule));
    }
    Ok(())
}

/// Insert one unique keyword rule.
///
/// # Errors
///
/// Returns an error for duplicate keywords or malformed rules.
fn insert_keyword(
    rules: &mut HashMap<String, KeywordRule>,
    tag_index: &mut HashMap<String, HashSet<String>>,
    items: &[Form],
    span: Span,
) -> Result<(), ScriptError> {
    let rule = KeywordRule::parse(items, span, tag_index)?;
    let keyword = rule.keyword.clone();

    // Each canonical keyword owns one rule.
    if rules.insert(keyword, rule).is_some() {
        return Err(items[0].error(ScriptErrorKind::DuplicateKeywordRule));
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// ParseTopLevelRule: Recognizes one outer rule or the terminating empty list.
// -----------------------------------------------------------------------------

/// Parse and insert one top-level rule.
///
/// Returns `false` for the empty-list terminator.
///
/// # Errors
///
/// Returns an error for a malformed, duplicate, or invalid rule.
fn parse_top_level_rule(
    form: Form,
    memory: &mut Option<MemoryRule>,
    rules: &mut HashMap<String, KeywordRule>,
    tag_index: &mut HashMap<String, HashSet<String>>,
) -> Result<Option<()>, ScriptError> {
    // Only parenthesized forms are valid at top level.
    let Form::List { items, span } = form else {
        return Err(form.error(ScriptErrorKind::TopLevelList));
    };

    // The empty list ends the historical script stream.
    if items.is_empty() {
        return Ok(None);
    }
    let head = items[0].atom(ScriptExpectation::RuleKeyword)?;
    if head.text == "MEMORY" {
        insert_memory(memory, &items)?;
    } else {
        insert_keyword(rules, tag_index, &items, span)?;
    }
    Ok(Some(()))
}

// -----------------------------------------------------------------------------
// Parse: Decodes links, patterns, templates, tags, words, and the header.
// -----------------------------------------------------------------------------

/// Parse a two-item keyword link.
///
/// # Errors
///
/// Returns an error when a link target is not atomic.
fn parse_link(form: &Form) -> Result<Option<String>, ScriptError> {
    // Atoms cannot encode a keyword link.
    let Form::List { items, .. } = form else {
        return Ok(None);
    };

    // Link forms contain exactly an equals marker and target.
    let [marker, target] = items.as_slice() else {
        return Ok(None);
    };

    // Only an equals marker identifies a link form.
    if marker.atom_text() != Some("=") {
        return Ok(None);
    }
    Ok(Some(
        target.atom(ScriptExpectation::LinkKeyword)?.text.clone(),
    ))
}

/// Parse an alternatives or tag pattern group.
///
/// # Errors
///
/// Returns an error for a missing marker, values, or unknown marker.
fn parse_pattern_group(form: &Form) -> Result<PatternItem, ScriptError> {
    let group = form.list(ScriptExpectation::PatternGroup)?;
    let head = group
        .first()
        .ok_or_else(|| form.error(ScriptErrorKind::EmptyPatternGroup))?
        .atom(ScriptExpectation::PatternGroupMarker)?;

    // Split the leading marker from an optional attached member.
    let mut characters = head.text.chars();
    let marker = characters
        .next()
        .ok_or_else(|| head.error(ScriptErrorKind::EmptyPatternMarker))?;
    let attached = characters.as_str();

    // The marker may carry the first value, as in /FAMILY.
    let mut values = Vec::new();
    if !attached.is_empty() {
        values.push(attached.to_owned());
    }
    for value in &group[1..] {
        values.push(
            value
                .atom(ScriptExpectation::PatternGroupValue)?
                .text
                .clone(),
        );
    }

    // A marker without members cannot match input.
    if values.is_empty() {
        return Err(form.error(ScriptErrorKind::EmptyPatternGroup));
    }
    match marker {
        '*' => Ok(PatternItem::Alternatives(values)),
        '/' => Ok(PatternItem::Tags(values)),
        _ => Err(head.error(ScriptErrorKind::PatternMarker)),
    }
}

/// Parse one atom or grouped pattern item.
///
/// # Errors
///
/// Returns an error for malformed alternatives or tags.
fn parse_pattern_item(form: &Form) -> Result<PatternItem, ScriptError> {
    // Atoms represent counts, wildcards, or literal words.
    if let Some(value) = form.atom_text() {
        return Ok(match value.parse::<usize>() {
            Ok(0) => PatternItem::Variable,
            Ok(count) => PatternItem::Fixed(count),
            Err(_) => PatternItem::Word(value.to_owned()),
        });
    }
    parse_pattern_group(form)
}

/// Parse a sequence of decomposition pattern items.
///
/// # Errors
///
/// Returns an error for an empty pattern or malformed group.
fn parse_pattern_items(items: &[Form]) -> Result<Vec<PatternItem>, ScriptError> {
    // Every decomposition needs a pattern, including the wildcard pattern 0.
    if items.is_empty() {
        return Err(ScriptError::at(Span::START, ScriptErrorKind::EmptyPattern));
    }
    items.iter().map(parse_pattern_item).collect()
}

/// Parse one decomposition pattern.
///
/// # Errors
///
/// Returns an error for malformed pattern items.
fn parse_pattern(form: &Form) -> Result<Vec<PatternItem>, ScriptError> {
    parse_pattern_items(form.list(ScriptExpectation::DecompositionPattern)?)
}

/// Parse one response word or capture reference.
///
/// # Errors
///
/// Returns an error for a non-atom or out-of-range capture.
fn parse_reassembly_item(form: &Form, pattern_len: usize) -> Result<AssemblyItem, ScriptError> {
    let atom = form.atom(ScriptExpectation::ReassemblyWord)?;

    // Non-numeric atoms are literal response words.
    let Ok(index) = atom.text.parse::<usize>() else {
        return Ok(AssemblyItem::Word(atom.text.clone()));
    };

    // Capture numbers are one-based and refer to pattern items.
    if index == 0 || index > pattern_len {
        return Err(atom.error(ScriptErrorKind::CaptureOutsidePattern {
            index,
            maximum: pattern_len,
        }));
    }
    Ok(AssemblyItem::Capture(index - 1))
}

/// Parse response words and capture references.
///
/// # Errors
///
/// Returns an error for non-atoms or out-of-range captures.
fn parse_reassembly_items(
    items: &[Form],
    pattern_len: usize,
) -> Result<Vec<AssemblyItem>, ScriptError> {
    items
        .iter()
        .map(|item| parse_reassembly_item(item, pattern_len))
        .collect()
}

/// Parse DLIST tag names.
///
/// # Errors
///
/// Returns an error when the tag container or an item is malformed.
fn parse_tags(form: &Form) -> Result<Vec<String>, ScriptError> {
    let mut tags = Vec::new();
    for item in form.list(ScriptExpectation::DlistTagList)? {
        let tag = item
            .atom(ScriptExpectation::DlistTag)?
            .text
            .trim_start_matches('/');

        // A bare slash carries no membership name.
        if tag.is_empty() {
            continue;
        }
        tags.push(tag.to_owned());
    }
    Ok(tags)
}

/// Parse a list containing only literal words.
///
/// # Errors
///
/// Returns an error when the form is not a list or contains another list.
fn parse_words(form: &Form) -> Result<Vec<String>, ScriptError> {
    form.list(ScriptExpectation::WordList)?
        .iter()
        .map(|item| {
            item.atom(ScriptExpectation::Word)
                .map(|atom| atom.text.clone())
        })
        .collect()
}

/// Parse the greeting and required START marker.
///
/// # Errors
///
/// Returns an error when either header form is absent or malformed.
fn parse_header(forms: &mut impl Iterator<Item = Form>) -> Result<String, ScriptError> {
    // The first list is the greeting emitted during startup.
    let greeting = parse_words(
        &forms
            .next()
            .ok_or_else(|| ScriptError::at(Span::START, ScriptErrorKind::MissingGreeting))?,
    )?
    .join(" ");

    // The second form enters rule-reading mode.
    let start = forms
        .next()
        .ok_or_else(|| ScriptError::at(Span::START, ScriptErrorKind::MissingStart))?;

    // START must be a standalone atom immediately after the greeting.
    if start.atom(ScriptExpectation::Start)?.text != "START" {
        return Err(start.error(ScriptErrorKind::MissingStart));
    }
    Ok(greeting)
}

// -----------------------------------------------------------------------------
// Script: Validates the header, memory rule, keywords, and tag index.
// -----------------------------------------------------------------------------

/// Validated 1966 DOCTOR script.
#[derive(Debug)]
pub(crate) struct Script {
    /// Number of independent response counters needed by each session.
    pub(crate) counter_count: usize,
    /// Opening response.
    pub(crate) greeting: String,
    /// Single hashed memory rule.
    pub(crate) memory: MemoryRule,
    /// Keyword rules indexed by canonical spelling.
    pub(crate) rules: HashMap<String, KeywordRule>,
    /// DLIST members indexed by tag.
    pub(crate) tags: HashMap<String, HashSet<String>>,
}

impl FromStr for Script {
    type Err = ScriptError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let mut forms = parse_forms(source)?.into_iter();
        let greeting = parse_header(&mut forms)?;
        let mut memory = None;
        let mut rules = HashMap::new();
        let mut tags = HashMap::new();

        // Read top-level rules until the historical empty-list terminator.
        for form in forms {
            if parse_top_level_rule(form, &mut memory, &mut rules, &mut tags)?.is_none() {
                break;
            }
        }

        let memory = memory
            .ok_or_else(|| ScriptError::at(Span::START, ScriptErrorKind::MissingMemoryRule))?;

        // NONE supplies the mandatory default transformation.
        if !rules.contains_key("NONE") {
            return Err(ScriptError::at(
                Span::START,
                ScriptErrorKind::MissingNoneRule,
            ));
        }

        // Give each validated decomposition one session-local counter slot.
        let mut counter_count = 0;
        for decomposition in rules.values_mut().flat_map(|rule| &mut rule.decompositions) {
            decomposition.counter_index = counter_count;
            counter_count += 1;
        }

        // Freeze the rule definitions together with their required counter count.
        Ok(Self {
            counter_count,
            greeting,
            memory,
            rules,
            tags,
        })
    }
}
