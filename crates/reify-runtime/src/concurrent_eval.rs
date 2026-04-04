//! Concurrent evaluation adapter bridging Engine state with the async scheduler.
//!
//! `ConcurrentEvalAdapter` implements `AsyncNodeEvaluator` using interior mutability
//! (std::sync::RwLock for values, Mutex for results) to enable safe concurrent
//! node evaluation across tokio tasks.
//!
//! Dirty/skip decisions are made by the scheduler using pre-computed `changed_vcids`
//! tracking, not by the adapter. The adapter is purely computational — it evaluates
//! expressions, computes content hashes, and records results.
//!
//! **Lock poisoning recovery:** All lock acquisitions recover gracefully from poisoned
//! locks via private helper methods (`read_values()`, `write_values()`,
//! `read_snapshot_values()`, `write_snapshot_values()`, `lock_results()`) that emit
//! `tracing::warn!` on recovery. A poisoned lock means a previous evaluation task
//! panicked while holding the lock — the data may be partially updated, but recovering
//! prevents cascading panics that would take down all concurrent tasks sharing the
//! adapter. The `into_result()` method uses inline recovery with `tracing::warn!`
//! because `self` is consumed by `Arc::try_unwrap`. This matches the pattern used in
//! `SharedPriorityPromoter`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

use reify_compiler::ValueCellKind;
use reify_eval::cache::{CachedResult, EvalOutcome, NodeId};
use reify_eval::deps::extract_dependency_trace;
use reify_eval::graph::EvaluationGraph;
use reify_eval::{CheckResult, ConcurrentEditResult, ConcurrentEditSetup, ConcurrentNodeResult};
use reify_types::{
    CompiledFunction, ContentHash, DeterminacyState, PersistentMap, Value, ValueCellId, ValueMap,
    VersionId,
};

use crate::concurrent::{
    AsyncNodeEvaluator, CancellationToken, ConcurrentScheduler, SchedulerError,
};

/// Adapter that implements `AsyncNodeEvaluator` for concurrent evaluation.
///
/// Wraps Engine state extracted by `prepare_concurrent_edit()` in interior-mutable
/// containers suitable for sharing across tokio tasks via `Arc<Self>`.
///
/// Uses `std::sync::RwLock` (not tokio's) because expression evaluation is CPU-bound
/// and completes in microseconds — locks are never held across .await points.
///
/// The adapter is purely computational: it evaluates expressions, computes content
/// hashes for Changed/Unchanged determination, and records results. Skip decisions
/// are made by the scheduler via pre-computed `changed_vcids` tracking.
///
/// All lock acquisitions recover gracefully from poisoning — if an evaluation task
/// panics mid-computation, subsequent lock acquisitions on the same adapter will
/// extract the inner data from the `PoisonError` rather than propagating the panic.
pub struct ConcurrentEvalAdapter {
    /// The evaluation graph (immutable during evaluation).
    graph: Arc<EvaluationGraph>,
    /// Current values, updated as nodes are evaluated.
    values: Arc<RwLock<ValueMap>>,
    /// Snapshot values with determinacy state, updated as nodes are evaluated.
    snapshot_values: Arc<RwLock<PersistentMap<ValueCellId, (Value, DeterminacyState)>>>,
    /// Pre-extracted content hashes for Changed/Unchanged determination.
    previous_hashes: Arc<HashMap<NodeId, ContentHash>>,
    /// Collected evaluation results.
    results: Arc<Mutex<Vec<ConcurrentNodeResult>>>,
    /// User-defined functions for evaluating UserFunctionCall nodes.
    functions: Vec<CompiledFunction>,
    /// Template-to-meta-entries mapping for resolving MetaAccess expressions.
    meta_map: Arc<HashMap<String, HashMap<String, String>>>,
    /// Version for this evaluation.
    #[allow(dead_code)]
    version: VersionId,
}

impl ConcurrentEvalAdapter {
    /// Create an adapter from a `ConcurrentEditSetup`.
    ///
    /// The adapter is purely computational — skip decisions are handled by the
    /// scheduler. No skip state or reverse dependency index is needed here.
    pub fn from_setup(setup: &ConcurrentEditSetup) -> Self {
        Self {
            graph: Arc::new(setup.graph.clone()),
            values: Arc::new(RwLock::new(setup.values.clone())),
            snapshot_values: Arc::new(RwLock::new(setup.snapshot_values.clone())),
            previous_hashes: Arc::new(setup.previous_hashes.clone()),
            results: Arc::new(Mutex::new(Vec::new())),
            functions: setup.functions.clone(),
            meta_map: Arc::clone(&setup.meta_map),
            version: setup.version,
        }
    }

    /// Acquire a read lock on `values`, recovering from poison with a warning.
    fn read_values(&self) -> RwLockReadGuard<'_, ValueMap> {
        self.values.read().unwrap_or_else(|e| {
            tracing::warn!("values RwLock poisoned, recovering: {e}");
            e.into_inner()
        })
    }

    /// Acquire a write lock on `values`, recovering from poison with a warning.
    fn write_values(&self) -> RwLockWriteGuard<'_, ValueMap> {
        self.values.write().unwrap_or_else(|e| {
            tracing::warn!("values RwLock poisoned, recovering: {e}");
            e.into_inner()
        })
    }

    /// Acquire a read lock on `snapshot_values`, recovering from poison with a warning.
    fn read_snapshot_values(
        &self,
    ) -> RwLockReadGuard<'_, PersistentMap<ValueCellId, (Value, DeterminacyState)>> {
        self.snapshot_values.read().unwrap_or_else(|e| {
            tracing::warn!("snapshot_values RwLock poisoned, recovering: {e}");
            e.into_inner()
        })
    }

    /// Acquire a write lock on `snapshot_values`, recovering from poison with a warning.
    fn write_snapshot_values(
        &self,
    ) -> RwLockWriteGuard<'_, PersistentMap<ValueCellId, (Value, DeterminacyState)>> {
        self.snapshot_values.write().unwrap_or_else(|e| {
            tracing::warn!("snapshot_values RwLock poisoned, recovering: {e}");
            e.into_inner()
        })
    }

    /// Acquire a lock on `results`, recovering from poison with a warning.
    fn lock_results(&self) -> MutexGuard<'_, Vec<ConcurrentNodeResult>> {
        self.results.lock().unwrap_or_else(|e| {
            tracing::warn!("results Mutex poisoned, recovering: {e}");
            e.into_inner()
        })
    }

    /// Get a snapshot of the current values (for testing/inspection).
    ///
    /// Recovers gracefully from poisoned locks via `read_values()` helper.
    pub fn values(&self) -> ValueMap {
        self.read_values().clone()
    }

    /// Get a snapshot of the current snapshot_values (for testing/inspection).
    ///
    /// Recovers gracefully from poisoned locks via `read_snapshot_values()` helper.
    pub fn snapshot_values(
        &self,
    ) -> PersistentMap<ValueCellId, (Value, DeterminacyState)> {
        self.read_snapshot_values().clone()
    }

    /// Take the collected results (for testing/inspection).
    ///
    /// Recovers gracefully from poisoned locks via `lock_results()` helper.
    pub fn take_results(&self) -> Vec<ConcurrentNodeResult> {
        self.lock_results().clone()
    }

    /// Build a `ConcurrentEditResult` via shared references (cloning).
    ///
    /// Used as a fallback when `Arc::try_unwrap` fails because outstanding
    /// references still exist. Slightly less efficient than `into_result`
    /// since it clones each inner container through locks.
    ///
    /// Recovers gracefully from poisoned locks via helper methods — if a prior
    /// evaluation task panicked, the data may be partially updated but this
    /// method will not panic.
    ///
    /// The `skipped` set is provided by the scheduler's `SchedulerResult`.
    pub fn build_result_shared(
        &self,
        eval_set: &[NodeId],
        skipped: HashSet<NodeId>,
    ) -> ConcurrentEditResult {
        let values = self.read_values().clone();
        let snapshot_values = self.read_snapshot_values().clone();
        let node_results = self.lock_results().clone();

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
    /// Extracts the final values, snapshot_values, and results.
    /// Recovers gracefully from poisoned locks on both the `into_inner()`
    /// (sole-owner) and `read()`/`lock()` (shared-reference fallback) paths,
    /// emitting `tracing::warn!` on each recovery. These stay inline because
    /// `self` is consumed by `Arc::try_unwrap` — the helper methods (which take
    /// `&self`) cannot be used here.
    ///
    /// The `skipped` set is provided by the scheduler's `SchedulerResult`.
    pub fn into_result(
        self,
        eval_set: &[NodeId],
        skipped: HashSet<NodeId>,
    ) -> ConcurrentEditResult {
        let values = match Arc::try_unwrap(self.values) {
            Ok(lock) => lock.into_inner().unwrap_or_else(|e| {
                tracing::warn!("values RwLock poisoned (into_inner), recovering: {e}");
                e.into_inner()
            }),
            Err(arc) => arc
                .read()
                .unwrap_or_else(|e| {
                    tracing::warn!("values RwLock poisoned (shared fallback), recovering: {e}");
                    e.into_inner()
                })
                .clone(),
        };
        let snapshot_values = match Arc::try_unwrap(self.snapshot_values) {
            Ok(lock) => lock.into_inner().unwrap_or_else(|e| {
                tracing::warn!("snapshot_values RwLock poisoned (into_inner), recovering: {e}");
                e.into_inner()
            }),
            Err(arc) => arc
                .read()
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        "snapshot_values RwLock poisoned (shared fallback), recovering: {e}"
                    );
                    e.into_inner()
                })
                .clone(),
        };
        let node_results = match Arc::try_unwrap(self.results) {
            Ok(lock) => lock.into_inner().unwrap_or_else(|e| {
                tracing::warn!("results Mutex poisoned (into_inner), recovering: {e}");
                e.into_inner()
            }),
            Err(arc) => arc
                .lock()
                .unwrap_or_else(|e| {
                    tracing::warn!("results Mutex poisoned (shared fallback), recovering: {e}");
                    e.into_inner()
                })
                .clone(),
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

// Test-only helpers that poison specific locks for recovery testing.
// Gated behind cfg(test) for unit tests and feature = "test-utils" for integration tests.
#[cfg(any(test, feature = "test-utils"))]
impl ConcurrentEvalAdapter {
    /// Poison the `results` Mutex.
    pub fn poison_results(&self) {
        let arc = Arc::clone(&self.results);
        std::thread::spawn(move || {
            let _guard = arc.lock().unwrap();
            panic!("intentional panic to poison results");
        })
        .join()
        .ok();
    }

    /// Poison the `values` RwLock.
    pub fn poison_values(&self) {
        let arc = Arc::clone(&self.values);
        std::thread::spawn(move || {
            let _guard = arc.write().unwrap();
            panic!("intentional panic to poison values");
        })
        .join()
        .ok();
    }

    /// Poison the `snapshot_values` RwLock.
    pub fn poison_snapshot_values(&self) {
        let arc = Arc::clone(&self.snapshot_values);
        std::thread::spawn(move || {
            let _guard = arc.write().unwrap();
            panic!("intentional panic to poison snapshot_values");
        })
        .join()
        .ok();
    }

    /// Return a clone of the `values` Arc so tests can hold a second reference,
    /// forcing `Arc::try_unwrap` to fail (Err branch) in `into_result()`.
    pub fn clone_values_arc(&self) -> Arc<std::sync::RwLock<ValueMap>> {
        Arc::clone(&self.values)
    }

    /// Return a clone of the `snapshot_values` Arc so tests can hold a second
    /// reference, forcing `Arc::try_unwrap` to fail (Err branch) in `into_result()`.
    pub fn clone_snapshot_values_arc(
        &self,
    ) -> Arc<RwLock<PersistentMap<ValueCellId, (Value, DeterminacyState)>>> {
        Arc::clone(&self.snapshot_values)
    }

    /// Return a clone of the `results` Arc so tests can hold a second reference,
    /// forcing `Arc::try_unwrap` to fail (Err branch) in `into_result()`.
    pub fn clone_results_arc(&self) -> Arc<Mutex<Vec<ConcurrentNodeResult>>> {
        Arc::clone(&self.results)
    }
}

impl AsyncNodeEvaluator for ConcurrentEvalAdapter {
    async fn evaluate(&self, node: NodeId) -> EvalOutcome {
        // Only evaluate Value nodes with expressions
        if let NodeId::Value(ref vcid) = node
            && let Some(cell_node) = self.graph.value_cells.get(vcid)
            && (cell_node.kind == ValueCellKind::Let || cell_node.kind == ValueCellKind::Auto)
            && cell_node.default_expr.is_some()
        {
            let expr = cell_node.default_expr.as_ref().unwrap();

            // Read current values (brief read lock)
            let current_values = { self.read_values().clone() };

            // Evaluate expression (pure, no lock held)
            let val = reify_expr::eval_expr(
                expr,
                &reify_expr::EvalContext::new(&current_values, &self.functions)
                    .with_meta(&self.meta_map),
            );

            // Compute dependency trace
            let trace = extract_dependency_trace(expr);

            // Compute content hash for early cutoff
            let cached_result = CachedResult::Value(val.clone(), DeterminacyState::Determined);
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
                self.write_values().insert(vcid.clone(), val.clone());
            }

            // Write to snapshot values (brief write lock)
            {
                self.write_snapshot_values()
                    .insert(vcid.clone(), (val.clone(), DeterminacyState::Determined));
            }

            // Record result (no early cutoff propagation — skip decisions
            // are made by the scheduler using pre-computed changed_vcids)
            self.lock_results().push(ConcurrentNodeResult {
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
/// 3. Run through `ConcurrentScheduler::execute()` with pre-computed skip logic
/// 4. Collect results into `ConcurrentEditResult` using scheduler's skipped set
///
/// After this returns, the caller should call `engine.apply_concurrent_edit()`
/// to merge the results back into the Engine.
pub async fn edit_param_concurrent(
    engine: &mut reify_eval::Engine,
    cell: ValueCellId,
    new_value: Value,
    cancel: &CancellationToken,
) -> Result<(ConcurrentEditSetup, ConcurrentEditResult), SchedulerError> {
    let setup = engine.prepare_concurrent_edit(cell, new_value)?;
    let eval_set = setup.eval_set.clone();
    let traces = setup.traces.clone();

    let adapter = ConcurrentEvalAdapter::from_setup(&setup);
    let adapter_arc = Arc::new(adapter);

    let scheduler = ConcurrentScheduler;
    match scheduler
        .execute(
            eval_set.clone(),
            Arc::clone(&adapter_arc),
            &traces,
            cancel,
            &setup.changed_cells,
        )
        .await
    {
        Ok(scheduler_result) => {
            // Extract result from adapter, passing the scheduler's skipped set.
            // After scheduler completes, the only remaining Arc reference should
            // be ours — but if a spawned task retained a clone, fall back to
            // building the result via shared access.
            let mut result = match Arc::try_unwrap(adapter_arc) {
                Ok(adapter) => adapter.into_result(&eval_set, scheduler_result.skipped),
                Err(arc) => arc.build_result_shared(&eval_set, scheduler_result.skipped),
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
        engine.check_constraints_with_values(&values)?;
    diagnostics.extend(constraint_diagnostics);

    Ok(CheckResult {
        values,
        constraint_results,
        diagnostics,
        resolved_params,
    })
}
