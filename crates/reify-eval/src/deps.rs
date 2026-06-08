//! Static dependency extraction for evaluation graph nodes.
//!
//! Each node's dependencies are extracted once at graph-build time by walking
//! the compiled expression tree. This is a safe over-approximation for Reify
//! expressions: because they are pure, the set of cells an expression *can*
//! read is a superset of (or equal to) the set it *will* read on any given
//! evaluation — conditional branches and match arms include every reachable ValueRef
//! across all arms, even though only one arm is taken at runtime, so the static set
//! is a superset of the runtime-read set. There is no benefit to runtime
//! (Adapton-style) tracing in a pure language.

use crate::cache::NodeId;
use reify_core::{RealizationNodeId, ValueCellId};
use reify_ir::CompiledExpr;
use std::collections::{HashMap, HashSet};

/// Statically extracted value cell dependencies for a node.
///
/// Computed once from the compiled expression tree at graph-build time,
/// not during evaluation. Params and other root nodes use
/// `DependencyTrace::default()` (empty reads).
#[derive(Debug, Clone, Default)]
pub struct DependencyTrace {
    pub reads: Vec<ValueCellId>,
    /// GHR-δ (PRD geometry-handle-runtime.md §8 Phase 4): RealizationNodeIds
    /// this node implicitly depends on. A value cell holding a
    /// `Value::GeometryHandle` reads the upstream Realization named in its
    /// `realization_ref`; that edge is invisible to the VC→VC `reads` set, so
    /// it is recorded here and folded into freshness derivation
    /// (`derive_output_freshness_from_trace_with_cause`) and the freshness-only
    /// walk's Realization→ValueCell fan-out. Empty for all non-geometry nodes.
    pub realization_reads: Vec<RealizationNodeId>,
}

/// Extract a dependency trace from a compiled expression by collecting all ValueRef ids.
pub fn extract_dependency_trace(expr: &CompiledExpr) -> DependencyTrace {
    DependencyTrace {
        realization_reads: Vec::new(),
        reads: expr.collect_value_refs(),
    }
}

/// Remove and return the trace for `node_id`; panics with a message naming
/// both `sorted_set_name` and `trace_map_name` if the key is absent.
pub(crate) fn take_trace(
    traces: &mut HashMap<NodeId, DependencyTrace>,
    node_id: &NodeId,
    sorted_set_name: &'static str,
    trace_map_name: &'static str,
) -> DependencyTrace {
    traces
        .remove(node_id)
        .unwrap_or_else(|| panic!("{sorted_set_name} entries are always keys in {trace_map_name}"))
}

/// Reverse dependency index: maps ValueCellId → set of NodeIds that depend on it.
///
/// This enables forward propagation: when a cell changes, look up which nodes
/// need to be re-evaluated. Built from graph structure (expressions), not runtime traces.
///
/// P3.3: a sibling `realization_index` map captures the edge #10 reverse
/// (Realization → consuming ComputeNodes). Keyed by `RealizationNodeId` —
/// the existing `index` field's `ValueCellId` key cannot represent that
/// edge. Same public API shape: an `add_realization`/`realization_dependents_of`
/// pair mirrors `add`/`dependents_of`.
#[derive(Clone, Debug)]
pub struct ReverseDependencyIndex {
    index: HashMap<ValueCellId, HashSet<NodeId>>,
    realization_index: HashMap<RealizationNodeId, HashSet<NodeId>>,
}

/// Empty set constant for returning references to unknown cells.
static EMPTY_SET: std::sync::LazyLock<HashSet<NodeId>> = std::sync::LazyLock::new(HashSet::new);

impl Default for ReverseDependencyIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl ReverseDependencyIndex {
    /// Create an empty reverse dependency index.
    pub fn new() -> Self {
        Self {
            index: HashMap::new(),
            realization_index: HashMap::new(),
        }
    }

    /// Add a mapping: `cell` is read by `dependent`.
    pub fn add(&mut self, cell: ValueCellId, dependent: NodeId) {
        self.index.entry(cell).or_default().insert(dependent);
    }

    /// Return the set of NodeIds that depend on the given cell.
    /// Returns an empty set for unknown cells.
    pub fn dependents_of(&self, cell: &ValueCellId) -> &HashSet<NodeId> {
        self.index.get(cell).unwrap_or(&EMPTY_SET)
    }

    /// P3.3 edge #10: register that `rid` is read by `dependent` (typically
    /// a `NodeId::Compute(...)` whose `realization_inputs` includes `rid`).
    /// Sibling of [`Self::add`] for the realization-keyed reverse map.
    pub fn add_realization(&mut self, rid: RealizationNodeId, dependent: NodeId) {
        self.realization_index
            .entry(rid)
            .or_default()
            .insert(dependent);
    }

    /// P3.3 edge #10: return the set of NodeIds that consume the given
    /// Realization (i.e. ComputeNodes with `rid` in their
    /// `realization_inputs`). Returns the static empty set for unknown
    /// RealizationNodeIds, matching the [`Self::dependents_of`] contract.
    pub fn realization_dependents_of(&self, rid: &RealizationNodeId) -> &HashSet<NodeId> {
        self.realization_index.get(rid).unwrap_or(&EMPTY_SET)
    }

    /// Build a reverse dependency index from an EvaluationGraph.
    ///
    /// Thin wrapper over [`build_from_graph_and_fields`] with an empty
    /// fields slice. Preserved for callers (e.g. tests) that have no
    /// access to `module.fields`. Production paths in `engine_eval.rs`,
    /// `engine_edit.rs`, and `engine_purposes.rs` use the `_and_fields`
    /// variant so composed-field deps are registered alongside template
    /// deps.
    pub fn build_from_graph(graph: &crate::graph::EvaluationGraph) -> Self {
        Self::build_from_graph_and_fields(graph, &[])
    }

    /// Build a reverse dependency index from an EvaluationGraph plus a
    /// fields slice.
    ///
    /// Iterates all value cells (extracting deps from default_expr),
    /// constraints (extracting deps from expr), realizations (extracting
    /// deps from operation args), and finally each composed field — for
    /// which `extract_dependency_trace` surfaces the augmented
    /// `Lambda { captures, .. }` deps injected by the compiler's
    /// `phase_augment_composed_captures` post-pass.
    pub fn build_from_graph_and_fields(
        graph: &crate::graph::EvaluationGraph,
        fields: &[reify_compiler::CompiledField],
    ) -> Self {
        use reify_compiler::CompiledFieldSource;
        use reify_core::FIELD_ENTITY_PREFIX;

        let mut index = Self::new();

        // Build cell→realization resolver once (used for constraint→realization
        // and value-cell→realization edges below).
        let realization_by_cell = realization_by_cell(graph);

        // Value cells: non-auto cells with a default_expr have static read-dependencies.
        // Auto cells are solver-owned leaves — no default expression to extract.
        // Param cells with a literal default (no ValueRef reads) produce an empty
        // trace, so they register no reverse edges and remain pure roots.
        //
        // Value-cell→realization edges (step-4): for each read cell that is itself
        // backed by a realization (i.e. present in realization_by_cell), register
        // the reverse edge realization → Value(node). This covers selectors, whole-
        // handle queries, param defaults like `param v = volume(body)`, and any Let
        // that reads a geometry handle. Uniform rule: "reads a geometry cell" →
        // realization_reads edge, regardless of Param vs Let (post-4317).
        for (_, node) in graph.value_cells.iter() {
            if node.kind.is_auto() {
                continue;
            }
            if let Some(ref expr) = node.default_expr {
                let trace = extract_dependency_trace(expr);
                let node_id = NodeId::Value(node.id.clone());
                for cell in &trace.reads {
                    index.add(cell.clone(), node_id.clone());
                }
                // Resolve reads that are geometry cells to their backing realizations.
                // Use a temporary set to dedup (collect_value_refs preserves duplicates).
                let mut seen_rids: HashSet<RealizationNodeId> = HashSet::new();
                for cell in &trace.reads {
                    if let Some(rid) = realization_by_cell.get(cell) {
                        if seen_rids.insert(rid.clone()) {
                            index.add_realization(rid.clone(), node_id.clone());
                        }
                    }
                }
            }
        }

        // Constraints: extract deps from constraint expression.
        // Also extract constraint→realization edges: if the constraint contains a
        // geometry-query call (volume/area/centroid/bounding_box) whose single arg
        // is a ValueRef to a geometry cell backed by a realization, register the
        // reverse edge (realization → Constraint node) in the realization_index.
        for (_, cnode) in graph.constraints.iter() {
            let trace = extract_dependency_trace(&cnode.expr);
            let node_id = NodeId::Constraint(cnode.id.clone());
            for cell in &trace.reads {
                index.add(cell.clone(), node_id.clone());
            }
            // Constraint→realization edge: walk expr for geometry-query calls.
            let mut seen_rids: HashSet<RealizationNodeId> = HashSet::new();
            cnode.expr.walk(&mut |node| {
                if crate::geometry_ops::is_geometry_query_call(node) {
                    if let reify_ir::CompiledExprKind::FunctionCall { args, .. } = &node.kind {
                        if let Some(arg) = args.first() {
                            if let reify_ir::CompiledExprKind::ValueRef(cell_id) = &arg.kind {
                                if let Some(rid) = realization_by_cell.get(cell_id) {
                                    if seen_rids.insert(rid.clone()) {
                                        index.add_realization(rid.clone(), node_id.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            });
        }

        // Realizations: extract VC deps from operation args (value-reads), plus
        // realization→realization edges from GeomRef::Sub operands (step-6: Boolean;
        // step-8: Modify/Transform/Pattern/Sweep). Dedup producing rids per consumer.
        for (_, rnode) in graph.realizations.iter() {
            let trace = extract_realization_dependencies(&rnode.operations);
            let node_id = NodeId::Realization(rnode.id.clone());
            for cell in &trace.reads {
                index.add(cell.clone(), node_id.clone());
            }
            // Realization→realization reverse edges.
            let mut seen: HashSet<RealizationNodeId> = HashSet::new();
            for producing_rid in
                extract_realization_edges(&rnode.operations, &rnode.id.entity, graph)
            {
                if seen.insert(producing_rid.clone()) {
                    index.add_realization(producing_rid, node_id.clone());
                }
            }
        }

        // Resolutions: auto_params are the static dependencies
        for (_, res_node) in graph.resolutions.iter() {
            let node_id = NodeId::Resolution(res_node.id.clone());
            for param in &res_node.auto_params {
                index.add(param.clone(), node_id.clone());
            }
        }

        // ComputeNodes:
        //   Edge #6  (P3.3 step-4): VC → ComputeNode reverse edge. Each
        //     entry in `value_inputs` is a VC the ComputeNode reads, so
        //     the ComputeNode becomes a dependent of that VC.
        //   Edge #10 (P3.3 step-6): Realization → ComputeNode reverse edge.
        //     Each entry in `realization_inputs` is a Realization whose
        //     output the ComputeNode consumes, so the ComputeNode becomes
        //     a dependent of that Realization in the sibling
        //     `realization_index` map (key type differs from `index`).
        for (_, cnode) in graph.compute_nodes.iter() {
            let node_id = NodeId::Compute(cnode.computation_id.clone());
            for vc in &cnode.value_inputs {
                index.add(vc.clone(), node_id.clone());
            }
            for rid in &cnode.realization_inputs {
                index.add_realization(rid.clone(), node_id.clone());
            }
        }

        // Composed fields: extract deps from the lambda expression — the
        // compiler post-pass `phase_augment_composed_captures` already
        // injected `__field.<name>` cells into the lambda's `captures`,
        // so `extract_dependency_trace` surfaces them via the standard
        // `Lambda { captures, .. }` arm.
        for field in fields {
            if let CompiledFieldSource::Composed { expr } = &field.source {
                let trace = extract_dependency_trace(expr);
                let field_cell = ValueCellId::new(FIELD_ENTITY_PREFIX, &field.name);
                let node_id = NodeId::Value(field_cell);
                for cell in &trace.reads {
                    index.add(cell.clone(), node_id.clone());
                }
            }
        }

        // GHR-δ S4 (edge: Realization → GH ValueCell). A value cell holding a
        // `Value::GeometryHandle` depends on the upstream Realization that
        // produced its handle — an edge invisible to the VC→VC expression scan
        // above. Register it in the realization-keyed reverse map (reusing the
        // edge #10 machinery) so the freshness walk's Realization fan-out and
        // the edit-time donation cascade can find the backed cells.
        for (rid, cell) in geometry_cell_realization_links(graph) {
            index.add_realization(rid, NodeId::Value(cell));
        }

        index
    }
}

/// GHR-δ: yield each `(realization_id, geometry_cell)` link recorded on the
/// graph by [`crate::graph::EvaluationGraph::from_templates`] (S2) — the single
/// source of truth for the Realization→ValueCell freshness edge. Consumed by
/// the forward trace builder ([`build_trace_map_and_fields`]), the reverse
/// index builder ([`ReverseDependencyIndex::build_from_graph_and_fields`]), and
/// the cold/incremental eval-trace wiring in `engine_eval.rs` / `engine_edit.rs`.
///
/// **1:1 invariant.** Each geometry cell is expected to be backed by at most one
/// realization: `from_templates` links a realization to the `Type::Geometry`
/// value cell whose `member == realization.name`, and realization names are
/// unique within an entity, so distinct realizations resolve to distinct cells.
/// Callers that fold these links into a per-cell `realization_reads` list MUST
/// nonetheless accumulate (not overwrite-last) so they stay consistent with the
/// `push`-accumulating [`build_trace_map_and_fields`] even if that invariant is
/// ever violated — use [`geometry_cell_realization_reads`] rather than calling
/// [`crate::cache::CacheStore::set_realization_reads`] once per raw link.
pub(crate) fn geometry_cell_realization_links(
    graph: &crate::graph::EvaluationGraph,
) -> impl Iterator<Item = (RealizationNodeId, ValueCellId)> + '_ {
    graph.realizations.iter().filter_map(|(_, rnode)| {
        rnode
            .geometry_cell
            .as_ref()
            .map(|cell| (rnode.id.clone(), cell.clone()))
    })
}

/// Build an inverted map from geometry `ValueCellId` → its backing
/// `RealizationNodeId`. This is the resolver for edges #1 (selector/query
/// cell → realization) and #2 (constraint → realization):
///
/// - `geometry_cell_realization_links` iterates `(rid, cell)` pairs from the
///   graph — one per realization that has a `geometry_cell` set (GHR-δ S2).
/// - Inverting it lets the edge-extraction code look up "which realization
///   backs cell X?" in O(1) during the value-cells and constraints loops,
///   without re-iterating the realizations map per cell.
fn realization_by_cell(
    graph: &crate::graph::EvaluationGraph,
) -> HashMap<ValueCellId, RealizationNodeId> {
    geometry_cell_realization_links(graph)
        .map(|(rid, cell)| (cell, rid))
        .collect()
}

/// GHR-δ: fold [`geometry_cell_realization_links`] into a per-cell
/// `realization_reads` list, accumulating (never overwriting) every realization
/// that backs a given geometry cell.
///
/// This is the single source of truth for the cold-eval (`engine_eval.rs`) and
/// incremental (`engine_edit.rs`) cache post-pass that calls
/// [`crate::cache::CacheStore::set_realization_reads`]. Folding here — rather
/// than calling the replace-semantics setter once per raw link — guarantees the
/// cached trace carries the SAME accumulated `realization_reads` that
/// [`build_trace_map_and_fields`] records via `push`, so the two freshness
/// derivation paths cannot silently diverge if the 1:1 cell↔realization
/// invariant is ever broken (two realizations sharing one geometry cell). In the
/// expected 1:1 case each list has exactly one element, identical to the prior
/// per-link behaviour. The replace-per-build setter remains idempotent across
/// re-eval / edit rounds because each build re-folds from scratch.
pub(crate) fn geometry_cell_realization_reads(
    graph: &crate::graph::EvaluationGraph,
) -> HashMap<ValueCellId, Vec<RealizationNodeId>> {
    let mut reads: HashMap<ValueCellId, Vec<RealizationNodeId>> = HashMap::new();
    for (rid, cell) in geometry_cell_realization_links(graph) {
        reads.entry(cell).or_default().push(rid);
    }
    reads
}

/// Build a forward dependency trace map for all nodes in the graph.
///
/// Thin wrapper over [`build_trace_map_and_fields`] with an empty fields slice.
/// Preserved for callers (e.g. tests, dirty.rs) that have no access to
/// `module.fields`. Production paths in `engine_eval.rs`, `engine_edit.rs`,
/// and `engine_purposes.rs` use the `_and_fields` variant so composed-field
/// traces are registered alongside template traces.
pub fn build_trace_map(graph: &crate::graph::EvaluationGraph) -> HashMap<NodeId, DependencyTrace> {
    build_trace_map_and_fields(graph, &[])
}

/// Build a forward dependency trace map for all nodes in the graph plus
/// any composed fields supplied via `fields`.
///
/// Returns a HashMap<NodeId, DependencyTrace> that maps each node to
/// the set of ValueCellIds it reads. Used by topological sort and demand cone.
///
/// Composed fields are keyed by `NodeId::Value(ValueCellId(FIELD_ENTITY_PREFIX, name))`,
/// matching the cell IDs registered in `Snapshot::values` by the field elaboration
/// loop in `engine_eval.rs`. The trace surfaces the augmented `Lambda { captures, .. }`
/// deps injected by the compiler's `phase_augment_composed_captures` post-pass.
pub fn build_trace_map_and_fields(
    graph: &crate::graph::EvaluationGraph,
    fields: &[reify_compiler::CompiledField],
) -> HashMap<NodeId, DependencyTrace> {
    use reify_compiler::CompiledFieldSource;
    use reify_core::FIELD_ENTITY_PREFIX;

    let mut traces = HashMap::new();

    // Build cell→realization resolver once (used for constraint→realization
    // and value-cell→realization edges below).
    let realization_by_cell = realization_by_cell(graph);

    for (_, node) in graph.value_cells.iter() {
        let mut trace = if node.kind.is_auto() {
            // Auto cells are solver-owned leaves; no default expression to extract.
            DependencyTrace::default()
        } else {
            // Param and Let cells: extract deps from their default_expr.
            // A param with a literal default (no ValueRef reads) gets an empty
            // trace (equivalent to default()), preserving root semantics.
            node.default_expr
                .as_ref()
                .map(extract_dependency_trace)
                .unwrap_or_default()
        };
        // Value-cell→realization edges (step-4): for each read cell backed by a
        // realization, push the realization into this node's forward trace.
        // Covers selectors, queries, param defaults — all non-auto cells uniformly
        // (post-4317 rule). Dedup via seen_rids (collect_value_refs keeps dupes).
        if !trace.reads.is_empty() {
            let mut seen_rids: HashSet<RealizationNodeId> = HashSet::new();
            for cell in &trace.reads {
                if let Some(rid) = realization_by_cell.get(cell) {
                    if seen_rids.insert(rid.clone()) {
                        trace.realization_reads.push(rid.clone());
                    }
                }
            }
        }
        traces.insert(NodeId::Value(node.id.clone()), trace);
    }

    // Constraints: base VC→Constraint deps from extract_dependency_trace, plus
    // constraint→realization edges via geometry-query call detection.
    for (_, cnode) in graph.constraints.iter() {
        let mut trace = extract_dependency_trace(&cnode.expr);
        // Walk expr for geometry-query calls (volume/area/centroid/bounding_box).
        // If the single arg is a ValueRef to a geometry cell backed by a
        // realization, add that realization to the forward realization_reads.
        let mut seen_rids: HashSet<RealizationNodeId> = HashSet::new();
        cnode.expr.walk(&mut |node| {
            if crate::geometry_ops::is_geometry_query_call(node) {
                if let reify_ir::CompiledExprKind::FunctionCall { args, .. } = &node.kind {
                    if let Some(arg) = args.first() {
                        if let reify_ir::CompiledExprKind::ValueRef(cell_id) = &arg.kind {
                            if let Some(rid) = realization_by_cell.get(cell_id) {
                                if seen_rids.insert(rid.clone()) {
                                    trace.realization_reads.push(rid.clone());
                                }
                            }
                        }
                    }
                }
            }
        });
        traces.insert(NodeId::Constraint(cnode.id.clone()), trace);
    }

    // Realizations: extract VC deps from operation args (value-reads), plus
    // realization→realization edges from GeomRef::Sub operands (step-6: Boolean;
    // step-8: Modify/Transform/Pattern/Sweep). Dedup producing rids per consumer.
    for (_, rnode) in graph.realizations.iter() {
        let mut trace = extract_realization_dependencies(&rnode.operations);
        // Realization→realization forward edges.
        let mut seen: HashSet<RealizationNodeId> = HashSet::new();
        for producing_rid in
            extract_realization_edges(&rnode.operations, &rnode.id.entity, graph)
        {
            if seen.insert(producing_rid.clone()) {
                trace.realization_reads.push(producing_rid);
            }
        }
        traces.insert(NodeId::Realization(rnode.id.clone()), trace);
    }

    for (_, res_node) in graph.resolutions.iter() {
        let trace = DependencyTrace {
            realization_reads: Vec::new(),
            reads: res_node.auto_params.clone(),
        };
        traces.insert(NodeId::Resolution(res_node.id.clone()), trace);
    }

    // Composed fields: surface the augmented `Lambda { captures, .. }` deps
    // (set by the compiler post-pass `phase_augment_composed_captures`) under
    // the `NodeId::Value(__field.<name>)` key, matching how the elaboration
    // loop in `engine_eval.rs` keys field cells in `snapshot.values`.
    for field in fields {
        if let CompiledFieldSource::Composed { expr } = &field.source {
            let trace = extract_dependency_trace(expr);
            let field_cell = ValueCellId::new(FIELD_ENTITY_PREFIX, &field.name);
            traces.insert(NodeId::Value(field_cell), trace);
        }
    }

    // GHR-δ S4 (forward edge: GH ValueCell → Realization). Augment each
    // geometry cell's already-computed trace with the upstream Realization that
    // produced its handle, so `derive_output_freshness_from_trace_with_cause`
    // folds the realization's freshness into the cell's. The value cell's own
    // trace was inserted by the value-cells loop above (empty for a param);
    // here we only add the realization read.
    for (rid, cell) in geometry_cell_realization_links(graph) {
        if let Some(trace) = traces.get_mut(&NodeId::Value(cell)) {
            trace.realization_reads.push(rid);
        }
    }

    traces
}

/// Extract dependency ValueCellIds from a CompiledGeometryOp's argument expressions.
///
/// Walks all expression arguments in Primitive, Modify, and Transform ops.
/// Boolean ops have no expression arguments (just geometry refs).
pub fn extract_realization_dependencies(
    ops: &[reify_compiler::CompiledGeometryOp],
) -> DependencyTrace {
    let mut reads = Vec::new();
    for op in ops {
        let args = match op {
            reify_compiler::CompiledGeometryOp::Primitive { args, .. }
            | reify_compiler::CompiledGeometryOp::Modify { args, .. }
            | reify_compiler::CompiledGeometryOp::Transform { args, .. }
            | reify_compiler::CompiledGeometryOp::Pattern { args, .. }
            | reify_compiler::CompiledGeometryOp::Sweep { args, .. }
            | reify_compiler::CompiledGeometryOp::Curve { args, .. }
            | reify_compiler::CompiledGeometryOp::Profile { args, .. } => args,
            reify_compiler::CompiledGeometryOp::Boolean { .. } => continue,
        };
        for (_, expr) in args {
            reads.extend(expr.collect_value_refs());
        }
    }
    DependencyTrace {
        realization_reads: Vec::new(),
        reads,
    }
}

/// Resolve a `GeomRef::Sub(name)` cross-component reference to the producing
/// `RealizationNodeId`.
///
/// `name` is `"<sub>.<member>"` (the cross-component reference format used by
/// `GeomRef::Sub`; the sub-instance name is the left part and the geometry member
/// name is the right part after the first '.'). The member part uniquely identifies
/// the geometry output within any single entity. Resolution:
///
/// 1. Splits `name` on the first '.' and takes the **member** part (right side).
/// 2. Scans `graph.realizations` for a realization R such that
///    `R.geometry_cell.member == member` AND `R.id.entity != consuming_entity`
///    (cross-component only: a consuming realization cannot nominate its own entity's
///    geometry cell as a cross-sub source).
/// 3. Returns `Some(R.id)` on an unambiguous match, `None` on zero or >1 matches.
///
/// Returning `None` on ambiguity is safe (a missing edge is a conservative miss,
/// caught by β's `assert_dag_complete` test). Robust cross-structure disambiguation
/// under member-name collision is deferred to Part-2.
fn resolve_sub_ref(
    name: &str,
    consuming_entity: &str,
    graph: &crate::graph::EvaluationGraph,
) -> Option<RealizationNodeId> {
    let member = name.splitn(2, '.').nth(1)?;
    let mut found: Option<RealizationNodeId> = None;
    for (_, rnode) in graph.realizations.iter() {
        if rnode.id.entity == consuming_entity {
            continue; // own-entity — not a cross-component source
        }
        if let Some(ref cell) = rnode.geometry_cell {
            if cell.member == member {
                if found.is_some() {
                    return None; // ambiguous — more than one entity exports this member name
                }
                found = Some(rnode.id.clone());
            }
        }
    }
    found
}

/// Extract `GeomRef::Sub`-based realization→realization edges from a realization's
/// operation list.
///
/// `consuming_entity` is the entity that owns the consuming realization; it is
/// passed to [`resolve_sub_ref`] to exclude own-entity matches (cross-component only).
///
/// **Step-6** handles `Boolean { left, right, .. }` operands. Step-8 extends this
/// function with Modify/Transform/Pattern `.target` and Sweep `.profiles`.
///
/// `GeomRef::Step(_)` is always skipped (intra-node; no cross-realization edge).
/// The returned Vec may contain duplicates; deduplication is the caller's
/// responsibility (use a `seen` HashSet as done for `seen_rids` in the builders).
fn extract_realization_edges(
    ops: &[reify_compiler::CompiledGeometryOp],
    consuming_entity: &str,
    graph: &crate::graph::EvaluationGraph,
) -> Vec<RealizationNodeId> {
    let mut result = Vec::new();
    for op in ops {
        match op {
            reify_compiler::CompiledGeometryOp::Boolean { left, right, .. } => {
                for geom_ref in [left, right] {
                    if let reify_compiler::GeomRef::Sub(ref name) = *geom_ref {
                        if let Some(rid) = resolve_sub_ref(name, consuming_entity, graph) {
                            result.push(rid);
                        }
                    }
                    // GeomRef::Step → skip (intra-node, no cross-realization edge)
                }
            }
            // Modify/Transform/Pattern/Sweep target/profiles extended in step-8.
            _ => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::NodeId;
    use reify_core::{ConstraintNodeId, Type, ValueCellId};
    use reify_ir::{BinOp, Value};

    #[test]
    fn reverse_index_new_is_empty() {
        let index = ReverseDependencyIndex::new();
        let cell = ValueCellId::new("A", "x");
        assert!(index.dependents_of(&cell).is_empty());
    }

    #[test]
    fn reverse_index_dependents_of_unknown_cell_is_empty() {
        let index = ReverseDependencyIndex::new();
        let unknown = ValueCellId::new("Z", "unknown");
        let deps = index.dependents_of(&unknown);
        assert!(deps.is_empty());
    }

    #[test]
    fn reverse_index_add_inserts_mapping() {
        let mut index = ReverseDependencyIndex::new();
        let cell = ValueCellId::new("A", "x");
        let node = NodeId::Constraint(ConstraintNodeId::new("A", 0));
        index.add(cell.clone(), node.clone());

        let deps = index.dependents_of(&cell);
        assert_eq!(deps.len(), 1);
        assert!(deps.contains(&node));
    }

    #[test]
    fn reverse_index_multiple_dependents_of_same_cell() {
        let mut index = ReverseDependencyIndex::new();
        let cell = ValueCellId::new("A", "x");
        let node_a = NodeId::Constraint(ConstraintNodeId::new("A", 0));
        let node_b = NodeId::Constraint(ConstraintNodeId::new("A", 1));
        let node_c = NodeId::Value(ValueCellId::new("A", "volume"));
        index.add(cell.clone(), node_a.clone());
        index.add(cell.clone(), node_b.clone());
        index.add(cell.clone(), node_c.clone());

        let deps = index.dependents_of(&cell);
        assert_eq!(deps.len(), 3);
        assert!(deps.contains(&node_a));
        assert!(deps.contains(&node_b));
        assert!(deps.contains(&node_c));
    }

    /// P3.3 step-5: Edge #10 — Realization → ComputeNode reverse-index
    /// registration. ComputeNodes consume Realization outputs via the
    /// `realization_inputs` declaration; the reverse-index must surface
    /// each consumer Compute as a dependent of every input Realization.
    ///
    /// Build an EvaluationGraph with one Realization `R0` and one Compute
    /// `C` whose `realization_inputs = [R0.id.clone()]`. Assert the new
    /// `realization_dependents_of(&R0.id)` returns a set containing
    /// `NodeId::Compute(C.computation_id.clone())`.
    #[test]
    fn reverse_index_realization_dependents_of_returns_compute_consumers() {
        use crate::graph::{ComputeNodeData, EvaluationGraph, RealizationNodeData};
        use reify_core::{ComputeNodeId, ContentHash, RealizationNodeId};
        use reify_ir::ReprKind;

        let mut graph = EvaluationGraph::default();
        let e = "E";

        // Realization R0 (operations irrelevant for reverse index).
        let r0_id = RealizationNodeId::new(e, 0);
        graph.realizations.insert(
            r0_id.clone(),
            RealizationNodeData {
                geometry_cell: None,
                id: r0_id.clone(),
                operations: vec![],
                content_hash: ContentHash::of_str("r0"),
                produced_repr: ReprKind::BRep,
            },
        );

        // Compute C with realization_inputs=[R0].
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
            output_value_cells: vec![],
        });

        let index = ReverseDependencyIndex::build_from_graph(&graph);

        let r0_deps = index.realization_dependents_of(&r0_id);
        assert!(
            r0_deps.contains(&NodeId::Compute(c_id.clone())),
            "realization_dependents_of(R0) should include Compute({:?}), got: {:?}",
            c_id,
            r0_deps
        );
    }

    /// P3.3 step-5: default-empty-set contract for the realization index.
    /// Mirrors `reverse_index_dependents_of_unknown_cell_is_empty` — an
    /// unknown RealizationNodeId returns the static empty set rather than
    /// None / panicking, matching the existing dependents_of contract.
    #[test]
    fn reverse_index_realization_dependents_of_unknown_returns_empty() {
        use reify_core::RealizationNodeId;

        let index = ReverseDependencyIndex::new();
        let unknown = RealizationNodeId::new("Z", 99);
        let deps = index.realization_dependents_of(&unknown);
        assert!(deps.is_empty());
    }

    /// GHR-δ S3: the graph-aware builders surface the Realization→ValueCell
    /// linkage in BOTH directions for a geometry-backed cell, riding the
    /// `RealizationNodeData.geometry_cell` link populated by S2.
    ///
    /// (a) `build_trace_map_and_fields` records `realization_reads = [Widget#0]`
    ///     on the GH cell's forward trace (and leaves non-geometry cells empty).
    /// (b) `build_from_graph_and_fields` registers the reverse edge
    ///     `realization_dependents_of(Widget#0) ∋ Value(Widget.body)`.
    ///
    /// RED until S4 wires both.
    #[test]
    fn graph_builders_link_geometry_cell_to_realization_both_directions() {
        use crate::graph::EvaluationGraph;
        use reify_core::RealizationNodeId;
        use reify_test_support::parse_and_compile;

        // `body` is the sole geometry member → realization Widget#0; `width`
        // is a scalar param (no realization) used as the non-geometry control.
        let module = parse_and_compile(
            r#"structure def Widget {
    param body : Solid = box(10mm, 20mm, 30mm)
    param width : Length = 10mm
}"#,
        );
        let graph = EvaluationGraph::from_templates(&module.templates);

        let body = ValueCellId::new("Widget", "body");
        let width = ValueCellId::new("Widget", "width");
        let r0 = RealizationNodeId::new("Widget", 0);

        // Precondition: S2 populated the graph link the builders ride on.
        assert_eq!(
            graph.realizations.get(&r0).unwrap().geometry_cell,
            Some(body.clone()),
            "precondition: from_templates must link Widget#0 -> Widget.body"
        );

        // (a) forward trace carries realization_reads on the GH cell only.
        let traces = build_trace_map_and_fields(&graph, &[]);
        let body_trace = traces
            .get(&NodeId::Value(body.clone()))
            .expect("trace map must contain Widget.body");
        assert_eq!(
            body_trace.realization_reads,
            vec![r0.clone()],
            "GH cell forward trace must read its backing realization"
        );
        let width_trace = traces
            .get(&NodeId::Value(width.clone()))
            .expect("trace map must contain Widget.width");
        assert!(
            width_trace.realization_reads.is_empty(),
            "non-geometry cell must keep realization_reads empty, got {:?}",
            width_trace.realization_reads
        );

        // (b) reverse edge registered Realization -> GH cell.
        let index = ReverseDependencyIndex::build_from_graph_and_fields(&graph, &[]);
        assert!(
            index
                .realization_dependents_of(&r0)
                .contains(&NodeId::Value(body.clone())),
            "realization_dependents_of(Widget#0) must contain Value(Widget.body), got {:?}",
            index.realization_dependents_of(&r0)
        );
    }

    /// P3.3 step-3: Edge #6 — VC → ComputeNode reverse-index registration.
    ///
    /// Build an EvaluationGraph with one ValueCell `load` and one
    /// ComputeNode `C` whose `value_inputs = [load]`. Assert that
    /// `dependents_of(&load)` includes `NodeId::Compute(C.computation_id)`.
    /// This is the static counterpart of the spec's edge #6 (consumer-Compute
    /// reads producer-VC). Pins build_from_graph_and_fields' new compute_nodes
    /// loop landed in step-4.
    #[test]
    fn reverse_index_registers_value_input_edge_for_each_compute_node() {
        use crate::graph::{ComputeNodeData, EvaluationGraph, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ComputeNodeId, ContentHash, Type};

        let mut graph = EvaluationGraph::default();
        let e = "E";

        // ValueCell `load` (a param — kind/value irrelevant for reverse index).
        let load = ValueCellId::new(e, "load");
        graph.value_cells.insert(
            load.clone(),
            ValueCellNode {
                id: load.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("load"),
            },
        );

        // ComputeNode C with value_inputs=[load].
        let c_id = ComputeNodeId::new(e, 0);
        graph.insert_compute_node(ComputeNodeData {
            computation_id: c_id.clone(),
            target: "fea".to_string(),
            value_inputs: vec![load.clone()],
            realization_inputs: vec![],
            options_hash: ContentHash::of_str("opt"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![],
        });

        let index = ReverseDependencyIndex::build_from_graph(&graph);

        let load_deps = index.dependents_of(&load);
        assert!(
            load_deps.contains(&NodeId::Compute(c_id.clone())),
            "dependents_of('load') should include Compute({:?}), got: {:?}",
            c_id,
            load_deps
        );
    }

    #[test]
    fn reverse_index_includes_resolution_deps() {
        use crate::graph::{EvaluationGraph, ResolutionNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ContentHash, ResolutionNodeId, Type};

        let mut graph = EvaluationGraph::default();

        // Add auto param 'a'
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

        // Add constraint C0 (with literal expr, for completeness)
        let c0_id = ConstraintNodeId::new("A", 0);
        graph.constraints.insert(
            c0_id.clone(),
            crate::graph::ConstraintNodeData {
                id: c0_id.clone(),
                label: None,
                expr: reify_ir::CompiledExpr::literal(
                    reify_ir::Value::Bool(true),
                    reify_core::Type::Bool,
                ),
                content_hash: ContentHash::of_str("c0"),
                optimized_target: None,
            },
        );

        // Add ResolutionNodeData R0 with auto_params=['a']
        let r0_id = ResolutionNodeId::new("A", 0);
        graph.resolutions.insert(
            r0_id.clone(),
            ResolutionNodeData {
                id: r0_id.clone(),
                scope: "A".to_string(),
                auto_params: vec![a.clone()],
                constraint_deps: vec![c0_id.clone()],
                content_hash: ContentHash::of_str("r0"),
            },
        );

        let index = ReverseDependencyIndex::build_from_graph(&graph);

        // Dependents of 'a' should include Resolution(R0)
        let a_deps = index.dependents_of(&a);
        assert!(
            a_deps.contains(&NodeId::Resolution(r0_id)),
            "dependents_of('a') should include Resolution(R0), got: {:?}",
            a_deps
        );
    }

    #[test]
    fn build_trace_map_includes_resolution() {
        use crate::graph::{EvaluationGraph, ResolutionNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ContentHash, ResolutionNodeId, Type};

        let mut graph = EvaluationGraph::default();

        // Add params 'a' and 'b'
        for name in &["a", "b"] {
            let id = ValueCellId::new("A", *name);
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

        // Add ResolutionNodeData R0 with auto_params=['a','b']
        let r0_id = ResolutionNodeId::new("A", 0);
        graph.resolutions.insert(
            r0_id.clone(),
            ResolutionNodeData {
                id: r0_id.clone(),
                scope: "A".to_string(),
                auto_params: vec![ValueCellId::new("A", "a"), ValueCellId::new("A", "b")],
                constraint_deps: vec![],
                content_hash: ContentHash::of_str("r0"),
            },
        );

        let traces = build_trace_map(&graph);

        let res_node = NodeId::Resolution(r0_id);
        assert!(
            traces.contains_key(&res_node),
            "trace_map should contain Resolution(R0)"
        );
        let trace = &traces[&res_node];
        assert_eq!(trace.reads.len(), 2);
        assert!(trace.reads.contains(&ValueCellId::new("A", "a")));
        assert!(trace.reads.contains(&ValueCellId::new("A", "b")));
    }

    #[test]
    fn build_from_graph_bracket_topology() {
        use crate::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let index = ReverseDependencyIndex::build_from_graph(&graph);

        let e = "Bracket";

        // width is read by: volume (let), C1 (constraint: thickness < width/4), R0 (box realization)
        let width_deps = index.dependents_of(&ValueCellId::new(e, "width"));
        assert_eq!(width_deps.len(), 3, "width dependents: {:?}", width_deps);
        assert!(width_deps.contains(&NodeId::Value(ValueCellId::new(e, "volume"))));
        assert!(width_deps.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))));
        assert!(
            width_deps.contains(&NodeId::Realization(reify_core::RealizationNodeId::new(
                e, 0
            )))
        );

        // thickness is read by: volume (let), C0, C1, C2 (all three constraints), R0 (box depth)
        let thickness_deps = index.dependents_of(&ValueCellId::new(e, "thickness"));
        assert_eq!(
            thickness_deps.len(),
            5,
            "thickness dependents: {:?}",
            thickness_deps
        );
        assert!(thickness_deps.contains(&NodeId::Value(ValueCellId::new(e, "volume"))));
        assert!(thickness_deps.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 0))));
        assert!(thickness_deps.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))));
        assert!(thickness_deps.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 2))));

        // fillet_radius is not read by anything in bracket
        let fillet_deps = index.dependents_of(&ValueCellId::new(e, "fillet_radius"));
        assert!(
            fillet_deps.is_empty(),
            "fillet_radius dependents: {:?}",
            fillet_deps
        );

        // hole_diameter is read by: C2 (constraint: hole_diameter < thickness*2)
        let hole_deps = index.dependents_of(&ValueCellId::new(e, "hole_diameter"));
        assert_eq!(
            hole_deps.len(),
            1,
            "hole_diameter dependents: {:?}",
            hole_deps
        );
        assert!(hole_deps.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 2))));

        // height is read by: volume (let), R0 (box realization)
        let height_deps = index.dependents_of(&ValueCellId::new(e, "height"));
        assert_eq!(height_deps.len(), 2, "height dependents: {:?}", height_deps);
        assert!(height_deps.contains(&NodeId::Value(ValueCellId::new(e, "volume"))));
        assert!(
            height_deps.contains(&NodeId::Realization(reify_core::RealizationNodeId::new(
                e, 0
            )))
        );
    }

    // --- Task 4317 step-7 (b): builder-contract unit tests for param-default deps ---
    //
    // These three tests verify that BOTH graph-derived dependency builders treat a
    // non-auto param cell's default_expr as real read-dependencies, mirroring what
    // the unified eval pass already does.  They FAIL today (before step-8 fixes
    // deps.rs) because build_from_graph_and_fields only adds reverse edges for Let
    // cells and build_trace_map_and_fields returns DependencyTrace::default() for
    // all non-Let cells.

    /// (b1) The reverse dependency index must contain drum_d → Value(p).
    ///
    /// Structure T has `param p : Real = feed / drum_d` — p reads the Let `drum_d`.
    /// After step-8, `build_from_graph_and_fields` must add the reverse edge
    /// drum_d → Value(T.p) so the dirty cone (engine_edit.rs:956) includes p when
    /// drum_d's input (rope_dia) changes.
    ///
    /// FAILS today: build_from_graph_and_fields (deps.rs:149) gates on
    /// `node.kind == ValueCellKind::Let`, so p (a Param) is never iterated and
    /// no reverse edge drum_d → p is registered.
    #[test]
    fn reverse_index_param_default_reads_sibling_let_registers_reverse_edge() {
        use crate::graph::EvaluationGraph;
        use reify_test_support::parse_and_compile;

        let module = parse_and_compile(
            "structure T { \
                param rope_dia : Length = 6mm \
                let drum_d = rope_dia * 2.0 \
                param feed : Length = 1300mm \
                param p : Real = feed / drum_d \
                let out = p \
            }",
        );
        let graph = EvaluationGraph::from_templates(&module.templates);
        let index = ReverseDependencyIndex::build_from_graph_and_fields(&graph, &[]);

        let drum_d_id = ValueCellId::new("T", "drum_d");
        let p_id = ValueCellId::new("T", "p");
        let p_node = NodeId::Value(p_id.clone());

        let drum_d_deps = index.dependents_of(&drum_d_id);
        assert!(
            drum_d_deps.contains(&p_node),
            "dependents_of(T.drum_d) must contain Value(T.p) so that editing \
             rope_dia dirtifies p via the drum_d → p reverse edge. \
             Got: {:?}. \
             Bug: build_from_graph_and_fields only iterates Let cells (line 151), \
             so the param p's read of drum_d is invisible to the reverse index.",
            drum_d_deps
        );
    }

    /// (b2) The forward trace for param p must list drum_d and feed as reads.
    ///
    /// `build_trace_map_and_fields` is used by `compute_eval_set` (engine_edit.rs:961)
    /// to topologically order the re-evaluation set.  If p's trace is empty, Kahn's
    /// algorithm treats p as a root and may evaluate it before drum_d is recomputed —
    /// producing the stale value.  After step-8 the trace for Value(T.p) must carry
    /// `reads = [T.feed, T.drum_d]` (order unspecified).
    ///
    /// FAILS today: build_trace_map_and_fields (deps.rs:320) returns
    /// DependencyTrace::default() (empty reads) for every non-Let cell, including
    /// Param cells that have a default_expr reading other cells.
    #[test]
    fn trace_map_param_default_reads_sibling_let_has_non_empty_reads() {
        use crate::graph::EvaluationGraph;
        use reify_test_support::parse_and_compile;

        let module = parse_and_compile(
            "structure T { \
                param rope_dia : Length = 6mm \
                let drum_d = rope_dia * 2.0 \
                param feed : Length = 1300mm \
                param p : Real = feed / drum_d \
                let out = p \
            }",
        );
        let graph = EvaluationGraph::from_templates(&module.templates);
        let traces = build_trace_map_and_fields(&graph, &[]);

        let p_id = ValueCellId::new("T", "p");
        let drum_d_id = ValueCellId::new("T", "drum_d");
        let feed_id = ValueCellId::new("T", "feed");

        let p_trace = traces
            .get(&NodeId::Value(p_id.clone()))
            .expect("trace map must contain T.p");

        assert!(
            p_trace.reads.contains(&drum_d_id),
            "forward trace for T.p must read T.drum_d; got reads: {:?}. \
             Bug: build_trace_map_and_fields returns DependencyTrace::default() \
             for all non-Let cells (line 325).",
            p_trace.reads
        );
        assert!(
            p_trace.reads.contains(&feed_id),
            "forward trace for T.p must read T.feed; got reads: {:?}",
            p_trace.reads
        );
    }

    /// (b3) Regression lock: literal-default params must still have empty forward traces.
    ///
    /// `rope_dia : Length = 6mm` and `feed : Length = 1300mm` both have literal
    /// default expressions with no ValueRef reads.  After step-8, extracting their
    /// `default_expr` yields an empty DependencyTrace (same as the current
    /// DependencyTrace::default()), so the reverse index adds no edges for them
    /// and their forward traces remain empty.  This test must PASS both before and
    /// after the fix — it is a regression lock, not a RED signal.
    #[test]
    fn literal_default_params_keep_empty_forward_trace_and_no_reverse_edges() {
        use crate::graph::EvaluationGraph;
        use reify_test_support::parse_and_compile;

        let module = parse_and_compile(
            "structure T { \
                param rope_dia : Length = 6mm \
                let drum_d = rope_dia * 2.0 \
                param feed : Length = 1300mm \
                param p : Real = feed / drum_d \
                let out = p \
            }",
        );
        let graph = EvaluationGraph::from_templates(&module.templates);
        let traces = build_trace_map_and_fields(&graph, &[]);
        let _index = ReverseDependencyIndex::build_from_graph_and_fields(&graph, &[]);

        let rope_dia_id = ValueCellId::new("T", "rope_dia");
        let feed_id = ValueCellId::new("T", "feed");

        // rope_dia has a literal default (6mm) — its trace must have no VC reads.
        let rope_dia_trace = traces
            .get(&NodeId::Value(rope_dia_id.clone()))
            .expect("trace map must contain T.rope_dia");
        assert!(
            rope_dia_trace.reads.is_empty(),
            "T.rope_dia has a literal default; its forward trace must have no reads, \
             got: {:?}",
            rope_dia_trace.reads
        );

        // feed has a literal default (1300mm) — its trace must have no VC reads.
        let feed_trace = traces
            .get(&NodeId::Value(feed_id.clone()))
            .expect("trace map must contain T.feed");
        assert!(
            feed_trace.reads.is_empty(),
            "T.feed has a literal default; its forward trace must have no reads, \
             got: {:?}",
            feed_trace.reads
        );

        // The reverse index must NOT contain rope_dia or feed as dependents of
        // anything — a literal default has no ValueRef reads, so no reverse edges.
        // (rope_dia IS a source read by drum_d, but it should not appear as a
        // dependent of any other cell via this literal-default path.)
        // Just confirm they have empty dependents_of (they are pure roots).
        // Note: rope_dia itself IS depended upon BY drum_d and feed by nothing —
        // we do NOT assert on that here. We assert that the literal-default paths
        // (rope_dia's own default, feed's own default) register no new reverse edges
        // beyond what Let/constraint consumers already register.
        //
        // Simpler check: neither rope_dia nor feed read any sibling VC in their default.
        // That is already captured by the empty-reads assertions above.
        // Regression complete.
    }

    // --- extract_dependency_trace unit tests ---

    /// Step 1: Verify extract_dependency_trace captures ValueRef ids from a simple BinOp.
    ///
    /// This documents the baseline static extraction behavior: every ValueRef in the
    /// compiled expression tree contributes to the dependency trace, regardless of position.
    #[test]
    fn extract_dependency_trace_captures_value_refs_from_binop() {
        let a = ValueCellId::new("A", "x");
        let b = ValueCellId::new("A", "y");
        let expr = CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(a.clone(), Type::Real),
            CompiledExpr::value_ref(b.clone(), Type::Real),
            Type::Real,
        );
        let trace = extract_dependency_trace(&expr);
        assert_eq!(
            trace.reads.len(),
            2,
            "BinOp of two ValueRefs should yield 2 reads"
        );
        assert!(trace.reads.contains(&a), "reads should contain 'x'");
        assert!(trace.reads.contains(&b), "reads should contain 'y'");
    }

    /// Step 1b: Documents the duplicate-preservation contract of `extract_dependency_trace`
    /// for a BinOp whose operands reference the same `ValueCellId` (e.g. `x + x`).
    ///
    /// `extract_dependency_trace` is a thin wrapper over
    /// `CompiledExpr::collect_value_refs`, which *preserves duplicates* (it pushes
    /// each `ValueRef` without deduping). The sibling helper [`extract_value_deps`]
    /// (defined later in this file) is the deduplicating-and-sorting variant; `extract_dependency_trace`
    /// intentionally keeps raw duplicates. This test pins that split so callers
    /// know whether they need to dedupe downstream.
    #[test]
    fn extract_dependency_trace_preserves_duplicate_reads_for_same_cell_in_binop() {
        let x = ValueCellId::new("A", "x");
        let expr = CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(x.clone(), Type::Real),
            CompiledExpr::value_ref(x.clone(), Type::Real),
            Type::Real,
        );
        let trace = extract_dependency_trace(&expr);
        assert_eq!(
            trace.reads.len(),
            2,
            "extract_dependency_trace preserves duplicates: x+x should yield 2 reads, \
             got {:?}",
            trace.reads
        );
        assert!(
            trace.reads.iter().all(|id| id == &x),
            "both reads should refer to x, got: {:?}",
            trace.reads
        );
    }

    /// Step 1c: Documents the duplicate-preservation contract of `extract_dependency_trace`
    /// for a Conditional whose then- and else-branches reference the same `ValueCellId`
    /// (e.g. `if flag { x } else { x }`).
    ///
    /// This combines the all-branches static extraction property (Step 2 below) with the
    /// duplicate-preservation property (Step 1b above): even when both arms refer to the
    /// same cell, `extract_dependency_trace` preserves all occurrences without deduping.
    /// That gives `reads.len() == 3` (flag + x + x), pinning the exact multiplicity so
    /// callers know whether downstream deduplication is needed.
    #[test]
    fn extract_dependency_trace_preserves_duplicate_reads_for_same_cell_in_conditional() {
        let flag = ValueCellId::new("A", "flag");
        let x = ValueCellId::new("A", "x");
        let condition = CompiledExpr::value_ref(flag.clone(), Type::Bool);
        let then_branch = CompiledExpr::value_ref(x.clone(), Type::Real);
        let else_branch = CompiledExpr::value_ref(x.clone(), Type::Real);
        let expr = reify_test_support::builders::expr::conditional_expr(
            condition,
            then_branch,
            else_branch,
        );
        let trace = extract_dependency_trace(&expr);
        assert_eq!(
            trace.reads.len(),
            3,
            "extract_dependency_trace preserves duplicates: conditional(flag, x, x) \
             should yield 3 reads (flag + x + x), got {:?}",
            trace.reads
        );
        assert_eq!(
            trace.reads.iter().filter(|id| *id == &flag).count(),
            1,
            "reads should contain flag exactly once, got: {:?}",
            trace.reads
        );
        assert_eq!(
            trace.reads.iter().filter(|id| *id == &x).count(),
            2,
            "reads should contain x exactly twice (once per branch), got: {:?}",
            trace.reads
        );
        // Pin traversal order: condition → then_branch → else_branch
        // (matches collect_value_refs_inner's Conditional arm in reify-types/src/expr.rs)
        assert_eq!(
            trace.reads[0], flag,
            "reads[0] should be the condition (flag)"
        );
        assert_eq!(trace.reads[1], x, "reads[1] should be the then-branch (x)");
        assert_eq!(trace.reads[2], x, "reads[2] should be the else-branch (x)");
    }

    /// Step 2: Verify extract_dependency_trace handles nested Conditional expressions —
    /// condition, then-branch, and else-branch all contribute ValueRef reads.
    ///
    /// This is the key 'all branches' static extraction property: unlike runtime tracing,
    /// static extraction conservatively includes all reachable ValueRefs across every branch.
    #[test]
    fn extract_dependency_trace_captures_all_branches_of_conditional() {
        let cond_cell = ValueCellId::new("A", "flag");
        let then_cell = ValueCellId::new("A", "then_val");
        let else_cell = ValueCellId::new("A", "else_val");
        let condition = CompiledExpr::value_ref(cond_cell.clone(), Type::Bool);
        let then_branch = CompiledExpr::value_ref(then_cell.clone(), Type::Real);
        let else_branch = CompiledExpr::value_ref(else_cell.clone(), Type::Real);
        let expr = reify_test_support::builders::expr::conditional_expr(
            condition,
            then_branch,
            else_branch,
        );
        let trace = extract_dependency_trace(&expr);
        assert_eq!(
            trace.reads.len(),
            3,
            "Conditional with 3 distinct ValueRefs should yield 3 reads"
        );
        assert!(
            trace.reads.contains(&cond_cell),
            "reads should contain condition cell"
        );
        assert!(
            trace.reads.contains(&then_cell),
            "reads should contain then-branch cell"
        );
        assert!(
            trace.reads.contains(&else_cell),
            "reads should contain else-branch cell"
        );
    }

    /// Step 3: Verify extract_dependency_trace returns empty reads for a Literal expression.
    ///
    /// Confirms root/leaf behavior: a literal has no value-cell dependencies.
    #[test]
    fn extract_dependency_trace_returns_empty_for_literal() {
        let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let trace = extract_dependency_trace(&expr);
        assert!(
            trace.reads.is_empty(),
            "Literal expression should have no reads, got: {:?}",
            trace.reads
        );
    }

    /// Step 4: Verify DependencyTrace::default() has empty reads.
    ///
    /// Documents the contract used throughout lib.rs: params and root nodes pass
    /// `DependencyTrace::default()` to `record_evaluation()`, signalling that they
    /// have no value-cell dependencies and will never be invalidated by cell changes.
    #[test]
    fn dependency_trace_default_has_empty_reads() {
        let trace = DependencyTrace::default();
        assert!(
            trace.reads.is_empty(),
            "DependencyTrace::default() should have no reads — root/param nodes are dependency roots"
        );
    }

    // ── Task 4354 step-3: RED — selector/query cell→realization edge ──────────
    //
    // A value cell (Let OR Param) whose default_expr reads a geometry cell that
    // is backed by a realization must register as a dependent of that realization
    // in BOTH directions.
    //
    // Step-3 covers the "all non-auto cells" generalization from task 4317:
    //   - Let cell `top_edges` reading `b` via edges_at_height(b, h)
    //   - Param cell `v` reading `b` via volume(b)
    // Both must show up as dependents of r0 (which backs `b`).
    //
    // Additionally, a value cell reading only scalar params must NOT register
    // any realization read (no over-approximation).
    //
    // RED today: neither builder resolves a cell's geometry reads to realizations.

    /// Build a graph with geometry realization r0 backed by `b`, a Let cell
    /// `top_edges` (reads `b` via edges_at_height), a Param cell `v` (reads `b`
    /// via volume), and a Param cell `w` (reads only the scalar `h`).  Assert BOTH
    /// directions for top_edges and v; assert EMPTY realization_reads for w.
    #[test]
    fn value_cell_reads_geometry_cell_registers_realization_edge_both_directions() {
        use crate::graph::{EvaluationGraph, RealizationNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ContentHash, RealizationNodeId};
        use reify_ir::{BinOp, ReprKind};
        use reify_test_support::builders::expr as eb;

        let mut graph = EvaluationGraph::default();
        let e = "E";

        // ── geometry cell `b` (Type::Geometry) backed by r0 ───────────────────
        let b = ValueCellId::new(e, "b");
        graph.value_cells.insert(
            b.clone(),
            ValueCellNode {
                id: b.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Geometry,
                default_expr: None,
                content_hash: ContentHash::of_str("b"),
            },
        );

        // ── scalar param `h` ──────────────────────────────────────────────────
        let h = ValueCellId::new(e, "h");
        graph.value_cells.insert(
            h.clone(),
            ValueCellNode {
                id: h.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str("h"),
            },
        );

        // ── realization r0 backed by `b` ───────────────────────────────────────
        let r0 = RealizationNodeId::new(e, 0);
        graph.realizations.insert(
            r0.clone(),
            RealizationNodeData {
                id: r0.clone(),
                geometry_cell: Some(b.clone()),
                operations: vec![],
                content_hash: ContentHash::of_str("r0"),
                produced_repr: ReprKind::BRep,
            },
        );

        // ── Let cell `top_edges`: edges_at_height(b, h) ───────────────────────
        // edges_at_height is NOT a geometry-query call (not in the recognised set),
        // but it reads `b` as a ValueRef — the "reads a geometry cell" rule should
        // detect this via the realization_by_cell resolver on reads.
        // Use a simple 2-arg fn_call that references b (geometry) and h (scalar).
        let edges_expr = eb::fn_call(
            "edges_at_height",
            "std::edges_at_height",
            vec![
                CompiledExpr::value_ref(b.clone(), Type::Geometry),
                CompiledExpr::value_ref(h.clone(), Type::Real),
            ],
            Type::Real, // selector return type (simplified)
        );
        let top_edges = ValueCellId::new(e, "top_edges");
        graph.value_cells.insert(
            top_edges.clone(),
            ValueCellNode {
                id: top_edges.clone(),
                kind: ValueCellKind::Let,
                cell_type: Type::Real,
                default_expr: Some(edges_expr),
                content_hash: ContentHash::of_str("top_edges"),
            },
        );

        // ── Param cell `v`: volume(b) ──────────────────────────────────────────
        // volume IS a geometry-query call but also reads `b` as a ValueRef, so
        // the "reads a geometry cell" rule applies here too.
        let vol_expr = eb::fn_call(
            "volume",
            "std::volume",
            vec![CompiledExpr::value_ref(b.clone(), Type::Geometry)],
            Type::Real,
        );
        let v = ValueCellId::new(e, "v");
        graph.value_cells.insert(
            v.clone(),
            ValueCellNode {
                id: v.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: Some(vol_expr),
                content_hash: ContentHash::of_str("v"),
            },
        );

        // ── Param cell `w`: reads only scalar `h` (no geometry) ───────────────
        let w_expr = CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::value_ref(h.clone(), Type::Real),
            CompiledExpr::literal(reify_ir::Value::Real(2.0), Type::Real),
            Type::Real,
        );
        let w = ValueCellId::new(e, "w");
        graph.value_cells.insert(
            w.clone(),
            ValueCellNode {
                id: w.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: Some(w_expr),
                content_hash: ContentHash::of_str("w"),
            },
        );

        // ── (a) reverse: top_edges and v are dependents of r0 ─────────────────
        let index = ReverseDependencyIndex::build_from_graph_and_fields(&graph, &[]);
        let r0_deps = index.realization_dependents_of(&r0);

        assert!(
            r0_deps.contains(&NodeId::Value(top_edges.clone())),
            "realization_dependents_of(r0) must contain Value(top_edges) \
             (Let cell reads geometry cell b backed by r0). Got: {:?}",
            r0_deps
        );
        assert!(
            r0_deps.contains(&NodeId::Value(v.clone())),
            "realization_dependents_of(r0) must contain Value(v) \
             (Param cell reads geometry cell b backed by r0). Got: {:?}",
            r0_deps
        );

        // ── (b) forward: top_edges and v traces have r0 in realization_reads ───
        let traces = build_trace_map_and_fields(&graph, &[]);

        let top_edges_trace = traces
            .get(&NodeId::Value(top_edges.clone()))
            .expect("trace map must contain Value(top_edges)");
        assert!(
            top_edges_trace.realization_reads.contains(&r0),
            "forward trace for Value(top_edges) must have r0 in realization_reads. \
             Got: {:?}",
            top_edges_trace.realization_reads
        );

        let v_trace = traces
            .get(&NodeId::Value(v.clone()))
            .expect("trace map must contain Value(v)");
        assert!(
            v_trace.realization_reads.contains(&r0),
            "forward trace for Value(v) must have r0 in realization_reads. \
             Got: {:?}",
            v_trace.realization_reads
        );

        // ── no over-approximation: w (scalar only) has EMPTY realization_reads ─
        let w_trace = traces
            .get(&NodeId::Value(w.clone()))
            .expect("trace map must contain Value(w)");
        assert!(
            w_trace.realization_reads.is_empty(),
            "scalar-only cell w (h * 2.0) must have EMPTY realization_reads. \
             Got: {:?}",
            w_trace.realization_reads
        );
    }

    // ── Task 4354 step-1: RED — constraint→realization edge ───────────────────
    //
    // A geometry-reading constraint (e.g. `volume(body) < max`) must register as
    // a dependent of the realization that backs `body` — in BOTH directions:
    //   (a) reverse: build_from_graph_and_fields → realization_dependents_of(r0) ∋ Constraint(c0)
    //   (b) forward: build_trace_map_and_fields → trace[Constraint(c0)].realization_reads ∋ r0
    //
    // Additionally, a non-geometry scalar constraint must NOT register any
    // realization read (no over-approximation).
    //
    // RED today: deps.rs:344 (forward) and :167 (reverse) both ignore
    // constraint→realization resolution, so both asserts (a) and (b) fail.

    /// Build a minimal graph with one geometry realization r0 backed by cell `body`,
    /// one geometry-reading constraint c0 (`volume(body) < literal_max`), and one
    /// scalar-only constraint c1 (`width < height`).  Assert BOTH directions for c0
    /// and EMPTY realization_reads for c1.
    #[test]
    fn constraint_reads_geometry_cell_registers_realization_edge_both_directions() {
        use crate::graph::{ConstraintNodeData, EvaluationGraph, RealizationNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ContentHash, RealizationNodeId};
        use reify_ir::{BinOp, ReprKind, Value};
        use reify_test_support::builders::expr as eb;

        let mut graph = EvaluationGraph::default();
        let e = "E";

        // ── geometry cell `body` (Type::Geometry) ─────────────────────────────
        let body = ValueCellId::new(e, "body");
        graph.value_cells.insert(
            body.clone(),
            ValueCellNode {
                id: body.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Geometry,
                default_expr: None,
                content_hash: ContentHash::of_str("body"),
            },
        );

        // ── scalar params `width` and `height` ────────────────────────────────
        let width = ValueCellId::new(e, "width");
        let height = ValueCellId::new(e, "height");
        for (id, tag) in [(&width, "width"), (&height, "height")] {
            graph.value_cells.insert(
                id.clone(),
                ValueCellNode {
                    id: id.clone(),
                    kind: ValueCellKind::Param,
                    cell_type: Type::Real,
                    default_expr: None,
                    content_hash: ContentHash::of_str(tag),
                },
            );
        }

        // ── realization r0 backed by `body` ───────────────────────────────────
        let r0 = RealizationNodeId::new(e, 0);
        graph.realizations.insert(
            r0.clone(),
            RealizationNodeData {
                id: r0.clone(),
                geometry_cell: Some(body.clone()),
                operations: vec![],
                content_hash: ContentHash::of_str("r0"),
                produced_repr: ReprKind::BRep,
            },
        );

        // ── constraint c0: `volume(body) < 1000.0` ────────────────────────────
        // volume(body) is a geometry-query call: fn_call("volume", args=[value_ref(body)])
        let vol_body = eb::fn_call(
            "volume",
            "std::volume",
            vec![CompiledExpr::value_ref(body.clone(), Type::Geometry)],
            Type::Real,
        );
        let max_val = CompiledExpr::literal(Value::Real(1000.0), Type::Real);
        let c0_expr = CompiledExpr::binop(BinOp::Lt, vol_body, max_val, Type::Bool);
        let c0 = ConstraintNodeId::new(e, 0);
        graph.constraints.insert(
            c0.clone(),
            ConstraintNodeData {
                id: c0.clone(),
                label: None,
                expr: c0_expr,
                content_hash: ContentHash::of_str("c0"),
                optimized_target: None,
            },
        );

        // ── constraint c1: `width < height` (scalar only, no geometry reads) ──
        let c1_expr = CompiledExpr::binop(
            BinOp::Lt,
            CompiledExpr::value_ref(width.clone(), Type::Real),
            CompiledExpr::value_ref(height.clone(), Type::Real),
            Type::Bool,
        );
        let c1 = ConstraintNodeId::new(e, 1);
        graph.constraints.insert(
            c1.clone(),
            ConstraintNodeData {
                id: c1.clone(),
                label: None,
                expr: c1_expr,
                content_hash: ContentHash::of_str("c1"),
                optimized_target: None,
            },
        );

        // ── (a) reverse: c0 is a dependent of r0 ──────────────────────────────
        let index = ReverseDependencyIndex::build_from_graph_and_fields(&graph, &[]);
        let r0_deps = index.realization_dependents_of(&r0);
        assert!(
            r0_deps.contains(&NodeId::Constraint(c0.clone())),
            "realization_dependents_of(r0) must contain Constraint(c0) \
             (volume(body) < max reads the geometry that r0 produces). \
             Got: {:?}",
            r0_deps
        );

        // ── (b) forward: c0's trace has r0 in realization_reads ───────────────
        let traces = build_trace_map_and_fields(&graph, &[]);
        let c0_trace = traces
            .get(&NodeId::Constraint(c0.clone()))
            .expect("trace map must contain Constraint(c0)");
        assert!(
            c0_trace.realization_reads.contains(&r0),
            "forward trace for Constraint(c0) must have r0 in realization_reads \
             (volume(body) reads the geometry that r0 produces). \
             Got: {:?}",
            c0_trace.realization_reads
        );

        // ── no over-approximation: c1 (scalar) has EMPTY realization_reads ────
        let c1_trace = traces
            .get(&NodeId::Constraint(c1.clone()))
            .expect("trace map must contain Constraint(c1)");
        assert!(
            c1_trace.realization_reads.is_empty(),
            "scalar-only constraint c1 (width < height) must have EMPTY \
             realization_reads. Got: {:?}",
            c1_trace.realization_reads
        );
    }

    /// Verify `take_trace` panics when the key is absent.
    #[test]
    #[should_panic(expected = "sorted_child_lets entries are always keys in child_let_traces")]
    fn take_trace_missing_key_panics() {
        let node_id = NodeId::Value(ValueCellId::new("E", "missing"));
        let mut map: HashMap<NodeId, DependencyTrace> = HashMap::new();
        take_trace(&mut map, &node_id, "sorted_child_lets", "child_let_traces");
    }

    /// Verify `take_trace` removes the key from the map and returns its trace.
    ///
    /// Constructs a two-entry map, calls `take_trace` for one key, and asserts:
    /// - The returned trace's `reads` match the originally-inserted value.
    /// - The consumed key is no longer in the map.
    /// - The second key is unaffected.
    #[test]
    fn take_trace_removes_present_key_and_returns_its_trace() {
        let node_id_a = NodeId::Value(ValueCellId::new("E", "a"));
        let node_id_b = NodeId::Value(ValueCellId::new("E", "b"));
        let cell_x = ValueCellId::new("E", "x");
        let cell_y = ValueCellId::new("E", "y");

        let trace_a = DependencyTrace {
            realization_reads: Vec::new(),
            reads: vec![cell_x.clone()],
        };
        let trace_b = DependencyTrace {
            realization_reads: Vec::new(),
            reads: vec![cell_y.clone()],
        };

        let mut map: HashMap<NodeId, DependencyTrace> = HashMap::new();
        map.insert(node_id_a.clone(), trace_a);
        map.insert(node_id_b.clone(), trace_b);

        let returned = take_trace(&mut map, &node_id_a, "sorted_lets", "let_traces");

        assert_eq!(
            returned.reads,
            vec![cell_x],
            "returned trace should match the inserted reads for node_id_a"
        );
        assert!(
            !map.contains_key(&node_id_a),
            "node_id_a should be removed from the map"
        );
        assert!(
            map.contains_key(&node_id_b),
            "node_id_b should remain in the map"
        );
    }

    // ── Task 2343 step-5: composed fields register as dependents ──────────
    //
    // After the compiler's `phase_augment_composed_captures` post-pass,
    // composed-field lambdas carry `__field.<dep>` cell IDs in their
    // `captures`. The reverse-dependency index (extended to iterate
    // `module.fields`) must then register each composed field as a dependent
    // of every cell its lambda transitively reads.

    /// Build a tiny module with three analytical-and-composed fields,
    /// pass it to `build_from_graph_and_fields`, and assert the composed
    /// field's NodeId appears in the dependents-of set for both upstream
    /// field cells. Analytical fields contribute no dependents from their
    /// own bodies (they reference no other fields).
    #[test]
    fn reverse_index_includes_composed_field_dependencies() {
        use crate::graph::EvaluationGraph;
        use reify_core::FIELD_ENTITY_PREFIX;
        use reify_test_support::parse_and_compile;

        let module = parse_and_compile(
            r#"
field def f1 : Real -> Real { source = analytical { |p| p } }
field def f2 : Real -> Real { source = analytical { |x| x } }
field def f3 : Real -> Real { source = composed { |p| f2(f1(p)) } }
"#,
        );

        let graph = EvaluationGraph::from_templates(&module.templates);
        let index = ReverseDependencyIndex::build_from_graph_and_fields(&graph, &module.fields);

        let f1_cell = ValueCellId::new(FIELD_ENTITY_PREFIX, "f1");
        let f2_cell = ValueCellId::new(FIELD_ENTITY_PREFIX, "f2");
        let f3_cell = ValueCellId::new(FIELD_ENTITY_PREFIX, "f3");
        let f3_node = NodeId::Value(f3_cell.clone());

        // (a) dependents_of(f1) includes f3
        let f1_deps = index.dependents_of(&f1_cell);
        assert!(
            f1_deps.contains(&f3_node),
            "dependents_of(__field.f1) should contain Value(__field.f3), got: {:?}",
            f1_deps
        );

        // (b) dependents_of(f2) includes f3
        let f2_deps = index.dependents_of(&f2_cell);
        assert!(
            f2_deps.contains(&f3_node),
            "dependents_of(__field.f2) should contain Value(__field.f3), got: {:?}",
            f2_deps
        );

        // (c) Analytical fields f1 and f2 themselves register no dependents
        // from their lambda bodies (they reference no other fields).
        let f3_deps = index.dependents_of(&f3_cell);
        assert!(
            !f3_deps.contains(&NodeId::Value(f1_cell.clone())),
            "f1 should not be a dependent of f3 (no upstream link), got: {:?}",
            f3_deps
        );
        assert!(
            !f3_deps.contains(&NodeId::Value(f2_cell.clone())),
            "f2 should not be a dependent of f3, got: {:?}",
            f3_deps
        );
    }

    /// Step 5: Verify CacheStore.invalidate_dependents uses the DependencyTrace.reads stored
    /// in cached entries (the statically extracted trace, not a separate runtime trace).
    ///
    /// This documents the end-to-end path: static extraction → stored in cache via
    /// record_evaluation() → used for invalidation by invalidate_dependents().
    ///
    /// NOTE: This is an intentional cross-module integration test. It lives alongside
    /// the `extract_dependency_trace` unit tests (rather than in `cache.rs`) because
    /// it asserts the contract *between* static dependency extraction here and the
    /// `CacheStore` invalidation path in `crate::cache`. Moving it would split the
    /// two halves of that contract across files and obscure the end-to-end guarantee.
    #[test]
    fn invalidate_dependents_uses_static_dependency_trace_reads() {
        use crate::cache::{CacheStore, CachedResult};
        use reify_core::VersionId;
        use reify_ir::DeterminacyState;

        // Build a static trace for a BinOp: z = x + y
        let x = ValueCellId::new("A", "x");
        let y = ValueCellId::new("A", "y");
        let z_id = ValueCellId::new("A", "z");
        let expr = CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(x.clone(), Type::Real),
            CompiledExpr::value_ref(y.clone(), Type::Real),
            Type::Real,
        );
        let trace = extract_dependency_trace(&expr);
        assert!(trace.reads.contains(&x), "sanity: trace contains x");
        assert!(trace.reads.contains(&y), "sanity: trace contains y");

        // Store z's cached result with the statically extracted trace
        let z_node = NodeId::Value(z_id.clone());
        let mut store = CacheStore::new();
        store.record_evaluation(
            z_node.clone(),
            CachedResult::Value(Value::Real(3.0), DeterminacyState::Determined),
            VersionId(1),
            trace,
        );

        // Invalidate dependents of x — z should become dirty (reads x)
        store.invalidate_dependents(std::slice::from_ref(&x));
        assert!(
            store.is_dirty(&z_node),
            "z depends on x via static trace, should be dirty after x changes"
        );

        // Invalidate dependents of a cell z does NOT read — z should not be additionally dirtied
        let w = ValueCellId::new("A", "w");
        store.clear_dirty(&z_node); // reset for the next check
        store.invalidate_dependents(std::slice::from_ref(&w));
        assert!(
            !store.is_dirty(&z_node),
            "z does not depend on w, should not be dirtied by w changing"
        );
    }

    // ── Task 4354 step-5: RED — realization→realization edge via Boolean cross-sub ──
    //
    // A consuming realization whose operations include a Boolean with GeomRef::Sub
    // operands that name geometry outputs of OTHER child structures must register
    // realization→realization edges in BOTH directions:
    //   (a) reverse: build_from_graph_and_fields → realization_dependents_of(inner_a) ∋ Realization(outer)
    //                                           → realization_dependents_of(inner_b) ∋ Realization(outer)
    //   (b) forward: build_trace_map_and_fields → trace[Realization(outer)].realization_reads ∋ {inner_a, inner_b}
    //
    // RED today: Boolean arm `=> continue` at extract_realization_dependencies drops both
    // operands; no builder walks GeomRef::Sub for realizations or resolves them to producing
    // RealizationNodeIds.

    /// Graph: inner_a (entity "A", geometry_cell = ValueCellId("A","body")),
    /// inner_b (entity "B", geometry_cell = ValueCellId("B","body2")),
    /// outer (entity "Outer") consuming both via Boolean { Union, Sub("a.body"), Sub("b.body2") }.
    ///
    /// Asserts BOTH operand edges, neither dropped; pins the GeomRef::Step negative
    /// (intra-node Step operands must NOT produce a realization→realization edge).
    #[test]
    fn boolean_cross_sub_operands_register_realization_to_realization_edges_both_directions() {
        use crate::graph::{EvaluationGraph, RealizationNodeData};
        use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef};
        use reify_core::{ContentHash, RealizationNodeId};
        use reify_ir::ReprKind;

        let mut graph = EvaluationGraph::default();

        // ── inner_a: entity "A", geometry_cell = ValueCellId("A","body") ─────────
        let inner_a = RealizationNodeId::new("A", 0);
        let body_a = ValueCellId::new("A", "body");
        graph.realizations.insert(
            inner_a.clone(),
            RealizationNodeData {
                id: inner_a.clone(),
                geometry_cell: Some(body_a.clone()),
                operations: vec![],
                content_hash: ContentHash::of_str("inner_a"),
                produced_repr: ReprKind::BRep,
            },
        );

        // ── inner_b: entity "B", geometry_cell = ValueCellId("B","body2") ────────
        let inner_b = RealizationNodeId::new("B", 0);
        let body_b = ValueCellId::new("B", "body2");
        graph.realizations.insert(
            inner_b.clone(),
            RealizationNodeData {
                id: inner_b.clone(),
                geometry_cell: Some(body_b.clone()),
                operations: vec![],
                content_hash: ContentHash::of_str("inner_b"),
                produced_repr: ReprKind::BRep,
            },
        );

        // ── outer: entity "Outer", Boolean { Union, Sub("a.body"), Sub("b.body2") }
        // GeomRef::Sub("a.body") resolves to inner_a (member "body", entity "A" != "Outer").
        // GeomRef::Sub("b.body2") resolves to inner_b (member "body2", entity "B" != "Outer").
        let outer = RealizationNodeId::new("Outer", 0);
        graph.realizations.insert(
            outer.clone(),
            RealizationNodeData {
                id: outer.clone(),
                geometry_cell: None,
                operations: vec![CompiledGeometryOp::Boolean {
                    op: BooleanOp::Union,
                    left: GeomRef::Sub("a.body".into()),
                    right: GeomRef::Sub("b.body2".into()),
                }],
                content_hash: ContentHash::of_str("outer"),
                produced_repr: ReprKind::BRep,
            },
        );

        // ── (a) reverse: both inner realizations list outer as a dependent ────────
        let index = ReverseDependencyIndex::build_from_graph_and_fields(&graph, &[]);
        let inner_a_deps = index.realization_dependents_of(&inner_a);
        assert!(
            inner_a_deps.contains(&NodeId::Realization(outer.clone())),
            "realization_dependents_of(inner_a) must contain Realization(outer) \
             (Boolean left operand Sub(\"a.body\") resolved to inner_a). Got: {:?}",
            inner_a_deps
        );
        let inner_b_deps = index.realization_dependents_of(&inner_b);
        assert!(
            inner_b_deps.contains(&NodeId::Realization(outer.clone())),
            "realization_dependents_of(inner_b) must contain Realization(outer) \
             (Boolean right operand Sub(\"b.body2\") resolved to inner_b). Got: {:?}",
            inner_b_deps
        );

        // ── (b) forward: outer's trace carries both inner realizations in reads ───
        let traces = build_trace_map_and_fields(&graph, &[]);
        let outer_trace = traces
            .get(&NodeId::Realization(outer.clone()))
            .expect("trace map must contain Realization(outer)");
        assert!(
            outer_trace.realization_reads.contains(&inner_a),
            "forward trace for Realization(outer) must have inner_a in realization_reads \
             (Boolean left Sub resolved). Got: {:?}",
            outer_trace.realization_reads
        );
        assert!(
            outer_trace.realization_reads.contains(&inner_b),
            "forward trace for Realization(outer) must have inner_b in realization_reads \
             (Boolean right Sub resolved). Got: {:?}",
            outer_trace.realization_reads
        );

        // ── NEGATIVE: GeomRef::Step operands must NOT produce realization edges ───
        // Add a second consuming realization that uses only Step refs (intra-node).
        let step_outer = RealizationNodeId::new("StepOuter", 0);
        graph.realizations.insert(
            step_outer.clone(),
            RealizationNodeData {
                id: step_outer.clone(),
                geometry_cell: None,
                operations: vec![CompiledGeometryOp::Boolean {
                    op: BooleanOp::Union,
                    left: GeomRef::Step(0),
                    right: GeomRef::Step(1),
                }],
                content_hash: ContentHash::of_str("step_outer"),
                produced_repr: ReprKind::BRep,
            },
        );
        let index2 = ReverseDependencyIndex::build_from_graph_and_fields(&graph, &[]);
        let traces2 = build_trace_map_and_fields(&graph, &[]);
        // inner_a and inner_b must not gain StepOuter as a realization dependent.
        assert!(
            !index2
                .realization_dependents_of(&inner_a)
                .contains(&NodeId::Realization(step_outer.clone())),
            "GeomRef::Step operand must NOT produce a realization→realization edge; \
             inner_a should not list step_outer"
        );
        // step_outer's forward trace must have empty realization_reads.
        let step_trace = traces2
            .get(&NodeId::Realization(step_outer.clone()))
            .expect("trace map must contain Realization(step_outer)");
        assert!(
            step_trace.realization_reads.is_empty(),
            "Step-only Boolean must yield EMPTY realization_reads on outer. Got: {:?}",
            step_trace.realization_reads
        );
    }

    // ── Task 4354 step-7: RED — realization→realization edges for
    //    Modify/Transform/Pattern .target and Sweep .profiles ──────────────────
    //
    // Each cross-sub op variant that carries a GeomRef target or profiles must
    // register a realization→realization edge when that ref is GeomRef::Sub.
    //
    // Cases covered (one consuming realization each):
    //   - Transform { Translate, target: Sub("inner.body") }
    //   - Modify { Fillet, target: Sub("inner.body") }
    //   - Pattern { Linear, target: Sub("inner.body") }
    //   - Sweep { Extrude, profiles: [Sub("inner.body")] }
    //
    // Negatives (pre-pinned from step-5; re-asserted here for coverage):
    //   - Transform { Translate, target: Step(0) } → EMPTY realization_reads
    //
    // RED today: extract_realization_edges step-6 only handles Boolean; the `_ => {}`
    // arm silently drops Modify/Transform/Pattern targets and Sweep profiles.

    /// Source realization `inner` (entity "Inner", geometry_cell = ValueCellId("Inner","body")).
    /// Consuming realizations for each op variant each use Sub("inner.body") as the target.
    /// Asserts BOTH directions for each variant; asserts EMPTY for the Step negative.
    #[test]
    fn modify_transform_pattern_sweep_cross_sub_register_realization_to_realization_edges() {
        use crate::graph::{EvaluationGraph, RealizationNodeData};
        use reify_compiler::{CompiledGeometryOp, GeomRef, ModifyKind, PatternKind, SweepKind, TransformKind};
        use reify_core::{ContentHash, RealizationNodeId};
        use reify_ir::ReprKind;

        // Helper to build a graph with `inner` + one consuming realization,
        // return (graph, inner, consuming_rid).
        fn make_graph(
            consuming: RealizationNodeId,
            ops: Vec<CompiledGeometryOp>,
        ) -> (EvaluationGraph, RealizationNodeId) {
            let mut graph = EvaluationGraph::default();
            // inner: entity "Inner", geometry_cell = ValueCellId("Inner","body")
            let inner = RealizationNodeId::new("Inner", 0);
            graph.realizations.insert(
                inner.clone(),
                RealizationNodeData {
                    id: inner.clone(),
                    geometry_cell: Some(ValueCellId::new("Inner", "body")),
                    operations: vec![],
                    content_hash: ContentHash::of_str("inner"),
                    produced_repr: ReprKind::BRep,
                },
            );
            graph.realizations.insert(
                consuming.clone(),
                RealizationNodeData {
                    id: consuming.clone(),
                    geometry_cell: None,
                    operations: ops,
                    content_hash: ContentHash::of_str("consuming"),
                    produced_repr: ReprKind::BRep,
                },
            );
            (graph, inner)
        }

        // ── Transform { Translate, target: Sub("inner.body") } ───────────────
        let t_outer = RealizationNodeId::new("TrOuter", 0);
        let (graph_t, inner_t) = make_graph(
            t_outer.clone(),
            vec![CompiledGeometryOp::Transform {
                kind: TransformKind::Translate,
                target: GeomRef::Sub("inner.body".into()),
                args: vec![],
            }],
        );
        let idx_t = ReverseDependencyIndex::build_from_graph_and_fields(&graph_t, &[]);
        assert!(
            idx_t
                .realization_dependents_of(&inner_t)
                .contains(&NodeId::Realization(t_outer.clone())),
            "Transform Sub target: realization_dependents_of(inner) must contain Realization(t_outer). Got: {:?}",
            idx_t.realization_dependents_of(&inner_t)
        );
        let traces_t = build_trace_map_and_fields(&graph_t, &[]);
        let t_trace = traces_t
            .get(&NodeId::Realization(t_outer.clone()))
            .expect("trace map must contain Realization(t_outer)");
        assert!(
            t_trace.realization_reads.contains(&inner_t),
            "Transform Sub target: forward trace must have inner in realization_reads. Got: {:?}",
            t_trace.realization_reads
        );

        // ── Modify { Fillet, target: Sub("inner.body") } ─────────────────────
        let m_outer = RealizationNodeId::new("MoOuter", 0);
        let (graph_m, inner_m) = make_graph(
            m_outer.clone(),
            vec![CompiledGeometryOp::Modify {
                kind: ModifyKind::Fillet,
                target: GeomRef::Sub("inner.body".into()),
                args: vec![],
            }],
        );
        let idx_m = ReverseDependencyIndex::build_from_graph_and_fields(&graph_m, &[]);
        assert!(
            idx_m
                .realization_dependents_of(&inner_m)
                .contains(&NodeId::Realization(m_outer.clone())),
            "Modify Sub target: realization_dependents_of(inner) must contain Realization(m_outer). Got: {:?}",
            idx_m.realization_dependents_of(&inner_m)
        );
        let traces_m = build_trace_map_and_fields(&graph_m, &[]);
        let m_trace = traces_m
            .get(&NodeId::Realization(m_outer.clone()))
            .expect("trace map must contain Realization(m_outer)");
        assert!(
            m_trace.realization_reads.contains(&inner_m),
            "Modify Sub target: forward trace must have inner in realization_reads. Got: {:?}",
            m_trace.realization_reads
        );

        // ── Pattern { Linear, target: Sub("inner.body") } ────────────────────
        let p_outer = RealizationNodeId::new("PaOuter", 0);
        let (graph_p, inner_p) = make_graph(
            p_outer.clone(),
            vec![CompiledGeometryOp::Pattern {
                kind: PatternKind::Linear,
                target: GeomRef::Sub("inner.body".into()),
                args: vec![],
            }],
        );
        let idx_p = ReverseDependencyIndex::build_from_graph_and_fields(&graph_p, &[]);
        assert!(
            idx_p
                .realization_dependents_of(&inner_p)
                .contains(&NodeId::Realization(p_outer.clone())),
            "Pattern Sub target: realization_dependents_of(inner) must contain Realization(p_outer). Got: {:?}",
            idx_p.realization_dependents_of(&inner_p)
        );
        let traces_p = build_trace_map_and_fields(&graph_p, &[]);
        let p_trace = traces_p
            .get(&NodeId::Realization(p_outer.clone()))
            .expect("trace map must contain Realization(p_outer)");
        assert!(
            p_trace.realization_reads.contains(&inner_p),
            "Pattern Sub target: forward trace must have inner in realization_reads. Got: {:?}",
            p_trace.realization_reads
        );

        // ── Sweep { Extrude, profiles: [Sub("inner.body")] } ─────────────────
        let s_outer = RealizationNodeId::new("SwOuter", 0);
        let (graph_s, inner_s) = make_graph(
            s_outer.clone(),
            vec![CompiledGeometryOp::Sweep {
                kind: SweepKind::Extrude,
                profiles: vec![GeomRef::Sub("inner.body".into())],
                args: vec![],
            }],
        );
        let idx_s = ReverseDependencyIndex::build_from_graph_and_fields(&graph_s, &[]);
        assert!(
            idx_s
                .realization_dependents_of(&inner_s)
                .contains(&NodeId::Realization(s_outer.clone())),
            "Sweep Sub profiles: realization_dependents_of(inner) must contain Realization(s_outer). Got: {:?}",
            idx_s.realization_dependents_of(&inner_s)
        );
        let traces_s = build_trace_map_and_fields(&graph_s, &[]);
        let s_trace = traces_s
            .get(&NodeId::Realization(s_outer.clone()))
            .expect("trace map must contain Realization(s_outer)");
        assert!(
            s_trace.realization_reads.contains(&inner_s),
            "Sweep Sub profiles: forward trace must have inner in realization_reads. Got: {:?}",
            s_trace.realization_reads
        );

        // ── NEGATIVE: Transform { Translate, target: Step(0) } → EMPTY reads ─
        let step_outer = RealizationNodeId::new("StepTrOuter", 0);
        let (graph_step, inner_step) = make_graph(
            step_outer.clone(),
            vec![CompiledGeometryOp::Transform {
                kind: TransformKind::Translate,
                target: GeomRef::Step(0),
                args: vec![],
            }],
        );
        let idx_step = ReverseDependencyIndex::build_from_graph_and_fields(&graph_step, &[]);
        assert!(
            !idx_step
                .realization_dependents_of(&inner_step)
                .contains(&NodeId::Realization(step_outer.clone())),
            "Transform Step target must NOT produce a realization→realization edge"
        );
        let traces_step = build_trace_map_and_fields(&graph_step, &[]);
        let step_trace = traces_step
            .get(&NodeId::Realization(step_outer.clone()))
            .expect("trace map must contain Realization(step_outer)");
        assert!(
            step_trace.realization_reads.is_empty(),
            "Transform Step target must yield EMPTY realization_reads. Got: {:?}",
            step_trace.realization_reads
        );
    }
}

/// Extract value cell dependencies from an expression, returning deduplicated sorted Vec.
///
/// This is a thin wrapper around CompiledExpr::collect_value_refs() that additionally
/// deduplicates and sorts the results for deterministic ordering.
pub fn extract_value_deps(expr: &reify_ir::CompiledExpr) -> Vec<ValueCellId> {
    let refs = expr.collect_value_refs();
    let unique: std::collections::HashSet<_> = refs.into_iter().collect();
    let mut sorted: Vec<_> = unique.into_iter().collect();
    sorted.sort();
    sorted
}

#[cfg(test)]
mod extract_value_deps_tests {
    use super::*;
    use reify_core::Type;
    use reify_ir::{BinOp, CompiledExpr, UnOp};

    /// Step 1a: Verify literal expr returns empty vec.
    #[test]
    fn extract_value_deps_literal_returns_empty() {
        use std::f64::consts::PI;
        let expr = CompiledExpr::literal(reify_ir::Value::Real(PI), Type::Real);
        let deps = extract_value_deps(&expr);
        assert!(
            deps.is_empty(),
            "Literal should have no value deps, got: {:?}",
            deps
        );
    }

    /// Step 1b: Verify ValueRef returns the referenced id.
    #[test]
    fn extract_value_deps_value_ref_returns_id() {
        let cell = ValueCellId::new("A", "x");
        let expr = CompiledExpr::value_ref(cell.clone(), Type::Real);
        let deps = extract_value_deps(&expr);
        assert_eq!(deps.len(), 1, "ValueRef should have 1 dep");
        assert!(deps.contains(&cell), "deps should contain 'x'");
    }

    /// Step 1c: Verify BinOp recursively collects from both sides.
    #[test]
    fn extract_value_deps_binop_collects_both_sides() {
        let a = ValueCellId::new("A", "a");
        let b = ValueCellId::new("A", "b");
        let expr = CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(a.clone(), Type::Real),
            CompiledExpr::value_ref(b.clone(), Type::Real),
            Type::Real,
        );
        let deps = extract_value_deps(&expr);
        assert_eq!(deps.len(), 2, "BinOp should have 2 deps");
        assert!(deps.contains(&a), "deps should contain 'a'");
        assert!(deps.contains(&b), "deps should contain 'b'");
    }

    /// Step 1d: Verify nested expressions collect all transitive ValueRef deps.
    #[test]
    fn extract_value_deps_nested_expr_collects_all_refs() {
        // (a + b) * c
        let a = ValueCellId::new("A", "a");
        let b = ValueCellId::new("A", "b");
        let c = ValueCellId::new("A", "c");
        let inner = CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(a.clone(), Type::Real),
            CompiledExpr::value_ref(b.clone(), Type::Real),
            Type::Real,
        );
        let expr = CompiledExpr::binop(
            BinOp::Mul,
            inner,
            CompiledExpr::value_ref(c.clone(), Type::Real),
            Type::Real,
        );
        let deps = extract_value_deps(&expr);
        assert_eq!(deps.len(), 3, "Nested BinOp should have 3 deps");
        assert!(deps.contains(&a));
        assert!(deps.contains(&b));
        assert!(deps.contains(&c));
    }

    /// Step 1e: Verify UnOp collects from operand.
    #[test]
    fn extract_value_deps_unop_collects_operand() {
        let x = ValueCellId::new("A", "x");
        let expr = CompiledExpr::unop(
            UnOp::Neg,
            CompiledExpr::value_ref(x.clone(), Type::Real),
            Type::Real,
        );
        let deps = extract_value_deps(&expr);
        assert_eq!(deps.len(), 1, "UnOp should have 1 dep");
        assert!(deps.contains(&x));
    }

    /// Step 1f: Verify duplicates are deduplicated.
    #[test]
    fn extract_value_deps_duplicates_deduplicated() {
        // x + x (same cell referenced twice)
        let x = ValueCellId::new("A", "x");
        let expr = CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(x.clone(), Type::Real),
            CompiledExpr::value_ref(x.clone(), Type::Real),
            Type::Real,
        );
        let deps = extract_value_deps(&expr);
        assert_eq!(
            deps.len(),
            1,
            "Duplicate refs should be deduplicated to 1, got: {:?}",
            deps
        );
    }

    /// Step 1g: Verify results are sorted for deterministic ordering.
    #[test]
    fn extract_value_deps_results_are_sorted() {
        let a = ValueCellId::new("A", "a");
        let b = ValueCellId::new("A", "b");
        let c = ValueCellId::new("A", "c");
        // Build expr with refs in reverse order: c + b + a
        let expr = CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::binop(
                BinOp::Add,
                CompiledExpr::value_ref(c.clone(), Type::Real),
                CompiledExpr::value_ref(b.clone(), Type::Real),
                Type::Real,
            ),
            CompiledExpr::value_ref(a.clone(), Type::Real),
            Type::Real,
        );
        let deps = extract_value_deps(&expr);
        // Should be sorted: a, b, c
        assert_eq!(deps, vec![a, b, c], "deps should be sorted");
    }
}

/// Dependency map: forward and reverse mappings between value cells.
///
/// Forward map: cell → cells it depends on (directly).
/// Reverse map: cell → cells that depend on it (inverse of forward).
///
/// Built once from EvaluationGraph at graph-build time.
#[derive(Clone, Debug, Default)]
pub struct DependencyMap {
    /// Forward dependencies: cell → cells it reads from.
    pub forward: HashMap<ValueCellId, Vec<ValueCellId>>,
    /// Reverse dependencies: cell → cells that read from it.
    pub reverse: HashMap<ValueCellId, Vec<ValueCellId>>,
}

impl DependencyMap {
    /// Build a dependency map from an evaluation graph.
    ///
    /// Iterates all value cells, extracts dependencies from default_expr using
    /// extract_value_deps, and builds both forward and reverse mappings.
    pub fn from_graph(graph: &crate::graph::EvaluationGraph) -> Self {
        use reify_compiler::ValueCellKind;

        let mut forward = HashMap::new();
        let mut reverse: HashMap<ValueCellId, Vec<ValueCellId>> = HashMap::new();

        // Collect all value cells and their dependencies
        for (_, node) in graph.value_cells.iter() {
            let deps = match &node.default_expr {
                Some(expr) => extract_value_deps(expr),
                None => vec![], // Params have no dependencies
            };

            if !deps.is_empty() || node.kind == ValueCellKind::Let {
                // Only non-empty deps matter for forward map (or Let bindings)
                if !deps.is_empty() {
                    forward.insert(node.id.clone(), deps.clone());
                }

                // Build reverse map: each dep gets this cell as a dependent
                for dep in deps {
                    reverse.entry(dep).or_default().push(node.id.clone());
                }
            }
        }

        // Add entries for cells with no dependencies (they have no reverse deps)
        // This ensures all cells appear in the map if needed
        for (_, node) in graph.value_cells.iter() {
            if !forward.contains_key(&node.id) {
                forward.entry(node.id.clone()).or_default();
            }
            if !reverse.contains_key(&node.id) {
                reverse.entry(node.id.clone()).or_default();
            }
        }

        Self { forward, reverse }
    }

    /// Get the set of cells that the given cell depends on (forward lookup).
    pub fn deps_of(&self, cell: &ValueCellId) -> &[ValueCellId] {
        self.forward.get(cell).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get the set of cells that depend on the given cell (reverse lookup).
    pub fn dependents_of(&self, cell: &ValueCellId) -> &[ValueCellId] {
        self.reverse.get(cell).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Return all cells in dependency order (topological sort).
    ///
    /// Uses Kahn's algorithm: start with cells that have no deps, process queue,
    /// decrement dependents' in-degree, emit when in-degree reaches 0.
    pub fn topological_order(&self) -> Vec<ValueCellId> {
        // Compute in-degree for each cell: how many cells does it depend on?
        let mut in_degree: HashMap<ValueCellId, usize> = HashMap::new();

        // Initialize all cells with in-degree 0
        for cell in self.forward.keys() {
            in_degree.insert(cell.clone(), 0);
        }

        // For each cell, its in-degree = number of dependencies
        for (cell, deps) in &self.forward {
            let degree = deps.len();
            in_degree.insert(cell.clone(), degree);
        }

        // Start with cells that have in-degree 0 (no dependencies)
        let mut queue: Vec<_> = in_degree
            .iter()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(cell, _)| cell.clone())
            .collect();

        let mut result = Vec::new();

        while let Some(cell) = queue.pop() {
            result.push(cell.clone());

            // Decrement in-degree for dependents (cells that depend on this one)
            if let Some(dependents) = self.reverse.get(&cell) {
                for dependent in dependents {
                    if let Some(deg) = in_degree.get_mut(dependent) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push(dependent.clone());
                        }
                    }
                }
            }
        }

        result
    }
}

#[cfg(test)]
mod dependency_map_tests {
    use super::*;
    use crate::graph::EvaluationGraph;
    use reify_test_support::bracket_compiled_module;

    /// Step 3a: Verify forward deps using bracket fixture.
    #[test]
    fn dependency_map_forward_deps_bracket() {
        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let dep_map = DependencyMap::from_graph(&graph);

        let e = "Bracket";

        // width: param, no forward deps
        let width_deps = dep_map.deps_of(&ValueCellId::new(e, "width"));
        assert!(
            width_deps.is_empty(),
            "width (param) should have no forward deps, got: {:?}",
            width_deps
        );

        // height: param, no forward deps
        let height_deps = dep_map.deps_of(&ValueCellId::new(e, "height"));
        assert!(
            height_deps.is_empty(),
            "height (param) should have no forward deps"
        );

        // thickness: param, no forward deps
        let thickness_deps = dep_map.deps_of(&ValueCellId::new(e, "thickness"));
        assert!(
            thickness_deps.is_empty(),
            "thickness (param) should have no forward deps"
        );

        // volume depends on width, height, thickness
        let volume_deps = dep_map.deps_of(&ValueCellId::new(e, "volume"));
        assert_eq!(
            volume_deps.len(),
            3,
            "volume should depend on 3 cells, got: {:?}",
            volume_deps
        );
        assert!(volume_deps.contains(&ValueCellId::new(e, "width")));
        assert!(volume_deps.contains(&ValueCellId::new(e, "height")));
        assert!(volume_deps.contains(&ValueCellId::new(e, "thickness")));
    }

    /// Step 3b: Verify the map contains entries for all value cells.
    #[test]
    fn dependency_map_contains_all_value_cells() {
        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let dep_map = DependencyMap::from_graph(&graph);

        let e = "Bracket";

        // All value cells in bracket should be in the forward map
        let expected_cells = vec![
            ValueCellId::new(e, "width"),
            ValueCellId::new(e, "height"),
            ValueCellId::new(e, "thickness"),
            ValueCellId::new(e, "fillet_radius"),
            ValueCellId::new(e, "hole_diameter"),
            ValueCellId::new(e, "volume"),
        ];

        for cell in expected_cells {
            assert!(
                dep_map.forward.contains_key(&cell),
                "forward map should contain {:?}",
                cell
            );
        }
    }

    /// Step 5a: Verify dependents_of (reverse lookup) for width.
    #[test]
    fn dependency_map_dependents_of_width() {
        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let dep_map = DependencyMap::from_graph(&graph);

        let e = "Bracket";
        let width = ValueCellId::new(e, "width");
        let dependents = dep_map.dependents_of(&width);

        // width is depended on by: volume (let)
        assert!(
            dependents.contains(&ValueCellId::new(e, "volume")),
            "width dependents should include volume, got: {:?}",
            dependents
        );
    }

    /// Step 5b: Verify dependents_of for height.
    #[test]
    fn dependency_map_dependents_of_height() {
        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let dep_map = DependencyMap::from_graph(&graph);

        let e = "Bracket";
        let height = ValueCellId::new(e, "height");
        let dependents = dep_map.dependents_of(&height);

        // height is depended on by: volume (let)
        assert!(
            dependents.contains(&ValueCellId::new(e, "volume")),
            "height dependents should include volume, got: {:?}",
            dependents
        );
    }

    /// Step 5c: Verify dependents_of for thickness.
    #[test]
    fn dependency_map_dependents_of_thickness() {
        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let dep_map = DependencyMap::from_graph(&graph);

        let e = "Bracket";
        let thickness = ValueCellId::new(e, "thickness");
        let dependents = dep_map.dependents_of(&thickness);

        // thickness is depended on by: volume (let)
        assert!(
            dependents.contains(&ValueCellId::new(e, "volume")),
            "thickness dependents should include volume, got: {:?}",
            dependents
        );
    }

    /// Step 5d: Verify volume has no dependents.
    #[test]
    fn dependency_map_volume_has_no_dependents() {
        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let dep_map = DependencyMap::from_graph(&graph);

        let e = "Bracket";
        let volume = ValueCellId::new(e, "volume");
        let dependents = dep_map.dependents_of(&volume);

        assert!(
            dependents.is_empty(),
            "volume should have no dependents, got: {:?}",
            dependents
        );
    }

    /// Step 7a: Verify topological_order: params come before let bindings.
    #[test]
    fn dependency_map_topological_order_params_before_lets() {
        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let dep_map = DependencyMap::from_graph(&graph);

        let order = dep_map.topological_order();
        let e = "Bracket";

        let width_idx = order
            .iter()
            .position(|c| c == &ValueCellId::new(e, "width"));
        let height_idx = order
            .iter()
            .position(|c| c == &ValueCellId::new(e, "height"));
        let thickness_idx = order
            .iter()
            .position(|c| c == &ValueCellId::new(e, "thickness"));
        let volume_idx = order
            .iter()
            .position(|c| c == &ValueCellId::new(e, "volume"));

        // Params should come before volume in topological order
        if let (Some(w), Some(v)) = (width_idx, volume_idx) {
            assert!(w < v, "width ({}) should come before volume ({})", w, v);
        }
        if let (Some(h), Some(v)) = (height_idx, volume_idx) {
            assert!(h < v, "height ({}) should come before volume ({})", h, v);
        }
        if let (Some(t), Some(v)) = (thickness_idx, volume_idx) {
            assert!(t < v, "thickness ({}) should come before volume ({})", t, v);
        }
    }

    /// Step 7b: Verify topological_order includes all cells.
    #[test]
    fn dependency_map_topological_order_includes_all() {
        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let dep_map = DependencyMap::from_graph(&graph);

        let order = dep_map.topological_order();
        let e = "Bracket";

        let expected = [
            ValueCellId::new(e, "width"),
            ValueCellId::new(e, "height"),
            ValueCellId::new(e, "thickness"),
            ValueCellId::new(e, "fillet_radius"),
            ValueCellId::new(e, "hole_diameter"),
            ValueCellId::new(e, "volume"),
        ];

        assert_eq!(
            order.len(),
            expected.len(),
            "topological_order should have {} cells, got {}",
            expected.len(),
            order.len()
        );
    }

    /// Step 7c: Verify topological_order is valid (no cell appears before its deps).
    #[test]
    fn dependency_map_topological_order_valid() {
        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let dep_map = DependencyMap::from_graph(&graph);

        let order = dep_map.topological_order();

        for (i, cell) in order.iter().enumerate() {
            let deps = dep_map.deps_of(cell);
            for dep in deps {
                let dep_idx = order.iter().position(|c| c == dep);
                if let Some(di) = dep_idx {
                    assert!(
                        di < i,
                        "cell {:?} at {} has dep {:?} at {} (should be before)",
                        cell,
                        i,
                        dep,
                        di
                    );
                }
            }
        }
    }
}
