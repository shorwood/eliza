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

use super::errors::{ScriptError, ScriptExpectation};
use super::parser::Parser;
use super::syntax::{Sexp, SexpList, SexpListSplit};

/// Defines the DOCTOR SCRIPT value used by this module.
const DOCTOR_SCRIPT: &str = include_str!("../../fixtures/eliza/doctor.script");

/// Lazily parsed bundled script shared by all fresh sessions.
static DOCTOR: OnceLock<Script> = OnceLock::new();

// -----------------------------------------------------------------------------
// Text: Normalizes input and formats assembled words.
// -----------------------------------------------------------------------------

// TODO: Refactor into a clean API. Something like `Token` and `TokenStream` so that the normalization
// and formatting can be done at the compiler level. And runtime never needs to deal with raw strings.
/// Normalize user input into the words used by script matching.
///
/// # Examples
///
/// ```rust
/// let words = text_tokenize_input("I’m worried—really!");
///
/// // Curly apostrophes survive as ASCII while other punctuation separates words.
/// assert_eq!(words, ["I'M", "WORRIED", "REALLY"]);
/// ```
fn text_tokenize_input(input: &str) -> Vec<String> {
    let punctuation_normalized = input.replace(['\u{2018}', '\u{2019}'], "'");
    let normalized = punctuation_normalized
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

/// Join reassembled words without spaces before punctuation or inside parens.
///
/// # Examples
///
/// ```rust
/// let words = ["WHY", "(", "NOW", ")", "?"].map(str::to_owned);
/// let output = text_format_words(&words);
///
/// // Reassembly tokens become natural response text.
/// assert_eq!(output, "WHY (NOW)?");
/// ```
fn text_format_words(words: &[String]) -> String {
    let mut text = words.join(" ");

    for punctuation in [".", ",", "?", "!", ":", ";"] {
        text = text.replace(&format!(" {punctuation}"), punctuation);
    }

    text = text.replace("( ", "(").replace(" )", ")");
    text.trim().to_owned()
}

// -----------------------------------------------------------------------------
// AnalyzedInput: Retains original and substituted words.
// -----------------------------------------------------------------------------

/// Represents `AnalyzedInput` state within this module.
#[derive(Debug, Clone)]
struct AnalyzedInput {
    /// User words before source-script substitutions.
    original: Vec<String>,
    /// User words after substitutions such as `I` -> `YOU`.
    canonical: Vec<String>,
}

impl From<&str> for AnalyzedInput {
    fn from(input: &str) -> Self {
        let canonical = text_tokenize_input(input);
        Self {
            original: canonical.clone(),
            canonical,
        }
    }
}

// -----------------------------------------------------------------------------
// Keyword: Provides canonical rule-table identity.
// -----------------------------------------------------------------------------

/// Stores the wrapped value owned by this declaration.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct Keyword(
    /// Canonical keyword spelling used for rule lookup.
    String,
);

impl Keyword {
    /// Read a keyword atom from a list position.
    ///
    /// # Errors
    ///
    /// Returns [`ScriptError::Expected`] when the item is missing or is not
    /// an atom.
    fn from_list_atom(
        list: SexpList<'_>,
        index: usize,
        expected: ScriptExpectation,
    ) -> Result<Self, ScriptError> {
        list.expect_atom(index, expected).map(Self::from)
    }

    /// Return the canonical keyword spelling.
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Keyword {
    fn from(atom: &str) -> Self {
        Self(atom.to_owned())
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

// -----------------------------------------------------------------------------
// TagName: Provides canonical DLIST identity.
// -----------------------------------------------------------------------------

/// Stores the wrapped value owned by this declaration.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct TagName(
    /// Canonical DLIST tag spelling.
    String,
);

impl TagName {
    /// Performs the from list operation for this abstraction.
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

// -----------------------------------------------------------------------------
// LinkTarget: Identifies a delegated keyword rule.
// -----------------------------------------------------------------------------

/// Stores the wrapped value owned by this declaration.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
struct LinkTarget(
    /// Keyword selected by the link.
    Keyword,
);

impl LinkTarget {
    /// Performs the from sexp operation for this abstraction.
    fn from_sexp(sexp: &Sexp) -> Option<Self> {
        // Accept compact atom forms before checking structured list forms.
        if let Some(atom) = sexp.atom() {
            return atom
                .strip_prefix('=')
                .map(|target| Self(Keyword::from(target)));
        }

        match sexp.list()?.as_slice() {
            [one] => one.atom().and_then(|atom| {
                atom.strip_prefix('=')
                    .map(|target| Self(Keyword::from(target)))
            }),
            [eq, target] if eq.atom() == Some("=") => {
                target.atom().map(|target| Self(Keyword::from(target)))
            }
            _ => None,
        }
    }

    /// Performs the as str operation for this abstraction.
    fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

// -----------------------------------------------------------------------------
// Precedence: Ranks competing keyword matches.
// -----------------------------------------------------------------------------

/// Stores the wrapped value owned by this declaration.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Default, derive_more::FromStr)]
struct Precedence(
    /// Numeric precedence used to rank matching keywords.
    i32,
);

// -----------------------------------------------------------------------------
// CaptureIndex: Identifies a reassembly capture.
// -----------------------------------------------------------------------------

/// Stores the wrapped value owned by this declaration.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct CaptureIndex(
    /// One-based reassembly capture position.
    NonZeroUsize,
);

// -----------------------------------------------------------------------------
// ReassemblyItem: Models literal and captured fragments.
// -----------------------------------------------------------------------------

/// Enumerates the supported `ReassemblyItem` cases.
#[derive(Debug, Clone)]
enum ReassemblyItem {
    /// Literal output word from the selected reassembly rule.
    /// Stores the wrapped value owned by this declaration.
    Word(
        /// Literal word copied into the response.
        String,
    ),
    /// One-based capture slot from the matched decomposition pattern.
    /// Stores the wrapped value owned by this declaration.
    Capture(
        /// Capture inserted into the response.
        CaptureIndex,
    ),
}

/// Performs the render reassembly words operation for this abstraction.
fn render_reassembly_words(items: &[ReassemblyItem], captures: &[Vec<String>]) -> Vec<String> {
    let mut words = Vec::new();

    for item in items {
        match item {
            ReassemblyItem::Word(word) => words.push(word.clone()),
            ReassemblyItem::Capture(capture) => {
                let values = captures.get(capture.0.get() - 1).into_iter().flatten();
                words.extend(values.cloned());
            }
        }
    }

    words
}

/// Performs the render reassembly operation for this abstraction.
fn render_reassembly(items: &[ReassemblyItem], captures: &[Vec<String>]) -> String {
    text_format_words(&render_reassembly_words(items, captures))
}

// -----------------------------------------------------------------------------
// PatternItem: Models one decomposition position.
// -----------------------------------------------------------------------------

/// Enumerates the supported `PatternItem` cases.
#[derive(Debug, Clone)]
enum PatternItem {
    /// `0` in the source script: match any number of words.
    Wildcard,
    /// Literal keyword match.
    /// Stores the wrapped value owned by this declaration.
    Word(
        /// Literal keyword required at this position.
        Keyword,
    ),
    /// Match any word that belongs to one of the named DLIST tags.
    /// Stores the wrapped value owned by this declaration.
    Tag(
        /// DLIST tags accepted at this position.
        Vec<TagName>,
    ),
    /// Match one of several literal alternatives.
    /// Stores the wrapped value owned by this declaration.
    Alternatives(
        /// Literal keywords accepted at this position.
        Vec<Keyword>,
    ),
}

// -----------------------------------------------------------------------------
// Reassembly: Models response and control alternatives.
// -----------------------------------------------------------------------------

/// Enumerates the supported `Reassembly` cases.
#[derive(Debug, Clone)]
enum Reassembly {
    /// Render literal words and capture references.
    /// Stores the wrapped value owned by this declaration.
    Words(
        /// Items rendered into a response.
        Vec<ReassemblyItem>,
    ),
    /// Defer response selection to another keyword.
    /// Stores the wrapped value owned by this declaration.
    Link(
        /// Keyword whose rules should be evaluated next.
        LinkTarget,
    ),
    /// Abandon this keyword and continue ranking.
    NewKey,
    /// Rewrite the input phrase, then evaluate another keyword.
    Pre {
        /// Stores the words value owned by this contract.
        words: Vec<ReassemblyItem>,
        /// Stores the link value owned by this contract.
        link: LinkTarget,
    },
}

// -----------------------------------------------------------------------------
// DecompositionRule: Couples patterns with reassemblies.
// -----------------------------------------------------------------------------

/// Represents `DecompositionRule` state within this module.
#[derive(Debug)]
struct DecompositionRule {
    /// Pattern matched against canonicalized user input.
    pattern: Vec<PatternItem>,
    /// Rotating responses used when this pattern matches.
    reassemblies: Vec<Reassembly>,
}

impl TryFrom<&Sexp> for DecompositionRule {
    type Error = ScriptError;

    fn try_from(sexp: &Sexp) -> Result<Self, Self::Error> {
        // Lower the decomposition pattern from its leading list.
        let items = SexpList::from_sexp(sexp, ScriptExpectation::DecompositionRule)?;
        let pattern_sexp = items.expect(0, ScriptExpectation::DecompositionPattern)?;
        let pattern_items =
            SexpList::from_sexp(pattern_sexp, ScriptExpectation::DecompositionPatternList)?;

        // Lower each pattern atom after validating the enclosing rule shape.
        let pattern = pattern_items
            .iter()
            .map(PatternItem::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        // Preserve the remaining response alternatives in source order.
        let reassembly_items = items.tail(1);
        let reassemblies = reassembly_items
            .iter()
            .map(Reassembly::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            pattern,
            reassemblies,
        })
    }
}

// -----------------------------------------------------------------------------
// MemoryDecomposition: Couples memory patterns with responses.
// -----------------------------------------------------------------------------

/// Represents `MemoryDecomposition` state within this module.
#[derive(Debug)]
struct MemoryDecomposition {
    /// Pattern used only when the memory keyword appears in original input.
    pattern: Vec<PatternItem>,
    /// Response stored for later no-keyword fallback.
    reassembly: Vec<ReassemblyItem>,
}

impl TryFrom<&Sexp> for MemoryDecomposition {
    type Error = ScriptError;

    fn try_from(sexp: &Sexp) -> Result<Self, Self::Error> {
        let parts = SexpList::from_sexp(sexp, ScriptExpectation::MemoryDecomposition)?;

        // Memory rules use `pattern = reassembly` instead of nested
        // reassembly lists, so split once on the separator atom.
        let SexpListSplit {
            before: pattern_items,
            after: reassembly_sexps,
        } = parts.split_once_atom("=").ok_or(
            sexp.span
                .expected(ScriptExpectation::MemoryReassemblySeparator),
        )?;

        // Lower both sides after validating the memory-rule separator.
        let pattern = pattern_items
            .iter()
            .map(PatternItem::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        let reassembly = ReassemblyItem::from_list(reassembly_sexps)?;

        // Preserve every optional keyword attribute in its typed form.
        Ok(Self {
            pattern,
            reassembly,
        })
    }
}

// -----------------------------------------------------------------------------
// MemoryRule: Owns deferred-response decompositions.
// -----------------------------------------------------------------------------

/// Represents `MemoryRule` state within this module.
#[derive(Debug)]
struct MemoryRule {
    /// Original-input keyword that activates memory storage.
    keyword: Keyword,
    /// Candidate memory patterns.
    decompositions: Vec<MemoryDecomposition>,
}

impl TryFrom<SexpList<'_>> for MemoryRule {
    type Error = ScriptError;

    fn try_from(items: SexpList<'_>) -> Result<Self, Self::Error> {
        let keyword = Keyword::from_list_atom(items, 1, ScriptExpectation::MemoryKeyword)?;
        let decomposition_items = items.tail(2);
        let decompositions = decomposition_items
            .iter()
            .map(MemoryDecomposition::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            keyword,
            decompositions,
        })
    }
}

// -----------------------------------------------------------------------------
// TransformRule: Owns executable keyword behavior.
// -----------------------------------------------------------------------------

/// Represents `TransformRule` state within this module.
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
// KeywordRule: Owns one parsed keyword declaration.
// -----------------------------------------------------------------------------

/// Represents `KeywordRule` state within this module.
struct KeywordRule {
    /// Stores the keyword value owned by this contract.
    keyword: Keyword,
    /// Stores the substitution value owned by this contract.
    substitution: Option<Keyword>,
    /// Stores the tags value owned by this contract.
    tags: Vec<TagName>,
    /// Executable behavior, absent only for metadata declarations.
    transform: TransformRule,
}

impl TryFrom<SexpList<'_>> for KeywordRule {
    type Error = ScriptError;

    fn try_from(items: SexpList<'_>) -> Result<Self, Self::Error> {
        let keyword = Keyword::from_list_atom(items, 0, ScriptExpectation::Keyword)?;
        let mut cursor = 1;

        // Optional `= WORD` substitution aliases one input word to another.
        let substitution = if items.atom_at(cursor) == Some("=") {
            cursor += 1;
            let substitution = Some(Keyword::from_list_atom(
                items,
                cursor,
                ScriptExpectation::Substitution,
            )?);
            cursor += 1;
            substitution
        } else {
            None
        };

        // Optional numeric precedence controls ranking when multiple
        // keywords occur in the same input.
        let mut precedence = Precedence::default();
        if let Some(atom) = items.atom_at(cursor)
            && let Ok(value) = atom.parse::<Precedence>()
        {
            precedence = value;
            cursor += 1;
        }

        // Optional DLIST attaches semantic tags to the keyword and, when a
        // substitution exists, to the replacement keyword as well.
        let tags = if items.atom_at(cursor) == Some("DLIST") {
            cursor += 1;
            let tag_sexp = items.expect(cursor, ScriptExpectation::DlistTagList)?;
            let tag_items = SexpList::from_sexp(tag_sexp, ScriptExpectation::DlistTagList)?;
            let tags = TagName::from_list(tag_items);
            cursor += 1;
            tags
        } else {
            Vec::new()
        };

        // Optional link can replace local decompositions or act as fallback.
        let link = items.get(cursor).and_then(LinkTarget::from_sexp);
        if link.is_some() {
            cursor += 1;
        }

        // Everything left is a decomposition rule in source order.
        let decomposition_items = items.tail(cursor);
        let decompositions = decomposition_items
            .iter()
            .map(DecompositionRule::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        let transform = TransformRule {
            precedence,
            link,
            decompositions,
        };

        // Return metadata and executable behavior as one keyword declaration.
        Ok(Self {
            keyword,
            substitution,
            tags,
            transform,
        })
    }
}

// -----------------------------------------------------------------------------
// RuleForm: Distinguishes memory and keyword declarations.
// -----------------------------------------------------------------------------

/// Enumerates the supported `RuleForm` cases.
enum RuleForm {
    /// Stores the wrapped value owned by this declaration.
    Memory(
        /// Parsed memory rule.
        MemoryRule,
    ),
    /// Stores the wrapped value owned by this declaration.
    Keyword(
        /// Parsed keyword rule.
        KeywordRule,
    ),
}

impl TryFrom<SexpList<'_>> for RuleForm {
    type Error = ScriptError;

    fn try_from(items: SexpList<'_>) -> Result<Self, Self::Error> {
        let first = Keyword::from_list_atom(items, 0, ScriptExpectation::Keyword)?;
        if first.as_str() == "MEMORY" {
            MemoryRule::try_from(items).map(Self::Memory)
        } else {
            KeywordRule::try_from(items).map(Self::Keyword)
        }
    }
}

impl TryFrom<&Sexp> for PatternItem {
    type Error = ScriptError;

    fn try_from(sexp: &Sexp) -> Result<Self, Self::Error> {
        // Interpret the script's zero atom as its wildcard sentinel.
        if sexp.atom() == Some("0") {
            return Ok(Self::Wildcard);
        }

        // Plain atoms represent literal keyword matches.
        if let Some(atom) = sexp.atom() {
            return Ok(Self::Word(Keyword::from(atom)));
        }

        let items = SexpList::from_sexp(sexp, ScriptExpectation::PatternItem)?;
        let head = items.expect_atom(0, ScriptExpectation::PatternListHead)?;

        // Interpret both spaced and joined stars as literal alternatives.
        if head == "*" || head.starts_with('*') {
            let mut alternatives = Vec::new();
            if let Some(stripped) = head.strip_prefix('*')
                && !stripped.is_empty()
            {
                alternatives.push(Keyword::from(stripped));
            }
            alternatives.extend(items.tail(1).atoms().map(Keyword::from));
            Ok(Self::Alternatives(alternatives))
        } else if head == "/" || head.starts_with('/') {
            // Both spaced and joined slash forms denote a tag lookup.
            Ok(Self::Tag(TagName::from_list(items)))
        } else {
            Err(sexp
                .span
                .expected(ScriptExpectation::PatternTagOrAlternativesList))
        }
    }
}

impl TryFrom<&Sexp> for Reassembly {
    type Error = ScriptError;

    fn try_from(sexp: &Sexp) -> Result<Self, Self::Error> {
        // Resolve standalone link forms before inspecting list commands.
        if let Some(link) = LinkTarget::from_sexp(sexp) {
            return Ok(Self::Link(link));
        }

        let items = SexpList::from_sexp(sexp, ScriptExpectation::ReassemblyList)?;

        // `NEWKEY` abandons this rule without requiring further operands.
        if items.atom_at(0) == Some("NEWKEY") {
            return Ok(Self::NewKey);
        }

        // PRE performs a local rewrite before jumping through a link.
        if items.atom_at(0) == Some("PRE") {
            let words = SexpList::from_sexp(
                items.expect(1, ScriptExpectation::PreReassemblyWords)?,
                ScriptExpectation::PreReassemblyWords,
            )?;
            let link = items
                .get(2)
                .and_then(LinkTarget::from_sexp)
                .ok_or(sexp.span.expected(ScriptExpectation::PreTargetLink))?;
            return Ok(Self::Pre {
                words: ReassemblyItem::from_list(words)?,
                link,
            });
        }

        Ok(Self::Words(ReassemblyItem::from_list(items)?))
    }
}

impl TryFrom<&Sexp> for ReassemblyItem {
    type Error = ScriptError;

    fn try_from(item: &Sexp) -> Result<Self, Self::Error> {
        let atom = item
            .atom()
            .ok_or(item.span.expected(ScriptExpectation::ReassemblyAtom))?;

        // Numeric atoms are capture references rather than literal words.
        if let Ok(value) = atom.parse::<usize>() {
            let capture = NonZeroUsize::new(value)
                .map(CaptureIndex)
                .ok_or(item.span.expected(ScriptExpectation::NonZeroCaptureIndex))?;
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
    /// Returns [`ScriptError`] when any item is not an atom or references capture
    /// index zero.
    fn from_list(items: SexpList<'_>) -> Result<Vec<Self>, ScriptError> {
        items.iter().map(Self::try_from).collect()
    }
}

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
// -----------------------------------------------------------------------------
// Script: Owns the lowered ELIZA program.
// -----------------------------------------------------------------------------

#[derive(Debug, Default)]
pub(crate) struct Script {
    /// Stores the substitutions value owned by this contract.
    substitutions: HashMap<Keyword, Keyword>,
    /// Stores the tags value owned by this contract.
    tags: HashMap<Keyword, BTreeSet<TagName>>,
    /// Stores the transforms value owned by this contract.
    transforms: HashMap<Keyword, TransformRule>,
    /// Stores the memory value owned by this contract.
    memory: Option<MemoryRule>,
}

impl Script {
    /// Preserve original words while applying the script's substitutions.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let analyzed = doctor_script().analyze("I am sad");
    ///
    /// // Keyword ranking sees the original input; matching sees canonical words.
    /// assert_eq!(analyzed.original, ["I", "AM", "SAD"]);
    /// assert_eq!(analyzed.canonical, ["YOU", "ARE", "SAD"]);
    /// ```
    fn analyze(&self, input: &str) -> AnalyzedInput {
        let original = text_tokenize_input(input);

        // Substitutions are applied after tokenization so matching uses the
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

    /// Performs the apply keyword rule operation for this abstraction.
    fn apply_keyword_rule(&mut self, rule: KeywordRule) {
        // Substitutions are kept separately because input normalization uses
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

        // Tags travel with both source and replacement keywords so tagged
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

        // A keyword without link or decompositions is only a substitution or
        // tag declaration, not a transform candidate.
        if rule.transform.link.is_none() && rule.transform.decompositions.is_empty() {
            return;
        }

        // Register the executable transform after metadata-only rules return.
        self.transforms.insert(rule.keyword.clone(), rule.transform);
    }

    /// Apply one lowered source declaration to the script.
    fn apply_rule_form(&mut self, form: RuleForm) {
        match form {
            RuleForm::Memory(memory) => self.memory = Some(memory),
            RuleForm::Keyword(rule) => self.apply_keyword_rule(rule),
        }
    }
}

impl FromStr for Script {
    type Err = ScriptError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        // Initialize an empty lowered script before visiting source forms.
        let sexps = Parser::parse(input)?;
        let mut script = Script::default();
        let mut saw_greeting = false;
        let mut started = false;

        for sexp in sexps {
            // Forms before START are greeting/banner material in the source
            // script; they prove the script has the expected outer shape.
            if sexp.atom() == Some("START") {
                started = true;
                continue;
            }

            let Some(items) = sexp.list() else {
                continue;
            };

            // Classify each list by its position relative to START.
            match items.as_slice() {
                // An empty list terminates the original script table.
                [] => break,
                _ if !started => {
                    saw_greeting |= items.atoms().next().is_some();
                }
                // After START every list must lower to memory or keyword
                // behavior.
                _ => script.apply_rule_form(RuleForm::try_from(items)?),
            }
        }

        // A script without a greeting cannot initialize a valid session.
        if !saw_greeting {
            return Err(ScriptError::MissingGreeting { line: 1, column: 1 });
        }

        Ok(script)
    }
}

/// Defines the MAX LINK DEPTH value used by this module.
const MAX_LINK_DEPTH: usize = 12;

// -----------------------------------------------------------------------------
// ElizaTurn: Reports one completed response.
// -----------------------------------------------------------------------------

/// Result of one ELIZA response operation.
#[derive(Debug, Clone)]
pub(crate) struct ElizaTurn {
    /// Canonical input after source-script substitutions were applied.
    #[cfg(test)]
    normalized_input: String,
    /// Provider-ready response text produced by reassembly.
    pub(crate) output: String,
    /// Keyword whose transform produced the output, when one matched directly.
    #[cfg(test)]
    matched_keyword: Option<String>,
}

// -----------------------------------------------------------------------------
// SessionState: Retains response rotation and memory.
// -----------------------------------------------------------------------------

/// Represents `SessionState` state within this module.
#[derive(Debug, Default)]
struct SessionState {
    /// Stores the cursors value owned by this contract.
    cursors: HashMap<CursorKey, usize>,
    /// Stores the memory queue value owned by this contract.
    memory_queue: VecDeque<String>,
}

// -----------------------------------------------------------------------------
// CursorKey: Identifies one rotating decomposition rule.
// -----------------------------------------------------------------------------

/// Stable identity for one rotating decomposition rule.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
struct CursorKey {
    /// Keyword owning the decomposition.
    keyword: Keyword,
    /// Zero-based decomposition position within that keyword.
    decomposition: usize,
}

// -----------------------------------------------------------------------------
// RankedKeyword: Retains keyword ranking inputs.
// -----------------------------------------------------------------------------

/// Keyword candidate annotated with its ranking inputs.
struct RankedKeyword {
    /// Script-defined ranking precedence.
    precedence: Precedence,
    /// Original input position used to break ranking ties.
    position: usize,
    /// Keyword to evaluate.
    keyword: Keyword,
}

// -----------------------------------------------------------------------------
// KeywordResult: Reports response text or continuation.
// -----------------------------------------------------------------------------

/// Enumerates the supported `KeywordResult` cases.
enum KeywordResult {
    /// Stores the wrapped value owned by this declaration.
    Text(
        /// Rendered response text.
        String,
    ),
    /// Represents the `NewKey` case.
    NewKey,
}

// -----------------------------------------------------------------------------
// IsPattern: Matches decompositions while recording captures.
// -----------------------------------------------------------------------------

/// Report whether one pattern item accepts one input word.
fn is_pattern_item_match(
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

/// Try every wildcard width until the remaining pattern matches.
fn is_pattern_wildcard_match(
    pattern: &[PatternItem],
    input: &[String],
    tags: &HashMap<Keyword, BTreeSet<TagName>>,
    captures: &mut Vec<Vec<String>>,
) -> bool {
    for width in 0..=input.len() {
        captures.push(input[..width].to_vec());

        // Preserve the first wildcard width whose suffix matches.
        if is_pattern_match_recursive(&pattern[1..], &input[width..], tags, captures) {
            return true;
        }
        captures.pop();
    }
    false
}

/// Match and capture one non-wildcard leading item.
fn is_pattern_leading_item_match(
    item: &PatternItem,
    pattern: &[PatternItem],
    input: &[String],
    tags: &HashMap<Keyword, BTreeSet<TagName>>,
    captures: &mut Vec<Vec<String>>,
) -> bool {
    // Non-wildcard items require one input word.
    let Some(word) = input.first() else {
        return false;
    };

    // Reject this branch as soon as its leading item differs.
    if !is_pattern_item_match(item, word, tags) {
        return false;
    }
    captures.push(vec![word.clone()]);
    if is_pattern_match_recursive(&pattern[1..], &input[1..], tags, captures) {
        true
    } else {
        captures.pop();
        false
    }
}

/// Recursively match a pattern while recording reassembly captures.
fn is_pattern_match_recursive(
    pattern: &[PatternItem],
    input: &[String],
    tags: &HashMap<Keyword, BTreeSet<TagName>>,
    captures: &mut Vec<Vec<String>>,
) -> bool {
    // Exhausted patterns match only when they consume the whole input.
    if pattern.is_empty() {
        return input.is_empty();
    }

    match &pattern[0] {
        PatternItem::Wildcard => is_pattern_wildcard_match(pattern, input, tags, captures),
        item => is_pattern_leading_item_match(item, pattern, input, tags, captures),
    }
}

// -----------------------------------------------------------------------------
// PatternCaptures: Returns captures for a complete decomposition match.
// -----------------------------------------------------------------------------

/// Performs the match pattern operation for this abstraction.
fn pattern_captures(
    pattern: &[PatternItem],
    input: &[String],
    tags: &HashMap<Keyword, BTreeSet<TagName>>,
) -> Option<Vec<Vec<String>>> {
    let mut captures = Vec::new();
    if is_pattern_match_recursive(pattern, input, tags, &mut captures) {
        Some(captures)
    } else {
        None
    }
}

// -----------------------------------------------------------------------------
// ElizaSession: Executes stateful conversations over one script.
// -----------------------------------------------------------------------------

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
///
/// session.respond("My mother is kind");
/// let recalled = session.respond("Boring words without a keyword");
///
/// // A session can recall memory recorded by an earlier turn.
/// assert_eq!(recalled.output, "LETS DISCUSS FURTHER WHY YOUR MOTHER IS KIND");
/// ```
#[derive(Debug)]
pub(crate) struct ElizaSession<'script> {
    /// Stores the script value owned by this contract.
    script: &'script Script,
    /// Stores the state value owned by this contract.
    state: SessionState,
}

impl ElizaSession<'_> {
    /// Add a unique transform-backed word to the ranked candidate list.
    fn add_ranked_keyword(&self, candidates: &mut Vec<RankedKeyword>, position: usize, word: &str) {
        // Words without transforms cannot become ranked candidates.
        let Some(rule) = self.script.transforms.get(word) else {
            return;
        };

        // Preserve only the first occurrence of each keyword.
        if candidates
            .iter()
            .any(|candidate| candidate.keyword.as_str() == word)
        {
            return;
        }

        // Record the first occurrence with its source position and precedence.
        candidates.push(RankedKeyword {
            precedence: rule.precedence,
            position,
            keyword: Keyword::from(word),
        });
    }

    /// Performs the ranked keywords operation for this abstraction.
    fn ranked_keywords(&self, analyzed: &AnalyzedInput) -> Vec<Keyword> {
        let mut candidates = Vec::new();

        // Rank original words, not substituted words, matching the classic
        // behavior where substitutions normalize captures but not trigger order.
        for (position, word) in analyzed.original.iter().enumerate() {
            self.add_ranked_keyword(&mut candidates, position, word);
        }

        candidates.sort_by(|left, right| {
            right
                .precedence
                .cmp(&left.precedence)
                .then_with(|| left.position.cmp(&right.position))
        });
        candidates
            .into_iter()
            .map(|candidate| candidate.keyword)
            .collect()
    }

    /// Performs the try keyword operation for this abstraction.
    fn try_keyword(
        &mut self,
        keyword: &str,
        analyzed: &AnalyzedInput,
        depth: usize,
    ) -> KeywordResult {
        // Bound malformed cyclic links before recursive evaluation continues.
        if depth > MAX_LINK_DEPTH {
            return KeywordResult::NewKey;
        }

        // Missing transform means this keyword cannot produce text.
        let Some(rule) = self.script.transforms.get(keyword) else {
            return KeywordResult::NewKey;
        };

        // Link-only keywords immediately delegate to their target.
        if rule.decompositions.is_empty() {
            // A link-only keyword delegates without attempting decomposition.
            if let Some(link) = &rule.link {
                return self.try_keyword(link.as_str(), analyzed, depth + 1);
            }
            return KeywordResult::NewKey;
        }

        for (decomposition_index, decomposition) in rule.decompositions.iter().enumerate() {
            // The first matching decomposition owns response selection.
            let Some(captures) = pattern_captures(
                &decomposition.pattern,
                &analyzed.canonical,
                &self.script.tags,
            ) else {
                continue;
            };

            // Each keyword/decomposition pair rotates independently through
            // its reassembly list.
            let cursor_key = CursorKey {
                keyword: Keyword::from(keyword),
                decomposition: decomposition_index,
            };
            let reassembly_index = {
                let cursor = self.state.cursors.entry(cursor_key).or_insert(0);
                let reassembly_index = *cursor % decomposition.reassemblies.len();
                *cursor += 1;
                reassembly_index
            };

            match &decomposition.reassemblies[reassembly_index] {
                // Literal reassembly completes this keyword evaluation.
                Reassembly::Words(words) => {
                    return KeywordResult::Text(render_reassembly(words.as_slice(), &captures));
                }
                // Linked reassembly delegates to the selected keyword.
                Reassembly::Link(link) => {
                    return self.try_keyword(link.as_str(), analyzed, depth + 1);
                }
                // `NEWKEY` tells the caller to continue its ranked search.
                Reassembly::NewKey => return KeywordResult::NewKey,
                // PRE rewrites the phrase before following its keyword link.
                Reassembly::Pre { words, link } => {
                    let phrase =
                        text_format_words(&render_reassembly_words(words.as_slice(), &captures));
                    let pre_analyzed = AnalyzedInput::from(phrase.as_str());
                    return self.try_keyword(link.as_str(), &pre_analyzed, depth + 1);
                }
            }
        }

        // A fallback link runs only after every decomposition fails.
        if let Some(link) = &rule.link {
            return self.try_keyword(link.as_str(), analyzed, depth + 1);
        }

        KeywordResult::NewKey
    }

    /// Performs the record memory operation for this abstraction.
    fn record_memory(&mut self, analyzed: &AnalyzedInput) {
        // Scripts without a memory rule have nothing to record.
        let Some(memory) = &self.script.memory else {
            return;
        };

        // Memory is triggered by the original input keyword before
        // substitutions, matching the script's `MEMORY <keyword>` declaration.
        if !analyzed
            .original
            .iter()
            .any(|word| word == memory.keyword.as_str())
        {
            return;
        }

        for decomposition in &memory.decompositions {
            // Store only the first memory decomposition that matches.
            let Some(captures) = pattern_captures(
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

    /// Performs the none response operation for this abstraction.
    fn none_response(&mut self, analyzed: &AnalyzedInput) -> String {
        match self.try_keyword("NONE", analyzed, 0) {
            KeywordResult::Text(output) => output,
            KeywordResult::NewKey => "PLEASE GO ON".to_owned(),
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
    pub(crate) fn respond(&mut self, input: &str) -> ElizaTurn {
        let analyzed = self.script.analyze(input);

        // Memory is recorded before normal response selection so the turn
        // can seed a later no-keyword response.
        self.record_memory(&analyzed);

        // Try ranked keywords until one produces text; `NEWKEY` keeps
        // scanning lower-ranked candidates.
        for keyword in self.ranked_keywords(&analyzed) {
            match self.try_keyword(keyword.as_str(), &analyzed, 0) {
                // The first ranked keyword producing text completes the turn.
                KeywordResult::Text(output) => {
                    return ElizaTurn {
                        #[cfg(test)]
                        normalized_input: text_format_words(&analyzed.canonical),
                        output,
                        #[cfg(test)]
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

        // No matched keyword means the response came from memory or NONE.
        ElizaTurn {
            #[cfg(test)]
            normalized_input: text_format_words(&analyzed.canonical),
            output,
            #[cfg(test)]
            matched_keyword: None,
        }
    }
}

impl<'script> From<&'script Script> for ElizaSession<'script> {
    fn from(script: &'script Script) -> Self {
        Self {
            script,
            state: SessionState::default(),
        }
    }
}

// -----------------------------------------------------------------------------
// Tests: Verify script parsing, lowering, and conversation behavior.
// -----------------------------------------------------------------------------

/// Checks ELIZA script parsing and turns behavior at its source owner.
#[cfg(test)]
#[expect(
    clippy::missing_panics_doc,
    reason = "test rationales replace public panic contracts"
)]
mod tests {
    mod it_should_converse {
        use super::*;

        /// A change here must not alter the accepted DOCTOR script behavior.
        #[test]
        fn it_should_keyword_rank_selects_highest_priority_match() {
            let mut session = ElizaSession::from(doctor_script());
            let turn = session.respond("I am worried about computers");
            assert_eq!(turn.output, "DO COMPUTERS WORRY YOU");
            assert_eq!(turn.matched_keyword.as_deref(), Some("COMPUTERS"));
        }

        /// A change here must not alter the accepted DOCTOR script behavior.
        #[test]
        fn it_should_decomposition_and_reassembly_use_source_captures() {
            let mut session = ElizaSession::from(doctor_script());
            let turn = session.respond("If I fail");
            assert_eq!(turn.output, "DO YOU THINK ITS LIKELY THAT YOU FAIL");
            assert_eq!(turn.matched_keyword.as_deref(), Some("IF"));
        }

        /// A change here must not alter the accepted DOCTOR script behavior.
        #[test]
        fn it_should_reflection_uses_source_substitutions() {
            let mut session = ElizaSession::from(doctor_script());
            let turn = session.respond("I am sad");
            assert_eq!(turn.normalized_input, "YOU ARE SAD");
            assert_eq!(turn.output, "I AM SORRY TO HEAR YOU ARE SAD");
        }

        /// A change here must not alter the accepted DOCTOR script behavior.
        #[test]
        fn it_should_memory_is_recorded_and_retrieved_before_none_fallback() {
            let mut session = ElizaSession::from(doctor_script());
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
            let mut first = ElizaSession::from(doctor_script());
            let mut second = ElizaSession::from(doctor_script());
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
    }

    mod it_should_parse_source {
        use super::*;

        /// A change here must not alter the accepted DOCTOR script behavior.
        #[test]
        fn it_should_parsed_script_can_drive_session_without_static_lifetime() {
            let script = "(HELLO)\nSTART\n(TEST\n  ((0 TEST 0)\n    (OK 3)))"
                .parse::<Script>()
                .expect("custom script should parse");
            let mut session = ElizaSession::from(&script);
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
    }

    mod it_should_reject_invalid_source {
        use super::*;

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

    use super::{ElizaSession, Script, doctor_script};
}

// -----------------------------------------------------------------------------
// DoctorScript: Exposes the validated bundled script.
// -----------------------------------------------------------------------------

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
#[cfg_attr(
    test,
    expect(
        clippy::items_after_test_module,
        reason = "rlib requires dependency-first declaration order for test modules"
    )
)]
pub(crate) fn doctor_script() -> &'static Script {
    DOCTOR.get_or_init(|| {
        DOCTOR_SCRIPT
            .parse()
            .expect("bundled ELIZA DOCTOR script should parse")
    })
}
