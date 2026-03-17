//! Integration test: Snapshot + EvaluationGraph with bracket fixture.

use reify_eval::snapshot::Snapshot;
use reify_test_support::bracket_compiled_module;
use reify_types::{
    ContentHash, DeterminacyState, SnapshotId, SnapshotProvenance, Value, ValueCellId, VersionId,
};

#[test]
fn snapshot_from_bracket_compiled_module() {
    let module = bracket_compiled_module();
    let snap = Snapshot::from_compiled_module(&module);

    // bracket_compiled_module has 5 params + 1 let (volume) = 6 value cells
    // (body is a realization in the real compiler, but the test fixture builder
    // does not add it as a realization — so 0 realizations from this fixture)
    assert_eq!(snap.graph.value_cells.len(), 6);
    assert_eq!(snap.graph.constraints.len(), 3);
    assert_eq!(snap.graph.realizations.len(), 0);

    // Provenance should be Initial
    assert_eq!(snap.provenance, SnapshotProvenance::Initial);

    // Version and snapshot IDs start at 0
    assert_eq!(snap.id, SnapshotId(0));
    assert_eq!(snap.version, VersionId(0));

    // All values should be (Undef, Undetermined)
    assert_eq!(snap.values.len(), 6);
    for (_id, (val, det)) in snap.values.iter() {
        assert!(val.is_undef());
        assert_eq!(*det, DeterminacyState::Undetermined);
    }

    // Verify specific value cells exist
    let expected_cells = [
        ("Bracket", "width"),
        ("Bracket", "height"),
        ("Bracket", "thickness"),
        ("Bracket", "fillet_radius"),
        ("Bracket", "hole_diameter"),
        ("Bracket", "volume"),
    ];
    for (entity, member) in &expected_cells {
        let id = ValueCellId::new(*entity, *member);
        assert!(
            snap.graph.value_cells.get(&id).is_some(),
            "Missing value cell: {}.{}",
            entity,
            member
        );
        assert!(
            snap.values.get(&id).is_some(),
            "Missing value entry: {}.{}",
            entity,
            member
        );
    }

    // Verify specific constraints exist
    for i in 0..3 {
        let cnid = reify_types::ConstraintNodeId::new("Bracket", i);
        assert!(
            snap.graph.constraints.get(&cnid).is_some(),
            "Missing constraint: Bracket#constraint[{}]",
            i
        );
    }

    // Topology fingerprint should be non-trivial
    assert_ne!(snap.topology_fingerprint, ContentHash(0));
}

#[test]
fn snapshot_clone_and_modify_independence() {
    let module = bracket_compiled_module();
    let snap = Snapshot::from_compiled_module(&module);

    // Clone the snapshot
    let mut modified = snap.clone();

    // Set width to a determined value in the clone
    let width_id = ValueCellId::new("Bracket", "width");
    modified.values.insert(
        width_id.clone(),
        (Value::length(0.08), DeterminacyState::Determined),
    );

    // Original is unchanged
    let (orig_val, orig_det) = snap.values.get(&width_id).unwrap();
    assert!(orig_val.is_undef());
    assert_eq!(*orig_det, DeterminacyState::Undetermined);

    // Clone has the new value
    let (mod_val, mod_det) = modified.values.get(&width_id).unwrap();
    assert!(!mod_val.is_undef());
    assert_eq!(*mod_det, DeterminacyState::Determined);

    // Both have the same number of entries
    assert_eq!(snap.values.len(), modified.values.len());

    // Graph is shared (same topology fingerprint)
    assert_eq!(snap.topology_fingerprint, modified.topology_fingerprint);
}
