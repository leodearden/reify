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
//! Step-17 RED scope: every test in this file fails today because
//! `OcctKernel::query` for `FaceSurfaceKind` / `EdgeCurveKind` returns
//! the step-14 stub `QueryFailed("face_surface_kind not yet wired" /
//! "edge_curve_kind not yet wired")`. Step-18 will replace the stubs
//! with FFI calls to OCCT's `BRepAdaptor_Surface::GetType()` and
//! `BRepAdaptor_Curve::GetType()` and these tests must turn green.

use reify_eval::selector_vocabulary_v2::{adjacent_to_face, owner_body_of};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_types::{
    GeometryKernel, GeometryOp, GeometryQuery, Value,
};

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
        other => panic!(
            "{ctx}: expected Value::String(kind_name), got {other:?}"
        ),
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

    let mut kernel = OcctKernelHandle::spawn();
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

    let mut kernel = OcctKernelHandle::spawn();
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
            panic!(
                "adjacent_to_face(box, face[{i}]={face_id:?}) should succeed, got {e:?}"
            )
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

    let mut kernel = OcctKernelHandle::spawn();
    let box_id = kernel
        .execute(&ten_mm_box_op())
        .expect("10mm box should build via OCCT")
        .id;
    let face_handles = kernel
        .extract_faces(box_id)
        .expect("extract_faces(box) should succeed");

    for (i, face_id) in face_handles.iter().enumerate() {
        let parent = owner_body_of(&kernel, *face_id).unwrap_or_else(|e| {
            panic!(
                "owner_body_of(face[{i}]={face_id:?}) should succeed, got {e:?}"
            )
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

    let mut kernel = OcctKernelHandle::spawn();
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
            panic!(
                "owner_body_of(edge[{i}]={edge_id:?}) should succeed, got {e:?}"
            )
        });
        assert_eq!(
            parent, box_id,
            "box edge {i} ({edge_id:?}) must resolve to box_id {box_id:?}, got {parent:?}"
        );
    }
}
