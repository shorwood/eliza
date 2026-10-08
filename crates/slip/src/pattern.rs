//! SLIP-style pattern matching and list assembly.

use thiserror::Error;

// -----------------------------------------------------------------------------
// Assembly: Describes output parts and reports missing captures.
// -----------------------------------------------------------------------------

/// One element in an assembled word list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssemblyItem {
    /// Copy a captured pattern element.
    Capture(
        /// Zero-based capture index.
        usize,
    ),
    /// Insert a literal word.
    Word(
        /// Literal output word.
        String,
    ),
}

/// Failure while assembling a word list.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("capture {capture} is outside the match")]
pub struct AssemblyError {
    /// One-based missing capture number.
    capture: usize,
}

/// Assemble literal words and captured pattern elements.
///
/// ```
/// use slip::pattern::{AssemblyItem, assemble};
///
/// let items = [AssemblyItem::Word("WHY".into()), AssemblyItem::Capture(1)];
/// let captures = vec![vec!["I".into()], vec!["CARE".into(), "NOW".into()]];
/// assert_eq!(assemble(&items, &captures).unwrap(), ["WHY", "CARE", "NOW"]);
/// ```
///
/// # Errors
///
/// Returns an error when an item names a capture that is not present.
pub fn assemble(
    items: &[AssemblyItem],
    captures: &[Vec<String>],
) -> Result<Vec<String>, AssemblyError> {
    let mut result = Vec::new();
    for item in items {
        match item {
            AssemblyItem::Capture(index) => {
                let capture = captures.get(*index).ok_or(AssemblyError {
                    capture: *index + 1,
                })?;
                result.extend(capture.iter().cloned());
            }
            AssemblyItem::Word(word) => result.push(word.clone()),
        }
    }
    Ok(result)
}

// -----------------------------------------------------------------------------
// Pattern: Matches bounded word sequences against the historical item forms.
// -----------------------------------------------------------------------------

/// One element in a word-list pattern.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PatternItem {
    /// Match one word from this set.
    Alternatives(
        /// Accepted literal words.
        Vec<String>,
    ),
    /// Match exactly this many words.
    Fixed(
        /// Number of words to capture.
        usize,
    ),
    /// Match one word belonging to any of these tags.
    Tags(
        /// Accepted tag names.
        Vec<String>,
    ),
    /// Match the shortest word sequence that lets the rest match.
    Variable,
    /// Match one literal word.
    Word(
        /// Required literal word.
        String,
    ),
}

/// Failure while matching a word list.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("pattern-match step limit exhausted")]
pub struct MatchError;

/// Pattern and input positions for one recursive matching step.
#[derive(Clone, Copy, Default)]
struct MatcherCursor {
    /// Index of the current pattern item.
    pattern: usize,
    /// Index of the next input word.
    word: usize,
}

/// Bounded recursive matcher over one pattern and input list.
struct Matcher<'input, F> {
    /// Caller-supplied tag-membership predicate.
    has_tag: &'input F,
    /// Complete pattern being matched.
    pattern: &'input [PatternItem],
    /// Recursive steps still available.
    remaining: usize,
    /// Complete input word list.
    words: &'input [String],
}

impl<F> Matcher<'_, F>
where
    F: Fn(&str, &str) -> bool,
{
    /// Capture one item and continue at the next pattern position.
    ///
    /// # Errors
    ///
    /// Returns an error when the recursive-step bound is exhausted.
    fn capture(
        &mut self,
        cursor: MatcherCursor,
        length: usize,
    ) -> Result<Option<Vec<Vec<String>>>, MatchError> {
        let end = cursor.word.saturating_add(length);

        // A fixed capture cannot extend beyond the available input.
        if end > self.words.len() {
            return Ok(None);
        }
        let next = MatcherCursor {
            pattern: cursor.pattern + 1,
            word: end,
        };

        // A capture is useful only when the remaining pattern also matches.
        let Some(mut captures) = self.match_from(next)? else {
            return Ok(None);
        };
        captures.insert(0, self.words[cursor.word..end].to_vec());
        Ok(Some(captures))
    }

    /// Try variable capture lengths from shortest to longest.
    ///
    /// # Errors
    ///
    /// Returns an error when the recursive-step bound is exhausted.
    fn match_variable(
        &mut self,
        cursor: MatcherCursor,
    ) -> Result<Option<Vec<Vec<String>>>, MatchError> {
        let maximum = self.words.len().saturating_sub(cursor.word);
        for length in 0..=maximum {
            let result = self.capture(cursor, length)?;

            // The first successful length preserves ELIZA's shortest match.
            if result.is_some() {
                return Ok(result);
            }
        }
        Ok(None)
    }

    /// Match and capture one word when the predicate accepts it.
    ///
    /// # Errors
    ///
    /// Returns an error when the recursive-step bound is exhausted.
    fn match_one<P>(
        &mut self,
        cursor: MatcherCursor,
        predicate: P,
    ) -> Result<Option<Vec<Vec<String>>>, MatchError>
    where
        P: FnOnce(&String) -> bool,
    {
        // A rejected or absent word cannot satisfy this pattern item.
        if self
            .words
            .get(cursor.word)
            .is_none_or(|word| !predicate(word))
        {
            return Ok(None);
        }
        self.capture(cursor, 1)
    }

    /// Match from one pair of pattern and input positions.
    ///
    /// # Errors
    ///
    /// Returns an error when the recursive-step bound is exhausted.
    fn match_from(
        &mut self,
        cursor: MatcherCursor,
    ) -> Result<Option<Vec<Vec<String>>>, MatchError> {
        self.remaining = self.remaining.checked_sub(1).ok_or(MatchError)?;

        // Exhausting the pattern succeeds only at the end of the input.
        let Some(item) = self.pattern.get(cursor.pattern).cloned() else {
            return Ok((cursor.word == self.words.len()).then(Vec::new));
        };

        // Dispatch the current item to its capture policy.
        match item {
            PatternItem::Alternatives(alternatives) => {
                self.match_one(cursor, |word| alternatives.contains(word))
            }
            PatternItem::Fixed(length) => self.capture(cursor, length),
            PatternItem::Tags(tags) => {
                let has_tag = self.has_tag;
                self.match_one(cursor, |word| tags.iter().any(|tag| has_tag(tag, word)))
            }
            PatternItem::Variable => self.match_variable(cursor),
            PatternItem::Word(expected) => self.match_one(cursor, |word| word == &expected),
        }
    }
}

/// Match a word list with bounded backtracking.
///
/// A variable element tries the shortest capture first, as the 1966 ELIZA
/// algorithm did.
///
/// ```
/// use slip::pattern::{PatternItem, match_pattern};
///
/// let pattern = [PatternItem::Variable, PatternItem::Word("NO".into())];
/// let words = ["ONE", "TWO", "THREE"].map(str::to_owned);
/// assert!(match_pattern(&pattern, &words, |_, _| false, 2).is_err());
/// ```
///
/// # Errors
///
/// Returns an error when matching uses more than the configured recursive steps.
pub fn match_pattern<F>(
    pattern: &[PatternItem],
    words: &[String],
    has_tag: F,
    step_limit: usize,
) -> Result<Option<Vec<Vec<String>>>, MatchError>
where
    F: Fn(&str, &str) -> bool,
{
    Matcher {
        has_tag: &has_tag,
        pattern,
        remaining: step_limit,
        words,
    }
    .match_from(MatcherCursor::default())
}
