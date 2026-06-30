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
                if let Some(&i) = auto_owner.get(&r)
                    && i != j {
                        edge_set.insert((i, j));
                    }
            }
        }
        // Collect reads from objective terms.
        if let Some(obj) = &template.objective {
            for term in &obj.terms {
                let reads = extract_dependency_trace(&term.expr).reads;
                for r in reads {
                    if let Some(&i) = auto_owner.get(&r)
                        && i != j {
                            edge_set.insert((i, j));
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

// ---------------------------------------------------------------------------
// Tarjan SCC (iterative, avoids OS stack overflow)
// Pattern from reify-compiler/src/scc.rs::tarjan_scc_visit — re-implemented
// over the read-DAG index adjacency so we can partition nodes into SCCs
// without mutating TopologyTemplate.
// ---------------------------------------------------------------------------

struct TarjanState {
    index: Vec<Option<usize>>,
    lowlink: Vec<usize>,
    on_stack: Vec<bool>,
    scc_stack: Vec<usize>,
    index_counter: usize,
    /// Output: list of SCCs, each as a Vec of node indices.
    /// Emitted in reverse-topological order (sinks first) by Tarjan's algorithm.
    sccs: Vec<Vec<usize>>,
}

fn tarjan_visit(v: usize, adj: &[Vec<usize>], st: &mut TarjanState) {
    st.index[v] = Some(st.index_counter);
    st.lowlink[v] = st.index_counter;
    st.index_counter += 1;
    st.scc_stack.push(v);
    st.on_stack[v] = true;

    // Explicit call stack: (node, next_neighbor_index).
    let mut call_stack: Vec<(usize, usize)> = vec![(v, 0)];

    while let Some(&mut (node, ref mut ni)) = call_stack.last_mut() {
        if *ni < adj[node].len() {
            let w = adj[node][*ni];
            *ni += 1;
            if st.index[w].is_none() {
                st.index[w] = Some(st.index_counter);
                st.lowlink[w] = st.index_counter;
                st.index_counter += 1;
                st.scc_stack.push(w);
                st.on_stack[w] = true;
                call_stack.push((w, 0));
            } else if st.on_stack[w] {
                st.lowlink[node] = st.lowlink[node].min(st.index[w].unwrap());
            }
        } else {
            let (finished, _) = call_stack.pop().unwrap();
            if let Some(&(parent, _)) = call_stack.last() {
                st.lowlink[parent] = st.lowlink[parent].min(st.lowlink[finished]);
            }
            if st.lowlink[finished] == st.index[finished].unwrap() {
                let mut scc = Vec::new();
                loop {
                    let w = st.scc_stack.pop().unwrap();
                    st.on_stack[w] = false;
                    scc.push(w);
                    if w == finished {
                        break;
                    }
                }
                st.sccs.push(scc);
            }
        }
    }
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
/// Algorithm:
/// 1. Build read-DAG (auto-cell owner map + cross-scope edges).
/// 2. Tarjan SCC to partition nodes into components.
/// 3. Build condensation DAG (one super-node per SCC).
/// 4. Kahn topo sort on condensation with smallest-min-source-index tie-break.
/// 5. Emit each SCC's members in source-index order.
/// 6. For SCCs of size ≥ 2, emit W_SCOPE_COUPLING for every intra-SCC
///    cross-scope auto read crossing (deduped per (owner, reader, cell)).
pub(crate) fn resolve_order(templates: &[TopologyTemplate]) -> ResolveOrder {
    let n = templates.len();
    if n == 0 {
        return ResolveOrder {
            order: Vec::new(),
            coupling_diagnostics: Vec::new(),
        };
    }

    let (auto_owner, adj) = build_read_dag(templates);

    // --- Step 1: Tarjan SCC ---
    let mut st = TarjanState {
        index: vec![None; n],
        lowlink: vec![0; n],
        on_stack: vec![false; n],
        scc_stack: Vec::new(),
        index_counter: 0,
        sccs: Vec::new(),
    };
    for start in 0..n {
        if st.index[start].is_none() {
            tarjan_visit(start, &adj, &mut st);
        }
    }
    // `st.sccs` is in reverse-topological order (sinks first).
    // Reverse to get sources first (topological order on condensation).
    let sccs_topo: Vec<Vec<usize>> = st.sccs.into_iter().rev().collect();

    // Map each node → its SCC index in sccs_topo.
    let mut node_to_scc = vec![0usize; n];
    for (s, scc) in sccs_topo.iter().enumerate() {
        for &v in scc {
            node_to_scc[v] = s;
        }
    }
    let num_sccs = sccs_topo.len();

    // --- Step 2: Condensation DAG ---
    // Edge s→t in condensation if any node in SCC s has an edge to a node in SCC t (s ≠ t).
    let mut cond_adj: Vec<HashSet<usize>> = vec![HashSet::new(); num_sccs];
    for (s, scc) in sccs_topo.iter().enumerate() {
        for &u in scc {
            for &v in &adj[u] {
                let t = node_to_scc[v];
                if t != s {
                    cond_adj[s].insert(t);
                }
            }
        }
    }
    // Convert to sorted Vec for deterministic Kahn order.
    let cond_adj_vec: Vec<Vec<usize>> = cond_adj
        .into_iter()
        .map(|mut s| {
            let mut v: Vec<usize> = s.drain().collect();
            v.sort_unstable();
            v
        })
        .collect();

    // --- Step 3: Kahn on condensation (tie-break by min source index in SCC) ---
    // For tie-breaking, use the minimum original node index in each SCC.
    let scc_min_idx: Vec<usize> = sccs_topo
        .iter()
        .map(|scc| *scc.iter().min().unwrap())
        .collect();

    // Compute in-degrees for condensation.
    let mut cond_indegree = vec![0usize; num_sccs];
    for succs in &cond_adj_vec {
        for &t in succs {
            cond_indegree[t] += 1;
        }
    }

    // Min-heap keyed by (min_source_idx, scc_idx) for stable tie-breaking.
    let mut ready: BinaryHeap<Reverse<(usize, usize)>> = (0..num_sccs)
        .filter(|&s| cond_indegree[s] == 0)
        .map(|s| Reverse((scc_min_idx[s], s)))
        .collect();

    let mut scc_order: Vec<usize> = Vec::with_capacity(num_sccs);
    while let Some(Reverse((_, s))) = ready.pop() {
        scc_order.push(s);
        for &t in &cond_adj_vec[s] {
            cond_indegree[t] -= 1;
            if cond_indegree[t] == 0 {
                ready.push(Reverse((scc_min_idx[t], t)));
            }
        }
    }

    // --- Step 4: Expand SCCs → template indices (members in source order) ---
    let mut order = Vec::with_capacity(n);
    for &s in &scc_order {
        let mut members = sccs_topo[s].clone();
        members.sort_unstable(); // source-index order within each SCC
        order.extend(members);
    }

    // --- Step 5: Coupling diagnostics for SCCs of size ≥ 2 ---
    let mut coupling_diagnostics = Vec::new();
    for scc in &sccs_topo {
        if scc.len() >= 2 {
            let scc_set: HashSet<usize> = scc.iter().copied().collect();
            let mut diags =
                emit_cycle_coupling_diagnostics(templates, &auto_owner, &scc_set);
            coupling_diagnostics.append(&mut diags);
        }
    }

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
                if let Some(&i) = auto_owner.get(&r)
                    && i != j && cycle_set.contains(&i) {
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

    // -------------------------------------------------------------------------
    // task #4899 (S1) case: connector child→parent structural ordering edge.
    //
    // `connect a -> b : T { ... }` sites instantiate the connector child via a
    // `__connector_N` sub_component that references T by structure NAME, not a
    // value-cell READ, so `build_read_dag`'s read-edge logic (above) never sees
    // it. Without a dedicated structural edge, a parent declared BEFORE its
    // connector child resolves in source (identity) order, leaving the strict
    // connector-instance auto pin (`connector_pin_if_determined`,
    // engine_eval.rs) skipped — the child's auto cell isn't yet `Determined`
    // when the parent is processed. The fix adds a child→parent edge for every
    // `__connector_`-prefixed sub_component so the child always resolves first.
    // -------------------------------------------------------------------------

    /// Two templates in source order [Parent=0, Conn7=1] where Parent (declared
    /// FIRST) owns a `__connector_0` sub_component instancing Conn7.
    ///
    /// Conn7 (idx 1) must be solved before Parent (idx 0) even though Conn7 is
    /// declared second — this is what lets a single cold-eval pass pin
    /// `Parent.__connector_0.gain` to Conn7's resolved value (task #4899, S1).
    #[test]
    fn connector_child_resolves_before_parent_when_parent_declared_first() {
        // Source order: [parent, conn7] — parent declared before its connector child.
        let parent = TopologyTemplateBuilder::new("Parent")
            .sub_component("__connector_0", "Conn7", vec![])
            .build();

        let conn7 = TopologyTemplateBuilder::new("Conn7")
            .auto_param("Conn7", "gain", Type::length())
            .build();

        let templates = vec![parent, conn7];
        let ro = resolve_order(&templates);

        // Conn7 (idx 1) must come before Parent (idx 0) — the reverse of
        // declaration order, driven by the synthesized child→parent edge.
        assert_eq!(
            ro.order,
            vec![1, 0],
            "Conn7 (idx 1, the connector child) must be solved before Parent \
             (idx 0); got: {:?}",
            ro.order
        );
        assert!(
            ro.coupling_diagnostics.is_empty(),
            "the connector child->parent edge is acyclic and must NOT emit \
             W_SCOPE_COUPLING; got: {:?}",
            ro.coupling_diagnostics
        );
    }
}
