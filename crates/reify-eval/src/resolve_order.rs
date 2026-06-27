//! Dependency-ordered scope resolution (task #4822, β).
//!
//! Computes the order in which `TopologyTemplate` scopes should be solved so
//! that a scope that reads another scope's auto cell is always solved AFTER the
//! scope that owns that cell.  This replaces the source-order walk in `eval()`
//! and `eval_cached()` with a stable topological sort over the cross-scope
//! read-DAG.
//!
//! ## Public surface
//!
//! `resolve_order(templates)` — given the flat source-ordered template slice of
//! a `CompiledModule`, returns a `ResolveOrder` whose `order` is a permutation
//! of `0..templates.len()` and whose `coupling_diagnostics` contains
//! `W_SCOPE_COUPLING` warnings for any irreducible read-cycles (SCCs of size ≥ 2).
//!
//! ## Invariants
//!
//! - **INV-2 back-compat identity**: for modules with no cross-scope auto reads
//!   (or where the source order already satisfies all dependencies),
//!   `order == [0, 1, .., n-1]` — byte-identical resolved values to the
//!   previous source-order walk.
//! - **INV-5 no per-occurrence split**: the function only reorders existing
//!   per-template solves; it never splits or merges a template's solve.
//! - **INV-7 cycle safety**: irreducible cycles (SCC size ≥ 2) are emitted in
//!   source order with `W_SCOPE_COUPLING` diagnostics; no panic or deadlock.

use reify_compiler::TopologyTemplate;
use reify_core::Diagnostic;

/// Result of computing the dependency-ordered resolution pass over a module's
/// template slice.
pub(crate) struct ResolveOrder {
    /// Permutation of `0..templates.len()` giving the solve order.
    ///
    /// `order[i]` is the index (into the original template slice) of the i-th
    /// template to solve.  For uncoupled modules this equals `[0, 1, .., n-1]`.
    pub(crate) order: Vec<usize>,

    /// `W_SCOPE_COUPLING` diagnostics for irreducible read-cycles (SCCs of size ≥ 2).
    ///
    /// Empty when the read-DAG is acyclic.  Acyclic crossings do NOT appear
    /// here — they are handled by the ordering itself.
    pub(crate) coupling_diagnostics: Vec<Diagnostic>,
}

/// Compute the dependency-ordered resolution order for `templates`.
///
/// Returns a [`ResolveOrder`] whose `order` is a stable permutation of
/// `0..templates.len()`.  The identity permutation `[0, 1, .., n-1]` is
/// returned when no cross-scope auto reads exist (INV-2).
///
/// This is a *structural* analysis — it reads only the compiled template
/// metadata (value_cells, constraints, objective terms) and requires no
/// solved values.  It is safe to call before any solver invocation.
pub(crate) fn resolve_order(templates: &[TopologyTemplate]) -> ResolveOrder {
    // Stub implementation: return identity (source) order with no diagnostics.
    // Replaced in step-2 (acyclic orderer) and step-4 (SCC + cycle handling).
    ResolveOrder {
        order: (0..templates.len()).collect(),
        coupling_diagnostics: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    // Unit tests added in step-1 (RED) and step-3 (RED).
}
