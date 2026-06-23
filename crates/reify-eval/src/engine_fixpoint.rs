//! Unified build-DAG fixpoint driver (task 4357 δ).
//!
//! This module holds `run_unified_pass` — an online Kahn topological worklist
//! over α's existing forward dependency-trace graph (O(V+E)) — plus the cycle
//! contract (Stage A hang-proof Kahn residue + Stage B Tarjan-SCC discriminator
//! → `E_EVAL_CYCLE`) and the geometry-backed-constraint-on-auto guard
//! (→ `E_EVAL_UNRESOLVED`).
//!
//! The driver is a PURE STRUCTURAL PLANNER: it returns a `(schedule, residue,
//! diagnostics)` triple and does NOT execute nodes (no kernel calls, no handle
//! inserts, no value writes). Node execution and the runtime `Determined`
//! readiness gate are layered on by the ε executors that consume the schedule.
//!
//! See `docs/prds/v0_6/engine-unified-build-dag.md` for the full design.
//!
//! The module and `run_unified_pass` compile unconditionally so the cycle
//! contract is always unit-testable; the `unified-dag` Cargo feature +
//! `REIFY_BUILD_SCHEDULER` env var gate ONLY the production activation of the
//! driver inside `Engine::build()`.

/// Build-time scheduler selection (task 4357 δ).
///
/// Selects between the legacy multi-pass build loop and the unified build-DAG
/// Kahn worklist driver. Defaults to [`BuildScheduler::LegacyMultiPass`] so an
/// un-configured engine keeps byte-identical legacy behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildScheduler {
    /// Legacy multi-pass build loop (default; byte-preserving).
    #[default]
    LegacyMultiPass,
    /// Unified build-DAG: `run_unified_pass` Kahn worklist + cycle contract.
    UnifiedDag,
}

impl BuildScheduler {
    /// Environment variable consulted by [`BuildScheduler::from_env`].
    pub const ENV_VAR: &'static str = "REIFY_BUILD_SCHEDULER";

    /// Pure parser: map an optional configuration string to a scheduler.
    ///
    /// Feature-INDEPENDENT — `Some("unified")` always parses to `UnifiedDag` so
    /// the parser stays unit-testable without the `unified-dag` Cargo feature.
    /// Matching is case-insensitive and tolerates surrounding whitespace. Any
    /// unrecognized value — including `None`, empty, or garbage — defaults to
    /// `LegacyMultiPass`.
    ///
    /// The production [`BuildScheduler::from_env`] layers the `unified-dag`
    /// feature gate on top of this parser.
    pub fn from_env_value(value: Option<&str>) -> Self {
        let normalized = value.map(|v| v.trim().to_ascii_lowercase());
        match normalized.as_deref() {
            Some("unified") => BuildScheduler::UnifiedDag,
            _ => BuildScheduler::LegacyMultiPass,
        }
    }

    /// Production selection: read `REIFY_BUILD_SCHEDULER` and apply the
    /// `unified-dag` feature gate.
    ///
    /// `UnifiedDag` is selectable ONLY when the `unified-dag` Cargo feature is
    /// enabled. When the feature is disabled (the default), this always returns
    /// `LegacyMultiPass` regardless of the env value — the env gate is inert
    /// without the feature, so production builds opt in deliberately.
    ///
    /// # Unit-test coverage
    ///
    /// This thin env-reading wrapper delegates to the pure
    /// [`BuildScheduler::from_env_value`] parser, which carries the exhaustive
    /// string→scheduler cases (`build_scheduler_from_env_value_parsing`). Mirroring
    /// the codebase convention (`warm_pool::WarmStatePool::from_env_or_default`,
    /// `dispatcher` long-chain threshold), the wrapper is intentionally NOT
    /// unit-tested with `std::env::set_var`/`remove_var` — both are `unsafe` in
    /// Rust 2024 and race-prone across parallel tests. BOTH configurations are
    /// still pinned without env mutation: the feature-off inert path
    /// (`from_env_is_inert_legacy_without_feature`) and the feature-on delegation
    /// (`from_env_feature_on_delegates_to_parser_over_real_env`).
    pub fn from_env() -> Self {
        #[cfg(feature = "unified-dag")]
        {
            Self::from_env_value(std::env::var(Self::ENV_VAR).ok().as_deref())
        }
        #[cfg(not(feature = "unified-dag"))]
        {
            BuildScheduler::LegacyMultiPass
        }
    }
}

use std::collections::{BTreeSet, HashMap, HashSet};

use reify_core::{ConstraintNodeId, Diagnostic, DiagnosticCode, RealizationNodeId};

use crate::cache::NodeId;
use crate::deps::DependencyTrace;
use crate::dirty::DebugOrd;
use crate::graph::EvaluationGraph;

/// Output of [`run_unified_pass`] — a pure structural plan (no node execution).
///
/// - `schedule`: the topological evaluation order of the in-set, in-degree-0
///   nodes reached by the Kahn worklist (ε's executors consume this).
/// - `residue`: node-set members never popped — cyclic nodes and any node
///   stranded downstream of a cycle (Stage A hang-proof output).
/// - `diagnostics`: `E_EVAL_CYCLE` (one per cyclic SCC, step-10/12) and
///   `E_EVAL_UNRESOLVED` (geometry-backed-constraint-on-auto, step-14).
#[derive(Debug, Default)]
pub struct UnifiedPassResult {
    pub schedule: Vec<NodeId>,
    pub residue: HashSet<NodeId>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Run a single unified build-DAG pass over α's forward dependency-trace graph.
///
/// A PURE STRUCTURAL PLANNER: it returns a `(schedule, residue, diagnostics)`
/// triple and does NOT execute nodes. The node set is the trace map's KEYS
/// (Value/Constraint/Realization/Resolution + composed-field Values) — α's
/// complete forward trace, eager-over-reachable. Compute nodes carry no forward
/// trace (reverse-index-only sinks) and stay outside this worklist; ε extends
/// the set if it needs to schedule them.
///
/// In-degree and forward adjacency are built by INVERTING the trace map (single
/// source ⇒ guaranteed consistent, no drift vs. the reverse index): for each
/// node `N`, predecessors = `{Value(r) | r ∈ reads} ∪ {Realization(rr) | rr ∈
/// realization_reads}`, counting/edging only those present in the node set.
/// A read naming an absent producer is not counted, so a missing-producer
/// consumer still reaches in-degree 0 and is scheduled — never residue (design
/// decision #6). Repeated reads (e.g. `a * a`) are deduped per node.
///
/// The Kahn worklist uses a `BTreeSet<DebugOrd>` ready set (`pop_first`), giving
/// a deterministic schedule. Single pass, no fixpoint ⇒ cannot hang (Stage A).
/// Cyclic nodes never reach in-degree 0, so they are never popped/scheduled and
/// land in `residue`. O(V+E).
///
/// Stage B runs Tarjan SCC over the residue subgraph, emitting one
/// `E_EVAL_CYCLE` per genuine cycle — a multi-node SCC (`|SCC| > 1`) or a
/// singleton self-loop. A final independent classifier emits one
/// `E_EVAL_UNRESOLVED` per constraint whose transitive geometry-backed read
/// closure reaches an auto value cell.
///
/// # Determinism (the total order of the returned diagnostic vector)
///
/// Every order-sensitive step rides the shared `DebugOrd` total order, so no
/// `HashMap`/`HashSet` iteration order ever leaks into `schedule` or
/// `diagnostics`: the Kahn ready set is a `BTreeSet<DebugOrd>` (`pop_first`);
/// Tarjan's outer node iteration and successor enumeration are `DebugOrd`-sorted
/// (via `debug_ord_sorted`); SCCs are emitted ordered by each component's
/// `DebugOrd`-min member; and the per-SCC ordered path is a DFS from that
/// `DebugOrd`-min member following SCC-internal successors in `DebugOrd` order.
///
/// The `diagnostics` vector therefore has ONE documented total order: ALL
/// `E_EVAL_CYCLE` diagnostics first (one per cyclic SCC, in SCC-min order), then
/// ALL `E_EVAL_UNRESOLVED` diagnostics (one per offending constraint, in the
/// constraint's `DebugOrd` order). The vector is byte-identical across runs and
/// across trace-map insertion orders — pinned by the step-15 determinism test.
pub fn run_unified_pass(
    graph: &EvaluationGraph,
    traces: &HashMap<NodeId, DependencyTrace>,
) -> UnifiedPassResult {
    // Node set = the trace map's keys.
    let node_set: HashSet<NodeId> = traces.keys().cloned().collect();

    // In-degree + forward adjacency by inverting the trace map.
    let mut in_degree: HashMap<NodeId, usize> =
        node_set.iter().map(|n| (n.clone(), 0usize)).collect();
    let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

    for node in &node_set {
        if let Some(tr) = traces.get(node) {
            // Unique in-set predecessors (dedup repeated reads).
            let mut preds: HashSet<NodeId> = HashSet::new();
            for r in &tr.reads {
                let p = NodeId::Value(r.clone());
                if node_set.contains(&p) {
                    preds.insert(p);
                }
            }
            for rr in &tr.realization_reads {
                let p = NodeId::Realization(rr.clone());
                if node_set.contains(&p) {
                    preds.insert(p);
                }
            }
            for p in preds {
                adjacency.entry(p).or_default().push(node.clone());
                *in_degree.get_mut(node).expect("node present in in_degree") += 1;
            }
        }
    }

    // Kahn worklist — DebugOrd-ordered ready set for a deterministic schedule.
    //
    // Invariant (task #4668, same-structure sibling realizations): for a
    // same-structure sibling pair `b` → `f` (where `f = fillet(b, …)` and `b`
    // is referenced as GeomRef::Sub("b")), `deps.rs::resolve_sibling_ref` adds
    // an explicit realization→realization edge `f depends-on b`.  That edge gives
    // `f` in-degree ≥ 1 until `b` is popped and its dependents' in-degrees are
    // decremented.  Consequently the Kahn scheduler emits `b` strictly before `f`
    // in `schedule`, guaranteeing that `named_steps["b"]` is populated by `b`'s
    // executor before `f`'s executor runs its `GeomRef::Sub("b")` lookup.
    let mut ready: BTreeSet<DebugOrd> = in_degree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(n, _)| DebugOrd(n.clone()))
        .collect();
    let mut schedule: Vec<NodeId> = Vec::with_capacity(node_set.len());

    while let Some(DebugOrd(node)) = ready.pop_first() {
        if let Some(deps) = adjacency.get(&node) {
            for dep in deps {
                let d = in_degree
                    .get_mut(dep)
                    .expect("dependent present in in_degree");
                debug_assert!(*d > 0, "in-degree underflow at {dep:?}");
                *d -= 1;
                if *d == 0 {
                    ready.insert(DebugOrd(dep.clone()));
                }
            }
        }
        schedule.push(node);
    }

    // Residue = node-set members never popped (cyclic / stranded-downstream).
    let scheduled: HashSet<NodeId> = schedule.iter().cloned().collect();
    let residue: HashSet<NodeId> = node_set.difference(&scheduled).cloned().collect();

    // --- Stage B: Tarjan SCC over the residue subgraph → E_EVAL_CYCLE ---
    // Decompose the residue's induced subgraph into strongly-connected
    // components. A genuine cycle is either a multi-node SCC (`|SCC| > 1`) or a
    // singleton carrying a self-edge (`let x = x`); each earns exactly one
    // `E_EVAL_CYCLE`. A singleton WITHOUT a self-edge is stranded downstream of
    // another cycle — left in residue, NO diagnostic. SCC enumeration, per-SCC
    // member ordering, and the inter-SCC order (by DebugOrd-min member) all ride
    // `DebugOrd`, so the diagnostic vector is deterministic regardless of
    // HashMap iteration order.
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for scc in residue_sccs(&residue, &adjacency) {
        let is_cycle = scc.len() > 1 || (scc.len() == 1 && has_self_edge(&scc[0], &adjacency));
        if is_cycle {
            diagnostics.push(eval_cycle_diagnostic(&scc, &adjacency));
        }
    }

    // --- E_EVAL_UNRESOLVED: geometry-backed-constraint-on-auto guard ---
    // Independent classifier over the existing trace edges (does NOT touch the
    // cycle residue). Appended after all cycle diagnostics; constraints are
    // visited in DebugOrd order, so the appended sub-vector is deterministic.
    diagnostics.extend(unresolved_diagnostics(graph, traces));

    UnifiedPassResult {
        schedule,
        residue,
        diagnostics,
    }
}

/// Run a unified build-DAG pass SEEDED from a dirty∩demand node set (task 4531 θ2).
///
/// The edit path (`edit_param`/`edit_source`/`edit_check`) re-evaluates only the
/// `eval_set = dirty ∩ demand` cone, not the whole graph. This entry orders THAT
/// cone through the same Kahn worklist as [`run_unified_pass`], so the edit value
/// executor rides the identical ordering core as cold/build/concurrent — making
/// the "warm output == cold output" claim structural on the edit surface.
///
/// BOUNDED COST (P0 latency gate): the node set is restricted to `seed` up front,
/// so in-degree/adjacency construction and the worklist are O(|seed| + edges
/// within seed) — proportional to the affected cone, NOT O(graph). A read naming a
/// producer OUTSIDE the seed is simply not counted (the producer was already
/// evaluated in a prior pass), so a seed node with an external-only producer still
/// reaches in-degree 0 and is scheduled — never residue (mirrors `run_unified_pass`
/// design decision #6).
///
/// Returns ONLY the schedule (a `Vec<NodeId>`): cycle/unresolved diagnostics stay a
/// `check()`/`build()` concern exactly as legacy edit does (edit consumes ordering
/// only, surfacing no `E_EVAL_CYCLE`/`E_EVAL_UNRESOLVED`). The schedule is a valid
/// topological order of the seed's induced subgraph; any cyclic seed member (not
/// expected on the acyclic edit cone) is simply absent from the returned vector and
/// is handled by the executor's deterministic residue-append fallback.
///
/// Determinism: the `BTreeSet<DebugOrd>` ready set (`pop_first`) carries over from
/// [`run_unified_pass`], so the schedule is byte-identical across runs and trace-map
/// insertion orders.
pub fn run_unified_pass_seeded(
    traces: &HashMap<NodeId, DependencyTrace>,
    seed: &HashSet<NodeId>,
) -> Vec<NodeId> {
    // Node set = the seed itself (bounded cost: O(|seed| + edges-within-seed),
    // NOT O(graph)). In-degree + forward adjacency are built by inverting the
    // trace map exactly as `run_unified_pass`, but counting/edging ONLY
    // predecessors that are also IN the seed. A read naming an out-of-seed
    // producer is not counted, so a seed node whose producer was already
    // evaluated upstream still reaches in-degree 0 and is scheduled — never
    // residue (mirrors `run_unified_pass` design decision #6). A seed node with
    // no trace entry contributes no edges and stays in-degree 0.
    let mut in_degree: HashMap<NodeId, usize> = seed.iter().map(|n| (n.clone(), 0usize)).collect();
    let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

    for node in seed {
        if let Some(tr) = traces.get(node) {
            // Unique in-seed predecessors (dedup repeated reads).
            let mut preds: HashSet<NodeId> = HashSet::new();
            for r in &tr.reads {
                let p = NodeId::Value(r.clone());
                if seed.contains(&p) {
                    preds.insert(p);
                }
            }
            for rr in &tr.realization_reads {
                let p = NodeId::Realization(rr.clone());
                if seed.contains(&p) {
                    preds.insert(p);
                }
            }
            for p in preds {
                adjacency.entry(p).or_default().push(node.clone());
                *in_degree
                    .get_mut(node)
                    .expect("seed node present in in_degree") += 1;
            }
        }
    }

    // Kahn worklist — DebugOrd-ordered ready set for a deterministic schedule,
    // identical to `run_unified_pass` (the SAME ordering core).
    let mut ready: BTreeSet<DebugOrd> = in_degree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(n, _)| DebugOrd(n.clone()))
        .collect();
    let mut schedule: Vec<NodeId> = Vec::with_capacity(seed.len());

    while let Some(DebugOrd(node)) = ready.pop_first() {
        if let Some(deps) = adjacency.get(&node) {
            for dep in deps {
                let d = in_degree
                    .get_mut(dep)
                    .expect("dependent present in in_degree");
                debug_assert!(*d > 0, "in-degree underflow at {dep:?}");
                *d -= 1;
                if *d == 0 {
                    ready.insert(DebugOrd(dep.clone()));
                }
            }
        }
        schedule.push(node);
    }

    // Cyclic seed members (not expected on the acyclic edit cone) never reach
    // in-degree 0, so they are simply absent from `schedule`; the edit executor's
    // residue-append fallback (step-4) covers any such node so every demanded cell
    // still evaluates. Diagnostics stay a check()/build() concern (edit consumes
    // ordering only).
    schedule
}

/// Sort nodes by the shared [`DebugOrd`] total order (Debug-repr lexicographic).
///
/// The SINGLE source of determinism for every order-sensitive Stage B step —
/// SCC outer iteration, successor enumeration, and the per-SCC ordered path —
/// so no HashMap iteration order ever leaks into the diagnostic vector.
fn debug_ord_sorted(nodes: impl IntoIterator<Item = NodeId>) -> Vec<NodeId> {
    let mut ordered: Vec<DebugOrd> = nodes.into_iter().map(DebugOrd).collect();
    ordered.sort();
    ordered.into_iter().map(|DebugOrd(n)| n).collect()
}

/// Does `node` carry a self-edge — does its own forward adjacency list itself?
/// A singleton SCC with a self-edge is a `let x = x` self-loop cycle; without
/// one it is stranded downstream and earns no diagnostic.
fn has_self_edge(node: &NodeId, adjacency: &HashMap<NodeId, Vec<NodeId>>) -> bool {
    adjacency
        .get(node)
        .is_some_and(|succs| succs.contains(node))
}

/// DebugOrd-ordered forward successors of `node`, restricted to `within`.
fn ordered_successors_within(
    node: &NodeId,
    adjacency: &HashMap<NodeId, Vec<NodeId>>,
    within: &HashSet<NodeId>,
) -> Vec<NodeId> {
    debug_ord_sorted(
        adjacency
            .get(node)
            .into_iter()
            .flatten()
            .filter(|s| within.contains(*s))
            .cloned(),
    )
}

/// Tarjan strongly-connected components over the subgraph induced on `residue`
/// (forward adjacency restricted to residue members).
///
/// Iterative (explicit work stack) to stay O(V+E) without recursion-depth risk
/// on long stranded chains. Outer node iteration and successor enumeration are
/// both `DebugOrd`-ordered, and the returned SCC list is sorted by each SCC's
/// DebugOrd-min member, so the result is fully deterministic.
fn residue_sccs(
    residue: &HashSet<NodeId>,
    adjacency: &HashMap<NodeId, Vec<NodeId>>,
) -> Vec<Vec<NodeId>> {
    /// One explicit DFS frame: a node, its DebugOrd-ordered residue-successors,
    /// and a cursor into that successor list.
    struct Frame {
        node: NodeId,
        succs: Vec<NodeId>,
        next: usize,
    }

    let mut index_counter = 0usize;
    let mut indices: HashMap<NodeId, usize> = HashMap::new();
    let mut lowlinks: HashMap<NodeId, usize> = HashMap::new();
    let mut on_stack: HashSet<NodeId> = HashSet::new();
    let mut tstack: Vec<NodeId> = Vec::new();
    let mut sccs: Vec<Vec<NodeId>> = Vec::new();

    for root in debug_ord_sorted(residue.iter().cloned()) {
        if indices.contains_key(&root) {
            continue;
        }
        // Register + push the root frame.
        indices.insert(root.clone(), index_counter);
        lowlinks.insert(root.clone(), index_counter);
        index_counter += 1;
        tstack.push(root.clone());
        on_stack.insert(root.clone());
        let succs = ordered_successors_within(&root, adjacency, residue);
        let mut work: Vec<Frame> = vec![Frame {
            node: root,
            succs,
            next: 0,
        }];

        while let Some(top) = work.last().map(|f| f.node.clone()) {
            let frame_idx = work.len() - 1;
            let (next, succ_len) = {
                let f = &work[frame_idx];
                (f.next, f.succs.len())
            };
            if next < succ_len {
                let w = work[frame_idx].succs[next].clone();
                work[frame_idx].next += 1;
                if !indices.contains_key(&w) {
                    // Tree edge: register w and descend.
                    indices.insert(w.clone(), index_counter);
                    lowlinks.insert(w.clone(), index_counter);
                    index_counter += 1;
                    tstack.push(w.clone());
                    on_stack.insert(w.clone());
                    let succs = ordered_successors_within(&w, adjacency, residue);
                    work.push(Frame {
                        node: w,
                        succs,
                        next: 0,
                    });
                } else if on_stack.contains(&w) {
                    // Back/cross edge to a stack node: pull lowlink down to its index.
                    let wi = indices[&w];
                    let cur = lowlinks[&top];
                    lowlinks.insert(top.clone(), cur.min(wi));
                }
            } else {
                // Successors exhausted: if `top` is an SCC root, pop the component.
                if lowlinks[&top] == indices[&top] {
                    let mut scc: Vec<NodeId> = Vec::new();
                    loop {
                        let w = tstack.pop().expect("tarjan stack nonempty at SCC root");
                        on_stack.remove(&w);
                        scc.push(w.clone());
                        if w == top {
                            break;
                        }
                    }
                    sccs.push(scc);
                }
                let low = lowlinks[&top];
                work.pop();
                if let Some(parent) = work.last() {
                    // Propagate child lowlink to parent on return.
                    let pnode = parent.node.clone();
                    let pcur = lowlinks[&pnode];
                    lowlinks.insert(pnode, pcur.min(low));
                }
            }
        }
    }

    // Deterministic SCC order: by each component's DebugOrd-min member.
    sccs.sort_by_key(|scc| scc.iter().cloned().map(DebugOrd).min());
    sccs
}

/// A deterministic ordered path through an SCC's members for the diagnostic
/// message: an iterative pre-order DFS confined to the SCC, starting at the
/// DebugOrd-min member and following SCC-internal successors in DebugOrd order.
///
/// Because the component is strongly connected, this reaches EVERY member, so
/// the path names them all (the acceptance bars require each member named).
fn scc_ordered_path(scc: &[NodeId], adjacency: &HashMap<NodeId, Vec<NodeId>>) -> Vec<NodeId> {
    let scc_set: HashSet<NodeId> = scc.iter().cloned().collect();
    let start = match debug_ord_sorted(scc.iter().cloned()).into_iter().next() {
        Some(s) => s,
        None => return Vec::new(),
    };
    let mut path: Vec<NodeId> = Vec::new();
    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut stack = vec![start];
    while let Some(node) = stack.pop() {
        if !visited.insert(node.clone()) {
            continue;
        }
        path.push(node.clone());
        // Push successors in REVERSE DebugOrd order so they pop ascending.
        let mut succ = ordered_successors_within(&node, adjacency, &scc_set);
        succ.reverse();
        for s in succ {
            if !visited.contains(&s) {
                stack.push(s);
            }
        }
    }
    path
}

/// Build one `E_EVAL_CYCLE` diagnostic for a cyclic SCC, naming its members via
/// [`NodeId::describe`] along the deterministic ordered path.
fn eval_cycle_diagnostic(scc: &[NodeId], adjacency: &HashMap<NodeId, Vec<NodeId>>) -> Diagnostic {
    let members = scc_ordered_path(scc, adjacency)
        .iter()
        .map(NodeId::describe)
        .collect::<Vec<_>>()
        .join(", ");
    Diagnostic::error(format!("evaluation cycle detected: [{members}]"))
        .with_code(DiagnosticCode::EvalCycle)
}

/// Geometry-backed-constraint-on-auto guard (→ `E_EVAL_UNRESOLVED`).
///
/// For each constraint (visited in `DebugOrd` order for determinism), the guard
/// asks whether the constraint's transitive geometry-backed read closure — the
/// backing realizations named by `realization_reads`, then each realization's own
/// `reads` (value cells) and `realization_reads` (nested realizations),
/// recursively — reaches any auto value cell ([`EvaluationGraph::is_auto_cell`]).
/// If so, the unified pass declines to solve that constraint and emits one
/// `E_EVAL_UNRESOLVED`.
///
/// The per-realization "does its closure reach an auto cell?" classification is
/// computed ONCE up front by [`realizations_reaching_auto`] and shared across all
/// constraints, so a realization sub-DAG common to many constraints is walked at
/// most once — keeping the guard O(V+E) rather than O(constraints × realizations)
/// on designs with many constraints over shared geometry.
///
/// An independent classifier: it never touches the cycle residue.
///
/// # Decline-to-solve is the intended δ contract (known limitation)
///
/// This guard UNCONDITIONALLY declares the geometry-backed-constraint-on-auto
/// class unsolvable: the δ driver is a pure structural planner with NO solver
/// knowledge, so it cannot distinguish a genuinely under-determined constraint
/// from one the solver would legitimately settle. A design whose solver resolves
/// such auto parameters builds cleanly under [`BuildScheduler::LegacyMultiPass`]
/// but would surface a hard [`reify_core::Severity::Error`] here — a deliberate
/// decline-to-solve, NOT a proven structural impossibility. This is exactly why
/// `E_EVAL_UNRESOLVED` is reachable only behind [`BuildScheduler::UnifiedDag`]
/// (default OFF): production is unaffected until ε wires the executors + the
/// runtime `DeterminacyState::Determined` readiness gate onto the schedule, the
/// layer that refines (and for solver-resolvable autos, suppresses) this class.
/// Such constraints are intentionally out of scope for δ's structural pass.
fn unresolved_diagnostics(
    graph: &EvaluationGraph,
    traces: &HashMap<NodeId, DependencyTrace>,
) -> Vec<Diagnostic> {
    // Emit one E_EVAL_UNRESOLVED per constraint whose transitive geometry-backed
    // read closure reaches an auto cell. The set is computed by
    // [`constraints_reaching_auto`] — the SAME predicate ε's Constraint executor
    // consults to DECLINE these constraints, so δ's diagnostic and ε's decline
    // cannot diverge: the executor skips precisely the constraints flagged here,
    // leaving E_EVAL_UNRESOLVED as the sole signal (no contradicting verdict).
    //
    // Sorting the set by the shared [`DebugOrd`] total order keeps the diagnostic
    // vector deterministic and byte-identical to the pre-refactor per-constraint
    // scan (which sorted ALL constraints then kept the auto-reaching ones — a
    // subsequence of a sorted list is itself sorted, so the order is unchanged).
    debug_ord_sorted(
        constraints_reaching_auto(graph, traces)
            .into_iter()
            .map(NodeId::Constraint),
    )
    .into_iter()
    .map(|c| {
        Diagnostic::error(format!(
            "unresolved constraint: {} transitively depends on auto parameter(s) \
             through geometry-backed inputs",
            c.describe()
        ))
        .with_code(DiagnosticCode::EvalUnresolved)
    })
    .collect()
}

/// The set of constraint nodes whose transitive geometry-backed read closure
/// reaches an auto value cell — EXACTLY the constraints for which
/// [`unresolved_diagnostics`] emits one `E_EVAL_UNRESOLVED`.
///
/// A constraint is in the set iff ANY realization in its `realization_reads` is
/// auto-reaching (classified ONCE by [`realizations_reaching_auto`], shared
/// across all constraints — so C constraints over a common R-realization sub-DAG
/// cost O(V+E), not O(C·R)).
///
/// This is the SINGLE source of the "geometry-backed-constraint-on-auto" class:
/// both δ's `unresolved_diagnostics` (which flags the class with
/// `E_EVAL_UNRESOLVED`) and ε's Constraint executor (which DECLINES the class —
/// `Engine::check_constraints_post_geometry`) derive from this one predicate, so
/// the diagnostic and the decline cannot diverge. The executor must DROP a
/// declined constraint's expr BEFORE any fold/eval (per esc-4358-124: an unfolded
/// `CrossSubGeometryRef` reaching `eval_expr` is a build PANIC, not `Undef`).
///
/// Order-insensitive — the result is consulted only via `contains` (the executor)
/// or sorted by [`debug_ord_sorted`] (the diagnostics), so no `HashMap` iteration
/// order leaks into either output.
pub(crate) fn constraints_reaching_auto(
    graph: &EvaluationGraph,
    traces: &HashMap<NodeId, DependencyTrace>,
) -> HashSet<ConstraintNodeId> {
    // Classify every realization's auto-reachability ONCE, shared across all
    // constraints (a per-constraint walk would re-classify shared realization
    // sub-DAGs — O(constraints × realizations)).
    let reaches_auto = realizations_reaching_auto(graph, traces);
    traces
        .iter()
        .filter_map(|(node, tr)| {
            let NodeId::Constraint(c) = node else {
                return None;
            };
            // In the set iff ANY backing realization's closure reaches an auto cell.
            tr.realization_reads
                .iter()
                .any(|r| reaches_auto.contains(r))
                .then(|| c.clone())
        })
        .collect()
}

/// Classify every realization in `traces` ONCE by whether its transitive
/// geometry-backed read closure (following `realization_reads` edges) reaches a
/// realization that directly reads an auto value cell
/// ([`EvaluationGraph::is_auto_cell`]).
///
/// Returns the set of realizations whose closure reaches an auto cell. A
/// constraint is unresolved iff ANY realization in its `realization_reads` is in
/// this set — equivalent to the old per-constraint closure walk, but each
/// realization is classified at most once per pass (shared memo), so C
/// constraints over a common R-realization sub-DAG cost O(V+E), not O(C·R).
///
/// Computed as backward reachability: seed the worklist with the realizations
/// that DIRECTLY read an auto cell, then propagate along reader edges (if `R`
/// reads `R'` and `R'` reaches auto, so does `R`). The visited-set bound keeps it
/// O(V+E) and makes realization↔realization cycles safe. Order-insensitive — the
/// result is consulted only via `contains`, so it never leaks `HashMap`
/// iteration order into the diagnostic vector.
///
/// # Known limitation (intentional δ scope)
///
/// The closure follows ONLY realization→realization (`realization_reads`) edges
/// and tests each realization's DIRECT `reads` for auto-ness. It does NOT chase
/// value-cell→value-cell read chains: a realization that reads a NON-auto `let`
/// cell which itself transitively depends on an auto param is NOT tainted, so a
/// constraint backed only through such a value chain emits no
/// `E_EVAL_UNRESOLVED`. This is deliberate — the δ contract requires auto to be
/// reached through a realization's DIRECT read, and the pure structural pass
/// stops here: telling a value-chain-to-auto the solver would legitimately settle
/// apart from one it would not is solver-aware reasoning that belongs to ε's
/// executors + the `DeterminacyState::Determined` readiness gate, not δ's planner.
/// Extending the seed to taint value→value-to-auto chains (with a covering test)
/// is the additive ε refinement if that class should also be declined.
fn realizations_reaching_auto(
    graph: &EvaluationGraph,
    traces: &HashMap<NodeId, DependencyTrace>,
) -> HashSet<RealizationNodeId> {
    // Reader edges (reverse of `realization_reads`) + the directly-auto seed set,
    // both built in a single pass over the trace map.
    let mut readers: HashMap<RealizationNodeId, Vec<RealizationNodeId>> = HashMap::new();
    let mut reaches: HashSet<RealizationNodeId> = HashSet::new();
    let mut stack: Vec<RealizationNodeId> = Vec::new();
    for (node, tr) in traces {
        let NodeId::Realization(r) = node else {
            continue;
        };
        // Seed: a realization that DIRECTLY reads an auto cell reaches auto.
        if tr.reads.iter().any(|vc| graph.is_auto_cell(vc)) && reaches.insert(r.clone()) {
            stack.push(r.clone());
        }
        // Reader edge: `r` reads each `rr`, so taint on `rr` propagates to `r`.
        for rr in &tr.realization_reads {
            readers.entry(rr.clone()).or_default().push(r.clone());
        }
    }
    // Propagate auto-reachability backward: anything that reads a reaches-auto
    // realization also reaches auto. Visited-set (`reaches`) bounds it to O(V+E).
    while let Some(r) = stack.pop() {
        if let Some(rs) = readers.get(&r) {
            for reader in rs {
                if reaches.insert(reader.clone()) {
                    stack.push(reader.clone());
                }
            }
        }
    }
    reaches
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Task 4362 ι (step-1/RED): `BuildScheduler::from_env_value` is the PURE
    /// (no real env read) string→scheduler parser. After the Stage-4 default
    /// flip, `UnifiedDag` is the default: `None`, empty, garbage, `"unified"`,
    /// and `"  UNIFIED "` all yield `UnifiedDag`; only `"legacy"` / `"Legacy"`
    /// yield `LegacyMultiPass` (the one-release kill-switch). Pure ⇒
    /// parallel-safe.
    #[test]
    fn build_scheduler_from_env_value_parsing() {
        // Default: absent env → UnifiedDag (post-cutover default).
        assert_eq!(
            BuildScheduler::from_env_value(None),
            BuildScheduler::UnifiedDag
        );
        // Kill-switch: "legacy" / "Legacy" → LegacyMultiPass.
        assert_eq!(
            BuildScheduler::from_env_value(Some("legacy")),
            BuildScheduler::LegacyMultiPass
        );
        assert_eq!(
            BuildScheduler::from_env_value(Some("Legacy")),
            BuildScheduler::LegacyMultiPass
        );
        // Explicit unified (pure parser — feature-independent).
        assert_eq!(
            BuildScheduler::from_env_value(Some("unified")),
            BuildScheduler::UnifiedDag
        );
        // Case-insensitive + surrounding whitespace tolerated (unified path).
        assert_eq!(
            BuildScheduler::from_env_value(Some("  UNIFIED ")),
            BuildScheduler::UnifiedDag
        );
        // Garbage / empty → default UnifiedDag (post-cutover).
        assert_eq!(
            BuildScheduler::from_env_value(Some("garbage")),
            BuildScheduler::UnifiedDag
        );
        assert_eq!(
            BuildScheduler::from_env_value(Some("")),
            BuildScheduler::UnifiedDag
        );
    }

    /// Task 4362 ι (step-1/RED): the `Default` impl must be `UnifiedDag` after
    /// the Stage-4 cutover. The legacy `#[default]` on `LegacyMultiPass` is
    /// moved to `UnifiedDag`.
    #[test]
    fn build_scheduler_default_is_unified_dag() {
        assert_eq!(BuildScheduler::default(), BuildScheduler::UnifiedDag);
    }

    /// Task 4362 ι (step-1/RED): direct coverage of the PRODUCTION
    /// [`BuildScheduler::from_env`] wrapper — now cfg-split-free. After the
    /// Stage-4 cutover `from_env()` is a single-line delegation to
    /// `from_env_value()` in BOTH feature configs, so the assertion is
    /// feature-independent and the two prior cfg-gated tests are replaced by
    /// this one. `from_env()` must equal `from_env_value` applied to the real
    /// env (`REIFY_BUILD_SCHEDULER`). No `std::env::set_var` — `unsafe` in
    /// Rust 2024 + race-prone across parallel tests.
    #[test]
    fn from_env_delegates_to_parser_over_real_env() {
        let expected =
            BuildScheduler::from_env_value(std::env::var(BuildScheduler::ENV_VAR).ok().as_deref());
        assert_eq!(BuildScheduler::from_env(), expected);
    }

    // --- run_unified_pass driver tests (step-7+) ---

    use crate::cache::NodeId;
    use crate::deps::DependencyTrace;
    use crate::graph::{EvaluationGraph, ValueCellNode};
    use reify_compiler::ValueCellKind;
    use reify_core::{
        ConstraintNodeId, ContentHash, DiagnosticCode, RealizationNodeId, ResolutionNodeId, Type,
        ValueCellId,
    };
    use std::collections::{HashMap, HashSet};

    /// Build a `DependencyTrace` from explicit reads + realization_reads.
    fn trace(
        reads: Vec<ValueCellId>,
        realization_reads: Vec<RealizationNodeId>,
    ) -> DependencyTrace {
        DependencyTrace {
            reads,
            realization_reads,
        }
    }

    /// Map each scheduled node to its position for ordering assertions.
    fn positions(schedule: &[NodeId]) -> HashMap<NodeId, usize> {
        schedule
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, n)| (n, i))
            .collect()
    }

    /// Assert `schedule` is a valid topological order over `traces`: every node
    /// appears after ALL of its in-set `reads` (→Value) and `realization_reads`
    /// (→Realization) predecessors.
    fn assert_topo_valid(schedule: &[NodeId], traces: &HashMap<NodeId, DependencyTrace>) {
        let pos = positions(schedule);
        for (node, tr) in traces {
            let npos = pos[node];
            for r in &tr.reads {
                let p = NodeId::Value(r.clone());
                if let Some(&pp) = pos.get(&p) {
                    assert!(pp < npos, "Value pred {p} must precede {node}");
                }
            }
            for rr in &tr.realization_reads {
                let p = NodeId::Realization(rr.clone());
                if let Some(&pp) = pos.get(&p) {
                    assert!(pp < npos, "Realization pred {p} must precede {node}");
                }
            }
        }
    }

    /// Task 4357 δ (step-7): a synthetic ACYCLIC graph spanning every
    /// forward-trace edge kind — a param VC, a realization reading it
    /// (VC→Realization), a geometry VC backed by that realization
    /// (Realization→Value), a constraint reading the geometry
    /// (Constraint→Realization), a realization→realization GeomRef::Sub edge
    /// (Realization→Realization), and a Resolution whose reads = auto_params
    /// (Resolution→Value). `run_unified_pass` must produce a valid topological
    /// schedule covering EXACTLY the trace-map keys, with empty residue and zero
    /// diagnostics. The realization→realization edge pins that `realization_reads`
    /// participates in in-degree (which `compute_levels` ignores).
    ///
    /// RED until step-8 implements `run_unified_pass`.
    #[test]
    fn unified_pass_acyclic_all_edge_kinds_schedules_everything() {
        let e = "E";
        let p = ValueCellId::new(e, "p");
        let g = ValueCellId::new(e, "g");
        let a = ValueCellId::new(e, "a");
        // Producer index 1, consumer index 0: the consumer reads the producer
        // via realization_reads, so honoring that edge forces producer(idx1)
        // BEFORE consumer(idx0) — contradicting DebugOrd's natural "0 < 1" order.
        let r_prod = RealizationNodeId::new(e, 1);
        let r_cons = RealizationNodeId::new(e, 0);
        let c0 = ConstraintNodeId::new(e, 0);
        let s0 = ResolutionNodeId::new(e, 0);

        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        // Roots.
        traces.insert(NodeId::Value(p.clone()), trace(vec![], vec![]));
        traces.insert(NodeId::Value(a.clone()), trace(vec![], vec![]));
        // VC → Realization (producer reads param p).
        traces.insert(
            NodeId::Realization(r_prod.clone()),
            trace(vec![p.clone()], vec![]),
        );
        // Realization → Realization (consumer reads producer via GeomRef::Sub).
        traces.insert(
            NodeId::Realization(r_cons.clone()),
            trace(vec![], vec![r_prod.clone()]),
        );
        // Realization → Value (geometry cell backed by producer).
        traces.insert(
            NodeId::Value(g.clone()),
            trace(vec![], vec![r_prod.clone()]),
        );
        // Constraint → Realization (constraint reads geometry/producer).
        traces.insert(
            NodeId::Constraint(c0.clone()),
            trace(vec![], vec![r_prod.clone()]),
        );
        // Resolution → Value (resolution reads auto param a).
        traces.insert(
            NodeId::Resolution(s0.clone()),
            trace(vec![a.clone()], vec![]),
        );

        let graph = EvaluationGraph::default();
        let result = run_unified_pass(&graph, &traces);

        // (a) valid topological order over all edge kinds.
        assert_topo_valid(&result.schedule, &traces);
        // realization_reads participates: producer(idx1) before consumer(idx0).
        let pos = positions(&result.schedule);
        assert!(
            pos[&NodeId::Realization(r_prod.clone())] < pos[&NodeId::Realization(r_cons.clone())],
            "producer realization must precede consumer despite lower DebugOrd; schedule={:?}",
            result.schedule
        );

        // (b) schedule covers EXACTLY the trace-map keys (no Compute nodes here).
        let scheduled: HashSet<NodeId> = result.schedule.iter().cloned().collect();
        let keys: HashSet<NodeId> = traces.keys().cloned().collect();
        assert_eq!(
            scheduled, keys,
            "schedule must cover exactly the trace-map keys"
        );
        assert_eq!(
            result.schedule.len(),
            traces.len(),
            "no node scheduled twice"
        );

        // (c) residue empty.
        assert!(
            result.residue.is_empty(),
            "acyclic graph must leave empty residue, got {:?}",
            result.residue
        );
        // (d) zero diagnostics.
        assert!(
            result.diagnostics.is_empty(),
            "acyclic graph must emit zero diagnostics, got {}",
            result.diagnostics.len()
        );
    }

    /// Task 4357 δ (step-9): a genuine `|SCC|>1` cycle — two value cells each
    /// reading the other (a param↔let value cycle) — plus a downstream acyclic
    /// consumer stranded behind the cycle. `run_unified_pass` must:
    /// (a) leave BOTH cycle members in `residue`, absent from `schedule` (they
    ///     never reach in-degree 0, so they are never executed);
    /// (b) emit EXACTLY ONE `E_EVAL_CYCLE` diagnostic (code == `EvalCycle`)
    ///     whose message names BOTH members via `NodeId::describe()`;
    /// (c) NOT emit a second `E_EVAL_CYCLE` for the stranded downstream consumer
    ///     (a singleton-no-self-edge SCC in residue → no diagnostic).
    ///
    /// RED until step-10 lands the Stage B Tarjan SCC discriminator.
    #[test]
    fn unified_pass_two_node_cycle_emits_single_eval_cycle() {
        let e = "E";
        let x = ValueCellId::new(e, "x");
        let y = ValueCellId::new(e, "y");
        // Downstream acyclic consumer reading a cycle member — stranded behind
        // the cycle (singleton-no-self-edge once the cycle pins it in residue).
        let d = ValueCellId::new(e, "d");

        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        // 2-cycle: x reads y, y reads x.
        traces.insert(NodeId::Value(x.clone()), trace(vec![y.clone()], vec![]));
        traces.insert(NodeId::Value(y.clone()), trace(vec![x.clone()], vec![]));
        // Downstream consumer reads x (in the cycle) ⇒ never reaches in-degree 0.
        traces.insert(NodeId::Value(d.clone()), trace(vec![x.clone()], vec![]));

        let graph = EvaluationGraph::default();
        let result = run_unified_pass(&graph, &traces);

        let nx = NodeId::Value(x.clone());
        let ny = NodeId::Value(y.clone());
        let nd = NodeId::Value(d.clone());

        // (a) both cycle members residue, never scheduled.
        assert!(result.residue.contains(&nx), "x must be in residue");
        assert!(result.residue.contains(&ny), "y must be in residue");
        assert!(
            !result.schedule.contains(&nx),
            "x must never be scheduled (cyclic)"
        );
        assert!(
            !result.schedule.contains(&ny),
            "y must never be scheduled (cyclic)"
        );
        // The stranded consumer is also residue (proves the singleton case is
        // exercised), but must NOT generate its own diagnostic — see (c).
        assert!(
            result.residue.contains(&nd),
            "downstream consumer must be stranded in residue"
        );

        // (b) exactly one E_EVAL_CYCLE diagnostic naming BOTH cycle members.
        let cycle_diags: Vec<&Diagnostic> = result
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::EvalCycle))
            .collect();
        assert_eq!(
            cycle_diags.len(),
            1,
            "exactly one E_EVAL_CYCLE expected; got {:?}",
            result
                .diagnostics
                .iter()
                .map(|d| (d.code, d.message.as_str()))
                .collect::<Vec<_>>()
        );
        let msg = cycle_diags[0].message.as_str();
        assert!(
            msg.contains(&nx.describe()),
            "cycle message must name x via describe() ({}); got: {msg}",
            nx.describe()
        );
        assert!(
            msg.contains(&ny.describe()),
            "cycle message must name y via describe() ({}); got: {msg}",
            ny.describe()
        );

        // (c) the stranded downstream consumer gets NO E_EVAL_CYCLE of its own:
        // only one cycle diagnostic total (asserted above) and it must not list
        // the singleton consumer in its ordered path.
        assert!(
            !msg.contains(&nd.describe()),
            "stranded consumer d must not appear in any cycle path; got: {msg}"
        );
    }

    /// All `E_EVAL_CYCLE` diagnostics in a result, in emission order.
    fn cycle_diags(result: &UnifiedPassResult) -> Vec<&Diagnostic> {
        result
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::EvalCycle))
            .collect()
    }

    /// Task 4357 δ (step-11a): self-loop `let x = x` — a singleton SCC carrying
    /// a self-edge — must emit EXACTLY ONE `E_EVAL_CYCLE` naming `x`. The node
    /// reads itself, so it never reaches in-degree 0 and lands in residue as a
    /// singleton; step-12 must classify the self-edge as a cycle (step-10's
    /// `|SCC|>1` rule alone leaves it undiagnosed).
    ///
    /// RED until step-12 handles the singleton-with-self-edge case.
    #[test]
    fn unified_pass_self_loop_emits_one_eval_cycle() {
        let e = "E";
        let x = ValueCellId::new(e, "x");
        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        // x = x — the node reads itself.
        traces.insert(NodeId::Value(x.clone()), trace(vec![x.clone()], vec![]));

        let graph = EvaluationGraph::default();
        let result = run_unified_pass(&graph, &traces);

        let nx = NodeId::Value(x.clone());
        assert!(
            result.residue.contains(&nx),
            "self-loop node must be residue"
        );
        assert!(
            !result.schedule.contains(&nx),
            "self-loop node never scheduled"
        );

        let cyc = cycle_diags(&result);
        assert_eq!(
            cyc.len(),
            1,
            "self-loop must emit exactly one E_EVAL_CYCLE; got {:?}",
            result
                .diagnostics
                .iter()
                .map(|d| (d.code, d.message.as_str()))
                .collect::<Vec<_>>()
        );
        assert!(
            cyc[0].message.contains(&nx.describe()),
            "self-loop cycle must name x ({}); got: {}",
            nx.describe(),
            cyc[0].message
        );
    }

    /// Task 4357 δ (step-11b): two disjoint cycles must emit EXACTLY TWO
    /// `E_EVAL_CYCLE` diagnostics in a deterministic order (ordered by each
    /// SCC's DebugOrd-min member, so the `a↔b` cycle precedes the `x↔y` cycle).
    #[test]
    fn unified_pass_two_disjoint_cycles_emit_two_eval_cycles() {
        let e = "E";
        let x = ValueCellId::new(e, "x");
        let y = ValueCellId::new(e, "y");
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");
        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        // cycle 1: x ↔ y
        traces.insert(NodeId::Value(x.clone()), trace(vec![y.clone()], vec![]));
        traces.insert(NodeId::Value(y.clone()), trace(vec![x.clone()], vec![]));
        // cycle 2: a ↔ b
        traces.insert(NodeId::Value(a.clone()), trace(vec![b.clone()], vec![]));
        traces.insert(NodeId::Value(b.clone()), trace(vec![a.clone()], vec![]));

        let graph = EvaluationGraph::default();
        let result = run_unified_pass(&graph, &traces);

        let cyc = cycle_diags(&result);
        assert_eq!(cyc.len(), 2, "two disjoint cycles → two E_EVAL_CYCLE");
        // Deterministic order: a↔b (min member `a`) before x↔y (min member `x`).
        let m0 = cyc[0].message.as_str();
        let m1 = cyc[1].message.as_str();
        assert!(
            m0.contains(&NodeId::Value(a.clone()).describe())
                && m0.contains(&NodeId::Value(b.clone()).describe()),
            "first diagnostic must be the a↔b cycle; got: {m0}"
        );
        assert!(
            m1.contains(&NodeId::Value(x.clone()).describe())
                && m1.contains(&NodeId::Value(y.clone()).describe()),
            "second diagnostic must be the x↔y cycle; got: {m1}"
        );
    }

    /// Task 4357 δ (step-11c): a cross-kind cycle of a DIFFERENT pair —
    /// realization ↔ realization via `realization_reads` (the GeomRef::Sub edge
    /// `compute_levels` ignores) — must emit one `E_EVAL_CYCLE`, proving the
    /// detector is kind-agnostic over every edge kind α's trace map encodes.
    #[test]
    fn unified_pass_realization_cycle_is_kind_agnostic() {
        let e = "E";
        let r0 = RealizationNodeId::new(e, 0);
        let r1 = RealizationNodeId::new(e, 1);
        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        // r0 reads r1, r1 reads r0 — realization ↔ realization (GeomRef::Sub).
        traces.insert(
            NodeId::Realization(r0.clone()),
            trace(vec![], vec![r1.clone()]),
        );
        traces.insert(
            NodeId::Realization(r1.clone()),
            trace(vec![], vec![r0.clone()]),
        );

        let graph = EvaluationGraph::default();
        let result = run_unified_pass(&graph, &traces);

        let cyc = cycle_diags(&result);
        assert_eq!(
            cyc.len(),
            1,
            "realization↔realization cycle → one E_EVAL_CYCLE"
        );
        let m = cyc[0].message.as_str();
        assert!(
            m.contains(&NodeId::Realization(r0.clone()).describe()),
            "cycle must name r0; got: {m}"
        );
        assert!(
            m.contains(&NodeId::Realization(r1.clone()).describe()),
            "cycle must name r1; got: {m}"
        );
        assert!(result.residue.contains(&NodeId::Realization(r0)));
        assert!(result.residue.contains(&NodeId::Realization(r1)));
    }

    /// Task 4357 δ (step-11d): missing-producer — a consumer whose `reads` name
    /// a node ABSENT from the trace map. Per design decision #6 (in-set-only
    /// in-degree), the consumer still reaches in-degree 0 and is scheduled; it
    /// is never residue and never a cycle.
    #[test]
    fn unified_pass_missing_producer_schedules_no_cycle() {
        let e = "E";
        let c = ValueCellId::new(e, "c");
        let absent = ValueCellId::new(e, "absent"); // no trace entry ⇒ not in node set
        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        traces.insert(
            NodeId::Value(c.clone()),
            trace(vec![absent.clone()], vec![]),
        );

        let graph = EvaluationGraph::default();
        let result = run_unified_pass(&graph, &traces);

        let nc = NodeId::Value(c.clone());
        assert!(
            result.schedule.contains(&nc),
            "missing-producer consumer must be scheduled"
        );
        assert!(
            result.residue.is_empty(),
            "missing producer is never residue"
        );
        assert_eq!(
            cycle_diags(&result).len(),
            0,
            "missing producer must not emit E_EVAL_CYCLE"
        );
    }

    /// Task 4357 δ (step-11e): failed-realization shape — an acyclic realization
    /// node present in the graph. It always reaches in-degree 0, so it is
    /// scheduled, never residue, and never a cycle (its runtime kernel failure
    /// surfaces as a geometry-error diagnostic downstream at ε, not here).
    #[test]
    fn unified_pass_acyclic_realization_schedules_no_cycle() {
        let e = "E";
        let p = ValueCellId::new(e, "p");
        let r = RealizationNodeId::new(e, 0);
        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        traces.insert(NodeId::Value(p.clone()), trace(vec![], vec![]));
        traces.insert(
            NodeId::Realization(r.clone()),
            trace(vec![p.clone()], vec![]),
        );

        let graph = EvaluationGraph::default();
        let result = run_unified_pass(&graph, &traces);

        let nr = NodeId::Realization(r.clone());
        assert!(
            result.schedule.contains(&nr),
            "acyclic realization must be scheduled"
        );
        assert!(
            result.residue.is_empty(),
            "acyclic realization is never residue"
        );
        assert_eq!(
            cycle_diags(&result).len(),
            0,
            "acyclic realization must not emit E_EVAL_CYCLE"
        );
    }

    /// All `E_EVAL_UNRESOLVED` diagnostics in a result, in emission order.
    fn unresolved_diags(result: &UnifiedPassResult) -> Vec<&Diagnostic> {
        result
            .diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::EvalUnresolved))
            .collect()
    }

    /// Insert a value cell node of the given kind (auto-guard fixture helper).
    fn insert_cell(graph: &mut EvaluationGraph, id: &ValueCellId, kind: ValueCellKind) {
        graph.value_cells.insert(
            id.clone(),
            ValueCellNode {
                id: id.clone(),
                kind,
                cell_type: Type::dimensionless_scalar(),
                default_expr: None,
                content_hash: ContentHash::of_str(&id.to_string()),
            },
        );
    }

    /// Task 4357 δ (step-13): the geometry-backed-constraint-on-auto class. A
    /// constraint whose `realization_reads` reaches a realization whose `reads`
    /// include an AUTO value cell (the transitive auto-read closure) must emit
    /// exactly one `E_EVAL_UNRESOLVED` naming the constraint, with the graph
    /// otherwise ACYCLIC (residue empty, NO E_EVAL_CYCLE). A sibling constraint
    /// whose backing realization reads only NON-auto cells emits nothing.
    ///
    /// RED until step-14 lands the auto-read closure guard.
    #[test]
    fn unified_pass_geometry_backed_constraint_on_auto_is_unresolved() {
        let e = "E";
        let a = ValueCellId::new(e, "a"); // AUTO cell
        let p = ValueCellId::new(e, "p"); // non-auto param
        let r_auto = RealizationNodeId::new(e, 0); // reads the auto cell
        let r_plain = RealizationNodeId::new(e, 1); // reads only the param
        let c_unres = ConstraintNodeId::new(e, 0); // geometry-backed by r_auto
        let c_ok = ConstraintNodeId::new(e, 1); // geometry-backed by r_plain

        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        traces.insert(NodeId::Value(a.clone()), trace(vec![], vec![]));
        traces.insert(NodeId::Value(p.clone()), trace(vec![], vec![]));
        traces.insert(
            NodeId::Realization(r_auto.clone()),
            trace(vec![a.clone()], vec![]),
        );
        traces.insert(
            NodeId::Realization(r_plain.clone()),
            trace(vec![p.clone()], vec![]),
        );
        traces.insert(
            NodeId::Constraint(c_unres.clone()),
            trace(vec![], vec![r_auto.clone()]),
        );
        traces.insert(
            NodeId::Constraint(c_ok.clone()),
            trace(vec![], vec![r_plain.clone()]),
        );

        let mut graph = EvaluationGraph::default();
        insert_cell(&mut graph, &a, ValueCellKind::Auto { free: false });
        insert_cell(&mut graph, &p, ValueCellKind::Param);

        let result = run_unified_pass(&graph, &traces);

        // Graph is acyclic: empty residue, no cycle diagnostics.
        assert!(
            result.residue.is_empty(),
            "graph must be acyclic; residue={:?}",
            result.residue
        );
        assert_eq!(
            cycle_diags(&result).len(),
            0,
            "no E_EVAL_CYCLE on an acyclic graph"
        );

        // Exactly one E_EVAL_UNRESOLVED, naming the geometry-on-auto constraint.
        let unres = unresolved_diags(&result);
        assert_eq!(
            unres.len(),
            1,
            "exactly one E_EVAL_UNRESOLVED expected; got {:?}",
            result
                .diagnostics
                .iter()
                .map(|d| (d.code, d.message.as_str()))
                .collect::<Vec<_>>()
        );
        let nc_unres = NodeId::Constraint(c_unres.clone());
        let nc_ok = NodeId::Constraint(c_ok.clone());
        assert!(
            unres[0].message.contains(&nc_unres.describe()),
            "unresolved diagnostic must name the geometry-on-auto constraint ({}); got: {}",
            nc_unres.describe(),
            unres[0].message
        );
        assert!(
            !unres[0].message.contains(&nc_ok.describe()),
            "the non-auto-backed constraint must NOT be reported; got: {}",
            unres[0].message
        );
    }

    /// Task 4357 δ (amendment #4): the cycle and auto-reach classifiers are
    /// INDEPENDENT. A graph in which the auto-read closure passes THROUGH a cyclic
    /// realization sub-DAG yields BOTH an `E_EVAL_CYCLE` (for the cyclic
    /// realizations) AND an `E_EVAL_UNRESOLVED` (for the downstream constraint) —
    /// neither classifier suppresses the other.
    ///
    /// A `Constraint` node can never itself be a NAMED cycle member: it is a pure
    /// sink in the inverted-trace adjacency (nothing reads a constraint, so it has
    /// no outgoing edge and cannot close an SCC). When its backing realization is
    /// cyclic the constraint is merely STRANDED in residue (a singleton-no-self-
    /// edge SCC → no cycle diagnostic of its own) while still earning its
    /// `E_EVAL_UNRESOLVED`. This pins the documented split so a future refactor
    /// can't silently start double-reporting the same node under one error.
    #[test]
    fn unified_pass_cyclic_and_auto_reaching_emits_both_diagnostics() {
        let e = "E";
        let a = ValueCellId::new(e, "a"); // AUTO cell
        let r0 = RealizationNodeId::new(e, 0);
        let r1 = RealizationNodeId::new(e, 1);
        let c = ConstraintNodeId::new(e, 0);

        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        // Auto cell root.
        traces.insert(NodeId::Value(a.clone()), trace(vec![], vec![]));
        // Realization cycle r0 ↔ r1, AND r0 directly reads the auto cell, so the
        // cycle's transitive closure reaches auto.
        traces.insert(
            NodeId::Realization(r0.clone()),
            trace(vec![a.clone()], vec![r1.clone()]),
        );
        traces.insert(
            NodeId::Realization(r1.clone()),
            trace(vec![], vec![r0.clone()]),
        );
        // Constraint backed by the cyclic realization ⇒ reaches auto through it,
        // and is stranded behind the cycle (never reaches in-degree 0).
        traces.insert(
            NodeId::Constraint(c.clone()),
            trace(vec![], vec![r0.clone()]),
        );

        let mut graph = EvaluationGraph::default();
        insert_cell(&mut graph, &a, ValueCellKind::Auto { free: false });

        let result = run_unified_pass(&graph, &traces);

        let nr0 = NodeId::Realization(r0.clone());
        let nr1 = NodeId::Realization(r1.clone());
        let nc = NodeId::Constraint(c.clone());

        // Cyclic realizations + the stranded constraint all land in residue.
        assert!(result.residue.contains(&nr0), "r0 must be residue (cyclic)");
        assert!(result.residue.contains(&nr1), "r1 must be residue (cyclic)");
        assert!(
            result.residue.contains(&nc),
            "constraint is stranded behind the cyclic realization"
        );

        // Exactly one E_EVAL_CYCLE, naming the cyclic realizations — NOT the
        // constraint (a sink can never be a cycle member).
        let cyc = cycle_diags(&result);
        assert_eq!(
            cyc.len(),
            1,
            "one cyclic SCC → one E_EVAL_CYCLE; got {:?}",
            result
                .diagnostics
                .iter()
                .map(|d| (d.code, d.message.as_str()))
                .collect::<Vec<_>>()
        );
        assert!(
            cyc[0].message.contains(&nr0.describe()) && cyc[0].message.contains(&nr1.describe()),
            "cycle must name both realizations; got: {}",
            cyc[0].message
        );
        assert!(
            !cyc[0].message.contains(&nc.describe()),
            "constraint must NOT be a named cycle member; got: {}",
            cyc[0].message
        );

        // Exactly one E_EVAL_UNRESOLVED, naming the constraint — the independent
        // classifier fires even though the constraint's closure is also cyclic.
        let unres = unresolved_diags(&result);
        assert_eq!(
            unres.len(),
            1,
            "constraint reaching auto → one E_EVAL_UNRESOLVED; got {:?}",
            result
                .diagnostics
                .iter()
                .map(|d| (d.code, d.message.as_str()))
                .collect::<Vec<_>>()
        );
        assert!(
            unres[0].message.contains(&nc.describe()),
            "unresolved must name the constraint; got: {}",
            unres[0].message
        );
    }

    /// Serialize a result's diagnostics to the ordered `(code, message)` vector
    /// the determinism contract is defined over.
    fn diag_vector(result: &UnifiedPassResult) -> Vec<(Option<DiagnosticCode>, String)> {
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.message.clone()))
            .collect()
    }

    /// Task 4357 δ (step-15): the determinism contract. A graph combining TWO
    /// disjoint cycles (`a↔b` and `x↔y`) AND a geometry-backed-constraint-on-auto
    /// must produce a byte-identical diagnostic vector — the ordered sequence of
    /// `(code, message)` pairs — across 100 runs AND across deliberately shuffled
    /// trace-map insertion orders. Each run rebuilds the trace map in a fresh
    /// `HashMap` (a new `RandomState` seed ⇒ a different iteration order), so any
    /// HashMap-order leak into the schedule, SCC enumeration, per-SCC ordered
    /// path, SCC emission order, or the cycle-vs-unresolved diagnostic order
    /// would surface as a mismatch.
    ///
    /// RED if any order-sensitive step is left unsorted.
    #[test]
    fn unified_pass_diagnostic_vector_is_deterministic() {
        let e = "E";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");
        let x = ValueCellId::new(e, "x");
        let y = ValueCellId::new(e, "y");
        let gcell = ValueCellId::new(e, "gcell"); // AUTO cell behind the constraint
        let r = RealizationNodeId::new(e, 0);
        let c = ConstraintNodeId::new(e, 0);

        // The full node set as (key, trace) entries — inserted in a permuted
        // sequence per run so the underlying HashMap layout varies.
        let entries: Vec<(NodeId, DependencyTrace)> = vec![
            // cycle 1: a ↔ b
            (NodeId::Value(a.clone()), trace(vec![b.clone()], vec![])),
            (NodeId::Value(b.clone()), trace(vec![a.clone()], vec![])),
            // cycle 2: x ↔ y
            (NodeId::Value(x.clone()), trace(vec![y.clone()], vec![])),
            (NodeId::Value(y.clone()), trace(vec![x.clone()], vec![])),
            // geometry-backed-constraint-on-auto: c → r → gcell(auto)
            (NodeId::Value(gcell.clone()), trace(vec![], vec![])),
            (
                NodeId::Realization(r.clone()),
                trace(vec![gcell.clone()], vec![]),
            ),
            (
                NodeId::Constraint(c.clone()),
                trace(vec![], vec![r.clone()]),
            ),
        ];
        let n = entries.len();

        // Rebuild the trace map by inserting `entries` in the given `order`.
        let build_map = |order: &[usize]| -> HashMap<NodeId, DependencyTrace> {
            let mut m = HashMap::new();
            for &i in order {
                let (k, v) = entries[i].clone();
                m.insert(k, v);
            }
            m
        };
        // `gcell` must read as auto in the graph for the unresolved guard.
        let build_graph = || {
            let mut g = EvaluationGraph::default();
            insert_cell(&mut g, &gcell, ValueCellKind::Auto { free: false });
            g
        };

        let canonical: Vec<usize> = (0..n).collect();

        // Reference vector + expected shape: two cycles then one unresolved, the
        // a↔b cycle (DebugOrd-min `a`) before the x↔y cycle (min `x`).
        let reference = diag_vector(&run_unified_pass(&build_graph(), &build_map(&canonical)));
        assert_eq!(
            reference.len(),
            3,
            "expected exactly 2 EvalCycle + 1 EvalUnresolved; got {reference:?}"
        );
        assert_eq!(reference[0].0, Some(DiagnosticCode::EvalCycle));
        assert_eq!(reference[1].0, Some(DiagnosticCode::EvalCycle));
        assert_eq!(reference[2].0, Some(DiagnosticCode::EvalUnresolved));
        assert!(
            reference[0]
                .1
                .contains(&NodeId::Value(a.clone()).describe())
                && reference[0]
                    .1
                    .contains(&NodeId::Value(b.clone()).describe()),
            "first cycle must be a↔b; got: {}",
            reference[0].1
        );
        assert!(
            reference[1]
                .1
                .contains(&NodeId::Value(x.clone()).describe())
                && reference[1]
                    .1
                    .contains(&NodeId::Value(y.clone()).describe()),
            "second cycle must be x↔y; got: {}",
            reference[1].1
        );
        assert!(
            reference[2]
                .1
                .contains(&NodeId::Constraint(c.clone()).describe()),
            "unresolved must name constraint c; got: {}",
            reference[2].1
        );

        // 100 fresh runs — each rebuilds the map (new RandomState seed).
        for i in 0..100 {
            let got = diag_vector(&run_unified_pass(&build_graph(), &build_map(&canonical)));
            assert_eq!(got, reference, "run {i} diverged from the reference vector");
        }

        // Deliberately shuffled insertion orders must not change the output.
        let mut shuffles: Vec<Vec<usize>> = Vec::new();
        shuffles.push((0..n).rev().collect()); // reversed
        for k in 1..n {
            let mut rot: Vec<usize> = (0..n).collect();
            rot.rotate_left(k);
            shuffles.push(rot); // every rotation
        }
        shuffles.push(vec![6, 0, 4, 2, 5, 1, 3]); // hand-picked scrambles
        shuffles.push(vec![3, 5, 1, 6, 0, 2, 4]);
        for order in &shuffles {
            let got = diag_vector(&run_unified_pass(&build_graph(), &build_map(order)));
            assert_eq!(
                got, reference,
                "insertion order {order:?} changed the diagnostic vector"
            );
        }
    }

    // --- run_unified_pass_seeded (dirty∩demand-seeded edit planner) tests (θ2 step-1) ---

    /// Build a `HashSet<NodeId>` seed from explicit value-cell node ids.
    fn seed_of(nodes: impl IntoIterator<Item = NodeId>) -> HashSet<NodeId> {
        nodes.into_iter().collect()
    }

    /// Task 4531 θ2 (step-1): on a linear chain `a → b → c` (b reads a, c reads b)
    /// seeded with `{b, c}`, the seeded planner must schedule EXACTLY the seed in a
    /// valid topological order — `[b, c]` — with the non-seed producer `a` ABSENT
    /// (bounded cost: the plan is O(seed), never the full graph).
    ///
    /// RED until step-2 implements `run_unified_pass_seeded`.
    #[test]
    fn seeded_pass_linear_chain_schedules_seed_in_topo_order() {
        let e = "E";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");
        let c = ValueCellId::new(e, "c");

        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        traces.insert(NodeId::Value(a.clone()), trace(vec![], vec![]));
        traces.insert(NodeId::Value(b.clone()), trace(vec![a.clone()], vec![]));
        traces.insert(NodeId::Value(c.clone()), trace(vec![b.clone()], vec![]));

        let seed = seed_of([NodeId::Value(b.clone()), NodeId::Value(c.clone())]);
        let schedule = run_unified_pass_seeded(&traces, &seed);

        assert_eq!(
            schedule,
            vec![NodeId::Value(b.clone()), NodeId::Value(c.clone())],
            "seed {{b, c}} must schedule [b, c] (b before c; producer a excluded)"
        );
        assert!(
            !schedule.contains(&NodeId::Value(a.clone())),
            "non-seed producer a must NOT appear in the schedule (bounded cost)"
        );
    }

    /// Task 4531 θ2 (step-1): an empty seed schedules nothing.
    ///
    /// RED until step-2.
    #[test]
    fn seeded_pass_empty_seed_is_empty() {
        let e = "E";
        let a = ValueCellId::new(e, "a");
        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        traces.insert(NodeId::Value(a.clone()), trace(vec![], vec![]));

        let seed: HashSet<NodeId> = HashSet::new();
        let schedule = run_unified_pass_seeded(&traces, &seed);
        assert!(
            schedule.is_empty(),
            "empty seed must yield an empty schedule, got {schedule:?}"
        );
    }

    /// Task 4531 θ2 (step-1): a diamond `top → {l, r} → bottom` seeded with the
    /// FULL cone must schedule all four nodes with every in-set predecessor before
    /// its consumer (parents precede children); `top` first, `bottom` last.
    ///
    /// RED until step-2.
    #[test]
    fn seeded_pass_diamond_full_cone_parents_precede_children() {
        let e = "E";
        let top = ValueCellId::new(e, "top");
        let l = ValueCellId::new(e, "l");
        let r = ValueCellId::new(e, "r");
        let bottom = ValueCellId::new(e, "bottom");

        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        traces.insert(NodeId::Value(top.clone()), trace(vec![], vec![]));
        traces.insert(NodeId::Value(l.clone()), trace(vec![top.clone()], vec![]));
        traces.insert(NodeId::Value(r.clone()), trace(vec![top.clone()], vec![]));
        traces.insert(
            NodeId::Value(bottom.clone()),
            trace(vec![l.clone(), r.clone()], vec![]),
        );

        let seed = seed_of([
            NodeId::Value(top.clone()),
            NodeId::Value(l.clone()),
            NodeId::Value(r.clone()),
            NodeId::Value(bottom.clone()),
        ]);
        let schedule = run_unified_pass_seeded(&traces, &seed);

        // Covers exactly the seed, no duplicates.
        let scheduled: HashSet<NodeId> = schedule.iter().cloned().collect();
        assert_eq!(scheduled, seed, "schedule must cover exactly the seed");
        assert_eq!(schedule.len(), 4, "no node scheduled twice");

        // Valid topological order (restricted to the seed nodes present).
        assert_topo_valid(&schedule, &traces);

        let pos = positions(&schedule);
        assert_eq!(
            pos[&NodeId::Value(top.clone())],
            0,
            "top (root) must be scheduled first"
        );
        assert_eq!(
            pos[&NodeId::Value(bottom.clone())],
            3,
            "bottom (sink) must be scheduled last"
        );
    }

    /// Task 4531 θ2 (step-1): a seed node whose ONLY producer is OUTSIDE the seed
    /// must still schedule (in-degree 0 because the external producer's edge is not
    /// counted) — never residue. Chain `a → b → c`, seed `{c}` only ⇒ `[c]`.
    ///
    /// RED until step-2.
    #[test]
    fn seeded_pass_external_producer_still_schedules() {
        let e = "E";
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");
        let c = ValueCellId::new(e, "c");

        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        traces.insert(NodeId::Value(a.clone()), trace(vec![], vec![]));
        traces.insert(NodeId::Value(b.clone()), trace(vec![a.clone()], vec![]));
        traces.insert(NodeId::Value(c.clone()), trace(vec![b.clone()], vec![]));

        let seed = seed_of([NodeId::Value(c.clone())]);
        let schedule = run_unified_pass_seeded(&traces, &seed);

        assert_eq!(
            schedule,
            vec![NodeId::Value(c.clone())],
            "seed {{c}} with external-only producer b must schedule [c] (never residue)"
        );
    }
}
