//! Priority promotion for in-flight tasks.
//!
//! When a higher-priority task depends on a lower-priority in-flight task,
//! the lower-priority task is promoted. Per §8.2: 'if a P1-slow task depends
//! on a P3 task already in-flight, the P3 task is promoted to P1-slow.'

use std::collections::HashMap;

use reify_eval::cache::NodeId;

use crate::Priority;

/// Tracks the effective priority of in-flight tasks and supports promotion.
///
/// The effective priority may differ from the original Task priority when
/// a higher-priority demanded node depends on this in-flight task.
/// The original Task struct is not mutated — PriorityPromoter tracks
/// the dynamic effective priority separately.
pub struct PriorityPromoter {
    /// Maps in-flight node → current effective priority.
    effective: HashMap<NodeId, Priority>,
}

impl PriorityPromoter {
    /// Create a new empty promoter.
    pub fn new() -> Self {
        Self {
            effective: HashMap::new(),
        }
    }

    /// Register an in-flight task with its initial priority.
    pub fn register(&mut self, node_id: NodeId, priority: Priority) {
        self.effective.insert(node_id, priority);
    }

    /// Get the current effective priority for a node, if registered.
    pub fn effective_priority(&self, node_id: &NodeId) -> Option<Priority> {
        self.effective.get(node_id).copied()
    }

    /// Promote a node to a higher priority.
    ///
    /// Only raises priority (lower enum value = higher priority via Ord).
    /// Same-or-lower-priority promotions are no-ops.
    pub fn promote(&mut self, node_id: &NodeId, new_priority: Priority) {
        if let Some(current) = self.effective.get_mut(node_id) {
            if new_priority < *current {
                *current = new_priority;
            }
        }
    }

    /// Remove a node from the promoter (on completion or cancellation).
    pub fn remove(&mut self, node_id: &NodeId) {
        self.effective.remove(node_id);
    }
}

impl Default for PriorityPromoter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::ValueCellId;

    fn make_node(name: &str) -> NodeId {
        NodeId::Value(ValueCellId::new("T", name))
    }

    #[test]
    fn register_and_effective_priority() {
        let mut promoter = PriorityPromoter::new();
        let node = make_node("a");
        promoter.register(node.clone(), Priority::P3Speculative);
        assert_eq!(
            promoter.effective_priority(&node),
            Some(Priority::P3Speculative)
        );
    }

    #[test]
    fn effective_priority_unknown_node_returns_none() {
        let promoter = PriorityPromoter::new();
        let node = make_node("unknown");
        assert_eq!(promoter.effective_priority(&node), None);
    }

    #[test]
    fn promote_raises_priority() {
        let mut promoter = PriorityPromoter::new();
        let node = make_node("a");
        promoter.register(node.clone(), Priority::P3Speculative);
        promoter.promote(&node, Priority::P1Slow);
        assert_eq!(
            promoter.effective_priority(&node),
            Some(Priority::P1Slow)
        );
    }

    #[test]
    fn promote_with_same_or_lower_priority_is_noop() {
        let mut promoter = PriorityPromoter::new();
        let node = make_node("a");
        promoter.register(node.clone(), Priority::P1Slow);
        // Promote to same priority → no-op
        promoter.promote(&node, Priority::P1Slow);
        assert_eq!(
            promoter.effective_priority(&node),
            Some(Priority::P1Slow)
        );
        // Promote to lower priority → no-op
        promoter.promote(&node, Priority::P3Speculative);
        assert_eq!(
            promoter.effective_priority(&node),
            Some(Priority::P1Slow)
        );
    }

    #[test]
    fn remove_cleans_up() {
        let mut promoter = PriorityPromoter::new();
        let node = make_node("a");
        promoter.register(node.clone(), Priority::P1Fast);
        promoter.remove(&node);
        assert_eq!(promoter.effective_priority(&node), None);
    }
}
