//! Concurrent evaluation adapter bridging Engine state with the async scheduler.
//!
//! `ConcurrentEvalAdapter` implements `AsyncNodeEvaluator` using interior mutability
//! (std::sync::RwLock for values, Mutex for results) to enable safe concurrent
//! node evaluation across tokio tasks.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

use reify_compiler::ValueCellKind;
use reify_eval::cache::{CachedResult, EvalOutcome, NodeId};
use reify_eval::deps::{extract_dependency_trace, ReverseDependencyIndex};
use reify_eval::graph::EvaluationGraph;
use reify_eval::{CheckResult, ConcurrentEditResult, ConcurrentEditSetup, ConcurrentNodeResult};
use reify_types::{
    ContentHash, DeterminacyState, PersistentMap, Value, ValueCellId, ValueMap, VersionId,
};

use crate::concurrent::{AsyncNodeEvaluator, CancellationToken, ConcurrentScheduler, SchedulerError};

/// Combined skip state for atomic check-and-modify in concurrent early cutoff.
///
/// A single Mutex wrapping both `skipped` and `has_changed_parent` prevents
/// TOCTOU races between concurrent tasks at the same topological level that
/// could otherwise interleave the has_changed_parent check and skipped insert.
struct SkipState {
    /// Nodes to skip due to early cutoff of upstream nodes.
    skipped: HashSet<NodeId>,
    /// Nodes that have at least one Changed parent (should never be skipped).
    has_changed_parent: HashSet<NodeId>,
}

/// Adapter that implements `AsyncNodeEvaluator` for concurrent evaluation.
///
/// Wraps Engine state extracted by `prepare_concurrent_edit()` in interior-mutable
/// containers suitable for sharing across tokio tasks via `Arc<Self>`.
///
/// Uses `std::sync::RwLock` (not tokio's) because expression evaluation is CPU-bound
/// and completes in microseconds — locks are never held across .await points.
pub struct ConcurrentEvalAdapter {
    /// The evaluation graph (immutable during evaluation).
    graph: Arc<EvaluationGraph>,
    /// Current values, updated as nodes are evaluated.
    values: Arc<RwLock<ValueMap>>,
    /// Snapshot values with determinacy state, updated as nodes are evaluated.
    snapshot_values: Arc<RwLock<PersistentMap<ValueCellId, (Value, DeterminacyState)>>>,
    /// Pre-extracted content hashes for Changed/Unchanged determination.
    previous_hashes: Arc<HashMap<NodeId, ContentHash>>,
    /// Reverse dependency index for early cutoff propagation.
    reverse_index: Arc<ReverseDependencyIndex>,
    /// Combined skip state: skipped set + has_changed_parent set, behind a
    /// single Mutex for atomic check-and-modify in concurrent evaluation.
    skip_state: Arc<Mutex<SkipState>>,
    /// Collected evaluation results.
    results: Arc<Mutex<Vec<ConcurrentNodeResult>>>,
    /// Version for this evaluation.
    #[allow(dead_code)]
    version: VersionId,
}

impl ConcurrentEvalAdapter {
    /// Create an adapter from a `ConcurrentEditSetup`.
    ///
    /// Seeds `has_changed_parent` with dependents of the changed cells.
    /// This ensures nodes that read the changed param directly are never
    /// incorrectly skipped by early cutoff from an unchanged intermediary.
    pub fn from_setup(setup: &ConcurrentEditSetup) -> Self {
        // Seed has_changed_parent from changed_cells' dependents
        let mut has_changed_parent = HashSet::new();
        for changed_cell in &setup.changed_cells {
            for dependent in setup.reverse_index.dependents_of(changed_cell) {
                has_changed_parent.insert(dependent.clone());
            }
        }

        Self {
            graph: Arc::new(setup.graph.clone()),
            values: Arc::new(RwLock::new(setup.values.clone())),
            snapshot_values: Arc::new(RwLock::new(setup.snapshot_values.clone())),
            previous_hashes: Arc::new(setup.previous_hashes.clone()),
            reverse_index: Arc::new(setup.reverse_index.clone()),
            skip_state: Arc::new(Mutex::new(SkipState {
                skipped: HashSet::new(),
                has_changed_parent,
            })),
            results: Arc::new(Mutex::new(Vec::new())),
            version: setup.version,
        }
    }

    /// Get a snapshot of the current values (for testing/inspection).
    pub fn values(&self) -> ValueMap {
        self.values.read().unwrap().clone()
    }

    /// Take the collected results (for testing/inspection).
    pub fn take_results(&self) -> Vec<ConcurrentNodeResult> {
        self.results.lock().unwrap().clone()
    }

    /// Get the set of skipped nodes (for testing/inspection).
    pub fn skipped(&self) -> HashSet<NodeId> {
        self.skip_state.lock().unwrap().skipped.clone()
    }

    /// Build a `ConcurrentEditResult` via shared references (cloning).
    ///
    /// Used as a fallback when `Arc::try_unwrap` fails because outstanding
    /// references still exist. Slightly less efficient than `into_result`
    /// since it clones each inner container through locks.
    pub fn build_result_shared(&self, eval_set: &[NodeId]) -> ConcurrentEditResult {
        let values = self.values.read().unwrap_or_else(|e| e.into_inner()).clone();
        let snapshot_values = self.snapshot_values.read().unwrap_or_else(|e| e.into_inner()).clone();
        let node_results = self.results.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let skipped = self.skip_state.lock().unwrap_or_else(|e| e.into_inner()).skipped.clone();

        let actual_eval_set: Vec<NodeId> = eval_set
            .iter()
            .filter(|n| !skipped.contains(n))
            .cloned()
            .collect();

        ConcurrentEditResult {
            values,
            snapshot_values,
            node_results,
            actual_eval_set,
            skipped,
            resolved_params: std::collections::HashMap::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Consume the adapter and produce a `ConcurrentEditResult`.
    ///
    /// Extracts the final values, snapshot_values, results, and skipped set.
    pub fn into_result(self, eval_set: &[NodeId]) -> ConcurrentEditResult {
        let values = match Arc::try_unwrap(self.values) {
            Ok(lock) => lock.into_inner().unwrap_or_else(|e| e.into_inner()),
            Err(arc) => arc.read().unwrap_or_else(|e| e.into_inner()).clone(),
        };
        let snapshot_values = match Arc::try_unwrap(self.snapshot_values) {
            Ok(lock) => lock.into_inner().unwrap_or_else(|e| e.into_inner()),
            Err(arc) => arc.read().unwrap_or_else(|e| e.into_inner()).clone(),
        };
        let node_results = match Arc::try_unwrap(self.results) {
            Ok(lock) => lock.into_inner().unwrap_or_else(|e| e.into_inner()),
            Err(arc) => arc.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        };
        let skipped = match Arc::try_unwrap(self.skip_state) {
            Ok(lock) => lock.into_inner().unwrap_or_else(|e| e.into_inner()).skipped,
            Err(arc) => arc.lock().unwrap_or_else(|e| e.into_inner()).skipped.clone(),
        };

        // actual_eval_set = eval_set nodes that weren't skipped
        let actual_eval_set: Vec<NodeId> = eval_set
            .iter()
            .filter(|n| !skipped.contains(n))
            .cloned()
            .collect();

        ConcurrentEditResult {
            values,
            snapshot_values,
            node_results,
            actual_eval_set,
            skipped,
            resolved_params: std::collections::HashMap::new(),
            diagnostics: Vec::new(),
        }
    }
}

impl AsyncNodeEvaluator for ConcurrentEvalAdapter {
    fn is_dirty(&self, node: &NodeId) -> bool {
        !self.skip_state.lock().unwrap().skipped.contains(node)
    }

    async fn evaluate(&self, node: NodeId) -> EvalOutcome {
        // Only evaluate Value nodes with expressions
        if let NodeId::Value(ref vcid) = node
            && let Some(cell_node) = self.graph.value_cells.get(vcid)
            && (cell_node.kind == ValueCellKind::Let || cell_node.kind == ValueCellKind::Auto)
            && cell_node.default_expr.is_some()
        {
            let expr = cell_node.default_expr.as_ref().unwrap();

            // Read current values (brief read lock)
            let current_values = {
                self.values.read().unwrap().clone()
            };

            // Evaluate expression (pure, no lock held)
            let val = reify_expr::eval_expr(expr, &current_values);

            // Compute dependency trace
            let trace = extract_dependency_trace(expr);

            // Compute content hash for early cutoff
            let cached_result = CachedResult::Value(
                val.clone(),
                DeterminacyState::Determined,
            );
            let new_hash = cached_result.content_hash();

            // Compare with previous hash
            let outcome = if let Some(old_hash) = self.previous_hashes.get(&node) {
                if new_hash == *old_hash {
                    EvalOutcome::Unchanged
                } else {
                    EvalOutcome::Changed
                }
            } else {
                // No previous hash → first evaluation, always Changed
                EvalOutcome::Changed
            };

            // Write result to shared values (brief write lock)
            {
                let mut values = self.values.write().unwrap();
                values.insert(vcid.clone(), val.clone());
            }

            // Write to snapshot values (brief write lock)
            {
                let mut sv = self.snapshot_values.write().unwrap();
                sv.insert(
                    vcid.clone(),
                    (val.clone(), DeterminacyState::Determined),
                );
            }

            // Early cutoff with mixed fan-in protection (atomic via Mutex):
            // - Changed: propagate has_changed_parent to dependents,
            //   remove them from skipped (in case an earlier Unchanged
            //   parent at the same level added them prematurely).
            // - Unchanged: only add dependents to skipped if they do NOT
            //   have a Changed parent.
            {
                let dependents = self.reverse_index.dependents_of(vcid);
                if !dependents.is_empty() {
                    let mut state = self.skip_state.lock().unwrap();
                    if outcome == EvalOutcome::Changed {
                        for dep in dependents {
                            state.has_changed_parent.insert(dep.clone());
                            state.skipped.remove(dep);
                        }
                    } else {
                        // Unchanged
                        for dep in dependents {
                            if !state.has_changed_parent.contains(dep) {
                                state.skipped.insert(dep.clone());
                            }
                        }
                    }
                }
            }

            // Record result
            self.results.lock().unwrap().push(ConcurrentNodeResult {
                node: node.clone(),
                value: val,
                determinacy: DeterminacyState::Determined,
                trace,
                outcome,
            });

            return outcome;
        }

        // Non-Value nodes or Value nodes without expressions: Unchanged
        EvalOutcome::Unchanged
    }
}

/// Execute a concurrent parameter edit using ConcurrentScheduler.
///
/// This is the main entry point for concurrent evaluation:
/// 1. Call `engine.prepare_concurrent_edit()` to extract state
/// 2. Create `ConcurrentEvalAdapter` from setup
/// 3. Run through `ConcurrentScheduler::execute()`
/// 4. Collect results into `ConcurrentEditResult`
///
/// After this returns, the caller should call `engine.apply_concurrent_edit()`
/// to merge the results back into the Engine.
pub async fn edit_param_concurrent(
    engine: &mut reify_eval::Engine,
    cell: ValueCellId,
    new_value: Value,
    cancel: &CancellationToken,
) -> Result<(ConcurrentEditSetup, ConcurrentEditResult), SchedulerError> {
    let setup = engine.prepare_concurrent_edit(cell, new_value);
    let eval_set = setup.eval_set.clone();
    let traces = setup.traces.clone();

    let adapter = ConcurrentEvalAdapter::from_setup(&setup);
    let adapter_arc = Arc::new(adapter);

    let scheduler = ConcurrentScheduler;
    match scheduler
        .execute(eval_set.clone(), Arc::clone(&adapter_arc), &traces, cancel)
        .await
    {
        Ok(_changed) => {
            // Extract result from adapter. After scheduler completes, the only
            // remaining Arc reference should be ours — but if a spawned task
            // retained a clone, fall back to building the result via shared access.
            let mut result = match Arc::try_unwrap(adapter_arc) {
                Ok(adapter) => adapter.into_result(&eval_set),
                Err(arc) => arc.build_result_shared(&eval_set),
            };

            // Resolution phase: run solver synchronously after concurrent
            // value evaluation completes. NLopt is single-threaded, so this
            // sequential step is architecturally correct.
            let (resolved_params, diagnostics) =
                engine.resolve_concurrent_edit(&setup, &mut result);

            result.resolved_params = resolved_params;
            result.diagnostics = diagnostics;

            Ok((setup, result))
        }
        Err(e) => {
            // Rollback: restore eval_set nodes from Pending → Final and
            // revert version/snapshot ID bumps to prevent resource leak.
            engine.rollback_concurrent_edit(&setup);
            Err(e)
        }
    }
}

/// Concurrent edit + constraint checking, analogous to `Engine::edit_check()`.
///
/// Calls `edit_param_concurrent` to compute new values (including auto-resolution),
/// applies the result to the engine, then checks all constraints against the
/// updated values.
///
/// Returns a `CheckResult` containing values, constraint results, diagnostics,
/// and resolved_params — the same shape as `edit_check` in the sequential path.
pub async fn edit_check_concurrent(
    engine: &mut reify_eval::Engine,
    cell: ValueCellId,
    new_value: Value,
    cancel: &CancellationToken,
) -> Result<CheckResult, SchedulerError> {
    let (setup, result) = edit_param_concurrent(engine, cell, new_value, cancel).await?;

    // Capture resolution metadata before apply consumes the result
    let resolved_params = result.resolved_params.clone();
    let mut diagnostics = result.diagnostics.clone();
    let values = result.values.clone();

    // Apply concurrent edit to update engine state
    engine.apply_concurrent_edit(&setup, result);

    // Check constraints against the updated values
    let (constraint_results, constraint_diagnostics) =
        engine.check_constraints_with_values(&values);
    diagnostics.extend(constraint_diagnostics);

    Ok(CheckResult {
        values,
        constraint_results,
        diagnostics,
        resolved_params,
    })
}
