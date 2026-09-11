//! Dirty cone computation and evaluation set construction.
//!
//! When a parameter changes, the dirty cone is the set of all nodes that
//! transitively depend on the changed cells. The evaluation set is the
//! intersection of the dirty cone and the demand cone, topologically sorted
//! so that dependencies are evaluated before their dependents.

use std::collections::{HashMap, HashSet, VecDeque};

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
/// # The realization fan-out is TRANSITIVE (amend, review rounds 1 and 2)
///
/// The realization frontier and the ValueCell frontier are two MUTUALLY
/// FEEDING producers drained to a single fixpoint, not two staged passes: a
/// realization expands onto ValueCells, and a ValueCell can just as well
/// expand back onto a realization. Running them sequentially would leave the
/// walk half-transitive — a realization discovered late, by the ValueCell BFS,
/// would never get its own fan-out walked.
///
/// A `NodeId::Realization` dependent is therefore re-seeded onto the
/// realization frontier WHEREVER it is found:
///
/// - in the realization-keyed map (deps.rs:210-214) — a downstream
///   realization consuming this one as a `GeomRef::Sub` operand;
/// - in the ValueCell-keyed map (deps.rs:203-208) — a realization that READS
///   the cell, registered for every value-read in its op args. Both arms that
///   feed the ValueCell frontier from a realization seed can land on such a
///   cell: the edge-#12 expansion off a `NodeId::Compute` dependent, and the
///   GHR-δ S4 `NodeId::Value` arm.
///
/// `ReverseDependencyIndex`'s realization-keyed map carries FOUR dependent
/// kinds, not just ComputeNodes, and each is re-seeded onto the walk rather
/// than treated as a leaf:
///
/// - `NodeId::Compute` — edge #10 (deps.rs:239-241). Expanded through its
///   `output_value_cells` (edge #12) onto the ValueCell frontier.
/// - `NodeId::Realization` — registered by `extract_realization_edges` for
///   `GeomRef::Sub` operands on Boolean/Modify/Transform/Pattern/Sweep/
///   Isosurface (deps.rs:210-214). Re-seeded onto a dedicated realization
///   frontier so ITS own fan-out is walked too. Without this a ComputeNode
///   consuming a realization two hops downstream of the moved one is never
///   reached: `engine_edit::compute_changed_realizations` folds only a
///   realization's OWN op args and is blind to cross-`Sub` geometry refs, so
///   the downstream realization is not in the caller's seed set on its own
///   merit either.
/// - `NodeId::Value` — the GHR-δ S4 Realization→geometry-ValueCell edge
///   (deps.rs:266-268). Pushed onto the ValueCell frontier, exactly as the
///   BFS loop below does for a Value dependent, so consumers of a cell whose
///   `Value::GeometryHandle` was re-backed also become dirty.
/// - `NodeId::Constraint` — the geometry-query edge (deps.rs:194-196).
///   A genuine leaf: constraints have no dependents of their own.
///
/// Every arm rounds toward INCLUSION, from BOTH directions. A node wrongly in
/// the cone costs a recompute; a node wrongly out of it serves stale geometry,
/// which is the failure β exists to prevent.
///
/// `seen_realizations` is the single visit set for both producers, so every
/// realization is pushed at most once regardless of which map discovered it —
/// that is what makes the fixpoint terminate. It is deliberately NOT `dirty`:
/// the seeds are pre-loaded into `seen_realizations` but never enter `dirty`
/// (β leaves realization cache entries to γ #4730), so a `dirty`-guarded
/// re-seed would let a cycle returning to a seed re-enter the frontier.
///
/// Seed discrimination (task-spec test 3, locked in by step-13): the caller
/// is responsible for only inserting Realizations that actually moved. As of
/// #4729 the sole production caller discriminates on the GHR-β INPUT-cone
/// hash, NOT on `content_hash` — see "Production wiring" below for why the
/// static hash cannot serve here. An empty `changed_realizations` set yields no
/// Realization-driven propagation — the function only iterates over the
/// supplied seeds. This mirrors the existing `EvalOutcome::Changed/Unchanged`
/// pattern at the seed boundary instead of duplicating the comparison logic
/// inside the walk.
///
/// # Production wiring
///
/// Called from `engine_edit::invalidate_realization_dirty_cone`, which both
/// edit entries reach at their post-value-cone seams — `Engine::edit_param`
/// just after its snapshot install, and `Engine::edit_source` just before
/// step (15)'s. Every `NodeId` this walk returns has any per-node warm state
/// donated to the `WarmStatePool` and then its cache entry dropped, which is
/// what forces the consuming `@optimized` nodes to re-dispatch on the next
/// eval while unaffected consumers keep their cached result.
///
/// The seed is `Engine::last_changed_realizations`, produced by
/// `engine_edit::compute_changed_realizations`. That helper compares the
/// recomputed GHR-β INPUT-cone hash against α's stored
/// `RealizationNodeData::input_cone_hash` — deliberately NOT the static
/// `RealizationNodeData::content_hash`, which is a Debug render of the op IR
/// and so provably never moves on a value-driven change. Keying this walk on
/// the static hash would return an empty cone and silently evict nothing.
///
/// See PRD `docs/prds/v0_6/selective-realization-eviction.md` task β (#4729).
pub fn compute_dirty_cone_with_realizations(
    changed_vcs: &HashSet<ValueCellId>,
    changed_realizations: &HashSet<RealizationNodeId>,
    reverse_index: &ReverseDependencyIndex,
    graph: &crate::graph::EvaluationGraph,
) -> HashSet<NodeId> {
    let mut dirty: HashSet<NodeId> = HashSet::new();
    let mut frontier: VecDeque<ValueCellId> = changed_vcs.iter().cloned().collect();

    // Seed from changed realizations. `seen_realizations` is the visit set for
    // the realization frontier (NOT `dirty`, which never receives the seeds
    // themselves: β leaves realization cache entries to γ #4730), so a cycle
    // back through a seed cannot loop forever.
    let mut realization_frontier: VecDeque<RealizationNodeId> =
        changed_realizations.iter().cloned().collect();
    let mut seen_realizations: HashSet<RealizationNodeId> =
        changed_realizations.iter().cloned().collect();

    // ONE fixpoint over two mutually-feeding frontiers — see the doc comment
    // above. Draining them sequentially would leave the walk half-transitive:
    // a realization discovered by the ValueCell BFS would never get its own
    // fan-out walked, and the ValueCells that fan-out yields would never be
    // drained in turn.
    while !realization_frontier.is_empty() || !frontier.is_empty() {
        // Realization-keyed dependents. Transitive over the map's four
        // dependent kinds — see the doc comment for why each arm re-seeds
        // instead of terminating.
        while let Some(rid) = realization_frontier.pop_front() {
            for dependent in reverse_index.realization_dependents_of(&rid) {
                if !dirty.insert(dependent.clone()) {
                    continue;
                }
                match dependent {
                    // Edge #12: the ComputeNode writes these cells directly, so
                    // they are not reachable through `dependents_of` and must be
                    // inserted here.
                    NodeId::Compute(cn_id) => {
                        if let Some(cn_data) = graph.compute_nodes.get(cn_id) {
                            for vc in &cn_data.output_value_cells {
                                if dirty.insert(NodeId::Value(vc.clone())) {
                                    frontier.push_back(vc.clone());
                                }
                            }
                        }
                    }
                    // A downstream realization consuming this one via GeomRef::Sub.
                    NodeId::Realization(rid2) => {
                        if seen_realizations.insert(rid2.clone()) {
                            realization_frontier.push_back(rid2.clone());
                        }
                    }
                    // GHR-δ S4: the geometry cell this realization backs. Treated
                    // exactly like a Value dependent in the BFS loop below.
                    NodeId::Value(vcid) => frontier.push_back(vcid.clone()),
                    // Constraints and resolutions are leaves in this walk.
                    NodeId::Constraint(_) | NodeId::Resolution(_) => {}
                }
            }
        }

        // BFS over ValueCell dependents — as `compute_dirty_cone`, plus the
        // `NodeId::Realization` re-seed that closes the other direction.
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
                    // A realization that READS this cell (deps.rs:203-208).
                    // Re-seed so ITS own realization-keyed fan-out is walked
                    // too. Guarded on `seen_realizations`, never on `dirty`:
                    // the seeds are absent from `dirty` by construction, so a
                    // `dirty` guard would re-admit a seed reached by a cycle.
                    if let NodeId::Realization(rid2) = dependent
                        && seen_realizations.insert(rid2.clone())
                    {
                        realization_frontier.push_back(rid2.clone());
                    }
                }
            }
        }
    }

    dirty
}

/// Topologically sort a set of nodes — a thin delegate to the ONE scheduling core.
///
/// This function owns no scheduling logic of its own: it forwards to
/// [`crate::engine_fixpoint::run_unified_pass_seeded`], the single Kahn core
/// (INV-EVAL-5) shared with the cold, build, edit and concurrent paths. For the
/// order's mechanics — drain shape, which reads contribute in-degree, how
/// out-of-set producers are treated — read that function's doc; deliberately not
/// restated here, so this comment cannot drift out of sync with the core.
///
/// # Cycle-drop contract
///
/// The one property callers rely on that is NOT visible from the core's
/// signature: this delegate is PURE and must stay so — it must never append
/// cyclic residue to recover a total order. A node inside a dependency cycle
/// never reaches in-degree 0, so it is absent from the returned vector, and
/// `sorted.len() < nodes.len()` is exactly the cycle signal three production
/// sites rely on: [`crate::engine_eval`]'s `detect_let_cycle` and
/// `build_combined_param_let_graph` (which checks it twice — once on the combined
/// param+let graph, once on the let-only re-check), and [`crate::unfold`]'s
/// `elaborate_child_lets_only`. Appending residue here would silently disable all
/// three cycle diagnostics while leaving their call sites looking correct.
/// Callers that need a total order over a possibly-cyclic set must append the
/// residue *themselves*, at their own call site, where the choice is visible.
pub fn topological_sort(
    nodes: &HashSet<NodeId>,
    traces: &HashMap<NodeId, DependencyTrace>,
) -> Vec<NodeId> {
    // Argument order is the core's: (traces, seed).
    crate::engine_fixpoint::run_unified_pass_seeded(traces, nodes)
}

/// Compute the evaluation set: intersection of dirty cone and demand cone,
/// topologically sorted so dependencies are evaluated before dependents.
///
/// The sort is [`topological_sort`], so this entry point inherits the ONE
/// scheduling core's order — and its cycle-drop contract — transitively. This is
/// the order the edit path re-evaluates in, and it is the same order the
/// cold/build paths schedule; the two surfaces cannot diverge because there is
/// only one implementation to diverge from.
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
///   `exec_order` **and** `P` has its own trace entry (i.e. `P` is a
///   known graph node that was simply not scheduled).  Producers absent
///   from *both* `exec_order` and `traces` indicate a trace-extraction
///   gap (e.g. a recursive or Pattern/Sweep shape the extractor did not
///   fully walk) rather than a scheduling omission — those are silently
///   skipped to avoid false-positive panics on every debug build.
/// - Returns `Err(DagViolation::BackwardEdge)` if `n` is a
///   `NodeId::Realization(C)` and `pos[P] >= pos[C]` (producer must
///   strictly precede consumer).
///
/// Non-realization consumers (Value/Constraint/Resolution/Compute) only
/// get the `MissingProducer` check — they have no position in `exec_order`,
/// so ordering cannot be verified.
///
/// Iterates in arbitrary hash-map order; when multiple violations exist
/// the returned violation is unspecified.  This avoids sorting the full
/// node set on every debug build — the overwhelmingly common happy path
/// returns `Ok(())` with zero allocation beyond the `pos` map.
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

    for (node, trace) in traces {
        for producer in &trace.realization_reads {
            match pos.get(producer) {
                None => {
                    // Only a scheduling bug when the producer is actually a
                    // graph node (has its own trace entry).  If absent from
                    // both exec_order and traces, this edge came from a
                    // trace-extraction gap — skip to avoid false positives.
                    if traces.contains_key(&NodeId::Realization(producer.clone())) {
                        return Err(DagViolation::MissingProducer {
                            producer: producer.clone(),
                            consumer: node.clone(),
                        });
                    }
                }
                Some(&p_pos) => {
                    // BackwardEdge only applies to Realization consumers.
                    if let NodeId::Realization(consumer_rid) = node
                        && let Some(&c_pos) = pos.get(consumer_rid)
                        && p_pos >= c_pos
                    {
                        return Err(DagViolation::BackwardEdge {
                            producer: producer.clone(),
                            consumer: consumer_rid.clone(),
                            producer_pos: p_pos,
                            consumer_pos: c_pos,
                        });
                    }
                    // If consumer is a Realization but absent from exec_order,
                    // that is the caller's problem — skip ordering check.
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
/// This function is compiled only under `debug_assertions`; callers must
/// gate the call (and the `exec_order` allocation) with
/// `#[cfg(debug_assertions)]` so the whole site disappears entirely in
/// release builds.
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
                    cell_type: Type::dimensionless_scalar(),
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
                    cell_type: Type::dimensionless_scalar(),
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
                cell_type: Type::dimensionless_scalar(),
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
                input_cone_hash: None,
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
                cell_type: Type::dimensionless_scalar(),
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
                input_cone_hash: None,
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
                cell_type: Type::dimensionless_scalar(),
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
                input_cone_hash: None,
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

    /// β step-14 (a): the TWO-HOP realization re-seed arm.
    ///
    /// The three sibling `compute_dirty_cone_with_realizations_*` tests above
    /// all seed a realization that consumers reach in ONE hop (edge #10,
    /// Realization → Compute). None exercises the `NodeId::Realization`
    /// re-seed arm, which is the arm that makes the fan-out transitive.
    ///
    /// Topology: `R0` → `R1` (a `GeomRef::Sub` operand edge — `deps.rs:210-214`
    /// registers it via `extract_realization_edges`), `R1` → `Compute(C1)`
    /// (edge #10), `C1.output_value_cells = [b]` (edge #12).
    ///
    /// Seeding only `{R0}` must reach `Compute(C1)` AND `Value(b)` two hops
    /// downstream. This is the UNDER-eviction direction the frontier walk
    /// fixes, and it cannot be papered over at the caller:
    /// `engine_edit::compute_changed_realizations` folds only a realization's
    /// OWN op args and is blind to cross-`Sub` geometry refs, so `R1` is not in
    /// the seed set on its own merit either. Without the re-seed arm `C1` is
    /// never evicted and serves STALE GEOMETRY — the exact failure β exists to
    /// prevent.
    ///
    /// The `R0 → Realization(R1)` edge is spliced in with `add_realization`
    /// rather than derived from a real `GeomRef::Sub` op, mirroring the
    /// `index.add(...)` splice the step-9/step-13 fixtures already use: it is
    /// the same primitive `build_from_graph_and_fields` calls internally
    /// (deps.rs:214), and keeps the fixture pure-synthetic.
    #[test]
    fn compute_dirty_cone_with_realizations_fans_out_transitively_through_a_downstream_realization()
    {
        use crate::dirty::compute_dirty_cone_with_realizations;
        use crate::graph::{ComputeNodeData, EvaluationGraph, RealizationNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ComputeNodeId, ContentHash, RealizationNodeId, Type};
        use reify_ir::ReprKind;

        let mut graph = EvaluationGraph::default();
        let e = "E";

        // VC b — output of the compute node two hops downstream of R0.
        let b = ValueCellId::new(e, "b");
        graph.value_cells.insert(
            b.clone(),
            ValueCellNode {
                id: b.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::dimensionless_scalar(),
                default_expr: None,
                content_hash: ContentHash::of_str("b"),
            },
        );

        // Realizations R0 (the moved one, the only seed) and R1 (consumes R0
        // as a GeomRef::Sub operand).
        let r0_id = RealizationNodeId::new(e, 0);
        let r1_id = RealizationNodeId::new(e, 1);
        for rid in [&r0_id, &r1_id] {
            graph.realizations.insert(
                rid.clone(),
                RealizationNodeData {
                    geometry_cell: None,
                    id: rid.clone(),
                    operations: vec![],
                    content_hash: ContentHash::of_str("r"),
                    produced_repr: ReprKind::BRep,
                    produced_kernel: None,
                    input_cone_hash: None,
                },
            );
        }

        // Compute C1 consumes R1 (NOT R0), and writes b.
        let c1_id = ComputeNodeId::new(e, 0);
        graph.insert_compute_node(ComputeNodeData {
            computation_id: c1_id.clone(),
            target: "fea".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![r1_id.clone()],
            options_hash: ContentHash::of_str("opt"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![b.clone()],
        });

        // build_from_graph registers R1 → Compute(C1) (edge #10). Splice the
        // realization→realization hop R0 → Realization(R1) on top.
        let mut index = ReverseDependencyIndex::build_from_graph(&graph);
        index.add_realization(r0_id.clone(), NodeId::Realization(r1_id.clone()));

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
            dirty.contains(&NodeId::Realization(r1_id.clone())),
            "dirty cone should include the downstream Realization(R1) reached from the R0 seed, got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Compute(c1_id.clone())),
            "dirty cone should include Compute(C1) TWO hops downstream (R0 → R1 → C1); \
             a single-hop Compute-only expansion misses it and serves stale geometry, got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Value(b.clone())),
            "dirty cone should include Value(b) via edge #12 off the two-hop Compute(C1), got: {:?}",
            dirty
        );
        // The seed itself is never in the cone: β leaves realization cache
        // entries to γ (#4730), and R0 has no incoming realization edge here.
        assert!(
            !dirty.contains(&NodeId::Realization(r0_id.clone())),
            "the seed realization itself must not enter the cone, got: {:?}",
            dirty
        );
    }

    /// β step-14 (b): the `NodeId::Value` arm of the realization fan-out.
    ///
    /// The GHR-δ S4 edge (`deps.rs:266-268`) registers `Realization → Value`
    /// in the realization-keyed map for the geometry cell a realization backs.
    /// That dependent must land on the ValueCell FRONTIER, not merely in
    /// `dirty` — otherwise the walk terminates at the geometry cell and every
    /// consumer of it keeps a cached value computed from the old geometry.
    ///
    /// Topology: `R0` → `Value(g)` (the geometry cell R0 backs), `g` → `h`
    /// (an ordinary VC → VC read, edge #1). Seeding `{R0}` must dirty BOTH.
    #[test]
    fn compute_dirty_cone_with_realizations_pushes_backed_geometry_cell_onto_the_value_frontier() {
        use crate::dirty::compute_dirty_cone_with_realizations;
        use crate::graph::{EvaluationGraph, RealizationNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ContentHash, RealizationNodeId, Type};
        use reify_ir::ReprKind;

        let mut graph = EvaluationGraph::default();
        let e = "E";

        // `g` is the geometry cell R0 backs; `h` reads it. The walk never
        // consults a cell's `cell_type`, so the scalar type here is inert —
        // the fixture only needs the two cells to exist and be linked.
        for name in &["g", "h"] {
            let id = ValueCellId::new(e, *name);
            graph.value_cells.insert(
                id.clone(),
                ValueCellNode {
                    id: id.clone(),
                    kind: ValueCellKind::Param,
                    cell_type: Type::dimensionless_scalar(),
                    default_expr: None,
                    content_hash: ContentHash::of_str(name),
                },
            );
        }
        let g = ValueCellId::new(e, "g");
        let h = ValueCellId::new(e, "h");

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
                input_cone_hash: None,
            },
        );

        // GHR-δ S4: R0 → Value(g); ordinary edge #1: g → Value(h).
        let mut index = ReverseDependencyIndex::build_from_graph(&graph);
        index.add_realization(r0_id.clone(), NodeId::Value(g.clone()));
        index.add(g.clone(), NodeId::Value(h.clone()));

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
            dirty.contains(&NodeId::Value(g.clone())),
            "dirty cone should include the geometry cell Value(g) R0 backs (GHR-δ S4), got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Value(h.clone())),
            "dirty cone should include Value(h): the geometry cell must land on the ValueCell \
             FRONTIER, not merely in `dirty`, or its consumers keep stale-derived values, got: {:?}",
            dirty
        );
    }

    /// β step-14 (c): a `Realization → Realization` cycle terminates.
    ///
    /// `seen_realizations` is deliberately a separate visit set from `dirty`,
    /// because `dirty` never receives the SEEDS themselves — so reusing it
    /// would leave a cycle through a seed with no visited marker and loop
    /// forever. Topology: `R0 → R1 → R0`, plus `R1 → Compute(C1)` with
    /// `C1.output_value_cells = [b]` so the test also proves the walk still
    /// completes its useful work rather than terminating early.
    ///
    /// A regression here HANGS rather than failing an assertion; the value of
    /// the test is that the hang is attributed to this walk. Note `R0` DOES
    /// appear in the cone here — not as a seed, but as a genuine dependent
    /// reached from `R1` — which is why the visit set cannot be `dirty`.
    #[test]
    fn compute_dirty_cone_with_realizations_terminates_on_a_realization_cycle() {
        use crate::dirty::compute_dirty_cone_with_realizations;
        use crate::graph::{ComputeNodeData, EvaluationGraph, RealizationNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ComputeNodeId, ContentHash, RealizationNodeId, Type};
        use reify_ir::ReprKind;

        let mut graph = EvaluationGraph::default();
        let e = "E";

        let b = ValueCellId::new(e, "b");
        graph.value_cells.insert(
            b.clone(),
            ValueCellNode {
                id: b.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::dimensionless_scalar(),
                default_expr: None,
                content_hash: ContentHash::of_str("b"),
            },
        );

        let r0_id = RealizationNodeId::new(e, 0);
        let r1_id = RealizationNodeId::new(e, 1);
        for rid in [&r0_id, &r1_id] {
            graph.realizations.insert(
                rid.clone(),
                RealizationNodeData {
                    geometry_cell: None,
                    id: rid.clone(),
                    operations: vec![],
                    content_hash: ContentHash::of_str("r"),
                    produced_repr: ReprKind::BRep,
                    produced_kernel: None,
                    input_cone_hash: None,
                },
            );
        }

        let c1_id = ComputeNodeId::new(e, 0);
        graph.insert_compute_node(ComputeNodeData {
            computation_id: c1_id.clone(),
            target: "fea".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![r1_id.clone()],
            options_hash: ContentHash::of_str("opt"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![b.clone()],
        });

        // The cycle: R0 → R1 and R1 → R0.
        let mut index = ReverseDependencyIndex::build_from_graph(&graph);
        index.add_realization(r0_id.clone(), NodeId::Realization(r1_id.clone()));
        index.add_realization(r1_id.clone(), NodeId::Realization(r0_id.clone()));

        let mut changed_realizations = HashSet::new();
        changed_realizations.insert(r0_id.clone());
        let changed_vcs: HashSet<ValueCellId> = HashSet::new();

        // Reaching this call's return at all IS the termination assertion.
        let dirty = compute_dirty_cone_with_realizations(
            &changed_vcs,
            &changed_realizations,
            &index,
            &graph,
        );

        assert!(
            dirty.contains(&NodeId::Compute(c1_id.clone())),
            "the cycle must not stop the walk short of its useful work, got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Value(b.clone())),
            "dirty cone should still include Value(b) via edge #12 despite the cycle, got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Realization(r0_id.clone())),
            "R0 is reached as a genuine dependent of R1, so it lands in the cone even though \
             it was also the seed — which is exactly why the visit set cannot be `dirty`, got: {:?}",
            dirty
        );
    }

    /// β step-16 (a): the OTHER direction of the transitive fan-out — a
    /// realization reached through a VALUECELL, entered via a ComputeNode.
    ///
    /// step-14/step-15 made the realization-keyed fan-out transitive, but only
    /// on the realization-keyed map's own `NodeId::Realization` arm. The walk
    /// also feeds the ValueCell BFS from realization seeds (edge #12 off a
    /// `NodeId::Compute` dependent, and the GHR-δ S4 `NodeId::Value` arm), and
    /// `build_from_graph_and_fields` registers ValueCell→Realization edges in
    /// the VALUECELL-keyed map for every value-read in a realization's op args
    /// (`deps.rs:203-208`). So the walk can hop
    /// `Realization → ValueCell → Realization` — and until step-17 it DEAD-ENDS
    /// there: the BFS inserts the `NodeId::Realization` dependent into `dirty`
    /// and falls through both of its `if let` arms, never walking that second
    /// realization's own realization-keyed dependents.
    ///
    /// Topology: `C0{realization_inputs:[R0], output_value_cells:[b]}` puts `b`
    /// on the ValueCell frontier from the `{R0}` seed via edges #10 + #12;
    /// `index.add(b, Realization(R1))` is the value-read hop; and
    /// `C1{realization_inputs:[R1], output_value_cells:[d]}` is R1's OWN
    /// edge-#10 fan-out, which is what must not be lost. Seeded with an EMPTY
    /// `changed_vcs`, mirroring the sole production call site
    /// (`engine_edit::invalidate_realization_dirty_cone`).
    #[test]
    fn compute_dirty_cone_with_realizations_fans_out_past_a_realization_behind_a_value_cell() {
        use crate::dirty::compute_dirty_cone_with_realizations;
        use crate::graph::{ComputeNodeData, EvaluationGraph, RealizationNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ComputeNodeId, ContentHash, RealizationNodeId, Type};
        use reify_ir::ReprKind;

        let mut graph = EvaluationGraph::default();
        let e = "E";

        // `b` is C0's output cell (the hop cell R1 reads); `d` is C1's output.
        for name in &["b", "d"] {
            let id = ValueCellId::new(e, *name);
            graph.value_cells.insert(
                id.clone(),
                ValueCellNode {
                    id: id.clone(),
                    kind: ValueCellKind::Param,
                    cell_type: Type::dimensionless_scalar(),
                    default_expr: None,
                    content_hash: ContentHash::of_str(name),
                },
            );
        }
        let b = ValueCellId::new(e, "b");
        let d = ValueCellId::new(e, "d");

        let r0_id = RealizationNodeId::new(e, 0);
        let r1_id = RealizationNodeId::new(e, 1);
        for rid in [&r0_id, &r1_id] {
            graph.realizations.insert(
                rid.clone(),
                RealizationNodeData {
                    geometry_cell: None,
                    id: rid.clone(),
                    operations: vec![],
                    content_hash: ContentHash::of_str("r"),
                    produced_repr: ReprKind::BRep,
                    produced_kernel: None,
                    input_cone_hash: None,
                },
            );
        }

        // C0 consumes the SEED R0 and writes `b`; C1 consumes R1 and writes `d`.
        let c0_id = ComputeNodeId::new(e, 0);
        let c1_id = ComputeNodeId::new(e, 1);
        graph.insert_compute_node(ComputeNodeData {
            computation_id: c0_id.clone(),
            target: "fea".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![r0_id.clone()],
            options_hash: ContentHash::of_str("opt0"),
            cache_key: ContentHash::of_str("ck0"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![b.clone()],
        });
        graph.insert_compute_node(ComputeNodeData {
            computation_id: c1_id.clone(),
            target: "fea".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![r1_id.clone()],
            options_hash: ContentHash::of_str("opt1"),
            cache_key: ContentHash::of_str("ck1"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![d.clone()],
        });

        // build_from_graph registers R0 → Compute(C0) and R1 → Compute(C1)
        // (edge #10). Splice the VALUECELL-keyed hop `b` → Realization(R1),
        // i.e. R1 reads `b` in its op args (deps.rs:203-208).
        let mut index = ReverseDependencyIndex::build_from_graph(&graph);
        index.add(b.clone(), NodeId::Realization(r1_id.clone()));

        let mut changed_realizations = HashSet::new();
        changed_realizations.insert(r0_id.clone());
        let changed_vcs: HashSet<ValueCellId> = HashSet::new();

        let dirty = compute_dirty_cone_with_realizations(
            &changed_vcs,
            &changed_realizations,
            &index,
            &graph,
        );

        // The first hop already works today — assert it so a failure below is
        // unambiguously the ValueCell→Realization dead-end, not a broken fixture.
        assert!(
            dirty.contains(&NodeId::Compute(c0_id.clone())),
            "fixture check: Compute(C0) consumes the seed R0 via edge #10, got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Value(b.clone())),
            "fixture check: Value(b) is C0's output cell (edge #12), got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Realization(r1_id.clone())),
            "fixture check: Realization(R1) reads `b`, so the BFS reaches it, got: {:?}",
            dirty
        );

        // The actual contract: R1's OWN fan-out must be walked too.
        assert!(
            dirty.contains(&NodeId::Compute(c1_id.clone())),
            "dirty cone should include Compute(C1), R1's own edge-#10 consumer: a realization \
             discovered through the VALUECELL-keyed map must be re-seeded onto the realization \
             frontier, not treated as a leaf — otherwise C1 serves STALE GEOMETRY, got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Value(d.clone())),
            "dirty cone should include Value(d) via edge #12 off C1, got: {:?}",
            dirty
        );
    }

    /// β step-16 (b): the same ValueCell hop, entered through the OTHER arm
    /// that feeds the BFS from a realization seed — the GHR-δ S4
    /// `Realization → Value` edge (`deps.rs:266-268`) rather than edge #12 off
    /// a ComputeNode. Both producers must reach the fixpoint, not just the
    /// Compute one.
    ///
    /// Topology: `R0 → Value(g)` (the geometry cell R0 backs),
    /// `g → Realization(R1)` (R1 reads `g` in its op args), and
    /// `C1{realization_inputs:[R1], output_value_cells:[d]}`.
    #[test]
    fn compute_dirty_cone_with_realizations_fans_out_past_a_realization_behind_a_geometry_cell() {
        use crate::dirty::compute_dirty_cone_with_realizations;
        use crate::graph::{ComputeNodeData, EvaluationGraph, RealizationNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ComputeNodeId, ContentHash, RealizationNodeId, Type};
        use reify_ir::ReprKind;

        let mut graph = EvaluationGraph::default();
        let e = "E";

        for name in &["g", "d"] {
            let id = ValueCellId::new(e, *name);
            graph.value_cells.insert(
                id.clone(),
                ValueCellNode {
                    id: id.clone(),
                    kind: ValueCellKind::Param,
                    cell_type: Type::dimensionless_scalar(),
                    default_expr: None,
                    content_hash: ContentHash::of_str(name),
                },
            );
        }
        let g = ValueCellId::new(e, "g");
        let d = ValueCellId::new(e, "d");

        let r0_id = RealizationNodeId::new(e, 0);
        let r1_id = RealizationNodeId::new(e, 1);
        for rid in [&r0_id, &r1_id] {
            graph.realizations.insert(
                rid.clone(),
                RealizationNodeData {
                    geometry_cell: None,
                    id: rid.clone(),
                    operations: vec![],
                    content_hash: ContentHash::of_str("r"),
                    produced_repr: ReprKind::BRep,
                    produced_kernel: None,
                    input_cone_hash: None,
                },
            );
        }

        let c1_id = ComputeNodeId::new(e, 0);
        graph.insert_compute_node(ComputeNodeData {
            computation_id: c1_id.clone(),
            target: "fea".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![r1_id.clone()],
            options_hash: ContentHash::of_str("opt"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![d.clone()],
        });

        // GHR-δ S4: R0 → Value(g). Then the value-read hop g → Realization(R1).
        let mut index = ReverseDependencyIndex::build_from_graph(&graph);
        index.add_realization(r0_id.clone(), NodeId::Value(g.clone()));
        index.add(g.clone(), NodeId::Realization(r1_id.clone()));

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
            dirty.contains(&NodeId::Value(g.clone())),
            "fixture check: Value(g) is the geometry cell R0 backs (GHR-δ S4), got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Realization(r1_id.clone())),
            "fixture check: Realization(R1) reads `g`, so the BFS reaches it, got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Compute(c1_id.clone())),
            "dirty cone should include Compute(C1): the GHR-δ S4 `NodeId::Value` arm feeds the \
             same ValueCell BFS, so a realization discovered from IT must also be re-seeded, \
             got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Value(d.clone())),
            "dirty cone should include Value(d) via edge #12 off C1, got: {:?}",
            dirty
        );
    }

    /// β step-16 (c): cycle safety across the MIXED Realization/ValueCell path.
    ///
    /// (a)'s topology, closed back to the seed with
    /// `index.add(d, Realization(R0))`. This pins the invariant step-17's
    /// re-seed arm must not break: the seeds live in `seen_realizations` but
    /// deliberately NEVER in `dirty` (β leaves realization cache entries to
    /// γ #4730), so the BFS re-seed must be guarded on `seen_realizations`.
    /// A `dirty`-guarded re-seed would let a cycle returning to a seed
    /// re-enter the realization frontier.
    ///
    /// A regression here HANGS rather than failing an assertion; the value of
    /// the test is that the hang is attributed to this walk. The membership
    /// assertions additionally prove the cycle does not stop the walk short of
    /// its useful work.
    #[test]
    fn compute_dirty_cone_with_realizations_terminates_on_a_mixed_valuecell_realization_cycle() {
        use crate::dirty::compute_dirty_cone_with_realizations;
        use crate::graph::{ComputeNodeData, EvaluationGraph, RealizationNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ComputeNodeId, ContentHash, RealizationNodeId, Type};
        use reify_ir::ReprKind;

        let mut graph = EvaluationGraph::default();
        let e = "E";

        for name in &["b", "d"] {
            let id = ValueCellId::new(e, *name);
            graph.value_cells.insert(
                id.clone(),
                ValueCellNode {
                    id: id.clone(),
                    kind: ValueCellKind::Param,
                    cell_type: Type::dimensionless_scalar(),
                    default_expr: None,
                    content_hash: ContentHash::of_str(name),
                },
            );
        }
        let b = ValueCellId::new(e, "b");
        let d = ValueCellId::new(e, "d");

        let r0_id = RealizationNodeId::new(e, 0);
        let r1_id = RealizationNodeId::new(e, 1);
        for rid in [&r0_id, &r1_id] {
            graph.realizations.insert(
                rid.clone(),
                RealizationNodeData {
                    geometry_cell: None,
                    id: rid.clone(),
                    operations: vec![],
                    content_hash: ContentHash::of_str("r"),
                    produced_repr: ReprKind::BRep,
                    produced_kernel: None,
                    input_cone_hash: None,
                },
            );
        }

        let c0_id = ComputeNodeId::new(e, 0);
        let c1_id = ComputeNodeId::new(e, 1);
        graph.insert_compute_node(ComputeNodeData {
            computation_id: c0_id.clone(),
            target: "fea".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![r0_id.clone()],
            options_hash: ContentHash::of_str("opt0"),
            cache_key: ContentHash::of_str("ck0"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![b.clone()],
        });
        graph.insert_compute_node(ComputeNodeData {
            computation_id: c1_id.clone(),
            target: "fea".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![r1_id.clone()],
            options_hash: ContentHash::of_str("opt1"),
            cache_key: ContentHash::of_str("ck1"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![d.clone()],
        });

        // R0 →#10 C0 →#12 b → R1 →#10 C1 →#12 d → R0: the cycle closes on the
        // SEED, through both maps.
        let mut index = ReverseDependencyIndex::build_from_graph(&graph);
        index.add(b.clone(), NodeId::Realization(r1_id.clone()));
        index.add(d.clone(), NodeId::Realization(r0_id.clone()));

        let mut changed_realizations = HashSet::new();
        changed_realizations.insert(r0_id.clone());
        let changed_vcs: HashSet<ValueCellId> = HashSet::new();

        // Reaching this call's return at all IS the termination assertion.
        let dirty = compute_dirty_cone_with_realizations(
            &changed_vcs,
            &changed_realizations,
            &index,
            &graph,
        );

        assert!(
            dirty.contains(&NodeId::Compute(c1_id.clone())),
            "the mixed cycle must not stop the walk short of its useful work, got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Value(d.clone())),
            "dirty cone should still include Value(d) via edge #12 despite the cycle, got: {:?}",
            dirty
        );
        assert!(
            dirty.contains(&NodeId::Realization(r0_id.clone())),
            "R0 is reached as a genuine ValueCell dependent of `d`, so it lands in `dirty` even \
             though it was also the seed — which is why the re-seed guard must be \
             `seen_realizations`, not `dirty`, got: {:?}",
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
                cell_type: Type::dimensionless_scalar(),
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
                producer_pos,
                consumer_pos,
            }) => {
                assert_eq!(producer, p_id, "wrong producer in BackwardEdge");
                assert_eq!(consumer, c_id, "wrong consumer in BackwardEdge");
                // exec_order = [c_id, p_id]: consumer at index 0, producer at index 1.
                assert_eq!(producer_pos, 1, "producer_pos should be 1");
                assert_eq!(consumer_pos, 0, "consumer_pos should be 0");
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
        let p_id = RealizationNodeId::new(e, 0); // producer — in graph but NOT in exec_order
        let c_id = RealizationNodeId::new(e, 1); // consumer

        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        // P exists as a graph node (has its own trace entry) but will be
        // absent from exec_order — this is the genuine MissingProducer case.
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
        let p_id = RealizationNodeId::new(e, 0); // producer — in graph but NOT in exec_order
        let k = ConstraintNodeId::new(e, 0);

        let mut traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        // P exists as a graph node (has its own trace entry) but will be
        // absent from exec_order — genuine MissingProducer for a constraint consumer.
        traces.insert(
            NodeId::Realization(p_id.clone()),
            DependencyTrace::default(),
        );
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
    ///
    /// Gated on `debug_assertions` to match `assert_dag_complete_from_graph`,
    /// which is compiled only under `debug_assertions` (the wrapper disappears
    /// entirely in release builds, so the test must too — otherwise the import
    /// fails to resolve under `--release`).
    #[cfg(debug_assertions)]
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
                input_cone_hash: None,
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
                input_cone_hash: None,
            },
        );

        // REVERSED exec_order: outer (consumer) before inner_a (producer)
        // → BackwardEdge → panic
        let exec_order = vec![outer.clone(), inner_a.clone()];

        // Should panic with message containing "assert_dag_complete"
        assert_dag_complete_from_graph(&graph, &[], &exec_order);
    }
}
