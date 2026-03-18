//! Concurrent evaluation scheduler and async runtime support.
//!
//! Provides `ConcurrentScheduler` which groups eval_set nodes by topological
//! level and spawns all nodes within a level concurrently using tokio tasks.

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
}
