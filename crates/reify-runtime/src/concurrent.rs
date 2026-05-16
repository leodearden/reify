//! Concurrent evaluation scheduler and async runtime support.
//!
//! Provides `ConcurrentScheduler` which groups eval_set nodes by topological
//! level and spawns all nodes within a level concurrently using tokio tasks.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{Arc, Mutex};

use reify_eval::cache::{CacheStore, EvalOutcome, NodeId};
use reify_eval::deps::DependencyTrace;
use reify_eval::gating;
use reify_types::{NodeTraitsMap, ValueCellId, WarmStartableRegistry};

use crate::Priority;
use crate::commitment::{CommitmentTracker, NodeCommitmentOverride, NodePolicyOverrides};
use crate::priority_promotion::SharedPriorityPromoter;
use crate::warm_startable_assert::assert_warm_startable_coextensive;

/// Configuration for [`ConcurrentScheduler::execute_with_config`].
///
/// Controls priority-based ordering, commitment tracking, and per-node
/// behavior overrides during concurrent evaluation. `Default` gives
/// exact current `execute` behavior (no priority sorting, no commitment
/// tracking, no skip overrides).
///
/// Fields:
/// - `commitment_tracker`: optional commitment-aware cancellation tracker.
/// - `priority_promoter`: optional priority-based spawn ordering.
/// - `node_overrides`: per-node commitment behavior overrides.
/// - `node_priorities`: per-node scheduling priorities.
/// - `cache`: optional cache reference for `OnlyRunOnFinalInputs` gating.
/// - `node_traits`: per-NodeId / per-kind trait override map with kind-derived
///   default fallback (per PRD §5 B1 / arch §7.6). Default-empty — every
///   `resolve` call returns the §7.6 architecture default until a downstream
///   task (γ/δ/η/θ) wires consumer dispatch on it.
/// - `warm_startable_registry`: optional bidirectional coextension fixture
///   (per PRD §5 B5 / I-3, M-013 fix). When `Some(_)`,
///   [`execute_with_config`](ConcurrentScheduler::execute_with_config) calls
///   [`assert_warm_startable_coextensive`] after the empty-eval-set
///   short-circuit to pin `kind.default_traits().contains(WARM_STARTABLE) ↔
///   registry.contains_kind(kind)` for every variant in `NodeKind::ALL`.
///   The assertion uses `debug_assert_eq!`, so release builds compile to a
///   no-op even when `Some`. `Default` is `None` to preserve every existing
///   scheduler test — production binaries opt in once a Resolution-side
///   `WarmStartable` producer lands (see PRD §5 B5).
pub struct SchedulerConfig<'a> {
    /// Optional commitment tracker for commitment-aware cancellation.
    pub commitment_tracker: Option<Arc<Mutex<CommitmentTracker>>>,
    /// Optional priority promoter for priority-based spawn ordering.
    pub priority_promoter: Option<Arc<SharedPriorityPromoter>>,
    /// Per-node commitment behavior overrides.
    pub node_overrides: NodePolicyOverrides,
    /// Per-node scheduling priorities.
    pub node_priorities: HashMap<NodeId, Priority>,
    /// Cache reference used by `gating::has_non_final_inputs` to decide
    /// whether `OnlyRunOnFinalInputs` nodes are runnable. `None` means no
    /// gating — equivalent to treating every node as having all-Final inputs.
    pub cache: Option<&'a CacheStore>,
    /// Per-NodeId / per-kind trait override map with kind-derived default
    /// fallback (per PRD §5 B1 / arch §7.6). Default-empty — every
    /// `resolve` call returns the §7.6 architecture default until a
    /// downstream task (γ/δ/η/θ) wires consumer dispatch on it.
    pub node_traits: NodeTraitsMap<NodeId>,
    /// Optional fixture for the PRD §5 B5 / I-3 bidirectional coextension
    /// assertion (M-013 fix). When `Some(_)`, the scheduler invokes
    /// [`assert_warm_startable_coextensive`] at execute init (after the
    /// empty-eval-set short-circuit). `None` preserves prior behaviour for
    /// every existing test.
    pub warm_startable_registry: Option<WarmStartableRegistry>,
}

impl Default for SchedulerConfig<'_> {
    fn default() -> Self {
        Self {
            commitment_tracker: None,
            priority_promoter: None,
            node_overrides: NodePolicyOverrides::new(),
            node_priorities: HashMap::new(),
            cache: None,
            node_traits: NodeTraitsMap::default(),
            warm_startable_registry: None,
        }
    }
}

/// Error type for concurrent scheduler execution.
///
/// Surfaces spawned task failures to callers rather than silently discarding them.
#[derive(Debug)]
pub enum SchedulerError {
    /// A spawned evaluation task panicked. Contains the raw panic payload
    /// (`Box<dyn Any + Send>`) so callers can downcast to the original type
    /// (&str, String, custom error types) for programmatic handling.
    TaskPanicked(Box<dyn std::any::Any + Send>),
    /// A spawned evaluation task was cancelled by the tokio runtime.
    TaskCancelled,
    /// The engine was not initialized (eval() not called before this operation).
    EngineNotInitialized(reify_eval::EngineError),
}

impl std::fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchedulerError::TaskPanicked(payload) => {
                if let Some(s) = payload.downcast_ref::<&str>() {
                    write!(f, "evaluation task panicked: {s}")
                } else if let Some(s) = payload.downcast_ref::<String>() {
                    write!(f, "evaluation task panicked: {s}")
                } else {
                    write!(f, "evaluation task panicked (unknown payload type)")
                }
            }
            SchedulerError::TaskCancelled => {
                write!(f, "evaluation task was cancelled")
            }
            SchedulerError::EngineNotInitialized(e) => {
                write!(f, "engine not initialized: {e}")
            }
        }
    }
}

impl std::error::Error for SchedulerError {}

impl From<reify_eval::EngineError> for SchedulerError {
    fn from(e: reify_eval::EngineError) -> Self {
        SchedulerError::EngineNotInitialized(e)
    }
}

/// A cancellation token for cooperative cancellation of async tasks.
///
/// Thin wrapper around `tokio_util::sync::CancellationToken` providing a
/// project-specific API surface that decouples from the transitive dependency.
#[derive(Clone)]
pub struct CancellationToken {
    inner: tokio_util::sync::CancellationToken,
}

impl CancellationToken {
    /// Create a new cancellation token.
    pub fn new() -> Self {
        Self {
            inner: tokio_util::sync::CancellationToken::new(),
        }
    }

    /// Signal cancellation.
    pub fn cancel(&self) {
        self.inner.cancel();
    }

    /// Check if cancellation has been signalled.
    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    /// Create a child token that is cancelled when this token is cancelled.
    pub fn child_token(&self) -> Self {
        Self {
            inner: self.inner.child_token(),
        }
    }

    /// Wait until cancellation is signalled.
    pub async fn cancelled(&self) {
        self.inner.cancelled().await;
    }
}

/// Trait for evaluating individual nodes asynchronously during concurrent scheduling.
///
/// Like [`crate::NodeEvaluator`] but takes `&self` (not `&mut self`) because concurrent
/// evaluation shares the evaluator across multiple spawned tasks via `Arc<E>`.
/// Implementors should use interior mutability (Mutex, RwLock) as needed.
///
/// `evaluate` takes `NodeId` by value (not by reference) to avoid lifetime issues
/// when moving data into spawned tasks.
///
/// Note: dirty/skip decisions are made by the scheduler using pre-computed
/// `changed_vcids` tracking, not by the evaluator. Evaluators are purely
/// computational — they only implement `evaluate()`.
pub trait AsyncNodeEvaluator: Send + Sync {
    /// Evaluate a node asynchronously and return whether its result changed.
    fn evaluate(&self, node: NodeId) -> impl Future<Output = EvalOutcome> + Send;
}

/// Result of concurrent scheduler execution.
///
/// Contains the set of nodes that were evaluated and returned `Changed`,
/// and the set of nodes that were skipped because none of their dependencies
/// were in the `changed_vcids` set.
#[derive(Debug)]
pub struct SchedulerResult {
    /// Nodes that were evaluated and returned `EvalOutcome::Changed`.
    pub changed: HashSet<NodeId>,
    /// Nodes that were skipped (not dirty per pre-computed changed_vcids).
    pub skipped: HashSet<NodeId>,
}

/// Concurrent scheduler: groups eval_set nodes by topological level and
/// spawns all nodes within a level concurrently using tokio tasks.
pub struct ConcurrentScheduler;

impl ConcurrentScheduler {
    /// Execute the eval set concurrently, grouped by topological level.
    ///
    /// Uses pre-computed skip logic: tracks `changed_vcids` seeded from
    /// `changed_cells`. Before each level, a node is dirty if any of its
    /// `trace.reads` intersects `changed_vcids` (or if traces are missing/empty
    /// as a safety default). After each level, Changed outcomes' ValueCellIds
    /// are added to `changed_vcids`. This makes the skip decision structurally
    /// race-free with zero Mutex contention during evaluation.
    ///
    /// For each level:
    /// - Check cancellation before starting the level
    /// - Pre-compute which nodes are dirty vs skippable
    /// - Spawn tokio tasks for dirty nodes
    /// - Join all tasks, collect changed/skipped sets
    /// - Update changed_vcids for next level
    pub async fn execute<E: AsyncNodeEvaluator + 'static>(
        &self,
        eval_set: Vec<NodeId>,
        evaluator: Arc<E>,
        traces: &HashMap<NodeId, DependencyTrace>,
        cancel: &CancellationToken,
        changed_cells: &HashSet<ValueCellId>,
    ) -> Result<SchedulerResult, SchedulerError> {
        self.execute_with_config(
            eval_set,
            evaluator,
            traces,
            cancel,
            changed_cells,
            SchedulerConfig::default(),
        )
        .await
    }

    /// Execute the eval set concurrently with additional configuration for
    /// priority ordering, commitment tracking, and per-node overrides.
    ///
    /// Extends [`execute`](Self::execute) with:
    /// - Priority-based spawn ordering within each level
    /// - Commitment-aware cancellation (committed results survive cancel)
    /// - `OnlyRunOnFinalInputs` skip logic
    /// - Registration/cleanup lifecycle for tracker and promoter
    pub async fn execute_with_config<E: AsyncNodeEvaluator + 'static>(
        &self,
        eval_set: Vec<NodeId>,
        evaluator: Arc<E>,
        traces: &HashMap<NodeId, DependencyTrace>,
        cancel: &CancellationToken,
        changed_cells: &HashSet<ValueCellId>,
        config: SchedulerConfig<'_>,
    ) -> Result<SchedulerResult, SchedulerError> {
        if eval_set.is_empty() {
            return Ok(SchedulerResult {
                changed: HashSet::new(),
                skipped: HashSet::new(),
            });
        }

        // PRD §5 B5 / I-3 (M-013 fix): when a `WarmStartableRegistry` fixture
        // is attached to the config, fire the bidirectional coextension
        // assertion before any spawn. The assertion body is `debug_assert_eq!`
        // so release builds compile to a no-op even on the `Some(_)` arm.
        if let Some(registry) = &config.warm_startable_registry {
            assert_warm_startable_coextensive(registry);
        }

        // Single helper that calls gating::has_non_final_inputs when a cache
        // reference is present; returns false otherwise (no-op default).
        let has_non_final = |node: &NodeId| -> bool {
            config
                .cache
                .is_some_and(|cs| gating::has_non_final_inputs(cs, node))
        };

        // Per-dirty-node metadata bundle. Fields are named so every downstream
        // loop reads `dn.id`, `dn.override_`, or `dn.has_non_final` rather than
        // positional `(_, _, _)` destructuring.
        struct DirtyNode {
            id: NodeId,
            override_: NodeCommitmentOverride,
            has_non_final: bool,
        }

        let node_set: HashSet<NodeId> = eval_set.into_iter().collect();
        let levels = reify_eval::dirty::compute_levels(&node_set, traces);
        let mut changed = HashSet::new();
        let mut skipped = HashSet::new();
        let mut changed_vcids: HashSet<ValueCellId> = changed_cells.clone();

        for level in levels {
            // Check cancellation before starting each level
            if cancel.is_cancelled() {
                break;
            }

            // Pre-compute dirty/skip for this level.
            // Each dirty node carries its pre-computed NodeCommitmentOverride and the
            // result of `has_non_final(&node)` so both are looked up exactly once (here)
            // and threaded through to downstream loops — preventing silent divergence
            // and eliminating the redundant second call in the spawn loop.
            let mut dirty_nodes: Vec<DirtyNode> = Vec::new();
            for node in level {
                let is_dirty = if let Some(trace) = traces.get(&node) {
                    if trace.reads.is_empty() {
                        // No reads (e.g. param node) — treat as dirty (safety default)
                        true
                    } else {
                        // Dirty if any read intersects changed_vcids
                        trace.reads.iter().any(|r| changed_vcids.contains(r))
                    }
                } else {
                    // No trace entry — safety default: treat as dirty
                    true
                };

                if is_dirty {
                    // Compute override and has_non_final flag once per dirty node.
                    // Both are stored in DirtyNode so every downstream loop reads
                    // named fields — no second closure invocation.
                    let override_ = config.node_overrides.resolve(&node);
                    let has_non_final_flag = has_non_final(&node);
                    // Check OnlyRunOnFinalInputs override before adding to dirty
                    if override_ == NodeCommitmentOverride::OnlyRunOnFinalInputs
                        && has_non_final_flag
                    {
                        skipped.insert(node);
                    } else {
                        dirty_nodes.push(DirtyNode {
                            id: node,
                            override_,
                            has_non_final: has_non_final_flag,
                        });
                    }
                } else {
                    skipped.insert(node);
                }
            }

            // Register dirty nodes in priority promoter and sort by priority
            if let Some(ref promoter) = config.priority_promoter {
                for dn in &dirty_nodes {
                    let priority = config
                        .node_priorities
                        .get(&dn.id)
                        .copied()
                        .unwrap_or(Priority::P3Speculative);
                    promoter.register(dn.id.clone(), priority);
                }
                // Sort by config priority: ascending (P0 < P3 in Ord).
                // At this point, nodes were just registered (lines above) with
                // values from config.node_priorities and no promotions have
                // occurred yet, so effective_priority == config priority.
                // Sorting directly from config avoids O(N log N) mutex
                // acquisitions through SharedPriorityPromoter.
                dirty_nodes.sort_by_key(|dn| {
                    config
                        .node_priorities
                        .get(&dn.id)
                        .copied()
                        .unwrap_or(Priority::P3Speculative)
                });
            }

            // Register dirty nodes in commitment tracker before spawning,
            // using the pre-computed override from the dirty/skip block above.
            if let Some(ref tracker) = config.commitment_tracker {
                let mut guard = tracker.lock().unwrap_or_else(|e| e.into_inner());
                for dn in &dirty_nodes {
                    guard.register_task(dn.id.clone(), dn.override_);
                }
            }

            // Spawn tasks for dirty nodes in this level using JoinSet.
            // JoinSet aborts all remaining tasks on drop (unlike Vec<JoinHandle>
            // which detaches them), and abort_all() is called explicitly on error
            // paths for clarity.
            // Returns (NodeId, Option<EvalOutcome>) where None = commitment-cancelled
            let mut join_set = tokio::task::JoinSet::new();
            for dn in &dirty_nodes {
                let eval = Arc::clone(&evaluator);
                let n = dn.id.clone();
                let cancel_clone = cancel.clone();
                let tracker_clone = config.commitment_tracker.clone();
                let has_intermediate = dn.has_non_final;
                join_set.spawn(async move {
                    let start = std::time::Instant::now();
                    let outcome = eval.evaluate(n.clone()).await;
                    // Commitment check: if cancel fired and tracker is present
                    if cancel_clone.is_cancelled()
                        && let Some(ref tracker) = tracker_clone
                    {
                        let elapsed = start.elapsed();
                        let progress = crate::commitment::TaskProgress {
                            elapsed,
                            reported_progress: None,
                            previous_runtime: None,
                        };
                        let mut guard = tracker.lock().unwrap_or_else(|e| e.into_inner());
                        guard.update_status(&n, &progress, has_intermediate);
                        if !guard.should_continue(&n, true) {
                            // Uncommitted in dirty cone — drop result
                            return (n, None);
                        }
                    }
                    (n, Some(outcome))
                });
            }

            // Cleanup closure: remove all dirty nodes from tracker and promoter.
            // Called on both normal completion and error paths to prevent stale entries.
            let cleanup_level = |dirty: &[DirtyNode]| {
                if let Some(ref tracker) = config.commitment_tracker {
                    let mut guard = tracker.lock().unwrap_or_else(|e| e.into_inner());
                    for dn in dirty {
                        guard.remove_task(&dn.id);
                    }
                }
                if let Some(ref promoter) = config.priority_promoter {
                    let node_ids: Vec<NodeId> = dirty.iter().map(|dn| dn.id.clone()).collect();
                    promoter.batch_remove(&node_ids);
                }
            };

            // Join all tasks in this level (completion order — safe because
            // results are collected into HashSets, so ordering is irrelevant)
            while let Some(result) = join_set.join_next().await {
                match result {
                    Ok((node, Some(EvalOutcome::Changed))) => {
                        // Add to changed_vcids for downstream dirty computation
                        if let NodeId::Value(ref vcid) = node {
                            changed_vcids.insert(vcid.clone());
                        }
                        changed.insert(node);
                    }
                    Ok((_, Some(EvalOutcome::Unchanged))) => {} // Unchanged — skip
                    Ok((_, None)) => {}                         // Commitment-cancelled — drop
                    Err(e) if e.is_panic() => {
                        join_set.abort_all();
                        cleanup_level(&dirty_nodes);
                        return Err(SchedulerError::TaskPanicked(e.into_panic()));
                    }
                    Err(_) => {
                        join_set.abort_all();
                        cleanup_level(&dirty_nodes);
                        return Err(SchedulerError::TaskCancelled);
                    }
                }
            }

            // Normal completion: cleanup all processed nodes
            cleanup_level(&dirty_nodes);
        }

        Ok(SchedulerResult { changed, skipped })
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_token_create_and_cancel() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn async_node_evaluator_mock_compiles() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_types::ValueCellId;

        struct MockAsyncEvaluator {
            result: EvalOutcome,
        }

        impl AsyncNodeEvaluator for MockAsyncEvaluator {
            async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
                self.result
            }
        }

        let mock = MockAsyncEvaluator {
            result: EvalOutcome::Changed,
        };

        let node = NodeId::Value(ValueCellId::new("A", "x"));
        let outcome = mock.evaluate(node).await;
        assert_eq!(outcome, EvalOutcome::Changed);
    }

    #[test]
    fn cancellation_token_child_follows_parent() {
        let parent = CancellationToken::new();
        let child = parent.child_token();
        assert!(!child.is_cancelled());
        parent.cancel();
        assert!(child.is_cancelled());
    }

    #[tokio::test]
    async fn cancellation_stops_evaluation() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use reify_types::ValueCellId;
        use std::collections::HashMap;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let e = "C";
        let a = NodeId::Value(ValueCellId::new(e, "a"));
        let b = NodeId::Value(ValueCellId::new(e, "b"));

        // b depends on a → a at level 0, b at level 1
        let mut traces = HashMap::new();
        traces.insert(a.clone(), DependencyTrace::default());
        traces.insert(
            b.clone(),
            DependencyTrace {
                reads: vec![ValueCellId::new(e, "a")],
            },
        );

        let cancel = CancellationToken::new();

        struct CancellingAsyncEvaluator {
            cancel: CancellationToken,
            eval_count: AtomicUsize,
        }

        impl AsyncNodeEvaluator for CancellingAsyncEvaluator {
            async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
                self.eval_count.fetch_add(1, Ordering::SeqCst);
                // Cancel after evaluating (node a triggers cancellation)
                self.cancel.cancel();
                EvalOutcome::Changed
            }
        }

        let evaluator = Arc::new(CancellingAsyncEvaluator {
            cancel: cancel.clone(),
            eval_count: AtomicUsize::new(0),
        });

        // Both a and b read nothing special — use changed_cells containing a's vcid
        // so all nodes with reads containing "a" are dirty
        let mut changed_cells = HashSet::new();
        changed_cells.insert(ValueCellId::new(e, "a"));

        let scheduler = ConcurrentScheduler;
        let eval_set = vec![a.clone(), b.clone()];
        let result = scheduler
            .execute(
                eval_set,
                evaluator.clone(),
                &traces,
                &cancel,
                &changed_cells,
            )
            .await
            .unwrap();

        // a should have been evaluated (it has empty reads → dirty by default)
        assert!(result.changed.contains(&a));
        // b should NOT have been evaluated (cancelled between levels)
        assert!(!result.changed.contains(&b));
        // Only 1 evaluation should have happened
        assert_eq!(evaluator.eval_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn concurrent_matches_sequential_bracket_topology() {
        use crate::{NodeEvaluator, SequentialScheduler};
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use reify_types::{ConstraintNodeId, ValueCellId};
        use std::collections::HashMap;
        use std::sync::Arc;

        let e = "B";
        let width = NodeId::Value(ValueCellId::new(e, "width"));
        let thickness = NodeId::Value(ValueCellId::new(e, "thickness"));
        let volume = NodeId::Value(ValueCellId::new(e, "volume"));
        let c1 = NodeId::Constraint(ConstraintNodeId::new(e, 1));

        let eval_set = vec![width.clone(), thickness.clone(), volume.clone(), c1.clone()];

        let mut traces = HashMap::new();
        traces.insert(width.clone(), DependencyTrace::default());
        traces.insert(thickness.clone(), DependencyTrace::default());
        traces.insert(
            volume.clone(),
            DependencyTrace {
                reads: vec![
                    ValueCellId::new(e, "width"),
                    ValueCellId::new(e, "thickness"),
                ],
            },
        );
        traces.insert(
            c1.clone(),
            DependencyTrace {
                reads: vec![
                    ValueCellId::new(e, "width"),
                    ValueCellId::new(e, "thickness"),
                ],
            },
        );

        // Sequential scheduler
        struct AllChanged;
        impl NodeEvaluator for AllChanged {
            fn is_dirty(&self, _node: &NodeId) -> bool {
                true
            }
            fn evaluate(&mut self, _node: &NodeId) -> EvalOutcome {
                EvalOutcome::Changed
            }
        }

        let seq_scheduler = SequentialScheduler;
        let mut seq_evaluator = AllChanged;
        let seq_changed = seq_scheduler.execute(eval_set.clone(), &mut seq_evaluator);

        // Concurrent scheduler
        struct AllChangedAsync;
        impl AsyncNodeEvaluator for AllChangedAsync {
            async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
                EvalOutcome::Changed
            }
        }

        let con_scheduler = ConcurrentScheduler;
        let con_evaluator = Arc::new(AllChangedAsync);
        let cancel = CancellationToken::new();
        // Use changed_cells containing both width and thickness so all nodes
        // that read them are considered dirty
        let mut changed_cells = HashSet::new();
        changed_cells.insert(ValueCellId::new(e, "width"));
        changed_cells.insert(ValueCellId::new(e, "thickness"));
        let con_result = con_scheduler
            .execute(eval_set, con_evaluator, &traces, &cancel, &changed_cells)
            .await
            .unwrap();

        // Both should produce the same changed set
        assert_eq!(seq_changed, con_result.changed);
        assert_eq!(con_result.changed.len(), 4);
        assert!(con_result.changed.contains(&width));
        assert!(con_result.changed.contains(&thickness));
        assert!(con_result.changed.contains(&volume));
        assert!(con_result.changed.contains(&c1));
    }

    /// Tests skip via traces/changed_cells: node `a` reads the changed param
    /// and is dirty; node `b` reads a different (unchanged) value and is skipped.
    #[tokio::test]
    async fn concurrent_scheduler_skips_non_dirty() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use reify_types::ValueCellId;
        use std::collections::{HashMap, HashSet};
        use std::sync::Arc;

        let changed_param = ValueCellId::new("A", "param");
        let other_param = ValueCellId::new("A", "other");

        let dirty_node = NodeId::Value(ValueCellId::new("A", "a"));
        let clean_node = NodeId::Value(ValueCellId::new("A", "b"));

        struct AllChangedAsync;
        impl AsyncNodeEvaluator for AllChangedAsync {
            async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
                EvalOutcome::Changed
            }
        }

        let evaluator = Arc::new(AllChangedAsync);
        let eval_set = vec![dirty_node.clone(), clean_node.clone()];

        // dirty_node reads the changed param, clean_node reads something else
        let mut traces = HashMap::new();
        traces.insert(
            dirty_node.clone(),
            DependencyTrace {
                reads: vec![changed_param.clone()],
            },
        );
        traces.insert(
            clean_node.clone(),
            DependencyTrace {
                reads: vec![other_param],
            },
        );

        let mut changed_cells = HashSet::new();
        changed_cells.insert(changed_param);

        let cancel = CancellationToken::new();
        let scheduler = ConcurrentScheduler;
        let result = scheduler
            .execute(eval_set, evaluator, &traces, &cancel, &changed_cells)
            .await
            .unwrap();

        assert_eq!(result.changed.len(), 1);
        assert!(result.changed.contains(&dirty_node));
        assert!(!result.changed.contains(&clean_node));
        // clean_node should be in skipped
        assert!(result.skipped.contains(&clean_node));
    }

    #[tokio::test]
    async fn concurrent_scheduler_multi_level_ordering() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use reify_types::ValueCellId;
        use std::collections::{HashMap, HashSet};
        use std::sync::{Arc, Mutex};

        /// Tracks evaluation order via a shared vec.
        struct TrackingAsyncEvaluator {
            eval_order: Arc<Mutex<Vec<NodeId>>>,
        }

        impl AsyncNodeEvaluator for TrackingAsyncEvaluator {
            async fn evaluate(&self, node: NodeId) -> EvalOutcome {
                self.eval_order.lock().unwrap().push(node);
                EvalOutcome::Changed
            }
        }

        let eval_order = Arc::new(Mutex::new(Vec::new()));
        let evaluator = Arc::new(TrackingAsyncEvaluator {
            eval_order: Arc::clone(&eval_order),
        });

        let e = "T";
        let a = NodeId::Value(ValueCellId::new(e, "a"));
        let b = NodeId::Value(ValueCellId::new(e, "b"));
        let c = NodeId::Value(ValueCellId::new(e, "c"));

        let eval_set = vec![a.clone(), b.clone(), c.clone()];

        let mut traces = HashMap::new();
        traces.insert(a.clone(), DependencyTrace::default());
        traces.insert(b.clone(), DependencyTrace::default());
        // c reads a and b
        traces.insert(
            c.clone(),
            DependencyTrace {
                reads: vec![ValueCellId::new(e, "a"), ValueCellId::new(e, "b")],
            },
        );

        // a and b have empty reads → dirty by default.
        // c reads a and b → dirty once a and b are in changed_vcids (after level 0).
        let changed_cells = HashSet::new();

        let cancel = CancellationToken::new();
        let scheduler = ConcurrentScheduler;
        let result = scheduler
            .execute(eval_set, evaluator, &traces, &cancel, &changed_cells)
            .await
            .unwrap();

        // All 3 nodes should be in the changed set
        assert_eq!(result.changed.len(), 3);
        assert!(result.changed.contains(&a));
        assert!(result.changed.contains(&b));
        assert!(result.changed.contains(&c));

        // c must appear after both a and b in eval order
        let order = eval_order.lock().unwrap();
        let a_pos = order.iter().position(|n| *n == a).unwrap();
        let b_pos = order.iter().position(|n| *n == b).unwrap();
        let c_pos = order.iter().position(|n| *n == c).unwrap();
        assert!(c_pos > a_pos, "c should be evaluated after a");
        assert!(c_pos > b_pos, "c should be evaluated after b");
    }

    #[tokio::test]
    async fn concurrent_scheduler_single_dirty_node() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use std::collections::{HashMap, HashSet};
        use std::sync::Arc;

        struct AllChangedAsync;

        impl AsyncNodeEvaluator for AllChangedAsync {
            async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
                EvalOutcome::Changed
            }
        }

        let scheduler = ConcurrentScheduler;
        let evaluator = Arc::new(AllChangedAsync);
        let node = NodeId::Value(reify_types::ValueCellId::new("A", "x"));
        let eval_set = vec![node.clone()];
        let mut traces = HashMap::new();
        traces.insert(node.clone(), DependencyTrace::default());
        let cancel = CancellationToken::new();
        // Empty reads → dirty by default
        let changed_cells = HashSet::new();

        let result = scheduler
            .execute(eval_set, evaluator, &traces, &cancel, &changed_cells)
            .await
            .unwrap();
        assert_eq!(result.changed.len(), 1);
        assert!(result.changed.contains(&node));
    }

    #[tokio::test]
    async fn execute_returns_error_on_task_panic() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use std::collections::{HashMap, HashSet};
        use std::sync::Arc;

        struct PanickingAsyncEvaluator;

        impl AsyncNodeEvaluator for PanickingAsyncEvaluator {
            async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
                panic!("evaluator bug");
            }
        }

        let scheduler = ConcurrentScheduler;
        let evaluator = Arc::new(PanickingAsyncEvaluator);
        let node = NodeId::Value(reify_types::ValueCellId::new("P", "x"));
        let eval_set = vec![node.clone()];
        let mut traces = HashMap::new();
        traces.insert(node.clone(), DependencyTrace::default());
        let cancel = CancellationToken::new();
        let changed_cells = HashSet::new();

        let result = scheduler
            .execute(eval_set, evaluator, &traces, &cancel, &changed_cells)
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            SchedulerError::TaskPanicked(_) => {} // expected
            other => panic!("Expected TaskPanicked, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn execute_panic_preserves_payload() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use std::collections::{HashMap, HashSet};
        use std::sync::Arc;

        struct PanickingWithPayload;

        impl AsyncNodeEvaluator for PanickingWithPayload {
            async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
                panic!("kaboom from evaluator");
            }
        }

        let scheduler = ConcurrentScheduler;
        let evaluator = Arc::new(PanickingWithPayload);
        let node = NodeId::Value(reify_types::ValueCellId::new("P", "y"));
        let eval_set = vec![node.clone()];
        let mut traces = HashMap::new();
        traces.insert(node.clone(), DependencyTrace::default());
        let cancel = CancellationToken::new();
        let changed_cells = HashSet::new();

        let result = scheduler
            .execute(eval_set, evaluator, &traces, &cancel, &changed_cells)
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            SchedulerError::TaskPanicked(payload) => {
                let msg = payload
                    .downcast_ref::<&str>()
                    .expect("panic payload should be &str");
                assert_eq!(*msg, "kaboom from evaluator");
            }
            other => panic!("Expected TaskPanicked, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn concurrent_scheduler_empty_eval_set() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use std::collections::{HashMap, HashSet};
        use std::sync::Arc;

        struct MockAsyncEvaluator;

        impl AsyncNodeEvaluator for MockAsyncEvaluator {
            async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
                EvalOutcome::Changed
            }
        }

        let scheduler = ConcurrentScheduler;
        let evaluator = Arc::new(MockAsyncEvaluator);
        let traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        let cancel = CancellationToken::new();
        let eval_set = vec![];
        let changed_cells = HashSet::new();

        let result = scheduler
            .execute(eval_set, evaluator, &traces, &cancel, &changed_cells)
            .await
            .unwrap();
        assert!(result.changed.is_empty());
        assert!(result.skipped.is_empty());
    }

    /// Priority sort from config: 3 nodes at level 0 with different priorities.
    /// Verifies nodes are spawned in priority order (P0 first, P3 last) when
    /// execute_with_config uses config.node_priorities for sorting.
    #[tokio::test]
    async fn priority_sort_from_config_orders_spawn_correctly() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use reify_types::ValueCellId;
        use std::collections::{HashMap, HashSet};
        use std::sync::{Arc, Mutex};

        /// Tracks evaluation order with a small delay to serialize spawn order.
        struct OrderTrackingEvaluator {
            eval_order: Arc<Mutex<Vec<NodeId>>>,
        }

        impl AsyncNodeEvaluator for OrderTrackingEvaluator {
            async fn evaluate(&self, node: NodeId) -> EvalOutcome {
                // Record the order immediately on spawn
                self.eval_order.lock().unwrap().push(node);
                EvalOutcome::Changed
            }
        }

        let eval_order = Arc::new(Mutex::new(Vec::new()));
        let evaluator = Arc::new(OrderTrackingEvaluator {
            eval_order: Arc::clone(&eval_order),
        });

        let e = "Prio";
        let a = NodeId::Value(ValueCellId::new(e, "a")); // will be P0
        let b = NodeId::Value(ValueCellId::new(e, "b")); // will be P1Slow
        let c = NodeId::Value(ValueCellId::new(e, "c")); // will be P3

        // All at level 0 (no reads → dirty by default)
        let mut traces = HashMap::new();
        traces.insert(a.clone(), DependencyTrace::default());
        traces.insert(b.clone(), DependencyTrace::default());
        traces.insert(c.clone(), DependencyTrace::default());

        // Configure priorities: a=P0, b=P1Slow, c=P3
        let mut node_priorities = HashMap::new();
        node_priorities.insert(a.clone(), Priority::P0Interactive);
        node_priorities.insert(b.clone(), Priority::P1Slow);
        node_priorities.insert(c.clone(), Priority::P3Speculative);

        let promoter = Arc::new(SharedPriorityPromoter::new());

        let config = SchedulerConfig {
            priority_promoter: Some(Arc::clone(&promoter)),
            node_priorities,
            ..SchedulerConfig::default()
        };

        let cancel = CancellationToken::new();
        let scheduler = ConcurrentScheduler;
        // Provide eval_set in reverse-priority order to prove sorting works
        let eval_set = vec![c.clone(), b.clone(), a.clone()];
        let changed_cells = HashSet::new();

        let result = scheduler
            .execute_with_config(
                eval_set,
                evaluator,
                &traces,
                &cancel,
                &changed_cells,
                config,
            )
            .await
            .unwrap();

        assert_eq!(result.changed.len(), 3);

        // Verify spawn order: a (P0) should be first in the sorted dirty_nodes
        let order = eval_order.lock().unwrap();
        let a_pos = order.iter().position(|n| *n == a).unwrap();
        let b_pos = order.iter().position(|n| *n == b).unwrap();
        let c_pos = order.iter().position(|n| *n == c).unwrap();
        assert!(
            a_pos < c_pos,
            "P0 node should be spawned before P3 node, but a_pos={a_pos} c_pos={c_pos}"
        );
        assert!(
            b_pos < c_pos,
            "P1Slow node should be spawned before P3 node, but b_pos={b_pos} c_pos={c_pos}"
        );
        assert!(
            a_pos < b_pos,
            "P0 node should be spawned before P1Slow node, but a_pos={a_pos} b_pos={b_pos}"
        );
    }

    /// Cleanup verification: after execute_with_config completes, the priority
    /// promoter should have count() == 0 — all registered nodes were cleaned up.
    /// This ensures the batch_remove cleanup path doesn't regress.
    #[tokio::test]
    async fn cleanup_removes_all_nodes_from_promoter() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use reify_types::ValueCellId;
        use std::collections::{HashMap, HashSet};
        use std::sync::Arc;

        struct AllChangedEval;

        impl AsyncNodeEvaluator for AllChangedEval {
            async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
                EvalOutcome::Changed
            }
        }

        let e = "CL";
        let a = NodeId::Value(ValueCellId::new(e, "a"));
        let b = NodeId::Value(ValueCellId::new(e, "b"));

        let mut traces = HashMap::new();
        traces.insert(a.clone(), DependencyTrace::default());
        traces.insert(b.clone(), DependencyTrace::default());

        let mut node_priorities = HashMap::new();
        node_priorities.insert(a.clone(), Priority::P1Fast);
        node_priorities.insert(b.clone(), Priority::P3Speculative);

        let promoter = Arc::new(SharedPriorityPromoter::new());

        let config = SchedulerConfig {
            priority_promoter: Some(Arc::clone(&promoter)),
            node_priorities,
            ..SchedulerConfig::default()
        };

        let cancel = CancellationToken::new();
        let scheduler = ConcurrentScheduler;
        let eval_set = vec![a.clone(), b.clone()];
        let changed_cells = HashSet::new();
        let evaluator = Arc::new(AllChangedEval);

        let result = scheduler
            .execute_with_config(
                eval_set,
                evaluator,
                &traces,
                &cancel,
                &changed_cells,
                config,
            )
            .await
            .unwrap();

        assert_eq!(result.changed.len(), 2);
        // All nodes should have been cleaned up from the promoter
        assert_eq!(
            promoter.count(),
            0,
            "promoter should have 0 tracked nodes after execution completes"
        );
    }

    /// Pre-computed skip: 3 parents (p1 Changed, p2/p3 Unchanged) fan into
    /// downstream d which reads ONLY p1/p2/p3 (not the changed param a).
    /// d must be evaluated (not skipped) because p1 is Changed.
    #[tokio::test]
    async fn scheduler_precomputed_skip_three_parent_mixed_fan_in() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use reify_types::ValueCellId;
        use std::collections::{HashMap, HashSet};
        use std::sync::Arc;

        let e = "M";
        let a = ValueCellId::new(e, "a");
        let p1_vcid = ValueCellId::new(e, "p1");
        let p2_vcid = ValueCellId::new(e, "p2");
        let p3_vcid = ValueCellId::new(e, "p3");
        let d_vcid = ValueCellId::new(e, "d");

        let p1 = NodeId::Value(p1_vcid.clone());
        let p2 = NodeId::Value(p2_vcid.clone());
        let p3 = NodeId::Value(p3_vcid.clone());
        let d = NodeId::Value(d_vcid.clone());

        // Traces: p1/p2/p3 read a (level 0), d reads p1+p2+p3 (level 1)
        let mut traces = HashMap::new();
        traces.insert(
            p1.clone(),
            DependencyTrace {
                reads: vec![a.clone()],
            },
        );
        traces.insert(
            p2.clone(),
            DependencyTrace {
                reads: vec![a.clone()],
            },
        );
        traces.insert(
            p3.clone(),
            DependencyTrace {
                reads: vec![a.clone()],
            },
        );
        traces.insert(
            d.clone(),
            DependencyTrace {
                reads: vec![p1_vcid.clone(), p2_vcid.clone(), p3_vcid.clone()],
            },
        );

        // Mock evaluator: p1→Changed, p2→Unchanged, p3→Unchanged, d→Changed
        let mut outcomes = HashMap::new();
        outcomes.insert(p1.clone(), EvalOutcome::Changed);
        outcomes.insert(p2.clone(), EvalOutcome::Unchanged);
        outcomes.insert(p3.clone(), EvalOutcome::Unchanged);
        outcomes.insert(d.clone(), EvalOutcome::Changed);

        struct MixedOutcomeEvaluator {
            outcomes: HashMap<NodeId, EvalOutcome>,
        }

        impl AsyncNodeEvaluator for MixedOutcomeEvaluator {
            async fn evaluate(&self, node: NodeId) -> EvalOutcome {
                self.outcomes
                    .get(&node)
                    .copied()
                    .unwrap_or(EvalOutcome::Unchanged)
            }
        }

        let evaluator = Arc::new(MixedOutcomeEvaluator { outcomes });

        let eval_set = vec![p1.clone(), p2.clone(), p3.clone(), d.clone()];
        let cancel = CancellationToken::new();

        let mut changed_cells = HashSet::new();
        changed_cells.insert(a);

        let scheduler = ConcurrentScheduler;
        let result = scheduler
            .execute(eval_set, evaluator, &traces, &cancel, &changed_cells)
            .await
            .unwrap();

        // d should be in changed (was evaluated, returned Changed)
        assert!(
            result.changed.contains(&d),
            "d should be in changed set: {:?}",
            result.changed
        );
        // d should NOT be in skipped
        assert!(
            !result.skipped.contains(&d),
            "d should NOT be in skipped set: {:?}",
            result.skipped
        );
    }

    /// All parents Unchanged → downstream d should be skipped.
    /// Topology: param a (changed_cells), p1 and p2 read a (level 0, both
    /// return Unchanged), d reads p1 and p2 (level 1).
    /// After level 0, changed_vcids stays {a}; d's reads [p1, p2] don't
    /// intersect {a}, so d is correctly skipped.
    #[tokio::test]
    async fn scheduler_precomputed_skip_all_unchanged_skips_downstream() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use reify_types::ValueCellId;
        use std::collections::{HashMap, HashSet};
        use std::sync::Arc;

        let e = "U";
        let a = ValueCellId::new(e, "a");
        let p1_vcid = ValueCellId::new(e, "p1");
        let p2_vcid = ValueCellId::new(e, "p2");
        let d_vcid = ValueCellId::new(e, "d");

        let p1 = NodeId::Value(p1_vcid.clone());
        let p2 = NodeId::Value(p2_vcid.clone());
        let d = NodeId::Value(d_vcid.clone());

        // Traces: p1/p2 read a (level 0), d reads p1+p2 (level 1)
        let mut traces = HashMap::new();
        traces.insert(
            p1.clone(),
            DependencyTrace {
                reads: vec![a.clone()],
            },
        );
        traces.insert(
            p2.clone(),
            DependencyTrace {
                reads: vec![a.clone()],
            },
        );
        traces.insert(
            d.clone(),
            DependencyTrace {
                reads: vec![p1_vcid.clone(), p2_vcid.clone()],
            },
        );

        // Both p1 and p2 return Unchanged
        let mut outcomes = HashMap::new();
        outcomes.insert(p1.clone(), EvalOutcome::Unchanged);
        outcomes.insert(p2.clone(), EvalOutcome::Unchanged);

        struct UnchangedEvaluator {
            outcomes: HashMap<NodeId, EvalOutcome>,
        }

        impl AsyncNodeEvaluator for UnchangedEvaluator {
            async fn evaluate(&self, node: NodeId) -> EvalOutcome {
                self.outcomes
                    .get(&node)
                    .copied()
                    .unwrap_or(EvalOutcome::Unchanged)
            }
        }

        let evaluator = Arc::new(UnchangedEvaluator { outcomes });
        let eval_set = vec![p1.clone(), p2.clone(), d.clone()];
        let cancel = CancellationToken::new();

        let mut changed_cells = HashSet::new();
        changed_cells.insert(a);

        let scheduler = ConcurrentScheduler;
        let result = scheduler
            .execute(eval_set, evaluator, &traces, &cancel, &changed_cells)
            .await
            .unwrap();

        // d should be in skipped (p1 and p2 both Unchanged, so changed_vcids
        // stays {a} and d's reads [p1, p2] don't intersect)
        assert!(
            result.skipped.contains(&d),
            "d should be in skipped set: {:?}",
            result.skipped
        );
        // d should NOT be in changed
        assert!(
            !result.changed.contains(&d),
            "d should NOT be in changed set: {:?}",
            result.changed
        );
    }

    /// changed_vcids grows correctly through 3 levels: a→b→c→d, all Changed.
    /// Verifies {a} → {a,b} → {a,b,c} propagation so d is dirty.
    #[tokio::test]
    async fn scheduler_changed_vcids_propagate_through_levels() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use reify_types::ValueCellId;
        use std::collections::{HashMap, HashSet};
        use std::sync::Arc;

        let e = "L";
        let a = ValueCellId::new(e, "a");
        let b_vcid = ValueCellId::new(e, "b");
        let c_vcid = ValueCellId::new(e, "c");
        let d_vcid = ValueCellId::new(e, "d");

        let b = NodeId::Value(b_vcid.clone());
        let c = NodeId::Value(c_vcid.clone());
        let d = NodeId::Value(d_vcid.clone());

        // b reads a (level 0), c reads b (level 1), d reads c (level 2)
        let mut traces = HashMap::new();
        traces.insert(
            b.clone(),
            DependencyTrace {
                reads: vec![a.clone()],
            },
        );
        traces.insert(
            c.clone(),
            DependencyTrace {
                reads: vec![b_vcid.clone()],
            },
        );
        traces.insert(
            d.clone(),
            DependencyTrace {
                reads: vec![c_vcid.clone()],
            },
        );

        // All return Changed
        struct AllChangedAsync;
        impl AsyncNodeEvaluator for AllChangedAsync {
            async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
                EvalOutcome::Changed
            }
        }

        let evaluator = Arc::new(AllChangedAsync);
        let eval_set = vec![b.clone(), c.clone(), d.clone()];
        let cancel = CancellationToken::new();

        let mut changed_cells = HashSet::new();
        changed_cells.insert(a);

        let scheduler = ConcurrentScheduler;
        let result = scheduler
            .execute(eval_set, evaluator, &traces, &cancel, &changed_cells)
            .await
            .unwrap();

        // All 3 should be in changed, none skipped
        assert!(result.changed.contains(&b), "b should be changed");
        assert!(result.changed.contains(&c), "c should be changed");
        assert!(result.changed.contains(&d), "d should be changed");
        assert!(
            result.skipped.is_empty(),
            "nothing should be skipped: {:?}",
            result.skipped
        );
    }

    /// Verify that reify_eval::dirty::compute_levels produces correct levels
    /// for the bracket topology: width+thickness at level 0, volume+C1 at level 1.
    #[test]
    fn compute_levels_bracket_topology() {
        use reify_eval::cache::NodeId;
        use reify_eval::deps::DependencyTrace;
        use reify_eval::dirty::compute_levels;
        use reify_types::{ConstraintNodeId, ValueCellId};
        use std::collections::{HashMap, HashSet};

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
                reads: vec![
                    ValueCellId::new(e, "width"),
                    ValueCellId::new(e, "thickness"),
                ],
            },
        );
        traces.insert(
            c1.clone(),
            DependencyTrace {
                reads: vec![
                    ValueCellId::new(e, "width"),
                    ValueCellId::new(e, "thickness"),
                ],
            },
        );

        let levels = compute_levels(&nodes, &traces);
        assert_eq!(levels.len(), 2, "expected 2 levels, got {:?}", levels);

        // Level 0: width + thickness (no in-set deps)
        assert_eq!(levels[0].len(), 2, "level 0: {:?}", levels[0]);
        assert!(levels[0].contains(&width));
        assert!(levels[0].contains(&thickness));

        // Level 1: volume + C1 (both depend on width + thickness)
        assert_eq!(levels[1].len(), 2, "level 1: {:?}", levels[1]);
        assert!(levels[1].contains(&volume));
        assert!(levels[1].contains(&c1));
    }

    /// Proves that error paths abort in-flight tasks rather than detaching them.
    ///
    /// Spawns 3 same-level nodes: one panics immediately (after yield_now),
    /// the other two sleep for 2 seconds then increment a shared AtomicUsize.
    /// After the scheduler returns Err(TaskPanicked), we wait 3 seconds and
    /// assert counter == 0. With Vec<JoinHandle> (drop = detach), the sleeping
    /// tasks continue in the background and the counter reaches 2 — test fails.
    /// With JoinSet + abort_all(), tasks are cancelled — test passes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn error_path_aborts_in_flight_tasks() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use reify_types::ValueCellId;
        use std::collections::{HashMap, HashSet};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let e = "Abort";
        // 3 nodes at the same level (all with empty reads → level 0)
        let panic_node = NodeId::Value(ValueCellId::new(e, "panic"));
        let slow_a = NodeId::Value(ValueCellId::new(e, "slow_a"));
        let slow_b = NodeId::Value(ValueCellId::new(e, "slow_b"));

        let mut traces = HashMap::new();
        traces.insert(panic_node.clone(), DependencyTrace::default());
        traces.insert(slow_a.clone(), DependencyTrace::default());
        traces.insert(slow_b.clone(), DependencyTrace::default());

        let counter = Arc::new(AtomicUsize::new(0));

        struct AbortTestEvaluator {
            counter: Arc<AtomicUsize>,
            panic_node: NodeId,
        }

        impl AsyncNodeEvaluator for AbortTestEvaluator {
            async fn evaluate(&self, node: NodeId) -> EvalOutcome {
                if node == self.panic_node {
                    // Yield once so the other tasks have a chance to be spawned
                    tokio::task::yield_now().await;
                    panic!("intentional panic for abort test");
                }
                // Slow tasks: yield in a loop for ~2 seconds then increment counter.
                // Each yield_now() is a cancellation point where abort can take effect.
                let start = std::time::Instant::now();
                while start.elapsed() < std::time::Duration::from_secs(2) {
                    tokio::task::yield_now().await;
                }
                self.counter.fetch_add(1, Ordering::SeqCst);
                EvalOutcome::Changed
            }
        }

        let evaluator = Arc::new(AbortTestEvaluator {
            counter: Arc::clone(&counter),
            panic_node: panic_node.clone(),
        });

        let eval_set = vec![panic_node.clone(), slow_a.clone(), slow_b.clone()];
        let cancel = CancellationToken::new();
        let changed_cells = HashSet::new();

        let scheduler = ConcurrentScheduler;
        let result = scheduler
            .execute(eval_set, evaluator, &traces, &cancel, &changed_cells)
            .await;

        // Should get TaskPanicked error
        assert!(result.is_err());
        match &result.unwrap_err() {
            SchedulerError::TaskPanicked(payload) => {
                let msg = payload
                    .downcast_ref::<&str>()
                    .expect("panic payload should be &str");
                assert_eq!(*msg, "intentional panic for abort test");
            }
            other => panic!("Expected TaskPanicked, got {:?}", other),
        }

        // Wait long enough for the slow tasks to complete if they were detached.
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // If tasks were properly aborted, counter should be 0.
        // If tasks were detached (Vec<JoinHandle> behavior), counter would be 2.
        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "In-flight tasks should have been aborted, not detached"
        );
    }
}
