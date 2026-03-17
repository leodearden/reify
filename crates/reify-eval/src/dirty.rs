//! Dirty cone computation and evaluation set construction.
//!
//! When a parameter changes, the dirty cone is the set of all nodes that
//! transitively depend on the changed cells. The evaluation set is the
//! intersection of the dirty cone and the demand cone, topologically sorted
//! so that dependencies are evaluated before their dependents.

use std::collections::{HashSet, VecDeque};

use crate::cache::NodeId;
use crate::deps::ReverseDependencyIndex;
use reify_types::ValueCellId;

/// Compute the dirty cone: all nodes that transitively depend on any changed cell.
///
/// BFS forward from changed cells through the reverse index. For each dependent
/// that is a Value(vcid), add vcid to the frontier for further propagation.
/// Constraint and Realization nodes are leaf nodes (no further propagation).
///
/// The changed cells themselves are NOT included in the result (they are roots).
pub fn compute_dirty_cone(
    changed: &HashSet<ValueCellId>,
    reverse_index: &ReverseDependencyIndex,
) -> HashSet<NodeId> {
    let mut dirty = HashSet::new();
    let mut frontier: VecDeque<ValueCellId> = changed.iter().cloned().collect();

    while let Some(cell) = frontier.pop_front() {
        for dependent in reverse_index.dependents_of(&cell) {
            if dirty.insert(dependent.clone()) {
                // If the dependent is a Value node, continue propagation
                if let NodeId::Value(vcid) = dependent {
                    frontier.push_back(vcid.clone());
                }
            }
        }
    }

    dirty
}

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

    #[test]
    fn dirty_cone_bracket_change_width() {
        // Change width → dirty = {volume, C1}
        // Excludes: fillet_radius, C0, C2
        use crate::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let index = ReverseDependencyIndex::build_from_graph(&graph);

        let e = "Bracket";
        let mut changed = HashSet::new();
        changed.insert(ValueCellId::new(e, "width"));

        let dirty = compute_dirty_cone(&changed, &index);

        assert!(dirty.contains(&NodeId::Value(ValueCellId::new(e, "volume"))));
        assert!(dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))));
        assert!(!dirty.contains(&NodeId::Value(ValueCellId::new(e, "fillet_radius"))));
        assert!(!dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 0))));
        assert!(!dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 2))));
        assert_eq!(dirty.len(), 2);
    }

    #[test]
    fn dirty_cone_bracket_change_thickness() {
        // Change thickness → dirty = {volume, C0, C1, C2}
        // All constraints read thickness, volume reads thickness
        use crate::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let index = ReverseDependencyIndex::build_from_graph(&graph);

        let e = "Bracket";
        let mut changed = HashSet::new();
        changed.insert(ValueCellId::new(e, "thickness"));

        let dirty = compute_dirty_cone(&changed, &index);

        assert!(dirty.contains(&NodeId::Value(ValueCellId::new(e, "volume"))));
        assert!(dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 0))));
        assert!(dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))));
        assert!(dirty.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 2))));
        assert!(!dirty.contains(&NodeId::Value(ValueCellId::new(e, "fillet_radius"))));
        assert_eq!(dirty.len(), 4);
    }

    #[test]
    fn dirty_cone_bracket_change_fillet_radius() {
        // Change fillet_radius → empty dirty cone (nothing reads fillet_radius)
        use crate::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let index = ReverseDependencyIndex::build_from_graph(&graph);

        let e = "Bracket";
        let mut changed = HashSet::new();
        changed.insert(ValueCellId::new(e, "fillet_radius"));

        let dirty = compute_dirty_cone(&changed, &index);

        assert!(dirty.is_empty(), "fillet_radius dirty cone: {:?}", dirty);
    }
}
