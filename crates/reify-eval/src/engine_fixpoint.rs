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

use reify_core::{Diagnostic, DiagnosticCode};

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
/// singleton self-loop. The `E_EVAL_UNRESOLVED` auto-guard (step-14) is layered
/// on in a later step.
pub fn run_unified_pass(
    graph: &EvaluationGraph,
    traces: &HashMap<NodeId, DependencyTrace>,
) -> UnifiedPassResult {
    // `graph` is consumed by the E_EVAL_UNRESOLVED auto-guard (step-14).
    let _ = graph;

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
    let mut ready: BTreeSet<DebugOrd> = in_degree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(n, _)| DebugOrd(n.clone()))
        .collect();
    let mut schedule: Vec<NodeId> = Vec::with_capacity(node_set.len());

    while let Some(DebugOrd(node)) = ready.pop_first() {
        if let Some(deps) = adjacency.get(&node) {
            for dep in deps {
                let d = in_degree.get_mut(dep).expect("dependent present in in_degree");
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

    UnifiedPassResult {
        schedule,
        residue,
        diagnostics,
    }
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
    adjacency.get(node).is_some_and(|succs| succs.contains(node))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Task 4357 δ (step-5): `BuildScheduler::from_env_value` is the PURE
    /// (no real env read) string→scheduler parser. Default is `LegacyMultiPass`;
    /// `"unified"` parses to `UnifiedDag` (feature-independent at the parser
    /// layer); case-insensitive + trimmed; any unrecognized/garbage value
    /// defaults to `LegacyMultiPass`. Pure ⇒ parallel-safe.
    ///
    /// RED until step-6 adds the enum + parser.
    #[test]
    fn build_scheduler_from_env_value_parsing() {
        // Default: absent env → Legacy.
        assert_eq!(
            BuildScheduler::from_env_value(None),
            BuildScheduler::LegacyMultiPass
        );
        // Explicit legacy.
        assert_eq!(
            BuildScheduler::from_env_value(Some("legacy")),
            BuildScheduler::LegacyMultiPass
        );
        // Explicit unified (pure parser — feature-independent).
        assert_eq!(
            BuildScheduler::from_env_value(Some("unified")),
            BuildScheduler::UnifiedDag
        );
        // Case-insensitive + surrounding whitespace tolerated.
        assert_eq!(
            BuildScheduler::from_env_value(Some("  UNIFIED ")),
            BuildScheduler::UnifiedDag
        );
        assert_eq!(
            BuildScheduler::from_env_value(Some("Legacy")),
            BuildScheduler::LegacyMultiPass
        );
        // Garbage / empty → default Legacy.
        assert_eq!(
            BuildScheduler::from_env_value(Some("garbage")),
            BuildScheduler::LegacyMultiPass
        );
        assert_eq!(
            BuildScheduler::from_env_value(Some("")),
            BuildScheduler::LegacyMultiPass
        );
    }

    /// Task 4357 δ (step-5): the `Default` impl must be `LegacyMultiPass` so an
    /// un-configured engine keeps byte-identical legacy behaviour.
    #[test]
    fn build_scheduler_default_is_legacy() {
        assert_eq!(BuildScheduler::default(), BuildScheduler::LegacyMultiPass);
    }

    // --- run_unified_pass driver tests (step-7+) ---

    use crate::cache::NodeId;
    use crate::deps::DependencyTrace;
    use crate::graph::EvaluationGraph;
    use reify_core::{
        ConstraintNodeId, DiagnosticCode, RealizationNodeId, ResolutionNodeId, ValueCellId,
    };
    use std::collections::{HashMap, HashSet};

    /// Build a `DependencyTrace` from explicit reads + realization_reads.
    fn trace(reads: Vec<ValueCellId>, realization_reads: Vec<RealizationNodeId>) -> DependencyTrace {
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
        traces.insert(NodeId::Value(g.clone()), trace(vec![], vec![r_prod.clone()]));
        // Constraint → Realization (constraint reads geometry/producer).
        traces.insert(
            NodeId::Constraint(c0.clone()),
            trace(vec![], vec![r_prod.clone()]),
        );
        // Resolution → Value (resolution reads auto param a).
        traces.insert(NodeId::Resolution(s0.clone()), trace(vec![a.clone()], vec![]));

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
        assert!(result.residue.contains(&nx), "self-loop node must be residue");
        assert!(!result.schedule.contains(&nx), "self-loop node never scheduled");

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
        assert_eq!(cyc.len(), 1, "realization↔realization cycle → one E_EVAL_CYCLE");
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
        traces.insert(NodeId::Value(c.clone()), trace(vec![absent.clone()], vec![]));

        let graph = EvaluationGraph::default();
        let result = run_unified_pass(&graph, &traces);

        let nc = NodeId::Value(c.clone());
        assert!(
            result.schedule.contains(&nc),
            "missing-producer consumer must be scheduled"
        );
        assert!(result.residue.is_empty(), "missing producer is never residue");
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
}
