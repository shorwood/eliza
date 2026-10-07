//! Native SLIP bridge used by the reconstructed MAD driver.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use mad::machine::Host;
use mad::word::Word;
use slip::arena::{Arena, ArenaError, Datum, ListHandle};
use slip::bcd::{BcdError, BcdWord};
use slip::pattern::{AssemblyError, AssemblyItem, MatchError, assemble, match_pattern};
use thiserror::Error;

use crate::script::{KeywordRule, Reassembly, Script};

// -----------------------------------------------------------------------------
// Trace: Records the mechanical path that produced one response.
// -----------------------------------------------------------------------------

/// Final rule coordinates selected during one turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TraceRule {
    /// Canonical keyword whose rule produced the response.
    pub(crate) keyword: String,
    /// Zero-based decomposition position.
    pub(crate) decomposition: usize,
    /// Zero-based reassembly position.
    pub(crate) reassembly: usize,
}

/// Mechanical response source selected during one turn.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum TraceSource {
    /// A ranked keyword rule produced the response.
    Keyword,
    /// A queued memory produced the response.
    Memory,
    /// The script's `NONE` rule or historical fallback produced the response.
    #[default]
    None,
}

/// Mutable trace facts captured directly by native operations.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TraceState {
    /// Canonical input words after substitutions and delimiter handling.
    pub(crate) normalized_input: Vec<String>,
    /// Keywords in the order the driver will inspect them.
    pub(crate) ranked_keywords: Vec<String>,
    /// Final rule that assembled response text.
    pub(crate) selected_rule: Option<TraceRule>,
    /// Branch that produced the response.
    pub(crate) source: TraceSource,
}

// -----------------------------------------------------------------------------
// Resource: Stores values referenced by MAD machine words.
// -----------------------------------------------------------------------------

/// Native value stored behind a one-based MAD handle.
enum Resource {
    /// SLIP list header.
    List(
        /// Arena list identity.
        ListHandle,
    ),
    /// Interned or input text.
    Text(
        /// Owned text value.
        String,
    ),
    /// Tokenized or assembled word list.
    Words(
        /// Words in source order.
        Vec<String>,
    ),
}

// -----------------------------------------------------------------------------
// KeyList: Retains the reusable keyword-stack list and resource handle.
// -----------------------------------------------------------------------------

/// Reusable SLIP keyword stack exposed to MAD.
#[derive(Clone, Copy)]
struct KeyList {
    /// MAD resource handle.
    handle: Word,
    /// Arena list identity.
    list: ListHandle,
}

/// Edge of the reusable keyword list selected by a native call.
#[derive(Clone, Copy)]
enum KeyListEnd {
    /// Tail of the list.
    Back,
    /// Head of the list.
    Front,
}

// -----------------------------------------------------------------------------
// NativeError: Declares failures at the MAD-to-SLIP boundary.
// -----------------------------------------------------------------------------

/// Native SLIP bridge failure.
#[derive(Debug, Error)]
pub(crate) enum NativeError {
    /// A native function received the wrong number of arguments.
    #[error("expected {expected} arguments, received {actual}")]
    Arity {
        /// Number of arguments received.
        actual: usize,
        /// Number of arguments required.
        expected: usize,
    },
    /// A SLIP arena operation failed.
    #[error(transparent)]
    Arena {
        /// Underlying arena failure.

        #[from]
        source: ArenaError,
    },
    /// A BCD conversion failed.
    #[error(transparent)]
    Bcd {
        /// Underlying BCD failure.

        #[from]
        source: BcdError,
    },
    /// A response capture could not be assembled.
    #[error(transparent)]
    Assembly {
        /// Underlying assembly failure.

        #[from]
        source: AssemblyError,
    },
    /// A word-list prefix exceeds the available words.
    #[error("DROPTO position is outside the word list")]
    DropPosition,
    /// No queued memory response is available.
    #[error("memory queue is empty")]
    EmptyMemoryQueue,
    /// A resource has the wrong type for a list operation.
    #[error("expected a SLIP list")]
    ExpectedList,
    /// A resource has the wrong type for a text operation.
    #[error("expected text")]
    ExpectedText,
    /// A resource has the wrong type for a word-list operation.
    #[error("expected a word list")]
    ExpectedWords,
    /// A word position exceeds the available words.
    #[error("GET position is outside the word list")]
    GetPosition,
    /// A keyword stack unexpectedly contains a nested list.
    #[error("keyword stack contains a nested list")]
    KeywordStackNestedList,
    /// A list resource cannot be rendered as terminal text.
    #[error("a SLIP list cannot be printed as text")]
    ListOutput,
    /// The memory hash selected no transformation.
    #[error("memory transformation {index} is missing")]
    MissingMemoryTransformation {
        /// Missing hash-table index.
        index: usize,
    },
    /// A bounded pattern match exhausted its step budget.
    #[error(transparent)]
    Pattern {
        /// Underlying matcher failure.

        #[from]
        source: MatchError,
    },
    /// A one-based position was zero, negative, or unrepresentable.
    #[error("position must be positive")]
    PositivePosition,
    /// A machine word does not identify an allocated resource.
    #[error("invalid resource handle")]
    ResourceHandle,
    /// The host cannot represent another resource index.
    #[error("session resource index overflow")]
    ResourceIndexOverflow,
    /// The session exhausted its resource budget.
    #[error("session resource limit exhausted")]
    ResourceLimit,
    /// A selected reassembly disappeared during rule application.
    #[error("selected reassembly disappeared")]
    SelectedReassemblyMissing,
    /// A word position cannot be replaced.
    #[error("SET position is outside the word list")]
    SetPosition,
    /// A truncation boundary exceeds the available words.
    #[error("TRUNC position is outside the word list")]
    TruncatePosition,
    /// A native dispatcher received an unsupported function name.
    #[error("unknown native function {name}")]
    UnknownFunction {
        /// Unsupported canonical function name.
        name: String,
    },
    /// An interned text resource does not name a keyword rule.
    #[error("unknown keyword {keyword}")]
    UnknownKeyword {
        /// Unknown canonical keyword.
        keyword: String,
    },
    /// A word-list length cannot be represented by a MAD integer.
    #[error("word list is too long")]
    WordListTooLong,
}

// -----------------------------------------------------------------------------
// ElizaHost: Bridges the reconstructed MAD driver to native SLIP behavior.
// -----------------------------------------------------------------------------

/// Native state exposed to the interpreted MAD driver.
pub(crate) struct ElizaHost {
    /// Bounded SLIP list arena.
    arena: Arena,
    /// Stable resource handle for the current input record.
    input: Word,
    /// Text values mapped to stable resource handles.
    interned: HashMap<String, Word>,
    /// Reusable keyword-stack list.
    key_list: Option<KeyList>,
    /// Most recently assembled response resource.
    last_result: Word,
    /// Most recent linked keyword resource.
    last_target: Word,
    /// Queued memory responses.
    memories: VecDeque<Vec<String>>,
    /// Next response template for each compiled decomposition.
    next_reassemblies: Vec<usize>,
    /// Values addressable from MAD words.
    resources: Vec<Resource>,
    /// Shared immutable DOCTOR rules.
    script: Arc<Script>,
    /// Mechanical facts captured for the current turn.
    trace: TraceState,
}

impl ElizaHost {
    /// Native return code for a completed response.
    const ACTION_COMPLETE: i64 = 1;

    /// Native return code for a link to another keyword rule.
    const ACTION_LINK: i64 = 3;

    /// Native return code requesting the next keyword.
    const ACTION_NEWKEY: i64 = 2;

    /// Maximum recursive steps in one SLIP-style pattern match.
    const MATCH_LIMIT: usize = 100_000;

    /// Maximum resources retained by one conversation session.
    const RESOURCE_LIMIT: usize = 100_000;

    /// Maximum direct cells retained by the SLIP arena.
    const SLIP_CELL_LIMIT: usize = 4_096;

    /// Validate the argument count for one native call.
    ///
    /// # Errors
    ///
    /// Returns an error when the received count differs from `expected`.
    fn exact_arity(arguments: &[Word], expected: usize) -> Result<(), NativeError> {
        if arguments.len() == expected {
            Ok(())
        } else {
            Err(NativeError::Arity {
                actual: arguments.len(),
                expected,
            })
        }
    }

    /// Convert a positive MAD integer to a host index or count.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is zero, negative, or outside `usize`.
    fn one_based(value: Word) -> Result<usize, NativeError> {
        let value = usize::try_from(value.to_i64()).map_err(|_| NativeError::PositivePosition)?;
        if value == 0 {
            Err(NativeError::PositivePosition)
        } else {
            Ok(value)
        }
    }

    /// Split an input record into words and historical punctuation delimiters.
    fn tokenize(input: &str) -> Vec<String> {
        let mut result = Vec::new();
        let mut word = String::new();
        for character in input.chars() {
            match character {
                ',' | '.' => {
                    let completed = std::mem::take(&mut word);
                    result.extend((!completed.is_empty()).then_some(completed));
                    result.push(character.to_string());
                }
                character if character.is_whitespace() => {
                    let completed = std::mem::take(&mut word);
                    result.extend((!completed.is_empty()).then_some(completed));
                }
                character => word.push(character),
            }
        }
        result.extend((!word.is_empty()).then_some(word));
        result
    }

    /// Convert a positive MAD handle to a zero-based resource index.
    ///
    /// # Errors
    ///
    /// Returns an error for a non-positive or out-of-range handle.
    fn resource_index(handle: Word) -> Result<usize, NativeError> {
        let value = usize::try_from(handle.to_i64()).map_err(|_| NativeError::ResourceHandle)?;
        value.checked_sub(1).ok_or(NativeError::ResourceHandle)
    }

    /// Borrow the mechanical facts captured for the current turn.
    pub(crate) fn trace_state(&self) -> &TraceState {
        &self.trace
    }

    /// Allocate one resource and return its one-based MAD handle.
    ///
    /// # Errors
    ///
    /// Returns an error when the session bound or host integer range is exceeded.
    fn allocate(&mut self, resource: Resource) -> Result<Word, NativeError> {
        // Resource growth is bounded independently from SLIP cells.
        if self.resources.len() >= Self::RESOURCE_LIMIT {
            return Err(NativeError::ResourceLimit);
        }
        self.resources.push(resource);
        let index =
            i64::try_from(self.resources.len()).map_err(|_| NativeError::ResourceIndexOverflow)?;
        Ok(Word::from_i64(index))
    }

    /// Intern text as a stable resource handle.
    ///
    /// # Errors
    ///
    /// Returns an error when allocating a new resource fails.
    fn intern(&mut self, text: &str) -> Result<Word, NativeError> {
        // Existing interned text preserves handle identity.
        if let Some(handle) = self.interned.get(text) {
            return Ok(*handle);
        }
        let owned = text.to_owned();
        let handle = self.allocate(Resource::Text(owned.clone()))?;
        self.interned.insert(owned, handle);
        Ok(handle)
    }

    /// Create a native host with fresh memory, resources, and keyword counters.
    ///
    /// # Errors
    ///
    /// Returns an error when initial resource allocation fails.
    pub(crate) fn new(script: Arc<Script>) -> Result<Self, NativeError> {
        let mut host = Self {
            arena: Arena::new(Self::SLIP_CELL_LIMIT),
            input: Word::ZERO,
            interned: HashMap::new(),
            key_list: None,
            last_result: Word::ZERO,
            last_target: Word::ZERO,
            memories: VecDeque::new(),
            next_reassemblies: vec![0; script.counter_count],
            resources: Vec::new(),
            script,
            trace: TraceState::default(),
        };
        host.input = host.allocate(Resource::Text(String::new()))?;
        for keyword in host.script.rules.keys().cloned().collect::<Vec<_>>() {
            host.intern(&keyword)?;
        }
        Ok(host)
    }

    /// Generate and queue a memory response when the keyword permits it.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid BCD, missing rules, or bounded matching.
    fn make_memory(&mut self, keyword: &str, words: &[String]) -> Result<(), NativeError> {
        // Only the script's memory keyword participates in memory generation.
        if keyword != self.script.memory.keyword {
            return Ok(());
        }

        // Empty input has no final word to hash.
        let Some(last_word) = words.last() else {
            return Ok(());
        };

        // Select one memory transformation with the historical two-bit hash.
        let bcd = BcdWord::last_chunk(last_word)?;
        let index = usize::from(bcd.hash(2));
        let transformations = &self.script.memory.decompositions;
        let transformation = transformations
            .get(index)
            .ok_or(NativeError::MissingMemoryTransformation { index })?;

        // Borrow tag membership used by grouped pattern items.
        let tags = &self.script.tags;

        // Match the selected transformation against the normalized input.
        let captures = match_pattern(
            &transformation.pattern,
            words,
            |tag, word| tags.get(tag).is_some_and(|set| set.contains(word)),
            Self::MATCH_LIMIT,
        )?;

        // Assemble and queue only a successful memory match.
        if let Some(captures) = captures {
            let memory = assemble(&transformation.reassembly, &captures)?;
            self.memories.push_back(memory);
        }
        Ok(())
    }

    /// Return the reusable empty keyword-stack list.
    ///
    /// # Errors
    ///
    /// Returns an error when clearing or allocating the list fails.
    fn new_list(&mut self) -> Result<Word, NativeError> {
        // Reuse the prior list resource while clearing its cells.
        if let Some(key_list) = self.key_list {
            self.arena.clear(key_list.list)?;
            return Ok(key_list.handle);
        }
        let list = self.arena.list();
        let handle = self.allocate(Resource::List(list))?;
        self.key_list = Some(KeyList { handle, list });
        Ok(handle)
    }

    /// Resolve an immutable resource handle.
    ///
    /// # Errors
    ///
    /// Returns an error for a non-positive or unallocated handle.
    fn resource(&self, handle: Word) -> Result<&Resource, NativeError> {
        let index = Self::resource_index(handle)?;
        self.resources.get(index).ok_or(NativeError::ResourceHandle)
    }

    /// Convert a response resource into display text.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid handle or a SLIP-list resource.
    pub(crate) fn output(&self, handle: Word) -> Result<String, NativeError> {
        match self.resource(handle)? {
            Resource::Text(text) => Ok(text.clone()),
            Resource::Words(words) => Ok(words.join(" ")),
            Resource::List(_) => Err(NativeError::ListOutput),
        }
    }

    /// Resolve a resource handle as a SLIP list.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid handle or another resource kind.
    fn list(&self, handle: Word) -> Result<ListHandle, NativeError> {
        match self.resource(handle)? {
            Resource::List(list) => Ok(*list),
            Resource::Text(_) | Resource::Words(_) => Err(NativeError::ExpectedList),
        }
    }

    /// Resolve a mutable resource handle.
    ///
    /// # Errors
    ///
    /// Returns an error for a non-positive or unallocated handle.
    fn resource_mut(&mut self, handle: Word) -> Result<&mut Resource, NativeError> {
        let index = Self::resource_index(handle)?;
        self.resources
            .get_mut(index)
            .ok_or(NativeError::ResourceHandle)
    }

    /// Replace the stable input resource with one terminal record.
    ///
    /// # Errors
    ///
    /// Returns an error if the stable input handle is invalid.
    pub(crate) fn store_input(&mut self, input: &str) -> Result<Word, NativeError> {
        self.trace = TraceState::default();
        let input_handle = self.input;
        *self.resource_mut(input_handle)? = Resource::Text(input.to_owned());
        Ok(input_handle)
    }

    /// Resolve a resource as text.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid handle or another resource kind.
    fn text(&self, handle: Word) -> Result<&str, NativeError> {
        match self.resource(handle)? {
            Resource::Text(text) => Ok(text),
            Resource::List(_) | Resource::Words(_) => Err(NativeError::ExpectedText),
        }
    }

    /// Resolve a keyword rule from an interned-text handle.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid text or an unknown keyword.
    fn rule(&self, handle: Word) -> Result<&KeywordRule, NativeError> {
        let keyword = self.text(handle)?;
        self.script
            .rules
            .get(keyword)
            .ok_or_else(|| NativeError::UnknownKeyword {
                keyword: keyword.to_owned(),
            })
    }

    /// Resolve a resource as an immutable word list.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid handle or another resource kind.
    fn words(&self, handle: Word) -> Result<&[String], NativeError> {
        match self.resource(handle)? {
            Resource::Words(words) => Ok(words),
            Resource::List(_) | Resource::Text(_) => Err(NativeError::ExpectedWords),
        }
    }

    /// Resolve a resource as a mutable word list.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid handle or another resource kind.
    fn words_mut(&mut self, handle: Word) -> Result<&mut Vec<String>, NativeError> {
        match self.resource_mut(handle)? {
            Resource::Words(words) => Ok(words),
            Resource::List(_) | Resource::Text(_) => Err(NativeError::ExpectedWords),
        }
    }

    /// Assemble and record the final rule-selected response.
    ///
    /// # Errors
    ///
    /// Returns an error when assembly or resource allocation fails.
    fn complete_rule(
        &mut self,
        rule: TraceRule,
        items: &[AssemblyItem],
        captures: &[Vec<String>],
    ) -> Result<i64, NativeError> {
        let result = assemble(items, captures)?;
        self.last_result = self.allocate(Resource::Words(result))?;
        self.trace.source = if rule.keyword == "NONE" {
            TraceSource::None
        } else {
            TraceSource::Keyword
        };
        self.trace.selected_rule = Some(rule);
        Ok(Self::ACTION_COMPLETE)
    }

    /// Apply the selected keyword rule to a tokenized input resource.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid handles, pattern bounds, or malformed rules.
    fn apply(&mut self, keyword: &str, words_handle: Word) -> Result<i64, NativeError> {
        let words = self.words(words_handle)?.to_vec();
        let tags = &self.script.tags;

        // Unknown keywords have no transformation.
        let Some(rule) = self.script.rules.get(keyword) else {
            return Ok(0);
        };

        let mut selected = None;
        for (index, decomposition) in rule.decompositions.iter().enumerate() {
            if let Some(captures) = match_pattern(
                &decomposition.pattern,
                &words,
                |tag, word| tags.get(tag).is_some_and(|members| members.contains(word)),
                Self::MATCH_LIMIT,
            )? {
                selected = Some((index, decomposition, captures));
                break;
            }
        }

        // A rule without a matching decomposition may still link elsewhere.
        let Some((decomposition_index, decomposition, captures)) = selected else {
            // A rule-level fallback delegates processing to its target.
            if let Some(target) = rule.fallback.clone() {
                self.last_target = self.intern(&target)?;
                return Ok(Self::ACTION_LINK);
            }
            return Ok(0);
        };

        // Select a response using this session's counter, leaving definitions immutable.
        let counter = &mut self.next_reassemblies[decomposition.counter_index];
        let reassembly_index = *counter;
        let reassembly = decomposition
            .reassemblies
            .get(reassembly_index)
            .cloned()
            .ok_or(NativeError::SelectedReassemblyMissing)?;

        // Advance the cycle even when the selected action links, yields, or fails.
        *counter = (*counter + 1) % decomposition.reassemblies.len();
        let selected_rule = TraceRule {
            keyword: keyword.to_owned(),
            decomposition: decomposition_index,
            reassembly: reassembly_index,
        };

        match reassembly {
            Reassembly::Link { keyword } => {
                self.last_target = self.intern(&keyword)?;
                Ok(Self::ACTION_LINK)
            }
            Reassembly::NewKey => Ok(Self::ACTION_NEWKEY),
            Reassembly::Pre { link, words } => {
                let replacement = assemble(&words, &captures)?;
                *self.words_mut(words_handle)? = replacement;
                self.last_target = self.intern(&link)?;
                Ok(Self::ACTION_LINK)
            }
            Reassembly::Words { items } => self.complete_rule(selected_rule, &items, &captures),
        }
    }
}

impl ElizaHost {
    /// Pop and mark one queued memory response.
    ///
    /// # Errors
    ///
    /// Returns an error when the queue is empty or allocation fails.
    fn pop_memory(&mut self) -> Result<Word, NativeError> {
        let memory = self
            .memories
            .pop_front()
            .ok_or(NativeError::EmptyMemoryQueue)?;
        self.trace.selected_rule = None;
        self.trace.source = TraceSource::Memory;
        self.allocate(Resource::Words(memory))
    }

    /// Dispatch memory-queue native functions.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid arguments, handles, or empty memory.
    fn call_memory_native(
        &mut self,
        name: &str,
        arguments: &mut [Word],
    ) -> Result<Word, NativeError> {
        match name {
            "MEMHAS" => {
                Self::exact_arity(arguments, 0)?;
                Ok(Word::from_i64(i64::from(!self.memories.is_empty())))
            }
            "MEMMAKE" => {
                Self::exact_arity(arguments, 2)?;
                let keyword = self.text(arguments[0])?.to_owned();
                let words = self.words(arguments[1])?.to_vec();
                self.make_memory(&keyword, &words)?;
                Ok(Word::ZERO)
            }
            "MEMPOP" => {
                Self::exact_arity(arguments, 0)?;
                self.pop_memory()
            }
            _ => Err(NativeError::UnknownFunction {
                name: name.to_owned(),
            }),
        }
    }

    /// Remove a one-based prefix from a word list.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid arguments, handles, or positions.
    fn drop_to(&mut self, arguments: &[Word]) -> Result<Word, NativeError> {
        Self::exact_arity(arguments, 2)?;
        let count = Self::one_based(arguments[1])?;
        let normalized = {
            let words = self.words_mut(arguments[0])?;

            // The removed prefix must fit in the word list.
            if count > words.len() {
                return Err(NativeError::DropPosition);
            }
            words.drain(..count);
            words.clone()
        };
        self.trace.normalized_input = normalized;
        Ok(arguments[0])
    }

    /// Intern the word at a one-based list position.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid arguments, handles, or positions.
    fn get_word(&mut self, arguments: &[Word]) -> Result<Word, NativeError> {
        Self::exact_arity(arguments, 2)?;
        let index = Self::one_based(arguments[1])? - 1;
        let word = self
            .words(arguments[0])?
            .get(index)
            .cloned()
            .ok_or(NativeError::GetPosition)?;
        self.intern(&word)
    }

    /// Pop one raw word from the reusable keyword stack.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid arguments, handles, or list contents.
    fn pop_front(&mut self, arguments: &[Word]) -> Result<Word, NativeError> {
        Self::exact_arity(arguments, 1)?;
        let list = self.list(arguments[0])?;
        let datum = self.arena.pop_front(list)?;
        match datum {
            Datum::Word(raw) => Ok(Word::from_raw(raw)),
            Datum::List(_) => Err(NativeError::KeywordStackNestedList),
        }
    }

    /// Push one raw word onto the selected keyword-stack edge.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid arguments, handles, or exhausted storage.
    fn push(&mut self, arguments: &[Word], end: KeyListEnd) -> Result<Word, NativeError> {
        Self::exact_arity(arguments, 2)?;
        let list = self.list(arguments[0])?;
        let keyword = self.text(arguments[1])?.to_owned();
        let datum = Datum::Word(arguments[1].raw());
        let result = match end {
            KeyListEnd::Back => self.arena.push_back(list, datum),
            KeyListEnd::Front => self.arena.push_front(list, datum),
        };
        result?;
        match end {
            KeyListEnd::Back => self.trace.ranked_keywords.push(keyword),
            KeyListEnd::Front => self.trace.ranked_keywords.insert(0, keyword),
        }
        Ok(arguments[0])
    }

    /// Dispatch SLIP-list native functions.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid arguments, handles, or list operations.
    fn call_list_native(
        &mut self,
        name: &str,
        arguments: &mut [Word],
    ) -> Result<Word, NativeError> {
        match name {
            "EMPTY" => {
                Self::exact_arity(arguments, 1)?;
                let list = self.list(arguments[0])?;
                let is_empty = self.arena.is_empty(list)?;
                Ok(Word::from_i64(i64::from(is_empty)))
            }
            "NEWLST" => {
                Self::exact_arity(arguments, 0)?;
                self.new_list()
            }
            "POPF" => self.pop_front(arguments),
            "PUSHB" => self.push(arguments, KeyListEnd::Back),
            "PUSHF" => self.push(arguments, KeyListEnd::Front),
            _ => Err(NativeError::UnknownFunction {
                name: name.to_owned(),
            }),
        }
    }

    /// Replace the word at a one-based list position.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid arguments, handles, or positions.
    fn set_word(&mut self, arguments: &[Word]) -> Result<Word, NativeError> {
        Self::exact_arity(arguments, 3)?;
        let index = Self::one_based(arguments[1])? - 1;
        let replacement = self.text(arguments[2])?.to_owned();
        let slot = self
            .words_mut(arguments[0])?
            .get_mut(index)
            .ok_or(NativeError::SetPosition)?;
        *slot = replacement;
        self.trace.normalized_input = self.words(arguments[0])?.to_vec();
        Ok(arguments[0])
    }

    /// Intern a rule's substitution or its own keyword.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid arguments, handles, or resource exhaustion.
    fn substitution(&mut self, arguments: &[Word]) -> Result<Word, NativeError> {
        Self::exact_arity(arguments, 1)?;
        let replacement = {
            let rule = self.rule(arguments[0])?;
            rule.substitution
                .as_deref()
                .unwrap_or(&rule.keyword)
                .to_owned()
        };
        self.intern(&replacement)
    }

    /// Dispatch keyword-rule native functions.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid arguments, handles, or rule data.
    fn call_rule_native(
        &mut self,
        name: &str,
        arguments: &mut [Word],
    ) -> Result<Word, NativeError> {
        match name {
            "APPLY" => {
                Self::exact_arity(arguments, 2)?;
                let keyword = self.text(arguments[0])?.to_owned();
                Ok(Word::from_i64(self.apply(&keyword, arguments[1])?))
            }
            "GREET" => {
                Self::exact_arity(arguments, 0)?;
                let greeting = self.script.greeting.clone();
                self.intern(&greeting)
            }
            "HASTRN" => {
                Self::exact_arity(arguments, 1)?;
                Ok(Word::from_i64(i64::from(
                    self.rule(arguments[0])?.has_transformation(),
                )))
            }
            "NONE" => {
                Self::exact_arity(arguments, 0)?;
                self.intern("NONE")
            }
            "PREC" => {
                Self::exact_arity(arguments, 1)?;
                Ok(Word::from_i64(self.rule(arguments[0])?.precedence))
            }
            "RESULT" => {
                Self::exact_arity(arguments, 0)?;
                Ok(self.last_result)
            }
            "RULE" => {
                Self::exact_arity(arguments, 1)?;
                let exists = self.script.rules.contains_key(self.text(arguments[0])?);
                Ok(if exists { arguments[0] } else { Word::ZERO })
            }
            "SUBST" => self.substitution(arguments),
            "TARGET" => {
                Self::exact_arity(arguments, 0)?;
                Ok(self.last_target)
            }
            _ => Err(NativeError::UnknownFunction {
                name: name.to_owned(),
            }),
        }
    }

    /// Truncate a word list before a one-based position.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid arguments, handles, or positions.
    fn truncate_words(&mut self, arguments: &[Word]) -> Result<Word, NativeError> {
        Self::exact_arity(arguments, 2)?;
        let position = Self::one_based(arguments[1])? - 1;
        let normalized = {
            let words = self.words_mut(arguments[0])?;

            // The truncation boundary may equal but not exceed the list length.
            if position > words.len() {
                return Err(NativeError::TruncatePosition);
            }
            words.truncate(position);
            words.clone()
        };
        self.trace.normalized_input = normalized;
        Ok(arguments[0])
    }

    /// Dispatch word-list native functions.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid arguments, handles, or positions.
    fn call_word_native(
        &mut self,
        name: &str,
        arguments: &mut [Word],
    ) -> Result<Word, NativeError> {
        match name {
            "DELIM" => {
                Self::exact_arity(arguments, 1)?;
                let delimiter = matches!(self.text(arguments[0])?, "," | "." | "BUT");
                Ok(Word::from_i64(i64::from(delimiter)))
            }
            "DROPTO" => self.drop_to(arguments),
            "GET" => self.get_word(arguments),
            "LENGTH" => {
                Self::exact_arity(arguments, 1)?;
                let length = i64::try_from(self.words(arguments[0])?.len())
                    .map_err(|_| NativeError::WordListTooLong)?;
                Ok(Word::from_i64(length))
            }
            "SET" => self.set_word(arguments),
            "TOKEN" => {
                Self::exact_arity(arguments, 1)?;
                let words = Self::tokenize(self.text(arguments[0])?);
                self.trace.normalized_input.clone_from(&words);
                self.allocate(Resource::Words(words))
            }
            "TRUNC" => self.truncate_words(arguments),
            _ => Err(NativeError::UnknownFunction {
                name: name.to_owned(),
            }),
        }
    }
}

impl Host for ElizaHost {
    type Error = NativeError;

    fn call(&mut self, name: &str, arguments: &mut [Word]) -> Result<Word, Self::Error> {
        match name {
            "APPLY" | "GREET" | "HASTRN" | "NONE" | "PREC" | "RESULT" | "RULE" | "SUBST"
            | "TARGET" => self.call_rule_native(name, arguments),
            "DELIM" | "DROPTO" | "GET" | "LENGTH" | "SET" | "TOKEN" | "TRUNC" => {
                self.call_word_native(name, arguments)
            }
            "MEMHAS" | "MEMMAKE" | "MEMPOP" => self.call_memory_native(name, arguments),
            "EMPTY" | "NEWLST" | "POPF" | "PUSHB" | "PUSHF" => {
                self.call_list_native(name, arguments)
            }
            _ => Err(NativeError::UnknownFunction {
                name: name.to_owned(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::ElizaHost;
    use crate::script::Script;

    /// Hosts share definitions while owning fresh response counters.
    ///
    /// # Panics
    /// Panics if parsing, initialization, or ownership changes.
    #[test]
    fn hosts_share_rules_and_own_response_counters() {
        let script = Arc::new(
            include_str!("../programs/1966/doctor.script")
                .parse::<Script>()
                .unwrap(),
        );
        let mut first = ElizaHost::new(Arc::clone(&script)).unwrap();
        let second = ElizaHost::new(Arc::clone(&script)).unwrap();
        assert!(Arc::ptr_eq(&first.script, &second.script));
        let index = script.rules["I"].decompositions[0].counter_index;
        first.next_reassemblies[index] = 1;
        assert_eq!(second.next_reassemblies[index], 0);
    }

    /// Splits words without treating delimiter substrings as punctuation.
    ///
    /// # Panics
    ///
    /// Panics when tokenization differs from the historical delimiter contract.
    #[test]
    fn tokenizes_historical_delimiters() {
        assert_eq!(
            ElizaHost::tokenize("ONE ABUTMENT, BUT THREE."),
            ["ONE", "ABUTMENT", ",", "BUT", "THREE", "."]
        );
    }
}
