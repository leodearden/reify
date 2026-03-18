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
use reify_eval::{ConcurrentEditResult, ConcurrentEditSetup, ConcurrentNodeResult};
use reify_types::{
    ContentHash, DeterminacyState, PersistentMap, Value, ValueCellId, ValueMap, VersionId,
};

use crate::concurrent::{AsyncNodeEvaluator, CancellationToken, ConcurrentScheduler, SchedulerError};

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
    /// Nodes to skip due to early cutoff of upstream nodes.
    skipped: Arc<RwLock<HashSet<NodeId>>>,
    /// Collected evaluation results.
    results: Arc<Mutex<Vec<ConcurrentNodeResult>>>,
    /// Version for this evaluation.
    #[allow(dead_code)]
    version: VersionId,
}

impl ConcurrentEvalAdapter {
    /// Create an adapter from a `ConcurrentEditSetup`.
    pub fn from_setup(setup: &ConcurrentEditSetup) -> Self {
        Self {
            graph: Arc::new(setup.graph.clone()),
            values: Arc::new(RwLock::new(setup.values.clone())),
            snapshot_values: Arc::new(RwLock::new(setup.snapshot_values.clone())),
            previous_hashes: Arc::new(setup.previous_hashes.clone()),
            reverse_index: Arc::new(setup.reverse_index.clone()),
            skipped: Arc::new(RwLock::new(HashSet::new())),
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
        self.skipped.read().unwrap().clone()
    }

    /// Consume the adapter and produce a `ConcurrentEditResult`.
    ///
    /// Extracts the final values, snapshot_values, results, and skipped set.
    pub fn into_result(self, eval_set: &[NodeId]) -> ConcurrentEditResult {
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
        let skipped = match Arc::try_unwrap(self.skipped) {
            Ok(lock) => lock.into_inner().unwrap(),
            Err(arc) => arc.read().unwrap().clone(),
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
        }
    }
}

impl AsyncNodeEvaluator for ConcurrentEvalAdapter {
    fn is_dirty(&self, node: &NodeId) -> bool {
        !self.skipped.read().unwrap().contains(node)
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

            // Early cutoff: if unchanged, mark dependents for skipping
            if outcome == EvalOutcome::Unchanged {
                let dependents = self.reverse_index.dependents_of(vcid);
                if !dependents.is_empty() {
                    let mut skipped = self.skipped.write().unwrap();
                    for dep in dependents {
                        skipped.insert(dep.clone());
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
            // remaining Arc reference should be ours.
            let adapter = match Arc::try_unwrap(adapter_arc) {
                Ok(a) => a,
                Err(_) => panic!("scheduler should have released all Arc references"),
            };
            let result = adapter.into_result(&eval_set);
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
