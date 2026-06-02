//! Integration test: Snapshot + EvaluationGraph with bracket fixture.

use reify_core::{ContentHash, RealizationNodeId, SnapshotId, ValueCellId, VersionId};
use reify_eval::snapshot::Snapshot;
use reify_ir::{DeterminacyState, SnapshotProvenance, Value};
use reify_test_support::bracket_compiled_module;

#[test]
fn snapshot_from_bracket_compiled_module() {
    let module = bracket_compiled_module();
    let snap = Snapshot::from_compiled_module(&module);

    // bracket_compiled_module has 5 params + 1 let (volume) = 6 value cells
    // and 1 realization (box with width, height, depth=thickness)
    assert_eq!(snap.graph.value_cells.len(), 6);
    assert_eq!(snap.graph.constraints.len(), 3);
    assert_eq!(snap.graph.realizations.len(), 1);

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
        let cnid = reify_core::ConstraintNodeId::new("Bracket", i);
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

#[test]
fn snapshot_from_template_with_realizations() {
    use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
    use reify_core::{ModulePath, Type};
    use reify_ir::CompiledExpr;
    use reify_test_support::{
        CompiledModuleBuilder, TopologyTemplateBuilder, gt, literal, value_ref,
    };
    use std::collections::HashSet;

    let ops = vec![CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            (
                "width".to_string(),
                CompiledExpr::literal(Value::length(0.08), Type::length()),
            ),
            (
                "height".to_string(),
                CompiledExpr::literal(Value::length(0.10), Type::length()),
            ),
            (
                "depth".to_string(),
                CompiledExpr::literal(Value::length(0.005), Type::length()),
            ),
        ],
    }];

    let template = TopologyTemplateBuilder::new("Widget")
        .param(
            "Widget",
            "width",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.08), Type::length())),
        )
        .param(
            "Widget",
            "height",
            Type::length(),
            Some(CompiledExpr::literal(Value::length(0.10), Type::length())),
        )
        .constraint(
            "Widget",
            0,
            None,
            gt(value_ref("Widget", "width"), literal(Value::length(0.0))),
        )
        .realization("Widget", 0, ops)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("widget"))
        .template(template)
        .build();

    let snap = Snapshot::from_compiled_module(&module);

    // Verify node counts
    assert_eq!(
        snap.graph.value_cells.len(),
        2,
        "expected 2 value cells (width, height)"
    );
    assert_eq!(snap.graph.constraints.len(), 1, "expected 1 constraint");
    assert_eq!(snap.graph.realizations.len(), 1, "expected 1 realization");

    // Verify the realization node
    let r_node = snap
        .graph
        .realizations
        .get(&RealizationNodeId::new("Widget", 0))
        .expect("missing realization Widget#realization[0]");
    assert_eq!(r_node.id, RealizationNodeId::new("Widget", 0));
    assert_eq!(r_node.operations.len(), 1);

    // Collect all content_hashes across all node types and assert uniqueness
    let mut all_hashes = HashSet::new();
    for (_, node) in snap.graph.value_cells.iter() {
        assert!(
            all_hashes.insert(node.content_hash),
            "duplicate content_hash found for value_cell {:?}",
            node.id
        );
    }
    for (_, node) in snap.graph.constraints.iter() {
        assert!(
            all_hashes.insert(node.content_hash),
            "duplicate content_hash found for constraint {:?}",
            node.id
        );
    }
    for (_, node) in snap.graph.realizations.iter() {
        assert!(
            all_hashes.insert(node.content_hash),
            "duplicate content_hash found for realization {:?}",
            node.id
        );
    }

    // Topology fingerprint should be non-zero
    assert_ne!(snap.topology_fingerprint, ContentHash(0));

    // Verify provenance is Initial
    assert_eq!(snap.provenance, SnapshotProvenance::Initial);

    // Verify values initialized
    assert_eq!(snap.values.len(), 2);
    for (_id, (val, det)) in snap.values.iter() {
        assert!(val.is_undef());
        assert_eq!(*det, DeterminacyState::Undetermined);
    }
}
