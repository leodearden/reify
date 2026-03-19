use reify_types::{CompiledExpr, CompiledExprKind, ValueCellId};

/// Tracks which value cells a node read during evaluation.
///
/// This is a minimal stub for task 12 (content-hash caching).
/// Task 11 will replace this with a full dependency tracing implementation.
#[derive(Debug, Clone, Default)]
pub struct DependencyTrace {
    pub reads: Vec<ValueCellId>,
}

/// Extract a dependency trace from a compiled expression by collecting all ValueRef ids.
pub fn extract_dependency_trace(expr: &CompiledExpr) -> DependencyTrace {
    let mut reads = Vec::new();
    collect_value_refs(expr, &mut reads);
    DependencyTrace { reads }
}

/// Recursively collect all ValueRef ids from a compiled expression tree.
pub fn collect_value_refs(expr: &CompiledExpr, out: &mut Vec<ValueCellId>) {
    match &expr.kind {
        CompiledExprKind::Literal(_) => {}
        CompiledExprKind::ValueRef(id) => {
            out.push(id.clone());
        }
        CompiledExprKind::BinOp { left, right, .. } => {
            collect_value_refs(left, out);
            collect_value_refs(right, out);
        }
        CompiledExprKind::UnOp { operand, .. } => {
            collect_value_refs(operand, out);
        }
        CompiledExprKind::FunctionCall { args, .. } => {
            for arg in args {
                collect_value_refs(arg, out);
            }
        }
        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_value_refs(condition, out);
            collect_value_refs(then_branch, out);
            collect_value_refs(else_branch, out);
        }
        CompiledExprKind::UserFunctionCall { args, .. } => {
            for arg in args {
                collect_value_refs(arg, out);
            }
        }
    }
}

use std::collections::{HashMap, HashSet};
use crate::cache::NodeId;

/// Reverse dependency index: maps ValueCellId → set of NodeIds that depend on it.
///
/// This enables forward propagation: when a cell changes, look up which nodes
/// need to be re-evaluated. Built from graph structure (expressions), not runtime traces.
#[derive(Clone, Debug)]
pub struct ReverseDependencyIndex {
    index: HashMap<ValueCellId, HashSet<NodeId>>,
}

/// Empty set constant for returning references to unknown cells.
static EMPTY_SET: std::sync::LazyLock<HashSet<NodeId>> =
    std::sync::LazyLock::new(HashSet::new);

impl Default for ReverseDependencyIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl ReverseDependencyIndex {
    /// Create an empty reverse dependency index.
    pub fn new() -> Self {
        Self {
            index: HashMap::new(),
        }
    }

    /// Add a mapping: `cell` is read by `dependent`.
    pub fn add(&mut self, cell: ValueCellId, dependent: NodeId) {
        self.index.entry(cell).or_default().insert(dependent);
    }

    /// Return the set of NodeIds that depend on the given cell.
    /// Returns an empty set for unknown cells.
    pub fn dependents_of(&self, cell: &ValueCellId) -> &HashSet<NodeId> {
        self.index.get(cell).unwrap_or(&EMPTY_SET)
    }

    /// Build a reverse dependency index from an EvaluationGraph.
    ///
    /// Iterates all value cells (extracting deps from default_expr),
    /// constraints (extracting deps from expr), and realizations
    /// (extracting deps from operation args).
    pub fn build_from_graph(graph: &crate::graph::EvaluationGraph) -> Self {
        use reify_compiler::ValueCellKind;

        let mut index = Self::new();

        // Value cells: only Let bindings have dependencies (params are roots)
        for (_, node) in graph.value_cells.iter() {
            if node.kind == ValueCellKind::Let
                && let Some(ref expr) = node.default_expr
            {
                let trace = extract_dependency_trace(expr);
                let node_id = NodeId::Value(node.id.clone());
                for cell in &trace.reads {
                    index.add(cell.clone(), node_id.clone());
                }
            }
        }

        // Constraints: extract deps from constraint expression
        for (_, cnode) in graph.constraints.iter() {
            let trace = extract_dependency_trace(&cnode.expr);
            let node_id = NodeId::Constraint(cnode.id.clone());
            for cell in &trace.reads {
                index.add(cell.clone(), node_id.clone());
            }
        }

        // Realizations: extract deps from operation args
        for (_, rnode) in graph.realizations.iter() {
            let trace = extract_realization_dependencies(&rnode.operations);
            let node_id = NodeId::Realization(rnode.id.clone());
            for cell in &trace.reads {
                index.add(cell.clone(), node_id.clone());
            }
        }

        // Resolutions: auto_params are the static dependencies
        for (_, res_node) in graph.resolutions.iter() {
            let node_id = NodeId::Resolution(res_node.id.clone());
            for param in &res_node.auto_params {
                index.add(param.clone(), node_id.clone());
            }
        }

        index
    }
}

/// Build a forward dependency trace map for all nodes in the graph.
///
/// Returns a HashMap<NodeId, DependencyTrace> that maps each node to
/// the set of ValueCellIds it reads. Used by topological sort and demand cone.
pub fn build_trace_map(
    graph: &crate::graph::EvaluationGraph,
) -> HashMap<NodeId, DependencyTrace> {
    use reify_compiler::ValueCellKind;

    let mut traces = HashMap::new();

    for (_, node) in graph.value_cells.iter() {
        let trace = if node.kind == ValueCellKind::Let {
            node.default_expr
                .as_ref()
                .map(extract_dependency_trace)
                .unwrap_or_default()
        } else {
            // Params are roots with no dependencies
            DependencyTrace::default()
        };
        traces.insert(NodeId::Value(node.id.clone()), trace);
    }

    for (_, cnode) in graph.constraints.iter() {
        let trace = extract_dependency_trace(&cnode.expr);
        traces.insert(NodeId::Constraint(cnode.id.clone()), trace);
    }

    for (_, rnode) in graph.realizations.iter() {
        let trace = extract_realization_dependencies(&rnode.operations);
        traces.insert(NodeId::Realization(rnode.id.clone()), trace);
    }

    for (_, res_node) in graph.resolutions.iter() {
        let trace = DependencyTrace {
            reads: res_node.auto_params.clone(),
        };
        traces.insert(NodeId::Resolution(res_node.id.clone()), trace);
    }

    traces
}

/// Extract dependency ValueCellIds from a CompiledGeometryOp's argument expressions.
///
/// Walks all expression arguments in Primitive, Modify, and Transform ops.
/// Boolean ops have no expression arguments (just geometry refs).
pub fn extract_realization_dependencies(
    ops: &[reify_compiler::CompiledGeometryOp],
) -> DependencyTrace {
    let mut reads = Vec::new();
    for op in ops {
        match op {
            reify_compiler::CompiledGeometryOp::Primitive { args, .. } => {
                for (_, expr) in args {
                    collect_value_refs(expr, &mut reads);
                }
            }
            reify_compiler::CompiledGeometryOp::Boolean { .. } => {
                // Boolean ops reference geometry handles, not value cells
            }
            reify_compiler::CompiledGeometryOp::Modify { args, .. } => {
                for (_, expr) in args {
                    collect_value_refs(expr, &mut reads);
                }
            }
            reify_compiler::CompiledGeometryOp::Transform { args, .. } => {
                for (_, expr) in args {
                    collect_value_refs(expr, &mut reads);
                }
            }
        }
    }
    DependencyTrace { reads }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::NodeId;
    use reify_types::{ConstraintNodeId, ValueCellId};

    #[test]
    fn reverse_index_new_is_empty() {
        let index = ReverseDependencyIndex::new();
        let cell = ValueCellId::new("A", "x");
        assert!(index.dependents_of(&cell).is_empty());
    }

    #[test]
    fn reverse_index_dependents_of_unknown_cell_is_empty() {
        let index = ReverseDependencyIndex::new();
        let unknown = ValueCellId::new("Z", "unknown");
        let deps = index.dependents_of(&unknown);
        assert!(deps.is_empty());
    }

    #[test]
    fn reverse_index_add_inserts_mapping() {
        let mut index = ReverseDependencyIndex::new();
        let cell = ValueCellId::new("A", "x");
        let node = NodeId::Constraint(ConstraintNodeId::new("A", 0));
        index.add(cell.clone(), node.clone());

        let deps = index.dependents_of(&cell);
        assert_eq!(deps.len(), 1);
        assert!(deps.contains(&node));
    }

    #[test]
    fn reverse_index_multiple_dependents_of_same_cell() {
        let mut index = ReverseDependencyIndex::new();
        let cell = ValueCellId::new("A", "x");
        let node_a = NodeId::Constraint(ConstraintNodeId::new("A", 0));
        let node_b = NodeId::Constraint(ConstraintNodeId::new("A", 1));
        let node_c = NodeId::Value(ValueCellId::new("A", "volume"));
        index.add(cell.clone(), node_a.clone());
        index.add(cell.clone(), node_b.clone());
        index.add(cell.clone(), node_c.clone());

        let deps = index.dependents_of(&cell);
        assert_eq!(deps.len(), 3);
        assert!(deps.contains(&node_a));
        assert!(deps.contains(&node_b));
        assert!(deps.contains(&node_c));
    }

    #[test]
    fn reverse_index_includes_resolution_deps() {
        use crate::graph::{EvaluationGraph, ResolutionNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_types::{ContentHash, ResolutionNodeId, Type};

        let mut graph = EvaluationGraph::default();

        // Add auto param 'a'
        let a = ValueCellId::new("A", "a");
        graph.value_cells.insert(a.clone(), ValueCellNode {
            id: a.clone(),
            kind: ValueCellKind::Param,
            cell_type: Type::Real,
            default_expr: None,
            content_hash: ContentHash::of_str("a"),
        });

        // Add constraint C0 (with literal expr, for completeness)
        let c0_id = ConstraintNodeId::new("A", 0);
        graph.constraints.insert(c0_id.clone(), crate::graph::ConstraintNodeData {
            id: c0_id.clone(),
            label: None,
            expr: reify_types::CompiledExpr::literal(reify_types::Value::Bool(true), reify_types::Type::Bool),
            content_hash: ContentHash::of_str("c0"),
        });

        // Add ResolutionNodeData R0 with auto_params=['a']
        let r0_id = ResolutionNodeId::new("A", 0);
        graph.resolutions.insert(r0_id.clone(), ResolutionNodeData {
            id: r0_id.clone(),
            scope: "A".to_string(),
            auto_params: vec![a.clone()],
            constraint_deps: vec![c0_id.clone()],
            content_hash: ContentHash::of_str("r0"),
        });

        let index = ReverseDependencyIndex::build_from_graph(&graph);

        // Dependents of 'a' should include Resolution(R0)
        let a_deps = index.dependents_of(&a);
        assert!(
            a_deps.contains(&NodeId::Resolution(r0_id)),
            "dependents_of('a') should include Resolution(R0), got: {:?}", a_deps
        );
    }

    #[test]
    fn build_trace_map_includes_resolution() {
        use crate::graph::{EvaluationGraph, ResolutionNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_types::{ContentHash, ResolutionNodeId, Type};

        let mut graph = EvaluationGraph::default();

        // Add params 'a' and 'b'
        for name in &["a", "b"] {
            let id = ValueCellId::new("A", *name);
            graph.value_cells.insert(id.clone(), ValueCellNode {
                id: id.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::Real,
                default_expr: None,
                content_hash: ContentHash::of_str(*name),
            });
        }

        // Add ResolutionNodeData R0 with auto_params=['a','b']
        let r0_id = ResolutionNodeId::new("A", 0);
        graph.resolutions.insert(r0_id.clone(), ResolutionNodeData {
            id: r0_id.clone(),
            scope: "A".to_string(),
            auto_params: vec![
                ValueCellId::new("A", "a"),
                ValueCellId::new("A", "b"),
            ],
            constraint_deps: vec![],
            content_hash: ContentHash::of_str("r0"),
        });

        let traces = build_trace_map(&graph);

        let res_node = NodeId::Resolution(r0_id);
        assert!(traces.contains_key(&res_node), "trace_map should contain Resolution(R0)");
        let trace = &traces[&res_node];
        assert_eq!(trace.reads.len(), 2);
        assert!(trace.reads.contains(&ValueCellId::new("A", "a")));
        assert!(trace.reads.contains(&ValueCellId::new("A", "b")));
    }

    #[test]
    fn build_from_graph_bracket_topology() {
        use crate::graph::EvaluationGraph;
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let graph = EvaluationGraph::from_templates(&module.templates);
        let index = ReverseDependencyIndex::build_from_graph(&graph);

        let e = "Bracket";

        // width is read by: volume (let), C1 (constraint: thickness < width/4), R0 (box realization)
        let width_deps = index.dependents_of(&ValueCellId::new(e, "width"));
        assert_eq!(width_deps.len(), 3, "width dependents: {:?}", width_deps);
        assert!(width_deps.contains(&NodeId::Value(ValueCellId::new(e, "volume"))));
        assert!(width_deps.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))));
        assert!(width_deps.contains(&NodeId::Realization(reify_types::RealizationNodeId::new(e, 0))));

        // thickness is read by: volume (let), C0, C1, C2 (all three constraints), R0 (box depth)
        let thickness_deps = index.dependents_of(&ValueCellId::new(e, "thickness"));
        assert_eq!(thickness_deps.len(), 5, "thickness dependents: {:?}", thickness_deps);
        assert!(thickness_deps.contains(&NodeId::Value(ValueCellId::new(e, "volume"))));
        assert!(thickness_deps.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 0))));
        assert!(thickness_deps.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))));
        assert!(thickness_deps.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 2))));

        // fillet_radius is not read by anything in bracket
        let fillet_deps = index.dependents_of(&ValueCellId::new(e, "fillet_radius"));
        assert!(fillet_deps.is_empty(), "fillet_radius dependents: {:?}", fillet_deps);

        // hole_diameter is read by: C2 (constraint: hole_diameter < thickness*2)
        let hole_deps = index.dependents_of(&ValueCellId::new(e, "hole_diameter"));
        assert_eq!(hole_deps.len(), 1, "hole_diameter dependents: {:?}", hole_deps);
        assert!(hole_deps.contains(&NodeId::Constraint(ConstraintNodeId::new(e, 2))));

        // height is read by: volume (let), R0 (box realization)
        let height_deps = index.dependents_of(&ValueCellId::new(e, "height"));
        assert_eq!(height_deps.len(), 2, "height dependents: {:?}", height_deps);
        assert!(height_deps.contains(&NodeId::Value(ValueCellId::new(e, "volume"))));
        assert!(height_deps.contains(&NodeId::Realization(reify_types::RealizationNodeId::new(e, 0))));
    }
}
