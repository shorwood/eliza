//! Bounded safe lists and readers.

use std::collections::HashSet;

use thiserror::Error;

// -----------------------------------------------------------------------------
// ListHandle: Identifies one allocated list header.
// -----------------------------------------------------------------------------

/// Opaque list identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ListHandle(
    /// Arena slot index.
    usize,
);

// -----------------------------------------------------------------------------
// ReaderHandle: Identifies one allocated sequential reader.
// -----------------------------------------------------------------------------

/// Opaque sequential-reader identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ReaderHandle(
    /// Arena slot index.
    usize,
);

// -----------------------------------------------------------------------------
// Datum: Represents one list cell payload.
// -----------------------------------------------------------------------------

/// One SLIP datum.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Datum {
    /// Link to another list.
    List(
        /// Linked list identity.
        ListHandle,
    ),
    /// Uninterpreted 36-bit machine datum.
    Word(
        /// Uninterpreted low-level value.
        u64,
    ),
}

// -----------------------------------------------------------------------------
// Direction: Selects sequential-reader traversal order.
// -----------------------------------------------------------------------------

/// Reader direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    /// Traverse from the first datum toward the last.
    LeftToRight,
    /// Traverse from the last datum toward the first.
    RightToLeft,
}

// -----------------------------------------------------------------------------
// List: Stores one header's direct cells and optional name.
// -----------------------------------------------------------------------------

/// Internal storage for one allocated list.
#[derive(Clone, Debug, Default)]
struct List {
    /// Direct list cells in traversal order.
    items: Vec<Datum>,
    /// Optional DLIST/name datum.
    name: Option<Datum>,
}

// -----------------------------------------------------------------------------
// Reader: Stores one sequential traversal cursor.
// -----------------------------------------------------------------------------

/// Internal state for one allocated reader.
#[derive(Clone, Debug)]
struct Reader {
    /// Traversal order.
    direction: Direction,
    /// List being traversed.
    list: ListHandle,
    /// Next forward index or exclusive reverse index.
    position: usize,
}

impl Reader {
    /// Position a new reader at the selected edge of a list.
    fn at_edge(list: ListHandle, direction: Direction, length: usize) -> Self {
        Self {
            direction,
            list,
            position: if direction == Direction::LeftToRight {
                0
            } else {
                length
            },
        }
    }
}

// -----------------------------------------------------------------------------
// Arena: Owns bounded lists, cells, and readers.
// -----------------------------------------------------------------------------

/// A bounded arena owning lists and traversal cursors.
#[derive(Debug)]
pub struct Arena {
    /// Maximum number of live direct cells.
    cell_limit: usize,
    /// Number of live direct cells.
    cells: usize,
    /// Released list slots available for reuse.
    free_lists: Vec<usize>,
    /// Released reader slots available for reuse.
    free_readers: Vec<usize>,
    /// List slots addressed by handles.
    lists: Vec<Option<List>>,
    /// Reader slots addressed by handles.
    readers: Vec<Option<Reader>>,
}

impl Arena {
    /// Construct an arena with a maximum total number of list cells.
    #[must_use]
    pub const fn new(cell_limit: usize) -> Self {
        Self {
            cell_limit,
            cells: 0,
            free_lists: Vec::new(),
            free_readers: Vec::new(),
            lists: Vec::new(),
            readers: Vec::new(),
        }
    }

    /// Allocate an empty list, reusing a released header when possible.
    pub fn list(&mut self) -> ListHandle {
        // Released headers retain stable slot identities for reuse.
        if let Some(index) = self.free_lists.pop() {
            self.lists[index] = Some(List::default());
            return ListHandle(index);
        }
        let handle = ListHandle(self.lists.len());
        self.lists.push(Some(List::default()));
        handle
    }
}

impl Arena {
    /// Allocate a list populated from the iterator.
    ///
    /// # Errors
    ///
    /// Returns [`ArenaError::Exhausted`] if the cell limit would be exceeded.
    pub fn list_from(
        &mut self,
        items: impl IntoIterator<Item = Datum>,
    ) -> Result<ListHandle, ArenaError> {
        let items = items.into_iter().collect::<Vec<_>>();
        self.reserve(items.len())?;
        let handle = self.list();
        self.cells += items.len();
        self.list_mut(handle)?.items = items;
        Ok(handle)
    }

    /// Remove every datum while retaining the header.
    ///
    /// # Errors
    ///
    /// Returns [`ArenaError::InvalidList`] for a stale handle.
    pub fn clear(&mut self, handle: ListHandle) -> Result<(), ArenaError> {
        let removed = self.list_ref(handle)?.items.len();
        self.list_mut(handle)?.items.clear();
        self.cells -= removed;
        Ok(())
    }

    /// Deep-copy a list and its nested lists.
    ///
    /// # Errors
    ///
    /// Returns an error for stale handles, cycles, or exhausted cells.
    pub fn copy(&mut self, handle: ListHandle) -> Result<ListHandle, ArenaError> {
        self.copy_inner(handle, &mut HashSet::new())
    }

    /// Release a list header and its direct cells for reuse.
    ///
    /// # Errors
    ///
    /// Returns [`ArenaError::InvalidList`] for a stale handle.
    pub fn erase(&mut self, handle: ListHandle) -> Result<(), ArenaError> {
        let list = self
            .lists
            .get_mut(handle.0)
            .and_then(Option::take)
            .ok_or(ArenaError::InvalidList)?;
        self.cells -= list.items.len();
        self.free_lists.push(handle.0);
        Ok(())
    }

    /// Return a datum by zero-based position.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale handle or out-of-range position.
    pub fn get(&self, handle: ListHandle, index: usize) -> Result<Datum, ArenaError> {
        self.list_ref(handle)?
            .items
            .get(index)
            .copied()
            .ok_or(ArenaError::Bounds)
    }

    /// Test whether a list contains no data.
    ///
    /// # Errors
    ///
    /// Returns [`ArenaError::InvalidList`] for a stale handle.
    pub fn is_empty(&self, handle: ListHandle) -> Result<bool, ArenaError> {
        Ok(self.list_ref(handle)?.items.is_empty())
    }

    /// Return the number of direct data cells.
    ///
    /// # Errors
    ///
    /// Returns [`ArenaError::InvalidList`] for a stale handle.
    pub fn len(&self, handle: ListHandle) -> Result<usize, ArenaError> {
        Ok(self.list_ref(handle)?.items.len())
    }

    /// Return a list's optional DLIST/name datum.
    ///
    /// # Errors
    ///
    /// Returns [`ArenaError::InvalidList`] for a stale handle.
    pub fn name(&self, handle: ListHandle) -> Result<Option<Datum>, ArenaError> {
        Ok(self.list_ref(handle)?.name)
    }

    /// Remove and return the last datum.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale or empty list.
    pub fn pop_back(&mut self, handle: ListHandle) -> Result<Datum, ArenaError> {
        let datum = self
            .list_mut(handle)?
            .items
            .pop()
            .ok_or(ArenaError::Empty)?;
        self.cells -= 1;
        Ok(datum)
    }

    /// Remove and return the first datum.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale or empty list.
    pub fn pop_front(&mut self, handle: ListHandle) -> Result<Datum, ArenaError> {
        // Removing index zero requires a nonempty list.
        if self.list_ref(handle)?.items.is_empty() {
            return Err(ArenaError::Empty);
        }
        let datum = self.list_mut(handle)?.items.remove(0);
        self.cells -= 1;
        Ok(datum)
    }

    /// Append one datum.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale handle or exhausted cells.
    pub fn push_back(&mut self, handle: ListHandle, datum: Datum) -> Result<(), ArenaError> {
        self.reserve(1)?;
        self.list_mut(handle)?.items.push(datum);
        self.cells += 1;
        Ok(())
    }

    /// Prepend one datum.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale handle or exhausted cells.
    pub fn push_front(&mut self, handle: ListHandle, datum: Datum) -> Result<(), ArenaError> {
        self.reserve(1)?;
        self.list_mut(handle)?.items.insert(0, datum);
        self.cells += 1;
        Ok(())
    }
}

impl Arena {
    /// Read the next datum and advance a reader.
    ///
    /// # Errors
    ///
    /// Returns an error for stale reader or list handles.
    pub fn read(&mut self, handle: ReaderHandle) -> Result<Option<Datum>, ArenaError> {
        let reader = self.reader_ref(handle)?.clone();
        let list = self.list_ref(reader.list)?;
        let datum = match reader.direction {
            Direction::LeftToRight => list.items.get(reader.position).copied(),
            Direction::RightToLeft => reader
                .position
                .checked_sub(1)
                .and_then(|index| list.items.get(index))
                .copied(),
        };
        if datum.is_some() {
            let reader = self.reader_mut(handle)?;
            match reader.direction {
                Direction::LeftToRight => reader.position += 1,
                Direction::RightToLeft => reader.position -= 1,
            }
        }
        Ok(datum)
    }

    /// Allocate a sequential reader.
    ///
    /// # Errors
    ///
    /// Returns [`ArenaError::InvalidList`] for a stale list handle.
    pub fn reader(
        &mut self,
        list: ListHandle,
        direction: Direction,
    ) -> Result<ReaderHandle, ArenaError> {
        // Reverse readers start at the exclusive end of the list.
        let len = self.list_ref(list)?.items.len();
        let reader = Reader::at_edge(list, direction, len);

        // Released readers retain stable slot identities for reuse.
        if let Some(index) = self.free_readers.pop() {
            self.readers[index] = Some(reader);
            return Ok(ReaderHandle(index));
        }
        let handle = ReaderHandle(self.readers.len());
        self.readers.push(Some(reader));
        Ok(handle)
    }

    /// Release a reader for reuse.
    ///
    /// # Errors
    ///
    /// Returns [`ArenaError::InvalidReader`] for a stale handle.
    pub fn release_reader(&mut self, handle: ReaderHandle) -> Result<(), ArenaError> {
        self.readers
            .get_mut(handle.0)
            .and_then(Option::take)
            .ok_or(ArenaError::InvalidReader)?;
        self.free_readers.push(handle.0);
        Ok(())
    }
}

impl Arena {
    /// Replace one datum by zero-based position.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale handle or out-of-range position.
    pub fn set(
        &mut self,
        handle: ListHandle,
        index: usize,
        datum: Datum,
    ) -> Result<Datum, ArenaError> {
        let slot = self
            .list_mut(handle)?
            .items
            .get_mut(index)
            .ok_or(ArenaError::Bounds)?;
        Ok(std::mem::replace(slot, datum))
    }

    /// Attach or remove a DLIST/name datum.
    ///
    /// # Errors
    ///
    /// Returns [`ArenaError::InvalidList`] for a stale handle.
    pub fn set_name(&mut self, handle: ListHandle, name: Option<Datum>) -> Result<(), ArenaError> {
        self.list_mut(handle)?.name = name;
        Ok(())
    }

    /// Snapshot direct list data for bridging and diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`ArenaError::InvalidList`] for a stale handle.
    pub fn to_vec(&self, handle: ListHandle) -> Result<Vec<Datum>, ArenaError> {
        Ok(self.list_ref(handle)?.items.clone())
    }
}

impl Arena {
    /// Recursively copy a list while detecting cycles.
    ///
    /// # Errors
    ///
    /// Returns an arena error for stale handles, cycles, or exhausted cells.
    fn copy_inner(
        &mut self,
        handle: ListHandle,
        active: &mut HashSet<ListHandle>,
    ) -> Result<ListHandle, ArenaError> {
        // Revisiting an active header proves a structural cycle.
        if !active.insert(handle) {
            return Err(ArenaError::Cycle);
        }
        let source = self.list_ref(handle)?.clone();
        let mut copied = Vec::with_capacity(source.items.len());
        for datum in source.items {
            copied.push(match datum {
                Datum::List(nested) => Datum::List(self.copy_inner(nested, active)?),
                Datum::Word(word) => Datum::Word(word),
            });
        }
        active.remove(&handle);
        let result = self.list_from(copied)?;
        self.set_name(result, source.name)?;
        Ok(result)
    }
}

impl Arena {
    /// Return a mutable list or reject a stale handle.
    ///
    /// # Errors
    ///
    /// Returns `ArenaError::InvalidList` for a stale handle.
    fn list_mut(&mut self, handle: ListHandle) -> Result<&mut List, ArenaError> {
        self.lists
            .get_mut(handle.0)
            .and_then(Option::as_mut)
            .ok_or(ArenaError::InvalidList)
    }

    /// Return a list or reject a stale handle.
    ///
    /// # Errors
    ///
    /// Returns `ArenaError::InvalidList` for a stale handle.
    fn list_ref(&self, handle: ListHandle) -> Result<&List, ArenaError> {
        self.lists
            .get(handle.0)
            .and_then(Option::as_ref)
            .ok_or(ArenaError::InvalidList)
    }

    /// Return a mutable reader or reject a stale handle.
    ///
    /// # Errors
    ///
    /// Returns `ArenaError::InvalidReader` for a stale handle.
    fn reader_mut(&mut self, handle: ReaderHandle) -> Result<&mut Reader, ArenaError> {
        self.readers
            .get_mut(handle.0)
            .and_then(Option::as_mut)
            .ok_or(ArenaError::InvalidReader)
    }

    /// Return a reader or reject a stale handle.
    ///
    /// # Errors
    ///
    /// Returns `ArenaError::InvalidReader` for a stale handle.
    fn reader_ref(&self, handle: ReaderHandle) -> Result<&Reader, ArenaError> {
        self.readers
            .get(handle.0)
            .and_then(Option::as_ref)
            .ok_or(ArenaError::InvalidReader)
    }

    /// Check that more cells fit in the configured bound.
    ///
    /// # Errors
    ///
    /// Returns `ArenaError::Exhausted` when the added cells exceed the bound.
    fn reserve(&self, additional: usize) -> Result<(), ArenaError> {
        if self.cells.saturating_add(additional) > self.cell_limit {
            Err(ArenaError::Exhausted)
        } else {
            Ok(())
        }
    }
}

// -----------------------------------------------------------------------------
// ArenaError: Reports invalid handles, bounds, cycles, and exhaustion.
// -----------------------------------------------------------------------------

/// Safe SLIP arena failure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ArenaError {
    /// A positional operation exceeded a list.
    #[error("list position is out of bounds")]
    Bounds,
    /// A recursive copy encountered a cycle.
    #[error("cyclic lists cannot be copied")]
    Cycle,
    /// A pop operation targeted an empty list.
    #[error("list is empty")]
    Empty,
    /// The configured cell limit would be exceeded.
    #[error("SLIP cell limit exhausted")]
    Exhausted,
    /// A list handle was released or never allocated.
    #[error("invalid SLIP list handle")]
    InvalidList,
    /// A reader handle was released or never allocated.
    #[error("invalid SLIP reader handle")]
    InvalidReader,
}

// -----------------------------------------------------------------------------
// Tests: Exercise mutation, deep copies, reuse, and reader direction.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{Arena, ArenaError, Datum, Direction};

    /// # Panics
    ///
    /// Panics when an arena operation or assertion fails.
    #[test]
    fn mutates_and_reuses_lists() {
        let mut arena = Arena::new(4);
        let list = arena.list_from([Datum::Word(1), Datum::Word(2)]).unwrap();
        arena.push_front(list, Datum::Word(0)).unwrap();
        arena.push_back(list, Datum::Word(3)).unwrap();
        assert_eq!(arena.to_vec(list).unwrap().len(), 4);
        assert_eq!(
            arena.push_back(list, Datum::Word(4)),
            Err(ArenaError::Exhausted)
        );
        arena.erase(list).unwrap();
        let reused = arena.list();
        assert_eq!(list, reused);
    }

    /// # Panics
    ///
    /// Panics when an arena operation or assertion fails.
    #[test]
    fn copies_nested_lists() {
        let mut arena = Arena::new(8);
        let child = arena.list_from([Datum::Word(7)]).unwrap();
        let parent = arena.list_from([Datum::List(child)]).unwrap();
        let copy = arena.copy(parent).unwrap();
        let Datum::List(copied_child) = arena.get(copy, 0).unwrap() else {
            panic!("nested list expected");
        };
        arena.set(child, 0, Datum::Word(8)).unwrap();
        assert_eq!(arena.get(copied_child, 0).unwrap(), Datum::Word(7));
    }

    /// # Panics
    ///
    /// Panics when an arena operation or assertion fails.
    #[test]
    fn readers_traverse_both_directions() {
        let mut arena = Arena::new(3);
        let list = arena
            .list_from([Datum::Word(1), Datum::Word(2), Datum::Word(3)])
            .unwrap();
        let right = arena.reader(list, Direction::LeftToRight).unwrap();
        let left = arena.reader(list, Direction::RightToLeft).unwrap();
        assert_eq!(arena.read(right).unwrap(), Some(Datum::Word(1)));
        assert_eq!(arena.read(left).unwrap(), Some(Datum::Word(3)));
    }
}
