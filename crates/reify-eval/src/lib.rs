pub mod cache;
pub mod demand;
pub mod deps;
pub mod dirty;
pub mod graph;
pub mod journal;
pub mod snapshot;

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use reify_compiler::{CompiledModule, ValueCellKind};
use reify_types::{
    AutoParam, ConstraintChecker, ConstraintInput, ConstraintSolver, ContentHash,
    DeterminacyState, Diagnostic, ExportFormat, GeometryHandleId, GeometryKernel,
    PersistentMap, ResolutionProblem, Satisfaction, SnapshotId, SnapshotProvenance,
    SolveResult, Value, ValueCellId, ValueMap, VersionId,
};

use crate::cache::{CacheStore, CachedResult, EvalOutcome, NodeId};
use crate::demand::DemandRegistry;
use crate::deps::{extract_dependency_trace, DependencyTrace, ReverseDependencyIndex};
use crate::journal::{EvalEvent, EventJournal, EventKind, EventPayload};
use crate::snapshot::Snapshot;

/// The engine facade — main entry point for evaluation.
pub struct Engine {
    constraint_checker: Box<dyn ConstraintChecker>,
    geometry_kernel: Option<Box<dyn GeometryKernel>>,
    solver: Option<Box<dyn ConstraintSolver>>,
    cache: CacheStore,
    /// Overridden param values (set by set_param_and_invalidate).
    param_overrides: std::collections::HashMap<ValueCellId, reify_types::Value>,
    /// Current snapshot from last eval() or edit_param().
    current_snapshot: Option<Snapshot>,
    /// Reverse dependency index for dirty cone computation.
    reverse_index: Option<ReverseDependencyIndex>,
    /// Forward dependency trace map for topological sort.
    trace_map: Option<HashMap<NodeId, DependencyTrace>>,
    /// Demand registry tracking which nodes are demanded.
    demand: DemandRegistry,
    /// Counter for snapshot IDs.
    next_snapshot_id: u64,
    /// Counter for version IDs.
    next_version_id: u64,
    /// The eval set from the last edit_param() or eval() call.
    last_eval_set: Vec<NodeId>,
    /// Event journal recording evaluation events.
    journal: EventJournal,
}

/// Statistics about cache behavior during a cached evaluation.
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub early_cutoffs: usize,
}

/// Result of a cached evaluation, wrapping EvalResult with stats.
#[derive(Debug)]
pub struct CachedEvalResult {
    pub eval_result: EvalResult,
    pub stats: CacheStats,
}

/// Result of evaluating a compiled module.
#[derive(Debug)]
pub struct EvalResult {
    pub values: ValueMap,
    pub diagnostics: Vec<Diagnostic>,
    pub resolved_params: HashMap<ValueCellId, reify_types::Value>,
}

/// Result of checking constraints.
#[derive(Debug)]
pub struct CheckResult {
    pub values: ValueMap,
    pub constraint_results: Vec<ConstraintCheckEntry>,
    pub diagnostics: Vec<Diagnostic>,
    pub resolved_params: HashMap<ValueCellId, reify_types::Value>,
}

/// A single constraint's check result.
#[derive(Debug)]
pub struct ConstraintCheckEntry {
    pub id: reify_types::ConstraintNodeId,
    pub label: Option<String>,
    pub satisfaction: Satisfaction,
}

/// Result of a full build (eval + geometry).
#[derive(Debug)]
pub struct BuildResult {
    pub values: ValueMap,
    pub constraint_results: Vec<ConstraintCheckEntry>,
    pub geometry_output: Option<Vec<u8>>,
    pub diagnostics: Vec<Diagnostic>,
    pub resolved_params: HashMap<ValueCellId, reify_types::Value>,
}

/// State extracted from Engine for concurrent evaluation.
///
/// Contains all the Clone-able, Send+Sync state needed for concurrent
/// evaluation. Produced by `Engine::prepare_concurrent_edit()` and consumed
/// by `ConcurrentEvalAdapter` in reify-runtime.
///
/// PersistentMap fields clone in O(1) via structural sharing.
#[derive(Debug)]
pub struct ConcurrentEditSetup {
    /// Nodes to evaluate (topologically sorted, dirty ∩ demand).
    pub eval_set: Vec<NodeId>,
    /// The evaluation graph (O(1) clone).
    pub graph: crate::graph::EvaluationGraph,
    /// Current values for all cells (O(1) clone).
    pub values: ValueMap,
    /// Snapshot values with determinacy state (O(1) clone).
    pub snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    /// Forward dependency traces for topological sort.
    pub traces: HashMap<NodeId, DependencyTrace>,
    /// Reverse dependency index for early cutoff propagation.
    pub reverse_index: ReverseDependencyIndex,
    /// Pre-extracted content hashes for Changed/Unchanged determination.
    pub previous_hashes: HashMap<NodeId, ContentHash>,
    /// Version for this edit.
    pub version: VersionId,
    /// Snapshot ID for this edit.
    pub snapshot_id: SnapshotId,
    /// Parent snapshot ID.
    pub parent_snapshot_id: SnapshotId,
    /// Set of changed cells (the edited parameter).
    pub changed_cells: HashSet<ValueCellId>,
}

/// Result of evaluating a single node during concurrent evaluation.
#[derive(Debug, Clone)]
pub struct ConcurrentNodeResult {
    /// The node that was evaluated.
    pub node: NodeId,
    /// The computed value.
    pub value: Value,
    /// Determinacy state of the result.
    pub determinacy: DeterminacyState,
    /// Dependency trace from expression evaluation.
    pub trace: DependencyTrace,
    /// Whether the result changed vs the previous evaluation.
    pub outcome: EvalOutcome,
}

/// Aggregate result from concurrent evaluation, ready for Engine::apply_concurrent_edit().
#[derive(Debug)]
pub struct ConcurrentEditResult {
    /// Updated values for all cells.
    pub values: ValueMap,
    /// Updated snapshot values with determinacy states.
    pub snapshot_values: PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    /// Per-node evaluation results.
    pub node_results: Vec<ConcurrentNodeResult>,
    /// Nodes that were actually evaluated (excludes skipped).
    pub actual_eval_set: Vec<NodeId>,
    /// Nodes skipped due to early cutoff.
    pub skipped: HashSet<NodeId>,
}

impl Engine {
    pub fn new(
        constraint_checker: Box<dyn ConstraintChecker>,
        geometry_kernel: Option<Box<dyn GeometryKernel>>,
    ) -> Self {
        Self {
            constraint_checker,
            geometry_kernel,
            solver: None,
            cache: CacheStore::new(),
            param_overrides: std::collections::HashMap::new(),
            current_snapshot: None,
            reverse_index: None,
            trace_map: None,
            demand: DemandRegistry::new(),
            next_snapshot_id: 0,
            next_version_id: 0,
            last_eval_set: Vec::new(),
            journal: EventJournal::new(),
        }
    }

    /// Set the constraint solver for resolving auto parameters.
    pub fn with_solver(mut self, solver: Box<dyn ConstraintSolver>) -> Self {
        self.solver = Some(solver);
        self
    }

    /// Access the cache store (for testing/inspection).
    pub fn cache_store(&self) -> &CacheStore {
        &self.cache
    }

    /// Access the current snapshot (for testing/inspection).
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.current_snapshot.as_ref()
    }

    /// Access the eval set from the last eval() or edit_param() call.
    pub fn last_eval_set(&self) -> &[NodeId] {
        &self.last_eval_set
    }

    /// Access the event journal (for testing/inspection).
    pub fn journal(&self) -> &EventJournal {
        &self.journal
    }

    /// Prepare state for concurrent evaluation after a parameter change.
    ///
    /// Extracts all Clone-able, Send+Sync state needed by the concurrent
    /// adapter. Uses PersistentMap O(1) clones for graph, values, and
    /// snapshot_values. Pre-extracts content hashes from CacheStore for
    /// Changed/Unchanged determination during concurrent eval.
    ///
    /// Requires a prior call to eval() to establish baseline state.
    pub fn prepare_concurrent_edit(
        &mut self,
        cell: ValueCellId,
        new_value: Value,
    ) -> ConcurrentEditSetup {
        let snapshot = self.current_snapshot.as_ref()
            .expect("prepare_concurrent_edit requires a prior call to eval()");
        let reverse_index = self.reverse_index.as_ref()
            .expect("prepare_concurrent_edit requires reverse_index from eval()");
        let trace_map = self.trace_map.as_ref()
            .expect("prepare_concurrent_edit requires trace_map from eval()");

        // Clone snapshot (O(1) via PersistentMap)
        let parent_id = snapshot.id;
        let mut new_snapshot_values = snapshot.values.clone();

        // Update the changed cell's value in snapshot
        new_snapshot_values.insert(
            cell.clone(),
            (new_value.clone(), DeterminacyState::Determined),
        );

        // Compute dirty cone and eval set
        let mut changed_set = HashSet::new();
        changed_set.insert(cell.clone());
        let dirty_cone = crate::dirty::compute_dirty_cone(&changed_set, reverse_index);
        let eval_set = crate::dirty::compute_eval_set(&dirty_cone, &self.demand, trace_map);

        // Build the full ValueMap from snapshot values
        let mut values = ValueMap::new();
        for (id, (val, _det)) in new_snapshot_values.iter() {
            values.insert(id.clone(), val.clone());
        }
        // Overwrite with the new param value
        values.insert(cell.clone(), new_value);

        // Bump snapshot/version IDs
        let snapshot_id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        let version_id = self.next_version_id;
        self.next_version_id += 1;

        // Extract previous content hashes from CacheStore for nodes in eval set
        let mut previous_hashes = HashMap::new();
        for node_id in &eval_set {
            if let Some(entry) = self.cache.get(node_id) {
                previous_hashes.insert(node_id.clone(), entry.result_hash);
            }
        }

        // Mark all nodes in the eval set as Pending
        self.cache.reset_pending_transition_count();
        for node_id in &eval_set {
            self.cache.mark_pending(node_id);
        }

        ConcurrentEditSetup {
            eval_set,
            graph: snapshot.graph.clone(),
            values,
            snapshot_values: new_snapshot_values,
            traces: trace_map.clone(),
            reverse_index: reverse_index.clone(),
            previous_hashes,
            version: VersionId(version_id),
            snapshot_id: SnapshotId(snapshot_id),
            parent_snapshot_id: parent_id,
            changed_cells: changed_set,
        }
    }

    /// Roll back the Engine state after a failed concurrent evaluation.
    ///
    /// Restores all eval_set nodes from Pending back to Final and decrements
    /// the snapshot/version ID counters to avoid gaps in numbering. This
    /// returns the engine to a consistent state as if prepare_concurrent_edit()
    /// was never called.
    ///
    /// Called on the error path when ConcurrentScheduler::execute() returns Err
    /// (e.g. TaskPanicked), to prevent nodes from being permanently stuck in
    /// Pending state.
    pub fn rollback_concurrent_edit(&mut self, setup: &ConcurrentEditSetup) {
        // Restore all eval_set nodes from Pending → Final
        for node_id in &setup.eval_set {
            self.cache.restore_final(node_id);
        }

        // Roll back the snapshot/version ID bumps done in prepare_concurrent_edit.
        // Safe because no external observer has seen these IDs yet — they only
        // exist in the ConcurrentEditSetup which is being discarded.
        self.next_snapshot_id = setup.snapshot_id.0;
        self.next_version_id = setup.version.0;
    }

    /// Apply the results of concurrent evaluation back to the Engine.
    ///
    /// Updates cache entries, journal, snapshot, and last_eval_set from the
    /// concurrent evaluation results. Called after edit_param_concurrent()
    /// completes to synchronize Engine state.
    pub fn apply_concurrent_edit(
        &mut self,
        setup: &ConcurrentEditSetup,
        result: ConcurrentEditResult,
    ) {
        // Record cache entries and journal events for each evaluated node
        for node_result in &result.node_results {
            let start = Instant::now();
            self.journal.record(EvalEvent {
                timestamp: start,
                node_id: node_result.node.clone(),
                kind: EventKind::Started,
                version: setup.version,
                payload: None,
            });

            let trace = node_result.trace.clone();
            let cached_result = CachedResult::Value(
                node_result.value.clone(),
                node_result.determinacy,
            );
            self.cache.record_evaluation(
                node_result.node.clone(),
                cached_result,
                setup.version,
                trace,
            );

            self.journal.record(EvalEvent {
                timestamp: Instant::now(),
                node_id: node_result.node.clone(),
                kind: EventKind::Completed { outcome: node_result.outcome },
                version: setup.version,
                payload: Some(EventPayload::Duration(start.elapsed())),
            });
        }

        // Restore freshness to Final for skipped nodes
        for node_id in &result.skipped {
            self.cache.restore_final(node_id);
        }

        // Update current snapshot
        let snapshot = self.current_snapshot.as_ref()
            .expect("apply_concurrent_edit requires snapshot from eval()");
        let mut new_snapshot = snapshot.clone();
        new_snapshot.id = setup.snapshot_id;
        new_snapshot.version = setup.version;
        new_snapshot.values = result.snapshot_values;
        new_snapshot.provenance = SnapshotProvenance::Edit {
            changed: setup.changed_cells.clone(),
            parent: setup.parent_snapshot_id,
        };
        self.current_snapshot = Some(new_snapshot);

        // Update last eval set
        self.last_eval_set = result.actual_eval_set;
    }

    /// Set a parameter override and invalidate cache entries that depend on it.
    pub fn set_param_and_invalidate(
        &mut self,
        param: &ValueCellId,
        value: reify_types::Value,
    ) {
        self.param_overrides.insert(param.clone(), value);
        // Mark the param's own cache entry as dirty
        let param_node = NodeId::Value(param.clone());
        self.cache.invalidate(&param_node);
        // Mark all nodes that depend on this param as dirty
        self.cache.invalidate_dependents(std::slice::from_ref(param));
    }

    /// Evaluate a compiled module, returning computed values.
    ///
    /// This is a cold-start evaluation that builds a new Snapshot and
    /// dependency structures. Subsequent calls to edit_param() can perform
    /// incremental re-evaluation using these structures.
    pub fn eval(
        &mut self,
        module: &CompiledModule,
    ) -> EvalResult {
        let mut values = ValueMap::new();
        let mut diagnostics = Vec::new();

        // Build Snapshot from CompiledModule (creates EvaluationGraph internally)
        let snapshot_id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        let version_id = self.next_version_id;
        self.next_version_id += 1;

        let mut snapshot = Snapshot::from_compiled_module(module);
        snapshot.id = SnapshotId(snapshot_id);
        snapshot.version = VersionId(version_id);
        snapshot.provenance = SnapshotProvenance::Initial;

        // Build dependency structures from the graph
        let reverse_index = ReverseDependencyIndex::build_from_graph(&snapshot.graph);
        let trace_map = crate::deps::build_trace_map(&snapshot.graph);

        // Set up demand registry: demand all value cells, constraints, and realizations
        let mut demand = DemandRegistry::new();
        for (_, node) in snapshot.graph.value_cells.iter() {
            demand.add_demand(NodeId::Value(node.id.clone()));
        }
        for (_, cnode) in snapshot.graph.constraints.iter() {
            demand.add_demand(NodeId::Constraint(cnode.id.clone()));
        }
        for (_, rnode) in snapshot.graph.realizations.iter() {
            demand.add_demand(NodeId::Realization(rnode.id.clone()));
        }
        demand.rebuild_cone(&snapshot.graph);

        // Two-pass evaluation (same logic as before)
        for template in &module.templates {
            // First pass: evaluate Param defaults and Auto cells to populate the value map
            for cell in &template.value_cells {
                if cell.kind == ValueCellKind::Auto {
                    // Auto cells: Undef with DeterminacyState::Auto
                    let node_id = NodeId::Value(cell.id.clone());
                    let start = Instant::now();
                    self.journal.record(EvalEvent {
                        timestamp: start,
                        node_id: node_id.clone(),
                        kind: EventKind::Started,
                        version: VersionId(version_id),
                        payload: None,
                    });

                    values.insert(cell.id.clone(), reify_types::Value::Undef);
                    snapshot.values.insert(
                        cell.id.clone(),
                        (reify_types::Value::Undef, DeterminacyState::Auto),
                    );

                    // Record in cache
                    let trace = DependencyTrace::default();
                    let cached_result =
                        CachedResult::Value(reify_types::Value::Undef, DeterminacyState::Auto);
                    let outcome = self.cache.record_evaluation(
                        node_id.clone(),
                        cached_result,
                        VersionId(version_id),
                        trace,
                    );

                    self.journal.record(EvalEvent {
                        timestamp: Instant::now(),
                        node_id,
                        kind: EventKind::Completed { outcome },
                        version: VersionId(version_id),
                        payload: Some(EventPayload::Duration(start.elapsed())),
                    });
                } else if cell.kind == ValueCellKind::Param
                    && let Some(ref expr) = cell.default_expr
                {
                    let node_id = NodeId::Value(cell.id.clone());
                    let start = Instant::now();
                    self.journal.record(EvalEvent {
                        timestamp: start,
                        node_id: node_id.clone(),
                        kind: EventKind::Started,
                        version: VersionId(version_id),
                        payload: None,
                    });

                    let val = reify_expr::eval_expr(expr, &values);
                    values.insert(cell.id.clone(), val.clone());

                    // Update snapshot values
                    snapshot.values.insert(
                        cell.id.clone(),
                        (val.clone(), DeterminacyState::Determined),
                    );

                    // Record in cache
                    let trace = DependencyTrace::default();
                    let cached_result =
                        CachedResult::Value(val, DeterminacyState::Determined);
                    let outcome = self.cache.record_evaluation(
                        node_id.clone(),
                        cached_result,
                        VersionId(version_id),
                        trace,
                    );

                    self.journal.record(EvalEvent {
                        timestamp: Instant::now(),
                        node_id,
                        kind: EventKind::Completed { outcome },
                        version: VersionId(version_id),
                        payload: Some(EventPayload::Duration(start.elapsed())),
                    });
                }
            }

            // Second pass: evaluate Let bindings (which may reference params)
            for cell in &template.value_cells {
                if cell.kind == ValueCellKind::Let
                    && let Some(ref expr) = cell.default_expr
                {
                    let node_id = NodeId::Value(cell.id.clone());
                    let start = Instant::now();
                    self.journal.record(EvalEvent {
                        timestamp: start,
                        node_id: node_id.clone(),
                        kind: EventKind::Started,
                        version: VersionId(version_id),
                        payload: None,
                    });

                    let val = reify_expr::eval_expr(expr, &values);
                    values.insert(cell.id.clone(), val.clone());

                    // Update snapshot values
                    snapshot.values.insert(
                        cell.id.clone(),
                        (val.clone(), DeterminacyState::Determined),
                    );

                    // Record in cache with dependency trace
                    let trace = extract_dependency_trace(expr);
                    let cached_result =
                        CachedResult::Value(val, DeterminacyState::Determined);
                    let outcome = self.cache.record_evaluation(
                        node_id.clone(),
                        cached_result,
                        VersionId(version_id),
                        trace,
                    );

                    self.journal.record(EvalEvent {
                        timestamp: Instant::now(),
                        node_id,
                        kind: EventKind::Completed { outcome },
                        version: VersionId(version_id),
                        payload: Some(EventPayload::Duration(start.elapsed())),
                    });
                }
            }
        }

        // Resolution phase: resolve auto params using the constraint solver.
        let mut resolved_params = HashMap::new();
        if let Some(ref solver) = self.solver {
            for template in &module.templates {
                // Collect auto param IDs for this template
                let auto_ids: std::collections::HashSet<ValueCellId> = template
                    .value_cells
                    .iter()
                    .filter(|cell| cell.kind == ValueCellKind::Auto)
                    .map(|cell| cell.id.clone())
                    .collect();

                if auto_ids.is_empty() {
                    continue;
                }

                // Find constraints whose dependency traces reference auto params
                let filtered_constraints: Vec<_> = template
                    .constraints
                    .iter()
                    .filter(|c| {
                        let trace = extract_dependency_trace(&c.expr);
                        trace.reads.iter().any(|r| auto_ids.contains(r))
                    })
                    .map(|c| (c.id.clone(), c.expr.clone()))
                    .collect();

                // Build AutoParam list from template value cells
                let auto_param_list: Vec<AutoParam> = template
                    .value_cells
                    .iter()
                    .filter(|cell| cell.kind == ValueCellKind::Auto)
                    .map(|cell| AutoParam {
                        id: cell.id.clone(),
                        param_type: cell.cell_type.clone(),
                        bounds: None,
                    })
                    .collect();

                // Build ResolutionProblem
                let problem = ResolutionProblem {
                    auto_params: auto_param_list,
                    constraints: filtered_constraints,
                    current_values: values.clone(),
                    objective: None,
                };

                let parent_snap_id = snapshot.id;

                match solver.solve(&problem) {
                    SolveResult::Solved { values: solver_values } => {
                        // Allocate new snapshot/version IDs BEFORE recording cache
                        // entries so all resolution-phase entries share the same
                        // basis_version as the snapshot. This preserves the invariant
                        // that try_fast_path relies on for incremental evaluation.
                        let res_snapshot_id = self.next_snapshot_id;
                        self.next_snapshot_id += 1;
                        let res_version_id = self.next_version_id;
                        self.next_version_id += 1;

                        // Update values map with resolved values
                        let mut resolved_ids = std::collections::HashSet::new();
                        for (id, val) in &solver_values {
                            let node_id = NodeId::Value(id.clone());
                            let start = Instant::now();
                            self.journal.record(EvalEvent {
                                timestamp: start,
                                node_id: node_id.clone(),
                                kind: EventKind::Started,
                                version: VersionId(res_version_id),
                                payload: None,
                            });

                            values.insert(id.clone(), val.clone());
                            resolved_params.insert(id.clone(), val.clone());
                            resolved_ids.insert(id.clone());

                            // Update snapshot values with (resolved_val, Determined)
                            snapshot.values.insert(
                                id.clone(),
                                (val.clone(), DeterminacyState::Determined),
                            );

                            // Update cache with res_version_id (matches snapshot)
                            let trace = DependencyTrace::default();
                            let cached_result =
                                CachedResult::Value(val.clone(), DeterminacyState::Determined);
                            let outcome = self.cache.record_evaluation(
                                node_id.clone(),
                                cached_result,
                                VersionId(res_version_id),
                                trace,
                            );

                            self.journal.record(EvalEvent {
                                timestamp: Instant::now(),
                                node_id,
                                kind: EventKind::Completed { outcome },
                                version: VersionId(res_version_id),
                                payload: Some(EventPayload::Duration(start.elapsed())),
                            });
                        }

                        // Set child snapshot with Resolution provenance
                        snapshot.id = SnapshotId(res_snapshot_id);
                        snapshot.version = VersionId(res_version_id);
                        snapshot.provenance = SnapshotProvenance::Resolution {
                            scope: template.name.clone(),
                            resolved: resolved_ids,
                            parent: parent_snap_id,
                        };

                        // Re-run let binding evaluation with resolved values
                        for cell in &template.value_cells {
                            if cell.kind == ValueCellKind::Let
                                && let Some(ref expr) = cell.default_expr
                            {
                                let node_id = NodeId::Value(cell.id.clone());
                                let start = Instant::now();
                                self.journal.record(EvalEvent {
                                    timestamp: start,
                                    node_id: node_id.clone(),
                                    kind: EventKind::Started,
                                    version: VersionId(res_version_id),
                                    payload: None,
                                });

                                let val = reify_expr::eval_expr(expr, &values);
                                values.insert(cell.id.clone(), val.clone());

                                snapshot.values.insert(
                                    cell.id.clone(),
                                    (val.clone(), DeterminacyState::Determined),
                                );

                                let trace = extract_dependency_trace(expr);
                                let cached_result =
                                    CachedResult::Value(val, DeterminacyState::Determined);
                                let outcome = self.cache.record_evaluation(
                                    node_id.clone(),
                                    cached_result,
                                    VersionId(res_version_id),
                                    trace,
                                );

                                self.journal.record(EvalEvent {
                                    timestamp: Instant::now(),
                                    node_id,
                                    kind: EventKind::Completed { outcome },
                                    version: VersionId(res_version_id),
                                    payload: Some(EventPayload::Duration(start.elapsed())),
                                });
                            }
                        }
                    }
                    SolveResult::Infeasible { diagnostics: solver_diags } => {
                        diagnostics.extend(solver_diags);
                    }
                    SolveResult::NoProgress { reason } => {
                        diagnostics.push(Diagnostic::warning(format!(
                            "Constraint solver made no progress: {}",
                            reason
                        )));
                    }
                }
            }
        }

        // Store internal state for incremental evaluation
        self.current_snapshot = Some(snapshot);
        self.reverse_index = Some(reverse_index);
        self.trace_map = Some(trace_map);
        self.demand = demand;
        self.last_eval_set = Vec::new(); // Cold start: no incremental eval set

        EvalResult { values, diagnostics, resolved_params }
    }

    /// Incrementally re-evaluate after changing a parameter value.
    ///
    /// Requires a prior call to eval() to establish the baseline snapshot
    /// and dependency structures. Creates a child snapshot with Edit provenance,
    /// computes dirty∩demand cone intersection, evaluates only Value nodes in
    /// the eval set (topologically sorted). Constraint/Realization nodes are
    /// tracked in the eval set but not evaluated (deferred to check()/build()).
    ///
    /// Returns EvalResult with all current values (both changed and unchanged).
    pub fn edit_param(
        &mut self,
        cell: ValueCellId,
        new_value: reify_types::Value,
    ) -> EvalResult {
        let snapshot = self.current_snapshot.as_ref()
            .expect("edit_param requires a prior call to eval()");
        let reverse_index = self.reverse_index.as_ref()
            .expect("edit_param requires reverse_index from eval()");
        let trace_map = self.trace_map.as_ref()
            .expect("edit_param requires trace_map from eval()");

        // Clone snapshot (O(1) via PersistentMap)
        let parent_id = snapshot.id;
        let mut new_snapshot = snapshot.clone();

        // Update snapshot ID, version, and provenance
        let snapshot_id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        let version_id = self.next_version_id;
        self.next_version_id += 1;
        new_snapshot.id = SnapshotId(snapshot_id);
        new_snapshot.version = VersionId(version_id);

        let mut changed_set = std::collections::HashSet::new();
        changed_set.insert(cell.clone());
        new_snapshot.provenance = SnapshotProvenance::Edit {
            changed: changed_set.clone(),
            parent: parent_id,
        };

        // Update the changed cell's value in snapshot
        new_snapshot.values.insert(
            cell.clone(),
            (new_value.clone(), DeterminacyState::Determined),
        );

        // Compute dirty cone and eval set
        let dirty_cone = crate::dirty::compute_dirty_cone(&changed_set, reverse_index);
        let eval_set = crate::dirty::compute_eval_set(&dirty_cone, &self.demand, trace_map);

        // Build the full ValueMap from snapshot values
        let mut values = ValueMap::new();
        for (id, (val, _det)) in new_snapshot.values.iter() {
            values.insert(id.clone(), val.clone());
        }
        // Overwrite with the new param value
        values.insert(cell.clone(), new_value);

        // Mark all nodes in the eval set as Pending before re-evaluation.
        // This transitions Final → Pending{last_substantive: hash}.
        self.cache.reset_pending_transition_count();
        for node_id in &eval_set {
            self.cache.mark_pending(node_id);
        }

        // Evaluate only Value nodes in the eval set (topo-sorted order).
        // Track nodes to skip due to early cutoff of upstream nodes.
        let mut skipped: std::collections::HashSet<NodeId> = std::collections::HashSet::new();
        let mut actual_eval_set: Vec<NodeId> = Vec::with_capacity(eval_set.len());

        // Track nodes that have at least one Changed parent. Seeded with
        // dependents of the original changed param (the param itself IS a
        // change, even though it's not in the eval set). This prevents
        // incorrectly skipping nodes that read both an Unchanged intermediary
        // AND the changed param directly (mixed fan-in).
        let mut has_changed_parent: std::collections::HashSet<NodeId> =
            std::collections::HashSet::new();
        if let Some(rev_idx) = &self.reverse_index {
            for dependent in rev_idx.dependents_of(&cell) {
                has_changed_parent.insert(dependent.clone());
            }
        }

        for node_id in &eval_set {
            if skipped.contains(node_id) {
                continue;
            }
            actual_eval_set.push(node_id.clone());

            if let NodeId::Value(vcid) = node_id
                && let Some(node) = new_snapshot.graph.value_cells.get(vcid)
                && let Some(ref expr) = node.default_expr
            {
                let start = Instant::now();
                self.journal.record(EvalEvent {
                    timestamp: start,
                    node_id: node_id.clone(),
                    kind: EventKind::Started,
                    version: VersionId(version_id),
                    payload: None,
                });

                let val = reify_expr::eval_expr(expr, &values);
                values.insert(vcid.clone(), val.clone());
                new_snapshot.values.insert(
                    vcid.clone(),
                    (val.clone(), DeterminacyState::Determined),
                );

                // Record in cache and check for early cutoff
                let trace = extract_dependency_trace(expr);
                let cached_result =
                    CachedResult::Value(val, DeterminacyState::Determined);
                let outcome = self.cache.record_evaluation(
                    node_id.clone(),
                    cached_result,
                    VersionId(version_id),
                    trace,
                );

                self.journal.record(EvalEvent {
                    timestamp: Instant::now(),
                    node_id: node_id.clone(),
                    kind: EventKind::Completed { outcome },
                    version: VersionId(version_id),
                    payload: Some(EventPayload::Duration(start.elapsed())),
                });

                // Early cutoff with mixed fan-in protection:
                // - Changed: propagate has_changed_parent to dependents,
                //   remove them from skipped (in case an earlier Unchanged
                //   parent added them prematurely).
                // - Unchanged: only add dependents to skipped if they do NOT
                //   have a Changed parent (i.e., not in has_changed_parent).
                if let Some(rev_idx) = &self.reverse_index {
                    let dependents = rev_idx.dependents_of(vcid);
                    if outcome == EvalOutcome::Changed {
                        for dependent in dependents {
                            has_changed_parent.insert(dependent.clone());
                            skipped.remove(dependent);
                        }
                    } else {
                        // Unchanged
                        for dependent in dependents {
                            if !has_changed_parent.contains(dependent) {
                                skipped.insert(dependent.clone());
                            }
                        }
                    }
                }
            }
            // Constraint/Realization nodes: tracked in eval set but not evaluated
            // (deferred to check()/build())
        }

        // Restore freshness to Final for nodes that were pre-marked Pending
        // but then skipped by early cutoff (they were never re-evaluated).
        for node_id in &skipped {
            self.cache.restore_final(node_id);
        }

        // ── Resolution phase ───────────────────────────────────────────
        // If a solver is present, check whether any constraints governing
        // auto params are in the dirty cone. If so, re-run the solver
        // to update auto param values and propagate to dependents.
        let mut resolved_params = HashMap::new();
        let mut diagnostics = Vec::new();

        if let Some(ref solver) = self.solver {
            // Collect auto param IDs grouped by scope (entity name)
            let mut auto_ids: HashSet<ValueCellId> = HashSet::new();
            let mut auto_param_list: Vec<AutoParam> = Vec::new();
            let mut scope_name: Option<String> = None;

            for (_, node) in new_snapshot.graph.value_cells.iter() {
                if node.kind == ValueCellKind::Auto {
                    auto_ids.insert(node.id.clone());
                    auto_param_list.push(AutoParam {
                        id: node.id.clone(),
                        param_type: node.cell_type.clone(),
                        bounds: None,
                    });
                    // Use entity from ValueCellId as scope
                    if scope_name.is_none() {
                        scope_name = Some(node.id.entity.clone());
                    }
                }
            }

            if !auto_ids.is_empty() {
                // Find constraints whose dependency traces reference auto params
                let filtered_constraints: Vec<_> = new_snapshot.graph.constraints.iter()
                    .filter(|(_, cnode)| {
                        let trace = extract_dependency_trace(&cnode.expr);
                        trace.reads.iter().any(|r| auto_ids.contains(r))
                    })
                    .map(|(_, cnode)| (cnode.id.clone(), cnode.expr.clone()))
                    .collect();

                // Check if any of those constraints are in the dirty cone
                let constraints_dirty = filtered_constraints.iter().any(|(cid, _)| {
                    dirty_cone.contains(&NodeId::Constraint(cid.clone()))
                });

                if constraints_dirty {
                    // Build ResolutionProblem and solve
                    let problem = ResolutionProblem {
                        auto_params: auto_param_list,
                        constraints: filtered_constraints,
                        current_values: values.clone(),
                        objective: None,
                    };

                    match solver.solve(&problem) {
                        SolveResult::Solved { values: solver_values } => {
                            let mut resolved_ids = HashSet::new();
                            for (id, val) in &solver_values {
                                values.insert(id.clone(), val.clone());
                                resolved_params.insert(id.clone(), val.clone());
                                resolved_ids.insert(id.clone());

                                // Update snapshot values
                                new_snapshot.values.insert(
                                    id.clone(),
                                    (val.clone(), DeterminacyState::Determined),
                                );

                                // Update param_overrides so subsequent edits
                                // use the resolved value
                                self.param_overrides.insert(id.clone(), val.clone());

                                // Update cache
                                let node_id = NodeId::Value(id.clone());
                                let trace = DependencyTrace::default();
                                let cached_result = CachedResult::Value(
                                    val.clone(),
                                    DeterminacyState::Determined,
                                );
                                self.cache.record_evaluation(
                                    node_id,
                                    cached_result,
                                    VersionId(version_id),
                                    trace,
                                );
                            }

                            // ── Second propagation wave ─────────────────────
                            // Re-resolved auto params may have changed value.
                            // Let bindings depending on them may NOT be in the
                            // original dirty cone. Compute a second dirty cone
                            // from the resolved auto param IDs and re-evaluate
                            // affected value nodes.
                            if !resolved_ids.is_empty() {
                                let wave2_dirty = crate::dirty::compute_dirty_cone(
                                    &resolved_ids,
                                    reverse_index,
                                );
                                let wave2_eval = crate::dirty::compute_eval_set(
                                    &wave2_dirty,
                                    &self.demand,
                                    trace_map,
                                );

                                for node_id in &wave2_eval {
                                    if let NodeId::Value(vcid) = node_id
                                        && let Some(node) = new_snapshot.graph.value_cells.get(vcid)
                                        && let Some(ref expr) = node.default_expr
                                    {
                                        let val = reify_expr::eval_expr(expr, &values);
                                        values.insert(vcid.clone(), val.clone());
                                        new_snapshot.values.insert(
                                            vcid.clone(),
                                            (val.clone(), DeterminacyState::Determined),
                                        );

                                        // Update cache for re-evaluated node
                                        let trace = extract_dependency_trace(expr);
                                        let cached_result = CachedResult::Value(
                                            val,
                                            DeterminacyState::Determined,
                                        );
                                        self.cache.record_evaluation(
                                            node_id.clone(),
                                            cached_result,
                                            VersionId(version_id),
                                            trace,
                                        );
                                    }
                                }
                            }
                        }
                        SolveResult::Infeasible { diagnostics: solver_diags } => {
                            diagnostics.extend(solver_diags);
                        }
                        SolveResult::NoProgress { reason } => {
                            diagnostics.push(Diagnostic::warning(format!(
                                "Constraint solver made no progress: {}",
                                reason
                            )));
                        }
                    }
                }
            }
        }

        // Store state (actual_eval_set excludes early-cutoff-skipped nodes)
        self.last_eval_set = actual_eval_set;
        self.current_snapshot = Some(new_snapshot);

        EvalResult {
            values,
            diagnostics,
            resolved_params,
        }
    }

    /// Incrementally re-evaluate and check constraints after changing a parameter.
    ///
    /// Combines edit_param() (incremental value evaluation + re-resolution)
    /// with constraint satisfaction checking against the updated values.
    /// Evaluates ALL constraints (not just dirty ones) to produce a complete
    /// CheckResult, mirroring check()'s pattern but incrementally.
    ///
    /// Requires a prior call to eval() or check() to establish the baseline.
    pub fn edit_check(
        &mut self,
        cell: ValueCellId,
        new_value: reify_types::Value,
    ) -> CheckResult {
        let eval_result = self.edit_param(cell, new_value);
        let mut constraint_results = Vec::new();
        let mut diagnostics = eval_result.diagnostics;

        // Evaluate ALL constraints from the snapshot's graph
        let snapshot = self.current_snapshot.as_ref()
            .expect("edit_check requires a snapshot from edit_param");

        let constraint_pairs: Vec<_> = snapshot
            .graph
            .constraints
            .iter()
            .map(|(_, cnode)| (cnode.id.clone(), &cnode.expr))
            .collect();

        if !constraint_pairs.is_empty() {
            let input = ConstraintInput {
                constraints: constraint_pairs,
                values: &eval_result.values,
            };

            let results = self.constraint_checker.check(&input);
            for result in results {
                diagnostics.extend(result.diagnostics.messages);
                constraint_results.push(ConstraintCheckEntry {
                    id: result.id,
                    label: None,
                    satisfaction: result.satisfaction,
                });
            }
        }

        CheckResult {
            values: eval_result.values,
            constraint_results,
            diagnostics,
            resolved_params: eval_result.resolved_params,
        }
    }

    /// Evaluate a compiled module with caching and early cutoff.
    ///
    /// On first call (cold start), behaves like eval() but populates the cache.
    /// On subsequent calls with the same version, uses version fast path.
    /// On calls with a new version after invalidation, re-evaluates dirty nodes
    /// and uses early cutoff to avoid propagating unchanged results.
    pub fn eval_cached(
        &mut self,
        module: &CompiledModule,
        version: VersionId,
    ) -> CachedEvalResult {
        let mut values = ValueMap::new();
        let diagnostics = Vec::new();
        let mut stats = CacheStats::default();

        for template in &module.templates {
            // First pass: evaluate Param defaults, Auto cells, (or use overrides)
            for cell in &template.value_cells {
                if cell.kind == ValueCellKind::Auto {
                    let node_id = NodeId::Value(cell.id.clone());

                    // Check version fast path
                    if let Some(CachedResult::Value(val, _)) =
                        self.cache.try_fast_path(&node_id, version)
                    {
                        self.journal.record(EvalEvent {
                            timestamp: Instant::now(),
                            node_id,
                            kind: EventKind::CacheHit,
                            version,
                            payload: None,
                        });
                        values.insert(cell.id.clone(), val);
                        stats.cache_hits += 1;
                        continue;
                    }

                    // Check cache reuse (not dirty, no override)
                    if !self.param_overrides.contains_key(&cell.id)
                        && !self.cache.is_dirty(&node_id)
                        && let Some(entry) = self.cache.get(&node_id)
                        && let CachedResult::Value(ref val, _) = entry.result
                    {
                        let val = val.clone();
                        values.insert(cell.id.clone(), val);
                        let trace = entry.dependency_trace.clone();
                        let result = entry.result.clone();
                        self.cache.record_evaluation(
                            node_id.clone(),
                            result,
                            version,
                            trace,
                        );
                        self.journal.record(EvalEvent {
                            timestamp: Instant::now(),
                            node_id,
                            kind: EventKind::CacheHit,
                            version,
                            payload: None,
                        });
                        stats.cache_hits += 1;
                        continue;
                    }

                    stats.cache_misses += 1;

                    let start = Instant::now();
                    self.journal.record(EvalEvent {
                        timestamp: start,
                        node_id: node_id.clone(),
                        kind: EventKind::Started,
                        version,
                        payload: None,
                    });

                    // Use override if available, otherwise Undef with Auto determinacy
                    let (val, det) = if let Some(override_val) = self.param_overrides.get(&cell.id) {
                        (override_val.clone(), DeterminacyState::Determined)
                    } else {
                        (reify_types::Value::Undef, DeterminacyState::Auto)
                    };

                    let trace = DependencyTrace::default();
                    let cached_result = CachedResult::Value(val.clone(), det);
                    let outcome =
                        self.cache
                            .record_evaluation(node_id.clone(), cached_result, version, trace);

                    self.journal.record(EvalEvent {
                        timestamp: Instant::now(),
                        node_id,
                        kind: EventKind::Completed { outcome },
                        version,
                        payload: Some(EventPayload::Duration(start.elapsed())),
                    });

                    if outcome == EvalOutcome::Unchanged {
                        stats.early_cutoffs += 1;
                    }

                    values.insert(cell.id.clone(), val);
                } else if cell.kind == ValueCellKind::Param {
                    let node_id = NodeId::Value(cell.id.clone());

                    // Check version fast path
                    if let Some(CachedResult::Value(val, _)) =
                        self.cache.try_fast_path(&node_id, version)
                    {
                        self.journal.record(EvalEvent {
                            timestamp: Instant::now(),
                            node_id,
                            kind: EventKind::CacheHit,
                            version,
                            payload: None,
                        });
                        values.insert(cell.id.clone(), val);
                        stats.cache_hits += 1;
                        continue;
                    }

                    // Check if cache entry still exists and is not dirty.
                    // For params without overrides, we can reuse cached values.
                    if !self.param_overrides.contains_key(&cell.id)
                        && !self.cache.is_dirty(&node_id)
                        && let Some(entry) = self.cache.get(&node_id)
                        && let CachedResult::Value(ref val, _) = entry.result
                    {
                        let val = val.clone();
                        values.insert(cell.id.clone(), val);
                        let trace = entry.dependency_trace.clone();
                        let result = entry.result.clone();
                        self.cache.record_evaluation(
                            node_id.clone(),
                            result,
                            version,
                            trace,
                        );
                        self.journal.record(EvalEvent {
                            timestamp: Instant::now(),
                            node_id,
                            kind: EventKind::CacheHit,
                            version,
                            payload: None,
                        });
                        stats.cache_hits += 1;
                        continue;
                    }

                    stats.cache_misses += 1;

                    let start = Instant::now();
                    self.journal.record(EvalEvent {
                        timestamp: start,
                        node_id: node_id.clone(),
                        kind: EventKind::Started,
                        version,
                        payload: None,
                    });

                    // Use override if available, otherwise evaluate default
                    let val = if let Some(override_val) = self.param_overrides.get(&cell.id) {
                        override_val.clone()
                    } else if let Some(ref expr) = cell.default_expr {
                        reify_expr::eval_expr(expr, &values)
                    } else {
                        reify_types::Value::Undef
                    };

                    // Build dependency trace (params have no reads - they are roots)
                    let trace = DependencyTrace::default();

                    let cached_result =
                        CachedResult::Value(val.clone(), DeterminacyState::Determined);
                    let outcome =
                        self.cache
                            .record_evaluation(node_id.clone(), cached_result, version, trace);

                    self.journal.record(EvalEvent {
                        timestamp: Instant::now(),
                        node_id,
                        kind: EventKind::Completed { outcome },
                        version,
                        payload: Some(EventPayload::Duration(start.elapsed())),
                    });

                    if outcome == EvalOutcome::Unchanged {
                        stats.early_cutoffs += 1;
                    }

                    values.insert(cell.id.clone(), val);
                }
            }

            // Second pass: evaluate Let bindings
            for cell in &template.value_cells {
                if cell.kind == ValueCellKind::Let
                    && let Some(ref expr) = cell.default_expr
                {
                    let node_id = NodeId::Value(cell.id.clone());

                    // Check version fast path
                    if let Some(CachedResult::Value(val, _)) =
                        self.cache.try_fast_path(&node_id, version)
                    {
                        self.journal.record(EvalEvent {
                            timestamp: Instant::now(),
                            node_id,
                            kind: EventKind::CacheHit,
                            version,
                            payload: None,
                        });
                        values.insert(cell.id.clone(), val);
                        stats.cache_hits += 1;
                        continue;
                    }

                    // Check if cache entry still exists and is not dirty.
                    // If so, the node's dependencies haven't changed, so we
                    // can reuse the cached result and update its basis_version.
                    if !self.cache.is_dirty(&node_id)
                        && let Some(entry) = self.cache.get(&node_id)
                        && let CachedResult::Value(ref val, _) = entry.result
                    {
                        let val = val.clone();
                        values.insert(cell.id.clone(), val);
                        let trace = entry.dependency_trace.clone();
                        let result = entry.result.clone();
                        self.cache.record_evaluation(
                            node_id.clone(),
                            result,
                            version,
                            trace,
                        );
                        self.journal.record(EvalEvent {
                            timestamp: Instant::now(),
                            node_id,
                            kind: EventKind::CacheHit,
                            version,
                            payload: None,
                        });
                        stats.cache_hits += 1;
                        continue;
                    }

                    stats.cache_misses += 1;
                    self.cache.clear_dirty(&node_id);

                    let start = Instant::now();
                    self.journal.record(EvalEvent {
                        timestamp: start,
                        node_id: node_id.clone(),
                        kind: EventKind::Started,
                        version,
                        payload: None,
                    });

                    let val = reify_expr::eval_expr(expr, &values);

                    // Build dependency trace from expression refs
                    let trace = extract_dependency_trace(expr);

                    let cached_result =
                        CachedResult::Value(val.clone(), DeterminacyState::Determined);
                    let outcome =
                        self.cache
                            .record_evaluation(node_id.clone(), cached_result, version, trace);

                    self.journal.record(EvalEvent {
                        timestamp: Instant::now(),
                        node_id,
                        kind: EventKind::Completed { outcome },
                        version,
                        payload: Some(EventPayload::Duration(start.elapsed())),
                    });

                    if outcome == EvalOutcome::Unchanged {
                        stats.early_cutoffs += 1;
                        // Early cutoff: clear dirty flags on nodes that
                        // depend on this cell, since its result hasn't changed.
                        self.cache.clear_dependents_dirty(&cell.id);
                    }

                    values.insert(cell.id.clone(), val);
                }
            }
        }

        CachedEvalResult {
            eval_result: EvalResult { values, diagnostics, resolved_params: HashMap::new() },
            stats,
        }
    }

    /// Evaluate and check constraints.
    pub fn check(
        &mut self,
        module: &CompiledModule,
    ) -> CheckResult {
        let eval_result = self.eval(module);
        let mut constraint_results = Vec::new();
        let mut diagnostics = eval_result.diagnostics;

        for template in &module.templates {
            if template.constraints.is_empty() {
                continue;
            }

            // Build ConstraintInput batch for this template
            let constraint_pairs: Vec<_> = template
                .constraints
                .iter()
                .map(|c| (c.id.clone(), &c.expr))
                .collect();

            let input = ConstraintInput {
                constraints: constraint_pairs,
                values: &eval_result.values,
            };

            let results = self.constraint_checker.check(&input);

            for (result, compiled) in results.into_iter().zip(template.constraints.iter()) {
                diagnostics.extend(result.diagnostics.messages);
                constraint_results.push(ConstraintCheckEntry {
                    id: result.id,
                    label: compiled.label.clone(),
                    satisfaction: result.satisfaction,
                });
            }
        }

        CheckResult {
            values: eval_result.values,
            constraint_results,
            diagnostics,
            resolved_params: eval_result.resolved_params,
        }
    }

    /// Full build: evaluate, check constraints, produce geometry.
    pub fn build(
        &mut self,
        module: &CompiledModule,
        format: ExportFormat,
    ) -> BuildResult {
        let check_result = self.check(module);
        let mut diagnostics = check_result.diagnostics;

        let geometry_output = if let Some(ref mut kernel) = self.geometry_kernel {
            // Execute geometry operations from realizations
            let mut last_handle: Option<GeometryHandleId> = None;
            let mut total_ops: usize = 0;

            for template in &module.templates {
                for realization in &template.realizations {
                    for op in &realization.operations {
                        total_ops += 1;
                        let geom_op =
                            compile_geometry_op(op, &check_result.values, &last_handle);
                        match geom_op {
                            Some(geom_op) => match kernel.execute(&geom_op) {
                                Ok(handle) => {
                                    last_handle = Some(handle.id);
                                }
                                Err(e) => {
                                    diagnostics.push(Diagnostic::error(
                                        format!("geometry error: {}", e),
                                    ));
                                }
                            },
                            None => {
                                diagnostics.push(Diagnostic::error(
                                    "failed to compile geometry operation",
                                ));
                            }
                        }
                    }
                }
            }

            if last_handle.is_none() && total_ops > 0 {
                // All geometry operations failed — skip export entirely
                diagnostics.push(Diagnostic::error(
                    "all geometry operations failed; no geometry output produced",
                ));
                None
            } else {
                // Export the result
                let export_handle = last_handle.unwrap_or(GeometryHandleId(0));
                let mut output = Vec::new();
                match kernel.export(export_handle, format, &mut output) {
                    Ok(()) => Some(output),
                    Err(e) => {
                        diagnostics.push(Diagnostic::error(
                            format!("export error: {}", e),
                        ));
                        None
                    }
                }
            }
        } else {
            None
        };

        BuildResult {
            values: check_result.values,
            constraint_results: check_result.constraint_results,
            geometry_output,
            diagnostics,
            resolved_params: check_result.resolved_params,
        }
    }
}

/// Compile a CompiledGeometryOp into a GeometryOp by evaluating expressions.
fn compile_geometry_op(
    op: &reify_compiler::CompiledGeometryOp,
    values: &ValueMap,
    last_handle: &Option<GeometryHandleId>,
) -> Option<reify_types::GeometryOp> {
    use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};

    match op {
        CompiledGeometryOp::Primitive { kind, args } => {
            let eval_arg = |name: &str| -> reify_types::Value {
                args.iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, expr)| reify_expr::eval_expr(expr, values))
                    .unwrap_or(reify_types::Value::Undef)
            };

            match kind {
                PrimitiveKind::Box => Some(reify_types::GeometryOp::Box {
                    width: eval_arg("width"),
                    height: eval_arg("height"),
                    depth: eval_arg("depth"),
                }),
                PrimitiveKind::Cylinder => Some(reify_types::GeometryOp::Cylinder {
                    radius: eval_arg("radius"),
                    height: eval_arg("height"),
                }),
                PrimitiveKind::Sphere => Some(reify_types::GeometryOp::Sphere {
                    radius: eval_arg("radius"),
                }),
            }
        }
        CompiledGeometryOp::Boolean { op, left, right } => {
            let resolve_ref = |r: &GeomRef| -> Option<GeometryHandleId> {
                match r {
                    GeomRef::Step(_idx) => *last_handle,
                    GeomRef::Sub(_name) => *last_handle,
                }
            };
            let left_id = resolve_ref(left)?;
            let right_id = resolve_ref(right)?;
            match op {
                BooleanOp::Union => Some(reify_types::GeometryOp::Union {
                    left: left_id,
                    right: right_id,
                }),
                BooleanOp::Difference => Some(reify_types::GeometryOp::Difference {
                    left: left_id,
                    right: right_id,
                }),
                BooleanOp::Intersection => Some(reify_types::GeometryOp::Intersection {
                    left: left_id,
                    right: right_id,
                }),
            }
        }
        CompiledGeometryOp::Modify { kind, target, args } => {
            let target_id = match target {
                GeomRef::Step(_) | GeomRef::Sub(_) => (*last_handle)?,
            };
            let eval_arg = |name: &str| -> reify_types::Value {
                args.iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, expr)| reify_expr::eval_expr(expr, values))
                    .unwrap_or(reify_types::Value::Undef)
            };
            match kind {
                reify_compiler::ModifyKind::Fillet => Some(reify_types::GeometryOp::Fillet {
                    target: target_id,
                    radius: eval_arg("radius"),
                }),
                reify_compiler::ModifyKind::Chamfer => Some(reify_types::GeometryOp::Chamfer {
                    target: target_id,
                    distance: eval_arg("distance"),
                }),
            }
        }
        CompiledGeometryOp::Transform { kind, target, args } => {
            let target_id = match target {
                GeomRef::Step(_) | GeomRef::Sub(_) => (*last_handle)?,
            };
            let eval_arg_f64 = |name: &str| -> f64 {
                args.iter()
                    .find(|(n, _)| n == name)
                    .and_then(|(_, expr)| reify_expr::eval_expr(expr, values).as_f64())
                    .unwrap_or(0.0)
            };
            match kind {
                reify_compiler::TransformKind::Translate => {
                    Some(reify_types::GeometryOp::Translate {
                        target: target_id,
                        dx: eval_arg_f64("dx"),
                        dy: eval_arg_f64("dy"),
                        dz: eval_arg_f64("dz"),
                    })
                }
                reify_compiler::TransformKind::Rotate => {
                    Some(reify_types::GeometryOp::Rotate {
                        target: target_id,
                        axis: [
                            eval_arg_f64("axis_x"),
                            eval_arg_f64("axis_y"),
                            eval_arg_f64("axis_z"),
                        ],
                        angle_rad: eval_arg_f64("angle"),
                    })
                }
            }
        }
    }
}

