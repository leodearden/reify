pub mod commitment;
pub mod concurrent;
pub mod concurrent_eval;
pub mod priority_promotion;
pub mod warm_startable_assert;

pub use warm_startable_assert::assert_warm_startable_coextensive;

use std::collections::HashSet;

use reify_eval::cache::{EvalOutcome, NodeId};
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

/// Trait for evaluating individual nodes during scheduling.
///
/// The scheduler doesn't need to know HOW nodes are evaluated — just
/// whether they're dirty and what the evaluation outcome is. The Engine
/// in reify-eval will implement this trait in a future integration step.
pub trait NodeEvaluator {
    /// Check if a node is still dirty (may have been cleared by upstream early cutoff).
    fn is_dirty(&self, node: &NodeId) -> bool;

    /// Evaluate a node and return whether its result changed.
    fn evaluate(&mut self, node: &NodeId) -> EvalOutcome;
}

/// Sequential scheduler: iterates eval_set in order, skips non-dirty nodes,
/// delegates evaluation to NodeEvaluator, collects changed nodes.
pub struct SequentialScheduler;

impl SequentialScheduler {
    /// Execute the eval set sequentially.
    ///
    /// For each node in the eval set:
    /// - Skip if no longer dirty (dynamic cutoff from upstream early cutoffs)
    /// - Call evaluator.evaluate() and collect nodes that returned Changed
    pub fn execute(
        &self,
        eval_set: Vec<NodeId>,
        evaluator: &mut dyn NodeEvaluator,
    ) -> HashSet<NodeId> {
        let mut changed = HashSet::new();

        for node in eval_set {
            if !evaluator.is_dirty(&node) {
                // Skip: upstream early cutoff cleared this node's dirty flag
                continue;
            }

            let outcome = evaluator.evaluate(&node);
            if outcome == EvalOutcome::Changed {
                changed.insert(node);
            }
        }

        changed
    }
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

    #[test]
    fn scheduler_dynamic_early_cutoff() {
        // eval_set = [A, B] where B depends on A.
        // A evaluates as Unchanged, then clears B's dirty flag internally.
        // Scheduler should skip B because is_dirty returns false.
        use reify_eval::cache::{EvalOutcome, NodeId};
        use std::cell::RefCell;

        struct CutoffEvaluator {
            // Track which node is "still dirty" — B starts dirty, gets cleared after A evaluates
            b_dirty: RefCell<bool>,
            a_node: NodeId,
        }

        impl NodeEvaluator for CutoffEvaluator {
            fn is_dirty(&self, node: &NodeId) -> bool {
                if *node == self.a_node {
                    true
                } else {
                    *self.b_dirty.borrow()
                }
            }
            fn evaluate(&mut self, node: &NodeId) -> EvalOutcome {
                if *node == self.a_node {
                    // A evaluates as Unchanged, clearing B's dirty flag
                    *self.b_dirty.borrow_mut() = false;
                    EvalOutcome::Unchanged
                } else {
                    panic!("B should not be evaluated");
                }
            }
        }

        let a = NodeId::Value(reify_types::ValueCellId::new("X", "a"));
        let b = NodeId::Constraint(reify_types::ConstraintNodeId::new("X", 0));

        let scheduler = SequentialScheduler;
        let mut evaluator = CutoffEvaluator {
            b_dirty: RefCell::new(true),
            a_node: a.clone(),
        };

        let eval_set = vec![a.clone(), b.clone()];
        let changed = scheduler.execute(eval_set, &mut evaluator);
        // A was Unchanged, B was skipped → no changes
        assert!(changed.is_empty(), "changed: {:?}", changed);
    }

    #[test]
    fn scheduler_all_nodes_changed() {
        // eval_set = [width, volume, C1], evaluator returns Changed for all
        use reify_eval::cache::{EvalOutcome, NodeId};

        struct AllChanged;
        impl NodeEvaluator for AllChanged {
            fn is_dirty(&self, _node: &NodeId) -> bool {
                true
            }
            fn evaluate(&mut self, _node: &NodeId) -> EvalOutcome {
                EvalOutcome::Changed
            }
        }

        let width = NodeId::Value(reify_types::ValueCellId::new("B", "width"));
        let volume = NodeId::Value(reify_types::ValueCellId::new("B", "volume"));
        let c1 = NodeId::Constraint(reify_types::ConstraintNodeId::new("B", 1));

        let scheduler = SequentialScheduler;
        let mut evaluator = AllChanged;
        let eval_set = vec![width.clone(), volume.clone(), c1.clone()];
        let changed = scheduler.execute(eval_set, &mut evaluator);

        assert_eq!(changed.len(), 3);
        assert!(changed.contains(&width));
        assert!(changed.contains(&volume));
        assert!(changed.contains(&c1));
    }

    #[test]
    fn integration_full_pipeline_bracket() {
        // Integration test: full pipeline from bracket module through to scheduler.
        // 1. Build EvaluationGraph from bracket topology
        // 2. Build ReverseDependencyIndex
        // 3. Build DemandRegistry demanding all constraints, rebuild cone
        // 4. Simulate width change via compute_dirty_cone
        // 5. compute_eval_set
        // 6. Verify eval_set = [C1] in correct order
        // 7. Execute via SequentialScheduler
        use reify_eval::cache::{EvalOutcome, NodeId};
        use reify_eval::demand::DemandRegistry;
        use reify_eval::deps::{ReverseDependencyIndex, build_trace_map};
        use reify_eval::dirty::{compute_dirty_cone, compute_eval_set};
        use reify_eval::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;
        use reify_types::{ConstraintNodeId, ValueCellId};
        use std::collections::HashSet;

        // 1. Build graph
        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);

        // 2. Build reverse index
        let index = ReverseDependencyIndex::build_from_graph(&graph);
        let traces = build_trace_map(&graph);

        let e = "Bracket";

        // 3. Demand all constraints
        let c0 = NodeId::Constraint(ConstraintNodeId::new(e, 0));
        let c1 = NodeId::Constraint(ConstraintNodeId::new(e, 1));
        let c2 = NodeId::Constraint(ConstraintNodeId::new(e, 2));

        let mut demand = DemandRegistry::new();
        demand.add_demand(c0.clone());
        demand.add_demand(c1.clone());
        demand.add_demand(c2.clone());
        demand.rebuild_cone(&graph);

        // 4. Simulate width change
        let mut changed_cells = HashSet::new();
        changed_cells.insert(ValueCellId::new(e, "width"));
        let dirty = compute_dirty_cone(&changed_cells, &index, &graph);

        // Verify dirty cone: volume, c1, and the realization (depends on width)
        assert!(dirty.contains(&NodeId::Value(ValueCellId::new(e, "volume"))));
        assert!(dirty.contains(&c1));
        assert!(
            dirty.contains(&NodeId::Realization(reify_types::RealizationNodeId::new(
                e, 0
            )))
        );
        assert_eq!(dirty.len(), 3);

        // 5. Compute eval set
        let eval_set = compute_eval_set(&dirty, &demand, &traces);

        // 6. Verify eval_set = [C1]
        assert_eq!(eval_set.len(), 1, "eval_set: {:?}", eval_set);
        assert_eq!(eval_set[0], c1);

        // 7. Execute via mock evaluator (always Changed)
        struct AlwaysDirtyChanged;
        impl NodeEvaluator for AlwaysDirtyChanged {
            fn is_dirty(&self, _node: &NodeId) -> bool {
                true
            }
            fn evaluate(&mut self, _node: &NodeId) -> EvalOutcome {
                EvalOutcome::Changed
            }
        }

        let scheduler = SequentialScheduler;
        let mut evaluator = AlwaysDirtyChanged;
        let result = scheduler.execute(eval_set, &mut evaluator);

        assert_eq!(result.len(), 1);
        assert!(result.contains(&c1));
    }
}
