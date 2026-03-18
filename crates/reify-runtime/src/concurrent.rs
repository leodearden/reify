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
    ) -> HashSet<NodeId> {
        if eval_set.is_empty() {
            return HashSet::new();
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
                if let Ok((node, outcome)) = handle.await {
                    if outcome == EvalOutcome::Changed {
                        changed.insert(node);
                    }
                }
            }
        }

        changed
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

        let changed = scheduler.execute(eval_set, evaluator, &traces, &cancel).await;
        assert_eq!(changed.len(), 1);
        assert!(changed.contains(&node));
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

        let changed = scheduler.execute(eval_set, evaluator, &traces, &cancel).await;
        assert!(changed.is_empty());
    }
}
