//! Classic ELIZA script contracts.
//!
//! The bundled DOCTOR script is data, not Rust control flow. This file owns the
//! full path from that source text to one deterministic conversation turn:
//!
//! ```text
//! fixtures/eliza/doctor.script
//!        |
//!        v
//! Parser -> Script { substitutions, tags, transforms, memory }
//!        |
//!        v
//! ElizaSession::respond(input)
//!        |
//!        v
//! ElizaTurn { normalized_input, output, matched_keyword }
//! ```
//!
//! Provider adapters pass user text into `ElizaSession`. They do not implement
//! normalization, keyword ranking, decomposition matching, memory, links, or
//! reassembly cycling.
//!
//! The source script is read in three layers. The lexer/parser preserves only
//! S-expression shape and spans; rule lowering gives those lists ELIZA meaning;
//! the session runtime applies the lowered rules against normalized user input.

use std::borrow::Borrow;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fmt;
use std::num::NonZeroUsize;
use std::str::FromStr;
use std::sync::OnceLock;

use crate::errors::AppError;

const DOCTOR_SCRIPT: &str = include_str!("../fixtures/eliza/doctor.script");

// -----------------------------------------------------------------------------
// Script diagnostics: every token keeps a line/column span so parser and rule
// lowering failures point back to the bundled script text.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct Span {
    line: usize,
    column: usize,
}

impl Span {
    const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }

    const fn expected(self, expected: &'static str) -> AppError {
        AppError::ScriptExpected {
            expected,
            line: self.line,
            column: self.column,
        }
    }
}

// -----------------------------------------------------------------------------
// Text normalization: external prose becomes the uppercase word stream expected
// by the source script, and assembled words become readable provider text again.
// -----------------------------------------------------------------------------

fn tokenize_input(input: &str) -> Vec<String> {
    let normalized = input
        .replace(['\u{2018}', '\u{2019}'], "'")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '\'' {
                character.to_ascii_uppercase()
            } else {
                ' '
            }
        })
        .collect::<String>();

    normalized
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect()
}

fn format_words(words: &[String]) -> String {
    let mut text = words.join(" ");

    for punctuation in [".", ",", "?", "!", ":", ";"] {
        text = text.replace(&format!(" {punctuation}"), punctuation);
    }

    text = text.replace("( ", "(").replace(" )", ")");
    text.trim().to_owned()
}

// -----------------------------------------------------------------------------
// Source language parser: recognizes the compact S-expression syntax used by
// DOCTOR before any ELIZA-specific rule meaning is applied.
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Eq, PartialEq)]
enum TokenKind {
    OpenParen,
    CloseParen,
    Atom(String),
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct Token {
    /// Raw script token before ELIZA-specific meaning is attached.
    kind: TokenKind,
    /// Start position used for diagnostics after later lowering failures.
    span: Span,
}

#[derive(Debug, Clone)]
struct LexedScript {
    /// Complete token stream without comments or whitespace.
    tokens: Vec<Token>,
    /// Final position used when EOF appears inside an open list.
    eof: Span,
}

struct Lexer<'a> {
    input: &'a str,
    offset: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    fn lex(input: &'a str) -> LexedScript {
        let mut lexer = Self::new(input);
        let mut tokens = Vec::new();

        // --- Keep lexing mechanical: comments and whitespace disappear, but
        // token spans survive for parser and lowering diagnostics.
        while let Some(token) = lexer.next_token() {
            tokens.push(token);
        }

        LexedScript {
            tokens,
            eof: Span::new(lexer.line, lexer.column),
        }
    }

    fn new(input: &'a str) -> Self {
        Self {
            input,
            offset: 0,
            line: 1,
            column: 1,
        }
    }

    fn next_token(&mut self) -> Option<Token> {
        self.skip_whitespace_and_comments();
        let span = self.current_span();
        let character = self.peek()?;

        match character {
            '(' => {
                self.bump();
                Some(Token {
                    kind: TokenKind::OpenParen,
                    span,
                })
            }
            ')' => {
                self.bump();
                Some(Token {
                    kind: TokenKind::CloseParen,
                    span,
                })
            }
            _ => Some(self.atom()),
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(';') => {
                    // --- Script comments run to the end of the current line.
                    while let Some(character) = self.peek() {
                        self.bump();
                        if character == '\n' {
                            break;
                        }
                    }
                }
                Some(character) if character.is_whitespace() => {
                    self.bump();
                }
                _ => break,
            }
        }
    }

    fn atom(&mut self) -> Token {
        let span = self.current_span();
        let mut atom = String::new();

        while let Some(character) = self.peek() {
            if character.is_whitespace() || character == '(' || character == ')' || character == ';'
            {
                break;
            }
            atom.push(character);
            self.bump();
        }

        Token {
            kind: TokenKind::Atom(atom),
            span,
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.offset..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.offset += character.len_utf8();
        if character == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(character)
    }

    fn current_span(&self) -> Span {
        Span::new(self.line, self.column)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum SexpKind {
    Atom(String),
    List(Vec<Sexp>),
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct Sexp {
    /// Recursive syntax node produced before script forms are interpreted.
    kind: SexpKind,
    /// Span of the opening token or atom that introduced this node.
    span: Span,
}

impl Sexp {
    fn atom(&self) -> Option<&str> {
        match &self.kind {
            SexpKind::Atom(atom) => Some(atom),
            SexpKind::List(_) => None,
        }
    }

    fn list(&self) -> Option<SexpList<'_>> {
        match &self.kind {
            SexpKind::Atom(_) => None,
            SexpKind::List(items) => Some(SexpList::new(items, self.span)),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SexpList<'a> {
    /// Borrowed child nodes; list helpers never allocate unless lowering does.
    items: &'a [Sexp],
    /// Parent list span used when a required child is missing.
    span: Span,
}

impl<'a> SexpList<'a> {
    fn new(items: &'a [Sexp], span: Span) -> Self {
        Self { items, span }
    }

    fn as_slice(self) -> &'a [Sexp] {
        self.items
    }

    /// Read one S-expression as a list.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ScriptExpected`] when the S-expression is an atom.
    fn from_sexp(sexp: &'a Sexp, expected: &'static str) -> Result<Self, AppError> {
        sexp.list().ok_or(sexp.span.expected(expected))
    }

    fn atom_at(self, index: usize) -> Option<&'a str> {
        self.items.get(index).and_then(Sexp::atom)
    }

    /// Return the list item at `index`.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ScriptExpected`] when the requested item is missing.
    fn expect(self, index: usize, expected: &'static str) -> Result<&'a Sexp, AppError> {
        self.items.get(index).ok_or(self.span.expected(expected))
    }

    /// Return the list item at `index` as an atom.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ScriptExpected`] when the item is missing or is a
    /// nested list.
    fn expect_atom(self, index: usize, expected: &'static str) -> Result<&'a str, AppError> {
        let item = self.expect(index, expected)?;
        item.atom().ok_or(item.span.expected(expected))
    }

    fn tail(self, index: usize) -> Self {
        Self::new(self.items.get(index..).unwrap_or_default(), self.span)
    }

    fn get(self, index: usize) -> Option<&'a Sexp> {
        self.items.get(index)
    }

    fn iter(self) -> impl Iterator<Item = &'a Sexp> + 'a {
        self.items.iter()
    }

    fn atoms(self) -> impl Iterator<Item = &'a str> + 'a {
        self.items.iter().filter_map(Sexp::atom)
    }

    fn split_once_atom(self, atom: &str) -> Option<(Self, Self)> {
        let separator = self
            .items
            .iter()
            .position(|item| item.atom() == Some(atom))?;
        Some((
            Self::new(&self.items[..separator], self.span),
            Self::new(&self.items[separator + 1..], self.span),
        ))
    }
}

struct Parser {
    /// Token stream produced by the lexer.
    tokens: Vec<Token>,
    /// Current parser position into `tokens`.
    cursor: usize,
    /// Span returned when a list reaches EOF before a close paren.
    eof: Span,
}

impl Parser {
    /// Parse a full script source into S-expressions.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] when parentheses are unbalanced or the parser sees
    /// a close paren without a matching open paren.
    fn parse(input: &str) -> Result<Vec<Sexp>, AppError> {
        let lexed = Lexer::lex(input);
        Self {
            tokens: lexed.tokens,
            cursor: 0,
            eof: lexed.eof,
        }
        .parse_all()
    }

    /// Parse every token from the lexer.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] from the first malformed S-expression.
    fn parse_all(mut self) -> Result<Vec<Sexp>, AppError> {
        let mut sexps = Vec::new();

        // --- Top-level forms are independent until script lowering decides
        // which forms are greeting text, memory, or keyword transforms.
        while !self.is_eof() {
            sexps.push(self.parse_one()?);
        }

        Ok(sexps)
    }

    /// Parse one atom or list at the current cursor.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] when the cursor is at EOF or an unexpected close
    /// paren.
    fn parse_one(&mut self) -> Result<Sexp, AppError> {
        let token = self.peek().ok_or(AppError::ScriptUnexpectedEnd {
            line: self.eof.line,
            column: self.eof.column,
        })?;

        match &token.kind {
            TokenKind::OpenParen => self.parse_list(),
            TokenKind::CloseParen => Err(AppError::ScriptUnexpectedClose {
                line: token.span.line,
                column: token.span.column,
            }),
            TokenKind::Atom(atom) => {
                let atom = atom.clone();
                let span = token.span;
                self.cursor += 1;
                Ok(Sexp {
                    kind: SexpKind::Atom(atom),
                    span,
                })
            }
        }
    }

    /// Parse one list starting at an open paren.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ScriptUnexpectedEnd`] when EOF appears before the
    /// closing paren.
    fn parse_list(&mut self) -> Result<Sexp, AppError> {
        let open = self.advance().ok_or(AppError::ScriptUnexpectedEnd {
            line: self.eof.line,
            column: self.eof.column,
        })?;
        let mut items = Vec::new();

        loop {
            match self.peek() {
                Some(Token {
                    kind: TokenKind::CloseParen,
                    ..
                }) => {
                    self.cursor += 1;
                    return Ok(Sexp {
                        kind: SexpKind::List(items),
                        span: open.span,
                    });
                }
                Some(_) => items.push(self.parse_one()?),
                None => {
                    return Err(AppError::ScriptUnexpectedEnd {
                        line: self.eof.line,
                        column: self.eof.column,
                    });
                }
            }
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.cursor)
    }

    fn advance(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.cursor).cloned();
        if token.is_some() {
            self.cursor += 1;
        }
        token
    }

    fn is_eof(&self) -> bool {
        self.cursor >= self.tokens.len()
    }
}

#[derive(Debug, Clone)]
struct AnalyzedInput {
    /// User words before source-script substitutions.
    original: Vec<String>,
    /// User words after substitutions such as `I` -> `YOU`.
    canonical: Vec<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct Keyword(String);

impl Keyword {
    fn as_str(&self) -> &str {
        &self.0
    }

    fn from_atom(atom: &str) -> Self {
        Self(atom.to_owned())
    }

    /// Read a keyword atom from a list position.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ScriptExpected`] when the item is missing or is not
    /// an atom.
    fn from_list_atom(
        list: SexpList<'_>,
        index: usize,
        expected_name: &'static str,
    ) -> Result<Self, AppError> {
        list.expect_atom(index, expected_name).map(Self::from_atom)
    }
}

impl Borrow<str> for Keyword {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for Keyword {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct TagName(String);

impl TagName {
    fn from_list(items: SexpList<'_>) -> Vec<Self> {
        items
            .atoms()
            .filter_map(|atom| {
                let tag = atom.trim_start_matches('/');
                (!tag.is_empty()).then(|| Self(tag.to_owned()))
            })
            .collect()
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
struct LinkTarget(Keyword);

impl LinkTarget {
    fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn from_sexp(sexp: &Sexp) -> Option<Self> {
        if let Some(atom) = sexp.atom() {
            return atom
                .strip_prefix('=')
                .map(|target| Self(Keyword::from_atom(target)));
        }

        match sexp.list()?.as_slice() {
            [one] => one.atom().and_then(|atom| {
                atom.strip_prefix('=')
                    .map(|target| Self(Keyword::from_atom(target)))
            }),
            [eq, target] if eq.atom() == Some("=") => {
                target.atom().map(|target| Self(Keyword::from_atom(target)))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Default)]
struct Precedence(i32);

impl FromStr for Precedence {
    type Err = std::num::ParseIntError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse::<i32>().map(Self)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct CaptureIndex(NonZeroUsize);

#[derive(Debug, Clone)]
enum ReassemblyItem {
    /// Literal output word from the selected reassembly rule.
    Word(String),
    /// One-based capture slot from the matched decomposition pattern.
    Capture(CaptureIndex),
}

fn render_reassembly_words(items: &[ReassemblyItem], captures: &[Vec<String>]) -> Vec<String> {
    let mut words = Vec::new();

    for item in items {
        match item {
            ReassemblyItem::Word(word) => words.push(word.clone()),
            ReassemblyItem::Capture(capture) => {
                if let Some(captured) = captures.get(capture.0.get() - 1) {
                    words.extend(captured.iter().cloned());
                }
            }
        }
    }

    words
}

fn render_reassembly(items: &[ReassemblyItem], captures: &[Vec<String>]) -> String {
    format_words(&render_reassembly_words(items, captures))
}

#[derive(Debug, Clone)]
enum PatternItem {
    /// `0` in the source script: match any number of words.
    Wildcard,
    /// Literal keyword match.
    Word(Keyword),
    /// Match any word that belongs to one of the named DLIST tags.
    Tag(Vec<TagName>),
    /// Match one of several literal alternatives.
    Alternatives(Vec<Keyword>),
}

#[derive(Debug, Clone)]
enum Reassembly {
    /// Render literal words and capture references.
    Words(Vec<ReassemblyItem>),
    /// Defer response selection to another keyword.
    Link(LinkTarget),
    /// Abandon this keyword and continue ranking.
    NewKey,
    /// Rewrite the input phrase, then evaluate another keyword.
    Pre {
        words: Vec<ReassemblyItem>,
        link: LinkTarget,
    },
}

#[derive(Debug)]
struct DecompositionRule {
    /// Pattern matched against canonicalized user input.
    pattern: Vec<PatternItem>,
    /// Rotating responses used when this pattern matches.
    reassemblies: Vec<Reassembly>,
}

#[derive(Debug)]
struct MemoryDecomposition {
    /// Pattern used only when the memory keyword appears in original input.
    pattern: Vec<PatternItem>,
    /// Response stored for later no-keyword fallback.
    reassembly: Vec<ReassemblyItem>,
}

#[derive(Debug)]
struct MemoryRule {
    /// Original-input keyword that activates memory storage.
    keyword: Keyword,
    /// Candidate memory patterns.
    decompositions: Vec<MemoryDecomposition>,
}

#[derive(Debug)]
struct TransformRule {
    /// Higher precedence keywords are tried before lower precedence keywords.
    precedence: Precedence,
    /// Optional fallback keyword when no local decomposition matches.
    link: Option<LinkTarget>,
    /// Pattern/reassembly rules owned by this keyword.
    decompositions: Vec<DecompositionRule>,
}

// -----------------------------------------------------------------------------
// Rule lowering: converts generic S-expression lists into the stable rule
// vocabulary used by the session engine.
// -----------------------------------------------------------------------------

struct KeywordRule {
    keyword: Keyword,
    substitution: Option<Keyword>,
    tags: Vec<TagName>,
    precedence: Precedence,
    link: Option<LinkTarget>,
    decompositions: Vec<DecompositionRule>,
}

impl TryFrom<SexpList<'_>> for KeywordRule {
    type Error = AppError;

    fn try_from(items: SexpList<'_>) -> Result<Self, Self::Error> {
        let keyword = Keyword::from_list_atom(items, 0, "keyword")?;
        let mut cursor = 1;

        // --- Optional `= WORD` substitution aliases one input word to another.
        let substitution = if items.atom_at(cursor) == Some("=") {
            cursor += 1;
            let substitution = Some(Keyword::from_list_atom(items, cursor, "substitution")?);
            cursor += 1;
            substitution
        } else {
            None
        };

        // --- Optional numeric precedence controls ranking when multiple
        // keywords occur in the same input.
        let mut precedence = Precedence::default();
        if let Some(atom) = items.atom_at(cursor)
            && let Ok(value) = atom.parse::<Precedence>()
        {
            precedence = value;
            cursor += 1;
        }

        // --- Optional DLIST attaches semantic tags to the keyword and, when a
        // substitution exists, to the replacement keyword as well.
        let tags = if items.atom_at(cursor) == Some("DLIST") {
            cursor += 1;
            let tag_sexp = items.expect(cursor, "DLIST tag list")?;
            let tag_items = SexpList::from_sexp(tag_sexp, "DLIST tag list")?;
            let tags = TagName::from_list(tag_items);
            cursor += 1;
            tags
        } else {
            Vec::new()
        };

        // --- Optional link can replace local decompositions or act as fallback.
        let link = items.get(cursor).and_then(LinkTarget::from_sexp);
        if link.is_some() {
            cursor += 1;
        }

        // --- Everything left is a decomposition rule in source order.
        let decompositions = items
            .tail(cursor)
            .iter()
            .map(DecompositionRule::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            keyword,
            substitution,
            tags,
            precedence,
            link,
            decompositions,
        })
    }
}

impl TryFrom<SexpList<'_>> for MemoryRule {
    type Error = AppError;

    fn try_from(items: SexpList<'_>) -> Result<Self, Self::Error> {
        let keyword = Keyword::from_list_atom(items, 1, "memory keyword")?;
        let decompositions = items
            .tail(2)
            .iter()
            .map(MemoryDecomposition::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            keyword,
            decompositions,
        })
    }
}

enum RuleForm {
    Memory(MemoryRule),
    Keyword(KeywordRule),
}

impl TryFrom<SexpList<'_>> for RuleForm {
    type Error = AppError;

    fn try_from(items: SexpList<'_>) -> Result<Self, Self::Error> {
        let first = Keyword::from_list_atom(items, 0, "keyword")?;
        if first.as_str() == "MEMORY" {
            MemoryRule::try_from(items).map(Self::Memory)
        } else {
            KeywordRule::try_from(items).map(Self::Keyword)
        }
    }
}

impl TryFrom<&Sexp> for MemoryDecomposition {
    type Error = AppError;

    fn try_from(sexp: &Sexp) -> Result<Self, Self::Error> {
        let parts = SexpList::from_sexp(sexp, "memory decomposition")?;
        // --- Memory rules use `pattern = reassembly` instead of nested
        // reassembly lists, so split once on the separator atom.
        let (pattern_items, reassembly_sexps) = parts
            .split_once_atom("=")
            .ok_or(sexp.span.expected("memory reassembly separator"))?;
        let pattern = pattern_items
            .iter()
            .map(PatternItem::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let reassembly = ReassemblyItem::from_list(reassembly_sexps)?;

        Ok(Self {
            pattern,
            reassembly,
        })
    }
}

impl TryFrom<&Sexp> for DecompositionRule {
    type Error = AppError;

    fn try_from(sexp: &Sexp) -> Result<Self, Self::Error> {
        let items = SexpList::from_sexp(sexp, "decomposition rule")?;
        let pattern_sexp = items.expect(0, "decomposition pattern")?;
        let pattern_items = SexpList::from_sexp(pattern_sexp, "decomposition pattern list")?;
        let pattern = pattern_items
            .iter()
            .map(PatternItem::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let reassemblies = items
            .tail(1)
            .iter()
            .map(Reassembly::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            pattern,
            reassemblies,
        })
    }
}

impl TryFrom<&Sexp> for PatternItem {
    type Error = AppError;

    fn try_from(sexp: &Sexp) -> Result<Self, Self::Error> {
        if sexp.atom() == Some("0") {
            return Ok(Self::Wildcard);
        }

        if let Some(atom) = sexp.atom() {
            return Ok(Self::Word(Keyword::from_atom(atom)));
        }

        let items = SexpList::from_sexp(sexp, "pattern item")?;
        let head = items.expect_atom(0, "pattern list head")?;

        // --- `(* A B)` and `(*A B)` both mean one-of literal alternatives.
        if head == "*" || head.starts_with('*') {
            let mut alternatives = Vec::new();
            if let Some(stripped) = head.strip_prefix('*')
                && !stripped.is_empty()
            {
                alternatives.push(Keyword::from_atom(stripped));
            }
            alternatives.extend(items.tail(1).atoms().map(Keyword::from_atom));
            Ok(Self::Alternatives(alternatives))
        // --- `(/ FAMILY)` and `(/FAMILY)` both mean a tag lookup.
        } else if head == "/" || head.starts_with('/') {
            Ok(Self::Tag(TagName::from_list(items)))
        } else {
            Err(sexp.span.expected("pattern tag or alternatives list"))
        }
    }
}

impl TryFrom<&Sexp> for Reassembly {
    type Error = AppError;

    fn try_from(sexp: &Sexp) -> Result<Self, Self::Error> {
        if let Some(link) = LinkTarget::from_sexp(sexp) {
            return Ok(Self::Link(link));
        }

        let items = SexpList::from_sexp(sexp, "reassembly list")?;

        if items.atom_at(0) == Some("NEWKEY") {
            return Ok(Self::NewKey);
        }

        // --- PRE performs a local rewrite before jumping through a link.
        if items.atom_at(0) == Some("PRE") {
            let words = SexpList::from_sexp(
                items.expect(1, "PRE reassembly words")?,
                "PRE reassembly words",
            )?;
            let link = items
                .get(2)
                .and_then(LinkTarget::from_sexp)
                .ok_or(sexp.span.expected("PRE target link"))?;
            return Ok(Self::Pre {
                words: ReassemblyItem::from_list(words)?,
                link,
            });
        }

        Ok(Self::Words(ReassemblyItem::from_list(items)?))
    }
}

impl TryFrom<&Sexp> for ReassemblyItem {
    type Error = AppError;

    fn try_from(item: &Sexp) -> Result<Self, Self::Error> {
        let atom = item.atom().ok_or(item.span.expected("reassembly atom"))?;

        if let Ok(value) = atom.parse::<usize>() {
            let capture = NonZeroUsize::new(value)
                .map(CaptureIndex)
                .ok_or(item.span.expected("non-zero capture index"))?;
            return Ok(Self::Capture(capture));
        }

        Ok(Self::Word(atom.to_owned()))
    }
}

impl ReassemblyItem {
    /// Convert a reassembly S-expression list into typed reassembly items.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] when any item is not an atom or references capture
    /// index zero.
    fn from_list(items: SexpList<'_>) -> Result<Vec<Self>, AppError> {
        items.iter().map(Self::try_from).collect()
    }
}

// -----------------------------------------------------------------------------
// Script model: typed representation of substitutions, tags, transforms, and
// memory after the S-expression forms have been accepted.
// -----------------------------------------------------------------------------

/// Parsed ELIZA script ready to drive one or more sessions.
///
/// ```
/// use eliza::eliza::{ElizaSession, Script};
///
/// let script: Script = "(HELLO)\nSTART\n(TEST\n  ((0 TEST 0)\n    (OK 3)))"
///     .parse()
///     .unwrap();
/// let mut session = ElizaSession::new(&script);
///
/// assert_eq!(session.respond("please test now").output, "OK NOW");
/// ```
#[derive(Debug)]
pub struct Script {
    substitutions: HashMap<Keyword, Keyword>,
    tags: HashMap<Keyword, BTreeSet<TagName>>,
    transforms: HashMap<Keyword, TransformRule>,
    memory: Option<MemoryRule>,
}

impl Script {
    fn analyze(&self, input: &str) -> AnalyzedInput {
        let original = tokenize_input(input);
        // --- Substitutions are applied after tokenization so matching uses the
        // script's canonical second-person vocabulary.
        let canonical = original
            .iter()
            .map(|word| {
                self.substitutions
                    .get(word.as_str())
                    .map_or_else(|| word.clone(), Keyword::to_string)
            })
            .collect();

        AnalyzedInput {
            original,
            canonical,
        }
    }

    fn direct_tokens(input: &str) -> AnalyzedInput {
        let canonical = tokenize_input(input);
        AnalyzedInput {
            original: canonical.clone(),
            canonical,
        }
    }

    fn apply_keyword_rule(&mut self, rule: KeywordRule) {
        // --- Substitutions are kept separately because input normalization uses
        // them before keyword ranking.
        if let Some(replacement) = rule.substitution.clone() {
            self.substitutions
                .insert(rule.keyword.clone(), replacement.clone());
            let replacement_tags = self
                .tags
                .get(replacement.as_str())
                .cloned()
                .unwrap_or_default();
            self.tags
                .entry(rule.keyword.clone())
                .or_default()
                .extend(replacement_tags);
        }

        // --- Tags travel with both source and replacement keywords so tagged
        // patterns continue to work after substitution.
        if !rule.tags.is_empty() {
            for word in [Some(rule.keyword.clone()), rule.substitution.clone()]
                .into_iter()
                .flatten()
            {
                self.tags
                    .entry(word)
                    .or_default()
                    .extend(rule.tags.iter().cloned());
            }
        }

        // --- A keyword without link or decompositions is only a substitution or
        // tag declaration, not a transform candidate.
        if rule.link.is_some() || !rule.decompositions.is_empty() {
            self.transforms.insert(
                rule.keyword.clone(),
                TransformRule {
                    precedence: rule.precedence,
                    link: rule.link,
                    decompositions: rule.decompositions,
                },
            );
        }
    }
}

impl FromStr for Script {
    type Err = AppError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let sexps = Parser::parse(input)?;
        let mut script = Script {
            substitutions: HashMap::new(),
            tags: HashMap::new(),
            transforms: HashMap::new(),
            memory: None,
        };
        let mut saw_greeting = false;
        let mut started = false;

        for sexp in sexps {
            // --- Forms before START are greeting/banner material in the source
            // script; they prove the script has the expected outer shape.
            if sexp.atom() == Some("START") {
                started = true;
                continue;
            }

            let Some(items) = sexp.list() else {
                continue;
            };

            match items.as_slice() {
                // --- An empty list terminates the original script table.
                [] => break,
                _ if !started => {
                    saw_greeting |= items.atoms().next().is_some();
                }
                // --- After START every list must lower to memory or keyword
                // behavior.
                _ => match RuleForm::try_from(items)? {
                    RuleForm::Memory(memory) => script.memory = Some(memory),
                    RuleForm::Keyword(rule) => script.apply_keyword_rule(rule),
                },
            }
        }

        if !saw_greeting {
            return Err(AppError::ScriptMissingGreeting { line: 1, column: 1 });
        }

        Ok(script)
    }
}

// -----------------------------------------------------------------------------
// Runtime engine: session state owns rule rotation and memory while the script
// remains immutable and shareable across requests.
// -----------------------------------------------------------------------------

const MAX_LINK_DEPTH: usize = 12;

/// Result of one ELIZA response operation.
#[derive(Debug, Clone)]
pub struct ElizaTurn {
    /// Canonical input after source-script substitutions were applied.
    pub normalized_input: String,
    /// Provider-ready response text produced by reassembly.
    pub output: String,
    /// Keyword whose transform produced the output, when one matched directly.
    pub matched_keyword: Option<String>,
}

#[derive(Debug, Default)]
struct SessionState {
    cursors: HashMap<(Keyword, usize), usize>,
    memory_queue: VecDeque<String>,
}

enum KeywordResult {
    Text(String),
    NewKey,
}

// -----------------------------------------------------------------------------
// Pattern matching: recursive decomposition matcher that records captures in the
// exact order reassembly indexes refer to them.
// -----------------------------------------------------------------------------

fn item_matches(
    item: &PatternItem,
    word: &str,
    tags: &HashMap<Keyword, BTreeSet<TagName>>,
) -> bool {
    match item {
        PatternItem::Wildcard => true,
        PatternItem::Word(expected) => expected.as_str() == word,
        PatternItem::Alternatives(alternatives) => {
            alternatives.iter().any(|option| option.as_str() == word)
        }
        PatternItem::Tag(expected_tags) => tags
            .get(word)
            .is_some_and(|word_tags| expected_tags.iter().any(|tag| word_tags.contains(tag))),
    }
}

fn match_pattern(
    pattern: &[PatternItem],
    input: &[String],
    tags: &HashMap<Keyword, BTreeSet<TagName>>,
) -> Option<Vec<Vec<String>>> {
    fn rec(
        pattern: &[PatternItem],
        input: &[String],
        tags: &HashMap<Keyword, BTreeSet<TagName>>,
        captures: &mut Vec<Vec<String>>,
    ) -> bool {
        if pattern.is_empty() {
            return input.is_empty();
        }

        match &pattern[0] {
            PatternItem::Wildcard => {
                // --- Wildcards are greedy by search, not by regex syntax:
                // each possible width is tried until the suffix matches.
                for width in 0..=input.len() {
                    captures.push(input[..width].to_vec());
                    if rec(&pattern[1..], &input[width..], tags, captures) {
                        return true;
                    }
                    captures.pop();
                }
                false
            }
            item => {
                // --- Non-wildcard items consume exactly one word and still
                // become capture slots for reassembly indexes.
                let Some(word) = input.first() else {
                    return false;
                };
                if !item_matches(item, word, tags) {
                    return false;
                }
                captures.push(vec![word.clone()]);
                if rec(&pattern[1..], &input[1..], tags, captures) {
                    true
                } else {
                    captures.pop();
                    false
                }
            }
        }
    }

    let mut captures = Vec::new();
    if rec(pattern, input, tags, &mut captures) {
        Some(captures)
    } else {
        None
    }
}

/// Stateful ELIZA conversation over one parsed script.
///
/// ```
/// use eliza::eliza::{ElizaSession, doctor_script};
///
/// let mut session = ElizaSession::new(doctor_script());
/// let turn = session.respond("I am sad");
///
/// assert_eq!(turn.normalized_input, "YOU ARE SAD");
/// assert_eq!(turn.output, "I AM SORRY TO HEAR YOU ARE SAD");
/// ```
#[derive(Debug)]
pub struct ElizaSession<'script> {
    script: &'script Script,
    state: SessionState,
}

impl<'script> ElizaSession<'script> {
    fn ranked_keywords(&self, analyzed: &AnalyzedInput) -> Vec<Keyword> {
        let mut candidates = Vec::new();

        // --- Rank original words, not substituted words, matching the classic
        // behavior where substitutions normalize captures but not trigger order.
        for (position, word) in analyzed.original.iter().enumerate() {
            if let Some(rule) = self.script.transforms.get(word.as_str())
                && !candidates
                    .iter()
                    .any(|(_, _, existing): &(Precedence, usize, Keyword)| {
                        existing.as_str() == word
                    })
            {
                candidates.push((rule.precedence, position, Keyword::from_atom(word)));
            }
        }

        candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        candidates
            .into_iter()
            .map(|(_, _, keyword)| keyword)
            .collect()
    }

    fn try_keyword(
        &mut self,
        keyword: &str,
        analyzed: &AnalyzedInput,
        depth: usize,
    ) -> KeywordResult {
        if depth > MAX_LINK_DEPTH {
            return KeywordResult::NewKey;
        }

        // --- Missing transform means this keyword cannot produce text.
        let Some(rule) = self.script.transforms.get(keyword) else {
            return KeywordResult::NewKey;
        };

        // --- Link-only keywords immediately delegate to their target.
        if rule.decompositions.is_empty() {
            if let Some(link) = &rule.link {
                return self.try_keyword(link.as_str(), analyzed, depth + 1);
            }
            return KeywordResult::NewKey;
        }

        for (decomposition_index, decomposition) in rule.decompositions.iter().enumerate() {
            // --- The first matching decomposition owns response selection.
            let Some(captures) = match_pattern(
                &decomposition.pattern,
                &analyzed.canonical,
                &self.script.tags,
            ) else {
                continue;
            };

            // --- Each keyword/decomposition pair rotates independently through
            // its reassembly list.
            let cursor_key = (Keyword::from_atom(keyword), decomposition_index);
            let reassembly_index = {
                let cursor = self.state.cursors.entry(cursor_key).or_insert(0);
                let reassembly_index = *cursor % decomposition.reassemblies.len();
                *cursor += 1;
                reassembly_index
            };

            match &decomposition.reassemblies[reassembly_index] {
                Reassembly::Words(words) => {
                    return KeywordResult::Text(render_reassembly(words.as_slice(), &captures));
                }
                Reassembly::Link(link) => {
                    return self.try_keyword(link.as_str(), analyzed, depth + 1);
                }
                Reassembly::NewKey => return KeywordResult::NewKey,
                Reassembly::Pre { words, link } => {
                    let phrase =
                        format_words(&render_reassembly_words(words.as_slice(), &captures));
                    let pre_analyzed = Script::direct_tokens(&phrase);
                    return self.try_keyword(link.as_str(), &pre_analyzed, depth + 1);
                }
            }
        }

        if let Some(link) = &rule.link {
            return self.try_keyword(link.as_str(), analyzed, depth + 1);
        }

        KeywordResult::NewKey
    }

    fn record_memory(&mut self, analyzed: &AnalyzedInput) {
        let Some(memory) = &self.script.memory else {
            return;
        };

        // --- Memory is triggered by the original input keyword before
        // substitutions, matching the script's `MEMORY <keyword>` declaration.
        if !analyzed
            .original
            .iter()
            .any(|word| word == memory.keyword.as_str())
        {
            return;
        }

        for decomposition in &memory.decompositions {
            // --- Store only the first memory decomposition that matches.
            let Some(captures) = match_pattern(
                &decomposition.pattern,
                &analyzed.canonical,
                &self.script.tags,
            ) else {
                continue;
            };

            self.state
                .memory_queue
                .push_back(render_reassembly(&decomposition.reassembly, &captures));
            break;
        }
    }

    fn none_response(&mut self, analyzed: &AnalyzedInput) -> String {
        match self.try_keyword("NONE", analyzed, 0) {
            KeywordResult::Text(output) => output,
            KeywordResult::NewKey => "PLEASE GO ON".to_owned(),
        }
    }

    /// Start a new session over an immutable parsed script.
    ///
    /// ```
    /// use eliza::eliza::{ElizaSession, doctor_script};
    ///
    /// let mut session = ElizaSession::new(doctor_script());
    /// assert!(!session.respond("Hello").output.is_empty());
    /// ```
    #[must_use]
    pub fn new(script: &'script Script) -> Self {
        Self {
            script,
            state: SessionState::default(),
        }
    }

    /// Produce one ELIZA response and advance response rotation/memory state.
    ///
    /// ```
    /// use eliza::eliza::{ElizaSession, doctor_script};
    ///
    /// let mut session = ElizaSession::new(doctor_script());
    /// let turn = session.respond("I am worried about computers");
    ///
    /// assert_eq!(turn.matched_keyword.as_deref(), Some("COMPUTERS"));
    /// assert_eq!(turn.output, "DO COMPUTERS WORRY YOU");
    /// ```
    pub fn respond(&mut self, input: &str) -> ElizaTurn {
        let analyzed = self.script.analyze(input);
        // --- Memory is recorded before normal response selection so the turn
        // can seed a later no-keyword response.
        self.record_memory(&analyzed);

        // --- Try ranked keywords until one produces text; `NEWKEY` keeps
        // scanning lower-ranked candidates.
        for keyword in self.ranked_keywords(&analyzed) {
            match self.try_keyword(keyword.as_str(), &analyzed, 0) {
                KeywordResult::Text(output) => {
                    return ElizaTurn {
                        normalized_input: format_words(&analyzed.canonical),
                        output,
                        matched_keyword: Some(keyword.to_string()),
                    };
                }
                KeywordResult::NewKey => {}
            }
        }

        let output = self
            .state
            .memory_queue
            .pop_front()
            .unwrap_or_else(|| self.none_response(&analyzed));

        // --- No matched keyword means the response came from memory or NONE.
        ElizaTurn {
            normalized_input: format_words(&analyzed.canonical),
            output,
            matched_keyword: None,
        }
    }
}

/// Return the parsed bundled DOCTOR script shared by provider requests.
///
/// # Panics
///
/// Panics only if the bundled `doctor.script` fails to parse, which indicates a
/// broken release artifact rather than runtime input.
///
/// ```
/// use eliza::eliza::{ElizaSession, doctor_script};
///
/// let mut session = ElizaSession::new(doctor_script());
/// assert_eq!(session.respond("If I fail").output, "DO YOU THINK ITS LIKELY THAT YOU FAIL");
/// ```
pub fn doctor_script() -> &'static Script {
    static SCRIPT: OnceLock<Script> = OnceLock::new();

    SCRIPT.get_or_init(|| {
        DOCTOR_SCRIPT
            .parse()
            .expect("bundled ELIZA DOCTOR script should parse")
    })
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::missing_panics_doc,
    reason = "test rationales replace public panic contracts"
)]
/// Checks ELIZA script parsing and turns behavior at its source owner.
mod tests {
    /// Holds request drivers and setup without copying production logic.
    mod support {
        /// Supplies setup for the script checks.
        pub(super) mod script {
            #![allow(
                clippy::missing_panics_doc,
                reason = "test assertions panic to report failures"
            )]
            pub(crate) use crate::eliza::{ElizaSession, Script, doctor_script};
        }
    }
    /// Checks the script contract.
    mod script {
        use super::support::script::*;
        /// A change here must not alter the accepted DOCTOR script behavior.
        #[test]
        fn it_should_keyword_rank_selects_highest_priority_match() {
            let mut session = ElizaSession::new(doctor_script());
            let turn = session.respond("I am worried about computers");
            assert_eq!(turn.output, "DO COMPUTERS WORRY YOU");
            assert_eq!(turn.matched_keyword.as_deref(), Some("COMPUTERS"));
        }
        /// A change here must not alter the accepted DOCTOR script behavior.
        #[test]
        fn it_should_decomposition_and_reassembly_use_source_captures() {
            let mut session = ElizaSession::new(doctor_script());
            let turn = session.respond("If I fail");
            assert_eq!(turn.output, "DO YOU THINK ITS LIKELY THAT YOU FAIL");
            assert_eq!(turn.matched_keyword.as_deref(), Some("IF"));
        }
        /// A change here must not alter the accepted DOCTOR script behavior.
        #[test]
        fn it_should_reflection_uses_source_substitutions() {
            let mut session = ElizaSession::new(doctor_script());
            let turn = session.respond("I am sad");
            assert_eq!(turn.normalized_input, "YOU ARE SAD");
            assert_eq!(turn.output, "I AM SORRY TO HEAR YOU ARE SAD");
        }
        /// A change here must not alter the accepted DOCTOR script behavior.
        #[test]
        fn it_should_memory_is_recorded_and_retrieved_before_none_fallback() {
            let mut session = ElizaSession::new(doctor_script());
            assert_eq!(
                session.respond("My mother is kind").output,
                "TELL ME MORE ABOUT YOUR FAMILY"
            );
            assert_eq!(
                session.respond("Boring words without a keyword").output,
                "LETS DISCUSS FURTHER WHY YOUR MOTHER IS KIND"
            );
        }
        /// A change here must not alter the accepted DOCTOR script behavior.
        #[test]
        fn it_should_replay_is_deterministic_for_same_transcript() {
            let transcript = ["I am sad", "Why can't you help me", "No"];
            let mut first = ElizaSession::new(doctor_script());
            let mut second = ElizaSession::new(doctor_script());
            let first_outputs = transcript
                .iter()
                .map(|input| first.respond(input).output)
                .collect::<Vec<_>>();
            let second_outputs = transcript
                .iter()
                .map(|input| second.respond(input).output)
                .collect::<Vec<_>>();
            assert_eq!(first_outputs, second_outputs);
        }
        /// A change here must not alter the accepted DOCTOR script behavior.
        #[test]
        fn it_should_parsed_script_can_drive_session_without_static_lifetime() {
            let script = "(HELLO)\nSTART\n(TEST\n  ((0 TEST 0)\n    (OK 3)))"
                .parse::<Script>()
                .expect("custom script should parse");
            let mut session = ElizaSession::new(&script);
            assert_eq!(session.respond("please test now").output, "OK NOW");
        }
        /// A change here must not alter the accepted DOCTOR script behavior.
        #[test]
        fn it_should_parser_ignores_comments_and_keeps_line_columns() {
            let error = "; comment\n(HELLO)\nSTART\n)"
                .parse::<Script>()
                .expect_err("script should fail");
            assert_eq!(error.to_string(), "unexpected closing parenthesis at 4:1");
        }
        /// A change here must not alter the accepted DOCTOR script behavior.
        #[test]
        fn it_should_parser_reports_unexpected_close_with_line_column() {
            let error = "START\n)"
                .parse::<Script>()
                .expect_err("script should fail");
            assert_eq!(error.to_string(), "unexpected closing parenthesis at 2:1");
        }
        /// A change here must not alter the accepted DOCTOR script behavior.
        #[test]
        fn it_should_parser_reports_unexpected_end_with_line_column() {
            let error = "(HELLO".parse::<Script>().expect_err("script should fail");
            assert_eq!(error.to_string(), "unexpected end of script at 1:7");
        }
        /// A change here must not alter the accepted DOCTOR script behavior.
        #[test]
        fn it_should_typed_lowering_reports_invalid_pattern_lists() {
            let error = "(HELLO)\nSTART\n(BAD\n  ((0 (BAD LIST) 0)\n    (THIS WILL NOT LOWER)))"
                .parse::<Script>()
                .expect_err("script should fail");
            assert_eq!(
                error.to_string(),
                "expected pattern tag or alternatives list at 4:7"
            );
        }
        /// A change here must not alter the accepted DOCTOR script behavior.
        #[test]
        fn it_should_typed_lowering_rejects_zero_capture_reassembly() {
            let error = "(HELLO)\nSTART\n(BAD\n  ((0)\n    (0)))"
                .parse::<Script>()
                .expect_err("script should fail");
            assert_eq!(error.to_string(), "expected non-zero capture index at 5:6");
        }
    }
}
