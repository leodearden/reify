//! Demand registry: tracks which nodes are "demanded" (their results are needed).
//!
//! A node is demanded if it is either always-demanded (e.g., an active constraint)
//! or feeds into an always-demanded node transitively. The demand cone is the set
//! of all such nodes, computed via backward BFS from always-demanded roots.

use crate::cache::NodeId;

/// Placeholder struct — methods will be implemented in step-6.
pub struct DemandRegistry;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::NodeId;
    use reify_types::{ConstraintNodeId, ValueCellId};

    #[test]
    fn demand_registry_new_is_empty() {
        let reg = DemandRegistry::new();
        let node = NodeId::Constraint(ConstraintNodeId::new("A", 0));
        assert!(!reg.is_demanded(&node));
    }

    #[test]
    fn demand_registry_add_demand() {
        let mut reg = DemandRegistry::new();
        let node = NodeId::Constraint(ConstraintNodeId::new("A", 0));
        reg.add_demand(node.clone());
        // Note: is_demanded checks the demand_cone, which is only populated
        // after rebuild_cone(). But always_demanded nodes should be in the
        // cone after a rebuild. For now, just verify add_demand doesn't panic.
        // The full is_demanded check will be tested after rebuild_cone.
    }

    #[test]
    fn demand_registry_remove_demand() {
        let mut reg = DemandRegistry::new();
        let node = NodeId::Constraint(ConstraintNodeId::new("A", 0));
        reg.add_demand(node.clone());
        reg.remove_demand(&node);
        // After remove, the node should not be always_demanded
    }

    #[test]
    fn demand_registry_add_demand_idempotent() {
        let mut reg = DemandRegistry::new();
        let node = NodeId::Constraint(ConstraintNodeId::new("A", 0));
        reg.add_demand(node.clone());
        reg.add_demand(node.clone());
        // Adding twice should be fine (idempotent)
    }

    #[test]
    fn demand_registry_is_demanded_checks_demand_cone() {
        let mut reg = DemandRegistry::new();
        let c0 = NodeId::Constraint(ConstraintNodeId::new("A", 0));
        let param = NodeId::Value(ValueCellId::new("A", "x"));
        reg.add_demand(c0.clone());
        // Before rebuild, demand_cone is empty, so is_demanded is false
        assert!(!reg.is_demanded(&c0));
        assert!(!reg.is_demanded(&param));
    }
}
