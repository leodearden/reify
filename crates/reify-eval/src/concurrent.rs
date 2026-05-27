// Concurrent edit support — structs and Engine methods for prepare/apply/rollback/resolve.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use reify_core::{ContentHash, Diagnostic, SnapshotId, ValueCellId, VersionId};
use reify_ir::{AutoParam, CompiledFunction, DeterminacyState, OptimizationObjective, PersistentMap, ResolutionProblem, SnapshotProvenance, SolveResult, Value, ValueMap};

use crate::cache::{CachedResult, EvalOutcome, NodeId};
use crate::deps::{DependencyTrace, ReverseDependencyIndex, extract_dependency_trace};
use crate::journal::{EvalEvent, EventKind, EventPayload};
use crate::{Engine, EngineError};

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
    /// Shares the same Arc allocation as Engine::functions — assigned via Arc::clone
    /// in prepare_concurrent_edit, so no deep copy of the function table is made
    /// (task #1997).
    pub functions: Arc<[CompiledFunction]>,
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
    /// Actual wall-clock time spent evaluating the expression, measured by
    /// the concurrent adapter around the `eval_expr` call. `None` if timing
    /// was not captured (e.g. for non-expression nodes). Used by
    /// `apply_concurrent_edit` to record accurate journal Duration payloads
    /// instead of measuring apply-loop time.
    pub eval_duration: Option<std::time::Duration>,
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
        let dirty_cone = crate::dirty::compute_dirty_cone(
            &changed_set,
            &state.reverse_index,
            &state.snapshot.graph,
        );
        let eval_set = crate::dirty::compute_eval_set(&dirty_cone, &self.demand, &state.trace_map);

        // Build the full ValueMap from snapshot values.
        // new_snapshot_values already contains the updated cell (inserted above via
        // new_snapshot_values.insert), so copying all entries here covers it — no
        // duplicate insert needed.
        let mut values = ValueMap::new();
        for (id, (val, _det)) in new_snapshot_values.iter() {
            values.insert(id.clone(), val.clone());
        }

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
            functions: Arc::clone(&self.functions),
            meta_map: Arc::clone(&self.meta_map),
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
    ) {
        // Record cache entries and journal events for each evaluated node
        for node_result in &result.node_results {
            // Use Instant::now() as the apply-time timestamp for ordering.
            // The Duration payload comes from node_result.eval_duration (the actual
            // expression evaluation time measured by the concurrent adapter), falling
            // back to apply-loop elapsed time only when eval_duration is absent.
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

            let duration = node_result.eval_duration.unwrap_or_else(|| start.elapsed());
            self.journal.record(EvalEvent {
                timestamp: Instant::now(),
                node_id: node_result.node.clone(),
                kind: EventKind::Completed {
                    outcome: node_result.outcome,
                },
                version: setup.version,
                payload: Some(EventPayload::Duration(duration)),
            });
        }

        // Restore freshness to Final for skipped nodes
        for node_id in &result.skipped {
            self.cache.restore_final(node_id);
        }

        // Commit solver-resolved auto param values to engine state.
        // These were computed by resolve_concurrent_edit but must only
        // be persisted here so that resolve does not write to param_overrides.
        // Note: resolve *does* mutate engine cache state via record_evaluation().
        for (id, val) in &result.resolved_params {
            self.param_overrides.insert(id.clone(), val.clone());
        }

        // Update current snapshot
        let state = self
            .eval_state
            .as_mut()
            .expect("apply_concurrent_edit requires eval_state from eval()");
        let mut new_snapshot = state.snapshot.clone();
        new_snapshot.id = setup.snapshot_id;
        new_snapshot.version = setup.version;
        new_snapshot.values = result.snapshot_values;
        new_snapshot.provenance = SnapshotProvenance::Edit {
            changed: setup.changed_cells.clone(),
            parent: setup.parent_snapshot_id,
        };
        state.snapshot = new_snapshot;

        // Update last eval set
        self.last_eval_set = result.actual_eval_set;
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
    ///
    /// # Side effects
    ///
    /// Mutates `self.cache` via `record_evaluation()` for resolved auto params
    /// and propagated let bindings. Does NOT write to `self.param_overrides` —
    /// that happens in `apply_concurrent_edit()`. Assigns resolved values and
    /// diagnostics directly onto `result.resolved_params` / `result.diagnostics`.
    ///
    /// # Panics
    ///
    /// Panics (in both debug and release builds) if `result.resolved_params` or
    /// `result.diagnostics` is non-empty on entry. Both fields are output buckets,
    /// not accumulators. A non-empty bucket indicates a double-call or result reuse;
    /// callers must always supply a freshly constructed `ConcurrentEditResult`.
    pub fn resolve_concurrent_edit(
        &mut self,
        setup: &ConcurrentEditSetup,
        result: &mut ConcurrentEditResult,
    ) {
        // Load-bearing contract: callers MUST pass a fresh ConcurrentEditResult
        // with empty resolved_params and diagnostics — these are output buckets,
        // not input or accumulator fields. The assert! enforces this uniformly
        // in both debug and release builds. A non-empty bucket on entry means the
        // caller has a bug (double-call, result reuse) that must be fixed there.
        assert!(
            result.resolved_params.is_empty(),
            "resolve_concurrent_edit: resolved_params must be empty on entry (double-call?)"
        );
        assert!(
            result.diagnostics.is_empty(),
            "resolve_concurrent_edit: diagnostics must be empty on entry (double-call?)"
        );

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
                if node.kind.is_auto() {
                    let entry = entity_groups
                        .entry(node.id.entity.clone())
                        .or_insert_with(|| (Vec::new(), HashSet::new()));
                    entry.0.push(AutoParam {
                        id: node.id.clone(),
                        param_type: node.cell_type.clone(),
                        bounds: None,
                        free: node.kind.is_auto_free(),
                    });
                    entry.1.insert(node.id.clone());
                }
            }

            // Compute dirty cone from changed cells
            let dirty_cone =
                crate::dirty::compute_dirty_cone(&setup.changed_cells, reverse_index, &setup.graph);

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
                    // Arc::clone is O(1) — threads the same Arc allocation held by
                    // ConcurrentEditSetup.functions (and transitively Engine.functions)
                    // into ResolutionProblem.functions (tasks #1997, #2286).
                    functions: Arc::clone(&setup.functions),
                };

                match solver.solve(&problem) {
                    SolveResult::Solved {
                        values: solver_values,
                        unique,
                    } => {
                        for (id, val) in &solver_values {
                            result.values.insert(id.clone(), val.clone());
                            result.resolved_params.insert(id.clone(), val.clone());
                            all_resolved_ids.insert(id.clone());

                            result
                                .snapshot_values
                                .insert(id.clone(), (val.clone(), DeterminacyState::Determined));

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
                        if !unique {
                            for ap in auto_param_list {
                                if ap.free {
                                    result.diagnostics.push(Diagnostic::warning(format!(
                                        "Parameter `{}` resolved via auto(free) \
                                         -- result is not uniquely determined.",
                                        ap.id.member
                                    )));
                                }
                            }
                        }
                    }
                    SolveResult::Infeasible {
                        diagnostics: solver_diags,
                    } => {
                        result.diagnostics.extend(solver_diags);
                    }
                    SolveResult::NoProgress { reason } => {
                        result.diagnostics.push(Diagnostic::warning(format!(
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
                let wave2_dirty = crate::dirty::compute_dirty_cone(
                    &all_resolved_ids,
                    reverse_index,
                    &setup.graph,
                );
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
                        result
                            .snapshot_values
                            .insert(vcid.clone(), (val.clone(), DeterminacyState::Determined));

                        let trace = extract_dependency_trace(expr);
                        let cached_result = CachedResult::Value(val, DeterminacyState::Determined);
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify ConcurrentNodeResult can be constructed and fields accessed.
    #[test]
    fn concurrent_node_result_construction_and_field_access() {
        let node_id = NodeId::Value(ValueCellId::new("E", "x"));
        let result = ConcurrentNodeResult {
            node: node_id.clone(),
            value: Value::Real(42.0),
            determinacy: DeterminacyState::Determined,
            trace: DependencyTrace::default(),
            outcome: EvalOutcome::Changed,
            eval_duration: None,
        };
        assert_eq!(result.node, node_id);
        assert_eq!(result.value, Value::Real(42.0));
        assert_eq!(result.determinacy, DeterminacyState::Determined);
        assert!(matches!(result.outcome, EvalOutcome::Changed));
    }

    /// Verify ConcurrentEditResult can be constructed with known values
    /// and all fields are accessible.
    #[test]
    fn concurrent_edit_result_construction_and_field_access() {
        let result = ConcurrentEditResult {
            values: ValueMap::new(),
            snapshot_values: PersistentMap::new(),
            node_results: vec![],
            actual_eval_set: vec![],
            skipped: HashSet::new(),
            resolved_params: HashMap::new(),
            diagnostics: vec![],
        };
        assert!(result.values.is_empty());
        assert!(result.node_results.is_empty());
        assert!(result.actual_eval_set.is_empty());
        assert!(result.skipped.is_empty());
        assert!(result.resolved_params.is_empty());
        assert!(result.diagnostics.is_empty());
    }

    /// Verify ConcurrentEditResult round-trips node results correctly.
    #[test]
    fn concurrent_edit_result_with_node_results() {
        let node_id = NodeId::Value(ValueCellId::new("E", "y"));
        let node_result = ConcurrentNodeResult {
            node: node_id.clone(),
            value: Value::Real(7.0),
            determinacy: DeterminacyState::Determined,
            trace: DependencyTrace::default(),
            outcome: EvalOutcome::Unchanged,
            eval_duration: None,
        };
        let result = ConcurrentEditResult {
            values: ValueMap::new(),
            snapshot_values: PersistentMap::new(),
            node_results: vec![node_result],
            actual_eval_set: vec![node_id.clone()],
            skipped: HashSet::new(),
            resolved_params: HashMap::new(),
            diagnostics: vec![],
        };
        assert_eq!(result.node_results.len(), 1);
        assert_eq!(result.node_results[0].node, node_id);
        assert_eq!(result.node_results[0].value, Value::Real(7.0));
        assert_eq!(result.actual_eval_set.len(), 1);
    }

    /// step-1: After prepare_concurrent_edit, values.get(cell) equals the new value.
    ///
    /// The loop at lines 130-132 copies every entry from new_snapshot_values into
    /// values, which already includes the updated cell (inserted at lines 117-119).
    /// This test establishes the baseline we must preserve after removing the
    /// redundant values.insert at line 134.
    #[test]
    fn prepare_concurrent_edit_values_has_updated_cell() {
        use reify_test_support::bracket_compiled_module;
        use reify_test_support::mocks::MockConstraintChecker;

        let module = bracket_compiled_module();
        let checker = MockConstraintChecker::new();
        let mut engine = Engine::new(Box::new(checker), None);
        let _initial = engine.eval(&module);

        let e = "Bracket";
        let width_id = ValueCellId::new(e, "width");
        let new_width = Value::length(0.1);

        let setup = engine
            .prepare_concurrent_edit(width_id.clone(), new_width.clone())
            .unwrap();

        // values.get(width_id) must equal the new value — established by the
        // loop that copies all new_snapshot_values entries into values.
        assert_eq!(
            setup.values.get(&width_id),
            Some(&new_width),
            "values map must contain the updated cell value (loop covers it)"
        );

        // snapshot_values should agree: it was updated before the loop.
        let (snap_val, snap_det) = setup.snapshot_values.get(&width_id).unwrap();
        assert_eq!(snap_val, &new_width, "snapshot_values should match");
        assert_eq!(snap_det, &DeterminacyState::Determined);
    }

    /// step-7 (revised): When eval_duration is None, apply_concurrent_edit falls
    /// back to `start.elapsed()` for the journal Completed event's Duration payload.
    ///
    /// Verifies the `unwrap_or_else(|| start.elapsed())` fallback path produces
    /// a non-None Duration — not that the struct field round-trips (which is a
    /// Rust language guarantee, not a behavioral assertion).
    #[test]
    fn apply_concurrent_edit_fallback_duration_when_eval_duration_none() {
        use reify_test_support::bracket_compiled_module;
        use reify_test_support::mocks::MockConstraintChecker;

        use crate::journal::{EventKind, EventPayload};

        let module = bracket_compiled_module();
        let checker = MockConstraintChecker::new();
        let mut engine = Engine::new(Box::new(checker), None);
        let _initial = engine.eval(&module);

        let e = "Bracket";
        let width_id = ValueCellId::new(e, "width");
        let volume_id = ValueCellId::new(e, "volume");
        let volume_node = NodeId::Value(volume_id.clone());

        let setup = engine
            .prepare_concurrent_edit(width_id.clone(), Value::length(0.1))
            .unwrap();

        let new_volume = Value::Scalar {
            si_value: 5e-5,
            dimension: reify_core::dimension::DimensionVector::VOLUME,
        };

        let mut snapshot_values = setup.snapshot_values.clone();
        snapshot_values.insert(
            volume_id.clone(),
            (new_volume.clone(), DeterminacyState::Determined),
        );
        let mut values = setup.values.clone();
        values.insert(volume_id.clone(), new_volume.clone());

        // eval_duration: None  →  fallback path is taken
        let node_results = vec![ConcurrentNodeResult {
            node: volume_node.clone(),
            value: new_volume.clone(),
            determinacy: DeterminacyState::Determined,
            trace: DependencyTrace::default(),
            outcome: EvalOutcome::Changed,
            eval_duration: None,
        }];

        let result = ConcurrentEditResult {
            values,
            snapshot_values,
            node_results,
            actual_eval_set: vec![volume_node.clone()],
            skipped: std::collections::HashSet::new(),
            resolved_params: HashMap::new(),
            diagnostics: vec![],
        };

        engine.apply_concurrent_edit(&setup, result);

        let volume_events = engine.journal().events_for_node(&volume_node);
        let completed = volume_events
            .iter()
            .filter(|ev| ev.version == setup.version)
            .find(|ev| matches!(ev.kind, EventKind::Completed { .. }))
            .expect("should have a Completed event for volume at setup.version");

        // The fallback path (`start.elapsed()`) must still produce Some(Duration(_)).
        assert!(
            matches!(completed.payload, Some(EventPayload::Duration(_))),
            "Completed event must carry a Duration payload via the start.elapsed() fallback; \
             got: {:?}",
            completed.payload
        );
    }
}
