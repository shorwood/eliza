//! Positioned S-expression syntax shared by parsing and ELIZA lowering.

use crate::errors::AppError;

// -----------------------------------------------------------------------------
// Span: Retains source positions for diagnostics.
// -----------------------------------------------------------------------------

/// Represents `Span` state within this module.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct Span {
    /// Stores the line value owned by this contract.
    pub(super) line: usize,
    /// Stores the column value owned by this contract.
    pub(super) column: usize,
}

impl Span {
    /// Performs the expected operation for this abstraction.
    pub(super) const fn expected(self, expected: &'static str) -> AppError {
        AppError::ScriptExpected {
            expected,
            line: self.line,
            column: self.column,
        }
    }
}

// -----------------------------------------------------------------------------
// SexpKind: Models atoms and lists before lowering.
// -----------------------------------------------------------------------------

/// Enumerates the supported `SexpKind` cases.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum SexpKind {
    /// Stores the wrapped value owned by this declaration.
    Atom(
        /// Atom text preserved from the token stream.
        String,
    ),
    /// Stores the wrapped value owned by this declaration.
    List(
        /// Child expressions enclosed by the list.
        Vec<Sexp>,
    ),
}

// -----------------------------------------------------------------------------
// Sexp: Retains one expression and its position.
// -----------------------------------------------------------------------------

/// Represents `Sexp` state within this module.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct Sexp {
    /// Recursive syntax node produced before script forms are interpreted.
    pub(super) kind: SexpKind,
    /// Span of the opening token or atom that introduced this node.
    pub(super) span: Span,
}

impl Sexp {
    /// Performs the atom operation for this abstraction.
    pub(super) fn atom(&self) -> Option<&str> {
        match &self.kind {
            SexpKind::Atom(atom) => Some(atom),
            SexpKind::List(_) => None,
        }
    }

    /// Performs the list operation for this abstraction.
    pub(super) fn list(&self) -> Option<SexpList<'_>> {
        match &self.kind {
            SexpKind::Atom(_) => None,
            SexpKind::List(items) => Some(SexpList::new(items, self.span)),
        }
    }
}

// -----------------------------------------------------------------------------
// SexpList: Provides validated list access.
// -----------------------------------------------------------------------------

/// Two list regions separated by a required atom.
pub(super) struct SexpListSplit<'a> {
    /// Items preceding the separator.
    pub(super) before: SexpList<'a>,
    /// Items following the separator.
    pub(super) after: SexpList<'a>,
}

/// Represents `SexpList` state within this module.
#[derive(Debug, Clone, Copy)]
pub(super) struct SexpList<'a> {
    /// Borrowed child nodes; list helpers never allocate unless lowering does.
    items: &'a [Sexp],
    /// Parent list span used when a required child is missing.
    span: Span,
}

impl<'a> SexpList<'a> {
    /// Read one S-expression as a list.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ScriptExpected`] when the S-expression is an atom.
    pub(super) fn from_sexp(sexp: &'a Sexp, expected: &'static str) -> Result<Self, AppError> {
        sexp.list().ok_or(sexp.span.expected(expected))
    }

    /// Performs the new operation for this abstraction.
    fn new(items: &'a [Sexp], span: Span) -> Self {
        Self { items, span }
    }

    /// Return all expressions in this validated list.
    pub(super) fn as_slice(self) -> &'a [Sexp] {
        self.items
    }

    /// Performs the atom at operation for this abstraction.
    pub(super) fn atom_at(self, index: usize) -> Option<&'a str> {
        self.items.get(index).and_then(Sexp::atom)
    }

    /// Return the list item at `index`.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ScriptExpected`] when the requested item is missing.
    pub(super) fn expect(self, index: usize, expected: &'static str) -> Result<&'a Sexp, AppError> {
        self.items.get(index).ok_or(self.span.expected(expected))
    }

    /// Return the list item at `index` as an atom.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ScriptExpected`] when the item is missing or is a
    /// nested list.
    pub(super) fn expect_atom(
        self,
        index: usize,
        expected: &'static str,
    ) -> Result<&'a str, AppError> {
        let item = self.expect(index, expected)?;
        item.atom().ok_or(item.span.expected(expected))
    }

    /// Performs the tail operation for this abstraction.
    pub(super) fn tail(self, index: usize) -> Self {
        Self::new(self.items.get(index..).unwrap_or_default(), self.span)
    }

    /// Performs the get operation for this abstraction.
    pub(super) fn get(self, index: usize) -> Option<&'a Sexp> {
        self.items.get(index)
    }

    /// Performs the iter operation for this abstraction.
    pub(super) fn iter(self) -> impl Iterator<Item = &'a Sexp> + 'a {
        self.items.iter()
    }

    /// Performs the atoms operation for this abstraction.
    pub(super) fn atoms(self) -> impl Iterator<Item = &'a str> + 'a {
        self.items.iter().filter_map(Sexp::atom)
    }

    /// Performs the split once atom operation for this abstraction.
    pub(super) fn split_once_atom(self, atom: &str) -> Option<SexpListSplit<'a>> {
        let separator = self
            .items
            .iter()
            .position(|item| item.atom() == Some(atom))?;
        Some(SexpListSplit {
            before: Self::new(&self.items[..separator], self.span),
            after: Self::new(&self.items[separator + 1..], self.span),
        })
    }
}

// -----------------------------------------------------------------------------
