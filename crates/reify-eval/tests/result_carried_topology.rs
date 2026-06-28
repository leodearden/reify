/// Task 4654 (R3a) §8 signal test — external integration test.
///
/// Exercises the pub API of `result_topology` as R3b would: build a
/// ModalResult-shaped Value with a populated `topology` field, call
/// `carried_topology_from_result`, and assert all of:
/// - Some(topo) returned
/// - face_normals() non-empty with per-face normals a selector predicate needs
/// - boundary() non-empty with OnFace(handle) entries matching face_normal handles
/// - part() carries the expected realization_ref (Part/LocationId association)
/// - all_finite()
/// - full round-trip equality
use reify_core::identity::RealizationNodeId;
use reify_ir::boundary_attachment::{BoundaryAssociation, NodeAttachment};
use reify_ir::geometry::{ElementOrderTag, GeometryHandleId, VolumeMesh};
use reify_ir::value::GeometryHandleRef;
use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

use reify_eval::compute_targets::result_topology::{
    CarriedTopology, carried_topology_from_result, from_realized_mesh,
};

/// Build a populated `CarriedTopology` using the public `from_realized_mesh`
/// builder, exercising the shared path (not constructing the struct directly).
fn make_populated_topology() -> CarriedTopology {
    let part = GeometryHandleRef {
        realization_ref: RealizationNodeId::new("body", 0),
        upstream_values_hash: [7u8; 32],
        kernel_handle: None,
    };

    let mut ba = BoundaryAssociation::default();
    ba.associate(0, NodeAttachment::OnFace(GeometryHandleId(10)));
    ba.associate(1, NodeAttachment::OnFace(GeometryHandleId(11)));
    ba.associate(2, NodeAttachment::OnEdge(GeometryHandleId(10)));

    let mesh = VolumeMesh {
        vertices: vec![
            0.0f32, 0.0, 0.0,
            1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
        ],
        tet_indices: vec![],
        element_order: ElementOrderTag::P1,
        normals: None,
        boundary: Some(ba),
    };

    // ≥ 2 per-face normals, handles matching OnFace entries in boundary
    let face_normals = vec![
        (GeometryHandleId(10), [0.0_f64, 0.0, 1.0]),
        (GeometryHandleId(11), [0.0_f64, 1.0, 0.0]),
    ];

    from_realized_mesh(part, &mesh, face_normals)
}

/// Wrap a CarriedTopology in a ModalResult-shaped StructureInstance, as
/// the real ModalResult producer does (topology field always present).
fn modal_result_with_topology(topo: &CarriedTopology) -> Value {
    let fields: PersistentMap<String, Value> = [
        ("part".to_string(), Value::Undef),
        ("modes".to_string(), Value::List(vec![])),
        ("boundary_conditions".to_string(), Value::List(vec![])),
        ("damping".to_string(), Value::Undef),
        ("mass_matrix_norm".to_string(), Value::Real(0.0)),
        ("stiffness_matrix_norm".to_string(), Value::Real(0.0)),
        ("topology".to_string(), topo.to_value()),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "ModalResult".to_string(),
        version: 1,
        fields,
    }))
}

/// §8 signal test: carried_topology_from_result surfaces the full topology
/// bundle from a ModalResult-shaped value, matching what R3b needs.
#[test]
fn carried_topology_from_modal_result_surfaces_full_topology() {
    let original = make_populated_topology();
    let modal_result = modal_result_with_topology(&original);

    // ── R3b accessor: extract topology from result ────────────────────────────
    let topo = carried_topology_from_result(&modal_result)
        .expect("carried_topology_from_result must return Some for a populated topology field");

    // ── face normals non-empty and expose per-face normals ────────────────────
    assert!(
        !topo.face_normals().is_empty(),
        "face_normals must be non-empty"
    );
    assert_eq!(topo.face_normals().len(), 2, "expected 2 face normals");
    assert_eq!(topo.face_normals()[0].0, GeometryHandleId(10));
    assert_eq!(topo.face_normals()[0].1, [0.0, 0.0, 1.0]);
    assert_eq!(topo.face_normals()[1].0, GeometryHandleId(11));
    assert_eq!(topo.face_normals()[1].1, [0.0, 1.0, 0.0]);

    // ── boundary non-empty with OnFace entries matching face_normal handles ───
    assert!(
        !topo.boundary().is_empty(),
        "boundary must be non-empty"
    );
    let face_handles: Vec<GeometryHandleId> =
        topo.face_normals().iter().map(|(h, _)| *h).collect();
    let has_matching_on_face = topo.boundary().iter().any(|(_, attach)| {
        matches!(attach, NodeAttachment::OnFace(h) if face_handles.contains(&h))
    });
    assert!(
        has_matching_on_face,
        "boundary must have OnFace entries whose handles match face_normals keys"
    );

    // ── part carries the expected realization_ref ─────────────────────────────
    assert_eq!(
        topo.part().realization_ref,
        RealizationNodeId::new("body", 0),
        "part realization_ref must match the fixture"
    );
    assert_eq!(topo.part().upstream_values_hash, [7u8; 32]);
    assert!(topo.part().kernel_handle.is_none());

    // ── all_finite ────────────────────────────────────────────────────────────
    assert!(topo.all_finite(), "all coords and normals must be finite");

    // ── full round-trip equality ──────────────────────────────────────────────
    assert_eq!(
        topo, original,
        "decoded topology must equal the original (lossless round-trip)"
    );
}

/// carried_topology_from_result returns None for an absent/Undef topology field.
#[test]
fn carried_topology_from_result_returns_none_for_undef_topology() {
    let fields: PersistentMap<String, Value> = [("topology".to_string(), Value::Undef)]
        .into_iter()
        .collect();
    let modal_result_undef = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "ModalResult".to_string(),
        version: 1,
        fields,
    }));
    assert!(
        carried_topology_from_result(&modal_result_undef).is_none(),
        "must return None for Value::Undef topology (synthetic-beam path)"
    );
}

/// carried_topology_from_result returns None for non-StructureInstance inputs.
#[test]
fn carried_topology_from_result_returns_none_for_non_si() {
    assert!(carried_topology_from_result(&Value::Undef).is_none());
    assert!(carried_topology_from_result(&Value::Int(0)).is_none());
}
