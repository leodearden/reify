//! Integration test for `OcctKernelHandle::extrude_with_history` —
//! the v0.2 persistent-naming-v2 sweep history-tracking primitive
//! for `BRepPrimAPI_MakePrism` (task 5a / #2573, step-7).
//!
//! Exercises the FFI primitive that wraps `BRepPrimAPI_MakePrism::Modified()`,
//! `Generated()`, `IsDeleted()`, `FirstShape()`, and `LastShape()` and
//! exposes the per-parent face/edge correspondence plus the cap-face
//! identification (start/end caps from FirstShape/LastShape).
//!
//! Mirrors the structure of `boolean_op_history_integration.rs`: gated on
//! `OCCT_AVAILABLE` and `#![cfg(has_occt)]` so non-OCCT builds skip without
//! linker errors.

#![cfg(has_occt)]

use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_types::{GeometryQuery};

/// 10×10mm rectangular face profile, expressed in SI metres.
const RECT_WIDTH_M: f64 = 10.0e-3;
const RECT_HEIGHT_M: f64 = 10.0e-3;
/// Extrude distance in +Z.
const PRISM_DISTANCE_M: f64 = 5.0e-3;

/// Build a 10×10mm rect_face profile via the kernel's primitive
/// constructor (the source language has no face primitive; this test
/// uses the kernel-direct primitive, which is how `OcctKernel::execute`
/// would build the profile at the FFI boundary).
fn make_rect_profile(kernel: &mut OcctKernelHandle) -> reify_types::GeometryHandleId {
    // The kernel API exposes profile-construction via `make_rect_face`
    // through the kernel's internal FFI, but at the Rust handle level
    // there is no `GeometryOp::RectFace` source-level primitive. The
    // tests in `crates/reify-kernel-occt/src/lib.rs` use the FFI
    // directly. We need a public path: the closest-equivalent for an
    // integration test is to use a FaceProfile via the inherent helper.
    //
    // Fallback: build the profile inside the kernel thread. We expose
    // a dedicated test helper through a public method on the handle
    // for this purpose. If unavailable, this test is gated out at
    // compile time; see `OcctKernelHandle::make_rect_profile_for_test`.
    kernel
        .make_rect_profile_for_test(RECT_WIDTH_M, RECT_HEIGHT_M)
        .expect("rect profile should build")
}

/// `BRepPrimAPI_MakePrism` history exposes Modified/Generated/Deleted for
/// each profile face/edge/vertex AND FirstShape/LastShape caps. The test:
///
/// - builds a 10×10mm rect_face profile;
/// - calls `extrude_with_history(profile, 5mm)`;
/// - asserts the result is a positive-volume solid;
/// - asserts the cap-index lists each contain exactly one entry (one face
///   per cap for a rect profile);
/// - asserts at least 4 generated face records (4 profile edges → 4 lateral
///   faces);
/// - asserts every record references in-range profile and result indices.
///
/// Compilation/linkage of this test pins step-8: it will fail to build
/// until the FFI primitive + Rust handle method ship.
#[test]
fn extrude_with_history_reports_caps_and_side_face_records() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();
    let profile_id = make_rect_profile(&mut kernel);

    let (result_handle, history) = kernel
        .extrude_with_history(profile_id, PRISM_DISTANCE_M)
        .expect("extrude_with_history should succeed for a rect_face profile");

    // (a) Result is a positive-volume solid.
    let vol = kernel
        .query(&GeometryQuery::Volume(result_handle))
        .expect("volume query on the prism result should succeed");
    let vol_si = vol.as_f64().expect("volume value should be numeric");
    assert!(
        vol_si > 0.0,
        "extruded prism must have positive volume, got {vol_si}"
    );
    // 10mm × 10mm × 5mm = 500 mm³ = 5e-7 m³ (allow 1% slack for FFI rounding).
    let expected_si = RECT_WIDTH_M * RECT_HEIGHT_M * PRISM_DISTANCE_M;
    assert!(
        (vol_si - expected_si).abs() / expected_si < 0.01,
        "prism volume ≈ width*height*distance: expected {expected_si}, got {vol_si}"
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
        "expected ≥4 generated faces (4 profile edges → ≥4 lateral side faces), \
         got {} ({:?})",
        history.face_generated.len(),
        history.face_generated
    );

    // (e) Every generated record's parent_subshape_index must be in-range
    //     for the profile (rect has 4 edges).
    for r in &history.face_generated {
        assert_eq!(
            r.parent_index, 0,
            "sweep records always have parent_index=0, got {}",
            r.parent_index
        );
        assert!(
            r.parent_subshape_index < 8, // 4 edges + 4 vertices, cap-loose bound
            "parent_subshape_index {} out of range for rect profile (≥8)",
            r.parent_subshape_index
        );
    }

    // (f) Result-side indices must be within the result face/edge maps.
    let result_faces = kernel
        .extract_faces(result_handle)
        .expect("extract_faces on the prism result should succeed");
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
        .expect("extract_edges on the prism result should succeed");
    let result_edge_count = result_edges.len() as u32;
    for r in history
        .edge_modified
        .iter()
        .chain(history.edge_generated.iter())
    {
        assert!(
            r.parent_index == 0,
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
}
