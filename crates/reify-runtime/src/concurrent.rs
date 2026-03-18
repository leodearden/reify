//! Concurrent evaluation scheduler and async runtime support.
//!
//! Provides `ConcurrentScheduler` which groups eval_set nodes by topological
//! level and spawns all nodes within a level concurrently using tokio tasks.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::sync::Arc;

use reify_eval::cache::{EvalOutcome, NodeId};
use reify_eval::deps::DependencyTrace;
use reify_types::ValueCellId;

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
        }
    }
}

impl std::error::Error for SchedulerError {}

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
pub trait AsyncNodeEvaluator: Send + Sync {
    /// Check if a node is still dirty (may have been cleared by upstream early cutoff).
    fn is_dirty(&self, node: &NodeId) -> bool;

    /// Evaluate a node asynchronously and return whether its result changed.
    fn evaluate(&self, node: NodeId) -> impl Future<Output = EvalOutcome> + Send;
}

/// Concurrent scheduler: groups eval_set nodes by topological level and
/// spawns all nodes within a level concurrently using tokio tasks.
pub struct ConcurrentScheduler;

impl ConcurrentScheduler {
    /// Execute the eval set concurrently, grouped by topological level.
    ///
    /// For each level:
    /// - Check cancellation before starting the level
    /// - Skip nodes that are no longer dirty
    /// - Spawn tokio tasks for dirty nodes
    /// - Join all tasks and collect changed nodes
    pub async fn execute<E: AsyncNodeEvaluator + 'static>(
        &self,
        eval_set: Vec<NodeId>,
        evaluator: Arc<E>,
        traces: &HashMap<NodeId, DependencyTrace>,
        cancel: &CancellationToken,
    ) -> Result<HashSet<NodeId>, SchedulerError> {
        if eval_set.is_empty() {
            return Ok(HashSet::new());
        }

        let levels = compute_levels(&eval_set, traces);
        let mut changed = HashSet::new();

        for level in levels {
            // Check cancellation before starting each level
            if cancel.is_cancelled() {
                break;
            }

            // Spawn tasks for dirty nodes in this level
            let mut handles = Vec::new();
            for node in level {
                if !evaluator.is_dirty(&node) {
                    continue;
                }

                let eval = Arc::clone(&evaluator);
                let n = node.clone();
                let handle = tokio::spawn(async move {
                    let outcome = eval.evaluate(n.clone()).await;
                    (n, outcome)
                });
                handles.push(handle);
            }

            // Join all tasks in this level
            for handle in handles {
                match handle.await {
                    Ok((node, EvalOutcome::Changed)) => {
                        changed.insert(node);
                    }
                    Ok(_) => {} // Unchanged — skip
                    Err(e) if e.is_panic() => {
                        return Err(SchedulerError::TaskPanicked(e.into_panic()));
                    }
                    Err(_) => {
                        return Err(SchedulerError::TaskCancelled);
                    }
                }
            }
        }

        Ok(changed)
    }
}

/// Compute topological levels from an eval_set using Kahn's algorithm.
///
/// Each level contains nodes with no dependencies on other nodes in later levels.
/// Nodes within a level have no dependencies on each other and can safely execute
/// concurrently. Mirrors the algorithm in `dirty.rs::topological_sort` but outputs
/// `Vec<Vec<NodeId>>` (batched by level) instead of flat `Vec<NodeId>`.
fn compute_levels(
    eval_set: &[NodeId],
    traces: &HashMap<NodeId, DependencyTrace>,
) -> Vec<Vec<NodeId>> {
    if eval_set.is_empty() {
        return Vec::new();
    }

    let node_set: HashSet<NodeId> = eval_set.iter().cloned().collect();

    // Build in-degree map (only counting edges within the node set)
    let mut in_degree: HashMap<NodeId, usize> =
        node_set.iter().map(|n| (n.clone(), 0)).collect();

    for node in &node_set {
        if let Some(trace) = traces.get(node) {
            // Deduplicate reads to avoid over-counting in-degree
            let unique_deps: HashSet<&ValueCellId> = trace.reads.iter().collect();
            for dep_cell in unique_deps {
                let dep_node = NodeId::Value(dep_cell.clone());
                if node_set.contains(&dep_node) {
                    *in_degree.get_mut(node).unwrap() += 1;
                }
            }
        }
    }

    // Use BTreeSet with Debug repr for deterministic tie-breaking
    let mut ready: BTreeSet<DebugOrd> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(n, _)| DebugOrd(n.clone()))
        .collect();

    let mut levels = Vec::new();

    while !ready.is_empty() {
        // All nodes currently ready form one level
        let current_level: Vec<NodeId> =
            ready.iter().map(|d| d.0.clone()).collect();
        ready.clear();

        // Decrement in-degree for dependents of nodes in this level
        for node in &current_level {
            if let NodeId::Value(vcid) = node {
                for candidate in &node_set {
                    if let Some(trace) = traces.get(candidate)
                        && trace.reads.contains(vcid)
                    {
                        let deg = in_degree.get_mut(candidate).unwrap();
                        *deg -= 1;
                        if *deg == 0 {
                            ready.insert(DebugOrd(candidate.clone()));
                        }
                    }
                }
            }
        }

        levels.push(current_level);
    }

    levels
}

/// Wrapper for NodeId that implements Ord based on Debug representation.
/// Used for deterministic tie-breaking in topological level computation.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DebugOrd(NodeId);

impl PartialOrd for DebugOrd {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DebugOrd {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        format!("{:?}", self.0).cmp(&format!("{:?}", other.0))
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
            all_dirty: bool,
            result: EvalOutcome,
        }

        impl AsyncNodeEvaluator for MockAsyncEvaluator {
            fn is_dirty(&self, _node: &NodeId) -> bool {
                self.all_dirty
            }

            async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
                self.result
            }
        }

        let mock = MockAsyncEvaluator {
            all_dirty: true,
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
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

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
            fn is_dirty(&self, _node: &NodeId) -> bool {
                true
            }

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

        let scheduler = ConcurrentScheduler;
        let eval_set = vec![a.clone(), b.clone()];
        let changed = scheduler
            .execute(eval_set, evaluator.clone(), &traces, &cancel)
            .await
            .unwrap();

        // a should have been evaluated
        assert!(changed.contains(&a));
        // b should NOT have been evaluated (cancelled between levels)
        assert!(!changed.contains(&b));
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

        let eval_set = vec![
            width.clone(),
            thickness.clone(),
            volume.clone(),
            c1.clone(),
        ];

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
            fn is_dirty(&self, _node: &NodeId) -> bool {
                true
            }
            async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
                EvalOutcome::Changed
            }
        }

        let con_scheduler = ConcurrentScheduler;
        let con_evaluator = Arc::new(AllChangedAsync);
        let cancel = CancellationToken::new();
        let con_changed = con_scheduler
            .execute(eval_set, con_evaluator, &traces, &cancel)
            .await
            .unwrap();

        // Both should produce the same changed set
        assert_eq!(seq_changed, con_changed);
        assert_eq!(con_changed.len(), 4);
        assert!(con_changed.contains(&width));
        assert!(con_changed.contains(&thickness));
        assert!(con_changed.contains(&volume));
        assert!(con_changed.contains(&c1));
    }

    #[tokio::test]
    async fn concurrent_scheduler_skips_non_dirty() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use std::collections::HashMap;
        use std::sync::Arc;

        let dirty_node = NodeId::Value(reify_types::ValueCellId::new("A", "a"));
        let clean_node = NodeId::Value(reify_types::ValueCellId::new("A", "b"));

        struct SelectiveDirty {
            dirty_node: NodeId,
        }

        impl AsyncNodeEvaluator for SelectiveDirty {
            fn is_dirty(&self, node: &NodeId) -> bool {
                *node == self.dirty_node
            }

            async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
                EvalOutcome::Changed
            }
        }

        let evaluator = Arc::new(SelectiveDirty {
            dirty_node: dirty_node.clone(),
        });
        let eval_set = vec![dirty_node.clone(), clean_node.clone()];
        let mut traces = HashMap::new();
        traces.insert(dirty_node.clone(), DependencyTrace::default());
        traces.insert(clean_node.clone(), DependencyTrace::default());
        let cancel = CancellationToken::new();

        let scheduler = ConcurrentScheduler;
        let changed = scheduler
            .execute(eval_set, evaluator, &traces, &cancel)
            .await
            .unwrap();

        assert_eq!(changed.len(), 1);
        assert!(changed.contains(&dirty_node));
        assert!(!changed.contains(&clean_node));
    }

    #[tokio::test]
    async fn concurrent_scheduler_multi_level_ordering() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        /// Tracks evaluation order via a shared vec.
        struct TrackingAsyncEvaluator {
            eval_order: Arc<Mutex<Vec<NodeId>>>,
        }

        impl AsyncNodeEvaluator for TrackingAsyncEvaluator {
            fn is_dirty(&self, _node: &NodeId) -> bool {
                true
            }

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
        let a = NodeId::Value(reify_types::ValueCellId::new(e, "a"));
        let b = NodeId::Value(reify_types::ValueCellId::new(e, "b"));
        let c = NodeId::Value(reify_types::ValueCellId::new(e, "c"));

        let eval_set = vec![a.clone(), b.clone(), c.clone()];

        let mut traces = HashMap::new();
        traces.insert(a.clone(), DependencyTrace::default());
        traces.insert(b.clone(), DependencyTrace::default());
        // c reads a and b
        traces.insert(
            c.clone(),
            DependencyTrace {
                reads: vec![
                    reify_types::ValueCellId::new(e, "a"),
                    reify_types::ValueCellId::new(e, "b"),
                ],
            },
        );

        let cancel = CancellationToken::new();
        let scheduler = ConcurrentScheduler;
        let changed = scheduler
            .execute(eval_set, evaluator, &traces, &cancel)
            .await
            .unwrap();

        // All 3 nodes should be in the changed set
        assert_eq!(changed.len(), 3);
        assert!(changed.contains(&a));
        assert!(changed.contains(&b));
        assert!(changed.contains(&c));

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
        use std::collections::HashMap;
        use std::sync::Arc;

        struct AllDirtyChanged;

        impl AsyncNodeEvaluator for AllDirtyChanged {
            fn is_dirty(&self, _node: &NodeId) -> bool {
                true
            }

            async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
                EvalOutcome::Changed
            }
        }

        let scheduler = ConcurrentScheduler;
        let evaluator = Arc::new(AllDirtyChanged);
        let node = NodeId::Value(reify_types::ValueCellId::new("A", "x"));
        let eval_set = vec![node.clone()];
        let mut traces = HashMap::new();
        traces.insert(node.clone(), DependencyTrace::default());
        let cancel = CancellationToken::new();

        let changed = scheduler.execute(eval_set, evaluator, &traces, &cancel).await.unwrap();
        assert_eq!(changed.len(), 1);
        assert!(changed.contains(&node));
    }

    #[tokio::test]
    async fn execute_returns_error_on_task_panic() {
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::deps::DependencyTrace;
        use std::collections::HashMap;
        use std::sync::Arc;

        struct PanickingAsyncEvaluator;

        impl AsyncNodeEvaluator for PanickingAsyncEvaluator {
            fn is_dirty(&self, _node: &NodeId) -> bool {
                true
            }

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

        let result = scheduler
            .execute(eval_set, evaluator, &traces, &cancel)
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
        use std::collections::HashMap;
        use std::sync::Arc;

        struct PanickingWithPayload;

        impl AsyncNodeEvaluator for PanickingWithPayload {
            fn is_dirty(&self, _node: &NodeId) -> bool {
                true
            }

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

        let result = scheduler
            .execute(eval_set, evaluator, &traces, &cancel)
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
        use std::collections::HashMap;
        use std::sync::Arc;

        struct MockAsyncEvaluator;

        impl AsyncNodeEvaluator for MockAsyncEvaluator {
            fn is_dirty(&self, _node: &NodeId) -> bool {
                true
            }

            async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
                EvalOutcome::Changed
            }
        }

        let scheduler = ConcurrentScheduler;
        let evaluator = Arc::new(MockAsyncEvaluator);
        let traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        let cancel = CancellationToken::new();
        let eval_set = vec![];

        let changed = scheduler.execute(eval_set, evaluator, &traces, &cancel).await.unwrap();
        assert!(changed.is_empty());
    }
}
