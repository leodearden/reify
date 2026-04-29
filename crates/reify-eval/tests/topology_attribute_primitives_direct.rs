//! Direct-OCCT integration tests for the v0.2 primitive attribute seeder
//! (PRD docs/prds/v0_2/persistent-naming-v2.md decomposition-plan task 6).
//!
//! These tests bypass the engine wire-up: they spawn an `OcctKernelHandle`,
//! build a primitive via `GeometryOp`, pre-extract face/edge handles, and
//! call [`reify_eval::seed_primitive_attributes`] directly. The contract
//! under test is that for each primitive, the seeder records one
//! `TopologyAttribute` per face/edge with the expected
//! `(role, local_index)` distribution. The Engine-level pipeline tests
//! live in `topology_attribute_primitives_e2e.rs`.
//!
//! Sibling pattern: `topology_attribute_e2e.rs`. Same `OCCT_AVAILABLE`
//! gate, same `BOX_SIDE_M = 10e-3` constant, same "extract face/edge
//! handle vectors ONCE and reuse" discipline (each `extract_*` allocates
//! fresh kernel handle ids, so the test must reuse the same vectors for
//! both seeding and lookup).

use std::collections::HashSet;

use reify_eval::seed_primitive_attributes;
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_types::{
    CapKind, FeatureId, GeometryOp, RealizationNodeId, Role, TopologyAttributeTable, Value,
};

/// 10×10×10 mm box, expressed in SI metres at the kernel boundary.
const BOX_SIDE_M: f64 = 10.0e-3;

/// 5mm-radius / 10mm-height cylinder for cap-classification tests.
const CYL_RADIUS_M: f64 = 5.0e-3;
const CYL_HEIGHT_M: f64 = 10.0e-3;

/// 5mm-radius sphere for the sphere-side tests.
const SPHERE_RADIUS_M: f64 = 5.0e-3;

fn box_op() -> GeometryOp {
    GeometryOp::Box {
        width: Value::Real(BOX_SIDE_M),
        height: Value::Real(BOX_SIDE_M),
        depth: Value::Real(BOX_SIDE_M),
    }
}

fn cylinder_op() -> GeometryOp {
    GeometryOp::Cylinder {
        radius: Value::Real(CYL_RADIUS_M),
        height: Value::Real(CYL_HEIGHT_M),
    }
}

fn sphere_op() -> GeometryOp {
    GeometryOp::Sphere {
        radius: Value::Real(SPHERE_RADIUS_M),
    }
}

fn body_realization_feature_id() -> FeatureId {
    FeatureId::from(&RealizationNodeId::new("Body", 0))
}

// ─── step-1: Box → 6 face entries, all Role::Side ─────────────────────────────

#[test]
fn seed_primitive_attributes_box_records_six_side_faces() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let box_id = kernel
        .execute(&box_op())
        .expect("box should build")
        .id;

    // Pre-extract face/edge handles ONCE — extract_* allocates fresh ids
    // on each call, so we must reuse these vectors for both seeding and
    // lookup. Edges are extracted here as well so the seeder receives the
    // shape its public signature expects, even though step-1 only checks
    // face entries (edges are step-7's contract pin).
    let face_handles = kernel
        .extract_faces(box_id)
        .expect("extract_faces(box) should succeed");
    let edge_handles = kernel
        .extract_edges(box_id)
        .expect("extract_edges(box) should succeed");
    assert_eq!(
        face_handles.len(),
        6,
        "a 10mm box should have exactly 6 faces in TopExp order"
    );

    let feature_id = body_realization_feature_id();
    let mut table = TopologyAttributeTable::default();
    seed_primitive_attributes(
        &mut table,
        &mut kernel,
        &face_handles,
        &edge_handles,
        &feature_id,
        &box_op(),
    )
    .expect("seed_primitive_attributes for a 10mm box should succeed");

    // After step-1 (faces only) the table should hold exactly 6 entries —
    // edges-only seeding lands in step-8 and is pinned in step-7's test.
    assert_eq!(
        table.len(),
        6,
        "step-1 contract: exactly 6 face entries (no edges yet)"
    );

    let mut local_indices: HashSet<u32> = HashSet::new();
    for (idx, &face_id) in face_handles.iter().enumerate() {
        let attr = table.lookup(face_id).unwrap_or_else(|| {
            panic!(
                "box face #{} (handle {:?}) must have a TopologyAttribute entry",
                idx, face_id
            )
        });
        assert_eq!(
            attr.feature_id, feature_id,
            "box face #{idx} feature_id should equal Body#realization[0]"
        );
        assert_eq!(
            attr.role,
            Role::Side,
            "box face #{idx} role should be Role::Side (no caps for a box)"
        );
        assert!(
            local_indices.insert(attr.local_index),
            "box face #{idx} has a duplicate local_index {}; \
             each face must have a unique local_index in 0..6",
            attr.local_index
        );
        assert!(
            attr.local_index < 6,
            "box face #{idx} local_index {} must be in 0..6",
            attr.local_index
        );
        assert_eq!(
            attr.user_label, None,
            "box face #{idx} user_label should be None per task-1 invariant"
        );
        assert!(
            attr.mod_history.is_empty(),
            "box face #{idx} mod_history should be empty per task-1 invariant"
        );
    }

    // Round-trip: each value in 0..6 appears exactly once across the 6 faces.
    let mut sorted: Vec<u32> = local_indices.into_iter().collect();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        (0..6u32).collect::<Vec<u32>>(),
        "box face local_indices must be a permutation of 0..6"
    );

    // Touch the imports we'll use in step-3/5/7 so the test compiles cleanly
    // when sphere/cylinder helpers are added (and so a future step doesn't
    // have to rewrite the imports block). No-op assertions.
    let _cyl = cylinder_op();
    let _sph = sphere_op();
    let _: CapKind = CapKind::Top;
}

// ─── step-3: Cylinder → 1×Cap(Top) + 1×Cap(Bottom) + 1×Side ───────────────────

#[test]
fn seed_primitive_attributes_cylinder_classifies_cap_top_cap_bottom_and_side() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let cyl_id = kernel
        .execute(&cylinder_op())
        .expect("cylinder should build")
        .id;

    let face_handles = kernel
        .extract_faces(cyl_id)
        .expect("extract_faces(cylinder) should succeed");
    let edge_handles = kernel
        .extract_edges(cyl_id)
        .expect("extract_edges(cylinder) should succeed");

    // OCCT emits exactly 3 faces for a cylinder: side + top cap + bottom cap.
    assert_eq!(
        face_handles.len(),
        3,
        "a 5mm-r / 10mm-h cylinder should have exactly 3 faces (side + 2 caps)"
    );

    let feature_id = body_realization_feature_id();
    let mut table = TopologyAttributeTable::default();
    seed_primitive_attributes(
        &mut table,
        &mut kernel,
        &face_handles,
        &edge_handles,
        &feature_id,
        &cylinder_op(),
    )
    .expect("seed_primitive_attributes for a cylinder should succeed");

    // Step-3 contract: faces only — exactly 3 entries, one per face. (Edges
    // are step-7's contract.)
    assert_eq!(
        table.len(),
        3,
        "step-3 contract: exactly 3 face entries (no edges yet)"
    );

    let mut cap_top_count = 0;
    let mut cap_bottom_count = 0;
    let mut side_count = 0;
    for (idx, &face_id) in face_handles.iter().enumerate() {
        let attr = table.lookup(face_id).unwrap_or_else(|| {
            panic!(
                "cylinder face #{} (handle {:?}) must have a TopologyAttribute entry",
                idx, face_id
            )
        });
        assert_eq!(
            attr.feature_id, feature_id,
            "cylinder face #{idx} feature_id should equal Body#realization[0]"
        );
        assert_eq!(
            attr.local_index, 0,
            "cylinder face #{idx}: each role has exactly one occurrence, so local_index must be 0"
        );
        assert_eq!(
            attr.user_label, None,
            "cylinder face #{idx} user_label should be None per task-1 invariant"
        );
        assert!(
            attr.mod_history.is_empty(),
            "cylinder face #{idx} mod_history should be empty per task-1 invariant"
        );
        match attr.role {
            Role::Cap(CapKind::Top) => cap_top_count += 1,
            Role::Cap(CapKind::Bottom) => cap_bottom_count += 1,
            Role::Side => side_count += 1,
            other => panic!(
                "cylinder face #{idx} role should be Cap(Top|Bottom) or Side, got {:?}",
                other
            ),
        }
    }
    assert_eq!(
        cap_top_count, 1,
        "exactly one cylinder face must be classified Role::Cap(CapKind::Top), got {cap_top_count}"
    );
    assert_eq!(
        cap_bottom_count, 1,
        "exactly one cylinder face must be classified Role::Cap(CapKind::Bottom), got {cap_bottom_count}"
    );
    assert_eq!(
        side_count, 1,
        "exactly one cylinder face must be classified Role::Side, got {side_count}"
    );
}
