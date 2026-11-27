//! The one place the evaluator reads a store cell from.
//!
//! Every store read an evaluation makes goes through a [`CellReader`], at the
//! bottom of the chain of local scopes (`crate::scope_chain`). There are two:
//! - `impl CellReader for ValueMap`, the old engine's reader: the store is a
//!   map, and a cell it does not hold reads `Undef`.
//! - [`RoundReader`], the new engine's reader for one round of
//!   `crate::eval_expr_or_pending`: it asks a [`PendingReader`], and a cell
//!   that is not here yet reads `Undef` as the walk's stand-in and is counted.

use std::cell::Cell;

use reify_core::ValueCellId;
use reify_ir::{Value, ValueMap};

/// What the new engine's store answers when asked for one cell.
#[derive(Debug, Clone, PartialEq)]
pub enum CellRead {
    /// The cell's value. `Value::Undef` is a value like any other.
    Here(Value),
    /// The cell will have a value, but does not have it yet.
    NotHereYet,
}

/// The new engine's store, as the evaluator reads it.
///
/// Contract: answer [`CellRead::NotHereYet`] only for a cell whose value will
/// arrive. A ref that names no cell of the store reads `Here(Value::Undef)`:
/// the walk asks for refs that may name nothing (the composed-field probe asks
/// for `__field.<name>` at every stdlib call outside a sealed frame), and an
/// evaluation that waited on such a ref would never finish.
pub trait PendingReader {
    /// The answer for the cell `id`.
    fn read(&self, id: &ValueCellId) -> CellRead;
}

/// A source of store cell values.
pub(crate) trait CellReader {
    /// The value of the store cell `id`, `Value::Undef` when it holds none.
    fn read(&self, id: &ValueCellId) -> Value;

    /// The store as one map, for a hook whose contract takes a map ([`crate::SymbolicSelectorCtorFn`]).
    fn as_value_map(&self) -> &ValueMap;

    /// Whether a read may be answered "not here yet". When it may, the walk
    /// evaluates every operand outside an arm, so that one round asks for
    /// every cell the expression reads.
    fn may_answer_not_here_yet(&self) -> bool;

    /// How many reads so far were answered "not here yet".
    fn not_here_yet_count(&self) -> usize;
}

impl CellReader for ValueMap {
    fn read(&self, id: &ValueCellId) -> Value {
        self.get_or_undef(id)
    }

    fn as_value_map(&self) -> &ValueMap {
        self
    }

    fn may_answer_not_here_yet(&self) -> bool {
        false
    }

    fn not_here_yet_count(&self) -> usize {
        0
    }
}

/// One round's reader over a [`PendingReader`]: a cell that is not here yet
/// reads `Value::Undef`, the walk's stand-in, and is counted.
pub(crate) struct RoundReader<'r> {
    store: &'r dyn PendingReader,
    not_here_yet: Cell<usize>,
    /// The new engine's store is not a map: a hook that takes one sees no cell.
    no_map: ValueMap,
}

impl<'r> RoundReader<'r> {
    pub(crate) fn new(store: &'r dyn PendingReader) -> Self {
        Self {
            store,
            not_here_yet: Cell::new(0),
            no_map: ValueMap::new(),
        }
    }
}

impl CellReader for RoundReader<'_> {
    fn read(&self, id: &ValueCellId) -> Value {
        match self.store.read(id) {
            CellRead::Here(value) => value,
            CellRead::NotHereYet => {
                self.not_here_yet.set(self.not_here_yet.get() + 1);
                Value::Undef
            }
        }
    }

    fn as_value_map(&self) -> &ValueMap {
        &self.no_map
    }

    fn may_answer_not_here_yet(&self) -> bool {
        true
    }

    fn not_here_yet_count(&self) -> usize {
        self.not_here_yet.get()
    }
}

#[cfg(test)]
mod tests;
