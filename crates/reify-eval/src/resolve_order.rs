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

use reify_compiler::{CompiledTrait, TopologyTemplate};
use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, ValueCellId};
use reify_ir::{CompiledExpr, CompiledFunction, ValueMap};

use crate::deps::{extract_dependency_trace, extract_value_deps};

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

    /// Pre-solve clusters: maximal groups of mutually-coupled scopes that a
    /// whole-model merged solve would co-optimize (M-WHOLE α, task #5013, PRD
    /// `docs/prds/v0_6/whole-model-objective-coupling.md` §5.1).
    ///
    /// A cluster is a union-find group (over template indices) of size ≥ 2
    /// seeded by (a) non-trivial SCCs and (b) scopes whose OBJECTIVE terms read
    /// another scope's auto cell.  Empty for modules with no cross-scope auto
    /// reads (INV-2).  Consumed by engine_eval to emit `W_COUPLING_APPROXIMATED`
    /// for over-cap clusters, and (once β lands) to drive the merged solve.
    pub(crate) clusters: Vec<Cluster>,
}

/// Merged auto-dimension cap for whole-model clusters (M-WHOLE α, task #5013).
///
/// A cluster whose structural auto-cell count exceeds this cap is degraded to
/// [`ClusterDisposition::ApproximatedFallback`] (bottom-up approximate
/// resolution) rather than attempting the merged solve, and surfaces
/// `W_COUPLING_APPROXIMATED`.  12 is the PRD §11 Q2 tactical value
/// (Nelder-Mead simplex-collapse knee ~10–15); it is a scalar constant, not a
/// design commitment.  In-crate unit tests reference this const so they survive
/// retuning.
pub(crate) const WHOLE_MODEL_CLUSTER_DIM_CAP: usize = 12;

/// Whether a cluster is small enough for the whole-model merged solve, or must
/// fall back to bottom-up approximate resolution (M-WHOLE α, task #5013).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClusterDisposition {
    /// Merged auto-dimension is within [`WHOLE_MODEL_CLUSTER_DIM_CAP`]; β will
    /// co-solve this cluster as one problem.  (Until β lands, the cluster still
    /// resolves bottom-up exactly as today — α only records the disposition.)
    MergedSolve,
    /// Merged auto-dimension exceeds the cap (or, in β, the cluster is otherwise
    /// un-mergeable); the merged solve is skipped and the cluster falls back to
    /// bottom-up approximate resolution.  Surfaces `W_COUPLING_APPROXIMATED`.
    ApproximatedFallback,
}

/// A maximal group of mutually-coupled scopes (M-WHOLE α, task #5013).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cluster {
    /// Member template indices, sorted ascending (into the original template
    /// slice).  Always length ≥ 2 (lone scopes are not clusters).
    pub(crate) scopes: Vec<usize>,
    /// Structural merged auto-dimension: the sum over member scopes of their
    /// `is_auto()` value-cell count.  A conservative upper bound on the true
    /// solver-variable count (α cannot exclude connector-pinned autos without a
    /// snapshot; β refines this).
    pub(crate) dim: usize,
    /// Whether this cluster is within-cap (`MergedSolve`) or over-cap
    /// (`ApproximatedFallback`).
    pub(crate) disposition: ClusterDisposition,
}

/// Cluster-formation context for the expansion-aware clustering pass
/// (JOINT-DRIVE δ, Gap C, task #5334, PRD
/// `docs/prds/v0_6/whole-model-joint-drive-seam.md` §13 Amendment A1).
///
/// Threaded as `Option<&ClusterFormationCtx>` through [`resolve_order`] /
/// [`resolve_order_ordering_and_clusters`] into [`compute_clusters`]'s `Some`
/// branch so that objective terms carrying structural queries
/// (`cost(self.descendants)`) can be EXPANDED at cluster time — surfacing the
/// derived `line_cost` reads that couple a parent objective to a child auto —
/// and so the transitive walk can STOP at `@optimized` cells (PRD design
/// decision 5). Callers that pass `None` keep the pre-#5334 direct-auto
/// objective seed byte-for-byte, which is the executable INV-2 fence.
///
/// The fields mirror the in-scope variables at the engine call sites
/// (`engine_eval.rs`): `values` (pre-solve structural counts only — autos are
/// still `Undef`), the module `functions` table, the shared structural-query
/// `trait_registry`, and the unfold budgets.
pub(crate) struct ClusterFormationCtx<'a> {
    /// Pre-solve value map — read ONLY for structural collection counts by the
    /// `enumerate_*` helpers (never solved autos), so cluster-time expansion
    /// respects `resolve_order`'s "no solved values" contract.
    pub(crate) values: &'a ValueMap,
    /// The module's compiled function table — resolves `@optimized`
    /// `UserFunctionCall` cells so the walk can stop at them.
    pub(crate) functions: &'a [CompiledFunction],
    /// Structural-query trait registry (prelude + module trait defs) used by
    /// `apply_trait_filters` / `apply_cost_aggregation` during expansion.
    pub(crate) trait_registry: &'a HashMap<String, &'a CompiledTrait>,
    /// `self.descendants` DFS depth guard (mirrors `Engine::max_unfold_depth`).
    pub(crate) max_unfold_depth: usize,
    /// `self.descendants` node-count budget (mirrors `Engine::max_unfold_nodes`).
    pub(crate) max_unfold_nodes: usize,
}

/// Output of [`build_read_dag`]: `(auto_owner, adj, objective_reads)`.
///
/// - `auto_owner`: `ValueCellId -> template_index` for all auto cells.
/// - `adj`: adjacency list `adj[i]` = sorted, deduped set of indices j where
///   scope i must be resolved before scope j (i.e. j reads i's auto cell).
/// - `objective_reads`: `objective_reads[j]` = the value-cell reads of template
///   j's objective terms (flattened across terms), cached here so the M-WHOLE α
///   clustering pass and cycle-coupling diagnostics don't re-walk the objective
///   expression trees a second and third time (task #5013).
type ReadDag = (
    HashMap<ValueCellId, usize>,
    Vec<Vec<usize>>,
    Vec<Vec<ValueCellId>>,
);

/// Build the cross-scope auto-cell read-DAG edges (see [`ReadDag`] for the
/// returned tuple's components).
fn build_read_dag(templates: &[TopologyTemplate]) -> ReadDag {
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

    // Build name -> template_index map for connector structural edges (below).
    let name_to_idx: HashMap<&str, usize> = templates
        .iter()
        .enumerate()
        .map(|(i, t)| (t.name.as_str(), i))
        .collect();

    // Build adjacency list: edge i→j means "i must be solved before j".
    // We deduplicate edges.
    let mut edge_set: HashSet<(usize, usize)> = HashSet::new();

    // Per-template objective-term reads, cached so the M-WHOLE α clustering pass
    // (`compute_clusters`) and the cycle-coupling diagnostics
    // (`emit_cycle_coupling_diagnostics`) reuse this single trace instead of
    // re-walking each objective expression tree (task #5013).
    let mut objective_reads: Vec<Vec<ValueCellId>> = vec![Vec::new(); n];

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
        // Collect reads from objective terms — and cache them per template so the
        // clustering pass and cycle-coupling diagnostics reuse this trace rather
        // than re-walking the objective trees (task #5013).
        if let Some(obj) = &template.objective {
            for term in &obj.terms {
                let reads = extract_dependency_trace(&term.expr).reads;
                for r in &reads {
                    if let Some(&i) = auto_owner.get(r)
                        && i != j {
                            edge_set.insert((i, j));
                        }
                }
                objective_reads[j].extend(reads);
            }
        }

        // Connector child→parent structural edges (task #4899, S1).
        //
        // `connect a -> b : T { ... }` sites instantiate the connector child T
        // via a `__connector_N` sub_component that references T by structure
        // NAME, not a value-cell read, so the read-edge logic above never sees
        // it. Without a dedicated edge, a parent declared before its connector
        // child resolves in source (identity) order, so
        // `connector_pin_if_determined` (engine_eval.rs) finds the child's
        // auto cell not yet `Determined` when the parent is processed and the
        // pin is skipped — permanently, since the cold `eval()` path has no
        // fixpoint driver. Force every connector child to resolve before its
        // parent so the single cold pass always pins it.
        //
        // Gated strictly on the `__connector_` prefix to mirror
        // `connector_pin_if_determined`'s exact gate, so ordering for regular
        // sub-components (e.g. `sub bolt = Bolt(...)`) — which are NOT pinned
        // and rely on source/read order — is unaffected (preserves INV-2 for
        // connector-free modules).
        for sub in &template.sub_components {
            if !sub.name.starts_with("__connector_") {
                continue;
            }
            if let Some(&i) = name_to_idx.get(sub.structure_name.as_str())
                && i != j
            {
                edge_set.insert((i, j));
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

    (auto_owner, adj, objective_reads)
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
///
/// `cluster_ctx` selects the cluster-formation seed rule (JOINT-DRIVE δ, task
/// #5334): `Some(ctx)` uses the expansion-aware transitive auto-reaching seed;
/// `None` keeps the legacy direct-auto objective seed byte-for-byte. It only
/// ever widens `clusters` — `order` and `coupling_diagnostics` are identical
/// either way.
pub(crate) fn resolve_order(
    templates: &[TopologyTemplate],
    cluster_ctx: Option<&ClusterFormationCtx>,
) -> ResolveOrder {
    resolve_order_impl(templates, cluster_ctx)
}

/// Ordering-AND-clusters variant for the warm `eval_cached` path (M-WHOLE
/// whole-model co-solve, task #5118).
///
/// Computes `order` AND `clusters` identically to [`resolve_order`] — both
/// share [`resolve_order_impl`] and clusters are always computed — and
/// differs only in `coupling_diagnostics`, which this variant always clears
/// to empty: `eval()` alone owns `W_SCOPE_COUPLING` / `W_COUPLING_APPROXIMATED`
/// emission (engine_eval.rs comment near the warm solver sub-pass) —
/// `eval_cached` must never emit these, so the contract is made explicit
/// here rather than relying on the caller to ignore the field.
pub(crate) fn resolve_order_ordering_and_clusters(
    templates: &[TopologyTemplate],
    cluster_ctx: Option<&ClusterFormationCtx>,
) -> ResolveOrder {
    let mut ro = resolve_order_impl(templates, cluster_ctx);
    ro.coupling_diagnostics = Vec::new();
    ro
}

/// Shared implementation of [`resolve_order`] / [`resolve_order_ordering_and_clusters`].
///
/// Both callers need the same `order` and `clusters` computation; they differ
/// only in whether `coupling_diagnostics` is kept (`resolve_order`, for the
/// cold `eval()` path's `W_COUPLING_APPROXIMATED` emission) or cleared
/// (`resolve_order_ordering_and_clusters`, for the warm `eval_cached` path,
/// which must never emit coupling diagnostics) — each wrapper handles that
/// difference itself after calling this shared implementation.
///
/// `cluster_ctx` is forwarded verbatim to [`compute_clusters`] (JOINT-DRIVE δ,
/// task #5334) and affects nothing else in this function.
fn resolve_order_impl(
    templates: &[TopologyTemplate],
    cluster_ctx: Option<&ClusterFormationCtx>,
) -> ResolveOrder {
    let n = templates.len();
    if n == 0 {
        return ResolveOrder {
            order: Vec::new(),
            coupling_diagnostics: Vec::new(),
            clusters: Vec::new(),
        };
    }

    let (auto_owner, adj, objective_reads) = build_read_dag(templates);

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

    // --- M-WHOLE α (#5013): compute pre-solve clusters ---
    // Purely additive structural analysis: seeds a union-find from the SCC
    // condensation (step-4) and, in step-6, cross-scope objective reads. Never
    // touches `order`. Empty for uncoupled modules (INV-2).
    //
    // Unconditional as of task #5118: both callers (the cold `resolve_order`
    // and the warm `resolve_order_ordering_and_clusters`) need `clusters` —
    // warm co-solves within-cap `MergedSolve` clusters exactly as cold does —
    // so there is no remaining caller that only wants `order`. Previously
    // gated behind a `compute_cluster_set` bool (task #5013) that a warm-only
    // caller set `false`; that caller (`resolve_order_ordering_only`) was
    // deleted when #5118 switched `eval_cached` onto this variant.
    let clusters = compute_clusters(
        templates,
        &auto_owner,
        &sccs_topo,
        &objective_reads,
        cluster_ctx,
    );

    // --- Step 5: Coupling diagnostics for SCCs of size ≥ 2 ---
    //
    // Graduation (M-WHOLE α, §3.4): an over-cap SCC surfaces the more-specific
    // W_COUPLING_APPROXIMATED (emitted from engine_eval, reading `clusters`)
    // INSTEAD of the generic W_SCOPE_COUPLING — emitting both is redundant/noisy.
    // So suppress W_SCOPE_COUPLING for SCCs whose member set belongs to an
    // ApproximatedFallback cluster. Within-cap SCCs still emit W_SCOPE_COUPLING:
    // until β lands they are solved bottom-up approximate, so the generic warning
    // stays accurate (keeps scope_coupling.rs test H green). Only coupled models
    // reach this loop, so resolve_order INV-2 (uncoupled ⇒ byte-identical) holds.
    let over_cap_scopes: HashSet<usize> = clusters
        .iter()
        .filter(|c| c.disposition == ClusterDisposition::ApproximatedFallback)
        .flat_map(|c| c.scopes.iter().copied())
        .collect();
    let mut coupling_diagnostics = Vec::new();
    for scc in &sccs_topo {
        if scc.len() < 2 {
            continue;
        }
        // A non-trivial SCC is always fully unioned into one cluster (seed a), so
        // its members are all-or-none over-cap; `all` reads as "this SCC's cluster
        // is the ApproximatedFallback one".
        if scc.iter().all(|v| over_cap_scopes.contains(v)) {
            continue;
        }
        let scc_set: HashSet<usize> = scc.iter().copied().collect();
        let mut diags =
            emit_cycle_coupling_diagnostics(templates, &auto_owner, &scc_set, &objective_reads);
        coupling_diagnostics.append(&mut diags);
    }

    ResolveOrder {
        order,
        coupling_diagnostics,
        clusters,
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
    objective_reads: &[Vec<ValueCellId>],
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
        // Objective reads come from the shared per-template cache built by
        // `build_read_dag` (task #5013) — no re-walk of the objective trees here.
        // Empty for objectiveless templates, so this is a no-op for them (same as
        // the prior `if let Some(obj)` guard).
        emit_for_reads(objective_reads[j].clone(), None);
    }

    diagnostics
}

// ---------------------------------------------------------------------------
// M-WHOLE α (#5013): pre-solve clustering.
//
// Graduates the SCC condensation from a warning-emitter into a clustering
// actuator. A cluster is a maximal union-find group over template indices,
// seeded by (a) non-trivial SCCs (mutually-coupled scopes — the cyclic case)
// and (b) scopes whose OBJECTIVE terms read another scope's auto cell (the
// acyclic spanning/aggregate-objective case). Groups of size ≥ 2 become
// clusters. Acyclic CONSTRAINT crossings are deliberately NOT clustered — they
// are resolved by ordering (the reader sees the owner frozen), so keying the
// acyclic trigger on OBJECTIVE reads preserves the scope_coupling A–G
// zero-diagnostic acyclic tests and resolve_order INV-2.
// ---------------------------------------------------------------------------

/// Union-find `find` with path compression over a flat parent array.
fn uf_find(parent: &mut [usize], x: usize) -> usize {
    // Walk to the root.
    let mut root = x;
    while parent[root] != root {
        root = parent[root];
    }
    // Path-compress: point every node on the walk directly at the root.
    let mut cur = x;
    while parent[cur] != root {
        let next = parent[cur];
        parent[cur] = root;
        cur = next;
    }
    root
}

/// Union the sets containing `a` and `b`.
fn uf_union(parent: &mut [usize], a: usize, b: usize) {
    let ra = uf_find(parent, a);
    let rb = uf_find(parent, b);
    if ra != rb {
        parent[ra] = rb;
    }
}

/// Count a template's auto value cells — the per-scope solver-variable count
/// (mirrors `build_read_dag`'s `cell.kind.is_auto()` filter).
fn auto_cell_count(template: &TopologyTemplate) -> usize {
    template
        .value_cells
        .iter()
        .filter(|cell| cell.kind.is_auto())
        .count()
}

/// Build the cross-template map of every non-auto value cell's `default_expr`,
/// keyed by cell id (JOINT-DRIVE δ, task #5334, C2).
///
/// Mirrors `build_dependent_cells`' cell_map construction (engine_eval.rs) —
/// autos have no `default_expr` and are skipped; ComputeNode-produced cells
/// without a foldable `default_expr` never enter.  First-writer-wins on
/// duplicate ids (`or_insert`), matching that precedent.  The forward walk in
/// [`union_via_transitive_auto_owners`] follows these derived cells' reads to
/// reach the autos they transitively depend on.
fn build_non_auto_cell_map(templates: &[TopologyTemplate]) -> HashMap<ValueCellId, &CompiledExpr> {
    let mut cell_map: HashMap<ValueCellId, &CompiledExpr> = HashMap::new();
    for template in templates {
        for cell in &template.value_cells {
            if cell.kind.is_auto() {
                continue;
            }
            if let Some(expr) = cell.default_expr.as_ref() {
                cell_map.entry(cell.id.clone()).or_insert(expr);
            }
        }
    }
    cell_map
}

/// Per-template objective reads harvested from CLUSTER-TIME-EXPANDED objective
/// terms (JOINT-DRIVE δ, task #5334, C1).
///
/// For each template carrying an objective, every term's expr is CLONED (PRD §10
/// Phase 1.5: "over throwaway copies — templates not mutated pre-solve"; the
/// templates are only reachable as `&[TopologyTemplate]` here anyway) and run
/// through the engine's own `expand_solver_position_expr`, so
/// `cost(self.descendants)` becomes `[ValueRef(<descendant>.line_cost) ...].sum`
/// and surfaces the derived reads that couple this scope to a child auto.  Plain
/// objectives are left byte-identical by that function's
/// `contains_structural_query` fast path, so this costs ~nothing for the
/// overwhelming majority of modules.
///
/// This vector is consumed ONLY by [`compute_clusters`] — PRD design decision 7's
/// "at cluster time only".  [`build_read_dag`]'s own `objective_reads` (and hence
/// `adj`, `order` and `emit_cycle_coupling_diagnostics`' W_SCOPE_COUPLING text)
/// are deliberately left untouched: feeding expanded reads back into the read-DAG
/// would add ordering edges and change both, breaking INV-2 and the scope_coupling
/// A–G diagnostic surface.
///
/// Expansion diagnostics are collected into a throwaway vec and dropped: the same
/// expansion runs again inside `build_solver_problem` /
/// `build_merged_solver_problem` over the same exprs, so surfacing them here would
/// duplicate every unfold-budget warning in user output (design decision 4).
fn expanded_objective_reads(
    templates: &[TopologyTemplate],
    ctx: &ClusterFormationCtx,
) -> Vec<Vec<ValueCellId>> {
    let mut out: Vec<Vec<ValueCellId>> = vec![Vec::new(); templates.len()];
    let mut throwaway_diags: Vec<Diagnostic> = Vec::new();
    for (j, template) in templates.iter().enumerate() {
        let Some(obj) = &template.objective else {
            continue;
        };
        for term in &obj.terms {
            let mut expanded = term.expr.clone();
            crate::engine_eval::expand_solver_position_expr(
                &mut expanded,
                template,
                templates,
                ctx.values,
                ctx.max_unfold_depth,
                ctx.max_unfold_nodes,
                ctx.trait_registry,
                &mut throwaway_diags,
            );
            out[j].extend(extract_dependency_trace(&expanded).reads);
        }
    }
    out
}

/// Forward-reachability walk that unions scope `j` with the owning scope of
/// every auto cell reachable from `seeds` through derived (non-auto) cells
/// (JOINT-DRIVE δ, task #5334, C2).
///
/// Starting from `seeds` (scope `j`'s objective reads), for each popped id:
/// (i) if it is auto-owned, union `j` with the owner and stop — an auto has no
/// `default_expr` to follow; (ii) otherwise, if it is a derived cell in
/// `cell_map`, follow its `default_expr` reads (`extract_value_deps`)
/// transitively, cross-scope.
///
/// Case (i) SUBSUMES the legacy direct-auto seed: an objective read that IS an
/// auto unions at walk step 1, before any derived cell is consulted.
///
/// A `visited` set makes the walk terminate on cyclic derived-cell reads.  This
/// is a plain forward reachability collecting union pairs — NOT a topological
/// sort — so it needs no `detect_let_cycle` (whose single-template contract
/// stays untouched).  The `@optimized` stop-check is added in step-6.
fn union_via_transitive_auto_owners(
    j: usize,
    seeds: &[ValueCellId],
    cell_map: &HashMap<ValueCellId, &CompiledExpr>,
    auto_owner: &HashMap<ValueCellId, usize>,
    path_map: &HashMap<String, String>,
    functions: &[CompiledFunction],
    parent: &mut [usize],
) {
    let mut visited: HashSet<ValueCellId> = HashSet::new();
    let mut frontier: Vec<ValueCellId> = Vec::new();
    for s in seeds {
        if visited.insert(s.clone()) {
            frontier.push(s.clone());
        }
    }
    while let Some(id) = frontier.pop() {
        // Instance-path fallback (task #5334, review round 2). Consult every map
        // with `id` AS-IS first — the overwhelmingly common structure-name-scoped
        // case — and fall back to the normalised form only on a miss, so this is
        // provably additive and cannot perturb an already-correct lookup. The
        // fallback is computed once per pop and reused by both lookups below.
        let normalized = normalize_cell_id(&id, path_map);
        // (i) Auto-owned id: union j with its owner (if a different scope) and
        // stop — autos are walk leaves (no default_expr).
        if let Some(&owner) = auto_owner
            .get(&id)
            .or_else(|| normalized.as_ref().and_then(|n| auto_owner.get(n)))
        {
            if owner != j {
                uf_union(parent, owner, j);
            }
            continue;
        }
        // (ii) Derived non-auto cell: follow its reads transitively — UNLESS it is
        // an `@optimized` UserFunctionCall cell, whose dependency frontier is
        // deliberately suppressed (PRD design decision 5 / §11). Such a cell's
        // value comes from the compute-dispatch registry, so `build_dependent_cells`
        // excludes it from the per-trial fold with this SAME predicate
        // (engine_eval.rs): a child whose `line_cost` cannot be recomputed per trial
        // must not be co-solved, because the merged problem's objective would be
        // constant in that child's auto. The cell stays `visited`, so the walk
        // terminates normally; only its frontier is cut.
        //
        // Resolve the derived-cell hit while remembering WHICH spelling produced
        // it. The normalised spelling is retired into `visited` ONLY when it was
        // the one actually expanded: marking it unconditionally would retire a
        // DIFFERENT, real cell that the as-is hit merely shadowed. Concretely, a
        // parent declaring its own `Parent.childinst.line_cost` cell (the
        // reify-compiler/src/expr.rs:843 shape) normalises to `Child.line_cost`;
        // if that child cell also exists and carries the `default_expr` chain
        // reaching the child's auto, blanket-marking it visited would drop that
        // chain and silently under-cluster. Termination does not depend on this
        // insert — every frontier push is already guarded by `visited.insert`.
        let hit = match cell_map.get(&id) {
            Some(expr) => Some((*expr, None)),
            None => normalized
                .as_ref()
                .and_then(|n| cell_map.get(n).map(|expr| (*expr, Some(n.clone())))),
        };
        if let Some((expr, expanded_via_normalized)) = hit {
            // Retire only the spelling actually expanded, so an id reached by
            // both spellings is expanded once.
            if let Some(n) = expanded_via_normalized {
                visited.insert(n);
            }
            if crate::engine_eval::is_optimized_userfn_cell(expr, functions) {
                continue;
            }
            for dep in extract_value_deps(expr) {
                if visited.insert(dep.clone()) {
                    frontier.push(dep);
                }
            }
        }
    }
}

/// Map every statically-reachable sub-component INSTANCE PATH to the name of
/// the structure it instantiates (JOINT-DRIVE δ, task #5334, review round 2).
///
/// Mirrors `enumerate_descendants`' path composition
/// (structural_query.rs:213 `{prefix}.{sub}`, :187 `{prefix}.{sub}[{idx}]`),
/// seeded once per template at `prefix = &template.name` exactly as
/// `expand_structural_query` seeds it.  A COLLECTION sub contributes its
/// INDEX-STRIPPED base path — `Rig.bolts[0]` and `Rig.bolts[7]` both denote
/// structure `Bolt`, and the runtime index is not knowable statically — which is
/// why [`normalize_cell_id`] strips `[...]` before consulting the map.
///
/// Deliberately NOT implemented by calling `enumerate_descendants`: that needs a
/// populated `ValueMap` for collection counts (unavailable on the warm path
/// before counts exist) and emits runtime-count-dependent `[idx]` paths.  Because
/// this re-implements those two `format!` shapes, the two sides are pinned
/// together by `instance_path_map_matches_enumerate_descendants_paths` rather
/// than by convention.
///
/// BOUNDS (task #5334, review round 3). The number of distinct instance paths is
/// multiplicative in branching factor across containment depth, so this walk is
/// bounded exactly the way its oracle is (structural_query.rs:157 depth guard,
/// :179/:205 node budget) — it must not be the one unbudgeted structural
/// enumeration on the interactive keystroke path:
///
/// * `max_depth` prunes at entry, `depth >= max_depth`, matching
///   `enumerate_descendants` — deliberately NOT a structure-name cycle set,
///   which would stop at the first repeat and so DIVERGE from the oracle for
///   recursive containment (`A` contains `B` contains `A`), silently missing
///   normalisations for exactly the deeply-nested designs this feature targets.
/// * ONE `max_nodes` budget is shared across every root, so total map size (and
///   hence the `format!`/`String` allocation count) is hard-bounded by
///   `max_nodes` regardless of template count.  A per-root budget would bound
///   only `templates.len() * max_nodes` — still multiplicative in the dimension
///   this guard exists to contain.
///
/// Truncation is SAFE in the conservative direction: a missing entry means
/// [`normalize_cell_id`] returns `None`, the walk finds no auto owner for that
/// id, and the affected scopes simply do not cluster — the pre-#5334 behaviour.
/// Unlike the oracle, exhaustion emits no diagnostic: cluster formation is an
/// optimisation, and the same expansion runs again (with the same budgets, and
/// there surfacing its own warnings) inside `build_solver_problem`.
///
/// One deliberate narrowing versus the oracle: a collection sub decrements the
/// budget ONCE (for its index-stripped base path) rather than once per runtime
/// index, so this side is strictly cheaper and can only ever truncate later.
pub(crate) fn build_instance_path_structure_map(
    templates: &[TopologyTemplate],
    max_depth: usize,
    max_nodes: usize,
) -> HashMap<String, String> {
    fn walk(
        template: &TopologyTemplate,
        templates: &[TopologyTemplate],
        prefix: &str,
        depth: usize,
        max_depth: usize,
        node_budget: &mut usize,
        out: &mut HashMap<String, String>,
    ) {
        // Entry depth guard, mirroring structural_query.rs:157.
        if depth >= max_depth {
            return;
        }
        for sub in &template.sub_components {
            // Node budget, mirroring structural_query.rs:179/:205 — one decrement
            // per emitted path, truncating (never panicking) on exhaustion.
            if *node_budget == 0 {
                return;
            }
            *node_budget -= 1;
            // Collection subs contribute the same index-stripped base path as a
            // plain sub — see the doc comment above.
            let node_path = format!("{}.{}", prefix, sub.name);
            out.entry(node_path.clone())
                .or_insert_with(|| sub.structure_name.clone());
            let Some(child) = templates.iter().find(|t| t.name == sub.structure_name) else {
                continue;
            };
            walk(
                child,
                templates,
                &node_path,
                depth + 1,
                max_depth,
                node_budget,
                out,
            );
        }
    }

    let mut out: HashMap<String, String> = HashMap::new();
    let mut node_budget = max_nodes;
    for template in templates {
        walk(
            template,
            templates,
            &template.name,
            0,
            max_depth,
            &mut node_budget,
            &mut out,
        );
    }
    out
}

/// Rewrite an INSTANCE-PATH-scoped cell id to its DECLARING TEMPLATE's scoping,
/// or `None` when `id.entity` is not a known instance path (JOINT-DRIVE δ, task
/// #5334, review round 2).
///
/// Root cause this exists to bridge, recorded so the next reader need not
/// re-derive it — the two sides are minted with DIFFERENT entity scopings:
///
/// * Template value cells are keyed by STRUCTURE NAME:
///   `ValueCellId::new(&structure.name, &param.name)` (reify-compiler/src/entity.rs:6066)
///   and `ValueCellId::new(&structure.name, &let_decl.name)` (:6197).  Both
///   `auto_owner` and [`build_non_auto_cell_map`] iterate `template.value_cells`
///   and key on `cell.id`, so both maps are structure-name-scoped.
/// * `apply_cost_aggregation` mints INSTANCE-PATH ids —
///   `ValueCellId::new(path, "line_cost")` (reify-eval/src/structural_query.rs:686)
///   over `enumerate_descendants`' composed prefixes (:187, :213) — so a real
///   module's expanded objective reads `Rig.bolts.line_cost`, never
///   `Bolt.line_cost`.
///
/// Instance-path ids are NOT confined to the seed layer: reify-compiler/src/expr.rs:843
/// (and :1211, :3904, :3935, :5595) compile a sub-member access as
/// `format!("{}.{}", scope.entity_name, sub_name)`, so a PARENT template's own
/// let/param `default_expr` reading `childinst.line_cost` yields
/// `ValueCellId::new("Parent.childinst", "line_cost")` — an instance-path id
/// living inside `templates[parent].value_cells` and surfaced by
/// `extract_value_deps` MID-WALK.  Normalisation therefore runs at every hop of
/// [`union_via_transitive_auto_owners`], not only on its seeds.
pub(crate) fn normalize_cell_id(
    id: &ValueCellId,
    path_map: &HashMap<String, String>,
) -> Option<ValueCellId> {
    let stripped = strip_collection_indices(&id.entity);
    path_map
        .get(stripped.as_ref())
        .map(|structure_name| ValueCellId::new(structure_name.as_str(), &id.member))
}

/// Remove every `[...]` collection-index segment from an instance path
/// (`Rig.bolts[3].head` → `Rig.bolts.head`), borrowing unchanged when there is
/// no index to strip (the common case).
pub(crate) fn strip_collection_indices(entity: &str) -> std::borrow::Cow<'_, str> {
    if !entity.contains('[') {
        return std::borrow::Cow::Borrowed(entity);
    }
    let mut out = String::with_capacity(entity.len());
    let mut depth = 0usize;
    for ch in entity.chars() {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Compute the pre-solve cluster set (M-WHOLE α, task #5013).
///
/// Seeds a union-find over `0..templates.len()` from (a) non-trivial SCCs
/// (every pair of mutually-coupled scopes) and (b) cross-scope objective reads,
/// then materialises each resulting group of size ≥ 2 as a [`Cluster`].  `dim`
/// is the structural auto-cell sum over the group; `disposition` is
/// `ApproximatedFallback` when `dim` exceeds [`WHOLE_MODEL_CLUSTER_DIM_CAP`],
/// else `MergedSolve`.  Clusters are sorted by their minimum member index so the
/// output is deterministic.  Returns an empty vec when no group reaches size 2
/// (INV-2).
///
/// `cluster_ctx` selects the objective seed rule (JOINT-DRIVE δ, task #5334):
/// `None` is the legacy direct-auto seed, `Some(ctx)` the transitive
/// auto-reaching walk.  Either way ONLY objective reads seed the union-find.
fn compute_clusters(
    templates: &[TopologyTemplate],
    auto_owner: &HashMap<ValueCellId, usize>,
    sccs_topo: &[Vec<usize>],
    objective_reads: &[Vec<ValueCellId>],
    cluster_ctx: Option<&ClusterFormationCtx>,
) -> Vec<Cluster> {
    let n = templates.len();
    // Union-find over template indices; each node starts in its own set.
    let mut parent: Vec<usize> = (0..n).collect();

    // Seed (a): every non-trivial SCC — union all its members together.
    for scc in sccs_topo {
        if scc.len() >= 2 {
            let first = scc[0];
            for &v in &scc[1..] {
                uf_union(&mut parent, first, v);
            }
        }
    }

    // Seed (b): cross-scope OBJECTIVE reads (the acyclic spanning/aggregate
    // case). For each scope j carrying an objective, union j with the owner of
    // every OTHER scope's auto cell its objective terms read. CONSTRAINT reads
    // are deliberately NOT unioned — an acyclic constraint crossing is resolved
    // by ordering (the reader sees the owner frozen), needs no merge, and must
    // keep forming zero clusters (preserves scope_coupling A–G and INV-2). Both
    // branches below read ONLY `objective_reads`, so that invariant holds
    // regardless of `cluster_ctx`.
    match cluster_ctx {
        // Legacy direct-auto seed (task #5013): union j with the owner of every
        // OTHER scope's auto cell its objective terms read DIRECTLY.
        //
        // Reads come from the `objective_reads` cache (built once in
        // build_read_dag), not a fresh `extract_dependency_trace` walk (task
        // #5013). `objective_reads[j]` is empty for objectiveless templates, so
        // those iterations are no-ops. Byte-identical to the pre-#5334
        // behaviour, and kept as the executable INV-2 fence.
        None => {
            for (j, reads) in objective_reads.iter().enumerate() {
                for r in reads {
                    if let Some(&i) = auto_owner.get(r)
                        && i != j
                    {
                        uf_union(&mut parent, i, j);
                    }
                }
            }
        }
        // Transitive auto-reaching seed (JOINT-DRIVE δ, task #5334, C2). Builds a
        // cross-template map of every non-auto cell's `default_expr`, then for
        // each scope j walks j's objective reads forward through those derived
        // cells, unioning j with the owner of any auto the walk reaches. This
        // SUBSUMES the direct seed above (an objective read that IS an auto
        // unions at walk step 1) and additionally follows a derived Let cell
        // (`line_cost = unit_cost * quantity_produced`) down to the child auto
        // behind it — the joint-drive leaf coupling.
        //
        // The walk is seeded from the CLUSTER-TIME-EXPANDED objective reads (C1,
        // [`expanded_objective_reads`]) rather than the unexpanded
        // `objective_reads` cache, so `cost(self.descendants)` surfaces its
        // `line_cost` reads before the walk starts. The unexpanded cache is still
        // what the `None` branch — and the read-DAG / cycle diagnostics — use.
        //
        // OBJECTIVELESS FAST PATH (task #5334, review round 3). Only objective
        // reads ever seed the union-find, so a module where NO template carries
        // an objective can never form a δ cluster — every structure built below
        // would be constructed and immediately discarded. This arm now runs on
        // the cold eval()/check() path for every module and, on the warm path,
        // on every keystroke whenever a solver is active, and objectiveless is
        // the common case; the `contains_structural_query` fast path inside
        // `expanded_objective_reads` covers only that one of the three builds.
        // Skipping is exactly equivalent: `expanded_objective_reads` would
        // return all-empty read vectors, over which the walk is a no-op.
        Some(ctx) if templates.iter().any(|t| t.objective.is_some()) => {
            let cell_map = build_non_auto_cell_map(templates);
            // Built ONCE per call (not once per scope): the instance-path →
            // structure-name map that lets the walk resolve compiler-emitted
            // instance-path ids (`Rig.bolts.line_cost`) against the
            // structure-name-keyed `auto_owner` / `cell_map` (`Bolt.line_cost`).
            // See [`normalize_cell_id`] for the two-sided root cause, and
            // [`build_instance_path_structure_map`] for why it is budgeted.
            let path_map = build_instance_path_structure_map(
                templates,
                ctx.max_unfold_depth,
                ctx.max_unfold_nodes,
            );
            let expanded_reads = expanded_objective_reads(templates, ctx);
            for (j, reads) in expanded_reads.iter().enumerate() {
                // An objectiveless scope contributes no seeds; skip the walk's
                // per-scope HashSet/Vec allocation rather than entering it empty.
                if reads.is_empty() {
                    continue;
                }
                union_via_transitive_auto_owners(
                    j,
                    reads,
                    &cell_map,
                    auto_owner,
                    &path_map,
                    ctx.functions,
                    &mut parent,
                );
            }
        }
        // `Some(ctx)` with no objective anywhere — nothing can seed; see above.
        Some(_) => {}
    }

    // Group members by union-find root.
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for v in 0..n {
        let root = uf_find(&mut parent, v);
        groups.entry(root).or_default().push(v);
    }

    // Materialise groups of size ≥ 2 as clusters (a lone scope is not a cluster).
    let mut clusters: Vec<Cluster> = Vec::new();
    for (_root, mut members) in groups {
        if members.len() < 2 {
            continue;
        }
        members.sort_unstable();
        let dim: usize = members
            .iter()
            .map(|&idx| auto_cell_count(&templates[idx]))
            .sum();
        // Over-cap gate: a merged auto-dimension above the cap degrades to
        // bottom-up approximate resolution (surfaces W_COUPLING_APPROXIMATED).
        let disposition = if dim > WHOLE_MODEL_CLUSTER_DIM_CAP {
            ClusterDisposition::ApproximatedFallback
        } else {
            ClusterDisposition::MergedSolve
        };
        clusters.push(Cluster {
            scopes: members,
            dim,
            disposition,
        });
    }

    // Deterministic order: by minimum member index (== scopes[0], since sorted).
    clusters.sort_by_key(|c| c.scopes[0]);
    clusters
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use reify_compiler::CompiledTrait;
    use reify_core::{ContentHash, Diagnostic, DimensionVector, Type, ValueCellId};
    use reify_ir::{
        BinOp, CompiledFnBody, CompiledFunction, ObjectiveSense, ObjectiveSet, Value, ValueMap,
    };
    use reify_test_support::{
        TopologyTemplateBuilder, binop, fn_call, gt, literal, method_call_expr, mm, user_fn_call,
        value_ref, value_ref_typed,
    };

    use super::{
        ClusterDisposition, ClusterFormationCtx, WHOLE_MODEL_CLUSTER_DIM_CAP, resolve_order,
        resolve_order_ordering_and_clusters,
    };

    // DIVISION OF LABOUR (task #5334, review round 2). The
    // `TopologyTemplateBuilder` fixtures below pin the union RULE in isolation —
    // that is the right tool for it. They do NOT pin the compiler's actual
    // CELL-ID SHAPE; `delta_cluster_forms_on_compiler_emitted_cell_ids` (built
    // from real `.ri` source) and tests/harness_engine/joint_drive_cluster_formation.rs
    // do that. Both halves are required: before this round the builder fixtures
    // declared each CHILD's own cells with entity `"Parent.childinst"` — a shape
    // reify-compiler never emits — and that synthetic scoping was the only reason
    // a walk which unioned nothing on every real module looked green. Every
    // builder fixture here now uses structure-name entities, matching
    // `ValueCellId::new(&structure.name, ..)` (reify-compiler/src/entity.rs:6066,
    // :6197). The single deliberate exception is
    // `derived_cell_reading_sub_member_path_reaches_child_auto`, where the
    // instance-path spelling IS the thing under test.

    /// Money-dimensioned scalar type shared by the derived-cost cluster fixtures
    /// (`line_cost : Money`), mirroring the stdlib `Costed` shape.
    fn money_ty() -> Type {
        Type::Scalar {
            dimension: DimensionVector::MONEY,
        }
    }

    /// The `minimize cost(self.descendants)` objective used by the elaborated
    /// derived-cost fixtures (BT-8b / BT-9). `self.descendants` is the compiler's
    /// `MethodCall { object: ValueRef(Parent.__self), method: "descendants" }`
    /// placeholder that `expand_structural_query` rewrites to a list of the
    /// parent's structural descendants, which `apply_cost_aggregation` then turns
    /// into `[ValueRef(<descendant>.line_cost) ...].sum`.
    fn cost_self_descendants_objective() -> ObjectiveSet {
        ObjectiveSet::single(
            ObjectiveSense::Minimize,
            fn_call(
                "cost",
                "cost",
                vec![method_call_expr(
                    value_ref("Parent", "__self"),
                    "descendants",
                    vec![],
                    Type::List(Box::new(Type::StructureRef("Structure".to_string()))),
                )],
                money_ty(),
            ),
        )
    }

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
        let ro = resolve_order(&templates, None);

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
        let ro = resolve_order(&templates, None);

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
        let ro = resolve_order(&templates, None);

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
        let ro = resolve_order(&templates, None);

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
        let ro = resolve_order(&templates, None);

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
        let ro = resolve_order(&templates, None);

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

    // -------------------------------------------------------------------------
    // M-WHOLE α (#5013): pre-solve clustering pass.
    //
    // resolve_order graduates its SCC condensation from a warning-emitter into a
    // clustering ACTUATOR: `ResolveOrder` gains a `clusters` field — a union-find
    // over template indices seeded by (a) non-trivial SCCs (mutually-coupled
    // scopes) and (b) scopes whose OBJECTIVE terms read another scope's auto
    // cell. Groups of size ≥ 2 become clusters carrying {scopes, dim,
    // disposition}. These in-crate tests assert the structural cluster set
    // directly (they can see the pub(crate) const/enum via `super::`).
    // -------------------------------------------------------------------------

    /// (α-INV-2) A module with NO cross-scope auto reads yields ZERO clusters
    /// AND a byte-identical resolution order to today (`[0, 1]`).
    ///
    /// Cluster computation is purely additive — it never touches `order`. An
    /// empty cluster set means engine_eval emits nothing (no behavior change).
    #[test]
    fn no_cross_scope_reads_yields_zero_clusters_and_identity_order() {
        // Two scopes, each constrains only its OWN auto cell — no crossing.
        let x = TopologyTemplateBuilder::new("X")
            .auto_param("X", "a", Type::length())
            .constraint("X", 0, None, gt(value_ref("X", "a"), literal(mm(0.0))))
            .build();

        let y = TopologyTemplateBuilder::new("Y")
            .auto_param("Y", "b", Type::length())
            .constraint("Y", 0, None, gt(value_ref("Y", "b"), literal(mm(0.0))))
            .build();

        let templates = vec![x, y];
        let ro = resolve_order(&templates, None);

        assert!(
            ro.clusters.is_empty(),
            "no cross-scope reads: cluster set must be empty (INV-2); got: {:?}",
            ro.clusters
        );
        assert_eq!(
            ro.order,
            vec![0, 1],
            "cluster computation must not perturb `order` (INV-2); got: {:?}",
            ro.order
        );
    }

    /// (α-SCC) An irreducible 2-cycle {A, B} (A reads B.m, B reads A.k), one
    /// auto cell each, forms exactly one cluster spanning both scopes.
    ///
    /// dim = 1 + 1 = 2 (within the cap) ⇒ `MergedSolve`.
    #[test]
    fn irreducible_two_cycle_forms_single_merged_cluster() {
        // A reads B.m, B reads A.k → irreducible 2-cycle (SCC of size 2).
        let a = TopologyTemplateBuilder::new("A")
            .auto_param("A", "k", Type::length())
            .constraint("A", 0, None, gt(value_ref("B", "m"), literal(mm(0.0))))
            .build();

        let b = TopologyTemplateBuilder::new("B")
            .auto_param("B", "m", Type::length())
            .constraint("B", 0, None, gt(value_ref("A", "k"), literal(mm(0.0))))
            .build();

        let templates = vec![a, b];
        let ro = resolve_order(&templates, None);

        assert_eq!(
            ro.clusters.len(),
            1,
            "SCC {{A,B}} must form exactly one cluster; got: {:?}",
            ro.clusters
        );
        let cluster = &ro.clusters[0];
        assert_eq!(
            cluster.scopes,
            vec![0, 1],
            "cluster must contain both scopes in sorted index order; got: {:?}",
            cluster.scopes
        );
        assert_eq!(
            cluster.dim, 2,
            "dim = sum of auto counts = 1 + 1 = 2; got: {}",
            cluster.dim
        );
        assert_eq!(
            cluster.disposition,
            ClusterDisposition::MergedSolve,
            "dim 2 is within cap ⇒ MergedSolve; got: {:?}",
            cluster.disposition
        );
    }

    /// (α-BT1) Acyclic aggregate-objective cluster. A Parent (idx 0) carries an
    /// objective `minimize ChildA.cost + ChildB.cost` reading two children's
    /// auto cells; ChildA (idx 1) and ChildB (idx 2) each own their `cost` auto.
    ///
    /// The read-DAG is acyclic (children → parent) so no SCC forms — but the
    /// objective-read union rule must still cluster all three scopes (the
    /// "degenerate aggregate over descendants" case, §3.2; matches BT1 parent
    /// minimize cost(children)). One cluster, scopes [0,1,2], within cap ⇒
    /// MergedSolve.
    #[test]
    fn aggregate_objective_forms_single_spanning_cluster() {
        // Parent: 1 own auto + objective minimize(ChildA.cost + ChildB.cost).
        let parent = TopologyTemplateBuilder::new("Parent")
            .auto_param("Parent", "total", Type::length())
            .objective(ObjectiveSet::single(
                ObjectiveSense::Minimize,
                binop(
                    BinOp::Add,
                    value_ref("ChildA", "cost"),
                    value_ref("ChildB", "cost"),
                ),
            ))
            .build();

        let child_a = TopologyTemplateBuilder::new("ChildA")
            .auto_param("ChildA", "cost", Type::length())
            .build();

        let child_b = TopologyTemplateBuilder::new("ChildB")
            .auto_param("ChildB", "cost", Type::length())
            .build();

        // Source order: [Parent=0, ChildA=1, ChildB=2].
        let templates = vec![parent, child_a, child_b];
        let ro = resolve_order(&templates, None);

        assert_eq!(
            ro.clusters.len(),
            1,
            "aggregate objective must form exactly one spanning cluster; got: {:?}",
            ro.clusters
        );
        let cluster = &ro.clusters[0];
        assert_eq!(
            cluster.scopes,
            vec![0, 1, 2],
            "cluster must span Parent + both children (sorted indices); got: {:?}",
            cluster.scopes
        );
        assert_eq!(
            cluster.disposition,
            ClusterDisposition::MergedSolve,
            "dim 3 is within cap ⇒ MergedSolve; got: {:?}",
            cluster.disposition
        );
    }

    /// (α-over-cap) A 2-cycle {A, B} whose merged auto-dimension exceeds the cap
    /// degrades to `ApproximatedFallback`.
    ///
    /// A carries `CAP + 1` auto cells and B carries 1, so merged dim = CAP + 2.
    /// This in-crate test references `super::WHOLE_MODEL_CLUSTER_DIM_CAP` so the
    /// assertion stays robust if the cap is retuned.
    #[test]
    fn over_cap_two_cycle_degrades_to_approximated_fallback() {
        let cap = WHOLE_MODEL_CLUSTER_DIM_CAP;

        // A: CAP + 1 auto cells (A.k0 is read by B; the rest pad the dimension)
        // plus a constraint reading B.m to close the 2-cycle.
        let mut a = TopologyTemplateBuilder::new("A").constraint(
            "A",
            0,
            None,
            gt(value_ref("B", "m"), literal(mm(0.0))),
        );
        for idx in 0..(cap + 1) {
            a = a.auto_param("A", &format!("k{idx}"), Type::length());
        }
        let a = a.build();

        // B: 1 auto cell (B.m) plus a constraint reading A.k0 to close the cycle.
        let b = TopologyTemplateBuilder::new("B")
            .auto_param("B", "m", Type::length())
            .constraint("B", 0, None, gt(value_ref("A", "k0"), literal(mm(0.0))))
            .build();

        let templates = vec![a, b];
        let ro = resolve_order(&templates, None);

        assert_eq!(
            ro.clusters.len(),
            1,
            "over-cap 2-cycle must still form exactly one cluster; got: {:?}",
            ro.clusters
        );
        let cluster = &ro.clusters[0];
        assert_eq!(
            cluster.dim,
            cap + 2,
            "dim = (CAP+1) A-autos + 1 B-auto = CAP+2; got: {}",
            cluster.dim
        );
        assert_eq!(
            cluster.disposition,
            ClusterDisposition::ApproximatedFallback,
            "dim {} > cap {} ⇒ ApproximatedFallback; got: {:?}",
            cluster.dim, cap, cluster.disposition
        );
    }

    // -------------------------------------------------------------------------
    // task #5118: `resolve_order_ordering_and_clusters` — the warm `eval_cached`
    // variant. It DOES compute the M-WHOLE α cluster set (so warm can co-solve
    // within-cap MergedSolve clusters), but — like the cold `resolve_order` —
    // must NOT let that leak into a diagnostic contract change: it clears
    // `coupling_diagnostics` unconditionally, since `eval()` alone owns
    // W_SCOPE_COUPLING / W_COUPLING_APPROXIMATED emission (engine_eval.rs
    // comment near :6459).
    // -------------------------------------------------------------------------

    /// A within-cap irreducible 2-cycle {A, B} (same fixture as
    /// `irreducible_two_cycle_forms_single_merged_cluster`) must, under the new
    /// warm variant: (a) resolve to the SAME `order` as the cold
    /// `resolve_order` (both share `resolve_order_impl`, so `order` is
    /// identical); (b) form exactly one `MergedSolve` cluster spanning both
    /// scopes; (c) emit ZERO coupling diagnostics.
    #[test]
    fn resolve_order_ordering_and_clusters_returns_within_cap_cluster_and_no_coupling_diags() {
        // A reads B.m, B reads A.k → irreducible 2-cycle (SCC of size 2), within-cap.
        let a = TopologyTemplateBuilder::new("A")
            .auto_param("A", "k", Type::length())
            .constraint("A", 0, None, gt(value_ref("B", "m"), literal(mm(0.0))))
            .build();

        let b = TopologyTemplateBuilder::new("B")
            .auto_param("B", "m", Type::length())
            .constraint("B", 0, None, gt(value_ref("A", "k"), literal(mm(0.0))))
            .build();

        let templates = vec![a, b];

        let cold = resolve_order(&templates, None);
        let ordering_and_clusters = resolve_order_ordering_and_clusters(&templates, None);

        // (a) `order` must be identical to the cold `resolve_order`'s order.
        assert_eq!(
            ordering_and_clusters.order, cold.order,
            "order must be identical to resolve_order's order (shared resolve_order_impl); got: {:?} vs {:?}",
            ordering_and_clusters.order, cold.order
        );

        // (b) exactly one MergedSolve cluster spanning both scopes.
        assert_eq!(
            ordering_and_clusters.clusters.len(),
            1,
            "within-cap 2-cycle must form exactly one cluster; got: {:?}",
            ordering_and_clusters.clusters
        );
        let cluster = &ordering_and_clusters.clusters[0];
        assert_eq!(
            cluster.scopes,
            vec![0, 1],
            "cluster must span both scopes (sorted indices); got: {:?}",
            cluster.scopes
        );
        assert_eq!(
            cluster.disposition,
            ClusterDisposition::MergedSolve,
            "dim 2 is within cap ⇒ MergedSolve; got: {:?}",
            cluster.disposition
        );

        // (c) warm must NOT emit coupling diagnostics (eval() alone owns
        // W_SCOPE_COUPLING / W_COUPLING_APPROXIMATED).
        assert!(
            ordering_and_clusters.coupling_diagnostics.is_empty(),
            "resolve_order_ordering_and_clusters must not emit coupling diagnostics; got: {:?}",
            ordering_and_clusters.coupling_diagnostics
        );
    }

    // -------------------------------------------------------------------------
    // Gap C (JOINT-DRIVE δ, task #5334): derived-cost coupling cluster formation.
    //
    // These tests drive the expansion-aware `Some(&ClusterFormationCtx)` branch of
    // `compute_clusters`, which forms a MergedSolve cluster for the joint-drive
    // leaf shape — a parent objective coupled to a child auto ONLY through a
    // derived Let cell (`line_cost`). The legacy `None` branch (exercised by every
    // test above) keeps the direct-auto seed byte-for-byte, which is the executable
    // INV-2 fence.
    // -------------------------------------------------------------------------

    /// (BT-8a, pre-expanded) A Parent (idx 0) objective that ALREADY reads the
    /// child's derived `line_cost` cell as a plain `ValueRef` (no `cost()`
    /// expansion needed, so ONLY the C2 transitive walk is under test) must form a
    /// single MergedSolve cluster with the Child (idx 1): the walk follows
    /// `line_cost`'s `default_expr` (`unit_cost * quantity_produced`) down to the
    /// child's auto `quantity_produced` and unions the two scopes.
    ///
    /// dim = 1 (the single child auto) ⇒ within cap ⇒ `MergedSolve`.
    #[test]
    fn bt8_pre_expanded_derived_cost_forms_single_merged_cluster() {
        // Parent (idx 0): objective minimize Child.line_cost — a plain ValueRef.
        let parent = TopologyTemplateBuilder::new("Parent")
            .objective(ObjectiveSet::single(
                ObjectiveSense::Minimize,
                value_ref_typed("Child", "line_cost", money_ty()),
            ))
            .build();

        // Child (idx 1): owns auto `quantity_produced`; derived Let
        // `line_cost = unit_cost * quantity_produced` transitively reads the auto.
        let child = TopologyTemplateBuilder::new("Child")
            .trait_bound("Costed")
            .auto_param_free("Child", "quantity_produced", Type::dimensionless_scalar())
            .let_binding(
                "Child",
                "line_cost",
                money_ty(),
                binop(
                    BinOp::Mul,
                    value_ref_typed("Child", "unit_cost", money_ty()),
                    value_ref("Child", "quantity_produced"),
                ),
            )
            .build();

        let templates = vec![parent, child];

        let values = ValueMap::default();
        let no_fns: [CompiledFunction; 0] = [];
        let registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let ctx = ClusterFormationCtx {
            values: &values,
            functions: &no_fns,
            trait_registry: &registry,
            max_unfold_depth: 64,
            max_unfold_nodes: 10_000,
        };
        let ro = resolve_order(&templates, Some(&ctx));

        assert_eq!(
            ro.clusters.len(),
            1,
            "derived-cost coupling must form exactly one cluster; got: {:?}",
            ro.clusters
        );
        let cluster = &ro.clusters[0];
        assert_eq!(
            cluster.scopes,
            vec![0, 1],
            "cluster must span Parent + Child (sorted indices); got: {:?}",
            cluster.scopes
        );
        assert_eq!(
            cluster.dim, 1,
            "dim = 1 (the single child auto `quantity_produced`); got: {}",
            cluster.dim
        );
        assert_eq!(
            cluster.disposition,
            ClusterDisposition::MergedSolve,
            "dim 1 is within cap ⇒ MergedSolve; got: {:?}",
            cluster.disposition
        );
    }

    /// (BT-8b, elaborated) A Parent (idx 0) carrying a REAL
    /// `minimize cost(self.descendants)` objective over its Costed child
    /// sub-component must form a single MergedSolve cluster with the Child (idx
    /// 1). Cluster-time expansion (C1) rewrites `cost(self.descendants)` to
    /// `[ValueRef(Parent.childinst.line_cost)].sum`, and the C2 walk follows that
    /// derived cell down to the child auto `quantity_produced`.
    ///
    /// The Child's value cells are STRUCTURE-NAME-scoped (`Child.line_cost`,
    /// `Child.quantity_produced`), exactly as reify-compiler mints them
    /// (entity.rs:6066/:6197), so the instance-path ValueRef that C1 produces
    /// only resolves via [`normalize_cell_id`] — this fixture therefore exercises
    /// the real end-to-end shape rather than a synthetic one.
    ///
    /// An EMPTY trait registry suffices: `satisfies_trait_bound` short-circuits on
    /// name equality (`trait_satisfies`, reify-compiler/src/entity.rs), so the
    /// child's declared `Costed` bound conforms without a registry entry — the
    /// registry is only consulted for trait REFINEMENT chains.
    #[test]
    fn bt8_cost_self_descendants_forms_single_merged_cluster() {
        let parent = TopologyTemplateBuilder::new("Parent")
            .sub_component("childinst", "Child", vec![])
            .objective(cost_self_descendants_objective())
            .build();

        let child = TopologyTemplateBuilder::new("Child")
            .trait_bound("Costed")
            .auto_param_free("Child", "quantity_produced", Type::dimensionless_scalar())
            .let_binding(
                "Child",
                "line_cost",
                money_ty(),
                binop(
                    BinOp::Mul,
                    value_ref_typed("Child", "unit_cost", money_ty()),
                    value_ref("Child", "quantity_produced"),
                ),
            )
            .build();

        let templates = vec![parent, child];

        let values = ValueMap::default();
        let no_fns: [CompiledFunction; 0] = [];
        let registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let ctx = ClusterFormationCtx {
            values: &values,
            functions: &no_fns,
            trait_registry: &registry,
            max_unfold_depth: 64,
            max_unfold_nodes: 10_000,
        };
        let ro = resolve_order(&templates, Some(&ctx));

        assert_eq!(
            ro.clusters.len(),
            1,
            "cost(self.descendants) must expand + form exactly one cluster; got: {:?}",
            ro.clusters
        );
        let cluster = &ro.clusters[0];
        assert_eq!(
            cluster.scopes,
            vec![0, 1],
            "cluster must span Parent + Child (sorted indices); got: {:?}",
            cluster.scopes
        );
        assert_eq!(
            cluster.dim, 1,
            "dim = 1 (the single child auto `quantity_produced`); got: {}",
            cluster.dim
        );
        assert_eq!(
            cluster.disposition,
            ClusterDisposition::MergedSolve,
            "dim 1 is within cap ⇒ MergedSolve; got: {:?}",
            cluster.disposition
        );
    }

    /// A `CompiledFunction` named `name` with one `length` param carrying
    /// `optimized_target = Some(target)` — the static marker
    /// `is_optimized_userfn_cell` reads to identify an `@optimized` call (copied
    /// from `tests/joint_drive_expansion_boundary.rs`). Its body is inert; cluster
    /// formation only inspects the name/arity/param-type match + optimized_target.
    /// The param type is `length` so it matches a bare `value_ref(..)` argument
    /// (which defaults to a length result type) under
    /// `find_matching_compiled_function`'s exact-type rule.
    fn optimized_length_fn(name: &str, target: &str) -> CompiledFunction {
        CompiledFunction {
            name: name.to_string(),
            doc: None,
            is_pub: false,
            params: vec![("x".to_string(), Type::length())],
            param_defaults: vec![None],
            return_type: Type::length(),
            body: CompiledFnBody {
                let_bindings: vec![],
                result_expr: literal(mm(0.0)),
            },
            content_hash: ContentHash::of_str(name),
            annotations: vec![],
            optimized_target: Some(target.to_string()),
            type_params: vec![],
        }
    }

    /// (BT-9, negative) The SAME elaborated shape as BT-8b, but the child's
    /// `line_cost` is an `@optimized` `UserFunctionCall` (`opt_fn(...)` whose
    /// `CompiledFunction.optimized_target` is `Some`). The walk must STOP at the
    /// `@optimized` cell and never reach the auto behind it, so NO cluster forms.
    ///
    /// PRD design decision 5 / §11: an `@optimized` cell's value comes from the
    /// compute-dispatch registry and is excluded from the per-trial fold, so a
    /// child whose `line_cost` cannot be recomputed per trial must NOT be
    /// co-solved — this is a deliberate non-coupling, not a missed edge.
    #[test]
    fn bt9_optimized_line_cost_forms_no_cluster() {
        let parent = TopologyTemplateBuilder::new("Parent")
            .sub_component("childinst", "Child", vec![])
            .objective(cost_self_descendants_objective())
            .build();

        let child = TopologyTemplateBuilder::new("Child")
            .trait_bound("Costed")
            .auto_param_free("Child", "quantity_produced", Type::dimensionless_scalar())
            // line_cost = opt_fn(quantity_produced) — an @optimized UserFunctionCall
            // that still transitively reads the child's auto.
            .let_binding(
                "Child",
                "line_cost",
                money_ty(),
                user_fn_call(
                    "opt_fn",
                    vec![value_ref("Child", "quantity_produced")],
                    money_ty(),
                ),
            )
            .build();

        let templates = vec![parent, child];

        let values = ValueMap::default();
        let fns = [optimized_length_fn("opt_fn", "kernel::line_cost")];
        let registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let ctx = ClusterFormationCtx {
            values: &values,
            functions: &fns,
            trait_registry: &registry,
            max_unfold_depth: 64,
            max_unfold_nodes: 10_000,
        };
        let ro = resolve_order(&templates, Some(&ctx));

        assert!(
            ro.clusters.is_empty(),
            "an @optimized derived cost deliberately decouples the child (PRD design \
             decision 5 / §11 documented limitation): the walk must stop at the \
             @optimized cell and form NO cluster; got: {:?}",
            ro.clusters
        );
    }

    // -------------------------------------------------------------------------
    // JOINT-DRIVE δ invariant fences (task #5334, step-7)
    //
    // These lock the contracts the production wiring (steps 9/10) must not
    // break. They are expected GREEN on arrival: if any turns RED, the
    // transitive walk is over-reaching and must be narrowed BEFORE the engine
    // call sites start passing `Some(&ctx)`.
    // -------------------------------------------------------------------------

    /// A Money-dimensioned scalar literal for the constraint-side fence below
    /// (no `money(..)` constructor exists in reify-test-support; the local
    /// `money_ty()` covers the type side).
    fn money(v: f64) -> Value {
        Value::Scalar {
            si_value: v,
            dimension: DimensionVector::MONEY,
        }
    }

    /// (BT-10, negative fence) A Parent whose CONSTRAINT — never its objective —
    /// transitively reads the Child's auto through the derived `line_cost` Let
    /// cell must still form ZERO clusters under `Some(&ctx)`.
    ///
    /// The transitive walk is seeded from OBJECTIVE reads ONLY, so this holds by
    /// construction; the test is the executable fence against a future widening
    /// that seeds it from constraint reads too. That widening would be a
    /// silent INV-2 break: an acyclic constraint crossing is resolved by
    /// ORDERING (the reader sees the owner frozen), needs no merged solve, and
    /// the `tests/scope_coupling.rs` A–G cases all depend on it forming zero
    /// clusters.
    ///
    /// Note the crossing is invisible to the read-DAG as well: `line_cost` is a
    /// Let, not an auto, so `build_read_dag` adds no edge and the order stays
    /// identity — exactly as it does on the pre-#5334 path.
    #[test]
    fn bt10_constraint_only_transitive_read_forms_no_cluster() {
        // Parent (idx 0): a CONSTRAINT (not an objective) reading the child's
        // derived cost cell — `Child.line_cost > 0` (Money).
        let parent = TopologyTemplateBuilder::new("Parent")
            .constraint(
                "Parent",
                0,
                None,
                gt(
                    value_ref_typed("Child", "line_cost", money_ty()),
                    literal(money(0.0)),
                ),
            )
            .build();

        // Child (idx 1): identical to BT-8a's — derived `line_cost` over the
        // child's own auto `quantity_produced`.
        let child = TopologyTemplateBuilder::new("Child")
            .trait_bound("Costed")
            .auto_param_free("Child", "quantity_produced", Type::dimensionless_scalar())
            .let_binding(
                "Child",
                "line_cost",
                money_ty(),
                binop(
                    BinOp::Mul,
                    value_ref_typed("Child", "unit_cost", money_ty()),
                    value_ref("Child", "quantity_produced"),
                ),
            )
            .build();

        let templates = vec![parent, child];

        let values = ValueMap::default();
        let no_fns: [CompiledFunction; 0] = [];
        let registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let ctx = ClusterFormationCtx {
            values: &values,
            functions: &no_fns,
            trait_registry: &registry,
            max_unfold_depth: 64,
            max_unfold_nodes: 10_000,
        };
        let ro = resolve_order(&templates, Some(&ctx));

        assert!(
            ro.clusters.is_empty(),
            "a CONSTRAINT-only transitive read must NOT form a cluster — the δ walk \
             is seeded from objective reads only (INV-2 / scope_coupling A–G); got: {:?}",
            ro.clusters
        );
        assert_eq!(
            ro.order,
            vec![0, 1],
            "a derived-cell crossing adds no read-DAG edge, so order stays identity; got: {:?}",
            ro.order
        );
    }

    /// (INV-2 under ctx) An uncoupled two-scope module resolves IDENTICALLY
    /// whether cluster formation runs the legacy direct-auto seed (`None`) or
    /// the δ expansion-aware seed (`Some(&ctx)`): same identity `order`, zero
    /// clusters, and the same `coupling_diagnostics`.
    ///
    /// This is the proof that cluster-time expansion did not leak back into the
    /// read-DAG (PRD design decision 7, "at cluster time only"): feeding
    /// expanded objective reads into `build_read_dag` would add ordering edges
    /// and change both `order` and the W_SCOPE_COUPLING text.
    ///
    /// The fixture is `no_cross_scope_reads_yields_zero_clusters_and_identity_order`'s,
    /// plus a SELF-objective on X (`minimize X.a`, X's own auto). The
    /// self-objective keeps the module uncoupled — a same-scope read unions
    /// nothing — while ensuring `expanded_objective_reads` actually RUNS, so the
    /// no-leak claim is tested rather than vacuously true of an objectiveless
    /// module.
    #[test]
    fn inv2_uncoupled_module_identical_with_and_without_cluster_ctx() {
        let x = TopologyTemplateBuilder::new("X")
            .auto_param("X", "a", Type::length())
            .constraint("X", 0, None, gt(value_ref("X", "a"), literal(mm(0.0))))
            .objective(ObjectiveSet::single(
                ObjectiveSense::Minimize,
                value_ref("X", "a"),
            ))
            .build();

        let y = TopologyTemplateBuilder::new("Y")
            .auto_param("Y", "b", Type::length())
            .constraint("Y", 0, None, gt(value_ref("Y", "b"), literal(mm(0.0))))
            .build();

        let templates = vec![x, y];

        let ro_legacy = resolve_order(&templates, None);

        let values = ValueMap::default();
        let no_fns: [CompiledFunction; 0] = [];
        let registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let ctx = ClusterFormationCtx {
            values: &values,
            functions: &no_fns,
            trait_registry: &registry,
            max_unfold_depth: 64,
            max_unfold_nodes: 10_000,
        };
        let ro_delta = resolve_order(&templates, Some(&ctx));

        assert_eq!(
            ro_delta.order,
            vec![0, 1],
            "uncoupled module keeps identity order under the δ seed (INV-2); got: {:?}",
            ro_delta.order
        );
        assert_eq!(
            ro_delta.order, ro_legacy.order,
            "the δ seed must not perturb `order` relative to the legacy seed; got: {:?} vs {:?}",
            ro_delta.order, ro_legacy.order
        );
        assert!(
            ro_delta.clusters.is_empty(),
            "a self-objective reads only its OWN scope's auto: no cluster; got: {:?}",
            ro_delta.clusters
        );
        assert_eq!(
            ro_delta.clusters, ro_legacy.clusters,
            "uncoupled module: both seeds must agree on the (empty) cluster set"
        );
        // `Diagnostic` has no `PartialEq`; compare its derived `Debug` rendering,
        // which covers severity, message, labels and code.
        assert_eq!(
            format!("{:?}", ro_delta.coupling_diagnostics),
            format!("{:?}", ro_legacy.coupling_diagnostics),
            "cluster-time expansion must not leak into the read-DAG: coupling \
             diagnostics must be byte-identical to the legacy seed's"
        );
    }

    /// (cold/warm parity) The cold entry point [`resolve_order`] and the warm
    /// entry point [`resolve_order_ordering_and_clusters`] must agree EXACTLY on
    /// the δ-formed cluster set, and the warm variant must still return no
    /// coupling diagnostics.
    ///
    /// This is the unit-level guard against re-opening the cold/warm cluster-set
    /// divergence task #5118 closed (esc-5014-10): warm computes its cluster set
    /// through its OWN entry point, so wiring only the cold call site would
    /// silently give the two paths different merged-solve behaviour. It is the
    /// primary coverage for the warm wiring (step-10).
    ///
    /// Fixture is BT-8b's elaborated `minimize cost(self.descendants)` shape, so
    /// the parity assertion is non-vacuous: a real cluster must exist on both
    /// sides.
    #[test]
    fn cold_and_warm_entry_points_agree_on_delta_cluster_set() {
        let parent = TopologyTemplateBuilder::new("Parent")
            .sub_component("childinst", "Child", vec![])
            .objective(cost_self_descendants_objective())
            .build();

        let child = TopologyTemplateBuilder::new("Child")
            .trait_bound("Costed")
            .auto_param_free("Child", "quantity_produced", Type::dimensionless_scalar())
            .let_binding(
                "Child",
                "line_cost",
                money_ty(),
                binop(
                    BinOp::Mul,
                    value_ref_typed("Child", "unit_cost", money_ty()),
                    value_ref("Child", "quantity_produced"),
                ),
            )
            .build();

        let templates = vec![parent, child];

        let values = ValueMap::default();
        let no_fns: [CompiledFunction; 0] = [];
        let registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let ctx = ClusterFormationCtx {
            values: &values,
            functions: &no_fns,
            trait_registry: &registry,
            max_unfold_depth: 64,
            max_unfold_nodes: 10_000,
        };

        let cold = resolve_order(&templates, Some(&ctx));
        let warm = resolve_order_ordering_and_clusters(&templates, Some(&ctx));

        assert!(
            !cold.clusters.is_empty(),
            "fixture must actually form a cluster, else the parity assertion below \
             is vacuous; got: {:?}",
            cold.clusters
        );
        assert_eq!(
            cold.clusters, warm.clusters,
            "cold and warm entry points must form the SAME δ cluster set (task #5118 \
             cold/warm co-solve parity); cold: {:?}, warm: {:?}",
            cold.clusters, warm.clusters
        );
        assert_eq!(
            cold.order, warm.order,
            "cold and warm entry points must also agree on `order`; got: {:?} vs {:?}",
            cold.order, warm.order
        );
        assert!(
            warm.coupling_diagnostics.is_empty(),
            "the warm entry point always clears coupling diagnostics — `eval_cached` \
             must emit neither W_SCOPE_COUPLING nor W_COUPLING_APPROXIMATED; got: {:?}",
            warm.coupling_diagnostics
        );
    }

    // -------------------------------------------------------------------------
    // step-11: δ cluster formation over the COMPILER's real cell-id shape
    // -------------------------------------------------------------------------

    /// The `Bolt` / `Rig` source fixture used by the compiler-real δ test.
    ///
    /// `Costed` refines `Buy`, so an implementing structure must also declare
    /// `supplier` / `part_number` / `unit_cost` / `lead_time` (the `CapScrew`
    /// def in `tests/cost_subtree_aggregate_eval.rs` is the working template
    /// this is modelled on).
    ///
    /// TRAP — the aggregate MUST stay inlined in the `minimize`. Writing it as
    /// `let subtree_cost : Money = cost(self.descendants)` + `minimize
    /// subtree_cost` forms NO cluster even with a correct normaliser, because
    /// C1 ([`expanded_objective_reads`]) expands objective TERMS only while
    /// [`build_non_auto_cell_map`] stores each cell's RAW unexpanded
    /// `default_expr` — `extract_value_deps` on an unexpanded
    /// `cost(self.descendants)` surfaces no `line_cost` read at all. That
    /// boundary is pinned separately by
    /// `objective_must_inline_the_aggregate_to_couple`.
    const COMPILER_REAL_DELTA_SOURCE: &str = r#"
structure def Bolt : Costed {
    param supplier          : String = "Acme"
    param part_number       : String = "B-1"
    param unit_cost         : Money  = 0.50USD
    param lead_time         : Time   = 24h
    param quantity_produced : Real   = auto
    constraint quantity_produced >= 1.0
    constraint quantity_produced <= 100.0
}
structure Rig {
    sub bolts = Bolt()
    minimize cost(self.descendants)
}
"#;

    /// Index of the template named `name` in `templates`.
    #[track_caller]
    fn template_index(templates: &[reify_compiler::TopologyTemplate], name: &str) -> usize {
        templates
            .iter()
            .position(|t| t.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "template {:?} must be present; got: {:?}",
                    name,
                    templates.iter().map(|t| &t.name).collect::<Vec<_>>()
                )
            })
    }

    /// δ cluster formation must fire on a module built by the REAL COMPILER, not
    /// only on hand-built `TopologyTemplateBuilder` fixtures (task #5334,
    /// review round 2, blocking issue 1).
    ///
    /// Unlike every other fixture in this module, the templates here come from
    /// real `.ri` source via `reify_test_support::parse_and_compile_with_stdlib`
    /// (a dev-dependency, hence visible to this in-crate `#[cfg(test)]` module),
    /// so the cell ids carry the compiler's ACTUAL entity scoping. That is the
    /// whole point: it is simultaneously compiler-real AND has direct visibility
    /// into the `pub(crate)` `ro.clusters`, which the integration-test
    /// diagnostic proxy cannot offer.
    ///
    /// RED before step-12, and the failure is a two-sided scoping mismatch:
    /// C1 expands `minimize cost(self.descendants)` to
    /// `[ValueRef(Rig.bolts.line_cost)].sum` — an INSTANCE-PATH id, composed by
    /// `enumerate_descendants` (structural_query.rs:187/:213) and minted by
    /// `apply_cost_aggregation` (structural_query.rs:686) — whereas
    /// `build_non_auto_cell_map` and `auto_owner` are both keyed by STRUCTURE
    /// NAME (`Bolt.line_cost` / `Bolt.quantity_produced`, minted at
    /// reify-compiler/src/entity.rs:6066/:6197). The seed therefore misses both
    /// maps on hop 1, the frontier empties, and the walk unions nothing.
    #[test]
    fn delta_cluster_forms_on_compiler_emitted_cell_ids() {
        let compiled =
            reify_test_support::parse_and_compile_with_stdlib(COMPILER_REAL_DELTA_SOURCE);
        let templates = &compiled.templates;

        let rig = template_index(templates, "Rig");
        let bolt = template_index(templates, "Bolt");
        let mut expected_scopes = vec![rig, bolt];
        expected_scopes.sort_unstable();

        // The trait registry is REQUIRED, not decorative: `apply_cost_aggregation`
        // drops every descendant whose structure fails
        // `satisfies_trait_bound(.., "Costed", registry)`, so an under-populated
        // registry silently yields an empty sum and a vacuous pass. Built the same
        // way `eval()` builds `sq_trait_registry` — prelude traits first so module
        // traits shadow them.
        let prelude = reify_compiler::stdlib_loader::load_stdlib();
        let registry = crate::structural_query::build_trait_registry(
            prelude
                .iter()
                .flat_map(|m| m.trait_defs.iter())
                .chain(compiled.trait_defs.iter()),
        );

        // `bolts` is a plain (non-collection) sub, so its `count_cell` is `None`
        // and `enumerate_descendants` needs no populated counts.
        let values = ValueMap::default();
        let ctx = ClusterFormationCtx {
            values: &values,
            functions: &compiled.functions,
            trait_registry: &registry,
            max_unfold_depth: 64,
            max_unfold_nodes: 10_000,
        };
        let ro = resolve_order(templates, Some(&ctx));

        assert_eq!(
            ro.clusters.len(),
            1,
            "a compiler-emitted module whose objective couples to a child's derived \
             `line_cost` must form exactly ONE δ cluster. Zero clusters means the \
             instance-path seed (`Rig.bolts.line_cost`) missed the structure-name-keyed \
             `auto_owner` / cell map (`Bolt.line_cost`) — the walk unions nothing and \
             the whole feature is a no-op on real `.ri` designs; got: {:?}",
            ro.clusters
        );
        let cluster = &ro.clusters[0];
        assert_eq!(
            cluster.scopes, expected_scopes,
            "the cluster must span BOTH the Rig (idx {}) and Bolt (idx {}) scopes; got: {:?}",
            rig, bolt, cluster.scopes
        );
        assert_eq!(
            cluster.dim, 1,
            "dim = 1 (Bolt's single auto `quantity_produced`); got: {}",
            cluster.dim
        );
        assert_eq!(
            cluster.disposition,
            ClusterDisposition::MergedSolve,
            "dim 1 is within cap ⇒ MergedSolve; got: {:?}",
            cluster.disposition
        );
    }

    /// The same fixture as [`COMPILER_REAL_DELTA_SOURCE`], but instantiating the
    /// Costed child through a COLLECTION sub — the way real `.ri` modules put
    /// many identical Costed children under one parent.
    ///
    /// This is the shape that exercises `enumerate_descendants`' `[idx]` arm
    /// (structural_query.rs:187), so the ids reaching the walk are
    /// `Rig.bolts[0].line_cost` … `Rig.bolts[2].line_cost`.
    const COMPILER_REAL_DELTA_COLLECTION_SOURCE: &str = r#"
structure def Bolt : Costed {
    param supplier          : String = "Acme"
    param part_number       : String = "B-1"
    param unit_cost         : Money  = 0.50USD
    param lead_time         : Time   = 24h
    param quantity_produced : Real   = auto
    constraint quantity_produced >= 1.0
    constraint quantity_produced <= 100.0
}
structure Rig {
    sub bolts : List<Bolt>
    constraint bolts.count == 3
    minimize cost(self.descendants)
}
"#;

    /// δ cluster formation must fire for a COLLECTION sub too (task #5334,
    /// review round 3).
    ///
    /// [`strip_collection_indices`] exists solely so an indexed instance path
    /// resolves, but every other cluster-formation fixture — the sibling
    /// compiler-real test, the integration test, and all the builder ones — uses
    /// a plain sub, leaving the indexed end-to-end chain
    /// (`apply_cost_aggregation` minting `Rig.bolts[0].line_cost` →
    /// `strip_collection_indices` → [`normalize_cell_id`] → union) pinned only
    /// by `instance_path_map_matches_enumerate_descendants_paths`, which checks
    /// the MAP in isolation and never runs the walk. A regression in that chain
    /// would leave the feature a no-op for collection-based designs with a fully
    /// green suite.
    #[test]
    fn delta_cluster_forms_over_a_collection_sub_instance_path() {
        let compiled = reify_test_support::parse_and_compile_with_stdlib(
            COMPILER_REAL_DELTA_COLLECTION_SOURCE,
        );
        let templates = &compiled.templates;

        let rig = template_index(templates, "Rig");
        let bolt = template_index(templates, "Bolt");
        let mut expected_scopes = vec![rig, bolt];
        expected_scopes.sort_unstable();

        // Required for the same reason as in the sibling test: an under-populated
        // registry makes `apply_cost_aggregation` drop every descendant.
        let prelude = reify_compiler::stdlib_loader::load_stdlib();
        let registry = crate::structural_query::build_trait_registry(
            prelude
                .iter()
                .flat_map(|m| m.trait_defs.iter())
                .chain(compiled.trait_defs.iter()),
        );

        // Unlike the plain-sub fixture, the count MUST be populated: the
        // collection arm reads the sub's synthetic `__count_bolts` cell out of
        // the ValueMap and folds a missing/undef value to 0 (structural_query.rs
        // :171-177), which would enumerate nothing and pass vacuously with zero
        // clusters.
        let count_cell = templates[rig]
            .sub_components
            .iter()
            .find(|s| s.name == "bolts")
            .and_then(|s| s.count_cell.clone())
            .expect("collection sub `bolts` must carry a synthetic count cell");
        let mut values = ValueMap::default();
        values.insert(count_cell, Value::Int(3));

        let ctx = ClusterFormationCtx {
            values: &values,
            functions: &compiled.functions,
            trait_registry: &registry,
            max_unfold_depth: 64,
            max_unfold_nodes: 10_000,
        };

        // Non-vacuity: the seeds really are INDEXED instance paths, so the union
        // below can only succeed by going through `strip_collection_indices`.
        let reads = super::expanded_objective_reads(templates, &ctx);
        assert!(
            reads[rig].iter().any(|r| r.entity.contains('[')),
            "the expanded objective must carry `[idx]` instance-path reads \
             (`Rig.bolts[0].line_cost`), else this test does not exercise \
             `strip_collection_indices` at all; got: {:?}",
            reads[rig]
        );

        let ro = resolve_order(templates, Some(&ctx));

        assert_eq!(
            ro.clusters.len(),
            1,
            "a collection-instantiated Costed child must couple exactly as a plain \
             sub does. Zero clusters means the indexed seed (`Rig.bolts[0].line_cost`) \
             failed to normalise to `Bolt.line_cost` — the feature is a no-op for \
             collection-based designs; got: {:?}",
            ro.clusters
        );
        let cluster = &ro.clusters[0];
        assert_eq!(
            cluster.scopes, expected_scopes,
            "the cluster must span BOTH the Rig (idx {}) and Bolt (idx {}) scopes; got: {:?}",
            rig, bolt, cluster.scopes
        );
        assert_eq!(
            cluster.dim, 1,
            "dim is the STRUCTURAL per-template auto sum (Bolt's single \
             `quantity_produced`), not per collection instance; got: {}",
            cluster.dim
        );
        assert_eq!(
            cluster.disposition,
            ClusterDisposition::MergedSolve,
            "dim 1 is within cap ⇒ MergedSolve; got: {:?}",
            cluster.disposition
        );
    }

    // -------------------------------------------------------------------------
    // step-13: fences for the instance-path normalisation layer
    // -------------------------------------------------------------------------

    /// DRIFT GUARD — [`build_instance_path_structure_map`] deliberately
    /// re-implements `enumerate_descendants`' two `format!` shapes
    /// (structural_query.rs:213 `{prefix}.{sub}`, :187 `{prefix}.{sub}[{idx}]`)
    /// rather than calling it. If either side is edited independently, the δ
    /// walk silently reverts to unioning nothing — exactly the failure class
    /// review round 2 caught, where the whole suite was green over a no-op.
    ///
    /// Uses `enumerate_descendants` itself as the ORACLE: every path it emits
    /// must, after `[...]` index-stripping, appear in the map with the SAME
    /// structure name. The fixture carries BOTH a plain sub and a collection sub
    /// (with a populated `__count_*` cell, so the collection arm actually fires)
    /// and recurses one level deeper through each, so a drift in either arm or
    /// in the recursion prefix fails loudly here.
    #[test]
    fn instance_path_map_matches_enumerate_descendants_paths() {
        let count_cell = ValueCellId::new("Rig", "__count_bolts");
        let rig = TopologyTemplateBuilder::new("Rig")
            .sub_component("head", "Head", vec![])
            .collection_sub_component("bolts", "Bolt", count_cell.clone())
            .build();
        // Both children carry their own sub, so the map's recursion (which
        // re-derives the prefix on each hop) is exercised through the plain arm
        // AND through the collection arm's `[idx]`-suffixed prefix.
        let head = TopologyTemplateBuilder::new("Head")
            .sub_component("washer", "Washer", vec![])
            .build();
        let bolt = TopologyTemplateBuilder::new("Bolt")
            .sub_component("washer", "Washer", vec![])
            .build();
        let washer = TopologyTemplateBuilder::new("Washer").build();
        let templates = vec![rig, head, bolt, washer];

        let mut values = ValueMap::default();
        values.insert(count_cell, Value::Int(2));

        let mut node_budget = 10_000usize;
        let mut diags: Vec<Diagnostic> = Vec::new();
        let emitted = crate::structural_query::enumerate_descendants(
            &templates[0],
            &templates,
            &values,
            "Rig",
            0,
            64,
            &mut node_budget,
            &mut diags,
        );
        assert!(
            diags.is_empty(),
            "the oracle must enumerate cleanly (no budget/depth truncation), else this \
             guard is comparing against a partial path set; got: {:?}",
            diags
        );

        // SAME budgets as the oracle above — the two sides are only comparable
        // when both run unbudgeted-in-practice (asserted for the oracle by
        // `diags.is_empty()`); under truncation the map is a strict subset by
        // design.
        let map = super::build_instance_path_structure_map(&templates, 64, 10_000);
        assert!(
            !map.is_empty(),
            "the instance-path map must be non-empty, else this guard passes vacuously"
        );

        let mut saw_collection_path = false;
        let mut checked = 0usize;
        for elem in &emitted {
            let (path, structure_name) = match (&elem.kind, &elem.result_type) {
                (reify_ir::CompiledExprKind::Literal(Value::String(p)), Type::StructureRef(tn)) => {
                    (p, tn)
                }
                other => panic!(
                    "enumerate_descendants must emit Literal(String) : StructureRef; got: {:?}",
                    other
                ),
            };
            if path.contains('[') {
                saw_collection_path = true;
            }
            let stripped = super::strip_collection_indices(path);
            assert_eq!(
                map.get(stripped.as_ref()),
                Some(structure_name),
                "enumerate_descendants emitted path {:?} (structure {:?}), whose \
                 index-stripped form {:?} is absent from / disagrees with \
                 `build_instance_path_structure_map`. The two path formats have DRIFTED \
                 — δ cluster formation is now a silent no-op for this shape. Map: {:?}",
                path,
                structure_name,
                stripped,
                map
            );
            checked += 1;
        }
        assert!(
            saw_collection_path,
            "the collection arm must have contributed at least one `[idx]` path, else \
             this guard never exercises structural_query.rs:187; emitted: {:?}",
            emitted
                .iter()
                .map(|e| format!("{:?}", e.kind))
                .collect::<Vec<_>>()
        );
        assert!(
            checked >= 5,
            "expected ≥5 enumerated descendants (Rig.head, Rig.head.washer, \
             Rig.bolts[0..2] + their washers); got {}",
            checked
        );
    }

    /// DRIFT GUARD, RECURSIVE-CONTAINMENT ARM (task #5334, review round 3).
    ///
    /// The sibling guard's fixture (Rig → Head/bolts → Washer) is acyclic, so it
    /// cannot see the one place the two walks used to diverge: the map pruned on
    /// a structure-name `on_path` set (stopping at the FIRST repeat of a
    /// structure name along the path) while `enumerate_descendants` prunes on
    /// `max_depth` (descending THROUGH the repeat until the depth budget runs
    /// out). For `A` containing `B` containing `A`, the oracle therefore emitted
    /// paths past the first cycle turn that the map did not contain —
    /// `normalize_cell_id` missed, and δ formation silently under-clustered for
    /// exactly the deeply-nested designs the feature targets, with the acyclic
    /// guard fully green.
    ///
    /// Both sides now prune on depth alone, so this pins that agreement
    /// executably: same `max_depth` in, same path set out, including the
    /// past-the-cycle-turn paths — and nothing DEEPER than the shared bound.
    #[test]
    fn instance_path_map_matches_enumerate_descendants_under_recursive_containment() {
        // Deliberately small, so the shared depth bound is reached inside a
        // fixture whose full path set can be written out by hand below.
        const MAX_DEPTH: usize = 4;

        // A contains B contains A — recursive containment.
        let a = TopologyTemplateBuilder::new("A")
            .sub_component("b", "B", vec![])
            .build();
        let b = TopologyTemplateBuilder::new("B")
            .sub_component("a", "A", vec![])
            .build();
        let templates = vec![a, b];
        let values = ValueMap::default();

        let mut node_budget = 10_000usize;
        let mut diags: Vec<Diagnostic> = Vec::new();
        let emitted = crate::structural_query::enumerate_descendants(
            &templates[0],
            &templates,
            &values,
            "A",
            0,
            MAX_DEPTH,
            &mut node_budget,
            &mut diags,
        );
        let oracle_paths: Vec<(String, String)> = emitted
            .iter()
            .map(|elem| match (&elem.kind, &elem.result_type) {
                (reify_ir::CompiledExprKind::Literal(Value::String(p)), Type::StructureRef(tn)) => {
                    (p.clone(), tn.clone())
                }
                other => panic!(
                    "enumerate_descendants must emit Literal(String) : StructureRef; got: {:?}",
                    other
                ),
            })
            .collect();

        // Non-vacuity: the oracle must actually descend PAST the first repeat of
        // structure `A`, else this fixture exercises nothing the acyclic guard
        // does not already cover.
        assert!(
            oracle_paths.iter().any(|(p, _)| p == "A.b.a.b"),
            "the oracle must descend past the first cycle turn (expected `A.b.a.b`); got: {:?}",
            oracle_paths
        );

        let map = super::build_instance_path_structure_map(&templates, MAX_DEPTH, 10_000);
        for (path, structure_name) in &oracle_paths {
            assert_eq!(
                map.get(path.as_str()),
                Some(structure_name),
                "enumerate_descendants emitted {:?} (structure {:?}) under recursive \
                 containment, but the map lacks it / disagrees. The two guards have \
                 DRIFTED — the map is pruning earlier than the oracle, so \
                 `normalize_cell_id` misses and δ formation silently under-clusters \
                 for deeply-nested designs. Map: {:?}",
                path,
                structure_name,
                map
            );
        }

        // The shared depth bound is real in BOTH directions: `A.b.a.b.a` is the
        // last path within `MAX_DEPTH` (emitted at depth 3), and the map must
        // stop there rather than running away on the cycle.
        assert_eq!(
            map.get("A.b.a.b.a"),
            Some(&"A".to_string()),
            "the map must reach the deepest in-bounds path `A.b.a.b.a`; got: {:?}",
            map
        );
        assert_eq!(
            map.get("A.b.a.b.a.b"),
            None,
            "the map must not exceed `max_depth` = {}; `A.b.a.b.a.b` sits one hop \
             past the bound the oracle also stops at. Map: {:?}",
            MAX_DEPTH,
            map
        );
    }

    /// MID-WALK normalisation — a seed-only fix would fail this.
    ///
    /// The Parent's objective reads its OWN structure-scoped let cell
    /// (`Parent.total`), whose `default_expr` reads
    /// `ValueCellId::new("Parent.childinst", "line_cost")` — the exact shape
    /// reify-compiler/src/expr.rs:843 emits for a sub-member access
    /// (`format!("{}.{}", scope.entity_name, sub_name)`) — while the Child
    /// declares its own cells structure-scoped as `Child.line_cost` /
    /// `Child.quantity_produced`.
    ///
    /// The instance-path id therefore appears MID-WALK, downstream of the seed
    /// vector, which is why [`normalize_cell_id`] is applied at every hop rather
    /// than only to the seeds. This is a DISTINCT code path from step-11's
    /// seed-level normalisation of `apply_cost_aggregation`'s output; verified
    /// non-vacuous by temporarily restricting normalisation to the seed set,
    /// under which THIS test is the only one in the module that goes red.
    ///
    /// The instance-path spelling here is the thing under test — do NOT "clean
    /// it up" to a structure-name entity (unlike the sibling fixtures, which
    /// step-15 realigned onto the compiler's real structure-name scoping).
    #[test]
    fn derived_cell_reading_sub_member_path_reaches_child_auto() {
        let parent = TopologyTemplateBuilder::new("Parent")
            .sub_component("childinst", "Child", vec![])
            .let_binding(
                "Parent",
                "total",
                money_ty(),
                // INTENTIONAL instance-path read (reify-compiler/src/expr.rs:843).
                value_ref_typed("Parent.childinst", "line_cost", money_ty()),
            )
            .objective(ObjectiveSet::single(
                ObjectiveSense::Minimize,
                value_ref_typed("Parent", "total", money_ty()),
            ))
            .build();

        // Child cells are STRUCTURE-scoped, as reify-compiler/src/entity.rs
        // :6066/:6197 mint them.
        let child = TopologyTemplateBuilder::new("Child")
            .trait_bound("Costed")
            .auto_param_free("Child", "quantity_produced", Type::dimensionless_scalar())
            .let_binding(
                "Child",
                "line_cost",
                money_ty(),
                binop(
                    BinOp::Mul,
                    value_ref_typed("Child", "unit_cost", money_ty()),
                    value_ref("Child", "quantity_produced"),
                ),
            )
            .build();

        let templates = vec![parent, child];

        let values = ValueMap::default();
        let no_fns: [CompiledFunction; 0] = [];
        let registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let ctx = ClusterFormationCtx {
            values: &values,
            functions: &no_fns,
            trait_registry: &registry,
            max_unfold_depth: 64,
            max_unfold_nodes: 10_000,
        };
        let ro = resolve_order(&templates, Some(&ctx));

        assert_eq!(
            ro.clusters.len(),
            1,
            "a derived Parent cell reading `Parent.childinst.line_cost` must reach the \
             Child's auto through MID-WALK normalisation; zero clusters means \
             normalisation is seed-only; got: {:?}",
            ro.clusters
        );
        assert_eq!(
            ro.clusters[0].scopes,
            vec![0, 1],
            "the cluster must span Parent + Child; got: {:?}",
            ro.clusters[0].scopes
        );
    }

    /// SHADOWED SPELLING — a hit on the AS-IS id must not retire the normalised
    /// id, which may name a DIFFERENT, real cell (task #5334, review round 3).
    ///
    /// The walk resolves each popped id AS-IS first and falls back to its
    /// normalised spelling only on a miss. When the as-is lookup hits, the
    /// normalised spelling was NOT the one expanded, so retiring it into
    /// `visited` would permanently drop whatever chain it carries.
    ///
    /// The fixture makes that concrete: `Parent` declares its own cell with the
    /// instance-path entity `Parent.childinst` (the reify-compiler/src/expr.rs:843
    /// shape) whose `default_expr` is a constant, SHADOWING the normalised
    /// `Child.line_cost` — which is the only route to the child's auto. The
    /// shadow is popped BEFORE `Child.line_cost` is discovered (the frontier is
    /// LIFO and `Parent.total` lists `other` first), so an unconditional
    /// `visited.insert(normalized)` retires `Child.line_cost` unexpanded and the
    /// module forms ZERO clusters — the pre-fix behaviour, silently
    /// under-clustering.
    ///
    /// The ordering above is deterministic, not incidental; should the frontier
    /// discipline ever change, this test still asserts the correct OUTCOME (it
    /// would simply stop being the tightest witness of the shadowing bug).
    #[test]
    fn as_is_cell_hit_must_not_retire_a_distinct_normalised_cell() {
        /// A MONEY-dimensioned constant for the shadow cell. Its VALUE is
        /// irrelevant — what matters is that the expr carries no reads, so the
        /// shadow contributes nothing to the frontier.
        fn money(v: f64) -> Value {
            Value::Scalar {
                si_value: v,
                dimension: DimensionVector::MONEY,
            }
        }

        let parent = TopologyTemplateBuilder::new("Parent")
            .sub_component("childinst", "Child", vec![])
            // `other` is the LEFT operand so the shadow id (the right operand)
            // is pushed last and therefore popped FIRST — see the doc comment.
            .let_binding(
                "Parent",
                "total",
                money_ty(),
                binop(
                    BinOp::Add,
                    value_ref_typed("Parent", "other", money_ty()),
                    value_ref_typed("Parent.childinst", "line_cost", money_ty()),
                ),
            )
            // THE SHADOW: same id as the normalisation target of
            // `Parent.childinst.line_cost`, but a constant — no reads to follow.
            .let_binding(
                "Parent.childinst",
                "line_cost",
                money_ty(),
                literal(money(1.0)),
            )
            // The ONLY route to the child's auto, and it is discovered strictly
            // AFTER the shadow has been popped.
            .let_binding(
                "Parent",
                "other",
                money_ty(),
                value_ref_typed("Child", "line_cost", money_ty()),
            )
            .objective(ObjectiveSet::single(
                ObjectiveSense::Minimize,
                value_ref_typed("Parent", "total", money_ty()),
            ))
            .build();

        let child = TopologyTemplateBuilder::new("Child")
            .trait_bound("Costed")
            .auto_param_free("Child", "quantity_produced", Type::dimensionless_scalar())
            .let_binding(
                "Child",
                "line_cost",
                money_ty(),
                binop(
                    BinOp::Mul,
                    value_ref_typed("Child", "unit_cost", money_ty()),
                    value_ref("Child", "quantity_produced"),
                ),
            )
            .build();

        let templates = vec![parent, child];

        let values = ValueMap::default();
        let no_fns: [CompiledFunction; 0] = [];
        let registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let ctx = ClusterFormationCtx {
            values: &values,
            functions: &no_fns,
            trait_registry: &registry,
            max_unfold_depth: 64,
            max_unfold_nodes: 10_000,
        };
        let ro = resolve_order(&templates, Some(&ctx));

        assert_eq!(
            ro.clusters.len(),
            1,
            "hitting the parent's shadow cell `Parent.childinst.line_cost` must NOT \
             retire the distinct, real `Child.line_cost`; zero clusters means the \
             child's chain to `quantity_produced` was dropped unexpanded; got: {:?}",
            ro.clusters
        );
        assert_eq!(
            ro.clusters[0].scopes,
            vec![0, 1],
            "the cluster must span Parent + Child; got: {:?}",
            ro.clusters[0].scopes
        );
    }

    /// C1 BOUNDARY FENCE — a documented limitation, deliberately recorded as an
    /// executable expectation rather than left latent.
    ///
    /// [`expanded_objective_reads`] (C1) expands objective TERMS only, while
    /// [`build_non_auto_cell_map`] stores each cell's RAW, UNEXPANDED
    /// `default_expr`. So an objective that reads a let cell whose own expr is
    /// `cost(self.descendants)` surfaces no `line_cost` read at all: the walk
    /// sees only the unexpanded `Parent.__self` placeholder and forms ZERO
    /// clusters.
    ///
    /// This is EXPECTED AND KNOWN, not a regression. The working form is to keep
    /// the aggregate inlined in the `minimize` (`minimize cost(self.descendants)`
    /// — see `delta_cluster_forms_on_compiler_emitted_cell_ids` and
    /// `bt8_cost_self_descendants_forms_single_merged_cluster`, both of which do
    /// form a cluster over the identical child shape). Lifting this boundary
    /// would mean expanding derived cells at cluster time too, which is a design
    /// change requiring review — a follow-up task, not a silent local fix.
    #[test]
    fn objective_must_inline_the_aggregate_to_couple() {
        let aggregate_term = cost_self_descendants_objective().terms[0].expr.clone();
        let parent = TopologyTemplateBuilder::new("Parent")
            .sub_component("childinst", "Child", vec![])
            // The aggregate lives in a derived cell, NOT in the objective term.
            .let_binding("Parent", "subtree_cost", money_ty(), aggregate_term)
            .objective(ObjectiveSet::single(
                ObjectiveSense::Minimize,
                value_ref_typed("Parent", "subtree_cost", money_ty()),
            ))
            .build();

        let child = TopologyTemplateBuilder::new("Child")
            .trait_bound("Costed")
            .auto_param_free("Child", "quantity_produced", Type::dimensionless_scalar())
            .let_binding(
                "Child",
                "line_cost",
                money_ty(),
                binop(
                    BinOp::Mul,
                    value_ref_typed("Child", "unit_cost", money_ty()),
                    value_ref("Child", "quantity_produced"),
                ),
            )
            .build();

        let templates = vec![parent, child];

        let values = ValueMap::default();
        let no_fns: [CompiledFunction; 0] = [];
        let registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let ctx = ClusterFormationCtx {
            values: &values,
            functions: &no_fns,
            trait_registry: &registry,
            max_unfold_depth: 64,
            max_unfold_nodes: 10_000,
        };
        let ro = resolve_order(&templates, Some(&ctx));

        assert!(
            ro.clusters.is_empty(),
            "EXPECTED AND KNOWN (C1 boundary): an aggregate hidden behind a derived cell \
             forms no cluster, because cluster-time expansion runs over objective TERMS \
             only while the cell map stores raw `default_expr`s. If this now forms a \
             cluster the boundary moved — that is a design change needing review, not a \
             test to update. Working form: keep the aggregate inlined in the `minimize`; \
             got: {:?}",
            ro.clusters
        );
    }
}
