// Snapshot: immutable evaluation state with provenance tracking.

#[cfg(test)]
mod tests {
    use reify_types::{
        ContentHash, DeterminacyState, PersistentMap, SnapshotId, SnapshotProvenance, Value,
        ValueCellId, VersionId,
    };

    use crate::graph::EvaluationGraph;

    use super::*;

    #[test]
    fn snapshot_construction() {
        let graph = EvaluationGraph::default();
        let values = PersistentMap::new();
        let fingerprint = ContentHash::of_str("empty");

        let snap = Snapshot {
            id: SnapshotId(0),
            version: VersionId(0),
            graph: graph.clone(),
            values,
            topology_fingerprint: fingerprint,
            provenance: SnapshotProvenance::Initial,
        };

        assert_eq!(snap.id, SnapshotId(0));
        assert_eq!(snap.version, VersionId(0));
        assert_eq!(snap.topology_fingerprint, fingerprint);
        assert_eq!(snap.provenance, SnapshotProvenance::Initial);
    }

    #[test]
    fn snapshot_from_compiled_module() {
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let snap = Snapshot::from_compiled_module(&module);

        // Check snapshot IDs start at 0
        assert_eq!(snap.id, SnapshotId(0));
        assert_eq!(snap.version, VersionId(0));

        // Check provenance is Initial
        assert_eq!(snap.provenance, SnapshotProvenance::Initial);

        // Check graph has correct number of nodes
        // bracket_compiled_module has 5 params + 1 let (volume) = 6 value cells
        assert_eq!(snap.graph.value_cells.len(), 6);
        // 3 constraints
        assert_eq!(snap.graph.constraints.len(), 3);

        // Check values are initialized to (Undef, Undetermined)
        assert_eq!(snap.values.len(), 6);
        for (_id, (val, det)) in snap.values.iter() {
            assert!(val.is_undef(), "Expected Undef, got {:?}", val);
            assert_eq!(*det, DeterminacyState::Undetermined);
        }

        // Check specific value cell exists
        let width_id = ValueCellId::new("Bracket", "width");
        assert!(snap.values.get(&width_id).is_some());
        let (val, det) = snap.values.get(&width_id).unwrap();
        assert!(val.is_undef());
        assert_eq!(*det, DeterminacyState::Undetermined);

        // Topology fingerprint should be non-zero
        assert_ne!(snap.topology_fingerprint, ContentHash(0));
    }

    #[test]
    fn snapshot_debug_and_clone() {
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let snap = Snapshot::from_compiled_module(&module);

        let debug = format!("{:?}", snap);
        assert!(debug.contains("Snapshot"));

        let cloned = snap.clone();
        assert_eq!(cloned.id, snap.id);
        assert_eq!(cloned.version, snap.version);
        assert_eq!(cloned.provenance, snap.provenance);
    }
}
