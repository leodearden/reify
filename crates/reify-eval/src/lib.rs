pub mod cache;
pub mod demand;
pub mod deps;
pub mod dirty;
pub mod graph;
pub mod journal;
pub mod snapshot;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use reify_compiler::{
    CompiledConstraint, CompiledModule, CompiledPurpose, TopologyTemplate, ValueCellKind,
};
use reify_types::{
    AutoParam, CompiledFunction, ConstraintChecker, ConstraintInput, ConstraintNodeId,
    ConstraintSolver, ContentHash, DeterminacyState, Diagnostic, ExportFormat, FIELD_ENTITY_PREFIX,
    GeometryHandleId, GeometryKernel, Mesh, OptimizationObjective, PersistentMap,
    ResolutionProblem, Satisfaction, SnapshotId, SnapshotProvenance, SolveResult, Value,
    ValueCellId, ValueMap, VersionId,
};

use crate::cache::{CacheStore, CachedResult, EvalOutcome, NodeId};
use crate::demand::DemandRegistry;
use crate::deps::{DependencyTrace, ReverseDependencyIndex, extract_dependency_trace};
use crate::dirty::topological_sort;
use crate::journal::{EvalEvent, EventJournal, EventKind, EventPayload};
use crate::snapshot::Snapshot;

/// Error returned when an operation requires prior eval() but none has been performed.
#[derive(Debug)]
pub enum EngineError {
    /// The engine has not been initialized — call eval() first.
    NotInitialized,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::NotInitialized => {
                write!(
                    f,
                    "engine not initialized: call eval() before this operation"
                )
            }
        }
    }
}

impl std::error::Error for EngineError {}

/// Consolidated evaluation state produced by eval().
///
/// Groups the snapshot, reverse dependency index, and trace map that are
/// always set/unset atomically. This replaces three separate Option fields
/// in Engine, enforcing the invariant that all three are present together.
#[derive(Debug)]
pub struct EvaluationState {
    /// Current snapshot from last eval() or edit_param().
    pub snapshot: Snapshot,
    /// Reverse dependency index for dirty cone computation.
    pub reverse_index: ReverseDependencyIndex,
    /// Forward dependency trace map for topological sort.
    pub trace_map: HashMap<NodeId, DependencyTrace>,
}

/// The engine facade — main entry point for evaluation.
pub struct Engine {
    constraint_checker: Box<dyn ConstraintChecker>,
    geometry_kernel: Option<Box<dyn GeometryKernel>>,
    solver: Option<Box<dyn ConstraintSolver>>,
    cache: CacheStore,
    /// Compiled stdlib prelude modules (cached via OnceLock; zero-cost borrow).
    prelude: &'static [CompiledModule],
    /// Overridden param values (set by set_param_and_invalidate).
    param_overrides: std::collections::HashMap<ValueCellId, reify_types::Value>,
    /// Consolidated evaluation state from last eval() or edit_param().
    /// None before the first eval() call; always Some after.
    eval_state: Option<EvaluationState>,
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
    /// User-defined functions from the last eval() call.
    /// Stored so that edit_param() and other incremental paths can evaluate
    /// expressions containing UserFunctionCall nodes.
    functions: Vec<CompiledFunction>,
    /// Compiled purpose declarations from the last eval() call.
    /// Stored so activate_purpose/deactivate_purpose can look up purposes by name.
    compiled_purposes: Vec<CompiledPurpose>,
    /// Currently active purposes: maps purpose name → injected constraint IDs.
    /// Used by deactivate_purpose to remove the injected constraints.
    active_purposes: HashMap<String, Vec<ConstraintNodeId>>,
    /// Active optimization objectives injected by purposes.
    /// Maps purpose name → optimization objective.
    active_objective_map: HashMap<String, OptimizationObjective>,
    /// Template meta entries from the last eval() call.
    /// Maps template name → meta key/value pairs from the template's meta block.
    /// Populated during eval() so that edit_param() and other incremental paths
    /// can resolve MetaAccess expressions without re-reading the module.
    meta_map: HashMap<String, HashMap<String, String>>,
    /// Template-native optimization objectives from the last eval() call.
    /// Maps template name → optimization objective declared in the template.
    /// Populated during eval() so that edit_param() can look up the objective
    /// by scope_name without needing access to the original templates.
    objectives: HashMap<String, OptimizationObjective>,
    /// Maximum depth for recursive sub-component unfolding.
    /// Prevents runaway recursion when guard expressions don't terminate.
    /// Default: 64.
    max_unfold_depth: usize,
    /// Maximum total nodes created during recursive sub-component unfolding.
    /// Prevents exponential blowup when a template has multiple recursive subs
    /// (e.g., binary tree with `left` and `right` produces B^D nodes).
    /// Default: 10_000.
    max_unfold_nodes: usize,
    /// Last-known-good instance count for each collection count cell when the
    /// Undef guard fires. Used to recover accurate old_count on the next
    /// Int(M) edit after an Int(N)→Undef transition.
    ///
    /// Lifecycle: inserted when Undef guard fires (only if old_count is Int(n)),
    /// read in old_count resolution as fallback for Value::Undef,
    /// removed after successful re-elaboration.
    preserved_counts: HashMap<ValueCellId, i64>,
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

/// Result of tessellating all realizations in a module for GUI mesh rendering.
///
/// Similar to [`BuildResult`] but produces per-realization meshes instead of
/// a single exported geometry file. Each mesh is paired with its entity path
/// (e.g., `"Bracket#realization[0]"`).
#[derive(Debug)]
pub struct TessellateResult {
    pub values: ValueMap,
    pub constraint_results: Vec<ConstraintCheckEntry>,
    /// Per-realization tessellated meshes: `(entity_path, mesh)`.
    pub meshes: Vec<(String, Mesh)>,
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
    /// User-defined functions from the module (for evaluating UserFunctionCall nodes).
    pub functions: Vec<CompiledFunction>,
    /// Template-to-meta-entries mapping, populated from Engine::meta_map.
    /// Used to resolve MetaAccess expressions during concurrent evaluation.
    pub meta_map: Arc<HashMap<String, HashMap<String, String>>>,
    /// Template-native optimization objective for this edit's scope, if any.
    /// Populated from Engine::objectives during prepare_concurrent_edit().
    pub objective: Option<OptimizationObjective>,
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
    /// Auto parameters resolved by the constraint solver during this edit.
    pub resolved_params: HashMap<ValueCellId, Value>,
    /// Diagnostics from constraint resolution (e.g., infeasibility messages).
    pub diagnostics: Vec<Diagnostic>,
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
            prelude: reify_compiler::stdlib_loader::load_stdlib(),
            param_overrides: std::collections::HashMap::new(),
            eval_state: None,
            demand: DemandRegistry::new(),
            next_snapshot_id: 0,
            next_version_id: 0,
            last_eval_set: Vec::new(),
            journal: EventJournal::new(),
            functions: Vec::new(),
            compiled_purposes: Vec::new(),
            active_purposes: HashMap::new(),
            active_objective_map: HashMap::new(),
            objectives: HashMap::new(),
            meta_map: HashMap::new(),
            max_unfold_depth: 64,
            max_unfold_nodes: 10_000,
            preserved_counts: HashMap::new(),
        }
    }

    /// Returns the compiled stdlib prelude modules stored by this engine.
    pub fn prelude(&self) -> &[CompiledModule] {
        self.prelude
    }

    /// Set the maximum depth for recursive sub-component unfolding.
    /// The default is 64. Lower values are useful for tests to keep execution fast.
    ///
    /// # Panics
    /// Panics if `depth == 0`. At depth 0 the guard check fires before any child entity
    /// is created, so parent let-bindings referencing `child.*` would silently resolve to
    /// Undef. Only values >= 1 are safe.
    pub fn set_max_unfold_depth(&mut self, depth: usize) {
        assert!(depth >= 1, "max_unfold_depth must be >= 1");
        self.max_unfold_depth = depth;
    }

    /// Set the maximum total nodes created during recursive sub-component unfolding.
    /// The default is 10,000. This prevents exponential blowup when templates have
    /// multiple recursive subs (B subs × D depth = B^D nodes without this limit).
    ///
    /// # Panics
    /// Panics if `limit == 0`.
    pub fn set_max_unfold_nodes(&mut self, limit: usize) {
        assert!(limit >= 1, "max_unfold_nodes must be >= 1");
        self.max_unfold_nodes = limit;
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

    /// Whether the engine has been initialized by a call to eval().
    pub fn is_initialized(&self) -> bool {
        self.eval_state.is_some()
    }

    /// Access the consolidated evaluation state (for testing/inspection).
    pub fn eval_state(&self) -> Option<&EvaluationState> {
        self.eval_state.as_ref()
    }

    /// Access the current snapshot (for testing/inspection).
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.eval_state.as_ref().map(|s| &s.snapshot)
    }

    /// Access the eval set from the last eval() or edit_param() call.
    pub fn last_eval_set(&self) -> &[NodeId] {
        &self.last_eval_set
    }

    /// Access the event journal (for testing/inspection).
    pub fn journal(&self) -> &EventJournal {
        &self.journal
    }

    /// Activate a purpose by name against a target entity.
    ///
    /// Looks up the compiled purpose by `purpose_name`, then injects its
    /// constraints into the current evaluation graph. The injected constraint
    /// node IDs use a `"purpose:<purpose_name>@<entity_ref>"` entity prefix to
    /// avoid collisions with structure-level constraints.
    ///
    /// Requires a prior call to `eval()` so an evaluation state exists.
    /// If the purpose is already active, this is a no-op.
    pub fn activate_purpose(&mut self, purpose_name: &str, entity_ref: &str) {
        // No-op if already active
        if self.active_purposes.contains_key(purpose_name) {
            return;
        }

        // Look up the compiled purpose
        let purpose = match self
            .compiled_purposes
            .iter()
            .find(|p| p.name == purpose_name)
        {
            Some(p) => p.clone(),
            None => return, // Purpose not found — silently ignore
        };

        // Get mutable access to the evaluation state
        let state = match self.eval_state.as_mut() {
            Some(s) => s,
            None => return, // No eval state — silently ignore
        };

        // Build a unique entity prefix for the purpose-injected constraints
        let purpose_entity = format!("purpose:{}@{}", purpose_name, entity_ref);

        // Rewrite compiled expressions: substitute ValueCellId(purpose_name, param)
        // with ValueCellId(entity_ref, param) so references resolve to existing
        // value cells in the evaluation graph.
        let mut rewritten_constraints = purpose.constraints.clone();
        for constraint in &mut rewritten_constraints {
            constraint.expr.remap_entity(purpose_name, entity_ref);
        }

        let rewritten_objective = purpose.objective.clone().map(|mut obj| {
            match &mut obj {
                OptimizationObjective::Minimize(expr) | OptimizationObjective::Maximize(expr) => {
                    expr.remap_entity(purpose_name, entity_ref);
                }
            }
            obj
        });

        // Inject each of the purpose's compiled constraints into the evaluation graph
        let mut injected_ids = Vec::new();
        for (i, constraint) in rewritten_constraints.iter().enumerate() {
            let constraint_id = ConstraintNodeId::new(&purpose_entity, i as u32);
            let node = crate::graph::ConstraintNodeData {
                id: constraint_id.clone(),
                label: constraint.label.clone(),
                expr: constraint.expr.clone(),
                content_hash: ContentHash::of_str(&format!(
                    "purpose:{}:constraint:{}",
                    purpose_name, i
                )),
            };
            state
                .snapshot
                .graph
                .constraints
                .insert(constraint_id.clone(), node);
            injected_ids.push(constraint_id);
        }

        // Update demand registry: demand each newly injected constraint node.
        for id in &injected_ids {
            self.demand.add_demand(NodeId::Constraint(id.clone()));
        }

        self.active_purposes
            .insert(purpose_name.to_string(), injected_ids);

        // Rebuild infrastructure so incremental eval (edit_param) propagates
        // through purpose constraint dependencies correctly.
        //
        // We reborrow eval_state mutably here — the immutable borrow (`state`)
        // created earlier was already released after inserting into the graph.
        if let Some(state) = self.eval_state.as_mut() {
            state.reverse_index = ReverseDependencyIndex::build_from_graph(&state.snapshot.graph);
            state.trace_map = crate::deps::build_trace_map(&state.snapshot.graph);
        }
        if let Some(state) = self.eval_state.as_ref() {
            self.demand.rebuild_cone(&state.snapshot.graph);
        }

        // Inject the optimization objective if the purpose has one
        if let Some(ref objective) = rewritten_objective {
            self.active_objective_map
                .insert(purpose_name.to_string(), objective.clone());
        }
    }

    /// Deactivate a purpose by name.
    ///
    /// Removes the constraints and objectives that were injected by `activate_purpose`.
    /// If the purpose is not active, this is a no-op.
    pub fn deactivate_purpose(&mut self, purpose_name: &str) {
        // Look up and remove the injected constraint IDs
        let injected_ids = match self.active_purposes.remove(purpose_name) {
            Some(ids) => ids,
            None => return, // Not active — no-op
        };

        // Update demand registry: remove demand for each ejected constraint node.
        for id in &injected_ids {
            self.demand.remove_demand(&NodeId::Constraint(id.clone()));
        }

        // Remove each injected constraint from the evaluation graph, then
        // rebuild the infrastructure so subsequent edit_param() calls no longer
        // route through purpose constraint dependencies.
        if let Some(state) = self.eval_state.as_mut() {
            for constraint_id in &injected_ids {
                state.snapshot.graph.constraints.remove(constraint_id);
            }
            state.reverse_index = ReverseDependencyIndex::build_from_graph(&state.snapshot.graph);
            state.trace_map = crate::deps::build_trace_map(&state.snapshot.graph);
        }
        if let Some(state) = self.eval_state.as_ref() {
            self.demand.rebuild_cone(&state.snapshot.graph);
        }

        // Remove the objective if one was injected
        self.active_objective_map.remove(purpose_name);
    }

    /// Check whether a purpose is currently active.
    pub fn is_purpose_active(&self, purpose_name: &str) -> bool {
        self.active_purposes.contains_key(purpose_name)
    }

    /// Returns the currently active optimization objectives (injected by purposes).
    pub fn active_objectives(&self) -> Vec<&OptimizationObjective> {
        self.active_objective_map.values().collect()
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
    ) -> Result<ConcurrentEditSetup, EngineError> {
        let state = self
            .eval_state
            .as_ref()
            .ok_or(EngineError::NotInitialized)?;

        // Clone snapshot (O(1) via PersistentMap)
        let parent_id = state.snapshot.id;
        let mut new_snapshot_values = state.snapshot.values.clone();

        // Update the changed cell's value in snapshot
        new_snapshot_values.insert(
            cell.clone(),
            (new_value.clone(), DeterminacyState::Determined),
        );

        // Compute dirty cone and eval set
        let mut changed_set = HashSet::new();
        changed_set.insert(cell.clone());
        let dirty_cone = crate::dirty::compute_dirty_cone(&changed_set, &state.reverse_index);
        let eval_set = crate::dirty::compute_eval_set(&dirty_cone, &self.demand, &state.trace_map);

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

        Ok(ConcurrentEditSetup {
            eval_set,
            graph: state.snapshot.graph.clone(),
            values,
            snapshot_values: new_snapshot_values,
            traces: state.trace_map.clone(),
            reverse_index: state.reverse_index.clone(),
            previous_hashes,
            version: VersionId(version_id),
            snapshot_id: SnapshotId(snapshot_id),
            parent_snapshot_id: parent_id,
            changed_cells: changed_set,
            functions: self.functions.clone(),
            meta_map: Arc::new(self.meta_map.clone()),
            objective: self.objectives.get(&cell.entity).cloned(),
        })
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
    ) -> Vec<Diagnostic> {
        let ConcurrentEditResult {
            values: result_values,
            snapshot_values,
            node_results,
            actual_eval_set,
            skipped,
            resolved_params,
            diagnostics: result_diagnostics,
        } = result;

        // Record cache entries and journal events for each evaluated node
        for node_result in &node_results {
            let start = Instant::now();
            self.journal.record(EvalEvent {
                timestamp: start,
                node_id: node_result.node.clone(),
                kind: EventKind::Started,
                version: setup.version,
                payload: None,
            });

            let trace = node_result.trace.clone();
            let cached_result =
                CachedResult::Value(node_result.value.clone(), node_result.determinacy);
            self.cache.record_evaluation(
                node_result.node.clone(),
                cached_result,
                setup.version,
                trace,
            );

            self.journal.record(EvalEvent {
                timestamp: Instant::now(),
                node_id: node_result.node.clone(),
                kind: EventKind::Completed {
                    outcome: node_result.outcome,
                },
                version: setup.version,
                payload: Some(EventPayload::Duration(start.elapsed())),
            });
        }

        // Restore freshness to Final for skipped nodes
        for node_id in &skipped {
            self.cache.restore_final(node_id);
        }

        // Commit solver-resolved auto param values to engine state.
        // These were computed by resolve_concurrent_edit but must only
        // be persisted here so that resolve remains side-effect-free.
        for (id, val) in &resolved_params {
            self.param_overrides.insert(id.clone(), val.clone());
        }

        // Clone old snapshot values before updating (O(1) PersistentMap clone).
        // This must happen before the new snapshot values are installed so that
        // re_elaborate_collections can compare old vs. new counts.
        let old_snapshot_values = self
            .eval_state
            .as_ref()
            .expect("apply_concurrent_edit requires eval_state from eval()")
            .snapshot
            .values
            .clone();

        // Build updated snapshot
        let mut new_snapshot = self
            .eval_state
            .as_ref()
            .expect("apply_concurrent_edit requires eval_state from eval()")
            .snapshot
            .clone();
        new_snapshot.id = setup.snapshot_id;
        new_snapshot.version = setup.version;
        new_snapshot.values = snapshot_values;
        new_snapshot.provenance = SnapshotProvenance::Edit {
            changed: setup.changed_cells.clone(),
            parent: setup.parent_snapshot_id,
        };

        // Run collection count re-elaboration using disjoint field borrows.
        // setup.functions / setup.meta_map are used instead of self.functions /
        // self.meta_map so that &mut self.preserved_counts can be passed
        // simultaneously without borrow conflicts.
        let mut values = result_values;
        let mut diagnostics = result_diagnostics;
        Self::re_elaborate_collections(
            &mut self.preserved_counts,
            &old_snapshot_values,
            &mut new_snapshot,
            &mut values,
            &setup.functions,
            &*setup.meta_map,
            &mut diagnostics,
        );

        // Commit snapshot and eval set
        self.eval_state
            .as_mut()
            .expect("apply_concurrent_edit requires eval_state from eval()")
            .snapshot = new_snapshot;
        self.last_eval_set = actual_eval_set;

        diagnostics
    }

    /// Run auto-resolution on a concurrent edit result.
    ///
    /// If a solver is present and constraints governing auto params are in the
    /// dirty cone (computed from changed_cells), re-runs the solver and updates
    /// the result's values and snapshot_values. Also propagates changes to
    /// dependent let bindings via a second dirty cone computation.
    ///
    /// Called between scheduler completion and apply_concurrent_edit() to insert
    /// resolution into the concurrent pipeline. The solver runs synchronously
    /// (NLopt is single-threaded), which is architecturally clean: concurrent
    /// evaluation handles parallelizable value nodes, then the solver runs
    /// sequentially with all values finalized.
    pub fn resolve_concurrent_edit(
        &mut self,
        setup: &ConcurrentEditSetup,
        result: &mut ConcurrentEditResult,
    ) -> (HashMap<ValueCellId, reify_types::Value>, Vec<Diagnostic>) {
        let mut resolved_params = HashMap::new();
        let mut diagnostics = Vec::new();

        if let Some(ref solver) = self.solver {
            let state = self
                .eval_state
                .as_ref()
                .expect("resolve_concurrent_edit requires eval_state from eval()");
            let reverse_index = &state.reverse_index;
            let trace_map = &state.trace_map;

            // Group auto params by entity (template) name
            let mut entity_groups: HashMap<String, (Vec<AutoParam>, HashSet<ValueCellId>)> =
                HashMap::new();

            for (_, node) in setup.graph.value_cells.iter() {
                if node.kind == ValueCellKind::Auto {
                    let entry = entity_groups
                        .entry(node.id.entity.clone())
                        .or_insert_with(|| (Vec::new(), HashSet::new()));
                    entry.0.push(AutoParam {
                        id: node.id.clone(),
                        param_type: node.cell_type.clone(),
                        bounds: None,
                    });
                    entry.1.insert(node.id.clone());
                }
            }

            // Compute dirty cone from changed cells
            let dirty_cone =
                crate::dirty::compute_dirty_cone(&setup.changed_cells, reverse_index);

            // Union of all resolved auto param IDs across groups for second wave
            let mut all_resolved_ids: HashSet<ValueCellId> = HashSet::new();

            // Snapshot current values BEFORE the loop so each group's solver
            // receives the same baseline — preventing cross-group contamination
            // where one group's resolved values leak into another group's input.
            let snapshot_values = result.values.clone();

            // Solve each entity group independently
            for (scope_name, (auto_param_list, auto_ids)) in &entity_groups {
                // Find constraints referencing this group's auto params
                let filtered_constraints: Vec<_> = setup
                    .graph
                    .constraints
                    .iter()
                    .filter(|(_, cnode)| {
                        let trace = extract_dependency_trace(&cnode.expr);
                        trace.reads.iter().any(|r| auto_ids.contains(r))
                    })
                    .map(|(_, cnode)| (cnode.id.clone(), cnode.expr.clone()))
                    .collect();

                // Check if any of those constraints are in the dirty cone
                let constraints_dirty = filtered_constraints
                    .iter()
                    .any(|(cid, _)| dirty_cone.contains(&NodeId::Constraint(cid.clone())));

                if !constraints_dirty {
                    continue;
                }

                // Look up the template-native objective by entity name.
                let objective = self.objectives.get(scope_name).cloned();

                let problem = ResolutionProblem {
                    auto_params: auto_param_list.clone(),
                    constraints: filtered_constraints,
                    current_values: snapshot_values.clone(),
                    objective,
                    functions: setup.functions.clone(),
                };

                match solver.solve(&problem) {
                    SolveResult::Solved {
                        values: solver_values,
                    } => {
                        for (id, val) in &solver_values {
                            result.values.insert(id.clone(), val.clone());
                            resolved_params.insert(id.clone(), val.clone());
                            all_resolved_ids.insert(id.clone());

                            result.snapshot_values.insert(
                                id.clone(),
                                (val.clone(), DeterminacyState::Determined),
                            );

                            let node_id = NodeId::Value(id.clone());
                            let trace = DependencyTrace::default();
                            let cached_result =
                                CachedResult::Value(val.clone(), DeterminacyState::Determined);
                            self.cache.record_evaluation(
                                node_id,
                                cached_result,
                                setup.version,
                                trace,
                            );
                        }
                    }
                    SolveResult::Infeasible {
                        diagnostics: solver_diags,
                    } => {
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

            // ── Second propagation wave (once, with union of all resolved IDs) ──
            // Re-resolved auto params may have changed value. Let bindings
            // depending on them may NOT be in the original dirty cone.
            if !all_resolved_ids.is_empty() {
                let wave2_dirty =
                    crate::dirty::compute_dirty_cone(&all_resolved_ids, reverse_index);
                let wave2_eval =
                    crate::dirty::compute_eval_set(&wave2_dirty, &self.demand, trace_map);

                for node_id in &wave2_eval {
                    if let NodeId::Value(vcid) = node_id
                        && let Some(node) = setup.graph.value_cells.get(vcid)
                        && let Some(ref expr) = node.default_expr
                    {
                        let val = reify_expr::eval_expr(
                            expr,
                            &reify_expr::EvalContext::new(&result.values, &setup.functions)
                                .with_meta(&setup.meta_map),
                        );
                        result.values.insert(vcid.clone(), val.clone());
                        result.snapshot_values.insert(
                            vcid.clone(),
                            (val.clone(), DeterminacyState::Determined),
                        );

                        let trace = extract_dependency_trace(expr);
                        let cached_result =
                            CachedResult::Value(val, DeterminacyState::Determined);
                        self.cache.record_evaluation(
                            node_id.clone(),
                            cached_result,
                            setup.version,
                            trace,
                        );
                    }
                }
            }
        }

        (resolved_params, diagnostics)
    }

    /// Set a parameter override and invalidate cache entries that depend on it.
    pub fn set_param_and_invalidate(&mut self, param: &ValueCellId, value: reify_types::Value) {
        self.param_overrides.insert(param.clone(), value);
        // Mark the param's own cache entry as dirty
        let param_node = NodeId::Value(param.clone());
        self.cache.invalidate(&param_node);
        // Mark all nodes that depend on this param as dirty
        self.cache
            .invalidate_dependents(std::slice::from_ref(param));
    }

    /// Evaluate a compiled module, returning computed values.
    ///
    /// This is a cold-start evaluation that builds a new Snapshot and
    /// dependency structures. Subsequent calls to edit_param() can perform
    /// incremental re-evaluation using these structures.
    pub fn eval(&mut self, module: &CompiledModule) -> EvalResult {
        // Store functions and purposes for this module (used by edit_param and purpose activation)
        self.functions = module.functions.clone();
        // Extend with prelude functions so user expressions can call stdlib functions.
        for pm in self.prelude {
            self.functions.extend(pm.functions.iter().cloned());
        }
        self.compiled_purposes = module.compiled_purposes.clone();
        // Clear stale purpose state from previous eval() calls — the fresh
        // snapshot discards all purpose-injected constraints/objectives.
        self.active_purposes.clear();
        self.active_objective_map.clear();
        // Clear incremental-edit metadata so that preserved collection counts
        // from a previous session do not bleed into this cold evaluation.
        self.preserved_counts.clear();
        // Build meta_map: template name → meta key/value pairs.
        // Only includes templates with non-empty meta blocks.
        self.meta_map = module
            .templates
            .iter()
            .filter(|t| !t.meta.is_empty())
            .map(|t| (t.name.clone(), t.meta.clone()))
            .collect();
        let functions = &module.functions;

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

        // Evaluate field declarations first: they must be available in the
        // values map before templates are evaluated, because structure
        // expressions may reference fields (e.g., `sample(my_field, point)`).
        for field in &module.fields {
            let lambda_value = match &field.source {
                reify_compiler::CompiledFieldSource::Analytical { expr } => {
                    let ctx =
                        reify_expr::EvalContext::new(&values, functions).with_meta(&self.meta_map);
                    let val = reify_expr::eval_expr(expr, &ctx);
                    Box::new(val)
                }
                reify_compiler::CompiledFieldSource::Composed { expr } => {
                    let ctx =
                        reify_expr::EvalContext::new(&values, functions).with_meta(&self.meta_map);
                    let val = reify_expr::eval_expr(expr, &ctx);
                    Box::new(val)
                }
                reify_compiler::CompiledFieldSource::Sampled { .. }
                | reify_compiler::CompiledFieldSource::Imported => Box::new(Value::Undef),
            };

            let source_kind = match &field.source {
                reify_compiler::CompiledFieldSource::Analytical { .. } => {
                    reify_types::FieldSourceKind::Analytical
                }
                reify_compiler::CompiledFieldSource::Sampled { .. } => {
                    reify_types::FieldSourceKind::Sampled
                }
                reify_compiler::CompiledFieldSource::Composed { .. } => {
                    reify_types::FieldSourceKind::Composed
                }
                reify_compiler::CompiledFieldSource::Imported => {
                    reify_types::FieldSourceKind::Imported
                }
            };

            let field_value = Value::Field {
                domain_type: field.domain_type.clone(),
                codomain_type: field.codomain_type.clone(),
                source: source_kind,
                lambda: lambda_value,
            };

            let field_id = ValueCellId::new(FIELD_ENTITY_PREFIX, &field.name);
            values.insert(field_id.clone(), field_value.clone());
            snapshot
                .values
                .insert(field_id, (field_value, DeterminacyState::Determined));
        }

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

                    let val = reify_expr::eval_expr(
                        expr,
                        &reify_expr::EvalContext::new(&values, functions)
                            .with_meta(&self.meta_map)
                            .with_determinacy(&snapshot.values),
                    );
                    values.insert(cell.id.clone(), val.clone());

                    // Update snapshot values
                    snapshot
                        .values
                        .insert(cell.id.clone(), (val.clone(), DeterminacyState::Determined));

                    // Record in cache
                    let trace = DependencyTrace::default();
                    let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
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

            // Second pass: evaluate Let bindings in topological order
            // (handles forward references where a let declared earlier
            //  depends on a let declared later)
            {
                let meta_map = self.meta_map.clone();
                self.evaluate_let_bindings(
                    template,
                    &mut values,
                    &mut snapshot,
                    version_id,
                    functions,
                    &meta_map,
                    &mut diagnostics,
                );
            }

            // Third pass: evaluate guarded groups.
            // Guard cells are Let-kind synthetic cells — evaluate their expressions,
            // then conditionally evaluate members based on guard truth value.
            for group in &template.guarded_groups {
                // Evaluate the guard cell expression
                let guard_val = reify_expr::eval_expr(
                    &group.guard_expr,
                    &reify_expr::EvalContext::new(&values, functions)
                        .with_meta(&self.meta_map)
                        .with_determinacy(&snapshot.values),
                );
                values.insert(group.guard_value_cell.clone(), guard_val.clone());

                let guard_determinacy = match &guard_val {
                    Value::Bool(_) => DeterminacyState::Determined,
                    _ => DeterminacyState::Undetermined,
                };
                snapshot.values.insert(
                    group.guard_value_cell.clone(),
                    (guard_val.clone(), guard_determinacy),
                );

                let guard_is_true = matches!(&guard_val, Value::Bool(true));
                let guard_is_false = matches!(&guard_val, Value::Bool(false));

                // Evaluate members (active when guard is true)
                for cell in &group.members {
                    if guard_is_true {
                        // Evaluate normally
                        if cell.kind == ValueCellKind::Param || cell.kind == ValueCellKind::Let {
                            if let Some(ref expr) = cell.default_expr {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &reify_expr::EvalContext::new(&values, functions)
                                        .with_meta(&self.meta_map)
                                        .with_determinacy(&snapshot.values),
                                );
                                values.insert(cell.id.clone(), val.clone());
                                snapshot
                                    .values
                                    .insert(cell.id.clone(), (val, DeterminacyState::Determined));
                            } else {
                                values.insert(cell.id.clone(), Value::Undef);
                                snapshot.values.insert(
                                    cell.id.clone(),
                                    (Value::Undef, DeterminacyState::Undetermined),
                                );
                            }
                        } else if cell.kind == ValueCellKind::Auto {
                            values.insert(cell.id.clone(), Value::Undef);
                            snapshot
                                .values
                                .insert(cell.id.clone(), (Value::Undef, DeterminacyState::Auto));
                        }
                    } else {
                        // Guard is false or Undef — member is inactive
                        values.insert(cell.id.clone(), Value::Undef);
                        let det = if cell.kind == ValueCellKind::Auto {
                            DeterminacyState::Auto
                        } else {
                            DeterminacyState::Undetermined
                        };
                        snapshot
                            .values
                            .insert(cell.id.clone(), (Value::Undef, det));
                    }
                }

                // Evaluate else_members (active when guard is false)
                for cell in &group.else_members {
                    if guard_is_false {
                        if cell.kind == ValueCellKind::Param || cell.kind == ValueCellKind::Let {
                            if let Some(ref expr) = cell.default_expr {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &reify_expr::EvalContext::new(&values, functions)
                                        .with_meta(&self.meta_map)
                                        .with_determinacy(&snapshot.values),
                                );
                                values.insert(cell.id.clone(), val.clone());
                                snapshot
                                    .values
                                    .insert(cell.id.clone(), (val, DeterminacyState::Determined));
                            } else {
                                values.insert(cell.id.clone(), Value::Undef);
                                snapshot.values.insert(
                                    cell.id.clone(),
                                    (Value::Undef, DeterminacyState::Undetermined),
                                );
                            }
                        } else if cell.kind == ValueCellKind::Auto {
                            values.insert(cell.id.clone(), Value::Undef);
                            snapshot
                                .values
                                .insert(cell.id.clone(), (Value::Undef, DeterminacyState::Auto));
                        }
                    } else {
                        // Guard is true or Undef — else member is inactive
                        values.insert(cell.id.clone(), Value::Undef);
                        let det = if cell.kind == ValueCellKind::Auto {
                            DeterminacyState::Auto
                        } else {
                            DeterminacyState::Undetermined
                        };
                        snapshot
                            .values
                            .insert(cell.id.clone(), (Value::Undef, det));
                    }
                }
            }
        }

        // Sub-component elaboration: evaluate child template params/lets
        // for each sub_component in each template.
        for template in &module.templates {
            for sub in &template.sub_components {
                // Find the referenced child template by name
                let child_template = match module
                    .templates
                    .iter()
                    .find(|t| t.name == sub.structure_name)
                {
                    Some(t) => t,
                    None => {
                        diagnostics.push(Diagnostic::error(format!(
                            "sub-component \"{}\" references unknown structure \"{}\"",
                            sub.name, sub.structure_name
                        )));
                        continue;
                    }
                };

                // Collection sub: determine count, then elaborate N instances
                if sub.is_collection {
                    let count = if let Some(ref count_cell_id) = sub.count_cell {
                        // The count cell value should already be evaluated (it's a let binding)
                        match values.get(count_cell_id) {
                            Some(Value::Int(n)) => Some(*n),
                            _ => None,
                        }
                    } else {
                        None
                    };

                    if let Some(n) = count {
                        for idx in 0..n {
                            let scoped_entity = format!("{}.{}[{}]", template.name, sub.name, idx);
                            elaborate_child_instance(
                                &mut values,
                                &mut snapshot,
                                functions,
                                &mut self.journal,
                                &mut self.cache,
                                version_id,
                                child_template,
                                &scoped_entity,
                                &sub.args,
                                &self.meta_map,
                                &mut diagnostics,
                            );
                        }

                        // Create per-member synthetic lists: __list_{name}__{member} for each value cell
                        for child_cell in &child_template.value_cells {
                            let member_items: Vec<Value> = (0..n)
                                .map(|idx| {
                                    let scoped_id = ValueCellId::new(
                                        format!("{}.{}[{}]", template.name, sub.name, idx),
                                        &child_cell.id.member,
                                    );
                                    values.get(&scoped_id).cloned().unwrap_or(Value::Undef)
                                })
                                .collect();
                            let member_list_id = ValueCellId::new(
                                &template.name,
                                format!("__list_{}__{}", sub.name, child_cell.id.member),
                            );
                            let member_list_val = Value::List(member_items);
                            values.insert(member_list_id.clone(), member_list_val.clone());
                            snapshot.values.insert(
                                member_list_id,
                                (member_list_val, DeterminacyState::Determined),
                            );
                        }
                    }
                    // If count is None (Undef), no instances are created
                    continue;
                }

                // Recursive sub: evaluate guard before elaborating, then unfold recursively.
                if template.is_recursive && sub.guard_expr.is_some() {
                    let mut unfold_budget = self.max_unfold_nodes;
                    unfold_recursive_sub(
                        &mut values,
                        &mut snapshot,
                        functions,
                        &mut self.journal,
                        &mut self.cache,
                        version_id,
                        template,       // scope_template: owns `sub` (guard/arg refs match)
                        child_template, // target template for Phase 1 instantiation
                        sub,
                        &template.name,
                        0,
                        self.max_unfold_depth,
                        &self.meta_map,
                        &mut diagnostics,
                        &module.templates,
                        &mut unfold_budget,
                    );
                    continue;
                }

                // Build scoped entity prefix: "ParentName.sub_name"
                let scoped_entity = format!("{}.{}", template.name, sub.name);

                elaborate_child_instance(
                    &mut values,
                    &mut snapshot,
                    functions,
                    &mut self.journal,
                    &mut self.cache,
                    version_id,
                    child_template,
                    &scoped_entity,
                    &sub.args,
                    &self.meta_map,
                    &mut diagnostics,
                );
            }

            // Re-evaluate let bindings that may depend on sub-component cells:
            // - collection subs create __list_* synthetic cells
            // - regular subs create {parent}.{sub}.{member} cells via elaborate_child_instance
            // Both become available only after elaboration, so re-evaluate if any subs exist.
            if !template.sub_components.is_empty() {
                let meta_map = self.meta_map.clone();
                self.evaluate_let_bindings(
                    template,
                    &mut values,
                    &mut snapshot,
                    version_id,
                    functions,
                    &meta_map,
                    &mut diagnostics,
                );
            }
        }

        // Resolution phase: resolve auto params using the constraint solver.
        let mut resolved_params = HashMap::new();
        if self.solver.is_some() {
            // Refresh template-native objectives so edit_param() can access them.
            self.objectives.clear();
            for template in &module.templates {
                if let Some(obj) = &template.objective {
                    self.objectives.insert(template.name.clone(), obj.clone());
                }
            }
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
                    objective: template.objective.clone(),
                    functions: module.functions.clone(),
                };

                let parent_snap_id = snapshot.id;
                // Use a temporary borrow of the solver so the reference
                // doesn't outlive the solve() call — this allows &mut self
                // for evaluate_let_bindings below.
                let solve_result = self.solver.as_ref().unwrap().solve(&problem);

                match solve_result {
                    SolveResult::Solved {
                        values: solver_values,
                    } => {
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
                            snapshot
                                .values
                                .insert(id.clone(), (val.clone(), DeterminacyState::Determined));

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

                        // Re-run let binding evaluation in topological order
                        let meta_map = self.meta_map.clone();
                        self.evaluate_let_bindings(
                            template,
                            &mut values,
                            &mut snapshot,
                            res_version_id,
                            &module.functions,
                            &meta_map,
                            &mut diagnostics,
                        );
                    }
                    SolveResult::Infeasible {
                        diagnostics: solver_diags,
                    } => {
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
        self.eval_state = Some(EvaluationState {
            snapshot,
            reverse_index,
            trace_map,
        });
        self.demand = demand;
        self.last_eval_set = Vec::new(); // Cold start: no incremental eval set

        EvalResult {
            values,
            diagnostics,
            resolved_params,
        }
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
    ) -> Result<EvalResult, EngineError> {
        let functions = self.functions.clone();
        let state = self
            .eval_state
            .as_ref()
            .ok_or(EngineError::NotInitialized)?;

        // Clone snapshot and extract references (O(1) via PersistentMap)
        let parent_id = state.snapshot.id;
        let mut new_snapshot = state.snapshot.clone();

        // Compute dirty cone and eval set while state borrow is active
        let mut changed_set = std::collections::HashSet::new();
        changed_set.insert(cell.clone());
        let dirty_cone = crate::dirty::compute_dirty_cone(&changed_set, &state.reverse_index);
        let eval_set = crate::dirty::compute_eval_set(&dirty_cone, &self.demand, &state.trace_map);

        // Seed has_changed_parent from dependents of the changed param
        let mut has_changed_parent: std::collections::HashSet<NodeId> =
            std::collections::HashSet::new();
        for dependent in state.reverse_index.dependents_of(&cell) {
            has_changed_parent.insert(dependent.clone());
        }
        // Release the immutable borrow of eval_state so we can mutate later
        let _ = state;

        // Update snapshot ID, version, and provenance
        let snapshot_id = self.next_snapshot_id;
        self.next_snapshot_id += 1;
        let version_id = self.next_version_id;
        self.next_version_id += 1;
        new_snapshot.id = SnapshotId(snapshot_id);
        new_snapshot.version = VersionId(version_id);

        new_snapshot.provenance = SnapshotProvenance::Edit {
            changed: changed_set.clone(),
            parent: parent_id,
        };

        // Update the changed cell's value in snapshot
        new_snapshot.values.insert(
            cell.clone(),
            (new_value.clone(), DeterminacyState::Determined),
        );

        // Update the param's cache entry to match the snapshot.
        // The param is a source node (not in dirty_cone / eval_set), so its
        // cache entry would otherwise retain the stale value from initial eval().
        self.cache.record_evaluation(
            NodeId::Value(cell.clone()),
            CachedResult::Value(new_value.clone(), DeterminacyState::Determined),
            VersionId(version_id),
            crate::deps::DependencyTrace::default(),
        );

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

                let val = reify_expr::eval_expr(
                    expr,
                    &reify_expr::EvalContext::new(&values, &functions).with_meta(&self.meta_map),
                );
                values.insert(vcid.clone(), val.clone());
                new_snapshot
                    .values
                    .insert(vcid.clone(), (val.clone(), DeterminacyState::Determined));

                // Record in cache and check for early cutoff
                let trace = extract_dependency_trace(expr);
                let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
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
                {
                    let dependents = self
                        .eval_state
                        .as_ref()
                        .unwrap()
                        .reverse_index
                        .dependents_of(vcid);
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

        // ── Guard re-elaboration phase ────────────────────────────────
        // If any structure_controlling cell changed, re-evaluate guarded groups
        // to flip which branch is active/inactive, and recompute fingerprint.
        {
            let graph = &new_snapshot.graph;
            let has_dirty_guards = graph.structure_controlling.iter().any(|sc_id| {
                dirty_cone.contains(&NodeId::Value(sc_id.clone())) || changed_set.contains(sc_id)
            });

            if has_dirty_guards {
                for group in &graph.guarded_groups {
                    // Re-evaluate the guard cell's expression
                    let guard_val = if let Some(node) = graph.value_cells.get(&group.guard_cell) {
                        if let Some(ref expr) = node.default_expr {
                            reify_expr::eval_expr(
                                expr,
                                &reify_expr::EvalContext::new(&values, &functions)
                                    .with_meta(&self.meta_map),
                            )
                        } else {
                            Value::Undef
                        }
                    } else {
                        Value::Undef
                    };
                    values.insert(group.guard_cell.clone(), guard_val.clone());
                    let guard_det = if matches!(&guard_val, Value::Bool(_)) {
                        DeterminacyState::Determined
                    } else {
                        DeterminacyState::Undetermined
                    };
                    new_snapshot
                        .values
                        .insert(group.guard_cell.clone(), (guard_val.clone(), guard_det));

                    let is_true = matches!(&guard_val, Value::Bool(true));
                    let is_false = matches!(&guard_val, Value::Bool(false));

                    for mid in &group.members {
                        if is_true {
                            if let Some(node) = graph.value_cells.get(mid)
                                && let Some(ref expr) = node.default_expr
                            {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &reify_expr::EvalContext::new(&values, &functions)
                                        .with_meta(&self.meta_map),
                                );
                                values.insert(mid.clone(), val.clone());
                                new_snapshot
                                    .values
                                    .insert(mid.clone(), (val, DeterminacyState::Determined));
                            }
                        } else {
                            // Skip Auto params — their lifecycle is managed by the
                            // solver, not guard activation/deactivation.
                            let is_auto = graph
                                .value_cells
                                .get(mid)
                                .is_some_and(|n| n.kind == ValueCellKind::Auto);
                            if !is_auto {
                                values.insert(mid.clone(), Value::Undef);
                                new_snapshot.values.insert(
                                    mid.clone(),
                                    (Value::Undef, DeterminacyState::Undetermined),
                                );
                            }
                        }
                    }
                    for mid in &group.else_members {
                        if is_false {
                            if let Some(node) = graph.value_cells.get(mid)
                                && let Some(ref expr) = node.default_expr
                            {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &reify_expr::EvalContext::new(&values, &functions)
                                        .with_meta(&self.meta_map),
                                );
                                values.insert(mid.clone(), val.clone());
                                new_snapshot
                                    .values
                                    .insert(mid.clone(), (val, DeterminacyState::Determined));
                            }
                        } else {
                            // Skip Auto params — their lifecycle is managed by the
                            // solver, not guard activation/deactivation.
                            let is_auto = graph
                                .value_cells
                                .get(mid)
                                .is_some_and(|n| n.kind == ValueCellKind::Auto);
                            if !is_auto {
                                values.insert(mid.clone(), Value::Undef);
                                new_snapshot.values.insert(
                                    mid.clone(),
                                    (Value::Undef, DeterminacyState::Undetermined),
                                );
                            }
                        }
                    }
                }

                // Recompute topology fingerprint including guard states
                let guard_state_hash = {
                    let hashes = graph.guarded_groups.iter().map(|g| {
                        let gv = values.get(&g.guard_cell).cloned().unwrap_or(Value::Undef);
                        ContentHash::of_str(&format!("{:?}", gv))
                    });
                    ContentHash::combine_all(hashes)
                };
                new_snapshot.topology_fingerprint =
                    graph.topology_fingerprint().combine(guard_state_hash);
            }
        }

        // ── Resolution phase ───────────────────────────────────────────
        // If a solver is present, check whether any constraints governing
        // auto params are in the dirty cone. If so, re-run the solver
        // to update auto param values and propagate to dependents.
        let mut resolved_params = HashMap::new();
        let mut diagnostics = Vec::new();

        if let Some(ref solver) = self.solver {
            // Group auto params by entity (template) name
            let mut entity_groups: HashMap<String, (Vec<AutoParam>, HashSet<ValueCellId>)> =
                HashMap::new();

            for (_, node) in new_snapshot.graph.value_cells.iter() {
                if node.kind == ValueCellKind::Auto {
                    let entry = entity_groups
                        .entry(node.id.entity.clone())
                        .or_insert_with(|| (Vec::new(), HashSet::new()));
                    entry.0.push(AutoParam {
                        id: node.id.clone(),
                        param_type: node.cell_type.clone(),
                        bounds: None,
                    });
                    entry.1.insert(node.id.clone());
                }
            }

            // Union of all resolved auto param IDs across groups for second wave
            let mut all_resolved_ids: HashSet<ValueCellId> = HashSet::new();

            // Snapshot current values BEFORE the loop so each group's solver
            // receives the same baseline — preventing cross-group contamination
            // where one group's resolved values leak into another group's input.
            let snapshot_values = values.clone();

            // Solve each entity group independently
            for (scope_name, (auto_param_list, auto_ids)) in &entity_groups {
                // Find constraints referencing this group's auto params
                let filtered_constraints: Vec<_> = new_snapshot
                    .graph
                    .constraints
                    .iter()
                    .filter(|(_, cnode)| {
                        let trace = extract_dependency_trace(&cnode.expr);
                        trace.reads.iter().any(|r| auto_ids.contains(r))
                    })
                    .map(|(_, cnode)| (cnode.id.clone(), cnode.expr.clone()))
                    .collect();

                // Check if any of those constraints are in the dirty cone
                let constraints_dirty = filtered_constraints
                    .iter()
                    .any(|(cid, _)| dirty_cone.contains(&NodeId::Constraint(cid.clone())));

                if !constraints_dirty {
                    continue;
                }

                // Look up the template-native objective by entity name.
                let objective = self.objectives.get(scope_name).cloned();

                // Build ResolutionProblem and solve
                let problem = ResolutionProblem {
                    auto_params: auto_param_list.clone(),
                    constraints: filtered_constraints,
                    current_values: snapshot_values.clone(),
                    objective,
                    functions: functions.clone(),
                };

                match solver.solve(&problem) {
                    SolveResult::Solved {
                        values: solver_values,
                    } => {
                        for (id, val) in &solver_values {
                            values.insert(id.clone(), val.clone());
                            resolved_params.insert(id.clone(), val.clone());
                            all_resolved_ids.insert(id.clone());

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
                            let cached_result =
                                CachedResult::Value(val.clone(), DeterminacyState::Determined);
                            self.cache.record_evaluation(
                                node_id,
                                cached_result,
                                VersionId(version_id),
                                trace,
                            );
                        }
                    }
                    SolveResult::Infeasible {
                        diagnostics: solver_diags,
                    } => {
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

            // ── Second propagation wave (once, with union of all resolved IDs) ──
            // Re-resolved auto params may have changed value. Let bindings
            // depending on them may NOT be in the original dirty cone.
            // Guard: skip if eval_state is None (defensive; the early guard at
            // edit_param entry ensures this is unreachable, but an if-let is
            // consistent with the guard re-elaboration phase below which uses
            // .and_then for the same field).
            if !all_resolved_ids.is_empty()
                && let Some(es) = self.eval_state.as_ref()
            {
                let wave2_dirty =
                    crate::dirty::compute_dirty_cone(&all_resolved_ids, &es.reverse_index);
                let wave2_eval =
                    crate::dirty::compute_eval_set(&wave2_dirty, &self.demand, &es.trace_map);

                for node_id in &wave2_eval {
                    if let NodeId::Value(vcid) = node_id
                        && let Some(node) = new_snapshot.graph.value_cells.get(vcid)
                        && let Some(ref expr) = node.default_expr
                    {
                        let val = reify_expr::eval_expr(
                            expr,
                            &reify_expr::EvalContext::new(&values, &functions)
                                .with_meta(&self.meta_map),
                        );
                        values.insert(vcid.clone(), val.clone());
                        new_snapshot.values.insert(
                            vcid.clone(),
                            (val.clone(), DeterminacyState::Determined),
                        );

                        // Update cache for re-evaluated node
                        let trace = extract_dependency_trace(expr);
                        let cached_result =
                            CachedResult::Value(val, DeterminacyState::Determined);
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

        // ── Guard re-elaboration phase ──────────────────────────────────
        // If any structure-controlling (guard) cells changed boolean value,
        // re-evaluate affected guarded group members: activate the correct
        // branch (members or else_members) and deactivate the other.
        // Finally, recompute topology fingerprint to reflect guard state.
        {
            let guard_changed = new_snapshot.graph.guarded_groups.iter().any(|group| {
                let new_val = values.get(&group.guard_cell);
                let old_val = self
                    .eval_state
                    .as_ref()
                    .and_then(|s| s.snapshot.values.get(&group.guard_cell))
                    .map(|(v, _)| v);
                new_val != old_val
            });

            if guard_changed {
                // Re-evaluate each guarded group based on current guard values
                for group in new_snapshot.graph.guarded_groups.clone() {
                    let guard_val = values
                        .get(&group.guard_cell)
                        .cloned()
                        .unwrap_or(Value::Undef);
                    let guard_is_true = matches!(&guard_val, Value::Bool(true));
                    let guard_is_false = matches!(&guard_val, Value::Bool(false));

                    // Process members (active when guard is true)
                    for member_id in &group.members {
                        if guard_is_true {
                            // Re-evaluate member from its default_expr
                            if let Some(node) = new_snapshot.graph.value_cells.get(member_id)
                                && let Some(ref expr) = node.default_expr
                            {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &reify_expr::EvalContext::new(&values, &functions)
                                        .with_meta(&self.meta_map),
                                );
                                values.insert(member_id.clone(), val.clone());
                                new_snapshot
                                    .values
                                    .insert(member_id.clone(), (val, DeterminacyState::Determined));
                            }
                        } else {
                            // Deactivate: set to Undef — but skip Auto params whose
                            // lifecycle is managed by the solver, not guard activation.
                            let is_auto = new_snapshot
                                .graph
                                .value_cells
                                .get(member_id)
                                .is_some_and(|n| n.kind == ValueCellKind::Auto);
                            if !is_auto {
                                values.insert(member_id.clone(), Value::Undef);
                                new_snapshot.values.insert(
                                    member_id.clone(),
                                    (Value::Undef, DeterminacyState::Undetermined),
                                );
                            }
                        }
                    }

                    // Process else_members (active when guard is false)
                    for member_id in &group.else_members {
                        if guard_is_false {
                            // Re-evaluate else member from its default_expr
                            if let Some(node) = new_snapshot.graph.value_cells.get(member_id)
                                && let Some(ref expr) = node.default_expr
                            {
                                let val = reify_expr::eval_expr(
                                    expr,
                                    &reify_expr::EvalContext::new(&values, &functions)
                                        .with_meta(&self.meta_map),
                                );
                                values.insert(member_id.clone(), val.clone());
                                new_snapshot
                                    .values
                                    .insert(member_id.clone(), (val, DeterminacyState::Determined));
                            }
                        } else {
                            // Deactivate: set to Undef — but skip Auto params whose
                            // lifecycle is managed by the solver, not guard activation.
                            let is_auto = new_snapshot
                                .graph
                                .value_cells
                                .get(member_id)
                                .is_some_and(|n| n.kind == ValueCellKind::Auto);
                            if !is_auto {
                                values.insert(member_id.clone(), Value::Undef);
                                new_snapshot.values.insert(
                                    member_id.clone(),
                                    (Value::Undef, DeterminacyState::Undetermined),
                                );
                            }
                        }
                    }
                }

                // Recompute topology fingerprint to include guard states
                let base_fingerprint = new_snapshot.graph.topology_fingerprint();
                let guard_state_hashes: Vec<ContentHash> = new_snapshot
                    .graph
                    .guarded_groups
                    .iter()
                    .map(|g| {
                        let val = values.get(&g.guard_cell).cloned().unwrap_or(Value::Undef);
                        ContentHash::of_str(&format!("guard:{}={:?}", g.guard_cell, val))
                    })
                    .collect();
                let guard_states_hash = ContentHash::combine_all(guard_state_hashes);
                new_snapshot.topology_fingerprint = base_fingerprint.combine(guard_states_hash);
            }
        }

        // ── Collection count re-elaboration phase ─────────────────────
        // If any structure_controlling cell is a collection count cell and
        // its value changed, add/remove instances to match the new count.
        // O(1) PersistentMap clone — shares structure with the old snapshot.
        let old_snapshot_values = self
            .eval_state
            .as_ref()
            .unwrap()
            .snapshot
            .values
            .clone();
        Self::re_elaborate_collections(
            &mut self.preserved_counts,
            &old_snapshot_values,
            &mut new_snapshot,
            &mut values,
            &functions,
            &self.meta_map,
            &mut diagnostics,
        );

        // Store state (actual_eval_set excludes early-cutoff-skipped nodes)
        self.last_eval_set = actual_eval_set;
        self.eval_state.as_mut().unwrap().snapshot = new_snapshot;

        Ok(EvalResult {
            values,
            diagnostics,
            resolved_params,
        })
    }

    /// Run the collection count re-elaboration phase for a given snapshot transition.
    ///
    /// Compares collection count cells between `old_snapshot_values` and the working
    /// `values` map. For each count cell that changed:
    /// - If the new count is `Undef`, skips re-elaboration and records the
    ///   last-known-good count in `preserved_counts` (only when old count is Int).
    /// - Otherwise, removes instances from `[0..old_count)` and creates instances
    ///   from `[0..new_count)`, using `preserved_counts` to recover `old_count` when
    ///   the snapshot count is `Undef` (i.e., after an Int→Undef transition).
    ///
    /// This is an associated function (not `&mut self`) so callers can pass
    /// `&mut self.preserved_counts` and `&self.eval_state` as disjoint field borrows
    /// without conflicting with the mutable borrows of `new_snapshot` and `values`.
    fn re_elaborate_collections(
        preserved_counts: &mut HashMap<ValueCellId, i64>,
        old_snapshot_values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
        new_snapshot: &mut Snapshot,
        values: &mut ValueMap,
        functions: &[reify_types::CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let collection_subs = new_snapshot.graph.collection_subs.clone();
        for col_sub in &collection_subs {
            let new_count_val = values
                .get(&col_sub.count_cell)
                .cloned()
                .unwrap_or(Value::Undef);
            let old_count_val = old_snapshot_values
                .get(&col_sub.count_cell)
                .map(|(v, _)| v.clone())
                .unwrap_or(Value::Undef);

            if new_count_val == old_count_val {
                continue;
            }

            // Guard: skip re-elaboration when the new count is Undef.
            // The count cell hasn't been evaluated yet (incremental re-eval
            // order may not have reached it). Destroying existing instances
            // now would be destructive — preserve them until the count cell
            // resolves to a definite value.
            if matches!(new_count_val, Value::Undef) {
                // Record the last-known-good instance count so that a
                // subsequent Int(M) edit can use it as old_count to clean
                // up stale instances [M..N) left from the Int(N) era.
                // Only record when old_count is Int(n); if old_count is
                // already Undef, a previously recorded preserved count (from
                // an earlier Int→Undef transition) is still valid and must
                // not be overwritten with 0.
                if let Value::Int(n) = &old_count_val {
                    preserved_counts.insert(col_sub.count_cell.clone(), *n);
                }
                diagnostics.push(Diagnostic::warning(format!(
                    "Collection count cell `{}` is Undef; skipping re-elaboration to preserve existing instances",
                    col_sub.count_cell
                )));
                continue;
            }

            // Remove old instances from graph and snapshot
            let old_count = match &old_count_val {
                Value::Int(n) => *n,
                // Value::Undef means the count cell was Undef in the snapshot.
                // This can happen after an Int(N)→Undef transition where the
                // Undef guard fired and skipped updating the snapshot count.
                // Fall back to preserved_counts to recover the true instance
                // count (N), so we clean up instances [M..N) correctly.
                // If no preserved count exists, genuinely 0 instances existed.
                Value::Undef => preserved_counts
                    .get(&col_sub.count_cell)
                    .copied()
                    .unwrap_or(0),
                // Any other non-Int type is unexpected; treat as 0 but warn.
                other => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "Collection count cell `{}` had unexpected non-Int prior value {:?}; treating as 0",
                        col_sub.count_cell, other
                    )));
                    0
                }
            };
            for i in 0..old_count {
                let scoped_entity =
                    format!("{}.{}[{}]", col_sub.parent_entity, col_sub.sub_name, i);
                for (member, _, _, _) in &col_sub.child_value_cells {
                    let scoped_id = ValueCellId::new(&scoped_entity, member);
                    new_snapshot.graph.value_cells.remove(&scoped_id);
                    new_snapshot.values.remove(&scoped_id);
                    values.remove(&scoped_id);
                }
            }

            // Create new instances based on new count.
            // Note: Value::Undef is unreachable here — the guard above
            // continues before reaching this point when new_count is Undef.
            let new_count = match &new_count_val {
                Value::Int(n) => *n,
                // Undef is guarded above; this arm is a safety fallback.
                Value::Undef => 0,
                // Any other non-Int type is unexpected; treat as 0 but warn.
                other => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "Collection count cell `{}` has unexpected non-Int value {:?}; treating count as 0",
                        col_sub.count_cell, other
                    )));
                    0
                }
            };
            for i in 0..new_count {
                let scoped_entity =
                    format!("{}.{}[{}]", col_sub.parent_entity, col_sub.sub_name, i);
                for (member, kind, cell_type, default_expr) in &col_sub.child_value_cells {
                    let scoped_id = ValueCellId::new(&scoped_entity, member);
                    let id_hash = ContentHash::of_str(&format!("{}", scoped_id));
                    let expr_hash = default_expr
                        .as_ref()
                        .map(|e| e.content_hash)
                        .unwrap_or(ContentHash(0));
                    let node = crate::graph::ValueCellNode {
                        id: scoped_id.clone(),
                        kind: *kind,
                        cell_type: cell_type.clone(),
                        default_expr: default_expr.clone(),
                        content_hash: id_hash.combine(expr_hash),
                    };
                    new_snapshot.graph.value_cells.insert(scoped_id.clone(), node);

                    // Evaluate the cell
                    let val = if let Some(expr) = default_expr {
                        reify_expr::eval_expr(
                            expr,
                            &reify_expr::EvalContext::new(values, functions)
                                .with_meta(meta_map),
                        )
                    } else {
                        Value::Undef
                    };
                    values.insert(scoped_id.clone(), val.clone());
                    new_snapshot
                        .values
                        .insert(scoped_id, (val, DeterminacyState::Determined));
                }
            }

            // Update per-member synthetic lists: __list_{name}__{member}
            for (member, _, _, _) in &col_sub.child_value_cells {
                let member_items: Vec<Value> = (0..new_count)
                    .map(|idx| {
                        let scoped_id = ValueCellId::new(
                            format!("{}.{}[{}]", col_sub.parent_entity, col_sub.sub_name, idx),
                            member,
                        );
                        values.get(&scoped_id).cloned().unwrap_or(Value::Undef)
                    })
                    .collect();
                let member_list_id = ValueCellId::new(
                    &col_sub.parent_entity,
                    format!("__list_{}__{}", col_sub.sub_name, member),
                );
                let member_list_val = Value::List(member_items);
                values.insert(member_list_id.clone(), member_list_val.clone());
                new_snapshot.values.insert(
                    member_list_id,
                    (member_list_val, DeterminacyState::Determined),
                );
            }

            // Recompute topology fingerprint to reflect count change
            let count_state_hash = ContentHash::of_str(&format!(
                "collection:{}={}",
                col_sub.count_cell, new_count
            ));
            new_snapshot.topology_fingerprint = new_snapshot
                .graph
                .topology_fingerprint()
                .combine(count_state_hash);

            // Re-elaboration succeeded with a definite count — clear the
            // preserved count entry so it doesn't interfere with future edits.
            preserved_counts.remove(&col_sub.count_cell);
        }
    }

    /// Replace occurrences of the raw ConstraintNodeId string in diagnostic
    /// messages with a human-readable label, when a label is present.
    ///
    /// This enriches engine-level diagnostics for constraint def instantiations
    /// so that messages read "constraint MinWall[0] violated" instead of
    /// "constraint S#constraint[0] violated". When `label` is `None` (inline
    /// constraints without a label), the messages are returned unchanged.
    fn labeled_diagnostics(
        messages: Vec<Diagnostic>,
        id: &reify_types::ConstraintNodeId,
        label: Option<&str>,
    ) -> Vec<Diagnostic> {
        let Some(lbl) = label else {
            return messages;
        };
        let id_str = id.to_string();
        messages
            .into_iter()
            .map(|mut d| {
                d.message = d.message.replace(&id_str, lbl);
                d
            })
            .collect()
    }

    /// Incrementally re-evaluate and check constraints after changing a parameter.
    ///
    /// Combines edit_param() (incremental value evaluation + re-resolution)
    /// with constraint satisfaction checking against the updated values.
    /// Check all constraints against the given values.
    ///
    /// Returns constraint check entries and any diagnostics produced by
    /// violated constraints. Uses the current snapshot's constraint graph.
    ///
    /// This is the shared constraint-checking logic used by both `edit_check`
    /// (sequential path) and `edit_check_concurrent` (concurrent path).
    pub fn check_constraints_with_values(
        &self,
        values: &ValueMap,
    ) -> Result<(Vec<ConstraintCheckEntry>, Vec<Diagnostic>), EngineError> {
        let mut constraint_results = Vec::new();
        let mut diagnostics = Vec::new();

        let state = self
            .eval_state
            .as_ref()
            .ok_or(EngineError::NotInitialized)?;

        let active_ids = state.snapshot.graph.active_constraint_ids(values);
        let constraint_nodes: Vec<_> = state
            .snapshot
            .graph
            .constraints
            .iter()
            .map(|(_, cnode)| cnode)
            .filter(|cnode| active_ids.contains(&cnode.id))
            .collect();

        if !constraint_nodes.is_empty() {
            let constraint_pairs: Vec<_> = constraint_nodes
                .iter()
                .map(|cnode| (cnode.id.clone(), &cnode.expr))
                .collect();

            let input = ConstraintInput {
                constraints: constraint_pairs,
                values,
                functions: &self.functions,
                determinacy: Some(&state.snapshot.values),
            };

            let results = self.constraint_checker.check(&input);
            for (result, cnode) in results.into_iter().zip(constraint_nodes.iter()) {
                diagnostics.extend(Self::labeled_diagnostics(
                    result.diagnostics.messages,
                    &result.id,
                    cnode.label.as_deref(),
                ));
                constraint_results.push(ConstraintCheckEntry {
                    id: result.id,
                    label: cnode.label.clone(),
                    satisfaction: result.satisfaction,
                });
            }
        }

        Ok((constraint_results, diagnostics))
    }

    /// Evaluates ALL constraints (not just dirty ones) to produce a complete
    /// CheckResult, mirroring check()'s pattern but incrementally.
    ///
    /// Requires a prior call to eval() or check() to establish the baseline.
    pub fn edit_check(
        &mut self,
        cell: ValueCellId,
        new_value: reify_types::Value,
    ) -> Result<CheckResult, EngineError> {
        let eval_result = self.edit_param(cell, new_value)?;
        let (constraint_results, constraint_diagnostics) =
            self.check_constraints_with_values(&eval_result.values)?;

        let mut diagnostics = eval_result.diagnostics;
        diagnostics.extend(constraint_diagnostics);

        Ok(CheckResult {
            values: eval_result.values,
            constraint_results,
            diagnostics,
            resolved_params: eval_result.resolved_params,
        })
    }

    /// Evaluate a compiled module with caching and early cutoff.
    ///
    /// On first call (cold start), behaves like eval() but populates the cache.
    /// On subsequent calls with the same version, uses version fast path.
    /// On calls with a new version after invalidation, re-evaluates dirty nodes
    /// and uses early cutoff to avoid propagating unchanged results.
    pub fn eval_cached(&mut self, module: &CompiledModule, version: VersionId) -> CachedEvalResult {
        let mut values = ValueMap::new();
        let diagnostics = Vec::new();
        let mut stats = CacheStats::default();

        // Build meta_map from module templates (same logic as eval()).
        // This ensures MetaAccess expressions resolve correctly even when
        // eval_cached is called without a prior eval().
        self.meta_map = module
            .templates
            .iter()
            .filter(|t| !t.meta.is_empty())
            .map(|t| (t.name.clone(), t.meta.clone()))
            .collect();

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
                        self.cache
                            .record_evaluation(node_id.clone(), result, version, trace);
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
                    let (val, det) = if let Some(override_val) = self.param_overrides.get(&cell.id)
                    {
                        (override_val.clone(), DeterminacyState::Determined)
                    } else {
                        (reify_types::Value::Undef, DeterminacyState::Auto)
                    };

                    let trace = DependencyTrace::default();
                    let cached_result = CachedResult::Value(val.clone(), det);
                    let outcome = self.cache.record_evaluation(
                        node_id.clone(),
                        cached_result,
                        version,
                        trace,
                    );

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
                        self.cache
                            .record_evaluation(node_id.clone(), result, version, trace);
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
                        reify_expr::eval_expr(
                            expr,
                            &reify_expr::EvalContext::new(&values, &module.functions)
                                .with_meta(&self.meta_map),
                        )
                    } else {
                        reify_types::Value::Undef
                    };

                    // Build dependency trace (params have no reads - they are roots)
                    let trace = DependencyTrace::default();

                    let cached_result =
                        CachedResult::Value(val.clone(), DeterminacyState::Determined);
                    let outcome = self.cache.record_evaluation(
                        node_id.clone(),
                        cached_result,
                        version,
                        trace,
                    );

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
                        self.cache
                            .record_evaluation(node_id.clone(), result, version, trace);
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

                    let val = reify_expr::eval_expr(
                        expr,
                        &reify_expr::EvalContext::new(&values, &module.functions)
                            .with_meta(&self.meta_map),
                    );

                    // Build dependency trace from expression refs
                    let trace = extract_dependency_trace(expr);

                    let cached_result =
                        CachedResult::Value(val.clone(), DeterminacyState::Determined);
                    let outcome = self.cache.record_evaluation(
                        node_id.clone(),
                        cached_result,
                        version,
                        trace,
                    );

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
            eval_result: EvalResult {
                values,
                diagnostics,
                resolved_params: HashMap::new(),
            },
            stats,
        }
    }

    /// Check constraints using the current snapshot values, without re-calling eval().
    ///
    /// Returns `None` if no snapshot exists (i.e. eval() hasn't been called yet).
    /// Otherwise builds a ValueMap from the snapshot, runs constraint checking,
    /// and returns constraint results. This is the incremental companion to check():
    /// after edit_param() updates values, call check_snapshot() to see constraint
    /// status without destroying the incremental state.
    pub fn check_snapshot(&self, module: &CompiledModule) -> Option<CheckResult> {
        let state = self.eval_state.as_ref()?;

        // Build ValueMap from snapshot values
        let mut values = ValueMap::new();
        for (id, (val, _det)) in state.snapshot.values.iter() {
            values.insert(id.clone(), val.clone());
        }

        let mut constraint_results = Vec::new();
        let mut diagnostics = Vec::new();

        for template in &module.templates {
            let active_constraints = Self::collect_active_constraints(template, &values);

            if active_constraints.is_empty() {
                continue;
            }

            let constraint_pairs: Vec<_> = active_constraints
                .iter()
                .map(|c| (c.id.clone(), &c.expr))
                .collect();

            let input = ConstraintInput {
                constraints: constraint_pairs,
                values: &values,
                functions: &module.functions,
                determinacy: Some(&state.snapshot.values),
            };

            let results = self.constraint_checker.check(&input);

            for (result, compiled) in results.into_iter().zip(active_constraints.iter()) {
                diagnostics.extend(Self::labeled_diagnostics(
                    result.diagnostics.messages,
                    &result.id,
                    compiled.label.as_deref(),
                ));
                constraint_results.push(ConstraintCheckEntry {
                    id: result.id,
                    label: compiled.label.clone(),
                    satisfaction: result.satisfaction,
                });
            }
        }

        Some(CheckResult {
            values,
            constraint_results,
            diagnostics,
            resolved_params: HashMap::new(),
        })
    }

    /// Collect active constraints from a template given current values.
    ///
    /// Returns top-level constraints unconditionally, plus guarded constraints
    /// whose guard is currently active (true→group.constraints,
    /// false→group.else_constraints, Undef→neither branch).
    fn collect_active_constraints<'a>(
        template: &'a TopologyTemplate,
        values: &ValueMap,
    ) -> Vec<&'a CompiledConstraint> {
        let mut active: Vec<&'a CompiledConstraint> = Vec::new();

        // Top-level (unguarded) constraints are always active
        for c in &template.constraints {
            active.push(c);
        }

        // Guard-gated constraints
        for group in &template.guarded_groups {
            let guard_val = values.get(&group.guard_value_cell);
            match guard_val {
                Some(Value::Bool(true)) => {
                    for c in &group.constraints {
                        active.push(c);
                    }
                }
                Some(Value::Bool(false)) => {
                    for c in &group.else_constraints {
                        active.push(c);
                    }
                }
                _ => {
                    // Undef or non-Bool: neither branch active
                }
            }
        }

        active
    }

    /// Evaluate and check constraints (guard-aware).
    ///
    /// Checks top-level (unguarded) constraints unconditionally, plus
    /// guarded constraints whose guard is active (true→group.constraints,
    /// false→group.else_constraints, Undef→neither).
    pub fn check(&mut self, module: &CompiledModule) -> CheckResult {
        let eval_result = self.eval(module);
        let mut constraint_results = Vec::new();
        let mut diagnostics = eval_result.diagnostics;

        for template in &module.templates {
            // Collect active constraints: top-level + guard-aware guarded
            let active_constraints =
                Self::collect_active_constraints(template, &eval_result.values);

            if active_constraints.is_empty() {
                continue;
            }

            let constraint_pairs: Vec<_> = active_constraints
                .iter()
                .map(|c| (c.id.clone(), &c.expr))
                .collect();

            // After eval(), eval_state is always Some — unwrap is safe here.
            let det_values = &self.eval_state.as_ref().unwrap().snapshot.values;
            let input = ConstraintInput {
                constraints: constraint_pairs,
                values: &eval_result.values,
                functions: &module.functions,
                determinacy: Some(det_values),
            };

            let results = self.constraint_checker.check(&input);

            for (result, compiled) in results.into_iter().zip(active_constraints.iter()) {
                diagnostics.extend(Self::labeled_diagnostics(
                    result.diagnostics.messages,
                    &result.id,
                    compiled.label.as_deref(),
                ));
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

    /// Build geometry from the current snapshot values, without re-calling eval().
    ///
    /// Returns `None` if no snapshot exists. Otherwise: checks constraints from
    /// snapshot (same as check_snapshot), then executes geometry operations from
    /// module realizations using the geometry kernel. This is the incremental
    /// companion to build(): after edit_param() updates values, call
    /// build_snapshot() to get updated geometry without a cold restart.
    pub fn build_snapshot(
        &mut self,
        module: &CompiledModule,
        format: ExportFormat,
    ) -> Option<BuildResult> {
        let state = self.eval_state.as_ref()?;

        // Build ValueMap from snapshot values
        let mut values = ValueMap::new();
        for (id, (val, _det)) in state.snapshot.values.iter() {
            values.insert(id.clone(), val.clone());
        }

        // Check constraints (guard-aware)
        let mut constraint_results = Vec::new();
        let mut diagnostics = Vec::new();

        for template in &module.templates {
            let active_constraints = Self::collect_active_constraints(template, &values);

            if !active_constraints.is_empty() {
                let constraint_pairs: Vec<_> = active_constraints
                    .iter()
                    .map(|c| (c.id.clone(), &c.expr))
                    .collect();

                let input = ConstraintInput {
                    constraints: constraint_pairs,
                    values: &values,
                    functions: &module.functions,
                    determinacy: Some(&state.snapshot.values),
                };

                let results = self.constraint_checker.check(&input);

                for (result, compiled) in results.into_iter().zip(active_constraints.iter()) {
                    diagnostics.extend(Self::labeled_diagnostics(
                        result.diagnostics.messages,
                        &result.id,
                        compiled.label.as_deref(),
                    ));
                    constraint_results.push(ConstraintCheckEntry {
                        id: result.id,
                        label: compiled.label.clone(),
                        satisfaction: result.satisfaction,
                    });
                }
            }
        }

        // Execute geometry operations
        let geometry_output = if let Some(ref mut kernel) = self.geometry_kernel {
            let mut step_handles: Vec<GeometryHandleId> = Vec::new();
            let mut total_ops: usize = 0;

            for template in &module.templates {
                for realization in &template.realizations {
                    let handle_start = step_handles.len();
                    for op in &realization.operations {
                        total_ops += 1;
                        let geom_op = compile_geometry_op(
                            op,
                            &values,
                            &step_handles[handle_start..],
                            &module.functions,
                            &self.meta_map,
                        );
                        match geom_op {
                            Some(geom_op) => match kernel.execute(&geom_op) {
                                Ok(handle) => {
                                    step_handles.push(handle.id);
                                }
                                Err(e) => {
                                    diagnostics
                                        .push(Diagnostic::error(format!("geometry error: {}", e)));
                                    break;
                                }
                            },
                            None => {
                                diagnostics.push(Diagnostic::error(
                                    "failed to compile geometry operation",
                                ));
                                break;
                            }
                        }
                    }
                    // Discard intermediate handles from partially-failed realizations
                    if step_handles.len() - handle_start < realization.operations.len() {
                        step_handles.truncate(handle_start);
                    }
                }
            }

            if total_ops == 0 {
                None
            } else if step_handles.is_empty() {
                diagnostics.push(Diagnostic::error(
                    "all geometry operations failed; no geometry output produced",
                ));
                None
            } else {
                let export_handle = *step_handles.last().unwrap();
                let mut output = Vec::new();
                match kernel.export(export_handle, format, &mut output) {
                    Ok(()) => Some(output),
                    Err(e) => {
                        diagnostics.push(Diagnostic::error(format!("export error: {}", e)));
                        None
                    }
                }
            }
        } else {
            None
        };

        Some(BuildResult {
            values,
            constraint_results,
            geometry_output,
            diagnostics,
            resolved_params: HashMap::new(),
        })
    }

    /// Full build: evaluate, check constraints, produce geometry.
    pub fn build(&mut self, module: &CompiledModule, format: ExportFormat) -> BuildResult {
        let check_result = self.check(module);
        let mut diagnostics = check_result.diagnostics;

        let geometry_output = if let Some(ref mut kernel) = self.geometry_kernel {
            // Execute geometry operations from realizations
            let mut step_handles: Vec<GeometryHandleId> = Vec::new();
            let mut total_ops: usize = 0;

            for template in &module.templates {
                for realization in &template.realizations {
                    let handle_start = step_handles.len();
                    for op in &realization.operations {
                        total_ops += 1;
                        let geom_op = compile_geometry_op(
                            op,
                            &check_result.values,
                            &step_handles[handle_start..],
                            &module.functions,
                            &self.meta_map,
                        );
                        match geom_op {
                            Some(geom_op) => match kernel.execute(&geom_op) {
                                Ok(handle) => {
                                    step_handles.push(handle.id);
                                }
                                Err(e) => {
                                    diagnostics
                                        .push(Diagnostic::error(format!("geometry error: {}", e)));
                                    break;
                                }
                            },
                            None => {
                                diagnostics.push(Diagnostic::error(
                                    "failed to compile geometry operation",
                                ));
                                break;
                            }
                        }
                    }
                    // Discard intermediate handles from partially-failed realizations
                    if step_handles.len() - handle_start < realization.operations.len() {
                        step_handles.truncate(handle_start);
                    }
                }
            }

            if step_handles.is_empty() && total_ops > 0 {
                // All geometry operations failed — skip export entirely
                diagnostics.push(Diagnostic::error(
                    "all geometry operations failed; no geometry output produced",
                ));
                None
            } else {
                // Export the result
                let export_handle = step_handles.last().copied().unwrap_or(GeometryHandleId(0));
                let mut output = Vec::new();
                match kernel.export(export_handle, format, &mut output) {
                    Ok(()) => Some(output),
                    Err(e) => {
                        diagnostics.push(Diagnostic::error(format!("export error: {}", e)));
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

    /// Tessellate all realizations in the module for GUI mesh rendering.
    ///
    /// Evaluates the module via [`check()`], then executes geometry operations
    /// per realization (same loop as [`build()`]) and tessellates each
    /// realization's final shape. Returns one `(entity_path, Mesh)` pair per
    /// realization that produced geometry.
    ///
    /// When no geometry kernel is configured, returns empty meshes with no
    /// error diagnostics (matching the pattern in [`build()`]).
    pub fn tessellate_realizations(&mut self, module: &CompiledModule) -> TessellateResult {
        let check_result = self.check(module);
        let mut diagnostics = check_result.diagnostics;
        let meshes = Self::tessellate_from_values(
            &mut self.geometry_kernel,
            module,
            &check_result.values,
            &mut diagnostics,
            &self.meta_map,
        );

        TessellateResult {
            values: check_result.values,
            constraint_results: check_result.constraint_results,
            meshes,
            diagnostics,
            resolved_params: check_result.resolved_params,
        }
    }

    /// Default tessellation tolerance in SI meters (0.1mm).
    const DEFAULT_TESSELLATION_TOLERANCE: f64 = 0.0001;

    /// Shared helper: execute geometry operations and tessellate each realization.
    ///
    /// Used by both `tessellate_realizations()` and `tessellate_snapshot()`.
    fn tessellate_from_values(
        geometry_kernel: &mut Option<Box<dyn GeometryKernel>>,
        module: &CompiledModule,
        values: &ValueMap,
        diagnostics: &mut Vec<Diagnostic>,
        meta_map: &HashMap<String, HashMap<String, String>>,
    ) -> Vec<(String, Mesh)> {
        let mut meshes = Vec::new();

        let kernel = match geometry_kernel.as_mut() {
            Some(k) => k,
            None => return meshes,
        };

        let mut step_handles: Vec<GeometryHandleId> = Vec::new();

        for template in &module.templates {
            for realization in &template.realizations {
                let handle_start = step_handles.len();

                for op in &realization.operations {
                    let geom_op = compile_geometry_op(
                        op,
                        values,
                        &step_handles[handle_start..],
                        &module.functions,
                        meta_map,
                    );
                    match geom_op {
                        Some(geom_op) => match kernel.execute(&geom_op) {
                            Ok(handle) => {
                                step_handles.push(handle.id);
                            }
                            Err(e) => {
                                diagnostics
                                    .push(Diagnostic::error(format!("geometry error: {}", e)));
                                break;
                            }
                        },
                        None => {
                            diagnostics
                                .push(Diagnostic::error("failed to compile geometry operation"));
                            break;
                        }
                    }
                }

                // Discard intermediate handles from partially-failed realizations
                if step_handles.len() - handle_start < realization.operations.len() {
                    step_handles.truncate(handle_start);
                }

                // Tessellate this realization's final handle (if any new handles were produced)
                if step_handles.len() > handle_start {
                    let last_handle = step_handles[step_handles.len() - 1];
                    match kernel.tessellate(last_handle, Self::DEFAULT_TESSELLATION_TOLERANCE) {
                        Ok(mesh) => {
                            meshes.push((realization.id.to_string(), mesh));
                        }
                        Err(e) => {
                            diagnostics
                                .push(Diagnostic::error(format!("tessellation error: {}", e)));
                        }
                    }
                }
            }
        }

        meshes
    }

    /// Tessellate realizations from the current snapshot values, without
    /// re-calling eval().
    ///
    /// Returns `None` if no snapshot exists (no prior `eval()` call).
    /// Otherwise: checks constraints from snapshot, then executes geometry
    /// operations and tessellates each realization. This is the incremental
    /// companion to `tessellate_realizations()`: after `edit_param()` updates
    /// values, call `tessellate_snapshot()` to get updated meshes without a
    /// cold restart.
    pub fn tessellate_snapshot(&mut self, module: &CompiledModule) -> Option<TessellateResult> {
        let state = self.eval_state.as_ref()?;

        // Build ValueMap from snapshot values
        let mut values = ValueMap::new();
        for (id, (val, _det)) in state.snapshot.values.iter() {
            values.insert(id.clone(), val.clone());
        }

        // Check constraints (guard-aware)
        let mut constraint_results = Vec::new();
        let mut diagnostics = Vec::new();

        for template in &module.templates {
            let active_constraints = Self::collect_active_constraints(template, &values);

            if !active_constraints.is_empty() {
                let constraint_pairs: Vec<_> = active_constraints
                    .iter()
                    .map(|c| (c.id.clone(), &c.expr))
                    .collect();

                let input = ConstraintInput {
                    constraints: constraint_pairs,
                    values: &values,
                    functions: &module.functions,
                    determinacy: Some(&state.snapshot.values),
                };

                let results = self.constraint_checker.check(&input);

                for (result, compiled) in results.into_iter().zip(active_constraints.iter()) {
                    diagnostics.extend(Self::labeled_diagnostics(
                        result.diagnostics.messages,
                        &result.id,
                        compiled.label.as_deref(),
                    ));
                    constraint_results.push(ConstraintCheckEntry {
                        id: result.id,
                        label: compiled.label.clone(),
                        satisfaction: result.satisfaction,
                    });
                }
            }
        }

        // Execute geometry and tessellate
        let meshes = Self::tessellate_from_values(
            &mut self.geometry_kernel,
            module,
            &values,
            &mut diagnostics,
            &self.meta_map,
        );

        Some(TessellateResult {
            values,
            constraint_results,
            meshes,
            diagnostics,
            resolved_params: HashMap::new(),
        })
    }

    /// Evaluate let bindings from a template in topological order.
    ///
    /// Collects let cells with expressions, builds dependency traces,
    /// topologically sorts, and evaluates each in order — recording
    /// journal events and cache entries. Used by both the initial eval()
    /// pass and the post-resolution re-evaluation pass.
    #[allow(clippy::too_many_arguments)]
    fn evaluate_let_bindings(
        &mut self,
        template: &reify_compiler::TopologyTemplate,
        values: &mut ValueMap,
        snapshot: &mut Snapshot,
        version_id: u64,
        functions: &[CompiledFunction],
        meta_map: &HashMap<String, HashMap<String, String>>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let let_cells: HashMap<NodeId, &reify_types::CompiledExpr> = template
            .value_cells
            .iter()
            .filter(|c| c.kind == ValueCellKind::Let && c.default_expr.is_some())
            .map(|c| {
                (
                    NodeId::Value(c.id.clone()),
                    c.default_expr.as_ref().unwrap(),
                )
            })
            .collect();

        let let_node_ids: HashSet<NodeId> = let_cells.keys().cloned().collect();
        let let_traces: HashMap<NodeId, DependencyTrace> = let_cells
            .iter()
            .map(|(nid, expr)| (nid.clone(), extract_dependency_trace(expr)))
            .collect();

        let sorted_lets = topological_sort(&let_node_ids, &let_traces);

        // Detect cyclic let-binding dependencies: if topological_sort dropped nodes
        // (Kahn's algorithm silently omits nodes in cycles), report them.
        if sorted_lets.len() < let_node_ids.len() {
            let sorted_set: HashSet<&NodeId> = sorted_lets.iter().collect();
            let mut cyclic_members: Vec<&str> = let_node_ids
                .iter()
                .filter(|nid| !sorted_set.contains(nid))
                .filter_map(|nid| match nid {
                    NodeId::Value(vcid) => Some(vcid.member.as_str()),
                    _ => None,
                })
                .collect();
            cyclic_members.sort();
            diagnostics.push(Diagnostic::error(format!(
                "circular let-binding dependency in template {}: [{}]",
                template.name,
                cyclic_members.join(", "),
            )));
        }

        for node_id in sorted_lets {
            let expr = let_cells[&node_id];
            let cell_id = match &node_id {
                NodeId::Value(vcid) => vcid,
                _ => unreachable!(),
            };

            let start = Instant::now();
            self.journal.record(EvalEvent {
                timestamp: start,
                node_id: node_id.clone(),
                kind: EventKind::Started,
                version: VersionId(version_id),
                payload: None,
            });

            let val = reify_expr::eval_expr(
                expr,
                &reify_expr::EvalContext::new(values, functions)
                    .with_meta(meta_map)
                    .with_determinacy(&snapshot.values),
            );
            values.insert(cell_id.clone(), val.clone());

            snapshot
                .values
                .insert(cell_id.clone(), (val.clone(), DeterminacyState::Determined));

            let trace = extract_dependency_trace(expr);
            let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
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

/// Compile a CompiledGeometryOp into a GeometryOp by evaluating expressions.
/// Translate a compiled geometry operation into a runtime `GeometryOp`.
///
/// Returns `None` when a required argument is missing, non-finite, or invalid
/// (e.g. negative scale factor), which signals the caller to skip this op and
/// emit a diagnostic.
///
/// # Ordering invariant for `functions`
///
/// `functions` is the slice of [`CompiledFunction`]s from the module.  The
/// evaluator passes the *full* module-level slice so that any expression
/// inside an op's args can reference user-defined functions by index.
/// Forward references within the same structure are resolved during
/// compilation (name → index), so the slice must preserve declaration order
/// to keep indices valid.  Callers that construct a partial functions slice
/// (e.g. for testing) must ensure indices in compiled expressions stay
/// in-bounds or the lookup will silently return `Value::Undef`.
fn compile_geometry_op(
    op: &reify_compiler::CompiledGeometryOp,
    values: &ValueMap,
    step_handles: &[GeometryHandleId],
    functions: &[CompiledFunction],
    meta_map: &HashMap<String, HashMap<String, String>>,
) -> Option<reify_types::GeometryOp> {
    use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};

    match op {
        CompiledGeometryOp::Primitive { kind, args } => {
            let eval_arg = |name: &str| -> reify_types::Value {
                args.iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, expr)| {
                        reify_expr::eval_expr(
                            expr,
                            &reify_expr::EvalContext::new(values, functions).with_meta(meta_map),
                        )
                    })
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
                    GeomRef::Step(idx) => step_handles.get(*idx).copied(),
                    GeomRef::Sub(_name) => step_handles.last().copied(),
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
                GeomRef::Step(idx) => step_handles.get(*idx).copied()?,
                GeomRef::Sub(_) => step_handles.last().copied()?,
            };
            let eval_arg = |name: &str| -> reify_types::Value {
                args.iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, expr)| {
                        reify_expr::eval_expr(
                            expr,
                            &reify_expr::EvalContext::new(values, functions).with_meta(meta_map),
                        )
                    })
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
                reify_compiler::ModifyKind::Shell => {
                    let thickness = eval_arg("thickness");
                    // Collect face indices from face_0, face_1, ...
                    let faces_to_remove: Vec<usize> = args
                        .iter()
                        .filter(|(n, _)| n.starts_with("face_"))
                        .filter_map(|(_, expr)| {
                            reify_expr::eval_expr(
                                expr,
                                &reify_expr::EvalContext::new(values, functions)
                                    .with_meta(meta_map),
                            )
                            .as_f64()
                            .map(|v| v as usize)
                        })
                        .collect();
                    Some(reify_types::GeometryOp::Shell {
                        target: target_id,
                        thickness,
                        faces_to_remove,
                    })
                }
                reify_compiler::ModifyKind::Draft => {
                    let angle = eval_arg("angle");
                    // plane is passed as an expression that evaluates to a value;
                    // at this level we don't have the geometry handle yet, so we
                    // use step_handles.last() as a placeholder for the plane reference.
                    let plane_id = step_handles.last().copied();
                    Some(reify_types::GeometryOp::Draft {
                        target: target_id,
                        angle,
                        plane: plane_id?,
                    })
                }
                reify_compiler::ModifyKind::Thicken => {
                    let offset = eval_arg("offset");
                    Some(reify_types::GeometryOp::Thicken {
                        target: target_id,
                        offset,
                    })
                }
            }
        }
        CompiledGeometryOp::Transform { kind, target, args } => {
            let target_id = match target {
                GeomRef::Step(idx) => step_handles.get(*idx).copied()?,
                GeomRef::Sub(_) => step_handles.last().copied()?,
            };
            let eval_arg_f64 = |name: &str| -> Option<f64> {
                let (_, expr) = args.iter().find(|(n, _)| n == name)?;
                reify_expr::eval_expr(
                    expr,
                    &reify_expr::EvalContext::new(values, functions).with_meta(meta_map),
                )
                .as_f64()
                .filter(|v| v.is_finite())
            };
            match kind {
                reify_compiler::TransformKind::Translate => {
                    Some(reify_types::GeometryOp::Translate {
                        target: target_id,
                        dx: eval_arg_f64("dx")?,
                        dy: eval_arg_f64("dy")?,
                        dz: eval_arg_f64("dz")?,
                    })
                }
                reify_compiler::TransformKind::Rotate => Some(reify_types::GeometryOp::Rotate {
                    target: target_id,
                    axis: [
                        eval_arg_f64("axis_x")?,
                        eval_arg_f64("axis_y")?,
                        eval_arg_f64("axis_z")?,
                    ],
                    angle_rad: eval_arg_f64("angle")?,
                }),
                reify_compiler::TransformKind::Scale => {
                    let factor = eval_arg_f64("factor")?;
                    // Reject negative scale: OCCT SetScale with negative factor
                    // produces inside-out geometry (point-symmetry), not mirroring.
                    if factor < 0.0 {
                        return None;
                    }
                    Some(reify_types::GeometryOp::Scale {
                        target: target_id,
                        factor,
                    })
                }
                reify_compiler::TransformKind::RotateAround => {
                    Some(reify_types::GeometryOp::RotateAround {
                        target: target_id,
                        point: [
                            eval_arg_f64("px")?,
                            eval_arg_f64("py")?,
                            eval_arg_f64("pz")?,
                        ],
                        axis: [
                            eval_arg_f64("axis_x")?,
                            eval_arg_f64("axis_y")?,
                            eval_arg_f64("axis_z")?,
                        ],
                        angle_rad: eval_arg_f64("angle")?,
                    })
                }
            }
        }
        CompiledGeometryOp::Pattern { kind, target, args } => {
            let eval_arg = |name: &str| -> Option<reify_types::Value> {
                let (_, expr) = args.iter().find(|(n, _)| n == name)?;
                Some(reify_expr::eval_expr(
                    expr,
                    &reify_expr::EvalContext::new(values, functions).with_meta(meta_map),
                ))
            };
            let eval_arg_f64 = |name: &str| -> Option<f64> {
                let (_, expr) = args.iter().find(|(n, _)| n == name)?;
                reify_expr::eval_expr(
                    expr,
                    &reify_expr::EvalContext::new(values, functions).with_meta(meta_map),
                )
                .as_f64()
                .filter(|v| v.is_finite())
            };
            // Pattern operations resolve target via step index
            let target_id = match target {
                GeomRef::Step(idx) => step_handles.get(*idx).copied()?,
                GeomRef::Sub(_) => step_handles.last().copied()?,
            };
            match kind {
                reify_compiler::PatternKind::Linear => {
                    Some(reify_types::GeometryOp::LinearPattern {
                        target: target_id,
                        direction: [
                            eval_arg_f64("dx")?,
                            eval_arg_f64("dy")?,
                            eval_arg_f64("dz")?,
                        ],
                        count: eval_arg_f64("count")? as usize,
                        spacing: eval_arg("spacing")?,
                    })
                }
                reify_compiler::PatternKind::Circular => {
                    Some(reify_types::GeometryOp::CircularPattern {
                        target: target_id,
                        axis_origin: [
                            eval_arg_f64("ox")?,
                            eval_arg_f64("oy")?,
                            eval_arg_f64("oz")?,
                        ],
                        axis_dir: [
                            eval_arg_f64("ax")?,
                            eval_arg_f64("ay")?,
                            eval_arg_f64("az")?,
                        ],
                        count: eval_arg_f64("count")? as usize,
                        angle: eval_arg("angle")?,
                    })
                }
                reify_compiler::PatternKind::Mirror => Some(reify_types::GeometryOp::Mirror {
                    target: target_id,
                    plane_origin: [
                        eval_arg_f64("ox")?,
                        eval_arg_f64("oy")?,
                        eval_arg_f64("oz")?,
                    ],
                    plane_normal: [
                        eval_arg_f64("nx")?,
                        eval_arg_f64("ny")?,
                        eval_arg_f64("nz")?,
                    ],
                }),
            }
        }
        CompiledGeometryOp::Sweep {
            kind,
            profiles,
            args,
        } => {
            match kind {
                reify_compiler::SweepKind::Loft => {
                    // Resolve each profile GeomRef to a handle via step_handles
                    let resolved: Option<Vec<GeometryHandleId>> = profiles
                        .iter()
                        .map(|r| match r {
                            GeomRef::Step(idx) => step_handles.get(*idx).copied(),
                            GeomRef::Sub(_) => step_handles.last().copied(),
                        })
                        .collect();
                    Some(reify_types::GeometryOp::Loft {
                        profiles: resolved?,
                    })
                }
                reify_compiler::SweepKind::Extrude => {
                    let profile_handle = match profiles.first()? {
                        GeomRef::Step(idx) => step_handles.get(*idx).copied()?,
                        GeomRef::Sub(_) => step_handles.last().copied()?,
                    };
                    let distance =
                        args.iter()
                            .find(|(n, _)| n == "distance")
                            .map(|(_, expr)| {
                                reify_expr::eval_expr(
                                    expr,
                                    &reify_expr::EvalContext::new(values, functions)
                                        .with_meta(meta_map),
                                )
                            })?;
                    let _distance_f64 = distance.as_f64().filter(|v| v.is_finite())?;
                    Some(reify_types::GeometryOp::Extrude {
                        profile: profile_handle,
                        distance,
                    })
                }
                reify_compiler::SweepKind::Revolve => {
                    let profile_handle = match profiles.first()? {
                        GeomRef::Step(idx) => step_handles.get(*idx).copied()?,
                        GeomRef::Sub(_) => step_handles.last().copied()?,
                    };
                    let eval_arg_f64 = |name: &str| -> Option<f64> {
                        let (_, expr) = args.iter().find(|(n, _)| n == name)?;
                        reify_expr::eval_expr(
                            expr,
                            &reify_expr::EvalContext::new(values, functions).with_meta(meta_map),
                        )
                        .as_f64()
                        .filter(|v| v.is_finite())
                    };
                    let axis_dir = [
                        eval_arg_f64("ax")?,
                        eval_arg_f64("ay")?,
                        eval_arg_f64("az")?,
                    ];
                    let mag = axis_dir.iter().map(|x| x * x).sum::<f64>().sqrt();
                    if !mag.is_finite() || mag < 1e-12 {
                        return None;
                    }
                    Some(reify_types::GeometryOp::Revolve {
                        profile: profile_handle,
                        axis_origin: [
                            eval_arg_f64("ox")?,
                            eval_arg_f64("oy")?,
                            eval_arg_f64("oz")?,
                        ],
                        axis_dir,
                        angle_rad: eval_arg_f64("angle")?,
                    })
                }
                reify_compiler::SweepKind::Sweep => {
                    // Resolve profile GeomRef (first entry in profiles) to a handle
                    let profile_handle = match profiles.first()? {
                        GeomRef::Step(idx) => step_handles.get(*idx).copied()?,
                        GeomRef::Sub(_) => step_handles.last().copied()?,
                    };
                    // Resolve path GeomRef (second entry in profiles) to a handle
                    let path_handle = match profiles.get(1)? {
                        GeomRef::Step(idx) => step_handles.get(*idx).copied()?,
                        GeomRef::Sub(_) => step_handles.last().copied()?,
                    };
                    Some(reify_types::GeometryOp::Sweep {
                        profile: profile_handle,
                        path: path_handle,
                    })
                }
            }
        }
    }
}

/// Recursively unfold a recursive sub-component until the guard evaluates to false
/// or the depth limit is reached.
///
/// The guard expression in `sub.guard_expr` uses the owning template's entity name (e.g., "A.n").
/// To correctly evaluate the guard at each recursion level, we build a "local" values context
/// by remapping the current parent entity's values to the `scope_template`'s namespace.
///
/// For self-recursion (S→S), scope_template == child_template. For mutual recursion
/// (A→B→A), scope_template is the template that owns `sub` (e.g., A for sub b=B),
/// while child_template is the target (B). This ensures guard/arg expressions match
/// local_values keys.
///
/// # Parameters
/// - `scope_template`: the template that owns `sub` (used for building local_values that
///   match the guard/arg expression references)
/// - `child_template`: the target template instantiated by `sub` (used for Phase 1 elaboration)
/// - `parent_entity`: the entity currently being processed (e.g., "A" at depth 0, "A.b" at depth 1)
/// - `depth`: current recursion depth (0 = processing the top-level template)
/// - `max_depth`: maximum allowed depth before stopping
/// - `templates`: all templates in the module, used to look up target templates for
///   child_template's recursive subs in Phase 2.
/// - `node_budget`: remaining total nodes allowed across all branches. Prevents exponential
///   blowup when B > 1 recursive subs exist (B^D total without this limit).
#[allow(clippy::too_many_arguments)]
fn unfold_recursive_sub<'t>(
    values: &mut ValueMap,
    snapshot: &mut Snapshot,
    functions: &[CompiledFunction],
    journal: &mut EventJournal,
    cache: &mut CacheStore,
    version_id: u64,
    scope_template: &'t reify_compiler::TopologyTemplate,
    child_template: &'t reify_compiler::TopologyTemplate,
    sub: &reify_compiler::SubComponentDecl,
    parent_entity: &str,
    depth: usize,
    max_depth: usize,
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
    templates: &'t [reify_compiler::TopologyTemplate],
    node_budget: &mut usize,
) {
    // Check total node budget before doing any work.
    if *node_budget == 0 {
        diagnostics.push(Diagnostic::error(format!(
            "recursive unfolding of '{}' stopped: total node budget exhausted at depth {}",
            parent_entity, depth,
        )));
        return;
    }

    let guard_expr = match &sub.guard_expr {
        Some(g) => g,
        None => return,
    };

    // Build a small overlay map with only the template-scoped entries needed for
    // guard/arg evaluation. Uses scope_template (the template that owns `sub`) so
    // the guard/arg expression references (e.g., A.n) match local_values keys.
    // For self-recursion scope_template == child_template; for mutual recursion they differ.
    let mut local_values = ValueMap::new();
    for cell in &scope_template.value_cells {
        let scoped_id = ValueCellId::new(parent_entity, &cell.id.member);
        if let Some(v) = values.get(&scoped_id) {
            local_values.insert(cell.id.clone(), v.clone());
        }
    }

    // Evaluate the guard in the local context.
    let guard_val = reify_expr::eval_expr(
        guard_expr,
        &reify_expr::EvalContext::new(&local_values, functions).with_meta(meta_map),
    );

    // Differentiate guard outcomes: Bool(true) continues, Bool(false)/Undef terminate
    // normally, any other type is a guard expression bug that deserves a diagnostic.
    match &guard_val {
        Value::Bool(true) => {
            if depth >= max_depth {
                // Use Error (not Warning) so callers know the result is potentially unsound:
                // child references beyond the truncated depth resolve to Undef.
                diagnostics.push(Diagnostic::error(format!(
                    "recursive unfolding of '{}' truncated at depth limit {} (guard still true)",
                    parent_entity, max_depth,
                )));
                return;
            }
        }
        Value::Bool(false) => return, // Normal termination — guard says stop
        Value::Undef => return,       // Param not yet determined — do not unfold (per spec)
        other => {
            diagnostics.push(Diagnostic::error(format!(
                "guard for recursive sub '{}' in '{}' evaluated to {:?} (expected Bool), treating as termination",
                sub.name, parent_entity, other,
            )));
            return;
        }
    }

    // Pre-evaluate args in the local context (so child uses current level's param values, not top-level).
    // Use the arg expression's declared result_type for the literal wrapper.
    let concrete_args: Vec<(String, reify_types::CompiledExpr)> = sub
        .args
        .iter()
        .map(|(name, arg_expr)| {
            let v = reify_expr::eval_expr(
                arg_expr,
                &reify_expr::EvalContext::new(&local_values, functions).with_meta(meta_map),
            );
            let ty = arg_expr.result_type.clone();
            (name.clone(), reify_types::CompiledExpr::literal(v, ty))
        })
        .collect();

    // Construct the next child's scoped entity name: parent_entity.sub_name
    let next_entity = format!("{}.{}", parent_entity, sub.name);

    // Consume one budget unit now that we know this call will actually create a node
    // (guard is true and depth is within limits). Decrementing here rather than at
    // function entry avoids wasting budget on guard-false or depth-limited returns.
    *node_budget -= 1;

    // Phase 1 (top-down): Set params for next_entity so the next recursion level
    // can evaluate its guard using the child's param values.
    let child_values = elaborate_child_params_only(
        values,
        snapshot,
        functions,
        journal,
        cache,
        version_id,
        child_template,
        &next_entity,
        &concrete_args,
        meta_map,
    );

    // Phase 2 (recurse): Unfold ALL of child_template's recursive subs at the next level
    // first (leaves-first ordering). Recomputing from child_template.sub_components (not
    // scope_template's) is critical for mutual recursion: when A→B, the next level must
    // iterate B's subs (not A's), so guard/arg expressions match B's value_cell namespace.
    let child_recursive_subs: Vec<&reify_compiler::SubComponentDecl> = child_template
        .sub_components
        .iter()
        .filter(|s| child_template.is_recursive && s.guard_expr.is_some())
        .collect();
    let child_recursive_sub_names: Vec<&str> = child_recursive_subs
        .iter()
        .map(|s| s.name.as_str())
        .collect();

    for next_sub in &child_recursive_subs {
        // Look up the target template for next_sub from the module's template list.
        // For self-recursion, this finds the same template. For mutual recursion (A→B→A),
        // this alternates: B's sub "a" targets A, A's sub "b" targets B.
        let next_child_template = match templates.iter().find(|t| t.name == next_sub.structure_name)
        {
            Some(t) => t,
            None => {
                diagnostics.push(Diagnostic::error(format!(
                    "recursive sub \"{}\" in \"{}\" at depth {} references unknown structure \"{}\"; skipping branch",
                    next_sub.name, next_entity, depth + 1, next_sub.structure_name
                )));
                continue;
            }
        };
        unfold_recursive_sub(
            values,
            snapshot,
            functions,
            journal,
            cache,
            version_id,
            child_template, // child_template owns next_sub → becomes scope_template
            next_child_template, // target template for next_sub's structure
            next_sub,
            &next_entity,
            depth + 1,
            max_depth,
            meta_map,
            diagnostics,
            templates,
            node_budget,
        );
    }

    // Phase 3 (bottom-up): Evaluate let-bindings for next_entity.
    // child_values is enriched inside elaborate_child_lets_only with sub-component
    // values projected from the global map — so cross-level references like
    // `S.child.total` resolve to the already-computed deeper-level value.
    // Pass child-scoped recursive sub names so BFS walks the correct branches.
    elaborate_child_lets_only(
        values,
        snapshot,
        functions,
        journal,
        cache,
        version_id,
        child_template,
        &next_entity,
        child_values,
        meta_map,
        &child_recursive_sub_names,
        templates,
        diagnostics,
    );
}

/// Elaborate a single child instance into the values/snapshot maps.
///
/// This handles both non-collection subs (single instance) and individual
/// collection sub instances (called in a loop for each index).
///
/// For non-recursive subs both phases run atomically (params then lets).
/// For recursive subs, use `elaborate_child_params_only` + `elaborate_child_lets_only`
/// to allow leaves-first ordering (recurse between the two phases).
#[allow(clippy::too_many_arguments)]
fn elaborate_child_instance(
    values: &mut ValueMap,
    snapshot: &mut Snapshot,
    functions: &[CompiledFunction],
    journal: &mut EventJournal,
    cache: &mut CacheStore,
    version_id: u64,
    child_template: &TopologyTemplate,
    scoped_entity: &str,
    args: &[(String, reify_types::CompiledExpr)],
    meta_map: &HashMap<String, HashMap<String, String>>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let child_values = elaborate_child_params_only(
        values,
        snapshot,
        functions,
        journal,
        cache,
        version_id,
        child_template,
        scoped_entity,
        args,
        meta_map,
    );
    elaborate_child_lets_only(
        values,
        snapshot,
        functions,
        journal,
        cache,
        version_id,
        child_template,
        scoped_entity,
        child_values,
        meta_map,
        &[],
        &[],
        diagnostics,
    );
}

/// Phase 1: Evaluate and store only the param cells for a child instance.
///
/// Returns the template-scoped child_values map (params only) for use in phase 2.
/// All param values are also written to the global `values`, `snapshot`, journal, and cache.
#[allow(clippy::too_many_arguments)]
fn elaborate_child_params_only(
    values: &mut ValueMap,
    snapshot: &mut Snapshot,
    functions: &[CompiledFunction],
    journal: &mut EventJournal,
    cache: &mut CacheStore,
    version_id: u64,
    child_template: &TopologyTemplate,
    scoped_entity: &str,
    args: &[(String, reify_types::CompiledExpr)],
    meta_map: &HashMap<String, HashMap<String, String>>,
) -> ValueMap {
    let mut child_values = ValueMap::new();

    for cell in &child_template.value_cells {
        if cell.kind != ValueCellKind::Param {
            continue;
        }

        let member = &cell.id.member;

        let val = if let Some((_name, arg_expr)) = args.iter().find(|(name, _)| name == member) {
            reify_expr::eval_expr(
                arg_expr,
                &reify_expr::EvalContext::new(values, functions).with_meta(meta_map),
            )
        } else if let Some(ref default_expr) = cell.default_expr {
            reify_expr::eval_expr(
                default_expr,
                &reify_expr::EvalContext::new(&child_values, functions).with_meta(meta_map),
            )
        } else {
            Value::Undef
        };

        child_values.insert(cell.id.clone(), val.clone());

        let scoped_id = ValueCellId::new(scoped_entity, member);
        let node_id = NodeId::Value(scoped_id.clone());
        let start = Instant::now();
        journal.record(EvalEvent {
            timestamp: start,
            node_id: node_id.clone(),
            kind: EventKind::Started,
            version: VersionId(version_id),
            payload: None,
        });

        values.insert(scoped_id.clone(), val.clone());
        snapshot.values.insert(
            scoped_id.clone(),
            (val.clone(), DeterminacyState::Determined),
        );

        let trace = DependencyTrace::default();
        let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
        let outcome =
            cache.record_evaluation(node_id.clone(), cached_result, VersionId(version_id), trace);

        journal.record(EvalEvent {
            timestamp: Instant::now(),
            node_id,
            kind: EventKind::Completed { outcome },
            version: VersionId(version_id),
            payload: Some(EventPayload::Duration(start.elapsed())),
        });
    }

    child_values
}

/// Phase 2: Evaluate and store the let-binding cells for a child instance.
///
/// `child_values` should contain the template-scoped params from phase 1.
/// Before evaluating lets, this function enriches `child_values` with sub-component
/// values projected from the global `values` map — this enables cross-level let
/// expressions like `let total = if n > 0 then n + S.child.total else n` to see
/// values computed by deeper recursion levels (leaves-first ordering).
///
/// Projection rule: for each global entry whose entity starts with
/// `"{scoped_entity}."`, strip that prefix and add `"{template_name}."` to produce
/// a template-scoped key. E.g., when evaluating lets for `S.child` (template `S`):
///   global["S.child.child", "total"] → child_values["S.child", "total"]
///
/// For templates with multiple recursive subs, `recursive_sub_names` contains all
/// sub names. A BFS walks the full entity tree under `scoped_entity` (following all
/// sub name branches at each level), so cross-sub values are projected correctly.
/// E.g., for subs [left, right] at `S.left`: both `S.left.left.*` and `S.left.right.*`
/// are projected, enabling lets like `let sum = S.left.val + S.right.val`.
///
/// For heterogeneous mutual recursion (A→B→A where A and B have different members),
/// the BFS queue carries `(entity_path, &TopologyTemplate)` tuples so each depth level
/// iterates the correct template's value_cells. When enqueuing children, the entity's
/// template's sub_components determine child sub names and their target templates.
#[allow(clippy::too_many_arguments)]
fn elaborate_child_lets_only<'t>(
    values: &mut ValueMap,
    snapshot: &mut Snapshot,
    functions: &[CompiledFunction],
    journal: &mut EventJournal,
    cache: &mut CacheStore,
    version_id: u64,
    child_template: &'t TopologyTemplate,
    scoped_entity: &str,
    mut child_values: ValueMap,
    meta_map: &HashMap<String, HashMap<String, String>>,
    recursive_sub_names: &[&str],
    templates: &'t [TopologyTemplate],
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Enrich child_values with sub-component values projected from the global map.
    // Only needed for recursive subs where deeper levels have already been elaborated
    // (leaves-first ordering).
    //
    // Uses BFS over the entity tree rooted at scoped_entity: starts with one immediate
    // child per sub name, then expands branches where values exist. This handles both
    // single-sub chains (O(D×C)) and multi-sub trees (O(B^D×C) where B=branching, D=depth).
    // The BFS terminates naturally when no values are found at a given entity.
    if !recursive_sub_names.is_empty() {
        let scoped_prefix = format!("{}.", scoped_entity);
        let template_prefix = format!("{}.", child_template.name);

        // BFS queue carries (entity_path, entity_template) so each depth level uses
        // the correct template's value_cells for projection (heterogeneous mutual recursion).
        let mut queue: std::collections::VecDeque<(String, &TopologyTemplate)> = recursive_sub_names
            .iter()
            .filter_map(|name| {
                // Look up the sub declaration to find its target template.
                let sub_decl = child_template.sub_components.iter().find(|s| s.name == *name)?;
                let target_tmpl = templates.iter().find(|t| t.name == sub_decl.structure_name).or_else(|| {
                    diagnostics.push(Diagnostic::error(format!(
                        "BFS seed: sub \"{}\" in \"{}\" references unknown structure \"{}\"; skipping",
                        name, scoped_entity, sub_decl.structure_name
                    )));
                    None
                })?;
                Some((format!("{}.{}", scoped_entity, name), target_tmpl))
            })
            .collect();

        while let Some((depth_entity, entity_template)) = queue.pop_front() {
            let mut found_any = false;
            // Use entity_template.value_cells — NOT child_template.value_cells.
            // This ensures B-specific members (e.g., "height") are projected for B entities.
            for cell in &entity_template.value_cells {
                let id = ValueCellId::new(&depth_entity, &cell.id.member);
                if let Some(val) = values.get(&id)
                    && let Some(suffix) = depth_entity.strip_prefix(&scoped_prefix)
                {
                    let remapped_entity = format!("{}{}", template_prefix, suffix);
                    child_values.insert(
                        ValueCellId::new(remapped_entity, &cell.id.member),
                        val.clone(),
                    );
                    found_any = true;
                }
            }
            // For structural intermediaries (zero value_cells), found_any is always
            // false. Check whether any key in `values` has this entity as a prefix,
            // proving unfold_recursive_sub actually created descendants. Without this
            // check, two structural intermediaries forming a cycle (W1→W2→W1) would
            // cause the BFS to generate ever-growing entity paths without bound.
            let intermediary_has_descendants = entity_template.value_cells.is_empty()
                && values
                    .iter()
                    .any(|(k, _)| k.entity.starts_with(&format!("{}.", depth_entity)));
            if found_any || intermediary_has_descendants {
                // Enqueue children if:
                // 1. found_any: values were projected from this entity (entity exists), OR
                // 2. intermediary_has_descendants: structural intermediary with zero
                //    value_cells but confirmed descendants in the values map.
                // For templates WITH value_cells, found_any==false means the entity was
                // never unfolded (e.g., guard was false), so BFS terminates naturally.
                // For structural intermediaries, the prefix check serves the same purpose.
                for sub_decl in &entity_template.sub_components {
                    if sub_decl.guard_expr.is_some() {
                        if let Some(target_tmpl) =
                            templates.iter().find(|t| t.name == sub_decl.structure_name)
                        {
                            queue.push_back((
                                format!("{}.{}", depth_entity, sub_decl.name),
                                target_tmpl,
                            ));
                        } else {
                            diagnostics.push(Diagnostic::error(format!(
                                "BFS expand: sub \"{}\" in \"{}\" references unknown structure \"{}\"; skipping subtree",
                                sub_decl.name, depth_entity, sub_decl.structure_name
                            )));
                        }
                    }
                }
            }
        }
    }

    // Evaluate let-bindings in topological order.
    let child_let_cells: HashMap<NodeId, &reify_types::CompiledExpr> = child_template
        .value_cells
        .iter()
        .filter(|c| c.kind == ValueCellKind::Let && c.default_expr.is_some())
        .map(|c| {
            (
                NodeId::Value(c.id.clone()),
                c.default_expr.as_ref().unwrap(),
            )
        })
        .collect();

    let child_let_node_ids: HashSet<NodeId> = child_let_cells.keys().cloned().collect();
    let child_let_traces: HashMap<NodeId, DependencyTrace> = child_let_cells
        .iter()
        .map(|(nid, expr)| (nid.clone(), extract_dependency_trace(expr)))
        .collect();

    let sorted_child_lets = topological_sort(&child_let_node_ids, &child_let_traces);

    // Detect cyclic let-binding dependencies: if topological_sort dropped nodes
    // (Kahn's algorithm silently omits nodes in cycles), report them.
    if sorted_child_lets.len() < child_let_node_ids.len() {
        let sorted_set: HashSet<&NodeId> = sorted_child_lets.iter().collect();
        let mut cyclic_members: Vec<&str> = child_let_node_ids
            .iter()
            .filter(|nid| !sorted_set.contains(nid))
            .filter_map(|nid| match nid {
                NodeId::Value(vcid) => Some(vcid.member.as_str()),
                _ => None,
            })
            .collect();
        cyclic_members.sort();
        diagnostics.push(Diagnostic::error(format!(
            "circular let-binding dependency in template {} (entity {}): [{}]",
            child_template.name,
            scoped_entity,
            cyclic_members.join(", "),
        )));
    }

    for child_node_id in sorted_child_lets {
        let expr = child_let_cells[&child_node_id];
        let child_cell_id = match &child_node_id {
            NodeId::Value(vcid) => vcid,
            _ => unreachable!(),
        };
        let member = &child_cell_id.member;

        let val = reify_expr::eval_expr(
            expr,
            &reify_expr::EvalContext::new(&child_values, functions).with_meta(meta_map),
        );
        child_values.insert(child_cell_id.clone(), val.clone());

        let scoped_id = ValueCellId::new(scoped_entity, member);
        let node_id = NodeId::Value(scoped_id.clone());
        let start = Instant::now();
        journal.record(EvalEvent {
            timestamp: start,
            node_id: node_id.clone(),
            kind: EventKind::Started,
            version: VersionId(version_id),
            payload: None,
        });

        values.insert(scoped_id.clone(), val.clone());
        snapshot.values.insert(
            scoped_id.clone(),
            (val.clone(), DeterminacyState::Determined),
        );

        let trace = extract_dependency_trace(expr);
        let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
        let outcome =
            cache.record_evaluation(node_id.clone(), cached_result, VersionId(version_id), trace);

        journal.record(EvalEvent {
            timestamp: Instant::now(),
            node_id,
            kind: EventKind::Completed { outcome },
            version: VersionId(version_id),
            payload: Some(EventPayload::Duration(start.elapsed())),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_compiler::{CompiledGeometryOp, GeomRef, PatternKind, SweepKind, TransformKind};
    use reify_types::GeometryHandleId;

    /// Helper: build a CompiledExpr literal from a constant f64.
    fn literal_f64(v: f64) -> reify_types::CompiledExpr {
        reify_types::CompiledExpr::literal(reify_types::Value::Real(v), reify_types::Type::Real)
    }

    /// Helper: build a CompiledExpr literal from a Scalar with LENGTH dimension.
    fn literal_length(meters: f64) -> reify_types::CompiledExpr {
        reify_types::CompiledExpr::literal(
            reify_types::Value::Scalar {
                si_value: meters,
                dimension: reify_types::DimensionVector::LENGTH,
            },
            reify_types::Type::length(),
        )
    }

    #[test]
    fn compile_geometry_op_scale_produces_scale_variant() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Scale,
            target: GeomRef::Step(0),
            args: vec![("factor".into(), literal_f64(2.0))],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        let result = result.expect("compile_geometry_op should return Some for Scale");

        match result {
            reify_types::GeometryOp::Scale { target, factor } => {
                assert_eq!(target, GeometryHandleId(42));
                assert!((factor - 2.0).abs() < 1e-12);
            }
            other => panic!("expected GeometryOp::Scale, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_rotate_around_produces_rotate_around_variant() {
        let step_handles = vec![GeometryHandleId(99)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::RotateAround,
            target: GeomRef::Step(0),
            args: vec![
                ("px".into(), literal_f64(0.05)),
                ("py".into(), literal_f64(0.0)),
                ("pz".into(), literal_f64(0.0)),
                ("axis_x".into(), literal_f64(0.0)),
                ("axis_y".into(), literal_f64(0.0)),
                ("axis_z".into(), literal_f64(1.0)),
                ("angle".into(), literal_f64(std::f64::consts::FRAC_PI_2)),
            ],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        let result = result.expect("compile_geometry_op should return Some for RotateAround");

        match result {
            reify_types::GeometryOp::RotateAround {
                target,
                point,
                axis,
                angle_rad,
            } => {
                assert_eq!(target, GeometryHandleId(99));
                assert!((point[0] - 0.05).abs() < 1e-12);
                assert!((point[1]).abs() < 1e-12);
                assert!((point[2]).abs() < 1e-12);
                assert!((axis[0]).abs() < 1e-12);
                assert!((axis[1]).abs() < 1e-12);
                assert!((axis[2] - 1.0).abs() < 1e-12);
                assert!((angle_rad - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
            }
            other => panic!("expected GeometryOp::RotateAround, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_sweep_resolves_distinct_profiles() {
        // Two distinct step handles representing two wire profiles
        let step_handles = vec![GeometryHandleId(100), GeometryHandleId(200)];
        let values = ValueMap::new();

        // Create a Loft sweep that references Step(0) and Step(1)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Loft,
            profiles: vec![GeomRef::Step(0), GeomRef::Step(1)],
            args: vec![],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        let result = result.expect("compile_geometry_op should return Some for Loft");

        match result {
            reify_types::GeometryOp::Loft { profiles } => {
                assert_eq!(
                    profiles,
                    vec![GeometryHandleId(100), GeometryHandleId(200)],
                    "Loft profiles should resolve Step(0) -> handle 100, Step(1) -> handle 200"
                );
            }
            other => panic!("expected GeometryOp::Loft, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_extrude_preserves_value_type() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![("distance".into(), literal_length(0.05))],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        let result = result.expect("compile_geometry_op should return Some for Extrude");

        match result {
            reify_types::GeometryOp::Extrude { profile, distance } => {
                assert_eq!(profile, GeometryHandleId(10));
                // The distance must preserve Scalar type (not be converted to Value::Real)
                match distance {
                    reify_types::Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert!((si_value - 0.05).abs() < 1e-12, "SI value should be 0.05m");
                        assert_eq!(
                            dimension,
                            reify_types::DimensionVector::LENGTH,
                            "dimension should be LENGTH"
                        );
                    }
                    other => panic!(
                        "expected Value::Scalar, got {:?} — Extrude distance must preserve SI unit info",
                        other
                    ),
                }
            }
            other => panic!("expected GeometryOp::Extrude, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_revolve_missing_arg_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Revolve with missing 'ox' arg — should return None, not silently use 0.0
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Revolve,
            profiles: vec![GeomRef::Step(0)],
            args: vec![
                // Deliberately omit "ox"
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("angle".into(), literal_f64(std::f64::consts::PI)),
            ],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        assert!(
            result.is_none(),
            "expected None for missing 'ox' arg, got {:?}",
            result
        );
    }

    #[test]
    fn compile_geometry_op_extrude_missing_distance_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        assert!(
            result.is_none(),
            "expected None for missing 'distance' arg, got {:?}",
            result
        );
    }

    #[test]
    fn compile_geometry_op_extrude_nan_distance_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Extrude with NaN distance — should return None (runtime edge case, not invariant)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![("distance".into(), literal_f64(f64::NAN))],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        assert!(result.is_none(), "NaN extrude distance should return None");
    }

    #[test]
    fn compile_geometry_op_extrude_inf_distance_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // Extrude with Inf distance — should return None (runtime edge case, not invariant)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![("distance".into(), literal_f64(f64::INFINITY))],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        assert!(result.is_none(), "Inf extrude distance should return None");
    }

    #[test]
    fn compile_geometry_op_revolve_zero_axis_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // All 7 args present and numeric, but ax=ay=az=0.0 (zero-length rotation axis)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Revolve,
            profiles: vec![GeomRef::Step(0)],
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(0.0)),
                ("angle".into(), literal_f64(std::f64::consts::PI)),
            ],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        assert!(
            result.is_none(),
            "zero-length rotation axis should return None"
        );
    }

    #[test]
    fn compile_geometry_op_revolve_nan_axis_returns_none() {
        let step_handles = vec![GeometryHandleId(10)];
        let values = ValueMap::new();

        // All 7 args present and numeric, but ax=NaN (non-finite rotation axis)
        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Revolve,
            profiles: vec![GeomRef::Step(0)],
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(f64::NAN)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(0.0)),
                ("angle".into(), literal_f64(std::f64::consts::PI)),
            ],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        assert!(result.is_none(), "NaN rotation axis should return None");
    }

    #[test]
    fn compile_geometry_op_revolve_produces_revolve_variant() {
        let step_handles = vec![GeometryHandleId(55)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Revolve,
            profiles: vec![GeomRef::Step(0)],
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("angle".into(), literal_f64(std::f64::consts::TAU)),
            ],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        let result =
            result.expect("compile_geometry_op should return Some for Revolve with valid axis");

        match result {
            reify_types::GeometryOp::Revolve {
                profile,
                axis_origin,
                axis_dir,
                angle_rad,
            } => {
                assert_eq!(profile, GeometryHandleId(55));
                assert!((axis_origin[0]).abs() < 1e-12);
                assert!((axis_origin[1]).abs() < 1e-12);
                assert!((axis_origin[2]).abs() < 1e-12);
                assert!((axis_dir[0]).abs() < 1e-12);
                assert!((axis_dir[1]).abs() < 1e-12);
                assert!((axis_dir[2] - 1.0).abs() < 1e-12);
                assert!((angle_rad - std::f64::consts::TAU).abs() < 1e-12);
            }
            other => panic!("expected GeometryOp::Revolve, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_extrude_produces_extrude_variant() {
        let step_handles = vec![GeometryHandleId(77)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![("distance".into(), literal_length(0.03))],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        let result = result.expect("compile_geometry_op should return Some for Extrude");

        match result {
            reify_types::GeometryOp::Extrude { profile, distance } => {
                assert_eq!(profile, GeometryHandleId(77));
                match distance {
                    reify_types::Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert!(
                            (si_value - 0.03).abs() < 1e-12,
                            "SI value should be 0.03m (30mm)"
                        );
                        assert_eq!(dimension, reify_types::DimensionVector::LENGTH);
                    }
                    other => panic!("expected Value::Scalar for distance, got {:?}", other),
                }
            }
            other => panic!("expected GeometryOp::Extrude, got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_scale_negative_factor_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Scale,
            target: GeomRef::Step(0),
            args: vec![("factor".into(), literal_f64(-1.0))],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        assert!(
            result.is_none(),
            "negative scale factor should return None (inside-out geometry)"
        );
    }

    #[test]
    fn compile_geometry_op_translate_missing_arg_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // Translate with only dx — missing dy, dz
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target: GeomRef::Step(0),
            args: vec![("dx".into(), literal_f64(1.0))],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        assert!(
            result.is_none(),
            "missing dy/dz should return None, not silently default to 0.0"
        );
    }

    #[test]
    fn compile_geometry_op_scale_nan_factor_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::Scale,
            target: GeomRef::Step(0),
            args: vec![("factor".into(), literal_f64(f64::NAN))],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        assert!(result.is_none(), "NaN scale factor should return None");
    }

    #[test]
    fn compile_geometry_op_rotate_around_missing_axis_returns_none() {
        let step_handles = vec![GeometryHandleId(99)];
        let values = ValueMap::new();

        // RotateAround with missing axis_z
        let op = CompiledGeometryOp::Transform {
            kind: TransformKind::RotateAround,
            target: GeomRef::Step(0),
            args: vec![
                ("px".into(), literal_f64(0.0)),
                ("py".into(), literal_f64(0.0)),
                ("pz".into(), literal_f64(0.0)),
                ("axis_x".into(), literal_f64(0.0)),
                ("axis_y".into(), literal_f64(1.0)),
                // axis_z deliberately omitted
                ("angle".into(), literal_f64(1.0)),
            ],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        assert!(result.is_none(), "missing axis_z should return None");
    }

    #[test]
    fn compile_geometry_op_linear_pattern_missing_spacing_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // LinearPattern with dx/dy/dz/count but OMITS spacing
        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(10.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(3.0)),
                // spacing deliberately omitted
            ],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        assert!(
            result.is_none(),
            "missing spacing should return None, not silently default to Value::Undef"
        );
    }

    #[test]
    fn compile_geometry_op_circular_pattern_missing_angle_returns_none() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        // CircularPattern with ox/oy/oz/ax/ay/az/count but OMITS angle
        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Circular,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("count".into(), literal_f64(4.0)),
                // angle deliberately omitted
            ],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        assert!(
            result.is_none(),
            "missing angle should return None, not silently default to Value::Undef"
        );
    }

    #[test]
    fn compile_geometry_op_linear_pattern_valid_args() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: GeomRef::Step(0),
            args: vec![
                ("dx".into(), literal_f64(10.0)),
                ("dy".into(), literal_f64(0.0)),
                ("dz".into(), literal_f64(0.0)),
                ("count".into(), literal_f64(3.0)),
                ("spacing".into(), literal_length(0.02)),
            ],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        match result {
            Some(reify_types::GeometryOp::LinearPattern {
                target,
                direction,
                count,
                spacing,
            }) => {
                assert_eq!(target, GeometryHandleId(42));
                assert_eq!(direction, [10.0, 0.0, 0.0]);
                assert_eq!(count, 3);
                // spacing should be a Scalar value, not Undef
                assert!(
                    !matches!(spacing, reify_types::Value::Undef),
                    "spacing should not be Undef when arg is present"
                );
            }
            other => panic!("expected Some(LinearPattern), got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_circular_pattern_valid_args() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Circular,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("ax".into(), literal_f64(0.0)),
                ("ay".into(), literal_f64(0.0)),
                ("az".into(), literal_f64(1.0)),
                ("count".into(), literal_f64(4.0)),
                ("angle".into(), literal_f64(std::f64::consts::FRAC_PI_2)),
            ],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        match result {
            Some(reify_types::GeometryOp::CircularPattern {
                target,
                axis_origin,
                axis_dir,
                count,
                angle,
            }) => {
                assert_eq!(target, GeometryHandleId(42));
                assert_eq!(axis_origin, [0.0, 0.0, 0.0]);
                assert_eq!(axis_dir, [0.0, 0.0, 1.0]);
                assert_eq!(count, 4);
                // angle should be a Real value, not Undef
                assert!(
                    !matches!(angle, reify_types::Value::Undef),
                    "angle should not be Undef when arg is present"
                );
            }
            other => panic!("expected Some(CircularPattern), got {:?}", other),
        }
    }

    #[test]
    fn compile_geometry_op_mirror_valid_args() {
        let step_handles = vec![GeometryHandleId(42)];
        let values = ValueMap::new();

        let op = CompiledGeometryOp::Pattern {
            kind: PatternKind::Mirror,
            target: GeomRef::Step(0),
            args: vec![
                ("ox".into(), literal_f64(0.0)),
                ("oy".into(), literal_f64(0.0)),
                ("oz".into(), literal_f64(0.0)),
                ("nx".into(), literal_f64(1.0)),
                ("ny".into(), literal_f64(0.0)),
                ("nz".into(), literal_f64(0.0)),
            ],
        };

        let result = compile_geometry_op(&op, &values, &step_handles, &[], &HashMap::new());
        match result {
            Some(reify_types::GeometryOp::Mirror {
                target,
                plane_origin,
                plane_normal,
            }) => {
                assert_eq!(target, GeometryHandleId(42));
                assert_eq!(plane_origin, [0.0, 0.0, 0.0]);
                assert_eq!(plane_normal, [1.0, 0.0, 0.0]);
            }
            other => panic!("expected Some(Mirror), got {:?}", other),
        }
    }
}
