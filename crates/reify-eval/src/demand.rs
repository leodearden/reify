//! Demand registry: tracks which nodes are "demanded" (their results are needed).
//!
//! A node is demanded if it is either always-demanded (e.g., an active constraint)
//! or feeds into an always-demanded node transitively. The demand cone is the set
//! of all such nodes, computed via backward BFS from always-demanded roots.

use std::collections::{HashSet, VecDeque};

use crate::cache::NodeId;
use crate::deps::{extract_dependency_trace, extract_realization_dependencies};

/// Tracks which nodes are demanded and maintains the demand cone.
///
/// `always_demanded` is the set of nodes explicitly requested (e.g., constraints
/// the UI is displaying). `demand_cone` is the full set of demanded nodes
/// including transitive backward dependencies, populated by `rebuild_cone()`.
pub struct DemandRegistry {
    always_demanded: HashSet<NodeId>,
    demand_cone: HashSet<NodeId>,
}

impl Default for DemandRegistry {
    fn default() -> Self {
        Self::new()
    }
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

    /// Return the number of nodes in the demand cone.
    pub fn cone_size(&self) -> usize {
        self.demand_cone.len()
    }

    /// Rebuild the demand cone by BFS backward from always_demanded nodes.
    ///
    /// For each demanded node, extract its dependency ValueCellIds from the graph,
    /// convert them to NodeId::Value, and add them to the cone. For Value nodes
    /// that are let bindings, continue BFS through their dependencies.
    pub fn rebuild_cone(&mut self, graph: &crate::graph::EvaluationGraph) {
        self.demand_cone.clear();

        let mut queue: VecDeque<NodeId> = self.always_demanded.iter().cloned().collect();

        while let Some(node) = queue.pop_front() {
            if !self.demand_cone.insert(node.clone()) {
                // Already visited
                continue;
            }

            // Extract dependencies for this node and add them to the cone
            let deps = match &node {
                NodeId::Value(vcid) => {
                    if let Some(cell_node) = graph.value_cells.get(vcid) {
                        cell_node
                            .default_expr
                            .as_ref()
                            .map(|e| extract_dependency_trace(e).reads)
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    }
                }
                NodeId::Constraint(cnid) => {
                    if let Some(cnode) = graph.constraints.get(cnid) {
                        extract_dependency_trace(&cnode.expr).reads
                    } else {
                        Vec::new()
                    }
                }
                NodeId::Realization(rnid) => {
                    if let Some(rnode) = graph.realizations.get(rnid) {
                        extract_realization_dependencies(&rnode.operations).reads
                    } else {
                        Vec::new()
                    }
                }
                NodeId::Resolution(res_id) => {
                    if let Some(res_node) = graph.resolutions.get(res_id) {
                        res_node.auto_params.clone()
                    } else {
                        Vec::new()
                    }
                }
                NodeId::Compute(cnid) => {
                    // P3.3: ComputeNode demand surfaces its declared
                    // `value_inputs` into the VC-keyed backward BFS.
                    //
                    // Amendment (Sugg 4): also pull each `realization_inputs`
                    // entry into the demand cone. Realizations are NodeIds,
                    // not ValueCellIds, so they bypass the post-match
                    // VC-to-Value conversion loop and go directly onto the
                    // queue as `NodeId::Realization`. The next BFS iteration
                    // processes them via the `NodeId::Realization` arm above,
                    // which surfaces the VCs read by the realization's
                    // operations. Without this push, a ComputeNode that
                    // consumes a Realization but has no value_inputs (e.g. a
                    // pure post-process of a meshed geometry) would not pull
                    // its producing Realization into the demand cone — the
                    // eval-set intersection (dirty ∩ demand) would then drop
                    // a Realization that drives a demanded ComputeNode.
                    if let Some(cn) = graph.compute_nodes.get(cnid) {
                        for rid in &cn.realization_inputs {
                            let realization_node = NodeId::Realization(rid.clone());
                            if !self.demand_cone.contains(&realization_node) {
                                queue.push_back(realization_node);
                            }
                        }
                        cn.value_inputs.clone()
                    } else {
                        Vec::new()
                    }
                }
            };

            // Convert dependencies to NodeId::Value and enqueue
            for cell_id in deps {
                let value_node = NodeId::Value(cell_id);
                if !self.demand_cone.contains(&value_node) {
                    queue.push_back(value_node);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::NodeId;
    use reify_core::{ConstraintNodeId, ValueCellId};

    #[test]
    fn demand_registry_new_is_empty() {
        let reg = DemandRegistry::new();
        let node = NodeId::Constraint(ConstraintNodeId::new("A", 0));
        assert!(!reg.is_demanded(&node));
    }

    #[test]
    fn demand_registry_add_demand() {
        use crate::graph::EvaluationGraph;
        use reify_core::Type;
        use reify_ir::{CompiledExpr, Value};
        use reify_test_support::TopologyTemplateBuilder;

        let e = "M";
        // Build minimal graph: one standalone constraint with literal expr (no deps)
        let template = TopologyTemplateBuilder::new(e)
            .constraint(
                e,
                0,
                None,
                CompiledExpr::literal(Value::Bool(true), Type::Bool),
            )
            .build();
        let graph = EvaluationGraph::from_templates(&[template]);

        let node = NodeId::Constraint(ConstraintNodeId::new(e, 0));
        let mut reg = DemandRegistry::new();
        reg.add_demand(node.clone());
        reg.rebuild_cone(&graph);

        // After rebuild, the demanded node should be in the cone
        assert!(
            reg.is_demanded(&node),
            "add_demand node should be demanded after rebuild_cone"
        );
    }

    #[test]
    fn demand_registry_remove_demand() {
        use crate::graph::EvaluationGraph;
        use reify_core::Type;
        use reify_ir::{CompiledExpr, Value};
        use reify_test_support::TopologyTemplateBuilder;

        let e = "M";
        let template = TopologyTemplateBuilder::new(e)
            .constraint(
                e,
                0,
                None,
                CompiledExpr::literal(Value::Bool(true), Type::Bool),
            )
            .build();
        let graph = EvaluationGraph::from_templates(&[template]);

        let node = NodeId::Constraint(ConstraintNodeId::new(e, 0));
        let mut reg = DemandRegistry::new();

        // Add → rebuild → verify demanded
        reg.add_demand(node.clone());
        reg.rebuild_cone(&graph);
        assert!(
            reg.is_demanded(&node),
            "node should be demanded after add + rebuild"
        );

        // Remove → rebuild → verify NOT demanded
        reg.remove_demand(&node);
        reg.rebuild_cone(&graph);
        assert!(
            !reg.is_demanded(&node),
            "node should not be demanded after remove + rebuild"
        );
        // Cone should be empty (no other demands exist)
        assert_eq!(
            reg.cone_size(),
            0,
            "demand cone should be empty after removing only demand"
        );
    }

    #[test]
    fn demand_registry_add_demand_idempotent() {
        use crate::graph::EvaluationGraph;
        use reify_core::Type;
        use reify_ir::{CompiledExpr, Value};
        use reify_test_support::TopologyTemplateBuilder;

        let e = "M";
        let template = TopologyTemplateBuilder::new(e)
            .constraint(
                e,
                0,
                None,
                CompiledExpr::literal(Value::Bool(true), Type::Bool),
            )
            .build();
        let graph = EvaluationGraph::from_templates(&[template]);

        let node = NodeId::Constraint(ConstraintNodeId::new(e, 0));
        let mut reg = DemandRegistry::new();
        reg.add_demand(node.clone());
        reg.add_demand(node.clone()); // duplicate add
        reg.rebuild_cone(&graph);

        assert!(
            reg.is_demanded(&node),
            "node should be demanded after duplicate add + rebuild"
        );
        // Cone should contain exactly 1 node (the constraint with no deps)
        // Duplicate adds should NOT inflate internal state
        assert_eq!(
            reg.cone_size(),
            1,
            "cone should contain exactly 1 node, not inflated by duplicate adds"
        );
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

    #[test]
    fn rebuild_cone_includes_resolution_and_deps() {
        use crate::graph::{EvaluationGraph, ResolutionNodeData, ValueCellNode};
        use reify_compiler::ValueCellKind;
        use reify_core::{ContentHash, ResolutionNodeId, Type};
        use reify_ir::{CompiledExpr, Value};

        let mut graph = EvaluationGraph::default();
        let e = "R";

        // Param 'a' (auto) and param 'b' (regular)
        for name in &["a", "b"] {
            let id = ValueCellId::new(e, *name);
            graph.value_cells.insert(
                id.clone(),
                ValueCellNode {
                    id: id.clone(),
                    kind: ValueCellKind::Param,
                    cell_type: Type::dimensionless_scalar(),
                    default_expr: Some(CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar())),
                    content_hash: ContentHash::of_str(name),
                },
            );
        }

        // Constraint C0 reading 'a'
        let c0_id = ConstraintNodeId::new(e, 0);
        graph.constraints.insert(
            c0_id.clone(),
            crate::graph::ConstraintNodeData {
                id: c0_id.clone(),
                label: None,
                expr: CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::dimensionless_scalar()),
                content_hash: ContentHash::of_str("c0"),
                optimized_target: None,
            },
        );

        // Resolution R0 with auto_params=['a']
        let r0_id = ResolutionNodeId::new(e, 0);
        graph.resolutions.insert(
            r0_id.clone(),
            ResolutionNodeData {
                id: r0_id.clone(),
                scope: e.to_string(),
                auto_params: vec![ValueCellId::new(e, "a")],
                constraint_deps: vec![c0_id],
                content_hash: ContentHash::of_str("r0"),
            },
        );

        let r0_node = NodeId::Resolution(r0_id);
        let mut reg = DemandRegistry::new();
        reg.add_demand(r0_node.clone());
        reg.rebuild_cone(&graph);

        // R0 is demanded
        assert!(reg.is_demanded(&r0_node));
        // 'a' is demanded (auto_param of R0)
        assert!(reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "a"))));
        // 'b' is NOT demanded
        assert!(!reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "b"))));
    }

    #[test]
    fn rebuild_cone_transitive_through_let_binding() {
        // Build graph: param a, param b, let c = a + b, constraint C0: c > 0
        // Demanding C0 → cone should include: C0, c, a, b (depth-2 transitive)
        use crate::graph::EvaluationGraph;
        use reify_core::Type;
        use reify_ir::{BinOp, CompiledExpr, Value};
        use reify_test_support::{TopologyTemplateBuilder, gt};

        let e = "T";

        // let c = a + b
        let c_expr = CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::dimensionless_scalar()),
            CompiledExpr::value_ref(ValueCellId::new(e, "b"), Type::dimensionless_scalar()),
            Type::dimensionless_scalar(),
        );

        // constraint: c > 0
        let c0_expr = gt(
            CompiledExpr::value_ref(ValueCellId::new(e, "c"), Type::dimensionless_scalar()),
            CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar()),
        );

        let template = TopologyTemplateBuilder::new(e)
            .param(
                e,
                "a",
                Type::dimensionless_scalar(),
                Some(CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar())),
            )
            .param(
                e,
                "b",
                Type::dimensionless_scalar(),
                Some(CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar())),
            )
            .let_binding(e, "c", Type::dimensionless_scalar(), c_expr)
            .constraint(e, 0, None, c0_expr)
            .build();

        let graph = EvaluationGraph::from_templates(&[template]);

        let c0 = NodeId::Constraint(ConstraintNodeId::new(e, 0));
        let mut reg = DemandRegistry::new();
        reg.add_demand(c0.clone());
        reg.rebuild_cone(&graph);

        // C0 is demanded
        assert!(reg.is_demanded(&c0));
        // C0 reads c → c is demanded
        assert!(reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "c"))));
        // c reads a and b → both are demanded (depth-2 transitive)
        assert!(reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "a"))));
        assert!(reg.is_demanded(&NodeId::Value(ValueCellId::new(e, "b"))));
    }

    /// Amendment (Sugg 4): demanding a ComputeNode with `realization_inputs`
    /// but NO `value_inputs` must still pull the producing Realization (and
    /// transitively the VCs it reads) into the demand cone. Without the
    /// realization-input enqueue in the Compute arm of `rebuild_cone`, the
    /// Realization-keyed dependency edge #10 would be silently dropped on
    /// the demand side, and the eval-set intersection would exclude any
    /// Realization that drives a demanded post-process Compute (e.g. an
    /// FEA pass over a meshed geometry).
    #[test]
    fn rebuild_cone_compute_with_realization_input_pulls_in_realization_and_its_reads() {
        use crate::graph::{ComputeNodeData, EvaluationGraph, RealizationNodeData, ValueCellNode};
        use reify_compiler::{CompiledGeometryOp, PrimitiveKind, ValueCellKind};
        use reify_core::{ComputeNodeId, ContentHash, RealizationNodeId, Type};
        use reify_ir::{CompiledExpr, ReprKind, Value};

        let mut graph = EvaluationGraph::default();
        let e = "E";

        // Param `width` — read by the realization's Box op.
        let width = ValueCellId::new(e, "width");
        graph.value_cells.insert(
            width.clone(),
            ValueCellNode {
                id: width.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::length(),
                default_expr: Some(CompiledExpr::literal(Value::length(0.08), Type::length())),
                content_hash: ContentHash::of_str("width"),
            },
        );

        // Realization R0 with a Box primitive that reads `width`.
        let r0_id = RealizationNodeId::new(e, 0);
        let r0_ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![(
                "width".to_string(),
                CompiledExpr::value_ref(width.clone(), Type::length()),
            )],
        }];
        graph.realizations.insert(
            r0_id.clone(),
            RealizationNodeData {
                geometry_cell: None,
                id: r0_id.clone(),
                operations: r0_ops,
                content_hash: ContentHash::of_str("r0"),
                produced_repr: ReprKind::BRep,
                produced_kernel: None,
            },
        );

        // Compute C with realization_inputs=[R0] and NO value_inputs. The
        // important property: without the demand-side realization push,
        // demanding C alone would leave R0 outside the cone, and the BFS
        // would never reach `width` either.
        let c_id = ComputeNodeId::new(e, 0);
        graph.insert_compute_node(ComputeNodeData {
            computation_id: c_id.clone(),
            target: "fea".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![r0_id.clone()],
            options_hash: ContentHash::of_str("opt"),
            cache_key: ContentHash::of_str("ck"),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![],
        });

        let c_node = NodeId::Compute(c_id);
        let mut reg = DemandRegistry::new();
        reg.add_demand(c_node.clone());
        reg.rebuild_cone(&graph);

        // C is demanded (root).
        assert!(reg.is_demanded(&c_node));
        // R0 is demanded via C's realization_inputs (edge #10 on the
        // demand side — the amendment under test).
        assert!(
            reg.is_demanded(&NodeId::Realization(r0_id)),
            "Realization R0 must be in the demand cone when a demanded \
             ComputeNode lists it under realization_inputs"
        );
        // width is demanded transitively (R0's Box op reads it).
        assert!(
            reg.is_demanded(&NodeId::Value(width)),
            "`width` must be in the demand cone via R0 → Box(width)"
        );
    }
}
