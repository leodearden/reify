//! The param defaults the declaration-site check (`entity.rs`
//! `check_param_default_type`) rejected, handed to the conformance pass
//! (`conformance` `check_param_default_conformance`) so one mistake gets one
//! diagnostic (PRD D4-4). A cell is identified by its `ValueCellId` and its
//! declaration `SourceSpan`; this module's tests show why both are needed.

use std::collections::{HashMap, HashSet};

use reify_core::{SourceSpan, ValueCellId};

use crate::types::ValueCellDecl;

#[derive(Debug, Default)]
pub(crate) struct RejectedParamDefaults {
    spans_by_cell: HashMap<ValueCellId, HashSet<SourceSpan>>,
}

impl RejectedParamDefaults {
    pub(crate) fn record(&mut self, cell: &ValueCellDecl) {
        self.spans_by_cell
            .entry(cell.id.clone())
            .or_default()
            .insert(cell.span);
    }

    pub(crate) fn contains(&self, cell: &ValueCellDecl) -> bool {
        self.spans_by_cell
            .get(&cell.id)
            .is_some_and(|spans| spans.contains(&cell.span))
    }

    pub(crate) fn absorb(&mut self, other: RejectedParamDefaults) {
        for (id, spans) in other.spans_by_cell {
            self.spans_by_cell.entry(id).or_default().extend(spans);
        }
    }
}

#[cfg(test)]
mod tests {
    use reify_core::Type;

    use super::*;
    use crate::types::{ValueCellKind, Visibility};

    fn param_cell(member: &str, span: SourceSpan) -> ValueCellDecl {
        ValueCellDecl {
            id: ValueCellId::new("S", member),
            kind: ValueCellKind::Param,
            visibility: Visibility::Private,
            is_aux: false,
            cell_type: Type::dimensionless_scalar(),
            default_expr: None,
            solver_hints: vec![],
            span,
        }
    }

    /// A recorded cell is contained; an empty set contains nothing.
    #[test]
    fn recorded_cell_is_contained_and_an_empty_set_contains_nothing() {
        let cell = param_cell("m", SourceSpan::new(10, 20));
        let mut rejected = RejectedParamDefaults::default();
        assert!(!rejected.contains(&cell));
        rejected.record(&cell);
        assert!(rejected.contains(&cell));
    }

    /// A trait-injected cell whose offsets (from another file) coincide is not skipped.
    #[test]
    fn a_cell_sharing_only_the_span_is_not_contained() {
        let mut rejected = RejectedParamDefaults::default();
        rejected.record(&param_cell("m", SourceSpan::new(10, 20)));
        assert!(!rejected.contains(&param_cell("d", SourceSpan::new(10, 20))));
    }

    /// Same-named params in one structure share an id, so the span tells them apart.
    #[test]
    fn a_same_named_twin_at_another_span_is_not_contained() {
        let mut rejected = RejectedParamDefaults::default();
        rejected.record(&param_cell("m", SourceSpan::new(10, 20)));
        assert!(!rejected.contains(&param_cell("m", SourceSpan::new(30, 40))));
    }

    /// Absorbing one set into another keeps the cells of both.
    #[test]
    fn absorb_keeps_the_cells_of_both_sets() {
        let m = param_cell("m", SourceSpan::new(10, 20));
        let n = param_cell("n", SourceSpan::new(30, 40));
        let mut a = RejectedParamDefaults::default();
        a.record(&m);
        let mut b = RejectedParamDefaults::default();
        b.record(&n);
        a.absorb(b);
        assert!(a.contains(&m));
        assert!(a.contains(&n));
    }
}
