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
    use reify_core::Type;
    use reify_test_support::{TopologyTemplateBuilder, gt, literal, mm, value_ref};

    use super::resolve_order;

    // -------------------------------------------------------------------------
    // step-1 cases: acyclic read-DAG reorder + back-compat identity (INV-2)
    // -------------------------------------------------------------------------

    /// (a) Two templates in source order [B, A] where B reads A's auto cell.
    ///
    /// B is declared first (index 0) but A must be solved first because B's
    /// constraint reads `A.k`.  Expected: `order == [1, 0]` (A first, then B).
    #[test]
    fn two_templates_b_reads_a_auto_cell_reordered_to_a_first() {
        // Source order: [b, a] — b declared before a.
        // b has a constraint that reads a's auto cell `A.k`.
        let b = TopologyTemplateBuilder::new("B")
            .auto_param("B", "y", Type::length())
            // B.y > A.k  (reads A's auto cell — cross-scope dependency)
            .constraint("B", 0, None, gt(value_ref("A", "k"), literal(mm(1.0))))
            .build();

        let a = TopologyTemplateBuilder::new("A")
            .auto_param("A", "k", Type::length())
            // self-constraint: A.k > 0mm
            .constraint("A", 0, None, gt(value_ref("A", "k"), literal(mm(0.0))))
            .build();

        let templates = vec![b, a];
        let ro = resolve_order(&templates);

        // A (index 1) must come before B (index 0).
        assert_eq!(
            ro.order,
            vec![1, 0],
            "B reads A.k, so A (idx 1) must be solved before B (idx 0); got: {:?}",
            ro.order
        );
        assert!(
            ro.coupling_diagnostics.is_empty(),
            "acyclic crossing must NOT emit W_SCOPE_COUPLING; got: {:?}",
            ro.coupling_diagnostics
        );
    }

    /// (b) Two templates [X, Y] with NO cross-scope auto reads.
    ///
    /// No ordering constraint exists — source order [0, 1] must be preserved
    /// (INV-2 back-compat identity).
    #[test]
    fn two_templates_no_cross_scope_reads_source_order_preserved() {
        let x = TopologyTemplateBuilder::new("X")
            .auto_param("X", "a", Type::length())
            .constraint("X", 0, None, gt(value_ref("X", "a"), literal(mm(0.0))))
            .build();

        let y = TopologyTemplateBuilder::new("Y")
            .auto_param("Y", "b", Type::length())
            .constraint("Y", 0, None, gt(value_ref("Y", "b"), literal(mm(0.0))))
            .build();

        let templates = vec![x, y];
        let ro = resolve_order(&templates);

        assert_eq!(
            ro.order,
            vec![0, 1],
            "no cross-scope reads: source order must be preserved (INV-2); got: {:?}",
            ro.order
        );
        assert!(ro.coupling_diagnostics.is_empty());
    }

    /// (c) Three templates [X, Y, Z] where only Z reads Y's auto cell.
    ///
    /// Y must come before Z.  X has no dependency, so it keeps its earliest
    /// source-index slot (stable tie-break: smallest source-index among
    /// in-degree-0 nodes is selected first).
    ///
    /// Expected order: X (0), Y (1), Z (2) — source order, because X wins
    /// tie-break (no deps), Y must be before Z.
    #[test]
    fn three_templates_z_reads_y_y_before_z_x_keeps_slot() {
        let x = TopologyTemplateBuilder::new("X")
            .auto_param("X", "a", Type::length())
            .build();

        let y = TopologyTemplateBuilder::new("Y")
            .auto_param("Y", "b", Type::length())
            .build();

        let z = TopologyTemplateBuilder::new("Z")
            .auto_param("Z", "c", Type::length())
            // Z.c > Y.b  (Z reads Y's auto cell)
            .constraint("Z", 0, None, gt(value_ref("Y", "b"), literal(mm(0.0))))
            .build();

        // Source order: [X=0, Y=1, Z=2]
        let templates = vec![x, y, z];
        let ro = resolve_order(&templates);

        // Z must come after Y.
        let y_pos = ro.order.iter().position(|&i| i == 1).unwrap();
        let z_pos = ro.order.iter().position(|&i| i == 2).unwrap();
        assert!(
            y_pos < z_pos,
            "Y (idx 1) must be solved before Z (idx 2); order = {:?}",
            ro.order
        );
        // X has no deps — stable tie-break selects it first (source index 0 is smallest).
        assert_eq!(
            ro.order[0], 0,
            "X (idx 0) has no deps and wins tie-break, so it should be first; order = {:?}",
            ro.order
        );
        assert!(ro.coupling_diagnostics.is_empty());
    }
}
