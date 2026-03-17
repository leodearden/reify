use reify_types::{ConstraintNodeId, RealizationNodeId, ValueCellId};

/// Unified identifier for any node in the evaluation graph.
/// Used as the key in the cache store.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeId {
    Value(ValueCellId),
    Constraint(ConstraintNodeId),
    Realization(RealizationNodeId),
}

impl From<ValueCellId> for NodeId {
    fn from(id: ValueCellId) -> Self {
        NodeId::Value(id)
    }
}

impl From<ConstraintNodeId> for NodeId {
    fn from(id: ConstraintNodeId) -> Self {
        NodeId::Constraint(id)
    }
}

impl From<RealizationNodeId> for NodeId {
    fn from(id: RealizationNodeId) -> Self {
        NodeId::Realization(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{ConstraintNodeId, RealizationNodeId, ValueCellId};

    #[test]
    fn node_id_from_value_cell_id() {
        let vcid = ValueCellId::new("Bracket", "width");
        let node: NodeId = NodeId::from(vcid.clone());
        assert_eq!(node, NodeId::Value(vcid));
    }

    #[test]
    fn node_id_from_constraint_node_id() {
        let cnid = ConstraintNodeId::new("Bracket", 0);
        let node: NodeId = NodeId::from(cnid.clone());
        assert_eq!(node, NodeId::Constraint(cnid));
    }

    #[test]
    fn node_id_from_realization_node_id() {
        let rnid = RealizationNodeId::new("Bracket", 0);
        let node: NodeId = NodeId::from(rnid.clone());
        assert_eq!(node, NodeId::Realization(rnid));
    }

    #[test]
    fn node_id_variants_not_equal_even_with_overlapping_strings() {
        let vcid = ValueCellId::new("Bracket", "width");
        let cnid = ConstraintNodeId::new("Bracket", 0);
        let rnid = RealizationNodeId::new("Bracket", 0);

        let v = NodeId::Value(vcid);
        let c = NodeId::Constraint(cnid);
        let r = NodeId::Realization(rnid);

        assert_ne!(v, c);
        assert_ne!(v, r);
        assert_ne!(c, r);
    }

    #[test]
    fn node_id_clone_and_debug() {
        let vcid = ValueCellId::new("Bracket", "width");
        let node = NodeId::Value(vcid);
        let cloned = node.clone();
        assert_eq!(node, cloned);

        let debug = format!("{:?}", node);
        assert!(debug.contains("Value"));
    }

    #[test]
    fn node_id_hash_as_map_key() {
        use std::collections::HashMap;
        let vcid = ValueCellId::new("Bracket", "width");
        let cnid = ConstraintNodeId::new("Bracket", 0);

        let mut map = HashMap::new();
        map.insert(NodeId::Value(vcid.clone()), "value");
        map.insert(NodeId::Constraint(cnid.clone()), "constraint");

        assert_eq!(map.get(&NodeId::Value(vcid)), Some(&"value"));
        assert_eq!(map.get(&NodeId::Constraint(cnid)), Some(&"constraint"));
    }

    // --- CachedResult tests ---

    #[test]
    fn cached_result_value_variant() {
        use reify_types::{DeterminacyState, Value};
        let result = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let cloned = result.clone();
        let debug = format!("{:?}", cloned);
        assert!(debug.contains("Value"));
        assert!(debug.contains("42"));
    }

    #[test]
    fn cached_result_satisfaction_variant() {
        use reify_types::Satisfaction;
        let result = CachedResult::Satisfaction(Satisfaction::Satisfied);
        let cloned = result.clone();
        let debug = format!("{:?}", cloned);
        assert!(debug.contains("Satisfaction"));
    }

    #[test]
    fn cached_result_geometry_handle_variant() {
        use reify_types::GeometryHandleId;
        let result = CachedResult::GeometryHandle(GeometryHandleId(7));
        let cloned = result.clone();
        let debug = format!("{:?}", cloned);
        assert!(debug.contains("GeometryHandle"));
    }

    #[test]
    fn cached_result_content_hash_value_variant() {
        use reify_types::{DeterminacyState, Value};
        let r1 = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let r2 = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        assert_eq!(r1.content_hash(), r2.content_hash());

        // Different value -> different hash
        let r3 = CachedResult::Value(Value::Int(99), DeterminacyState::Determined);
        assert_ne!(r1.content_hash(), r3.content_hash());

        // Same value, different determinacy -> different hash
        let r4 = CachedResult::Value(Value::Int(42), DeterminacyState::Undetermined);
        assert_ne!(r1.content_hash(), r4.content_hash());
    }

    #[test]
    fn cached_result_content_hash_satisfaction_variant() {
        use reify_types::Satisfaction;
        let r1 = CachedResult::Satisfaction(Satisfaction::Satisfied);
        let r2 = CachedResult::Satisfaction(Satisfaction::Satisfied);
        assert_eq!(r1.content_hash(), r2.content_hash());

        let r3 = CachedResult::Satisfaction(Satisfaction::Violated);
        assert_ne!(r1.content_hash(), r3.content_hash());
    }

    #[test]
    fn cached_result_content_hash_geometry_variant() {
        use reify_types::GeometryHandleId;
        let r1 = CachedResult::GeometryHandle(GeometryHandleId(7));
        let r2 = CachedResult::GeometryHandle(GeometryHandleId(7));
        assert_eq!(r1.content_hash(), r2.content_hash());

        let r3 = CachedResult::GeometryHandle(GeometryHandleId(8));
        assert_ne!(r1.content_hash(), r3.content_hash());
    }

    #[test]
    fn cached_result_content_hash_domain_separation() {
        // Ensure different variants produce different hashes even with
        // "similar" inner data
        use reify_types::{DeterminacyState, GeometryHandleId, Satisfaction, Value};
        let val = CachedResult::Value(Value::Int(0), DeterminacyState::Determined);
        let sat = CachedResult::Satisfaction(Satisfaction::Satisfied);
        let geo = CachedResult::GeometryHandle(GeometryHandleId(0));

        assert_ne!(val.content_hash(), sat.content_hash());
        assert_ne!(val.content_hash(), geo.content_hash());
        assert_ne!(sat.content_hash(), geo.content_hash());
    }
}
