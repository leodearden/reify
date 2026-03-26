//! Concurrent evaluation adapter bridging Engine state with the async scheduler.
//!
//! `ConcurrentEvalAdapter` implements `AsyncNodeEvaluator` using interior mutability
//! (std::sync::RwLock for values, Mutex for results) to enable safe concurrent
//! node evaluation across tokio tasks.
//!
//! Dirty/skip decisions are made by the scheduler using pre-computed `changed_vcids`
//! tracking, not by the adapter. The adapter is purely computational — it evaluates
//! expressions, computes content hashes, and records results.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

use reify_compiler::ValueCellKind;
use reify_eval::cache::{CachedResult, EvalOutcome, NodeId};
use reify_eval::deps::extract_dependency_trace;
use reify_eval::graph::EvaluationGraph;
use reify_eval::{CheckResult, ConcurrentEditResult, ConcurrentEditSetup, ConcurrentNodeResult, MetaMap};
use reify_types::{
    CompiledFunction, ContentHash, DeterminacyState, PersistentMap, Value, ValueCellId, ValueMap,
    VersionId,
};

use crate::concurrent::{AsyncNodeEvaluator, CancellationToken, ConcurrentScheduler, SchedulerError};

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
    meta_map: Arc<MetaMap>,
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

    /// Get a snapshot of the current values (for testing/inspection).
    pub fn values(&self) -> ValueMap {
        self.values.read().unwrap().clone()
    }

    /// Take the collected results (for testing/inspection).
    pub fn take_results(&self) -> Vec<ConcurrentNodeResult> {
        self.results.lock().unwrap().clone()
    }

    /// Build a `ConcurrentEditResult` via shared references (cloning).
    ///
    /// Used as a fallback when `Arc::try_unwrap` fails because outstanding
    /// references still exist. Slightly less efficient than `into_result`
    /// since it clones each inner container through locks.
    ///
    /// The `skipped` set is provided by the scheduler's `SchedulerResult`.
    pub fn build_result_shared(&self, eval_set: &[NodeId], skipped: HashSet<NodeId>) -> ConcurrentEditResult {
        let values = self.values.read().unwrap().clone();
        let snapshot_values = self.snapshot_values.read().unwrap().clone();
        let node_results = self.results.lock().unwrap().clone();

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
    /// The `skipped` set is provided by the scheduler's `SchedulerResult`.
    pub fn into_result(self, eval_set: &[NodeId], skipped: HashSet<NodeId>) -> ConcurrentEditResult {
        let values = match Arc::try_unwrap(self.values) {
            Ok(lock) => lock.into_inner().unwrap(),
            Err(arc) => arc.read().unwrap().clone(),
        };
        let snapshot_values = match Arc::try_unwrap(self.snapshot_values) {
            Ok(lock) => lock.into_inner().unwrap(),
            Err(arc) => arc.read().unwrap().clone(),
        };
        let node_results = match Arc::try_unwrap(self.results) {
            Ok(lock) => lock.into_inner().unwrap(),
            Err(arc) => arc.lock().unwrap().clone(),
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
}

impl AsyncNodeEvaluator for ConcurrentEvalAdapter {
    /// Evaluate a single node's expression against the current shared value state.
    ///
    /// # Async safety
    ///
    /// **Snapshot reads (F13):** Each node reads only its dependency values from
    /// the shared `values` map, producing a local snapshot that may not include
    /// results from concurrently-evaluated peers. This is safe because the
    /// [`ConcurrentScheduler`] enforces topological ordering — all of a node's
    /// predecessors complete (and write back) before that node is scheduled.
    /// The targeted-read approach (reading only `trace.reads` entries) makes
    /// this guarantee even clearer: we only read values we depend on, and
    /// those are guaranteed to be final.
    ///
    /// **Non-atomic writes (F14):** After evaluation, we perform three separate
    /// lock acquisitions: `values` write, `snapshot_values` write, and `results`
    /// lock. These are not atomic, so another task could theoretically read
    /// `values` between the first and second write. This is safe because:
    /// - No concurrent task depends on a node that hasn't completed `evaluate()`
    /// - The scheduler only releases dependents after `evaluate()` returns
    /// - Only the owning task writes to a given node's slot
    async fn evaluate(&self, node: NodeId) -> EvalOutcome {
        // Only evaluate Value nodes with expressions
        if let NodeId::Value(ref vcid) = node
            && let Some(cell_node) = self.graph.value_cells.get(vcid)
            && (cell_node.kind == ValueCellKind::Let || cell_node.kind == ValueCellKind::Auto)
            && cell_node.default_expr.is_some()
        {
            let expr = cell_node.default_expr.as_ref().unwrap();

            // Compute dependency trace to know which values the expression reads
            let trace = extract_dependency_trace(expr);

            // Read only the referenced values (brief read lock, O(k) where k = arity).
            // This replaces a full ValueMap clone (O(n)) that was the #1 performance
            // bottleneck: O(n) per node × n nodes = O(n²) total for the graph.
            let local_values = {
                let values = self.values.read().unwrap();
                let mut local = ValueMap::new();
                for dep_id in &trace.reads {
                    if let Some(v) = values.get(dep_id) {
                        local.insert(dep_id.clone(), v.clone());
                    }
                }
                // Also include the node's own current value (needed for self-referencing exprs)
                if let Some(v) = values.get(vcid) {
                    local.insert(vcid.clone(), v.clone());
                }
                local
            };

            // Evaluate expression (pure, no lock held)
            let val = reify_expr::eval_expr(expr, &reify_expr::EvalContext::new(&local_values, &self.functions).with_meta(&self.meta_map));

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

            // Record result (no early cutoff propagation — skip decisions
            // are made by the scheduler using pre-computed changed_vcids)
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

    let adapter = ConcurrentEvalAdapter::from_setup(&setup);
    let adapter_arc = Arc::new(adapter);

    let scheduler = ConcurrentScheduler;
    // Pass eval_set.clone() (consumed by execute) and borrow traces from setup directly,
    // avoiding the pre-clone of both fields.
    match scheduler
        .execute(setup.eval_set.clone(), Arc::clone(&adapter_arc), &setup.traces, cancel, &setup.changed_cells)
        .await
    {
        Ok(scheduler_result) => {
            // Extract result from adapter, passing the scheduler's skipped set.
            // After scheduler completes, the only remaining Arc reference should
            // be ours — but if a spawned task retained a clone, fall back to
            // building the result via shared access.
            let mut result = match Arc::try_unwrap(adapter_arc) {
                Ok(adapter) => adapter.into_result(&setup.eval_set, scheduler_result.skipped),
                Err(arc) => arc.build_result_shared(&setup.eval_set, scheduler_result.skipped),
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
    let (setup, mut result) = edit_param_concurrent(engine, cell, new_value, cancel).await?;

    // Extract fields not consumed by apply_concurrent_edit to avoid cloning.
    // Only resolved_params needs cloning (used by both apply and the caller).
    let resolved_params = result.resolved_params.clone();
    let mut diagnostics = std::mem::take(&mut result.diagnostics);
    let values = std::mem::take(&mut result.values);

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
