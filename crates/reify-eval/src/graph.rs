// EvaluationGraph: typed graph nodes backed by PersistentMap.

use std::collections::HashSet;

use reify_compiler::{CompiledGeometryOp, TopologyTemplate, ValueCellKind};
use reify_types::{
    CompiledExpr, ConstraintNodeId, ContentHash, PersistentMap, RealizationNodeId,
    ResolutionNodeId, Type, ValueCellId,
};

/// A value cell node in the evaluation graph.
/// Corresponds to a param or let binding in the topology.
#[derive(Debug, Clone)]
pub struct ValueCellNode {
    pub id: ValueCellId,
    pub kind: ValueCellKind,
    pub cell_type: Type,
    pub default_expr: Option<CompiledExpr>,
    pub content_hash: ContentHash,
}

/// A constraint node in the evaluation graph.
/// Holds the compiled constraint expression, optional label, and its content hash.
#[derive(Debug, Clone)]
pub struct ConstraintNodeData {
    pub id: ConstraintNodeId,
    pub label: Option<String>,
    pub expr: CompiledExpr,
    pub content_hash: ContentHash,
}

/// A realization node in the evaluation graph.
/// Holds the compiled geometry operations and content hash.
#[derive(Debug, Clone)]
pub struct RealizationNodeData {
    pub id: RealizationNodeId,
    pub operations: Vec<CompiledGeometryOp>,
    pub content_hash: ContentHash,
}

/// A resolution node in the evaluation graph.
/// Holds references to auto parameters and constraint dependencies
/// for constraint resolution (solving). Dependencies are static (from the template).
#[derive(Debug, Clone)]
pub struct ResolutionNodeData {
    pub id: ResolutionNodeId,
    pub scope: String,
    pub auto_params: Vec<ValueCellId>,
    pub constraint_deps: Vec<ConstraintNodeId>,
    pub content_hash: ContentHash,
}

/// Metadata for a guarded group in the evaluation graph.
/// Tracks which cells and constraints are conditionally active.
#[derive(Debug, Clone)]
pub struct GuardedGroupInfo {
    /// The guard ValueCellId (Bool, Let kind) that controls this group.
    pub guard_cell: ValueCellId,
    /// Members active when guard is true.
    pub members: Vec<ValueCellId>,
    /// Constraints active when guard is true.
    pub constraints: Vec<ConstraintNodeId>,
    /// Members active when guard is false (else branch).
    pub else_members: Vec<ValueCellId>,
    /// Constraints active when guard is false (else branch).
    pub else_constraints: Vec<ConstraintNodeId>,
}

/// The evaluation graph: holds all typed nodes in PersistentMaps
/// for O(1) clone with structural sharing.
#[derive(Debug, Clone, Default)]
pub struct EvaluationGraph {
    pub value_cells: PersistentMap<ValueCellId, ValueCellNode>,
    pub constraints: PersistentMap<ConstraintNodeId, ConstraintNodeData>,
    pub realizations: PersistentMap<RealizationNodeId, RealizationNodeData>,
    pub resolutions: PersistentMap<ResolutionNodeId, ResolutionNodeData>,
    /// Guarded groups with conditional membership.
    pub guarded_groups: Vec<GuardedGroupInfo>,
    /// ValueCellIds whose boolean value controls topology (guard cells).
    pub structure_controlling: HashSet<ValueCellId>,
}

impl EvaluationGraph {
    /// Build an EvaluationGraph from compiled topology templates.
    ///
    /// Converts each template's declarations into typed graph nodes:
    /// - ValueCellDecl → ValueCellNode
    /// - CompiledConstraint → ConstraintNodeData
    /// - RealizationDecl → RealizationNodeData
    pub fn from_templates(templates: &[TopologyTemplate]) -> Self {
        let mut graph = EvaluationGraph::default();

        for template in templates {
            for cell in &template.value_cells {
                let id_hash = ContentHash::of_str(&format!("{}", cell.id));
                let expr_hash = cell.default_expr.as_ref()
                    .map(|e| e.content_hash)
                    .unwrap_or(ContentHash(0));
                let node = ValueCellNode {
                    id: cell.id.clone(),
                    kind: cell.kind,
                    cell_type: cell.cell_type.clone(),
                    default_expr: cell.default_expr.clone(),
                    content_hash: id_hash.combine(expr_hash),
                };
                graph.value_cells.insert(cell.id.clone(), node);
            }

            for constraint in &template.constraints {
                let id_hash = ContentHash::of_str(&format!("{}", constraint.id));
                let node = ConstraintNodeData {
                    id: constraint.id.clone(),
                    label: constraint.label.clone(),
                    expr: constraint.expr.clone(),
                    content_hash: id_hash.combine(constraint.expr.content_hash),
                };
                graph.constraints.insert(constraint.id.clone(), node);
            }

            for realization in &template.realizations {
                let id_hash = ContentHash::of_str(&format!("{}", realization.id));
                let ops_hash = ContentHash::combine_all(
                    realization.operations.iter().map(|op| {
                        ContentHash::of_str(&format!("{:?}", op))
                    }),
                );
                let node = RealizationNodeData {
                    id: realization.id.clone(),
                    operations: realization.operations.clone(),
                    content_hash: id_hash.combine(ops_hash),
                };
                graph.realizations.insert(realization.id.clone(), node);
            }

            // Sub-component elaboration: create scoped ValueCellNode entries
            for sub in &template.sub_components {
                let child_template = match templates.iter().find(|t| t.name == sub.structure_name) {
                    Some(t) => t,
                    None => continue, // skip unknown structures silently
                };

                let scoped_entity = format!("{}.{}", template.name, sub.name);

                for child_cell in &child_template.value_cells {
                    let scoped_id = ValueCellId::new(&scoped_entity, &child_cell.id.member);
                    let id_hash = ContentHash::of_str(&format!("{}", scoped_id));
                    let expr_hash = child_cell.default_expr.as_ref()
                        .map(|e| e.content_hash)
                        .unwrap_or(ContentHash(0));
                    let node = ValueCellNode {
                        id: scoped_id.clone(),
                        kind: child_cell.kind,
                        cell_type: child_cell.cell_type.clone(),
                        default_expr: child_cell.default_expr.clone(),
                        content_hash: id_hash.combine(expr_hash),
                    };
                    graph.value_cells.insert(scoped_id, node);
                }
            }
        }

        graph
    }

    /// Compute a deterministic fingerprint of the graph topology.
    ///
    /// Computes three per-type sub-hashes (value_cells, constraints, realizations)
    /// independently, then combines them in fixed order. This ensures:
    /// - Determinism regardless of PersistentMap iteration order (via sorting)
    /// - Domain separation: a value_cell with hash H won't alias with a constraint of hash H
    pub fn topology_fingerprint(&self) -> ContentHash {
        let vc_hash = {
            let mut hashes: Vec<ContentHash> = self.value_cells.iter().map(|(_, n)| n.content_hash).collect();
            hashes.sort_by_key(|h| h.0);
            ContentHash::combine_all(hashes)
        };
        let cn_hash = {
            let mut hashes: Vec<ContentHash> = self.constraints.iter().map(|(_, n)| n.content_hash).collect();
            hashes.sort_by_key(|h| h.0);
            ContentHash::combine_all(hashes)
        };
        let real_hash = {
            let mut hashes: Vec<ContentHash> = self.realizations.iter().map(|(_, n)| n.content_hash).collect();
            hashes.sort_by_key(|h| h.0);
            ContentHash::combine_all(hashes)
        };
        let res_hash = {
            let mut hashes: Vec<ContentHash> = self.resolutions.iter().map(|(_, n)| n.content_hash).collect();
            hashes.sort_by_key(|h| h.0);
            ContentHash::combine_all(hashes)
        };

        let guard_hash = {
            let hashes: Vec<ContentHash> = self.guarded_groups.iter().map(|g| {
                ContentHash::of_str(&format!("{}", g.guard_cell))
            }).collect();
            ContentHash::combine_all(hashes)
        };

        ContentHash::combine_all([vc_hash, cn_hash, real_hash, res_hash, guard_hash])
    }
}

#[cfg(test)]
mod tests {
    use reify_compiler::{CompiledGeometryOp, PrimitiveKind, ValueCellKind};
    use reify_types::{
        CompiledExpr, ConstraintNodeId, ContentHash, RealizationNodeId, Type, Value, ValueCellId,
    };

    use super::*;

    #[test]
    fn value_cell_node_construction() {
        let id = ValueCellId::new("Bracket", "width");
        let node = ValueCellNode {
            id: id.clone(),
            kind: ValueCellKind::Param,
            cell_type: Type::length(),
            default_expr: Some(CompiledExpr::literal(Value::length(0.08), Type::length())),
            content_hash: ContentHash::of_str("width"),
        };

        assert_eq!(node.id, id);
        assert_eq!(node.kind, ValueCellKind::Param);
        assert_eq!(node.cell_type, Type::length());
        assert!(node.default_expr.is_some());
        assert_eq!(node.content_hash, ContentHash::of_str("width"));
    }

    #[test]
    fn value_cell_node_let_kind() {
        let id = ValueCellId::new("Bracket", "volume");
        let node = ValueCellNode {
            id: id.clone(),
            kind: ValueCellKind::Let,
            cell_type: Type::Real,
            default_expr: None,
            content_hash: ContentHash::of_str("volume"),
        };

        assert_eq!(node.kind, ValueCellKind::Let);
        assert!(node.default_expr.is_none());
    }

    #[test]
    fn value_cell_node_debug_and_clone() {
        let node = ValueCellNode {
            id: ValueCellId::new("Bracket", "width"),
            kind: ValueCellKind::Param,
            cell_type: Type::length(),
            default_expr: None,
            content_hash: ContentHash::of_str("width"),
        };

        let debug = format!("{:?}", node);
        assert!(debug.contains("ValueCellNode"));

        let cloned = node.clone();
        assert_eq!(cloned.id, node.id);
        assert_eq!(cloned.kind, node.kind);
    }

    #[test]
    fn constraint_node_data_construction() {
        let id = ConstraintNodeId::new("Bracket", 0);
        let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let hash = ContentHash::of_str("constraint0");

        let node = ConstraintNodeData {
            id: id.clone(),
            label: None,
            expr: expr.clone(),
            content_hash: hash,
        };

        assert_eq!(node.id, id);
        assert_eq!(node.content_hash, hash);
        let debug = format!("{:?}", node);
        assert!(debug.contains("ConstraintNodeData"));

        let cloned = node.clone();
        assert_eq!(cloned.id, node.id);
        assert_eq!(cloned.content_hash, node.content_hash);
    }

    #[test]
    fn realization_node_data_construction() {
        let id = RealizationNodeId::new("Bracket", 0);
        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".to_string(), CompiledExpr::literal(Value::length(0.08), Type::length())),
                ("height".to_string(), CompiledExpr::literal(Value::length(0.10), Type::length())),
                ("depth".to_string(), CompiledExpr::literal(Value::length(0.005), Type::length())),
            ],
        }];
        let hash = ContentHash::of_str("realization0");

        let node = RealizationNodeData {
            id: id.clone(),
            operations: ops,
            content_hash: hash,
        };

        assert_eq!(node.id, id);
        assert_eq!(node.operations.len(), 1);
        assert_eq!(node.content_hash, hash);

        let debug = format!("{:?}", node);
        assert!(debug.contains("RealizationNodeData"));

        let cloned = node.clone();
        assert_eq!(cloned.id, node.id);
        assert_eq!(cloned.operations.len(), 1);
    }

    #[test]
    fn resolution_node_data_construction() {
        use reify_types::ResolutionNodeId;

        let id = ResolutionNodeId::new("Bracket", 0);
        let auto_params = vec![ValueCellId::new("Bracket", "x")];
        let constraint_deps = vec![ConstraintNodeId::new("Bracket", 0)];
        let hash = ContentHash::of_str("res0");

        let node = ResolutionNodeData {
            id: id.clone(),
            scope: "Bracket".to_string(),
            auto_params: auto_params.clone(),
            constraint_deps: constraint_deps.clone(),
            content_hash: hash,
        };

        assert_eq!(node.id, id);
        assert_eq!(node.scope, "Bracket");
        assert_eq!(node.auto_params, auto_params);
        assert_eq!(node.constraint_deps, constraint_deps);
        assert_eq!(node.content_hash, hash);

        // Test Debug derive
        let debug = format!("{:?}", node);
        assert!(debug.contains("ResolutionNodeData"));

        // Test Clone derive
        let cloned = node.clone();
        assert_eq!(cloned.id, node.id);
        assert_eq!(cloned.scope, node.scope);
        assert_eq!(cloned.auto_params, node.auto_params);
        assert_eq!(cloned.constraint_deps, node.constraint_deps);
    }

    #[test]
    fn evaluation_graph_has_resolutions_map() {
        let graph = EvaluationGraph::default();
        assert!(graph.resolutions.is_empty());
        assert_eq!(graph.resolutions.len(), 0);
    }

    #[test]
    fn evaluation_graph_empty() {
        let graph = EvaluationGraph::default();
        assert!(graph.value_cells.is_empty());
        assert!(graph.constraints.is_empty());
        assert!(graph.realizations.is_empty());
        assert!(graph.resolutions.is_empty());
        assert_eq!(graph.value_cells.len(), 0);
    }

    #[test]
    fn evaluation_graph_insert_and_get() {
        let mut graph = EvaluationGraph::default();

        let vcid = ValueCellId::new("Bracket", "width");
        let node = ValueCellNode {
            id: vcid.clone(),
            kind: ValueCellKind::Param,
            cell_type: Type::length(),
            default_expr: None,
            content_hash: ContentHash::of_str("width"),
        };
        graph.value_cells.insert(vcid.clone(), node);
        assert_eq!(graph.value_cells.len(), 1);
        assert!(graph.value_cells.get(&vcid).is_some());

        let cnid = ConstraintNodeId::new("Bracket", 0);
        let cnode = ConstraintNodeData {
            id: cnid.clone(),
            label: None,
            expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
            content_hash: ContentHash::of_str("c0"),
        };
        graph.constraints.insert(cnid.clone(), cnode);
        assert_eq!(graph.constraints.len(), 1);

        let rnid = RealizationNodeId::new("Bracket", 0);
        let rnode = RealizationNodeData {
            id: rnid.clone(),
            operations: vec![],
            content_hash: ContentHash::of_str("r0"),
        };
        graph.realizations.insert(rnid.clone(), rnode);
        assert_eq!(graph.realizations.len(), 1);
    }

    #[test]
    fn evaluation_graph_clone_independence() {
        let mut graph = EvaluationGraph::default();
        let vcid = ValueCellId::new("Bracket", "width");
        graph.value_cells.insert(
            vcid.clone(),
            ValueCellNode {
                id: vcid.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::length(),
                default_expr: None,
                content_hash: ContentHash::of_str("width"),
            },
        );

        let mut cloned = graph.clone();
        let vcid2 = ValueCellId::new("Bracket", "height");
        cloned.value_cells.insert(
            vcid2.clone(),
            ValueCellNode {
                id: vcid2.clone(),
                kind: ValueCellKind::Param,
                cell_type: Type::length(),
                default_expr: None,
                content_hash: ContentHash::of_str("height"),
            },
        );

        // Original unchanged
        assert_eq!(graph.value_cells.len(), 1);
        assert!(!graph.value_cells.contains_key(&vcid2));

        // Clone has both
        assert_eq!(cloned.value_cells.len(), 2);
        assert!(cloned.value_cells.contains_key(&vcid2));
    }

    #[test]
    fn evaluation_graph_from_templates() {
        use reify_test_support::{TopologyTemplateBuilder, gt, lt, literal, value_ref};

        let template = TopologyTemplateBuilder::new("Bracket")
            .param("Bracket", "width", Type::length(), Some(CompiledExpr::literal(Value::length(0.08), Type::length())))
            .param("Bracket", "height", Type::length(), Some(CompiledExpr::literal(Value::length(0.10), Type::length())))
            .let_binding("Bracket", "volume", Type::Real, CompiledExpr::literal(Value::Real(0.0), Type::Real))
            .constraint("Bracket", 0, None, gt(value_ref("Bracket", "width"), literal(Value::length(0.01))))
            .constraint("Bracket", 1, Some("max_height"), lt(value_ref("Bracket", "height"), literal(Value::length(1.0))))
            .build();

        let graph = EvaluationGraph::from_templates(&[template]);

        // 2 params + 1 let = 3 value cells
        assert_eq!(graph.value_cells.len(), 3);
        assert!(graph.value_cells.get(&ValueCellId::new("Bracket", "width")).is_some());
        assert!(graph.value_cells.get(&ValueCellId::new("Bracket", "height")).is_some());
        assert!(graph.value_cells.get(&ValueCellId::new("Bracket", "volume")).is_some());

        // Check kinds
        let width_node = graph.value_cells.get(&ValueCellId::new("Bracket", "width")).unwrap();
        assert_eq!(width_node.kind, ValueCellKind::Param);
        let vol_node = graph.value_cells.get(&ValueCellId::new("Bracket", "volume")).unwrap();
        assert_eq!(vol_node.kind, ValueCellKind::Let);

        // 2 constraints
        assert_eq!(graph.constraints.len(), 2);
        assert!(graph.constraints.get(&ConstraintNodeId::new("Bracket", 0)).is_some());
        assert!(graph.constraints.get(&ConstraintNodeId::new("Bracket", 1)).is_some());

        // 0 realizations (none added via builder)
        assert_eq!(graph.realizations.len(), 0);
    }

    #[test]
    fn topology_fingerprint_same_structure_same_hash() {
        use reify_test_support::{TopologyTemplateBuilder, gt, literal, value_ref};

        let template1 = TopologyTemplateBuilder::new("A")
            .param("A", "x", Type::length(), Some(CompiledExpr::literal(Value::length(0.08), Type::length())))
            .constraint("A", 0, None, gt(value_ref("A", "x"), literal(Value::length(0.0))))
            .build();
        let template2 = TopologyTemplateBuilder::new("A")
            .param("A", "x", Type::length(), Some(CompiledExpr::literal(Value::length(0.08), Type::length())))
            .constraint("A", 0, None, gt(value_ref("A", "x"), literal(Value::length(0.0))))
            .build();

        let g1 = EvaluationGraph::from_templates(&[template1]);
        let g2 = EvaluationGraph::from_templates(&[template2]);

        assert_eq!(g1.topology_fingerprint(), g2.topology_fingerprint());
    }

    #[test]
    fn topology_fingerprint_different_structure_different_hash() {
        use reify_test_support::{TopologyTemplateBuilder, gt, literal, value_ref};

        let template1 = TopologyTemplateBuilder::new("A")
            .param("A", "x", Type::length(), Some(CompiledExpr::literal(Value::length(0.08), Type::length())))
            .build();
        let template2 = TopologyTemplateBuilder::new("A")
            .param("A", "x", Type::length(), Some(CompiledExpr::literal(Value::length(0.08), Type::length())))
            .constraint("A", 0, None, gt(value_ref("A", "x"), literal(Value::length(0.0))))
            .build();

        let g1 = EvaluationGraph::from_templates(&[template1]);
        let g2 = EvaluationGraph::from_templates(&[template2]);

        assert_ne!(g1.topology_fingerprint(), g2.topology_fingerprint());
    }

    #[test]
    fn topology_fingerprint_order_independent() {
        // Insert same nodes in different order, should produce same fingerprint
        let mut g1 = EvaluationGraph::default();
        let mut g2 = EvaluationGraph::default();

        let a = ValueCellId::new("X", "a");
        let b = ValueCellId::new("X", "b");
        let node_a = ValueCellNode {
            id: a.clone(),
            kind: ValueCellKind::Param,
            cell_type: Type::length(),
            default_expr: None,
            content_hash: ContentHash::of_str("a"),
        };
        let node_b = ValueCellNode {
            id: b.clone(),
            kind: ValueCellKind::Param,
            cell_type: Type::length(),
            default_expr: None,
            content_hash: ContentHash::of_str("b"),
        };

        // Different insertion order
        g1.value_cells.insert(a.clone(), node_a.clone());
        g1.value_cells.insert(b.clone(), node_b.clone());

        g2.value_cells.insert(b.clone(), node_b);
        g2.value_cells.insert(a.clone(), node_a);

        assert_eq!(g1.topology_fingerprint(), g2.topology_fingerprint());
    }

    #[test]
    fn content_hash_includes_node_id_for_value_cells() {
        use reify_test_support::TopologyTemplateBuilder;

        // Two params with different IDs but identical default expressions
        let template_a = TopologyTemplateBuilder::new("A")
            .param("A", "width", Type::length(), Some(CompiledExpr::literal(Value::length(0.08), Type::length())))
            .build();
        let template_b = TopologyTemplateBuilder::new("A")
            .param("A", "height", Type::length(), Some(CompiledExpr::literal(Value::length(0.08), Type::length())))
            .build();

        let graph_a = EvaluationGraph::from_templates(&[template_a]);
        let graph_b = EvaluationGraph::from_templates(&[template_b]);

        let hash_width = graph_a.value_cells.get(&ValueCellId::new("A", "width")).unwrap().content_hash;
        let hash_height = graph_b.value_cells.get(&ValueCellId::new("A", "height")).unwrap().content_hash;

        // Different IDs with same expression must produce different content hashes
        assert_ne!(hash_width, hash_height, "content_hash must incorporate node ID");
    }

    #[test]
    fn content_hash_includes_node_id_for_constraints() {
        use reify_test_support::{TopologyTemplateBuilder, gt, literal, value_ref};

        let expr = gt(value_ref("A", "x"), literal(Value::length(0.0)));
        let template = TopologyTemplateBuilder::new("A")
            .param("A", "x", Type::length(), None)
            .constraint("A", 0, None, expr.clone())
            .constraint("A", 1, None, expr)
            .build();

        let graph = EvaluationGraph::from_templates(&[template]);

        let hash_0 = graph.constraints.get(&ConstraintNodeId::new("A", 0)).unwrap().content_hash;
        let hash_1 = graph.constraints.get(&ConstraintNodeId::new("A", 1)).unwrap().content_hash;

        // Different constraint IDs with same expression must produce different content hashes
        assert_ne!(hash_0, hash_1, "content_hash must incorporate constraint node ID");
    }

    #[test]
    fn content_hash_no_expr_value_cell_uses_id() {
        use reify_test_support::TopologyTemplateBuilder;

        // A param with no default_expr should still have a non-zero content_hash derived from its ID
        let template = TopologyTemplateBuilder::new("A")
            .param("A", "x", Type::length(), None)
            .build();

        let graph = EvaluationGraph::from_templates(&[template]);
        let node = graph.value_cells.get(&ValueCellId::new("A", "x")).unwrap();

        assert_ne!(node.content_hash, ContentHash(0), "param without default_expr should have non-zero content_hash");
    }

    #[test]
    fn from_templates_with_realizations() {
        use reify_test_support::TopologyTemplateBuilder;

        let ops = vec![CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![
                ("width".to_string(), CompiledExpr::literal(Value::length(0.08), Type::length())),
                ("height".to_string(), CompiledExpr::literal(Value::length(0.10), Type::length())),
                ("depth".to_string(), CompiledExpr::literal(Value::length(0.005), Type::length())),
            ],
        }];

        let template = TopologyTemplateBuilder::new("A")
            .param("A", "w", Type::length(), Some(CompiledExpr::literal(Value::length(0.08), Type::length())))
            .realization("A", 0, ops.clone())
            .build();

        let graph = EvaluationGraph::from_templates(&[template]);

        // Realization should be populated
        assert_eq!(graph.realizations.len(), 1);
        let r_node = graph.realizations.get(&RealizationNodeId::new("A", 0)).unwrap();
        assert_eq!(r_node.id, RealizationNodeId::new("A", 0));
        assert_eq!(r_node.operations.len(), 1);

        // Verify content_hash matches manually computed value:
        // id_hash.combine(ops_hash)
        let expected_id_hash = ContentHash::of_str(&format!("{}", RealizationNodeId::new("A", 0)));
        let expected_ops_hash = ContentHash::combine_all(
            ops.iter().map(|op| ContentHash::of_str(&format!("{:?}", op)))
        );
        let expected_hash = expected_id_hash.combine(expected_ops_hash);
        assert_eq!(r_node.content_hash, expected_hash, "realization content_hash should be id_hash.combine(ops_hash)");
        assert_ne!(r_node.content_hash, ContentHash(0));
    }

    #[test]
    fn fingerprint_domain_separates_node_types() {
        // graph_a has a value_cell with hash H, graph_b has a constraint with hash H
        let hash_h = ContentHash::of_str("same");

        let mut graph_a = EvaluationGraph::default();
        graph_a.value_cells.insert(
            ValueCellId::new("X", "a"),
            ValueCellNode {
                id: ValueCellId::new("X", "a"),
                kind: ValueCellKind::Param,
                cell_type: Type::length(),
                default_expr: None,
                content_hash: hash_h,
            },
        );

        let mut graph_b = EvaluationGraph::default();
        graph_b.constraints.insert(
            ConstraintNodeId::new("X", 0),
            ConstraintNodeData {
                id: ConstraintNodeId::new("X", 0),
                label: None,
                expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
                content_hash: hash_h,
            },
        );

        assert_ne!(
            graph_a.topology_fingerprint(),
            graph_b.topology_fingerprint(),
            "fingerprint must domain-separate value_cells from constraints"
        );
    }

    #[test]
    fn fingerprint_domain_separates_all_three_types() {
        let hash_h = ContentHash::of_str("same");

        let mut graph_a = EvaluationGraph::default();
        graph_a.value_cells.insert(
            ValueCellId::new("X", "a"),
            ValueCellNode {
                id: ValueCellId::new("X", "a"),
                kind: ValueCellKind::Param,
                cell_type: Type::length(),
                default_expr: None,
                content_hash: hash_h,
            },
        );

        let mut graph_b = EvaluationGraph::default();
        graph_b.constraints.insert(
            ConstraintNodeId::new("X", 0),
            ConstraintNodeData {
                id: ConstraintNodeId::new("X", 0),
                label: None,
                expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
                content_hash: hash_h,
            },
        );

        let mut graph_c = EvaluationGraph::default();
        graph_c.realizations.insert(
            RealizationNodeId::new("X", 0),
            RealizationNodeData {
                id: RealizationNodeId::new("X", 0),
                operations: vec![],
                content_hash: hash_h,
            },
        );

        let fp_a = graph_a.topology_fingerprint();
        let fp_b = graph_b.topology_fingerprint();
        let fp_c = graph_c.topology_fingerprint();

        // All three must be pairwise distinct
        assert_ne!(fp_a, fp_b, "value_cell vs constraint fingerprints must differ");
        assert_ne!(fp_a, fp_c, "value_cell vs realization fingerprints must differ");
        assert_ne!(fp_b, fp_c, "constraint vs realization fingerprints must differ");
    }

    #[test]
    fn evaluation_graph_resolution_clone_independence() {
        use reify_types::ResolutionNodeId;

        let mut graph = EvaluationGraph::default();
        let r0_id = ResolutionNodeId::new("A", 0);
        graph.resolutions.insert(r0_id.clone(), ResolutionNodeData {
            id: r0_id.clone(),
            scope: "A".to_string(),
            auto_params: vec![ValueCellId::new("A", "x")],
            constraint_deps: vec![],
            content_hash: ContentHash::of_str("r0"),
        });

        let mut cloned = graph.clone();
        let r1_id = ResolutionNodeId::new("A", 1);
        cloned.resolutions.insert(r1_id.clone(), ResolutionNodeData {
            id: r1_id.clone(),
            scope: "A".to_string(),
            auto_params: vec![ValueCellId::new("A", "y")],
            constraint_deps: vec![],
            content_hash: ContentHash::of_str("r1"),
        });

        // Original unchanged
        assert_eq!(graph.resolutions.len(), 1);
        assert!(!graph.resolutions.contains_key(&r1_id));

        // Clone has both
        assert_eq!(cloned.resolutions.len(), 2);
        assert!(cloned.resolutions.contains_key(&r0_id));
        assert!(cloned.resolutions.contains_key(&r1_id));
    }

    #[test]
    fn topology_fingerprint_includes_resolutions() {
        use reify_test_support::TopologyTemplateBuilder;
        use reify_types::{CompiledExpr, ResolutionNodeId, Type, Value};

        // Build two identical graphs from same template
        let template1 = TopologyTemplateBuilder::new("A")
            .param("A", "x", Type::Real, Some(CompiledExpr::literal(Value::Real(1.0), Type::Real)))
            .build();
        let template2 = TopologyTemplateBuilder::new("A")
            .param("A", "x", Type::Real, Some(CompiledExpr::literal(Value::Real(1.0), Type::Real)))
            .build();

        let g1 = EvaluationGraph::from_templates(&[template1]);
        let mut g2 = EvaluationGraph::from_templates(&[template2]);

        // Before adding resolution, fingerprints should be equal
        assert_eq!(g1.topology_fingerprint(), g2.topology_fingerprint());

        // Add a ResolutionNodeData to g2
        let r0_id = ResolutionNodeId::new("A", 0);
        g2.resolutions.insert(r0_id.clone(), ResolutionNodeData {
            id: r0_id,
            scope: "A".to_string(),
            auto_params: vec![ValueCellId::new("A", "x")],
            constraint_deps: vec![],
            content_hash: ContentHash::of_str("r0"),
        });

        // After adding resolution, fingerprints must differ
        assert_ne!(g1.topology_fingerprint(), g2.topology_fingerprint(),
            "fingerprint must change when resolution node is added");

        // Two graphs with identical resolutions should have same fingerprint
        let mut g3 = g1.clone();
        let r0_id2 = ResolutionNodeId::new("A", 0);
        g3.resolutions.insert(r0_id2.clone(), ResolutionNodeData {
            id: r0_id2,
            scope: "A".to_string(),
            auto_params: vec![ValueCellId::new("A", "x")],
            constraint_deps: vec![],
            content_hash: ContentHash::of_str("r0"),
        });
        assert_eq!(g2.topology_fingerprint(), g3.topology_fingerprint());
    }

    #[test]
    fn fingerprint_domain_separates_resolution_from_others() {
        let hash_h = ContentHash::of_str("same");
        use reify_types::ResolutionNodeId;

        let mut graph_a = EvaluationGraph::default();
        graph_a.value_cells.insert(
            ValueCellId::new("X", "a"),
            ValueCellNode {
                id: ValueCellId::new("X", "a"),
                kind: ValueCellKind::Param,
                cell_type: Type::length(),
                default_expr: None,
                content_hash: hash_h,
            },
        );

        let mut graph_d = EvaluationGraph::default();
        graph_d.resolutions.insert(
            ResolutionNodeId::new("X", 0),
            ResolutionNodeData {
                id: ResolutionNodeId::new("X", 0),
                scope: "X".to_string(),
                auto_params: vec![],
                constraint_deps: vec![],
                content_hash: hash_h,
            },
        );

        assert_ne!(
            graph_a.topology_fingerprint(),
            graph_d.topology_fingerprint(),
            "fingerprint must domain-separate value_cells from resolutions"
        );
    }

    #[test]
    fn sub_component_nodes_in_evaluation_graph() {
        use reify_test_support::TopologyTemplateBuilder;
        use reify_types::{BinOp, CompiledExpr, Type, Value};

        // Child: param height, let half_h = height / 2
        let height_ref = || CompiledExpr::value_ref(ValueCellId::new("Child", "height"), Type::length());
        let half_h_expr = CompiledExpr::binop(
            BinOp::Div,
            height_ref(),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            Type::length(),
        );
        let child = TopologyTemplateBuilder::new("Child")
            .param("Child", "height", Type::length(), Some(CompiledExpr::literal(Value::length(0.01), Type::length())))
            .let_binding("Child", "half_h", Type::length(), half_h_expr)
            .build();

        // Parent: param width, sub rib = Child(height: width * 0.5)
        let width_ref = || CompiledExpr::value_ref(ValueCellId::new("Parent", "width"), Type::length());
        let arg_expr = CompiledExpr::binop(
            BinOp::Mul,
            width_ref(),
            CompiledExpr::literal(Value::Real(0.5), Type::Real),
            Type::length(),
        );
        let parent = TopologyTemplateBuilder::new("Parent")
            .param("Parent", "width", Type::length(), Some(CompiledExpr::literal(Value::length(0.08), Type::length())))
            .sub_component("rib", "Child", vec![("height".to_string(), arg_expr)])
            .build();

        let graph = EvaluationGraph::from_templates(&[child, parent]);

        // Should have scoped entries for sub-component
        let scoped_height = ValueCellId::new("Parent.rib", "height");
        let scoped_half_h = ValueCellId::new("Parent.rib", "half_h");

        assert!(
            graph.value_cells.get(&scoped_height).is_some(),
            "graph should contain scoped Parent.rib.height node"
        );
        assert!(
            graph.value_cells.get(&scoped_half_h).is_some(),
            "graph should contain scoped Parent.rib.half_h node"
        );

        // Verify kinds are preserved
        let h_node = graph.value_cells.get(&scoped_height).unwrap();
        assert_eq!(h_node.kind, ValueCellKind::Param);
        let hh_node = graph.value_cells.get(&scoped_half_h).unwrap();
        assert_eq!(hh_node.kind, ValueCellKind::Let);
    }
}
