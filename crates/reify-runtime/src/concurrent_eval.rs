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
//! **Lock poisoning recovery contract:** The public operations `values()`,
//! `snapshot_values()`, `take_results()`, `build_result_shared()`, `into_result()`,
//! and the `AsyncNodeEvaluator::evaluate()` implementation all guarantee that they
//! will not panic when an internal lock is poisoned by a prior evaluation task that
//! panicked while holding it. Each recovery emits a structured `tracing::warn!`
//! (see maintainer note below) so that cascading panics in a concurrent batch can
//! be detected and diagnosed. Recovered data may reflect a partially-completed write
//! from the panicking task. Without this graceful recovery, one panicking task would
//! cascade to every concurrent task sharing the adapter via `Arc`, taking down the
//! entire evaluation batch instead of just the faulting node.
//!
//! **Maintainer note:** When adding new public methods, route lock acquisitions through the private
//! helper family — `read_values()`, `write_values()`, `read_snapshot_values()`,
//! `write_snapshot_values()`, and `lock_results()` — which encapsulate the `unwrap_or_else` +
//! `tracing::warn!` + `into_inner()` recovery pattern. The exception is methods that consume
//! `self` (such as `into_result()`), which must use the inline `Arc::try_unwrap` +
//! `into_inner()` pattern instead because the `&self` helpers cannot be called after consuming
//! the receiver; see `into_result()`'s doc comment for the full rationale.
//!
//! Each recovery warning is emitted with structured fields whose values are defined
//! as `pub const &str` items in the [`poison_fields`] submodule — see that module
//! for the authoritative schema. New helpers MUST use those constants so that
//! Datadog/Jaeger filter rules stay in sync with the emitted field values.
//! In brief: `lock` names the affected lock, `access` or `path` describes how it was
//! acquired (helper methods use `access`, `into_result()` inline sites use `path`),
//! and `error = %e` carries the poison error. The message is always the value of
//! [`poison_fields::MSG_LOCK_POISONED`].

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

use reify_compiler::ValueCellKind;
use reify_eval::cache::{CachedResult, EvalOutcome, NodeId};
use reify_eval::deps::extract_dependency_trace;
use reify_eval::graph::EvaluationGraph;
use reify_eval::{CheckResult, ConcurrentEditResult, ConcurrentEditSetup, ConcurrentNodeResult};
use reify_types::{
    CompiledFunction, ContentHash, DeterminacyState, PersistentMap, Value, ValueCellId, ValueMap,
};

use crate::concurrent::{
    AsyncNodeEvaluator, CancellationToken, ConcurrentScheduler, SchedulerError,
};

/// Compile-time constants for the structured poison-recovery warning schema.
///
/// This module is the **single source of truth** for every field value emitted
/// by the `tracing::warn!` calls in `ConcurrentEvalAdapter`'s lock-recovery
/// paths, as established by Task 600.
///
/// Each constant's string value is a Datadog/Jaeger filter key that operators
/// use in alerting rules. **Any rename is a breaking change to alerting rules**
/// and must be coordinated with the observability team.
///
/// New helpers that acquire poisonable locks MUST use these constants so that
/// filter rules stay in sync with the emitted field values.
pub mod poison_fields {
    /// The `lock` field value for the `values` RwLock.
    pub const LOCK_VALUES: &str = "values";
    /// The `lock` field value for the `snapshot_values` RwLock.
    pub const LOCK_SNAPSHOT_VALUES: &str = "snapshot_values";
    /// The `lock` field value for the `results` Mutex.
    pub const LOCK_RESULTS: &str = "results";
    /// The `access` field value for read-lock acquisitions in helper methods.
    pub const ACCESS_READ: &str = "read";
    /// The `access` field value for write-lock acquisitions in helper methods.
    pub const ACCESS_WRITE: &str = "write";
    /// The `access` field value for exclusive (Mutex) lock acquisitions in helper methods.
    pub const ACCESS_EXCLUSIVE: &str = "exclusive";
    /// The `path` field value for recovery via `into_inner()` (sole-owner unwrap).
    pub const PATH_INTO_INNER: &str = "into_inner";
    /// The `path` field value for recovery via the shared-reference fallback.
    pub const PATH_SHARED_FALLBACK: &str = "shared_fallback";
    /// The fixed message emitted by every poison-recovery warning.
    pub const MSG_LOCK_POISONED: &str = "lock poisoned, recovering";
}

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
/// This adapter upholds the module-level poison-recovery contract: all public operations
/// remain panic-safe against prior task panics, so a single faulting node cannot take
/// down the entire evaluation batch sharing this adapter via `Arc`.
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
    /// Shares the same Arc as ConcurrentEditSetup::functions (task #1997).
    functions: Arc<[CompiledFunction]>,
    /// Template-to-meta-entries mapping for resolving MetaAccess expressions.
    meta_map: Arc<HashMap<String, HashMap<String, String>>>,
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
            functions: Arc::clone(&setup.functions),
            meta_map: Arc::clone(&setup.meta_map),
        }
    }

    /// Acquire a read lock on `values`, recovering from poison with a warning.
    fn read_values(&self) -> RwLockReadGuard<'_, ValueMap> {
        self.values.read().unwrap_or_else(|e| {
            tracing::warn!(lock = poison_fields::LOCK_VALUES, access = poison_fields::ACCESS_READ, error = %e, "{}", poison_fields::MSG_LOCK_POISONED);
            e.into_inner()
        })
    }

    /// Acquire a write lock on `values`, recovering from poison with a warning.
    fn write_values(&self) -> RwLockWriteGuard<'_, ValueMap> {
        self.values.write().unwrap_or_else(|e| {
            tracing::warn!(lock = poison_fields::LOCK_VALUES, access = poison_fields::ACCESS_WRITE, error = %e, "{}", poison_fields::MSG_LOCK_POISONED);
            e.into_inner()
        })
    }

    /// Acquire a read lock on `snapshot_values`, recovering from poison with a warning.
    fn read_snapshot_values(
        &self,
    ) -> RwLockReadGuard<'_, PersistentMap<ValueCellId, (Value, DeterminacyState)>> {
        self.snapshot_values.read().unwrap_or_else(|e| {
            tracing::warn!(lock = poison_fields::LOCK_SNAPSHOT_VALUES, access = poison_fields::ACCESS_READ, error = %e, "{}", poison_fields::MSG_LOCK_POISONED);
            e.into_inner()
        })
    }

    /// Acquire a write lock on `snapshot_values`, recovering from poison with a warning.
    fn write_snapshot_values(
        &self,
    ) -> RwLockWriteGuard<'_, PersistentMap<ValueCellId, (Value, DeterminacyState)>> {
        self.snapshot_values.write().unwrap_or_else(|e| {
            tracing::warn!(lock = poison_fields::LOCK_SNAPSHOT_VALUES, access = poison_fields::ACCESS_WRITE, error = %e, "{}", poison_fields::MSG_LOCK_POISONED);
            e.into_inner()
        })
    }

    /// Acquire a lock on `results`, recovering from poison with a warning.
    fn lock_results(&self) -> MutexGuard<'_, Vec<ConcurrentNodeResult>> {
        self.results.lock().unwrap_or_else(|e| {
            tracing::warn!(lock = poison_fields::LOCK_RESULTS, access = poison_fields::ACCESS_EXCLUSIVE, error = %e, "{}", poison_fields::MSG_LOCK_POISONED);
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
    pub fn snapshot_values(&self) -> PersistentMap<ValueCellId, (Value, DeterminacyState)> {
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
                tracing::warn!(lock = poison_fields::LOCK_VALUES, path = poison_fields::PATH_INTO_INNER, error = %e, "{}", poison_fields::MSG_LOCK_POISONED);
                e.into_inner()
            }),
            Err(arc) => arc
                .read()
                .unwrap_or_else(|e| {
                    tracing::warn!(lock = poison_fields::LOCK_VALUES, path = poison_fields::PATH_SHARED_FALLBACK, error = %e, "{}", poison_fields::MSG_LOCK_POISONED);
                    e.into_inner()
                })
                .clone(),
        };
        let snapshot_values = match Arc::try_unwrap(self.snapshot_values) {
            Ok(lock) => lock.into_inner().unwrap_or_else(|e| {
                tracing::warn!(lock = poison_fields::LOCK_SNAPSHOT_VALUES, path = poison_fields::PATH_INTO_INNER, error = %e, "{}", poison_fields::MSG_LOCK_POISONED);
                e.into_inner()
            }),
            Err(arc) => arc
                .read()
                .unwrap_or_else(|e| {
                    tracing::warn!(lock = poison_fields::LOCK_SNAPSHOT_VALUES, path = poison_fields::PATH_SHARED_FALLBACK, error = %e, "{}", poison_fields::MSG_LOCK_POISONED);
                    e.into_inner()
                })
                .clone(),
        };
        let node_results = match Arc::try_unwrap(self.results) {
            Ok(lock) => lock.into_inner().unwrap_or_else(|e| {
                tracing::warn!(lock = poison_fields::LOCK_RESULTS, path = poison_fields::PATH_INTO_INNER, error = %e, "{}", poison_fields::MSG_LOCK_POISONED);
                e.into_inner()
            }),
            Err(arc) => arc
                .lock()
                .unwrap_or_else(|e| {
                    tracing::warn!(lock = poison_fields::LOCK_RESULTS, path = poison_fields::PATH_SHARED_FALLBACK, error = %e, "{}", poison_fields::MSG_LOCK_POISONED);
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

/// Generates a `pub fn $fn_name(&self) -> $ret_ty` method that returns
/// `Arc::clone(&self.$field)`.  Intended for use inside the test-utils impl
/// block of [`ConcurrentEvalAdapter`] to eliminate repetitive Arc-accessor
/// boilerplate.
#[cfg(any(test, feature = "test-utils"))]
macro_rules! arc_accessor {
    ($(#[$meta:meta])* $fn_name:ident => $field:ident : $ret_ty:ty) => {
        $(#[$meta])*
        pub fn $fn_name(&self) -> $ret_ty {
            Arc::clone(&self.$field)
        }
    };
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

    arc_accessor!(
        /// Return a second Arc owner for `values`, preventing `into_result()` from
        /// taking exclusive ownership via `try_unwrap`. Intended for tests that need
        /// to exercise the shared-reference fallback path.
        values_arc => values : Arc<std::sync::RwLock<ValueMap>>
    );

    arc_accessor!(
        /// Return a second Arc owner for `snapshot_values`, preventing `into_result()`
        /// from taking exclusive ownership via `try_unwrap`. Intended for tests that
        /// need to exercise the shared-reference fallback path.
        snapshot_values_arc => snapshot_values : Arc<RwLock<PersistentMap<ValueCellId, (Value, DeterminacyState)>>>
    );

    arc_accessor!(
        /// Return a second Arc owner for `results`, preventing `into_result()` from
        /// taking exclusive ownership via `try_unwrap`. Intended for tests that need
        /// to exercise the shared-reference fallback path.
        results_arc => results : Arc<Mutex<Vec<ConcurrentNodeResult>>>
    );
}

impl AsyncNodeEvaluator for ConcurrentEvalAdapter {
    async fn evaluate(&self, node: NodeId) -> EvalOutcome {
        // Only evaluate Value nodes with expressions
        if let NodeId::Value(ref vcid) = node
            && let Some(cell_node) = self.graph.value_cells.get(vcid)
            && (cell_node.kind == ValueCellKind::Let || cell_node.kind.is_auto())
            && let Some(expr) = cell_node.default_expr.as_ref()
        {

            // Read current values (brief read lock)
            let current_values = { self.read_values().clone() };

            // Evaluate expression (pure, no lock held) — capture wall-clock time.
            let eval_start = std::time::Instant::now();
            let val = reify_expr::eval_expr(
                expr,
                &reify_expr::EvalContext::new(&current_values, &self.functions)
                    .with_meta(&self.meta_map),
            );
            let eval_duration = eval_start.elapsed();

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
                eval_duration: Some(eval_duration),
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
            // Populates result.resolved_params and result.diagnostics in-place.
            engine.resolve_concurrent_edit(&setup, &mut result);

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
    // Invariant maintained by `edit_param_concurrent`: `result.values` and
    // `result.snapshot_values` carry the same set of ValueCellId keys and the
    // same `Value` payloads for every key. The two fields differ only in that
    // `snapshot_values` additionally carries `DeterminacyState` per entry —
    // they are populated together by the concurrent evaluator. This lets us
    // (1) clone just `result.values` (a `ValueMap`) as the input to
    // `check_constraints_with_values` below, and (2) move `result` into
    // `apply_concurrent_edit` to commit `snapshot_values` as the engine's new
    // snapshot — with the guarantee that the constraint check sees the same
    // post-edit values that the engine will commit. If `edit_param_concurrent`
    // ever diverges the two maps, the constraint results here will be
    // inconsistent with the committed engine state.
    debug_assert!(
        result.values.len() == result.snapshot_values.len()
            && result.values.iter().all(|(id, val)| {
                result
                    .snapshot_values
                    .get(id)
                    .is_some_and(|(sv, _)| sv == val)
            }),
        "edit_param_concurrent invariant violated: values and snapshot_values have inconsistent keys or values"
    );
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
