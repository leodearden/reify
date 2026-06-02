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

use reify_core::RealizationNodeId;
use reify_eval::{seed_primitive_attributes, seed_primitive_attributes_for_handle};
use reify_ir::{AxisSign, CapKind, FeatureId, GeometryOp, Role, TopologyAttributeTable, Value};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};

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
    let box_id = kernel.execute(&box_op()).expect("box should build").id;

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
        &[],
        &feature_id,
        &box_op(),
    )
    .expect("seed_primitive_attributes for a 10mm box should succeed");

    // The faces-only assertion this test originally pinned was relaxed in
    // step-8 once the seeder also writes edges (step-7's test covers the
    // edges contract). The 6-face contract is still pinned per-handle below;
    // the table simply holds 6 face entries plus 12 edge entries now.
    assert_eq!(
        table.len(),
        6 + edge_handles.len(),
        "box: 6 face entries + edge entries (step-1 face contract preserved)"
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
        &[],
        &feature_id,
        &cylinder_op(),
    )
    .expect("seed_primitive_attributes for a cylinder should succeed");

    // Step-3 originally pinned faces only; step-8 widened to include edges
    // too (covered by step-7's dedicated test). The 3-face contract is still
    // pinned per-role below; the table simply also holds the cylinder's edges.
    assert_eq!(
        table.len(),
        3 + edge_handles.len(),
        "cylinder: 3 face entries + edge entries (step-3 face contract preserved)"
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

// ─── step-5: Sphere → ≥1 face entries, all Role::Side ─────────────────────────

#[test]
fn seed_primitive_attributes_sphere_records_role_side_for_each_face() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let sphere_id = kernel
        .execute(&sphere_op())
        .expect("sphere should build")
        .id;

    // Pre-extract face/edge handles ONCE — fresh ids each call.
    let face_handles = kernel
        .extract_faces(sphere_id)
        .expect("extract_faces(sphere) should succeed");
    let edge_handles = kernel
        .extract_edges(sphere_id)
        .expect("extract_edges(sphere) should succeed");

    // OCCT's sphere parameterisation may emit 1+ faces (the count varies
    // across OCCT versions / sphere parameterisations); the contract is
    // "at least 1". We don't pin an exact count.
    assert!(
        !face_handles.is_empty(),
        "a sphere should have at least 1 face from extract_faces"
    );

    let feature_id = body_realization_feature_id();
    let mut table = TopologyAttributeTable::default();
    seed_primitive_attributes(
        &mut table,
        &mut kernel,
        &face_handles,
        &edge_handles,
        &[],
        &feature_id,
        &sphere_op(),
    )
    .expect("seed_primitive_attributes for a sphere should succeed");

    // Step-5 originally pinned faces only; step-8 widened to include edges
    // (covered by step-7's dedicated test). The "one entry per face" contract
    // is still pinned per-handle below; the table simply also holds any edges
    // OCCT produces for the sphere.
    assert_eq!(
        table.len(),
        face_handles.len() + edge_handles.len(),
        "sphere: one entry per face + one per edge (step-5 face contract preserved)"
    );

    for (idx, &face_id) in face_handles.iter().enumerate() {
        let attr = table.lookup(face_id).unwrap_or_else(|| {
            panic!(
                "sphere face #{} (handle {:?}) must have a TopologyAttribute entry",
                idx, face_id
            )
        });
        assert_eq!(
            attr.feature_id, feature_id,
            "sphere face #{idx} feature_id should equal Body#realization[0]"
        );
        assert_eq!(
            attr.role,
            Role::Side,
            "sphere face #{idx} role should be Role::Side (sphere has no caps)"
        );
        assert_eq!(
            attr.local_index, idx as u32,
            "sphere face #{idx} local_index should be the consecutive 0..n value {idx}"
        );
        assert_eq!(
            attr.user_label, None,
            "sphere face #{idx} user_label should be None per task-1 invariant"
        );
        assert!(
            attr.mod_history.is_empty(),
            "sphere face #{idx} mod_history should be empty per task-1 invariant"
        );
    }
}

// ─── step-7: Edge seeding for Box / Cylinder / Sphere ─────────────────────────
//
// Every face arm (Box / Cylinder / Sphere) must also walk `edge_handles` and
// record one `TopologyAttribute` per edge with `Role::NewEdge` and a sequential
// `local_index` (PRD line 66 — construction-order tiebreak for genuine
// geometric ties). This is a single integration test that asserts the
// edges-and-faces co-population contract for all three seedable primitives in
// one place: the seeder writes both kinds of entries in a single call, and
// neither side regresses. The corresponding implementation lands in step-8.

#[test]
fn seed_primitive_attributes_records_new_edge_for_every_extracted_edge() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    // ── Sub-case (1): Box → 12 edges, all Role::NewEdge ──────────────────────
    {
        let mut kernel = OcctKernelHandle::spawn();
        let box_id = kernel.execute(&box_op()).expect("box should build").id;
        let face_handles = kernel
            .extract_faces(box_id)
            .expect("extract_faces(box) should succeed");
        let edge_handles = kernel
            .extract_edges(box_id)
            .expect("extract_edges(box) should succeed");
        assert_eq!(
            edge_handles.len(),
            12,
            "a 10mm box should have exactly 12 edges in TopExp order"
        );
        // Face-count regression guard from step-1: must still be 6.
        assert_eq!(
            face_handles.len(),
            6,
            "step-1 regression: box should still emit 6 faces"
        );

        let feature_id = body_realization_feature_id();
        let mut table = TopologyAttributeTable::default();
        seed_primitive_attributes(
            &mut table,
            &mut kernel,
            &face_handles,
            &edge_handles,
            &[],
            &feature_id,
            &box_op(),
        )
        .expect("seed_primitive_attributes for a box should succeed (step-7: faces + edges)");

        // After step-8 the table holds 6 face entries + 12 edge entries.
        assert_eq!(
            table.len(),
            6 + 12,
            "box: 6 face entries + 12 edge entries (step-7 contract)"
        );

        // Faces still correct (regression guard for step-2's helper).
        for &face_id in face_handles.iter() {
            let attr = table
                .lookup(face_id)
                .expect("box face must still have an entry after step-8");
            assert_eq!(
                attr.role,
                Role::Side,
                "box face role must remain Role::Side after edges were added"
            );
        }

        // Edges: each Role::NewEdge with local_index == idx, default metadata.
        let mut edge_local_indices: HashSet<u32> = HashSet::new();
        for (idx, &edge_id) in edge_handles.iter().enumerate() {
            let attr = table.lookup(edge_id).unwrap_or_else(|| {
                panic!(
                    "box edge #{} (handle {:?}) must have a TopologyAttribute entry",
                    idx, edge_id
                )
            });
            assert_eq!(
                attr.feature_id, feature_id,
                "box edge #{idx} feature_id should equal Body#realization[0]"
            );
            assert_eq!(
                attr.role,
                Role::NewEdge,
                "box edge #{idx} role should be Role::NewEdge"
            );
            assert_eq!(
                attr.local_index, idx as u32,
                "box edge #{idx} local_index should be the consecutive 0..n value {idx}"
            );
            assert_eq!(
                attr.user_label, None,
                "box edge #{idx} user_label should be None per task-1 invariant"
            );
            assert!(
                attr.mod_history.is_empty(),
                "box edge #{idx} mod_history should be empty per task-1 invariant"
            );
            assert!(
                edge_local_indices.insert(attr.local_index),
                "box edge #{idx} has duplicate local_index {}; \
                 each edge must have a unique local_index in 0..12",
                attr.local_index
            );
        }
        assert_eq!(
            edge_local_indices.len(),
            12,
            "box edge local_indices must cover 12 distinct values"
        );
    }

    // ── Sub-case (2): Cylinder → ≥2 edges, all Role::NewEdge ─────────────────
    {
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
        // OCCT cylinder edge count varies (2-3) depending on seam handling;
        // the contract is "at least 2" (the two cap circles are non-negotiable).
        assert!(
            edge_handles.len() >= 2,
            "cylinder should emit at least 2 edges (top + bottom cap circles), got {}",
            edge_handles.len()
        );
        // Face-count regression guard from step-3.
        assert_eq!(
            face_handles.len(),
            3,
            "step-3 regression: cylinder should still emit 3 faces"
        );

        let feature_id = body_realization_feature_id();
        let mut table = TopologyAttributeTable::default();
        seed_primitive_attributes(
            &mut table,
            &mut kernel,
            &face_handles,
            &edge_handles,
            &[],
            &feature_id,
            &cylinder_op(),
        )
        .expect("seed_primitive_attributes for a cylinder should succeed (step-7: faces + edges)");

        assert_eq!(
            table.len(),
            face_handles.len() + edge_handles.len(),
            "cylinder: face entries + edge entries (step-7 contract)"
        );

        // Edges: each Role::NewEdge with sequential local_index.
        let mut edge_local_indices: HashSet<u32> = HashSet::new();
        for (idx, &edge_id) in edge_handles.iter().enumerate() {
            let attr = table.lookup(edge_id).unwrap_or_else(|| {
                panic!(
                    "cylinder edge #{} (handle {:?}) must have a TopologyAttribute entry",
                    idx, edge_id
                )
            });
            assert_eq!(
                attr.feature_id, feature_id,
                "cylinder edge #{idx} feature_id should equal Body#realization[0]"
            );
            assert_eq!(
                attr.role,
                Role::NewEdge,
                "cylinder edge #{idx} role should be Role::NewEdge"
            );
            assert_eq!(
                attr.local_index, idx as u32,
                "cylinder edge #{idx} local_index should be the consecutive 0..n value {idx}"
            );
            assert_eq!(
                attr.user_label, None,
                "cylinder edge #{idx} user_label should be None per task-1 invariant"
            );
            assert!(
                attr.mod_history.is_empty(),
                "cylinder edge #{idx} mod_history should be empty per task-1 invariant"
            );
            assert!(
                edge_local_indices.insert(attr.local_index),
                "cylinder edge #{idx} has duplicate local_index {}",
                attr.local_index
            );
        }
    }

    // ── Sub-case (3): Sphere → 0+ edges, all Role::NewEdge if any ────────────
    {
        let mut kernel = OcctKernelHandle::spawn();
        let sphere_id = kernel
            .execute(&sphere_op())
            .expect("sphere should build")
            .id;
        let face_handles = kernel
            .extract_faces(sphere_id)
            .expect("extract_faces(sphere) should succeed");
        let edge_handles = kernel
            .extract_edges(sphere_id)
            .expect("extract_edges(sphere) should succeed");
        // Face-count regression guard from step-5.
        assert!(
            !face_handles.is_empty(),
            "step-5 regression: sphere should still emit ≥1 face"
        );

        let feature_id = body_realization_feature_id();
        let mut table = TopologyAttributeTable::default();
        seed_primitive_attributes(
            &mut table,
            &mut kernel,
            &face_handles,
            &edge_handles,
            &[],
            &feature_id,
            &sphere_op(),
        )
        .expect("seed_primitive_attributes for a sphere should succeed (step-7: faces + edges)");

        assert_eq!(
            table.len(),
            face_handles.len() + edge_handles.len(),
            "sphere: face entries + edge entries (step-7 contract)"
        );

        // Edges: if any, each must be Role::NewEdge with sequential local_index.
        // OCCT may emit 0 edges (smooth sphere) or a meridian seam — record
        // whatever extract_edges returns. Skip the per-edge assertion only if
        // the kernel returned an empty vector.
        if !edge_handles.is_empty() {
            let mut edge_local_indices: HashSet<u32> = HashSet::new();
            for (idx, &edge_id) in edge_handles.iter().enumerate() {
                let attr = table.lookup(edge_id).unwrap_or_else(|| {
                    panic!(
                        "sphere edge #{} (handle {:?}) must have a TopologyAttribute entry",
                        idx, edge_id
                    )
                });
                assert_eq!(
                    attr.feature_id, feature_id,
                    "sphere edge #{idx} feature_id should equal Body#realization[0]"
                );
                assert_eq!(
                    attr.role,
                    Role::NewEdge,
                    "sphere edge #{idx} role should be Role::NewEdge"
                );
                assert_eq!(
                    attr.local_index, idx as u32,
                    "sphere edge #{idx} local_index should be the consecutive 0..n value {idx}"
                );
                assert_eq!(
                    attr.user_label, None,
                    "sphere edge #{idx} user_label should be None per task-1 invariant"
                );
                assert!(
                    attr.mod_history.is_empty(),
                    "sphere edge #{idx} mod_history should be empty per task-1 invariant"
                );
                assert!(
                    edge_local_indices.insert(attr.local_index),
                    "sphere edge #{idx} has duplicate local_index {}",
                    attr.local_index
                );
            }
        }
    }
}

// ─── task-3633 step-1: Box → 8 corner vertex entries (CornerVertex role) ──────

#[test]
fn seed_primitive_attributes_box_records_eight_corner_vertex_entries_with_distinct_payloads() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let box_id = kernel.execute(&box_op()).expect("box should build").id;

    let face_handles = kernel
        .extract_faces(box_id)
        .expect("extract_faces(box) should succeed");
    let edge_handles = kernel
        .extract_edges(box_id)
        .expect("extract_edges(box) should succeed");
    let vertex_handles = kernel
        .extract_vertices(box_id)
        .expect("extract_vertices(box) should succeed");
    assert_eq!(
        vertex_handles.len(),
        8,
        "a 10mm box should have exactly 8 vertices"
    );

    let feature_id = body_realization_feature_id();
    let mut table = TopologyAttributeTable::default();
    seed_primitive_attributes(
        &mut table,
        &mut kernel,
        &face_handles,
        &edge_handles,
        &vertex_handles,
        &feature_id,
        &box_op(),
    )
    .expect("seed_primitive_attributes for a 10mm box with vertices should succeed");

    // Collect all CornerVertex entries by their (x,y,z) sign triple.
    let mut sign_combo_to_local_index: std::collections::HashMap<
        (AxisSign, AxisSign, AxisSign),
        u32,
    > = std::collections::HashMap::new();

    for (idx, &vertex_id) in vertex_handles.iter().enumerate() {
        let attr = table.lookup(vertex_id).unwrap_or_else(|| {
            panic!(
                "box vertex #{} (handle {:?}) must have a TopologyAttribute entry",
                idx, vertex_id
            )
        });
        assert_eq!(
            attr.feature_id, feature_id,
            "box vertex #{idx} feature_id should equal Body#realization[0]"
        );
        assert_eq!(
            attr.user_label, None,
            "box vertex #{idx} user_label should be None per task-1 invariant"
        );
        assert!(
            attr.mod_history.is_empty(),
            "box vertex #{idx} mod_history should be empty per task-1 invariant"
        );
        match attr.role {
            Role::CornerVertex { x, y, z } => {
                let prev = sign_combo_to_local_index.insert((x, y, z), attr.local_index);
                assert!(
                    prev.is_none(),
                    "box vertex #{idx} has duplicate (x={x:?}, y={y:?}, z={z:?}) sign combo"
                );
            }
            other => panic!(
                "box vertex #{idx} role should be Role::CornerVertex {{ .. }}, got {:?}",
                other
            ),
        }
    }

    // All 8 sign combos must be present.
    let expected_combos: std::collections::HashSet<(AxisSign, AxisSign, AxisSign)> = {
        use AxisSign::{Neg, Pos};
        [
            (Pos, Pos, Pos),
            (Pos, Pos, Neg),
            (Pos, Neg, Pos),
            (Pos, Neg, Neg),
            (Neg, Pos, Pos),
            (Neg, Pos, Neg),
            (Neg, Neg, Pos),
            (Neg, Neg, Neg),
        ]
        .iter()
        .copied()
        .collect()
    };
    let actual_combos: std::collections::HashSet<(AxisSign, AxisSign, AxisSign)> =
        sign_combo_to_local_index.keys().copied().collect();
    assert_eq!(
        actual_combos, expected_combos,
        "box vertex sign combos must cover all 8 (±X, ±Y, ±Z) combinations"
    );

    // Each local_index must be in 0..8 and distinct.
    let mut local_indices: HashSet<u32> = HashSet::new();
    for &li in sign_combo_to_local_index.values() {
        assert!(li < 8, "box vertex local_index {li} must be in 0..8");
        assert!(
            local_indices.insert(li),
            "box vertex local_index {li} appears twice — must be unique across the 8 corners"
        );
    }
    assert_eq!(
        local_indices.len(),
        8,
        "box vertex local_indices must cover 8 distinct values"
    );
}

// ─── task-3633 step-1: Cylinder + Sphere → no vertex entries seeded ───────────

#[test]
fn cylinder_and_sphere_do_not_record_any_vertex_entries() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    // ── Sub-case (1): Cylinder ────────────────────────────────────────────────
    {
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
        let vertex_handles = kernel
            .extract_vertices(cyl_id)
            .expect("extract_vertices(cylinder) should succeed");

        let feature_id = body_realization_feature_id();
        let mut table = TopologyAttributeTable::default();
        seed_primitive_attributes(
            &mut table,
            &mut kernel,
            &face_handles,
            &edge_handles,
            &vertex_handles,
            &feature_id,
            &cylinder_op(),
        )
        .expect("seed_primitive_attributes for a cylinder should succeed");

        // No vertex entries: cylinder has no analytic vertices per PRD §2 Q-MM2-1.
        for (idx, &vertex_id) in vertex_handles.iter().enumerate() {
            assert!(
                table.lookup(vertex_id).is_none(),
                "cylinder vertex #{idx} (handle {:?}) must NOT have an entry — \
                 Cylinder has no analytic vertices per PRD §2 Q-MM2-1",
                vertex_id
            );
        }
    }

    // ── Sub-case (2): Sphere ──────────────────────────────────────────────────
    {
        let mut kernel = OcctKernelHandle::spawn();
        let sphere_id = kernel
            .execute(&sphere_op())
            .expect("sphere should build")
            .id;
        let face_handles = kernel
            .extract_faces(sphere_id)
            .expect("extract_faces(sphere) should succeed");
        let edge_handles = kernel
            .extract_edges(sphere_id)
            .expect("extract_edges(sphere) should succeed");
        let vertex_handles = kernel
            .extract_vertices(sphere_id)
            .expect("extract_vertices(sphere) should succeed");

        let feature_id = body_realization_feature_id();
        let mut table = TopologyAttributeTable::default();
        seed_primitive_attributes(
            &mut table,
            &mut kernel,
            &face_handles,
            &edge_handles,
            &vertex_handles,
            &feature_id,
            &sphere_op(),
        )
        .expect("seed_primitive_attributes for a sphere should succeed");

        // No vertex entries: sphere has no analytic vertices per PRD §2 Q-MM2-1.
        for (idx, &vertex_id) in vertex_handles.iter().enumerate() {
            assert!(
                table.lookup(vertex_id).is_none(),
                "sphere vertex #{idx} (handle {:?}) must NOT have an entry — \
                 Sphere has no analytic vertices per PRD §2 Q-MM2-1",
                vertex_id
            );
        }
    }
}

// ─── task-3633 step-3: seed_primitive_attributes_for_handle extracts + seeds vertices ─

#[test]
fn seed_primitive_attributes_for_handle_box_extracts_and_seeds_vertices_too() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let box_id = kernel.execute(&box_op()).expect("box should build").id;

    // Verify extract_vertices returns 8 handles directly (precondition).
    let vertex_handles = kernel
        .extract_vertices(box_id)
        .expect("extract_vertices(box) should succeed");
    assert_eq!(
        vertex_handles.len(),
        8,
        "extract_vertices(box_id) must return exactly 8 handles"
    );

    // Call the wrapper — it should extract vertices internally for Box ops
    // and pass them through to seed_primitive_attributes, populating the
    // 8 CornerVertex entries.
    let feature_id = body_realization_feature_id();
    let mut table = TopologyAttributeTable::default();
    seed_primitive_attributes_for_handle(&mut table, &mut kernel, box_id, &feature_id, &box_op())
        .expect("seed_primitive_attributes_for_handle(box) should succeed");

    // Each manually-extracted vertex handle must now have a CornerVertex entry.
    for (idx, &vertex_id) in vertex_handles.iter().enumerate() {
        let attr = table.lookup(vertex_id).unwrap_or_else(|| {
            panic!(
                "box vertex #{} (handle {:?}) must have a CornerVertex entry after \
                 seed_primitive_attributes_for_handle",
                idx, vertex_id
            )
        });
        assert!(
            matches!(attr.role, Role::CornerVertex { .. }),
            "box vertex #{idx} role must be Role::CornerVertex {{..}}, got {:?}",
            attr.role
        );
        assert_eq!(
            attr.feature_id, feature_id,
            "box vertex #{idx} feature_id must equal Body#realization[0]"
        );
        assert_eq!(
            attr.user_label, None,
            "box vertex #{idx} user_label must be None"
        );
        assert!(
            attr.mod_history.is_empty(),
            "box vertex #{idx} mod_history must be empty"
        );
    }
}
