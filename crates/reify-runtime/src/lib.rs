// Stub — M1 implementation pending

use reify_types::ValueCellId;

/// Task scheduling priority.
///
/// Variants are ordered from highest priority (P0Interactive) to lowest
/// (P3Speculative). Derived Ord respects declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// User-interactive: must respond within a frame budget.
    P0Interactive,
    /// Fast background: lightweight computations (expression eval).
    P1Fast,
    /// Slow background: heavier computations (constraint solving).
    P1Slow,
    /// Speculative: pre-computation that may be discarded.
    P3Speculative,
}

/// A unit of work for the evaluation scheduler.
///
/// Each task targets a single value cell and carries a scheduling priority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// The value cell to evaluate.
    pub node_id: ValueCellId,
    /// Scheduling priority for this task.
    pub priority: Priority,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_creation() {
        let id = reify_types::ValueCellId::new("Bracket", "width");
        let task = Task {
            node_id: id.clone(),
            priority: Priority::P1Fast,
        };
        assert_eq!(task.node_id, id);
        assert_eq!(task.priority, Priority::P1Fast);

        // Verify Clone and Debug
        let task2 = task.clone();
        assert_eq!(task, task2);
        let _ = format!("{:?}", task);
    }

    #[test]
    fn test_tasks_sort_by_priority() {
        let tasks = vec![
            Task {
                node_id: reify_types::ValueCellId::new("B", "z"),
                priority: Priority::P3Speculative,
            },
            Task {
                node_id: reify_types::ValueCellId::new("B", "y"),
                priority: Priority::P0Interactive,
            },
            Task {
                node_id: reify_types::ValueCellId::new("B", "x"),
                priority: Priority::P1Slow,
            },
            Task {
                node_id: reify_types::ValueCellId::new("B", "w"),
                priority: Priority::P1Fast,
            },
        ];

        let mut sorted = tasks;
        sorted.sort_by_key(|t| t.priority);

        assert_eq!(sorted[0].priority, Priority::P0Interactive);
        assert_eq!(sorted[1].priority, Priority::P1Fast);
        assert_eq!(sorted[2].priority, Priority::P1Slow);
        assert_eq!(sorted[3].priority, Priority::P3Speculative);
    }

    #[test]
    fn test_priority_ordering() {
        assert!(Priority::P0Interactive < Priority::P1Fast);
        assert!(Priority::P1Fast < Priority::P1Slow);
        assert!(Priority::P1Slow < Priority::P3Speculative);

        // Verify equality
        assert_eq!(Priority::P0Interactive, Priority::P0Interactive);

        // Verify Copy
        let p = Priority::P1Fast;
        let p2 = p;
        assert_eq!(p, p2);
    }

    #[test]
    fn scheduler_empty_eval_set() {
        use reify_eval::cache::{EvalOutcome, NodeId};

        struct NoopEvaluator;
        impl NodeEvaluator for NoopEvaluator {
            fn is_dirty(&self, _node: &NodeId) -> bool {
                false
            }
            fn evaluate(&mut self, _node: &NodeId) -> EvalOutcome {
                EvalOutcome::Unchanged
            }
        }

        let scheduler = SequentialScheduler;
        let mut evaluator = NoopEvaluator;
        let eval_set = vec![];
        let changed = scheduler.execute(eval_set, &mut evaluator);
        assert!(changed.is_empty());
    }

    #[test]
    fn scheduler_single_changed_node() {
        use reify_eval::cache::{EvalOutcome, NodeId};

        struct AlwaysChanged;
        impl NodeEvaluator for AlwaysChanged {
            fn is_dirty(&self, _node: &NodeId) -> bool {
                true
            }
            fn evaluate(&mut self, _node: &NodeId) -> EvalOutcome {
                EvalOutcome::Changed
            }
        }

        let scheduler = SequentialScheduler;
        let mut evaluator = AlwaysChanged;
        let node = NodeId::Value(reify_types::ValueCellId::new("A", "x"));
        let eval_set = vec![node.clone()];
        let changed = scheduler.execute(eval_set, &mut evaluator);
        assert_eq!(changed.len(), 1);
        assert!(changed.contains(&node));
    }
}
