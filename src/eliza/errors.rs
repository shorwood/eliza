//! Diagnostics raised while parsing and lowering ELIZA scripts.

use std::fmt;

use miette::Diagnostic;
use thiserror::Error;

// -----------------------------------------------------------------------------
// ScriptError: Reports positioned script failures.
// -----------------------------------------------------------------------------

/// Failure raised while parsing or lowering an ELIZA script.
#[derive(Debug, Diagnostic, Error)]
pub(crate) enum ScriptError {
    /// Script lowering found the wrong S-expression shape for a known form.
    #[error("expected {expected} at {line}:{column}")]
    #[diagnostic(code(eliza::script::expected))]
    Expected {
        /// Typed grammar element required at this position.
        expected: ScriptExpectation,
        /// One-based source line containing the invalid form.
        line: usize,
        /// One-based source column containing the invalid form.
        column: usize,
    },

    /// Script did not provide the greeting form before `START`.
    #[error("script is missing a greeting at {line}:{column}")]
    #[diagnostic(code(eliza::script::missing_greeting))]
    MissingGreeting {
        /// One-based source line at which lowering detected the omission.
        line: usize,
        /// One-based source column at which lowering detected the omission.
        column: usize,
    },

    /// Script parser saw a close paren without a matching open paren.
    #[error("unexpected closing parenthesis at {line}:{column}")]
    #[diagnostic(code(eliza::script::unexpected_close))]
    UnexpectedClose {
        /// One-based source line containing the unmatched delimiter.
        line: usize,
        /// One-based source column containing the unmatched delimiter.
        column: usize,
    },

    /// Script parser reached EOF while still expecting a token or close paren.
    #[error("unexpected end of script at {line}:{column}")]
    #[diagnostic(code(eliza::script::unexpected_end))]
    UnexpectedEnd {
        /// One-based source line where the parser stopped.
        line: usize,
        /// One-based source column where the parser stopped.
        column: usize,
    },
}

// -----------------------------------------------------------------------------
// ScriptExpectation: Enumerates every grammar expectation and its wording.
// -----------------------------------------------------------------------------

/// Grammar element named by [`ScriptError::Expected`].
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum ScriptExpectation {
    /// `DLIST` tag list.
    DlistTagList,
    /// Complete decomposition rule.
    DecompositionRule,
    /// Decomposition pattern.
    DecompositionPattern,
    /// List containing a decomposition pattern.
    DecompositionPatternList,
    /// Keyword atom.
    Keyword,
    /// Memory-rule decomposition.
    MemoryDecomposition,
    /// Memory-rule keyword.
    MemoryKeyword,
    /// Separator between a memory pattern and reassembly.
    MemoryReassemblySeparator,
    /// Nonzero reassembly capture index.
    NonZeroCaptureIndex,
    /// One pattern item.
    PatternItem,
    /// First atom in a structured pattern.
    PatternListHead,
    /// Structured pattern containing a tag or alternatives.
    PatternTagOrAlternativesList,
    /// Words rewritten by `PRE`.
    PreReassemblyWords,
    /// Link target selected by `PRE`.
    PreTargetLink,
    /// Reassembly atom.
    ReassemblyAtom,
    /// List containing a reassembly.
    ReassemblyList,
    /// Keyword substitution atom.
    Substitution,
}

impl fmt::Display for ScriptExpectation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::DlistTagList => "DLIST tag list",
            Self::DecompositionRule => "decomposition rule",
            Self::DecompositionPattern => "decomposition pattern",
            Self::DecompositionPatternList => "decomposition pattern list",
            Self::Keyword => "keyword",
            Self::MemoryDecomposition => "memory decomposition",
            Self::MemoryKeyword => "memory keyword",
            Self::MemoryReassemblySeparator => "memory reassembly separator",
            Self::NonZeroCaptureIndex => "non-zero capture index",
            Self::PatternItem => "pattern item",
            Self::PatternListHead => "pattern list head",
            Self::PatternTagOrAlternativesList => "pattern tag or alternatives list",
            Self::PreReassemblyWords => "PRE reassembly words",
            Self::PreTargetLink => "PRE target link",
            Self::ReassemblyAtom => "reassembly atom",
            Self::ReassemblyList => "reassembly list",
            Self::Substitution => "substitution",
        })
    }
}
