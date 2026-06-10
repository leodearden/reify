//! Dirty cone computation and evaluation set construction.
//!
//! When a parameter changes, the dirty cone is the set of all nodes that
//! transitively depend on the changed cells. The evaluation set is the
//! intersection of the dirty cone and the demand cone, topologically sorted
//! so that dependencies are evaluated before their dependents.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use crate::cache::NodeId;
use crate::demand::DemandRegistry;
use crate::deps::{DependencyTrace, ReverseDependencyIndex};
use reify_core::{RealizationNodeId, ValueCellId};

/// Compute the dirty cone: all nodes that transitively depend on any changed cell.
///
/// BFS forward from changed cells through the reverse index. For each dependent
/// that is a Value(vcid), add vcid to the frontier for further propagation.
/// P3.3: for Compute(cn_id) dependents, look up
/// `graph.compute_nodes[cn_id].output_value_cells` and push each onto the
/// frontier — this realises edge #12 (ComputeNode → output ValueCell) inline
/// with the existing edge-#6 (VC → consumer) propagation.
/// Constraint and Realization nodes remain leaf nodes (no further propagation).
///
/// The changed cells themselves are NOT included in the result (they are roots).
pub fn compute_dirty_cone(
    changed: &HashSet<ValueCellId>,
    reverse_index: &ReverseDependencyIndex,
    graph: &crate::graph::EvaluationGraph,
) -> HashSet<NodeId> {
    let mut dirty = HashSet::new();
    let mut frontier: VecDeque<ValueCellId> = changed.iter().cloned().collect();

    while let Some(cell) = frontier.pop_front() {
        for dependent in reverse_index.dependents_of(&cell) {
            if dirty.insert(dependent.clone()) {
                // If the dependent is a Value node, continue propagation
                if let NodeId::Value(vcid) = dependent {
                    frontier.push_back(vcid.clone());
                }
                // P3.3 edge #12: if the dependent is a Compute node, mark
                // each of its declared output_value_cells as dirty AND push
                // it onto the frontier so its downstream dependents
                // (constraints, let-bindings, further compute nodes…) become
                // dirty in the same BFS pass. The output VCs are *direct*
                // downstream of the Compute node (the Compute writes them),
                // not edges in the reverse index, so they must be inserted
                // here — they don't surface via `dependents_of(cell)`.
                if let NodeId::Compute(cn_id) = dependent
                    && let Some(cn_data) = graph.compute_nodes.get(cn_id)
                {
                    for vc in &cn_data.output_value_cells {
                        if dirty.insert(NodeId::Value(vc.clone())) {
                            frontier.push_back(vc.clone());
                        }
                    }
                }
            }
        }
    }

    dirty
}

/// Compute the dirty cone seeded by both changed ValueCells and changed
/// RealizationNodes.
///
/// Extends [`compute_dirty_cone`] with a second source of seeds: a set of
/// Realizations whose result content-hash actually changed. For each such
/// Realization, every ComputeNode consuming it via edge #10
/// (`realization_inputs`) is marked dirty, and each ComputeNode's declared
/// `output_value_cells` (edge #12) is seeded onto the BFS frontier so its
/// downstream consumers propagate the same way `compute_dirty_cone` does
/// for ValueCell-seeded changes.
///
/// Seed discrimination (task-spec test 3, locked in by step-13): the caller
/// is responsible for only inserting Realizations whose content-hash
/// actually differs. An empty `changed_realizations` set yields no
/// Realization-driven propagation — the function only iterates over the
/// supplied seeds. This mirrors the existing `EvalOutcome::Changed/Unchanged`
/// pattern at the seed boundary instead of duplicating the comparison logic
/// inside the walk.
///
/// # Production wiring (staging note)
///
/// This entry point has no production call site yet — it is staged for
/// P3.4+, the upcoming ComputeNode-evaluation pipeline. P3.4 will compare
/// each evaluated RealizationNode's new `content_hash` against its cached
/// value (mirroring `record_evaluation_propagating_freshness`'s
/// `EvalOutcome::Changed/Unchanged` discrimination at the seed boundary),
/// route the truly-changed Realizations into `changed_realizations`, and
/// hand the result to this walk. The function is `pub` (so it is exempt
/// from `dead_code` lint) and is exercised by the tests below until the
/// production call site lands.
pub fn compute_dirty_cone_with_realizations(
    changed_vcs: &HashSet<ValueCellId>,
    changed_realizations: &HashSet<RealizationNodeId>,
    reverse_index: &ReverseDependencyIndex,
    graph: &crate::graph::EvaluationGraph,
) -> HashSet<NodeId> {
    let mut dirty: HashSet<NodeId> = HashSet::new();
    let mut frontier: VecDeque<ValueCellId> = changed_vcs.iter().cloned().collect();

    // Seed from changed realizations via edge #10 (Realization → Compute).
    // For each consuming ComputeNode, also seed edge #12 (Compute → output
    // ValueCells) onto the frontier so downstream BFS picks up dependents
    // of those output cells.
    for rid in changed_realizations {
        for dependent in reverse_index.realization_dependents_of(rid) {
            if dirty.insert(dependent.clone())
                && let NodeId::Compute(cn_id) = dependent
                && let Some(cn_data) = graph.compute_nodes.get(cn_id)
            {
                for vc in &cn_data.output_value_cells {
                    if dirty.insert(NodeId::Value(vc.clone())) {
                        frontier.push_back(vc.clone());
                    }
                }
            }
        }
    }

    // BFS over ValueCell dependents — identical to `compute_dirty_cone`.
    while let Some(cell) = frontier.pop_front() {
        for dependent in reverse_index.dependents_of(&cell) {
            if dirty.insert(dependent.clone()) {
                if let NodeId::Value(vcid) = dependent {
                    frontier.push_back(vcid.clone());
                }
                if let NodeId::Compute(cn_id) = dependent
                    && let Some(cn_data) = graph.compute_nodes.get(cn_id)
                {
                    for vc in &cn_data.output_value_cells {
                        if dirty.insert(NodeId::Value(vc.clone())) {
                            frontier.push_back(vc.clone());
                        }
                    }
                }
            }
        }
    }

    dirty
}

/// Topologically sort a set of nodes using Kahn's algorithm.
///
/// Only considers edges within the node set (external dependencies are ignored).
/// Tie-breaking uses Debug representation for deterministic output.
///
/// This is a convenience wrapper around [`compute_levels`] that flattens the
/// leveled output into a single ordered vector.
pub fn topological_sort(
    nodes: &HashSet<NodeId>,
    traces: &HashMap<NodeId, DependencyTrace>,
) -> Vec<NodeId> {
    compute_levels(nodes, traces)
        .into_iter()
        .flatten()
        .collect()
}

/// Compute topological levels from a set of nodes using Kahn's algorithm.
///
/// Each level contains nodes whose in-set dependencies have all been placed
/// in earlier levels. Nodes within a level have no dependencies on each other
/// and can safely execute concurrently.
///
/// Only considers edges within the node set (external dependencies are ignored).
/// Tie-breaking uses Debug representation for deterministic output within each level.
pub fn compute_levels(
    nodes: &HashSet<NodeId>,
    traces: &HashMap<NodeId, DependencyTrace>,
) -> Vec<Vec<NodeId>> {
    if nodes.is_empty() {
        return Vec::new();
    }

    // Build in-degree map (only counting edges within the node set)
    let mut in_degree: HashMap<NodeId, usize> = nodes.iter().map(|n| (n.clone(), 0)).collect();

    for node in nodes {
        if let Some(trace) = traces.get(node) {
            // Deduplicate reads to avoid over-counting in-degree
            // (e.g. expression `a * a` reads 'a' twice but has only 1 unique dep)
            let unique_deps: HashSet<&ValueCellId> = trace.reads.iter().collect();
            for dep_cell in unique_deps {
                let dep_node = NodeId::Value(dep_cell.clone());
                if nodes.contains(&dep_node) {
                    *in_degree.get_mut(node).unwrap() += 1;
                }
            }
        }
    }

    // Use BTreeSet with Debug repr for deterministic tie-breaking
    let mut ready: BTreeSet<DebugOrd> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(n, _)| DebugOrd(n.clone()))
        .collect();

    let mut levels = Vec::new();

    while !ready.is_empty() {
        // All nodes currently ready form one level
        let current_level: Vec<NodeId> = ready.iter().map(|d| d.0.clone()).collect();
        ready.clear();

        // Decrement in-degree for dependents of nodes in this level
        for node in &current_level {
            if let NodeId::Value(vcid) = node {
                for candidate in nodes {
                    if let Some(trace) = traces.get(candidate)
                        && trace.reads.contains(vcid)
                    {
                        let deg = in_degree.get_mut(candidate).unwrap();
                        debug_assert!(*deg > 0, "in-degree underflow: node {:?}", candidate);
                        *deg -= 1;
                        if *deg == 0 {
                            ready.insert(DebugOrd(candidate.clone()));
                        }
                    }
                }
            }
        }

        levels.push(current_level);
    }

    levels
}

/// Compute the evaluation set: intersection of dirty cone and demand cone,
/// topologically sorted so dependencies are evaluated before dependents.
pub fn compute_eval_set(
    dirty: &HashSet<NodeId>,
    demand: &DemandRegistry,
    traces: &HashMap<NodeId, DependencyTrace>,
) -> Vec<NodeId> {
    let intersection: HashSet<NodeId> = dirty
        .iter()
        .filter(|n| demand.is_demanded(n))
        .cloned()
        .collect();

    topological_sort(&intersection, traces)
}

/// A violation detected by [`check_dag_complete`].
///
/// Returned as the `Err` variant when the declared execution order (L(B))
/// is **not** a linear extension of the partial order induced by the
/// realization-edge graph.
///
/// Two variants:
/// - `MissingProducer`: the trace records an edge to a producer that never
///   appears in `exec_order` (i.e. a realization that was supposed to have
///   been built but wasn't scheduled at all).
/// - `BackwardEdge`: a realization-to-realization edge where the producer
///   is scheduled *after* its consumer (producer_pos >= consumer_pos).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DagViolation {
    /// A consumer depends on a producer realization that is not present in
    /// the execution order.
    MissingProducer {
        producer: RealizationNodeId,
        consumer: NodeId,
    },
    /// A realization consumer is scheduled before its producer realization.
    BackwardEdge {
        producer: RealizationNodeId,
        consumer: RealizationNodeId,
        producer_pos: usize,
        consumer_pos: usize,
    },
}

impl DagViolation {
    /// Human-readable description of the violation, used in `panic!` messages.
    pub(crate) fn describe(&self) -> String {
        match self {
            DagViolation::MissingProducer { producer, consumer } => {
                format!(
                    "assert_dag_complete: MissingProducer — \
                     consumer {:?} depends on producer {:?} \
                     which is absent from exec_order",
                    consumer, producer
                )
            }
            DagViolation::BackwardEdge {
                producer,
                consumer,
                producer_pos,
                consumer_pos,
            } => {
                format!(
                    "assert_dag_complete: BackwardEdge — \
                     producer {:?} (pos {}) must precede consumer {:?} (pos {}), \
                     but producer appears after consumer in exec_order",
                    producer, producer_pos, consumer, consumer_pos
                )
            }
        }
    }
}

/// Check that `exec_order` is a linear extension of the realization-edge
/// partial order encoded in `traces`.
///
/// For every consumer node `n` and every producer `P` in
/// `n.realization_reads`:
/// - Returns `Err(DagViolation::MissingProducer)` if `P` is not in
///   `exec_order`.
/// - Returns `Err(DagViolation::BackwardEdge)` if `n` is a
///   `NodeId::Realization(C)` and `pos[P] >= pos[C]` (producer must
///   strictly precede consumer).
///
/// Non-realization consumers (Value/Constraint/Resolution/Compute) only
/// get the `MissingProducer` check — they have no position in `exec_order`,
/// so ordering cannot be verified.
///
/// Iterates with a deterministic tie-break (node keys sorted by `Debug`
/// representation, mirroring [`DebugOrd`] in topological sort) so the
/// first-reported violation is stable across runs.
///
/// Returns `Ok(())` when no violation is found.
pub(crate) fn check_dag_complete(
    traces: &HashMap<NodeId, DependencyTrace>,
    exec_order: &[RealizationNodeId],
) -> Result<(), DagViolation> {
    // Build position map: RealizationNodeId → index in exec_order.
    let pos: HashMap<&RealizationNodeId, usize> = exec_order
        .iter()
        .enumerate()
        .map(|(i, rid)| (rid, i))
        .collect();

    // Sort nodes by Debug repr for deterministic first-violation reporting.
    let mut sorted_nodes: Vec<&NodeId> = traces.keys().collect();
    sorted_nodes.sort_by_key(|n| format!("{:?}", n));

    for node in sorted_nodes {
        let trace = &traces[node];
        for producer in &trace.realization_reads {
            match pos.get(producer) {
                None => {
                    return Err(DagViolation::MissingProducer {
                        producer: producer.clone(),
                        consumer: node.clone(),
                    });
                }
                Some(&p_pos) => {
                    // BackwardEdge only applies to Realization consumers.
                    if let NodeId::Realization(consumer_rid) = node {
                        if let Some(&c_pos) = pos.get(consumer_rid) {
                            if p_pos >= c_pos {
                                return Err(DagViolation::BackwardEdge {
                                    producer: producer.clone(),
                                    consumer: consumer_rid.clone(),
                                    producer_pos: p_pos,
                                    consumer_pos: c_pos,
                                });
                            }
                        }
                        // If consumer is a Realization but absent from exec_order,
                        // that is the caller's problem — skip ordering check.
                    }
                }
            }
        }
    }

    Ok(())
}

/// Debug-only wrapper: build the complete forward dependency trace map from
/// `graph` and `fields`, then assert that `exec_order` is a linear extension
/// of the realization-edge partial order.
///
/// Panics with a human-readable [`DagViolation::describe`] message when any
/// producer→consumer edge is missing or reversed in `exec_order`.
///
/// This is a no-op in release builds (the function body is empty when
/// `debug_assertions` are disabled — callers should gate the call with
/// `#[cfg(debug_assertions)]` so the exec_order allocation also disappears).
///
/// # Panics
///
/// Panics if `check_dag_complete` returns `Err(_)`.  The panic message always
/// contains `"assert_dag_complete"` so that `#[should_panic(expected =
/// "assert_dag_complete")]` tests reliably match it.
#[cfg(debug_assertions)]
pub(crate) fn assert_dag_complete_from_graph(
    graph: &crate::graph::EvaluationGraph,
    fields: &[reify_compiler::CompiledField],
    exec_order: &[RealizationNodeId],
) {
    let traces = crate::deps::build_trace_map_and_fields(graph, fields);
    if let Err(violation) = check_dag_complete(&traces, exec_order) {
        panic!("{}", violation.describe());
    }
}

/// Wrapper for NodeId that implements Ord based on Debug representation.
/// Used for deterministic tie-breaking in topological sort.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DebugOrd(NodeId);

impl PartialOrd for DebugOrd {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DebugOrd {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        format!("{:?}", self.0).cmp(&format!("{:?}", other.0))
    }
}

#[cfg(test)]
mod tests {
    use crate::cache::NodeId;
    use crate::deps::ReverseDependencyIndex;
    use crate::dirty::compute_dirty_cone;
    use reify_core::{ConstraintNodeId, ValueCellId};
    use std::collections::HashSet;

    #[test]
    fn dirty_cone_empty_changed_set() {
        use crate::graph::EvaluationGraph;
        let index = ReverseDependencyIndex::new();
        let graph = EvaluationGraph::default();
        let changed: HashSet<ValueCellId> = HashSet::new();
        let dirty = compute_dirty_cone(&changed, &index, &graph);
        assert!(dirty.is_empty());
    }

    #[test]
    fn dirty_cone_single_changed_param() {
        // width is read by volume (let) and C1 (constraint)
        use crate::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let index = ReverseDependencyIndex::build_from_graph(&graph);

        let e = "Bracket";
        let mut changed = HashSet::new();
        changed.insert(ValueCellId::new(e, "width"));

        let dirty = compute_dirty_cone(&changed, &index, &graph);

        // Dirty should contain volume and C1 (both read width)
        assert!(dirty.contains(&NodeId::Value(ValueCellId::new(e, "volume"))));
        assert!(dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))));
        // Changed cell itself is NOT in dirty cone
        assert!(!dirty.contains(&NodeId::Value(ValueCellId::new(e, "width"))));
        // Other nodes not affected
        assert!(!dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 0))));
        assert!(!dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 2))));
    }

    #[test]
    fn dirty_cone_bracket_change_width() {
        // Change width → dirty = {volume, C1, R0}
        // Excludes: fillet_radius, C0, C2
        use crate::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let index = ReverseDependencyIndex::build_from_graph(&graph);

        let e = "Bracket";
        let mut changed = HashSet::new();
        changed.insert(ValueCellId::new(e, "width"));

        let dirty = compute_dirty_cone(&changed, &index, &graph);

        assert!(dirty.contains(&NodeId::Value(ValueCellId::new(e, "volume"))));
        assert!(dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))));
        assert!(
            dirty.contains(&NodeId::Realization(reify_core::RealizationNodeId::new(
                e, 0
            )))
        );
        assert!(!dirty.contains(&NodeId::Value(ValueCellId::new(e, "fillet_radius"))));
        assert!(!dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 0))));
        assert!(!dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 2))));
        assert_eq!(dirty.len(), 3);
    }

    #[test]
    fn dirty_cone_bracket_change_thickness() {
        // Change thickness → dirty = {volume, C0, C1, C2, R0}
        // All constraints read thickness, volume reads thickness, R0 uses thickness as depth
        use crate::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let index = ReverseDependencyIndex::build_from_graph(&graph);

        let e = "Bracket";
        let mut changed = HashSet::new();
        changed.insert(ValueCellId::new(e, "thickness"));

        let dirty = compute_dirty_cone(&changed, &index, &graph);

        assert!(dirty.contains(&NodeId::Value(ValueCellId::new(e, "volume"))));
        assert!(dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 0))));
        assert!(dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))));
        assert!(dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 2))));
        assert!(
            dirty.contains(&NodeId::Realization(reify_core::RealizationNodeId::new(
                e, 0
            )))
        );
        assert!(!dirty.contains(&NodeId::Value(ValueCellId::new(e, "fillet_radius"))));
        assert_eq!(dirty.len(), 5);
    }

    #[test]
    fn dirty_cone_bracket_change_fillet_radius() {
        // Change fillet_radius → empty dirty cone (nothing reads fillet_radius)
        use crate::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let index = ReverseDependencyIndex::build_from_graph(&graph);

        let e = "Bracket";
        let mut changed = HashSet::new();
        changed.insert(ValueCellId::new(e, "fillet_radius"));

        let dirty = compute_dirty_cone(&changed, &index, &graph);

        assert!(dirty.is_empty(), "fillet_radius dirty cone: {:?}", dirty);
    }

    /// P3.3 step-7: edge #6 → edge #12 composition inside compute_dirty_cone.
    ///
    /// Topology: VC `a`, Compute `C` (value_inputs=[a], output_value_cells=[b]),
    /// VC `b`. The reverse index registers a → Compute(C) (from step-4).
    /// `compute_dirty_cone(&{a}, &idx, &graph)` must return a dirty set
    /// containing BOTH `NodeId::Compute(C)` and `NodeId::Value(b)`: the first
    /// from edge #6 (a → C), the second from edge #12 (C → b).
    ///
    /// Fails today because compute_dirty_cone does not yet take a graph
    /// parameter and does not propagate from a Compute dependent.
    #[test]
    fn compute_dirty_cone_propagates_through_compute_node_to_output_value_cells() {
        use crate::graph::{ComputeNodeData, EvaluationGraph, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ComputeNodeId, ContentHash, Type};

        let mut graph = EvaluationGraph::default();
        let e = "E";

        // Params a and b (Param kind — default_expr=None irrelevant here).
        for name in &["a", "b"] {
            let id = ValueCellId::new(e, *name);
            graph.value_cells.insert(
                id.clone(),
                ValueCellNode {
                    id: id.clone(),
                    kind: ValueCellKind::Param,
                    cell_type: Type::Real,
                    default_expr: None,
                    content_hash: ContentHash::of_str(name),
                },
            );
        }
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");

        // Compute C with value_inputs=[a], output_value_cells=[b].
        let c_id = ComputeNodeId::new(e, 0);
        graph.insert_compute_node(ComputeNodeData {
            computation_id: c_id.clone(),
            target: "fea".to_string(),
            value_inputs: vec![a.clone()],
            realization_inputs: vec![],
            options_hash: ContentHash::of_str("opt"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![b.clone()],
        });

        let index = ReverseDependencyIndex::build_from_graph(&graph);

        let mut changed = HashSet::new();
        changed.insert(a.clone());

        let dirty = compute_dirty_cone(&changed, &index, &graph);

        assert!(
            dirty.contains(&NodeId::Compute(c_id.clone())),
            "dirty cone should include Compute(C) via edge #6, got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Value(b.clone())),
            "dirty cone should include Value(b) via edge #12 (C's output_value_cells), got: {:?}",
            dirty
        );
    }

    /// P3.3 step-9: multi-hop Value → Compute → Value → Constraint propagation.
    ///
    /// Topology: VC `a`; Compute `C` (value_inputs=[a], output_value_cells=[b]);
    /// VC `b`; Constraint `C0` whose `expr` reads `b`. Reverse-index entries:
    /// a → Compute(C) (edge #6 from step-4); b → Constraint(C0) (constraint
    /// extracted in the standard build, added here directly to keep the
    /// fixture pure-synthetic).
    ///
    /// `compute_dirty_cone(&{a}, &idx, &graph)` must mark ALL of
    /// `Compute(C)`, `Value(b)`, and `Constraint(C0)` dirty — pins the
    /// architecture-doc claim (§5 line 199) that ComputeNode → ConstraintNode
    /// routes through the intermediate ValueCell `b` rather than via any
    /// direct ComputeNode→Constraint edge.
    #[test]
    fn compute_dirty_cone_multi_hop_value_through_compute_to_constraint() {
        use crate::graph::{ComputeNodeData, EvaluationGraph, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ComputeNodeId, ContentHash, Type};

        let mut graph = EvaluationGraph::default();
        let e = "E";

        // Params a and b — b has no default_expr, so its only invalidation
        // source is the ComputeNode that writes it.
        for name in &["a", "b"] {
            let id = ValueCellId::new(e, *name);
            graph.value_cells.insert(
                id.clone(),
                ValueCellNode {
                    id: id.clone(),
                    kind: ValueCellKind::Param,
                    cell_type: Type::Real,
                    default_expr: None,
                    content_hash: ContentHash::of_str(name),
                },
            );
        }
        let a = ValueCellId::new(e, "a");
        let b = ValueCellId::new(e, "b");

        // Compute C: value_inputs=[a], output_value_cells=[b].
        let c_id = ComputeNodeId::new(e, 0);
        graph.insert_compute_node(ComputeNodeData {
            computation_id: c_id.clone(),
            target: "fea".to_string(),
            value_inputs: vec![a.clone()],
            realization_inputs: vec![],
            options_hash: ContentHash::of_str("opt"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![b.clone()],
        });

        // Build reverse index over the graph (registers a → Compute(C)),
        // then manually splice in b → Constraint(C0). Manual `add` is the
        // same primitive `build_from_graph_and_fields` uses internally
        // (see deps.rs:147,157,166); this keeps the fixture pure-synthetic
        // without forcing us to construct a CompiledExpr that reads `b`.
        let mut index = ReverseDependencyIndex::build_from_graph(&graph);
        let c0_id = ConstraintNodeId::new(e, 0);
        index.add(b.clone(), NodeId::Constraint(c0_id.clone()));

        let mut changed = HashSet::new();
        changed.insert(a.clone());

        let dirty = compute_dirty_cone(&changed, &index, &graph);

        assert!(
            dirty.contains(&NodeId::Compute(c_id.clone())),
            "dirty cone should include Compute(C) via edge #6, got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Value(b.clone())),
            "dirty cone should include Value(b) via edge #12, got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Constraint(c0_id.clone())),
            "dirty cone should include Constraint(C0) via b's dependents (edge #6 → #12 → constraint), got: {:?}",
            dirty
        );
    }

    /// P3.3 step-11: Realization-seeded propagation through edge #10 → edge #12.
    ///
    /// Topology: Realization `R0`; Compute `C` (realization_inputs=[R0],
    /// output_value_cells=[b]); VC `b`. Reverse-index entries: R0 → Compute(C)
    /// (edge #10 from step-6) lives in the `realization_index` map.
    ///
    /// `compute_dirty_cone_with_realizations(&{}, &{R0}, &idx, &graph)` must
    /// mark BOTH `Compute(C)` (consumes R0 via edge #10) and `Value(b)`
    /// (output of C via edge #12) dirty — pins task-spec test 2
    /// ("Recompute a RealizationNode whose new content-hash differs →
    /// consuming ComputeNodes become dirty") and shows that the
    /// edge-#10 → edge-#12 composition mirrors edge-#6 → edge-#12.
    ///
    /// Fails because `compute_dirty_cone_with_realizations` does not yet
    /// exist.
    #[test]
    fn compute_dirty_cone_with_realizations_marks_consuming_compute_nodes_and_their_outputs() {
        use crate::dirty::compute_dirty_cone_with_realizations;
        use crate::graph::{ComputeNodeData, EvaluationGraph, RealizationNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ComputeNodeId, ContentHash, RealizationNodeId, Type};
        use reify_ir::ReprKind;

        let mut graph = EvaluationGraph::default();
        let e = "E";

        // VC b — output of the compute node.
        let b = ValueCellId::new(e, "b");
        graph.value_cells.insert(
            b.clone(),
            ValueCellNode {
                id: b.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("b"),
            },
        );

        // Realization R0.
        let r0_id = RealizationNodeId::new(e, 0);
        graph.realizations.insert(
            r0_id.clone(),
            RealizationNodeData {
                geometry_cell: None,
                id: r0_id.clone(),
                operations: vec![],
                content_hash: ContentHash::of_str("r0"),
                produced_repr: ReprKind::BRep,
                produced_kernel: None,
            },
        );

        // Compute C: realization_inputs=[R0], output_value_cells=[b].
        let c_id = ComputeNodeId::new(e, 0);
        graph.insert_compute_node(ComputeNodeData {
            computation_id: c_id.clone(),
            target: "fea".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![r0_id.clone()],
            options_hash: ContentHash::of_str("opt"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![b.clone()],
        });

        let index = ReverseDependencyIndex::build_from_graph(&graph);

        let mut changed_realizations = HashSet::new();
        changed_realizations.insert(r0_id.clone());
        let changed_vcs: HashSet<ValueCellId> = HashSet::new();

        let dirty = compute_dirty_cone_with_realizations(
            &changed_vcs,
            &changed_realizations,
            &index,
            &graph,
        );

        assert!(
            dirty.contains(&NodeId::Compute(c_id.clone())),
            "dirty cone should include Compute(C) via edge #10 (R0 → C), got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Value(b.clone())),
            "dirty cone should include Value(b) via edge #12 (C → b) composed with edge #10, got: {:?}",
            dirty
        );
    }

    /// P3.3 step-13: seed-discrimination negative case for the
    /// Realization-recompute-with-same-hash early cutoff (task-spec test 3).
    ///
    /// Same topology as step-11 (R0 → Compute(C) → Value(b)): the graph
    /// and reverse index still encode edges #10 and #12. The difference is
    /// at the seed boundary: the caller did NOT add R0 to
    /// `changed_realizations`, modelling the contract where the eval
    /// pipeline compares the new content-hash of R0 against its cached
    /// hash and withholds R0 from the dirty-seed set when they match.
    ///
    /// `compute_dirty_cone_with_realizations(&{}, &{}, &idx, &graph)` must
    /// therefore return an empty dirty set — the walk is conservative and
    /// faithfully propagates whatever (nothing) the caller seeded. This
    /// locks the seed-discrimination contract in place against any future
    /// regression where the walk might start unconditionally inserting
    /// from the reverse-index maps regardless of the seed input.
    #[test]
    fn compute_dirty_cone_with_realizations_negative_case_does_not_propagate_with_empty_seed() {
        use crate::dirty::compute_dirty_cone_with_realizations;
        use crate::graph::{ComputeNodeData, EvaluationGraph, RealizationNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ComputeNodeId, ContentHash, RealizationNodeId, Type};
        use reify_ir::ReprKind;

        let mut graph = EvaluationGraph::default();
        let e = "E";

        // VC b — output of the compute node.
        let b = ValueCellId::new(e, "b");
        graph.value_cells.insert(
            b.clone(),
            ValueCellNode {
                id: b.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("b"),
            },
        );

        // Realization R0 — present in graph, but caller will NOT seed it.
        let r0_id = RealizationNodeId::new(e, 0);
        graph.realizations.insert(
            r0_id.clone(),
            RealizationNodeData {
                geometry_cell: None,
                id: r0_id.clone(),
                operations: vec![],
                content_hash: ContentHash::of_str("r0"),
                produced_repr: ReprKind::BRep,
                produced_kernel: None,
            },
        );

        // Compute C: realization_inputs=[R0], output_value_cells=[b].
        let c_id = ComputeNodeId::new(e, 0);
        graph.insert_compute_node(ComputeNodeData {
            computation_id: c_id.clone(),
            target: "fea".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![r0_id.clone()],
            options_hash: ContentHash::of_str("opt"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![b.clone()],
        });

        let index = ReverseDependencyIndex::build_from_graph(&graph);

        // Empty seeds on BOTH inputs — models the "Realization recomputed
        // with same content hash" cutoff at the caller boundary.
        let changed_vcs: HashSet<ValueCellId> = HashSet::new();
        let changed_realizations: HashSet<RealizationNodeId> = HashSet::new();

        let dirty = compute_dirty_cone_with_realizations(
            &changed_vcs,
            &changed_realizations,
            &index,
            &graph,
        );

        assert!(
            dirty.is_empty(),
            "empty seeds must yield empty dirty cone (seed-discrimination contract); got: {:?}",
            dirty
        );
    }

    /// Amendment (Sugg 3b): transitive Realization → Compute → output VC →
    /// downstream Constraint propagation.
    ///
    /// Closes the loop on `compute_dirty_cone_with_realizations` by extending
    /// the topology from step-11 with a Constraint that reads the
    /// ComputeNode's output VC. Verifies that once the output VC is seeded
    /// onto the BFS frontier (edge #10 → edge #12), the standard
    /// `dependents_of(out_vc)` propagation kicks in and pulls the Constraint
    /// into the dirty cone — exactly the same way `compute_dirty_cone`'s
    /// multi-hop test (step-9) does for a VC-seeded change.
    ///
    /// Topology: Realization `R0`; Compute `C` (realization_inputs=[R0],
    /// output_value_cells=[b]); VC `b`; Constraint `C0` reads `b`.
    /// Reverse-index entries: R0 → Compute(C) (edge #10); b → Constraint(C0)
    /// (manually added like step-9 does).
    #[test]
    fn compute_dirty_cone_with_realizations_propagates_transitively_to_constraint_reading_output_vc()
     {
        use crate::dirty::compute_dirty_cone_with_realizations;
        use crate::graph::{ComputeNodeData, EvaluationGraph, RealizationNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ComputeNodeId, ContentHash, RealizationNodeId, Type};
        use reify_ir::ReprKind;

        let mut graph = EvaluationGraph::default();
        let e = "E";

        // VC b — output of the compute node and read by the constraint.
        let b = ValueCellId::new(e, "b");
        graph.value_cells.insert(
            b.clone(),
            ValueCellNode {
                id: b.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("b"),
            },
        );

        // Realization R0.
        let r0_id = RealizationNodeId::new(e, 0);
        graph.realizations.insert(
            r0_id.clone(),
            RealizationNodeData {
                geometry_cell: None,
                id: r0_id.clone(),
                operations: vec![],
                content_hash: ContentHash::of_str("r0"),
                produced_repr: ReprKind::BRep,
                produced_kernel: None,
            },
        );

        // Compute C: realization_inputs=[R0], output_value_cells=[b].
        let c_id = ComputeNodeId::new(e, 0);
        graph.insert_compute_node(ComputeNodeData {
            computation_id: c_id.clone(),
            target: "fea".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![r0_id.clone()],
            options_hash: ContentHash::of_str("opt"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![b.clone()],
        });

        // Reverse index: build_from_graph picks up R0 → Compute(C). Splice
        // b → Constraint(C0) manually (pattern mirrors step-9 — keeps the
        // fixture pure-synthetic without forcing a CompiledExpr that
        // reads `b`).
        let mut index = ReverseDependencyIndex::build_from_graph(&graph);
        let c0_id = ConstraintNodeId::new(e, 0);
        index.add(b.clone(), NodeId::Constraint(c0_id.clone()));

        let mut changed_realizations = HashSet::new();
        changed_realizations.insert(r0_id.clone());
        let changed_vcs: HashSet<ValueCellId> = HashSet::new();

        let dirty = compute_dirty_cone_with_realizations(
            &changed_vcs,
            &changed_realizations,
            &index,
            &graph,
        );

        // All three must be dirty: edge #10 (R0 → C), edge #12 (C → b),
        // and the standard edge #1 (b → C0) picked up by the BFS over
        // value-cell dependents.
        assert!(
            dirty.contains(&NodeId::Compute(c_id.clone())),
            "dirty cone should include Compute(C) via edge #10, got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Value(b.clone())),
            "dirty cone should include Value(b) via edge #12, got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Constraint(c0_id.clone())),
            "dirty cone should include Constraint(C0) via b's dependents (edge #10 → #12 → constraint), got: {:?}",
            dirty
        );
    }

    #[test]
    fn dirty_cone_includes_resolution_node() {
        use crate::graph::{EvaluationGraph, ResolutionNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ContentHash, ResolutionNodeId, Type};

        let mut graph = EvaluationGraph::default();

        // Param 'a'
        let a = ValueCellId::new("A", "a");
        graph.value_cells.insert(
            a.clone(),
            ValueCellNode {
                id: a.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("a"),
            },
        );

        // Resolution R0 with auto_params=['a']
        let r0_id = ResolutionNodeId::new("A", 0);
        graph.resolutions.insert(
            r0_id.clone(),
            ResolutionNodeData {
                id: r0_id.clone(),
                scope: "A".to_string(),
                auto_params: vec![a.clone()],
                constraint_deps: vec![],
                content_hash: ContentHash::of_str("r0"),
            },
        );

        let index = ReverseDependencyIndex::build_from_graph(&graph);

        let mut changed = HashSet::new();
        changed.insert(a.clone());
        let dirty = compute_dirty_cone(&changed, &index, &graph);

        assert!(
            dirty.contains(&NodeId::Resolution(r0_id)),
            "dirty cone should include Resolution(R0) when 'a' changes, got: {:?}",
            dirty
        );
    }

    #[test]
    fn topo_sort_empty_set() {
        use crate::deps::DependencyTrace;
        use crate::dirty::topological_sort;
        use std::collections::HashMap;

        let nodes: HashSet<NodeId> = HashSet::new();
        let traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        let sorted = topological_sort(&nodes, &traces);
        assert!(sorted.is_empty());
    }

    #[test]
    fn topo_sort_single_node() {
        use crate::deps::DependencyTrace;
        use crate::dirty::topological_sort;
        use std::collections::HashMap;

        let node = NodeId::Value(ValueCellId::new("A", "x"));
        let mut nodes = HashSet::new();
        nodes.insert(node.clone());
        let mut traces = HashMap::new();
        traces.insert(node.clone(), DependencyTrace::default());
        let sorted = topological_sort(&nodes, &traces);
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0], node);
    }

    #[test]
    fn topo_sort_independent_nodes() {
        use crate::deps::DependencyTrace;
        use crate::dirty::topological_sort;
        use std::collections::HashMap;

        let a = NodeId::Value(ValueCellId::new("A", "a"));
        let b = NodeId::Value(ValueCellId::new("A", "b"));
        let c = NodeId::Constraint(ConstraintNodeId::new("A", 0));
        let mut nodes = HashSet::new();
        nodes.insert(a.clone());
        nodes.insert(b.clone());
        nodes.insert(c.clone());
        let mut traces = HashMap::new();
        traces.insert(a.clone(), DependencyTrace::default());
        traces.insert(b.clone(), DependencyTrace::default());
        traces.insert(c.clone(), DependencyTrace::default());
        let sorted = topological_sort(&nodes, &traces);
        assert_eq!(sorted.len(), 3);
        // All three present (any order for independent nodes)
        assert!(sorted.contains(&a));
        assert!(sorted.contains(&b));
        assert!(sorted.contains(&c));
    }

    #[test]
    fn topo_sort_volume_after_params() {
        use crate::deps::DependencyTrace;
        use crate::dirty::topological_sort;
        use std::collections::HashMap;

        let e = "B";
        let width = NodeId::Value(ValueCellId::new(e, "width"));
        let height = NodeId::Value(ValueCellId::new(e, "height"));
        let thickness = NodeId::Value(ValueCellId::new(e, "thickness"));
        let volume = NodeId::Value(ValueCellId::new(e, "volume"));

        let mut nodes = HashSet::new();
        nodes.insert(width.clone());
        nodes.insert(height.clone());
        nodes.insert(thickness.clone());
        nodes.insert(volume.clone());

        let mut traces = HashMap::new();
        traces.insert(width.clone(), DependencyTrace::default());
        traces.insert(height.clone(), DependencyTrace::default());
        traces.insert(thickness.clone(), DependencyTrace::default());
        traces.insert(
            volume.clone(),
            DependencyTrace {
                realization_reads: Vec::new(),
                reads: vec![
                    ValueCellId::new(e, "width"),
                    ValueCellId::new(e, "height"),
                    ValueCellId::new(e, "thickness"),
                ],
            },
        );

        let sorted = topological_sort(&nodes, &traces);
        assert_eq!(sorted.len(), 4);
        // volume should appear after all three params
        let vol_pos = sorted.iter().position(|n| n == &volume).unwrap();
        let w_pos = sorted.iter().position(|n| n == &width).unwrap();
        let h_pos = sorted.iter().position(|n| n == &height).unwrap();
        let t_pos = sorted.iter().position(|n| n == &thickness).unwrap();
        assert!(vol_pos > w_pos, "volume should appear after width");
        assert!(vol_pos > h_pos, "volume should appear after height");
        assert!(vol_pos > t_pos, "volume should appear after thickness");
    }

    #[test]
    fn topo_sort_constraint_after_deps() {
        // C1 depends on width and thickness, both in set → C1 after both
        use crate::deps::DependencyTrace;
        use crate::dirty::topological_sort;
        use std::collections::HashMap;

        let e = "B";
        let width = NodeId::Value(ValueCellId::new(e, "width"));
        let thickness = NodeId::Value(ValueCellId::new(e, "thickness"));
        let c1 = NodeId::Constraint(ConstraintNodeId::new(e, 1));

        let mut nodes = HashSet::new();
        nodes.insert(width.clone());
        nodes.insert(thickness.clone());
        nodes.insert(c1.clone());

        let mut traces = HashMap::new();
        traces.insert(width.clone(), DependencyTrace::default());
        traces.insert(thickness.clone(), DependencyTrace::default());
        traces.insert(
            c1.clone(),
            DependencyTrace {
                realization_reads: Vec::new(),
                reads: vec![
                    ValueCellId::new(e, "width"),
                    ValueCellId::new(e, "thickness"),
                ],
            },
        );

        let sorted = topological_sort(&nodes, &traces);
        assert_eq!(sorted.len(), 3);
        let c1_pos = sorted.iter().position(|n| n == &c1).unwrap();
        let w_pos = sorted.iter().position(|n| n == &width).unwrap();
        let t_pos = sorted.iter().position(|n| n == &thickness).unwrap();
        assert!(c1_pos > w_pos, "C1 should appear after width");
        assert!(c1_pos > t_pos, "C1 should appear after thickness");
    }

    #[test]
    fn topo_sort_mixed_set() {
        // {volume, C1, width, thickness}: width+thickness first,
        // then volume and C1 (both depend on width/thickness)
        use crate::deps::DependencyTrace;
        use crate::dirty::topological_sort;
        use std::collections::HashMap;

        let e = "B";
        let width = NodeId::Value(ValueCellId::new(e, "width"));
        let thickness = NodeId::Value(ValueCellId::new(e, "thickness"));
        let volume = NodeId::Value(ValueCellId::new(e, "volume"));
        let c1 = NodeId::Constraint(ConstraintNodeId::new(e, 1));

        let mut nodes = HashSet::new();
        nodes.insert(width.clone());
        nodes.insert(thickness.clone());
        nodes.insert(volume.clone());
        nodes.insert(c1.clone());

        let mut traces = HashMap::new();
        traces.insert(width.clone(), DependencyTrace::default());
        traces.insert(thickness.clone(), DependencyTrace::default());
        traces.insert(
            volume.clone(),
            DependencyTrace {
                realization_reads: Vec::new(),
                reads: vec![
                    ValueCellId::new(e, "width"),
                    ValueCellId::new(e, "thickness"),
                ],
            },
        );
        traces.insert(
            c1.clone(),
            DependencyTrace {
                realization_reads: Vec::new(),
                reads: vec![
                    ValueCellId::new(e, "width"),
                    ValueCellId::new(e, "thickness"),
                ],
            },
        );

        let sorted = topological_sort(&nodes, &traces);
        assert_eq!(sorted.len(), 4);
        // width and thickness before volume and C1
        let w_pos = sorted.iter().position(|n| n == &width).unwrap();
        let t_pos = sorted.iter().position(|n| n == &thickness).unwrap();
        let vol_pos = sorted.iter().position(|n| n == &volume).unwrap();
        let c1_pos = sorted.iter().position(|n| n == &c1).unwrap();
        assert!(vol_pos > w_pos);
        assert!(vol_pos > t_pos);
        assert!(c1_pos > w_pos);
        assert!(c1_pos > t_pos);
    }

    #[test]
    fn topo_sort_duplicate_reads() {
        // Exposes the duplicate-reads bug: when trace.reads = [a, a] (e.g. `a * a`),
        // in-degree is over-counted to 2 but only decremented once via .contains(),
        // causing 'sq' to be silently dropped from the sorted output.
        use crate::deps::DependencyTrace;
        use crate::dirty::topological_sort;
        use std::collections::HashMap;

        let e = "D";
        let a = NodeId::Value(ValueCellId::new(e, "a"));
        let sq = NodeId::Value(ValueCellId::new(e, "sq"));

        let mut nodes = HashSet::new();
        nodes.insert(a.clone());
        nodes.insert(sq.clone());

        let mut traces = HashMap::new();
        traces.insert(a.clone(), DependencyTrace::default());
        // sq reads 'a' twice (simulating expression `a * a`)
        traces.insert(
            sq.clone(),
            DependencyTrace {
                realization_reads: Vec::new(),
                reads: vec![ValueCellId::new(e, "a"), ValueCellId::new(e, "a")],
            },
        );

        let sorted = topological_sort(&nodes, &traces);
        assert_eq!(
            sorted.len(),
            2,
            "both nodes must appear in sorted output, got: {:?}",
            sorted
        );
        // sq must appear after a
        let a_pos = sorted.iter().position(|n| n == &a).unwrap();
        let sq_pos = sorted.iter().position(|n| n == &sq).unwrap();
        assert!(sq_pos > a_pos, "sq should appear after a");
    }

    #[test]
    fn eval_set_empty_dirty() {
        use crate::demand::DemandRegistry;
        use crate::deps::DependencyTrace;
        use crate::dirty::compute_eval_set;
        use std::collections::HashMap;

        let dirty: HashSet<NodeId> = HashSet::new();
        let demand = DemandRegistry::new();
        let traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        let eval_set = compute_eval_set(&dirty, &demand, &traces);
        assert!(eval_set.is_empty());
    }

    #[test]
    fn eval_set_dirty_outside_demand_cone() {
        use crate::demand::DemandRegistry;
        use crate::deps::DependencyTrace;
        use crate::dirty::compute_eval_set;
        use std::collections::HashMap;

        // volume is dirty but not demanded
        let volume = NodeId::Value(ValueCellId::new("B", "volume"));
        let mut dirty = HashSet::new();
        dirty.insert(volume.clone());

        let demand = DemandRegistry::new(); // empty demand cone
        let mut traces = HashMap::new();
        traces.insert(volume.clone(), DependencyTrace::default());

        let eval_set = compute_eval_set(&dirty, &demand, &traces);
        assert!(eval_set.is_empty());
    }

    #[test]
    fn eval_set_dirty_inside_demand_cone() {
        use crate::demand::DemandRegistry;
        use crate::dirty::compute_eval_set;
        use crate::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);

        let e = "Bracket";
        let c0 = NodeId::Constraint(ConstraintNodeId::new(e, 0));
        let c1 = NodeId::Constraint(ConstraintNodeId::new(e, 1));

        // Demand C0 and C1
        let mut demand = DemandRegistry::new();
        demand.add_demand(c0.clone());
        demand.add_demand(c1.clone());
        demand.rebuild_cone(&graph);

        // Both C0 and C1 are dirty and demanded
        let mut dirty = HashSet::new();
        dirty.insert(c0.clone());
        dirty.insert(c1.clone());

        let traces = crate::deps::build_trace_map(&graph);

        let eval_set = compute_eval_set(&dirty, &demand, &traces);
        assert_eq!(eval_set.len(), 2);
        assert!(eval_set.contains(&c0));
        assert!(eval_set.contains(&c1));
    }

    #[test]
    fn eval_set_bracket_change_width_demand_all_constraints() {
        // Exit criteria: demand all constraints, change width → eval_set = {C1}
        // volume is dirty but not demanded (no constraint reads volume)
        use crate::demand::DemandRegistry;
        use crate::dirty::compute_eval_set;
        use crate::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let index = ReverseDependencyIndex::build_from_graph(&graph);
        let traces = crate::deps::build_trace_map(&graph);

        let e = "Bracket";
        let c1 = NodeId::Constraint(ConstraintNodeId::new(e, 1));

        // Demand all constraints
        let mut demand = DemandRegistry::new();
        demand.add_demand(NodeId::Constraint(ConstraintNodeId::new(e, 0)));
        demand.add_demand(NodeId::Constraint(ConstraintNodeId::new(e, 1)));
        demand.add_demand(NodeId::Constraint(ConstraintNodeId::new(e, 2)));
        demand.rebuild_cone(&graph);

        // Change width
        let mut changed = HashSet::new();
        changed.insert(ValueCellId::new(e, "width"));
        let dirty = compute_dirty_cone(&changed, &index, &graph);

        let eval_set = compute_eval_set(&dirty, &demand, &traces);
        assert_eq!(eval_set.len(), 1, "eval_set: {:?}", eval_set);
        assert_eq!(eval_set[0], c1);
    }

    #[test]
    fn eval_set_bracket_change_thickness_demand_all_constraints() {
        // Change thickness → eval_set = {C0, C1, C2} (all read thickness)
        use crate::demand::DemandRegistry;
        use crate::dirty::compute_eval_set;
        use crate::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let index = ReverseDependencyIndex::build_from_graph(&graph);
        let traces = crate::deps::build_trace_map(&graph);

        let e = "Bracket";

        // Demand all constraints
        let mut demand = DemandRegistry::new();
        demand.add_demand(NodeId::Constraint(ConstraintNodeId::new(e, 0)));
        demand.add_demand(NodeId::Constraint(ConstraintNodeId::new(e, 1)));
        demand.add_demand(NodeId::Constraint(ConstraintNodeId::new(e, 2)));
        demand.rebuild_cone(&graph);

        // Change thickness
        let mut changed = HashSet::new();
        changed.insert(ValueCellId::new(e, "thickness"));
        let dirty = compute_dirty_cone(&changed, &index, &graph);

        let eval_set = compute_eval_set(&dirty, &demand, &traces);
        // volume is dirty but not demanded → excluded
        // C0, C1, C2 are dirty and demanded → included
        assert_eq!(eval_set.len(), 3, "eval_set: {:?}", eval_set);
        assert!(eval_set.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 0))));
        assert!(eval_set.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))));
        assert!(eval_set.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 2))));
    }

    #[test]
    fn eval_set_demand_subset_excludes_realization() {
        // Build graph with bracket params + realization that reads width.
        // Demand only constraints (not realization).
        // Change width → dirty cone includes {volume, C1, Realization(0)}.
        // Eval set should NOT include Realization(0).
        use crate::demand::DemandRegistry;
        use crate::dirty::compute_eval_set;
        use crate::graph::EvaluationGraph;
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
        use reify_core::{RealizationNodeId, Type};
        use reify_ir::{BinOp, CompiledExpr, Value};
        use reify_test_support::TopologyTemplateBuilder;

        let e = "B";
        let width_ref = || CompiledExpr::value_ref(ValueCellId::new(e, "width"), Type::length());
        let thickness_ref =
            || CompiledExpr::value_ref(ValueCellId::new(e, "thickness"), Type::length());
        let mm = |v: f64| CompiledExpr::literal(Value::length(v * 0.001), Type::length());

        // constraint: thickness < width / 4
        let c1_expr = CompiledExpr::binop(
            BinOp::Lt,
            thickness_ref(),
            CompiledExpr::binop(
                BinOp::Div,
                width_ref(),
                CompiledExpr::literal(Value::Int(4), Type::Int),
                Type::length(),
            ),
            Type::Bool,
        );

        // Realization with a Box primitive that reads width
        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".to_string(), width_ref()),
                ("height".to_string(), mm(100.0)),
                ("depth".to_string(), mm(5.0)),
            ],
        }];

        let template = TopologyTemplateBuilder::new(e)
            .param(e, "width", Type::length(), Some(mm(80.0)))
            .param(e, "thickness", Type::length(), Some(mm(5.0)))
            .constraint(e, 1, None, c1_expr)
            .realization(e, 0, ops)
            .build();

        let graph = EvaluationGraph::from_templates(&[template]);
        let index = ReverseDependencyIndex::build_from_graph(&graph);
        let traces = crate::deps::build_trace_map(&graph);

        // Demand only constraints (not realization)
        let c1 = NodeId::Constraint(ConstraintNodeId::new(e, 1));
        let mut demand = DemandRegistry::new();
        demand.add_demand(c1.clone());
        demand.rebuild_cone(&graph);

        // Change width
        let mut changed = HashSet::new();
        changed.insert(ValueCellId::new(e, "width"));
        let dirty = compute_dirty_cone(&changed, &index, &graph);

        // Dirty should include C1 and Realization(0)
        assert!(dirty.contains(&c1));
        assert!(dirty.contains(&NodeId::Realization(RealizationNodeId::new(e, 0))));

        // Eval set should include only C1 (realization not demanded)
        let eval_set = compute_eval_set(&dirty, &demand, &traces);
        assert_eq!(eval_set.len(), 1, "eval_set: {:?}", eval_set);
        assert_eq!(eval_set[0], c1);
        assert!(
            !eval_set.contains(&NodeId::Realization(RealizationNodeId::new(e, 0))),
            "realization should not be in eval_set"
        );
    }

    // --- compute_levels tests ---

    #[test]
    fn compute_levels_empty_input() {
        use crate::deps::DependencyTrace;
        use crate::dirty::compute_levels;
        use std::collections::HashMap;

        let nodes: HashSet<NodeId> = HashSet::new();
        let traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        let levels = compute_levels(&nodes, &traces);
        assert!(levels.is_empty());
    }

    #[test]
    fn compute_levels_fan_out() {
        // a -> b, a -> c => levels: [[a], [b, c]]
        use crate::deps::DependencyTrace;
        use crate::dirty::compute_levels;
        use std::collections::HashMap;

        let a = NodeId::Value(ValueCellId::new("X", "a"));
        let b = NodeId::Value(ValueCellId::new("X", "b"));
        let c = NodeId::Value(ValueCellId::new("X", "c"));

        let mut nodes = HashSet::new();
        nodes.insert(a.clone());
        nodes.insert(b.clone());
        nodes.insert(c.clone());

        let mut traces = HashMap::new();
        traces.insert(a.clone(), DependencyTrace::default());
        traces.insert(
            b.clone(),
            DependencyTrace {
                realization_reads: Vec::new(),
                reads: vec![ValueCellId::new("X", "a")],
            },
        );
        traces.insert(
            c.clone(),
            DependencyTrace {
                realization_reads: Vec::new(),
                reads: vec![ValueCellId::new("X", "a")],
            },
        );

        let levels = compute_levels(&nodes, &traces);
        assert_eq!(levels.len(), 2, "expected 2 levels, got {:?}", levels);
        assert_eq!(levels[0], vec![a.clone()]);
        // b and c should both be in level 1 (order determined by DebugOrd)
        assert_eq!(levels[1].len(), 2);
        assert!(levels[1].contains(&b));
        assert!(levels[1].contains(&c));
    }

    #[test]
    fn compute_levels_chain() {
        // a -> b -> c => levels: [[a], [b], [c]]
        use crate::deps::DependencyTrace;
        use crate::dirty::compute_levels;
        use std::collections::HashMap;

        let a = NodeId::Value(ValueCellId::new("X", "a"));
        let b = NodeId::Value(ValueCellId::new("X", "b"));
        let c = NodeId::Value(ValueCellId::new("X", "c"));

        let mut nodes = HashSet::new();
        nodes.insert(a.clone());
        nodes.insert(b.clone());
        nodes.insert(c.clone());

        let mut traces = HashMap::new();
        traces.insert(a.clone(), DependencyTrace::default());
        traces.insert(
            b.clone(),
            DependencyTrace {
                realization_reads: Vec::new(),
                reads: vec![ValueCellId::new("X", "a")],
            },
        );
        traces.insert(
            c.clone(),
            DependencyTrace {
                realization_reads: Vec::new(),
                reads: vec![ValueCellId::new("X", "b")],
            },
        );

        let levels = compute_levels(&nodes, &traces);
        assert_eq!(levels.len(), 3, "expected 3 levels, got {:?}", levels);
        assert_eq!(levels[0], vec![a]);
        assert_eq!(levels[1], vec![b]);
        assert_eq!(levels[2], vec![c]);
    }

    // --- check_dag_complete positive tests (step-1) ---

    /// (a) realization→realization: producer P before consumer C in exec_order → Ok(())
    #[test]
    fn check_dag_complete_realization_to_realization_ordered_ok() {
        use crate::deps::DependencyTrace;
        use crate::dirty::check_dag_complete;
        use reify_core::RealizationNodeId;
        use std::collections::HashMap;

        let e = "E";
        let p_id = RealizationNodeId::new(e, 0); // producer
        let c_id = RealizationNodeId::new(e, 1); // consumer

        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        // Producer has no deps
        traces.insert(
            NodeId::Realization(p_id.clone()),
            DependencyTrace::default(),
        );
        // Consumer reads producer
        traces.insert(
            NodeId::Realization(c_id.clone()),
            DependencyTrace {
                realization_reads: vec![p_id.clone()],
                reads: vec![],
            },
        );

        // exec_order: producer first, consumer second
        let exec_order = vec![p_id, c_id];

        let result = check_dag_complete(&traces, &exec_order);
        assert!(result.is_ok(), "expected Ok(()), got: {:?}", result);
    }

    /// (b) value→realization: value cell reads producer realization; producer present → Ok(())
    #[test]
    fn check_dag_complete_value_to_realization_producer_present_ok() {
        use crate::deps::DependencyTrace;
        use crate::dirty::check_dag_complete;
        use reify_core::RealizationNodeId;
        use std::collections::HashMap;

        let e = "E";
        let p_id = RealizationNodeId::new(e, 0); // producer realization
        let selector = ValueCellId::new(e, "sel");

        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        // Producer realization
        traces.insert(
            NodeId::Realization(p_id.clone()),
            DependencyTrace::default(),
        );
        // Value (selector) reads the producer realization
        traces.insert(
            NodeId::Value(selector.clone()),
            DependencyTrace {
                realization_reads: vec![p_id.clone()],
                reads: vec![],
            },
        );

        // exec_order contains the producer
        let exec_order = vec![p_id];

        let result = check_dag_complete(&traces, &exec_order);
        assert!(result.is_ok(), "expected Ok(()), got: {:?}", result);
    }

    /// (c) constraint→realization: constraint reads producer realization; producer present → Ok(())
    #[test]
    fn check_dag_complete_constraint_to_realization_producer_present_ok() {
        use crate::deps::DependencyTrace;
        use crate::dirty::check_dag_complete;
        use reify_core::RealizationNodeId;
        use std::collections::HashMap;

        let e = "E";
        let p_id = RealizationNodeId::new(e, 0); // producer realization
        let k = ConstraintNodeId::new(e, 0);

        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        // Producer realization
        traces.insert(
            NodeId::Realization(p_id.clone()),
            DependencyTrace::default(),
        );
        // Constraint reads the producer realization
        traces.insert(
            NodeId::Constraint(k.clone()),
            DependencyTrace {
                realization_reads: vec![p_id.clone()],
                reads: vec![],
            },
        );

        // exec_order contains the producer
        let exec_order = vec![p_id];

        let result = check_dag_complete(&traces, &exec_order);
        assert!(result.is_ok(), "expected Ok(()), got: {:?}", result);
    }

    // --- check_dag_complete negative-teeth tests (step-3) ---

    /// (a) backward realization→realization: producer after consumer → BackwardEdge
    #[test]
    fn check_dag_complete_backward_realization_to_realization_err() {
        use crate::deps::DependencyTrace;
        use crate::dirty::{DagViolation, check_dag_complete};
        use reify_core::RealizationNodeId;
        use std::collections::HashMap;

        let e = "E";
        let p_id = RealizationNodeId::new(e, 0); // producer
        let c_id = RealizationNodeId::new(e, 1); // consumer

        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        traces.insert(
            NodeId::Realization(p_id.clone()),
            DependencyTrace::default(),
        );
        traces.insert(
            NodeId::Realization(c_id.clone()),
            DependencyTrace {
                realization_reads: vec![p_id.clone()],
                reads: vec![],
            },
        );

        // REVERSED order: consumer at position 0, producer at position 1
        let exec_order = vec![c_id.clone(), p_id.clone()];

        let result = check_dag_complete(&traces, &exec_order);
        match result {
            Err(DagViolation::BackwardEdge {
                producer,
                consumer,
                ..
            }) => {
                assert_eq!(producer, p_id, "wrong producer in BackwardEdge");
                assert_eq!(consumer, c_id, "wrong consumer in BackwardEdge");
            }
            other => panic!(
                "expected Err(BackwardEdge {{ producer: {:?}, consumer: {:?} }}), got: {:?}",
                p_id, c_id, other
            ),
        }
    }

    /// (b) missing producer for Realization consumer → MissingProducer
    #[test]
    fn check_dag_complete_missing_producer_for_realization_consumer_err() {
        use crate::deps::DependencyTrace;
        use crate::dirty::{DagViolation, check_dag_complete};
        use reify_core::RealizationNodeId;
        use std::collections::HashMap;

        let e = "E";
        let p_id = RealizationNodeId::new(e, 0); // producer — NOT in exec_order
        let c_id = RealizationNodeId::new(e, 1); // consumer

        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        // P is absent from traces (no DependencyTrace inserted for it)
        traces.insert(
            NodeId::Realization(c_id.clone()),
            DependencyTrace {
                realization_reads: vec![p_id.clone()],
                reads: vec![],
            },
        );

        // exec_order only contains C, P is never built
        let exec_order = vec![c_id.clone()];

        let result = check_dag_complete(&traces, &exec_order);
        match result {
            Err(DagViolation::MissingProducer { producer, consumer }) => {
                assert_eq!(producer, p_id, "wrong producer in MissingProducer");
                assert_eq!(
                    consumer,
                    NodeId::Realization(c_id.clone()),
                    "wrong consumer in MissingProducer"
                );
            }
            other => panic!(
                "expected Err(MissingProducer {{ producer: {:?}, consumer: {:?} }}), got: {:?}",
                p_id,
                NodeId::Realization(c_id),
                other
            ),
        }
    }

    /// (c) missing producer for non-realization consumer (Constraint) → MissingProducer
    #[test]
    fn check_dag_complete_missing_producer_for_constraint_consumer_err() {
        use crate::deps::DependencyTrace;
        use crate::dirty::{DagViolation, check_dag_complete};
        use reify_core::RealizationNodeId;
        use std::collections::HashMap;

        let e = "E";
        let p_id = RealizationNodeId::new(e, 0); // producer — NOT in exec_order
        let k = ConstraintNodeId::new(e, 0);

        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        traces.insert(
            NodeId::Constraint(k.clone()),
            DependencyTrace {
                realization_reads: vec![p_id.clone()],
                reads: vec![],
            },
        );

        // exec_order is empty — P is never built
        let exec_order: Vec<RealizationNodeId> = vec![];

        let result = check_dag_complete(&traces, &exec_order);
        match result {
            Err(DagViolation::MissingProducer { producer, consumer }) => {
                assert_eq!(producer, p_id, "wrong producer in MissingProducer");
                assert_eq!(
                    consumer,
                    NodeId::Constraint(k.clone()),
                    "wrong consumer in MissingProducer"
                );
            }
            other => panic!(
                "expected Err(MissingProducer {{ producer: {:?}, consumer: Constraint({:?}) }}), got: {:?}",
                p_id, k, other
            ),
        }
    }

    // --- assert_dag_complete_from_graph wrapper tests (step-5) ---

    /// #[should_panic] wrapper test: minimal two-entity EvaluationGraph with
    /// cross-sub Boolean edge (inner_a → outer). Reversed exec_order
    /// (outer before inner_a) → BackwardEdge → wrapper must panic with
    /// a message containing "assert_dag_complete".
    ///
    /// RED until step-6 adds assert_dag_complete_from_graph.
    #[test]
    #[should_panic(expected = "assert_dag_complete")]
    fn assert_dag_complete_from_graph_panics_on_reversed_cross_sub_exec_order() {
        use crate::dirty::assert_dag_complete_from_graph;
        use crate::graph::{EvaluationGraph, RealizationNodeData};
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};
        use reify_core::{ContentHash, RealizationNodeId, ValueCellId};
        use reify_ir::ReprKind;

        let mut graph = EvaluationGraph::default();

        // inner_a: entity "A", geometry_cell = ValueCellId("A","body")
        let inner_a = RealizationNodeId::new("A", 0);
        let body_a = ValueCellId::new("A", "body");
        graph.realizations.insert(
            inner_a.clone(),
            RealizationNodeData {
                produced_kernel: None,
                id: inner_a.clone(),
                geometry_cell: Some(body_a.clone()),
                operations: vec![],
                content_hash: ContentHash::of_str("inner_a"),
                produced_repr: ReprKind::BRep,
            },
        );

        // outer: entity "Outer", Boolean { Union, Sub("a.body"), Sub("a.body") }
        // (using same ref twice so we only need one inner entity for simplicity)
        let outer = RealizationNodeId::new("Outer", 0);
        graph.realizations.insert(
            outer.clone(),
            RealizationNodeData {
                produced_kernel: None,
                id: outer.clone(),
                geometry_cell: None,
                operations: vec![CompiledGeometryOp::Boolean {
                    op: BooleanOp::Union,
                    left: GeomRef::Sub("a.body".into()),
                    right: GeomRef::Sub("a.body".into()),
                }],
                content_hash: ContentHash::of_str("outer"),
                produced_repr: ReprKind::BRep,
            },
        );

        // REVERSED exec_order: outer (consumer) before inner_a (producer)
        // → BackwardEdge → panic
        let exec_order = vec![outer.clone(), inner_a.clone()];

        // Should panic with message containing "assert_dag_complete"
        assert_dag_complete_from_graph(&graph, &[], &exec_order);
    }
}
