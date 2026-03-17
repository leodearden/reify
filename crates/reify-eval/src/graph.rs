// EvaluationGraph: typed graph nodes backed by PersistentMap.

use reify_compiler::{CompiledGeometryOp, ValueCellKind};
use reify_types::{CompiledExpr, ConstraintNodeId, ContentHash, RealizationNodeId, Type, ValueCellId};

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
}
