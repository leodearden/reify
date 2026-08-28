//! Branch signatures: which non-smooth (kink) branch each dual evaluation took.
//!
//! Task #6672 (solver-unification ε).  Design reference:
//! `docs/prds/v0_6/geometry-algebra-solver-unification.md` §7.7.
//!
//! Forward-mode AD over an expression containing `if`, `min`, `max`, `abs`,
//! `clamp`, a comparison, or a field reduction returns the derivative of the
//! *branch that was actually taken* — the active-branch (Clarke) Jacobian.
//! That is the right answer locally, but it is only meaningful together with a
//! record of *which* branch was taken: two evaluations that took different
//! branches are two different smooth functions, and a solver that treats their
//! Jacobians as samples of one function will chatter across the kink forever.
//!
//! [`BranchRecord`] is that record.  λ (#6679) consumes it to detect chatter.
//!
//! This module currently carries the minimum the core traversal needs; the
//! kink vocabulary and λ's consumption API land with the kink arms.

/// The branches taken by one dual evaluation of one expression, in traversal
/// order.
///
/// An empty record means the traversal encountered no non-smooth node at all —
/// the expression is smooth at this point, and its Jacobian row is an ordinary
/// derivative rather than a Clarke selection.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BranchRecord {
    /// Reserved for the kink entries; see the module docs.
    entries: Vec<()>,
}

impl BranchRecord {
    /// An empty record — no kink has been traversed yet.
    pub fn new() -> Self {
        BranchRecord::default()
    }

    /// Number of non-smooth nodes traversed.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when the traversal encountered no non-smooth node.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
