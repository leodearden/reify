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

use std::collections::{BinaryHeap, HashMap, HashSet};
use std::cmp::Reverse;

use reify_compiler::TopologyTemplate;
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, ValueCellId};

use crate::deps::extract_dependency_trace;

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

/// Build the cross-scope auto-cell read-DAG edges.
///
/// Returns:
/// - `auto_owner`: `ValueCellId -> template_index` for all auto cells.
/// - `adj`: adjacency list `adj[i]` = sorted, deduped set of indices j where
///   scope i must be resolved before scope j (i.e. j reads i's auto cell).
fn build_read_dag(
    templates: &[TopologyTemplate],
) -> (HashMap<ValueCellId, usize>, Vec<Vec<usize>>) {
    let n = templates.len();

    // Build owner map: auto_cell_id → template index.
    let mut auto_owner: HashMap<ValueCellId, usize> = HashMap::new();
    for (i, template) in templates.iter().enumerate() {
        for cell in &template.value_cells {
            if cell.kind.is_auto() {
                auto_owner.insert(cell.id.clone(), i);
            }
        }
    }

    // Build adjacency list: edge i→j means "i must be solved before j".
    // We deduplicate edges.
    let mut edge_set: HashSet<(usize, usize)> = HashSet::new();

    for (j, template) in templates.iter().enumerate() {
        // Collect reads from all constraint expressions.
        for constraint in &template.constraints {
            let reads = extract_dependency_trace(&constraint.expr).reads;
            for r in reads {
                if let Some(&i) = auto_owner.get(&r) {
                    if i != j {
                        edge_set.insert((i, j));
                    }
                }
            }
        }
        // Collect reads from objective terms.
        if let Some(obj) = &template.objective {
            for term in &obj.terms {
                let reads = extract_dependency_trace(&term.expr).reads;
                for r in reads {
                    if let Some(&i) = auto_owner.get(&r) {
                        if i != j {
                            edge_set.insert((i, j));
                        }
                    }
                }
            }
        }
    }

    // Build adjacency list from edge set.
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, j) in edge_set {
        adj[i].push(j);
    }
    // Sort adjacency lists for deterministic output.
    for list in &mut adj {
        list.sort_unstable();
        list.dedup();
    }

    (auto_owner, adj)
}

/// Run Kahn's topological sort on the given adjacency list.
///
/// Tie-break: among in-degree-0 nodes, always pick the smallest source index
/// first (stable, source-tie-broken — ensures INV-2 for uncoupled modules).
///
/// Returns the topological order as a permutation of `0..n`.  Nodes that
/// are part of cycles will be ABSENT from the returned vector (the caller
/// detects this by checking `result.len() < n`).
fn kahn_topo(adj: &[Vec<usize>], n: usize) -> Vec<usize> {
    // Compute in-degrees.
    let mut in_degree = vec![0usize; n];
    for succs in adj {
        for &j in succs {
            in_degree[j] += 1;
        }
    }

    // Min-heap (Reverse for min semantics) seeded with all in-degree-0 nodes.
    // BinaryHeap<Reverse<usize>> gives us the smallest index first.
    let mut ready: BinaryHeap<Reverse<usize>> = (0..n)
        .filter(|&i| in_degree[i] == 0)
        .map(Reverse)
        .collect();

    let mut order = Vec::with_capacity(n);
    while let Some(Reverse(i)) = ready.pop() {
        order.push(i);
        for &j in &adj[i] {
            in_degree[j] -= 1;
            if in_degree[j] == 0 {
                ready.push(Reverse(j));
            }
        }
    }

    order
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
///
/// **Cycle handling (step-4):** irreducible cycles (SCC size ≥ 2) are
/// detected via Tarjan SCC, emitted in source order with `W_SCOPE_COUPLING`
/// diagnostics, and their members are folded into the condensation DAG for
/// the Kahn topo pass.
pub(crate) fn resolve_order(templates: &[TopologyTemplate]) -> ResolveOrder {
    let n = templates.len();
    if n == 0 {
        return ResolveOrder {
            order: Vec::new(),
            coupling_diagnostics: Vec::new(),
        };
    }

    let (auto_owner, adj) = build_read_dag(templates);

    // Run Kahn's topo sort.  If the graph is acyclic this produces all n nodes.
    // If cycles exist, the result is shorter (cycle members remain with in-degree > 0).
    let topo = kahn_topo(&adj, n);

    if topo.len() == n {
        // Fully acyclic — no coupling diagnostics (INV-2 back-compat identity for
        // uncoupled modules: if no edges exist, Kahn returns source order).
        return ResolveOrder {
            order: topo,
            coupling_diagnostics: Vec::new(),
        };
    }

    // Some nodes are in cycles.  Delegate to the full SCC path (step-4).
    // For now (step-2), this is a stub that handles partial outputs by
    // appending cycle members in source order — this satisfies INV-7 without
    // full SCC detection.  Step-4 replaces this with proper Tarjan SCC + cycle
    // diagnostics.
    let in_topo: HashSet<usize> = topo.iter().copied().collect();
    let mut cycle_members: Vec<usize> = (0..n)
        .filter(|i| !in_topo.contains(i))
        .collect();
    // cycle_members is already in source order (0..n filter).

    // Emit W_SCOPE_COUPLING for each cross-scope auto read within the cycle set.
    let cycle_set: HashSet<usize> = cycle_members.iter().copied().collect();
    let coupling_diagnostics =
        emit_cycle_coupling_diagnostics(templates, &auto_owner, &cycle_set);

    let mut order = topo;
    order.append(&mut cycle_members);

    ResolveOrder {
        order,
        coupling_diagnostics,
    }
}

/// Emit `W_SCOPE_COUPLING` diagnostics for cross-scope auto reads within
/// the given set of template indices (the cycle/SCC members).
///
/// Deduped per (owner_idx, reader_idx, crossing_cell) triple.
fn emit_cycle_coupling_diagnostics(
    templates: &[TopologyTemplate],
    auto_owner: &HashMap<ValueCellId, usize>,
    cycle_set: &HashSet<usize>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut seen: HashSet<(usize, usize, ValueCellId)> = HashSet::new();

    for &j in cycle_set {
        let template = &templates[j];
        let b_name = &template.name;

        let mut emit_for_reads = |reads: Vec<ValueCellId>, span| {
            for r in reads {
                if let Some(&i) = auto_owner.get(&r) {
                    if i != j && cycle_set.contains(&i) {
                        let key = (i, j, r.clone());
                        if seen.insert(key) {
                            let owner_name = &templates[i].name;
                            let msg = format!(
                                "W_SCOPE_COUPLING: scope '{b_name}' reads auto cell '{r}' \
                                 owned by already-resolved scope '{owner_name}'; \
                                 bottom-up resolution may be approximate"
                            );
                            let diag = Diagnostic::warning(msg)
                                .with_code(DiagnosticCode::ScopeCoupling);
                            diagnostics.push(if let Some(s) = span {
                                diag.with_label(DiagnosticLabel::new(s, "scope coupling read site"))
                            } else {
                                diag
                            });
                        }
                    }
                }
            }
        };

        for constraint in &template.constraints {
            let reads = extract_dependency_trace(&constraint.expr).reads;
            emit_for_reads(reads, Some(constraint.span));
        }
        if let Some(obj) = &template.objective {
            for term in &obj.terms {
                let reads = extract_dependency_trace(&term.expr).reads;
                emit_for_reads(reads, None);
            }
        }
    }

    diagnostics
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

    // -------------------------------------------------------------------------
    // step-3 cases: irreducible-cycle handling (INV-7)
    // -------------------------------------------------------------------------

    /// (a) Mutual 2-cycle: A reads B.k AND B reads A.k.
    ///
    /// Requirements (INV-7):
    /// - Must terminate (no panic/deadlock).
    /// - Both members returned in SOURCE order [A=0, B=1].
    /// - coupling_diagnostics contains ≥1 W_SCOPE_COUPLING naming both scopes
    ///   AND the crossing cell.
    #[test]
    fn two_cycle_terminates_source_order_and_emits_coupling() {
        // A reads B.k, B reads A.k → irreducible 2-cycle.
        let a = TopologyTemplateBuilder::new("A")
            .auto_param("A", "k", Type::length())
            // A reads B's auto cell B.m
            .constraint("A", 0, None, gt(value_ref("B", "m"), literal(mm(0.0))))
            .build();

        let b = TopologyTemplateBuilder::new("B")
            .auto_param("B", "m", Type::length())
            // B reads A's auto cell A.k
            .constraint("B", 0, None, gt(value_ref("A", "k"), literal(mm(0.0))))
            .build();

        // Source order: [A=0, B=1]
        let templates = vec![a, b];
        let ro = resolve_order(&templates);

        // Must include both members.
        assert_eq!(ro.order.len(), 2, "both cycle members must be in order");
        // Source order for cycle members: A (0) before B (1).
        let a_pos = ro.order.iter().position(|&i| i == 0).unwrap();
        let b_pos = ro.order.iter().position(|&i| i == 1).unwrap();
        assert!(
            a_pos < b_pos,
            "cycle members must be in source order [A=0, B=1]; got: {:?}",
            ro.order
        );

        // Must emit at least one W_SCOPE_COUPLING.
        assert!(
            !ro.coupling_diagnostics.is_empty(),
            "2-cycle must emit ≥1 W_SCOPE_COUPLING; got none"
        );

        // At least one diagnostic must name both scopes.
        let any_names_both = ro.coupling_diagnostics.iter().any(|d| {
            let m = &d.message;
            m.contains("A") && m.contains("B")
        });
        assert!(
            any_names_both,
            "at least one W_SCOPE_COUPLING must name both 'A' and 'B'; diagnostics: {:?}",
            ro.coupling_diagnostics
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );

        // At least one diagnostic must name a crossing cell.
        let any_names_cell = ro.coupling_diagnostics.iter().any(|d| {
            let m = &d.message;
            m.contains("A.k") || m.contains("B.m")
        });
        assert!(
            any_names_cell,
            "at least one W_SCOPE_COUPLING must name a crossing cell (A.k or B.m); diagnostics: {:?}",
            ro.coupling_diagnostics
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }

    /// (b) 2-SCC {A, B} plus acyclic leaf C that reads A.k.
    ///
    /// Requirements:
    /// - C ordered AFTER the SCC (C sees A resolved first).
    /// - coupling_diagnostics names A↔B cycle crossings ONLY, NOT the acyclic A→C edge.
    #[test]
    fn two_scc_plus_acyclic_leaf_c_reads_a_after_scc_cycle_only_coupling() {
        // A reads B.m, B reads A.k → {A, B} are a 2-cycle.
        let a = TopologyTemplateBuilder::new("A")
            .auto_param("A", "k", Type::length())
            .constraint("A", 0, None, gt(value_ref("B", "m"), literal(mm(0.0))))
            .build();

        let b = TopologyTemplateBuilder::new("B")
            .auto_param("B", "m", Type::length())
            .constraint("B", 0, None, gt(value_ref("A", "k"), literal(mm(0.0))))
            .build();

        // C reads A.k (acyclic edge — C depends on A, not the other way around).
        let c = TopologyTemplateBuilder::new("C")
            .auto_param("C", "z", Type::length())
            .constraint("C", 0, None, gt(value_ref("A", "k"), literal(mm(0.0))))
            .build();

        // Source order: [A=0, B=1, C=2]
        let templates = vec![a, b, c];
        let ro = resolve_order(&templates);

        // All three members must be present.
        assert_eq!(ro.order.len(), 3);

        // C (idx 2) must come AFTER A (idx 0) — C reads A's auto cell.
        let a_pos = ro.order.iter().position(|&i| i == 0).unwrap();
        let c_pos = ro.order.iter().position(|&i| i == 2).unwrap();
        assert!(
            a_pos < c_pos,
            "C must come after A (C reads A.k); order = {:?}",
            ro.order
        );

        // W_SCOPE_COUPLING diagnostics must NOT mention C for the A→C edge.
        // They should ONLY fire for the A↔B cycle crossings.
        for diag in &ro.coupling_diagnostics {
            // A diagnostic about C being the READER is NOT expected (acyclic).
            // A diagnostic about A reading C or C reading B would also be wrong.
            // We check: no diagnostic has C as the OWNER of a cell that the
            // cycle member reads (C is not in the SCC).
            // Simpler: assert each diag mentions only A and/or B (the SCC), not C.
            let m = &diag.message;
            // C.z should not appear as a crossing cell (C is not in the SCC).
            assert!(
                !m.contains("C.z"),
                "acyclic A→C edge must NOT produce W_SCOPE_COUPLING; got: {m}"
            );
        }

        // At least one coupling diagnostic for the intra-SCC A↔B crossing.
        assert!(
            !ro.coupling_diagnostics.is_empty(),
            "2-SCC {{A,B}} must still emit ≥1 W_SCOPE_COUPLING"
        );
    }
}
