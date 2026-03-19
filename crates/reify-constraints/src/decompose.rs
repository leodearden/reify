//! Connected-component decomposition for constraint problems.
//!
//! Builds a bipartite graph of constraints ↔ auto params and uses
//! union-find to identify independent sub-problems.

use reify_types::{AutoParam, CompiledExpr, ConstraintDomain, ConstraintNodeId, ValueCellId};
use std::collections::HashSet;

/// An independent sub-problem extracted from a larger constraint problem.
#[derive(Debug)]
pub struct SubProblem {
    /// The auto parameters in this sub-problem.
    pub auto_params: HashSet<ValueCellId>,
    /// The constraints in this sub-problem (id + expression).
    pub constraints: Vec<(ConstraintNodeId, CompiledExpr)>,
    /// The domain classification for this sub-problem.
    pub domain: ConstraintDomain,
}

/// Decompose a constraint problem into independent connected components.
pub fn decompose_into_components(
    _auto_params: &[AutoParam],
    _constraints: &[(ConstraintNodeId, CompiledExpr)],
) -> Vec<SubProblem> {
    todo!("decompose_into_components not yet implemented")
}
