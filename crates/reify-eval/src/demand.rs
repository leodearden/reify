//! Demand registry: tracks which nodes are "demanded" (their results are needed).
//!
//! A node is demanded if it is either always-demanded (e.g., an active constraint)
//! or feeds into an always-demanded node transitively. The demand cone is the set
//! of all such nodes, computed via backward BFS from always-demanded roots.

use std::collections::HashSet;

use crate::cache::NodeId;

/// Tracks which nodes are demanded and maintains the demand cone.
///
/// `always_demanded` is the set of nodes explicitly requested (e.g., constraints
/// the UI is displaying). `demand_cone` is the full set of demanded nodes
/// including transitive backward dependencies, populated by `rebuild_cone()`.
pub struct DemandRegistry {
    always_demanded: HashSet<NodeId>,
    demand_cone: HashSet<NodeId>,
}

impl DemandRegistry {
    /// Create an empty demand registry.
    pub fn new() -> Self {
        Self {
            always_demanded: HashSet::new(),
            demand_cone: HashSet::new(),
        }
    }

    /// Add a node to the always-demanded set.
    pub fn add_demand(&mut self, node: NodeId) {
        self.always_demanded.insert(node);
    }

    /// Remove a node from the always-demanded set.
    pub fn remove_demand(&mut self, node: &NodeId) {
        self.always_demanded.remove(node);
    }

    /// Check if a node is in the demand cone.
    ///
    /// Returns true only after `rebuild_cone()` has been called.
    pub fn is_demanded(&self, node: &NodeId) -> bool {
        self.demand_cone.contains(node)
    }
}

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

    #[test]
    fn rebuild_cone_demand_c0_only() {
        // C0: thickness > 2mm → reads thickness
        // Demanding only C0 → cone = {C0, thickness}
        use crate::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);

        let e = "Bracket";
        let c0 = NodeId::Constraint(ConstraintNodeId::new(e, 0));

        let mut reg = DemandRegistry::new();
        reg.add_demand(c0.clone());
        reg.rebuild_cone(&graph);

        assert!(reg.is_demanded(&c0));
        assert!(reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "thickness"))));
        // width, height, fillet_radius, hole_diameter, volume should NOT be in cone
        assert!(!reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "width"))));
        assert!(!reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "height"))));
        assert!(!reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "volume"))));
        assert!(!reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "fillet_radius"))));
        assert!(!reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "hole_diameter"))));
    }

    #[test]
    fn rebuild_cone_demand_c1_only() {
        // C1: thickness < width / 4 → reads thickness, width
        // Demanding only C1 → cone = {C1, thickness, width}
        use crate::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);

        let e = "Bracket";
        let c1 = NodeId::Constraint(ConstraintNodeId::new(e, 1));

        let mut reg = DemandRegistry::new();
        reg.add_demand(c1.clone());
        reg.rebuild_cone(&graph);

        assert!(reg.is_demanded(&c1));
        assert!(reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "thickness"))));
        assert!(reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "width"))));
        // Others not in cone
        assert!(!reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "height"))));
        assert!(!reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "volume"))));
    }

    #[test]
    fn rebuild_cone_demand_all_constraints() {
        // Demand C0, C1, C2 → cone includes all three constraints
        // plus thickness, width, hole_diameter (params read by constraints).
        // volume is NOT in cone (no constraint reads it).
        use crate::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);

        let e = "Bracket";
        let c0 = NodeId::Constraint(ConstraintNodeId::new(e, 0));
        let c1 = NodeId::Constraint(ConstraintNodeId::new(e, 1));
        let c2 = NodeId::Constraint(ConstraintNodeId::new(e, 2));

        let mut reg = DemandRegistry::new();
        reg.add_demand(c0.clone());
        reg.add_demand(c1.clone());
        reg.add_demand(c2.clone());
        reg.rebuild_cone(&graph);

        // Constraints themselves
        assert!(reg.is_demanded(&c0));
        assert!(reg.is_demanded(&c1));
        assert!(reg.is_demanded(&c2));
        // Params read by constraints
        assert!(reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "thickness"))));
        assert!(reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "width"))));
        assert!(reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "hole_diameter"))));
        // volume NOT in cone (not read by any constraint)
        assert!(!reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "volume"))));
        // height NOT in cone (not read by any constraint)
        assert!(!reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "height"))));
        // fillet_radius NOT in cone
        assert!(!reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "fillet_radius"))));
    }
}
