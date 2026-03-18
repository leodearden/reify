//! Priority promotion for in-flight tasks.
//!
//! When a higher-priority task depends on a lower-priority in-flight task,
//! the lower-priority task is promoted. Per §8.2: 'if a P1-slow task depends
//! on a P3 task already in-flight, the P3 task is promoted to P1-slow.'

use std::collections::HashMap;

use reify_eval::cache::NodeId;

use crate::Priority;

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
