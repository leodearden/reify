//! The one place the evaluator reads a store cell from.
//!
//! Every store read an evaluation makes goes through a [`CellReader`], at the
//! bottom of the chain of local scopes (`crate::scope_chain`). The
//! `impl CellReader for ValueMap` below is the old engine's reader: the store
//! is a map, and a cell it does not hold reads `Undef`.

use reify_core::ValueCellId;
use reify_ir::{Value, ValueMap};

/// A source of store cell values.
pub(crate) trait CellReader {
    /// The value of the store cell `id`, `Value::Undef` when it holds none.
    fn read(&self, id: &ValueCellId) -> Value;

    /// The store as one map, for a hook whose contract takes a map ([`crate::SymbolicSelectorCtorFn`]).
    fn as_value_map(&self) -> &ValueMap;
}

impl CellReader for ValueMap {
    fn read(&self, id: &ValueCellId) -> Value {
        self.get_or_undef(id)
    }

    fn as_value_map(&self) -> &ValueMap {
        self
    }
}

#[cfg(test)]
mod tests;
