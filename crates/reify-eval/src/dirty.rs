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

    #[test]
    fn topo_sort_empty_set() {
        use crate::deps::DependencyTrace;
        use crate::dirty::topological_sort;
        use std::collections::HashMap;

        let nodes: HashSet<NodeId> = HashSet::new();
        let traces: HashMap<NodeId, DependencyTrace> = HashMap::new();
        let sorted = topological_sort(&nodes, &traces);
        assert!(sorted.is_empty());
    }

    #[test]
    fn topo_sort_single_node() {
        use crate::deps::DependencyTrace;
        use crate::dirty::topological_sort;
        use std::collections::HashMap;

        let node = NodeId::Value(ValueCellId::new("A", "x"));
        let mut nodes = HashSet::new();
        nodes.insert(node.clone());
        let mut traces = HashMap::new();
        traces.insert(node.clone(), DependencyTrace::default());
        let sorted = topological_sort(&nodes, &traces);
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0], node);
    }

    #[test]
    fn topo_sort_independent_nodes() {
        use crate::deps::DependencyTrace;
        use crate::dirty::topological_sort;
        use std::collections::HashMap;

        let a = NodeId::Value(ValueCellId::new("A", "a"));
        let b = NodeId::Value(ValueCellId::new("A", "b"));
        let c = NodeId::Constraint(ConstraintNodeId::new("A", 0));
        let mut nodes = HashSet::new();
        nodes.insert(a.clone());
        nodes.insert(b.clone());
        nodes.insert(c.clone());
        let mut traces = HashMap::new();
        traces.insert(a.clone(), DependencyTrace::default());
        traces.insert(b.clone(), DependencyTrace::default());
        traces.insert(c.clone(), DependencyTrace::default());
        let sorted = topological_sort(&nodes, &traces);
        assert_eq!(sorted.len(), 3);
        // All three present (any order for independent nodes)
        assert!(sorted.contains(&a));
        assert!(sorted.contains(&b));
        assert!(sorted.contains(&c));
    }

    #[test]
    fn topo_sort_volume_after_params() {
        use crate::deps::DependencyTrace;
        use crate::dirty::topological_sort;
        use std::collections::HashMap;

        let e = "B";
        let width = NodeId::Value(ValueCellId::new(e, "width"));
        let height = NodeId::Value(ValueCellId::new(e, "height"));
        let thickness = NodeId::Value(ValueCellId::new(e, "thickness"));
        let volume = NodeId::Value(ValueCellId::new(e, "volume"));

        let mut nodes = HashSet::new();
        nodes.insert(width.clone());
        nodes.insert(height.clone());
        nodes.insert(thickness.clone());
        nodes.insert(volume.clone());

        let mut traces = HashMap::new();
        traces.insert(width.clone(), DependencyTrace::default());
        traces.insert(height.clone(), DependencyTrace::default());
        traces.insert(thickness.clone(), DependencyTrace::default());
        traces.insert(
            volume.clone(),
            DependencyTrace {
                reads: vec![
                    ValueCellId::new(e, "width"),
                    ValueCellId::new(e, "height"),
                    ValueCellId::new(e, "thickness"),
                ],
            },
        );

        let sorted = topological_sort(&nodes, &traces);
        assert_eq!(sorted.len(), 4);
        // volume should appear after all three params
        let vol_pos = sorted.iter().position(|n| n == &volume).unwrap();
        let w_pos = sorted.iter().position(|n| n == &width).unwrap();
        let h_pos = sorted.iter().position(|n| n == &height).unwrap();
        let t_pos = sorted.iter().position(|n| n == &thickness).unwrap();
        assert!(vol_pos > w_pos, "volume should appear after width");
        assert!(vol_pos > h_pos, "volume should appear after height");
        assert!(vol_pos > t_pos, "volume should appear after thickness");
    }
}
