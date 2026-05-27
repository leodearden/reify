//! Integration test for `OcctKernelHandle::sweep_with_history` —
//! the v0.2 persistent-naming-v2 sweep history-tracking primitive
//! for `BRepOffsetAPI_MakePipe` (task 5b / #2619, step-3).
//!
//! Exercises the FFI primitive that wraps `BRepOffsetAPI_MakePipe::Modified()`,
//! `Generated()`, `IsDeleted()`, `FirstShape()`, and `LastShape()` and
//! exposes the per-parent face/edge correspondence plus the cap-face
//! identification (start/end caps from FirstShape/LastShape). Because
//! `BRepOffsetAPI_MakePipe` inherits from `BRepBuilderAPI_MakeShape`,
//! the same templated C++ helpers used by extrude / revolve are reused
//! verbatim — sweep is single-parent like those ops.
//!
//! Mirrors the structure of `extrude_with_history_integration.rs`: gated
//! on `OCCT_AVAILABLE` and `#![cfg(has_occt)]` so non-OCCT builds skip
//! without linker errors.

#![cfg(has_occt)]

use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery};

/// 10×10mm rectangular face profile, expressed in SI metres.
const RECT_WIDTH_M: f64 = 10.0e-3;
const RECT_HEIGHT_M: f64 = 10.0e-3;
/// Sweep spine length in +Z (30mm).
const SPINE_LENGTH_M: f64 = 30.0e-3;

/// Build a 10×10mm rect_face profile using the public test-fixture
/// helper (see `OcctKernelHandle::make_rect_profile_for_test`).
fn make_rect_profile(kernel: &mut OcctKernelHandle) -> GeometryHandleId {
    kernel
        .make_rect_profile_for_test(RECT_WIDTH_M, RECT_HEIGHT_M)
        .expect("rect profile should build")
}

/// Build a straight `+Z` spine of length `SPINE_LENGTH_M` from
/// (0,0,0) to (0,0,L) via `GeometryOp::LineSegment`.
fn make_straight_spine(kernel: &mut OcctKernelHandle) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::LineSegment {
            x1: 0.0,
            y1: 0.0,
            z1: 0.0,
            x2: 0.0,
            y2: 0.0,
            z2: SPINE_LENGTH_M,
        })
        .expect("LineSegment (spine) creation should succeed")
        .id
}

/// `BRepOffsetAPI_MakePipe` history exposes Modified/Generated/Deleted
/// for each profile face/edge/vertex AND FirstShape/LastShape caps. The
/// test:
///
/// - builds a 10×10mm rect_face profile;
/// - builds a straight 30mm +Z spine;
/// - calls `sweep_with_history(profile, spine)`;
/// - asserts the result is a positive-volume solid;
/// - asserts the cap-index lists each contain exactly one entry (one
///   face per cap for a rect profile);
/// - asserts at least 4 generated face records (4 profile edges → 4
///   lateral faces);
/// - asserts every record references in-range profile and result indices.
///
/// Compilation/linkage of this test pins step-4: it will fail to build
/// until the FFI primitive + Rust handle method ship.
#[test]
fn sweep_with_history_reports_caps_and_swept_face_records() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let profile_id = make_rect_profile(&mut kernel);
    let spine_id = make_straight_spine(&mut kernel);

    let (result_handle, history) = kernel
        .sweep_with_history(profile_id, spine_id)
        .expect("sweep_with_history should succeed for a rect_face + straight spine");

    // (a) Result is a positive-volume solid.
    let vol = kernel
        .query(&GeometryQuery::Volume(result_handle))
        .expect("volume query on the swept result should succeed");
    let vol_si = vol.as_f64().expect("volume value should be numeric");
    assert!(
        vol_si > 0.0,
        "swept solid must have positive volume, got {vol_si}"
    );
    // 10mm × 10mm × 30mm = 3000 mm³ = 3e-6 m³ (allow 1% slack for FFI rounding).
    let expected_si = RECT_WIDTH_M * RECT_HEIGHT_M * SPINE_LENGTH_M;
    assert!(
        (vol_si - expected_si).abs() / expected_si < 0.01,
        "swept volume ≈ width*height*spine_length: expected {expected_si}, got {vol_si}"
    );

    // (b) Start cap: rect_face has one face → exactly one start cap face.
    assert_eq!(
        history.start_cap_face_indices.len(),
        1,
        "expected 1 start cap face for a rect profile, got {} ({:?})",
        history.start_cap_face_indices.len(),
        history.start_cap_face_indices
    );

    // (c) End cap: same — exactly one end cap face.
    assert_eq!(
        history.end_cap_face_indices.len(),
        1,
        "expected 1 end cap face for a rect profile, got {} ({:?})",
        history.end_cap_face_indices.len(),
        history.end_cap_face_indices
    );

    // (d) face_generated: at least 4 lateral faces (one per profile edge).
    assert!(
        history.face_generated.len() >= 4,
        "expected ≥4 generated faces (4 profile edges → ≥4 lateral swept faces), \
         got {} ({:?})",
        history.face_generated.len(),
        history.face_generated
    );

    // (e) Every generated record's parent_index is 0 (sweep is single-parent).
    for r in &history.face_generated {
        assert_eq!(
            r.parent_index, 0,
            "sweep records always have parent_index=0, got {}",
            r.parent_index
        );
    }

    // (f) Result-side indices must be within the result face/edge maps.
    let result_faces = kernel
        .extract_faces(result_handle)
        .expect("extract_faces on the swept result should succeed");
    let result_face_count = result_faces.len() as u32;
    for r in history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
    {
        assert!(
            r.result_subshape_index < result_face_count,
            "face record result_subshape_index {} out of range; result has {} faces",
            r.result_subshape_index,
            result_face_count
        );
    }
    for &cap_idx in history
        .start_cap_face_indices
        .iter()
        .chain(history.end_cap_face_indices.iter())
    {
        assert!(
            cap_idx < result_face_count,
            "cap face index {} out of range; result has {} faces",
            cap_idx,
            result_face_count
        );
    }

    let result_edges = kernel
        .extract_edges(result_handle)
        .expect("extract_edges on the swept result should succeed");
    let result_edge_count = result_edges.len() as u32;
    for r in history
        .edge_modified
        .iter()
        .chain(history.edge_generated.iter())
    {
        assert_eq!(
            r.parent_index, 0,
            "sweep edge record parent_index must be 0, got {}",
            r.parent_index
        );
        assert!(
            r.result_subshape_index < result_edge_count,
            "edge record result_subshape_index {} out of range; result has {} edges",
            r.result_subshape_index,
            result_edge_count
        );
    }

    // (g) Silent-drop counter must be zero for a well-formed pipe sweep.
    assert_eq!(
        history.silent_drop_count, 0,
        "vanilla pipe sweep should not silently drop any Modified/Generated child — got {}",
        history.silent_drop_count
    );
}
