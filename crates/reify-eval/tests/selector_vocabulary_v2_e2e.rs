//! End-to-end integration tests for the v0.2 selector vocabulary v2
//! (task 2658, PRD `docs/prds/v0_2/persistent-naming-v2.md` task 10).
//!
//! Mock-kernel coverage of the in-process selector logic
//! (combinators, direction filters, extremals, history/attribute
//! selectors, geometry-type filters) lives in
//! `selector_vocabulary_v2_mock.rs`. This file's purpose is to prove
//! the kernel-side wiring — particularly the new
//! `GeometryQuery::FaceSurfaceKind` / `GeometryQuery::EdgeCurveKind`
//! variants — works against the real OCCT FFI surface, with handles
//! allocated by [`OcctKernelHandle`] rather than hand-built.
//!
//! Pattern after `topology_attribute_resolver_e2e.rs` and
//! `topology_attribute_primitives_direct.rs`: same `OCCT_AVAILABLE`
//! gate, same `BOX_SIDE_M = 10e-3` constant, same "extract face/edge
//! handles ONCE and reuse" discipline (each `extract_*` allocates fresh
//! kernel handle ids).
//!
//! These tests cover the OCCT FFI wiring landed in steps 17–18
//! (`face_surface_kind` / `edge_curve_kind`), the `adjacent_to_face`
//! integration (steps 25–26), the `owner_body` provenance (step-29),
//! and the compositional smoke chain (step-31).

use reify_eval::selector_vocabulary_v2::{adjacent_to_face, owner_body_of};
// Step-31 / step-32 cross-reference: the compositional smoke test below
// imports the v2 vocabulary via the top-level `reify_eval::{…}` surface
// (the `pub use` re-exports finalised in step-32) rather than through
// `reify_eval::selector_vocabulary_v2::*`. This pins the public API
// path that downstream callers (`.ri` language wiring, future
// integration tests) are expected to use.
use reify_eval::{
    Axis, ExtremalSense, created_by_feature, extremal_by_centroid, faces_by_surface_kind,
    has_user_label, intersect, siblings_of_face, user_label_eq,
};
use reify_ir::{
    CapKind, FaceSurfaceKind, FeatureId, GeometryHandleId, GeometryOp, GeometryQuery, Role,
    TopologyAttribute, TopologyAttributeTable, Value,
};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};

/// 10×10×10 mm box, expressed in SI metres at the kernel boundary.
const BOX_SIDE_M: f64 = 10.0e-3;

/// 5mm-radius / 10mm-height cylinder for the surface-kind classification
/// tests (matches the cylinder fixture in
/// `topology_attribute_primitives_direct.rs`).
const CYL_RADIUS_M: f64 = 5.0e-3;
const CYL_HEIGHT_M: f64 = 10.0e-3;

fn ten_mm_box_op() -> GeometryOp {
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

/// Extract the canonical kind-name string from a `Value::String` reply,
/// failing the test with a clear diagnostic on any other shape.
fn unwrap_kind_string(value: &Value, ctx: &str) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => panic!("{ctx}: expected Value::String(kind_name), got {other:?}"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FaceSurfaceKind on a 10mm box — every face must classify as "Plane"
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn face_surface_kind_classifies_box_faces_as_plane() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let kernel = OcctKernelHandle::spawn();
    let box_id = kernel
        .execute(&ten_mm_box_op())
        .expect("10mm box should build via OCCT")
        .id;
    let face_handles = kernel
        .extract_faces(box_id)
        .expect("extract_faces(box) should succeed");
    assert_eq!(
        face_handles.len(),
        6,
        "a 10mm box must have exactly 6 faces in TopExp order"
    );

    // Each box face is a planar surface — OCCT must classify all six as
    // "Plane" (canonical name, decoded by `FaceSurfaceKind::try_from_str`
    // into `FaceSurfaceKind::Plane`).
    for (i, face_id) in face_handles.iter().enumerate() {
        let value = kernel
            .query(&GeometryQuery::FaceSurfaceKind(*face_id))
            .unwrap_or_else(|e| {
                panic!(
                    "FaceSurfaceKind({face_id:?}) for box face {i} should succeed once OCCT FFI is wired, got {e:?}"
                )
            });
        let name = unwrap_kind_string(&value, &format!("FaceSurfaceKind({face_id:?})"));
        assert_eq!(
            name, "Plane",
            "box face {i} ({face_id:?}) must classify as Plane, got {name:?}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FaceSurfaceKind on a cylinder — exactly two planar caps and at least one
// cylindrical lateral face. OCCT may emit one or more lateral faces depending
// on internal seam handling; the integration contract is "≥1 Cylinder + 2
// Plane" rather than a tight count.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn face_surface_kind_classifies_cylinder_caps_and_lateral() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let kernel = OcctKernelHandle::spawn();
    let cyl_id = kernel
        .execute(&cylinder_op())
        .expect("5mm/10mm cylinder should build via OCCT")
        .id;
    let face_handles = kernel
        .extract_faces(cyl_id)
        .expect("extract_faces(cylinder) should succeed");
    assert!(
        !face_handles.is_empty(),
        "a closed cylinder must have at least one extractable face"
    );

    let mut plane_count = 0usize;
    let mut cylinder_count = 0usize;
    let mut other = Vec::new();
    for face_id in &face_handles {
        let value = kernel
            .query(&GeometryQuery::FaceSurfaceKind(*face_id))
            .unwrap_or_else(|e| {
                panic!(
                    "FaceSurfaceKind({face_id:?}) for cylinder face should succeed once OCCT FFI is wired, got {e:?}"
                )
            });
        let name = unwrap_kind_string(&value, &format!("FaceSurfaceKind({face_id:?})"));
        match name.as_str() {
            "Plane" => plane_count += 1,
            "Cylinder" => cylinder_count += 1,
            kind => other.push(kind.to_string()),
        }
    }

    assert_eq!(
        plane_count, 2,
        "cylinder must have exactly 2 planar caps; saw {plane_count} (other kinds: {other:?})"
    );
    assert!(
        cylinder_count >= 1,
        "cylinder must have at least 1 cylindrical lateral face; saw {cylinder_count} (other kinds: {other:?})"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// EdgeCurveKind on a 10mm box — all 12 edges must classify as "Line"
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn edge_curve_kind_classifies_box_edges_as_line() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let kernel = OcctKernelHandle::spawn();
    let box_id = kernel
        .execute(&ten_mm_box_op())
        .expect("10mm box should build via OCCT")
        .id;
    let edge_handles = kernel
        .extract_edges(box_id)
        .expect("extract_edges(box) should succeed");
    assert_eq!(
        edge_handles.len(),
        12,
        "a 10mm box must have exactly 12 edges in TopExp order"
    );

    for (i, edge_id) in edge_handles.iter().enumerate() {
        let value = kernel
            .query(&GeometryQuery::EdgeCurveKind(*edge_id))
            .unwrap_or_else(|e| {
                panic!(
                    "EdgeCurveKind({edge_id:?}) for box edge {i} should succeed once OCCT FFI is wired, got {e:?}"
                )
            });
        let name = unwrap_kind_string(&value, &format!("EdgeCurveKind({edge_id:?})"));
        assert_eq!(
            name, "Line",
            "box edge {i} ({edge_id:?}) must classify as Line, got {name:?}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// adjacent_to_face on a 10mm box — every face is adjacent to exactly 4 others
// (the four side faces of a cube). The union of all neighbours over all 6
// faces must cover the full extracted face list (every face is adjacent to
// every other face except its opposite).
//
// This e2e test proves the v0.1 `extract_faces` ↔ `AdjacentFaces` index
// mapping (1-based `face_map.FindKey(i+1)` ↔ 0-based slot in the returned
// Vec) is preserved for the v2 selector. No FFI changes are required for
// step-26 — the test should pass on first run, validating that the v2
// selector layers cleanly on the existing v0.1 OCCT primitives.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn adjacent_to_face_box_each_face_has_four_neighbours() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let box_id = kernel
        .execute(&ten_mm_box_op())
        .expect("10mm box should build via OCCT")
        .id;
    let face_handles = kernel
        .extract_faces(box_id)
        .expect("extract_faces(box) should succeed");
    assert_eq!(
        face_handles.len(),
        6,
        "a 10mm box must have exactly 6 faces in TopExp order"
    );

    // For every face, adjacent_to_face must return exactly 4 face handles
    // (the four side neighbours), all of which are in the canonical
    // extract_faces output and none of which is the queried face itself.
    for (i, face_id) in face_handles.iter().enumerate() {
        let neighbours = adjacent_to_face(&mut kernel, box_id, *face_id).unwrap_or_else(|e| {
            panic!("adjacent_to_face(box, face[{i}]={face_id:?}) should succeed, got {e:?}")
        });
        assert_eq!(
            neighbours.len(),
            4,
            "box face {i} ({face_id:?}) should be adjacent to exactly 4 faces, got {neighbours:?}"
        );
        for n in &neighbours {
            assert!(
                face_handles.contains(n),
                "neighbour {n:?} of box face {i} must be in extract_faces output"
            );
            assert!(
                *n != *face_id,
                "neighbour list must not include the queried face {face_id:?}"
            );
        }
    }
}

#[test]
fn adjacent_to_face_box_neighbours_cover_all_other_faces() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let box_id = kernel
        .execute(&ten_mm_box_op())
        .expect("10mm box should build via OCCT")
        .id;
    let face_handles = kernel
        .extract_faces(box_id)
        .expect("extract_faces(box) should succeed");

    // The union of neighbour-sets over all 6 faces must cover every face
    // except (potentially) each face's own opposite — but since every
    // face appears as a neighbour of 4 of the other 5, the *union* must
    // cover all 6 faces. (Each face appears in 4 neighbour-sets.)
    let mut seen = std::collections::HashSet::new();
    for face_id in &face_handles {
        let neighbours = adjacent_to_face(&mut kernel, box_id, *face_id)
            .expect("adjacent_to_face on a box face should succeed");
        for n in neighbours {
            seen.insert(n);
        }
    }
    assert_eq!(
        seen.len(),
        6,
        "union of all neighbour-sets must cover every box face (got {} of 6)",
        seen.len()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// owner_body_of on a 10mm box — every face / edge sub-handle resolves to the
// original box solid handle. Confirms the kernel records the parent on every
// `extract_*` call (the provenance contract).
//
// This test will FAIL until step-30 lands the parent_handle map on
// OcctKernel + the OwnerBody query routing.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn owner_body_of_box_face_resolves_to_box() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let kernel = OcctKernelHandle::spawn();
    let box_id = kernel
        .execute(&ten_mm_box_op())
        .expect("10mm box should build via OCCT")
        .id;
    let face_handles = kernel
        .extract_faces(box_id)
        .expect("extract_faces(box) should succeed");

    for (i, face_id) in face_handles.iter().enumerate() {
        let parent = owner_body_of(&kernel, *face_id).unwrap_or_else(|e| {
            panic!("owner_body_of(face[{i}]={face_id:?}) should succeed, got {e:?}")
        });
        assert_eq!(
            parent, box_id,
            "box face {i} ({face_id:?}) must resolve to box_id {box_id:?}, got {parent:?}"
        );
    }
}

#[test]
fn owner_body_of_box_edge_resolves_to_box() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let kernel = OcctKernelHandle::spawn();
    let box_id = kernel
        .execute(&ten_mm_box_op())
        .expect("10mm box should build via OCCT")
        .id;
    let edge_handles = kernel
        .extract_edges(box_id)
        .expect("extract_edges(box) should succeed");
    assert_eq!(
        edge_handles.len(),
        12,
        "a 10mm box must have exactly 12 edges in TopExp order"
    );

    for (i, edge_id) in edge_handles.iter().enumerate() {
        let parent = owner_body_of(&kernel, *edge_id).unwrap_or_else(|e| {
            panic!("owner_body_of(edge[{i}]={edge_id:?}) should succeed, got {e:?}")
        });
        assert_eq!(
            parent, box_id,
            "box edge {i} ({edge_id:?}) must resolve to box_id {box_id:?}, got {parent:?}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Compositional smoke test (PRD task 10, lines 74-82) — exercise a realistic
// chain of v2 selectors against a 10mm box without per-call extract churn.
//
// Pipeline:
//   1. extract_faces(box) once.
//   2. faces_by_surface_kind(_, _, Plane) — all 6 faces of a box are planar,
//      so this returns the full slice (the `%Plane` filter is identity here
//      but pins the FFI-backed surface-kind classification end-to-end).
//   3. extremal_by_centroid(_, &planar, Z, Max, 1e-6) — picks the unique
//      top face (centroid Z = +5e-3 m, the next-highest pair sit at 0).
//   4. owner_body_of(_, top[0]) — must round-trip back to the original
//      `box_id`, demonstrating the parent_handle provenance map records
//      every `extract_faces` child.
//   5. siblings_of_face(_, box, top[0]) — returns the other 5 box faces.
//   6. intersect(planar, &[top[0]]) — pure-Rust combinator, must return
//      the singleton top face (proves combinators compose with the
//      kernel-side filters without extra extraction calls).
//
// Step-31 RED: the `use reify_eval::{…}` import block above pulls v2
// vocabulary through the top-level re-exports promised by step-32; until
// step-32 lands those `pub use` declarations the test fails to compile.
// Step-32 (GREEN) adds the re-exports and turns this test green.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn compositional_smoke_box_top_planar_face_chain() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let box_id = kernel
        .execute(&ten_mm_box_op())
        .expect("10mm box should build via OCCT")
        .id;

    // (1) Single upfront extract_faces — the v2 vocabulary chains over
    // this slice without re-extracting at every step.
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces(box) should succeed");
    assert_eq!(faces.len(), 6, "a 10mm box must have 6 faces");

    // (2) %Plane filter — all 6 box faces are planar.
    let planar = faces_by_surface_kind(&mut kernel, box_id, FaceSurfaceKind::Plane)
        .expect("faces_by_surface_kind(_, Plane) should succeed");
    assert_eq!(
        planar.len(),
        6,
        "every face of a box is planar; got {} (faces = {faces:?})",
        planar.len()
    );

    // (3) Pick the unique top face by centroid Z. With OCCT's default box
    // (origin at one corner, body extends along +X/+Y/+Z), the top face's
    // centroid sits at z = BOX_SIDE_M / 2 + half-thickness offset; the
    // bottom and sides have lower centroid Z. The cluster within 1e-6 m
    // of the global max must therefore be a singleton.
    let top = extremal_by_centroid(&mut kernel, &planar, Axis::Z, ExtremalSense::Max, 1e-6)
        .expect("extremal_by_centroid(_, Z, Max) should succeed");
    assert_eq!(
        top.len(),
        1,
        "expected unique top face; got cluster of {} (cluster = {top:?})",
        top.len()
    );
    let top_face_h = top[0];

    // (4) owner_body_of must round-trip the sub-handle back to the box.
    let parent =
        owner_body_of(&kernel, top_face_h).expect("owner_body_of(top_face) should succeed");
    assert_eq!(
        parent, box_id,
        "top face must resolve to box_id; got {parent:?}"
    );

    // (5) siblings_of_face: 6 - 1 = 5 elements, none of which is `top_face_h`.
    let sibs = siblings_of_face(&mut kernel, box_id, top_face_h)
        .expect("siblings_of_face(_, top_face) should succeed");
    assert_eq!(
        sibs.len(),
        5,
        "siblings_of_face must return 5 of the 6 faces; got {} (sibs = {sibs:?})",
        sibs.len()
    );
    assert!(
        !sibs.contains(&top_face_h),
        "siblings list must not include the queried face {top_face_h:?}"
    );

    // (6) Pure-Rust combinator threading: intersect(planar, [top_face_h])
    // must yield the singleton top face — proves the combinators compose
    // with the kernel-side filters without an extra extraction call.
    let chained = intersect(&planar, &[top_face_h]);
    assert_eq!(
        chained,
        vec![top_face_h],
        "intersect(planar, [top]) must collapse to the top face singleton"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Compositional smoke test — TopologyAttributeTable filters
// (`created_by_feature` and `has_user_label` / `user_label_eq`) over the
// extract_faces output of a 10mm box. The table is seeded by hand to model
// the auto-attribute scheme (PRD line 82); the .ri language wiring sits
// downstream of this contract.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn compositional_smoke_attribute_filters_over_box_faces() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let kernel = OcctKernelHandle::spawn();
    let box_id = kernel
        .execute(&ten_mm_box_op())
        .expect("10mm box should build via OCCT")
        .id;
    let faces: Vec<GeometryHandleId> = kernel
        .extract_faces(box_id)
        .expect("extract_faces(box) should succeed");
    assert_eq!(faces.len(), 6);

    // Seed an attribute table modelling the v0.2 auto-attribute scheme:
    // every face is `created_by` the box feature; faces[0] additionally
    // carries a user label "top".
    let box_feature = FeatureId::new("box-2658-smoke");
    let other_feature = FeatureId::new("not-a-real-feature");

    let mut table = TopologyAttributeTable::default();
    for (i, fid) in faces.iter().enumerate() {
        let user_label = if i == 0 {
            Some("top".to_string())
        } else {
            None
        };
        table.record(
            *fid,
            TopologyAttribute {
                feature_id: box_feature.clone(),
                role: Role::Cap(CapKind::Top),
                local_index: i as u32,
                user_label,
                mod_history: Vec::new(),
            },
        );
    }

    // created_by_feature(box_feature) must return all 6 faces in order.
    let by_box = created_by_feature(&table, &faces, &box_feature);
    assert_eq!(
        by_box, faces,
        "created_by_feature(box) must return all box faces in encounter order"
    );

    // created_by_feature(other) must return nothing.
    let by_other = created_by_feature(&table, &faces, &other_feature);
    assert!(
        by_other.is_empty(),
        "created_by_feature(other_feature) must return [] when no face matches; got {by_other:?}"
    );

    // has_user_label must pull out the single labelled face.
    let labelled = has_user_label(&table, &faces);
    assert_eq!(
        labelled,
        vec![faces[0]],
        "has_user_label must surface only the face with a user_label"
    );

    // user_label_eq("top") must match the same face exactly.
    let top_by_label = user_label_eq(&table, &faces, "top");
    assert_eq!(
        top_by_label,
        vec![faces[0]],
        "user_label_eq(\"top\") must surface only the face labelled \"top\""
    );

    // user_label_eq("nope") returns [].
    let none_match = user_label_eq(&table, &faces, "nope");
    assert!(
        none_match.is_empty(),
        "user_label_eq(\"nope\") must return [] when no face carries that label; got {none_match:?}"
    );
}
