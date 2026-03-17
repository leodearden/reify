//! Dirty cone computation and evaluation set construction.
//!
//! When a parameter changes, the dirty cone is the set of all nodes that
//! transitively depend on the changed cells. The evaluation set is the
//! intersection of the dirty cone and the demand cone, topologically sorted
//! so that dependencies are evaluated before their dependents.

#[cfg(test)]
mod tests {
    use crate::cache::NodeId;
    use crate::deps::ReverseDependencyIndex;
    use crate::dirty::compute_dirty_cone;
    use reify_types::{ConstraintNodeId, ValueCellId};
    use std::collections::HashSet;

    #[test]
    fn dirty_cone_empty_changed_set() {
        let index = ReverseDependencyIndex::new();
        let changed: HashSet<ValueCellId> = HashSet::new();
        let dirty = compute_dirty_cone(&changed, &index);
        assert!(dirty.is_empty());
    }

    #[test]
    fn dirty_cone_single_changed_param() {
        // width is read by volume (let) and C1 (constraint)
        use crate::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let index = ReverseDependencyIndex::build_from_graph(&graph);

        let e = "Bracket";
        let mut changed = HashSet::new();
        changed.insert(ValueCellId::new(e, "width"));

        let dirty = compute_dirty_cone(&changed, &index);

        // Dirty should contain volume and C1 (both read width)
        assert!(dirty.contains(&NodeId::Value(ValueCellId::new(e, "volume"))));
        assert!(dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))));
        // Changed cell itself is NOT in dirty cone
        assert!(!dirty.contains(&NodeId::Value(ValueCellId::new(e, "width"))));
        // Other nodes not affected
        assert!(!dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 0))));
        assert!(!dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 2))));
    }
}
