// EvaluationGraph: typed graph nodes backed by PersistentMap.

use reify_compiler::{CompiledGeometryOp, TopologyTemplate, ValueCellKind};
use reify_types::{
    CompiledExpr, ConstraintNodeId, ContentHash, PersistentMap, RealizationNodeId, Type,
    ValueCellId,
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
/// Holds the compiled constraint expression and its content hash.
#[derive(Debug, Clone)]
pub struct ConstraintNodeData {
    pub id: ConstraintNodeId,
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

/// The evaluation graph: holds all typed nodes in PersistentMaps
/// for O(1) clone with structural sharing.
#[derive(Debug, Clone, Default)]
pub struct EvaluationGraph {
    pub value_cells: PersistentMap<ValueCellId, ValueCellNode>,
    pub constraints: PersistentMap<ConstraintNodeId, ConstraintNodeData>,
    pub realizations: PersistentMap<RealizationNodeId, RealizationNodeData>,
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
                let node = ValueCellNode {
                    id: cell.id.clone(),
                    kind: cell.kind,
                    cell_type: cell.cell_type.clone(),
                    default_expr: cell.default_expr.clone(),
                    content_hash: cell.default_expr.as_ref()
                        .map(|e| e.content_hash)
                        .unwrap_or_else(|| ContentHash::of_str(&format!("{}", cell.id))),
                };
                graph.value_cells.insert(cell.id.clone(), node);
            }

            for constraint in &template.constraints {
                let node = ConstraintNodeData {
                    id: constraint.id.clone(),
                    expr: constraint.expr.clone(),
                    content_hash: constraint.expr.content_hash,
                };
                graph.constraints.insert(constraint.id.clone(), node);
            }

            for realization in &template.realizations {
                let content_hash = ContentHash::combine_all(
                    realization.operations.iter().map(|op| {
                        // Hash based on operation debug repr for now
                        ContentHash::of_str(&format!("{:?}", op))
                    }),
                );
                let node = RealizationNodeData {
                    id: realization.id.clone(),
                    operations: realization.operations.clone(),
                    content_hash,
                };
                graph.realizations.insert(realization.id.clone(), node);
            }
        }

        graph
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
    fn evaluation_graph_empty() {
        let graph = EvaluationGraph::default();
        assert!(graph.value_cells.is_empty());
        assert!(graph.constraints.is_empty());
        assert!(graph.realizations.is_empty());
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
}
