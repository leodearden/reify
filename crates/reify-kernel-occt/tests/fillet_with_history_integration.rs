//! Integration test for `OcctKernelHandle::fillet_with_history` —
//! the v0.2 persistent-naming-v2 local-feature history-tracking primitive
//! for `BRepFilletAPI_MakeFillet` (task 2655, step-1/step-2).
//!
//! Exercises the FFI primitive that wraps `BRepFilletAPI_MakeFillet::Modified()`,
//! `Generated()`, and `IsDeleted()` and exposes the per-parent face/edge
//! correspondence for face and edge topology.
//!
//! Mirrors the structure of `boolean_op_history_integration.rs`: gated on
//! `OCCT_AVAILABLE` and `#![cfg(has_occt)]` so non-OCCT builds skip without
//! linker errors.

#![cfg(has_occt)]

use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_types::{GeometryOp, GeometryQuery, Value};

/// 10×10×10 mm box, expressed in SI metres at the kernel boundary.
const BOX_SIDE_M: f64 = 10.0e-3;

/// Fillet radius: 1 mm. Small enough that every edge gets a fillet face
/// without geometric collapse on a 10mm cube.
const FILLET_RADIUS_M: f64 = 1.0e-3;

/// Build the `GeometryOp::Box` for a 10mm cube.
fn ten_mm_box_op() -> GeometryOp {
    GeometryOp::Box {
        width: Value::Real(BOX_SIDE_M),
        height: Value::Real(BOX_SIDE_M),
        depth: Value::Real(BOX_SIDE_M),
    }
}

/// `BRepFilletAPI_MakeFillet` history exposes Modified/Generated/Deleted for
/// each parent face and edge. The test:
///
/// - builds a 10×10×10mm box;
/// - calls `fillet_with_history(box_id, 1mm)`;
/// - asserts result volume positive and < original (fillet removes material);
/// - asserts `face_modified` non-empty (every box face is trimmed by adjacent fillets);
/// - asserts `face_generated` non-empty (each filleted edge generates a fillet face);
/// - asserts every record has `parent_index == 0`;
/// - asserts `face_modified.parent_subshape_index < 6` (box has 6 faces);
/// - asserts `face_generated.parent_subshape_index < 12` (fillet lateral faces
///   are generated FROM edges; box has 12 edges);
/// - asserts every `result_subshape_index` is in-range for the result shape.
///
/// Compilation/linkage of this test pins step-2: it will fail to build
/// until the FFI primitive + Rust handle method ship.
#[test]
fn fillet_with_history_reports_face_records() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernelHandle::spawn();

    let box_handle = kernel
        .execute(&ten_mm_box_op())
        .expect("box should build");

    let (result_id, history) = kernel
        .fillet_with_history(box_handle.id, FILLET_RADIUS_M)
        .expect("fillet_with_history should succeed for a 10mm box with 1mm radius");

    // (a) Result volume is positive and strictly less than the original box.
    // Original box: 10mm × 10mm × 10mm = 1000 mm³ = 1.0e-6 m³.
    // Filleting removes material from every edge and corner, so result < original.
    let orig_vol = BOX_SIDE_M * BOX_SIDE_M * BOX_SIDE_M; // 1e-6 m³
    let vol = kernel
        .query(&GeometryQuery::Volume(result_id))
        .expect("volume query on the fillet result should succeed");
    let vol_si = vol.as_f64().expect("volume value should be numeric");
    assert!(
        vol_si > 0.0,
        "filleted result must have positive volume, got {vol_si}"
    );
    assert!(
        vol_si < orig_vol,
        "filleted volume must be strictly less than the original (fillets remove material): \
         got {vol_si}, original {orig_vol}"
    );
    // Allow up to 10% material removal for a small 1mm fillet on a 10mm cube.
    assert!(
        vol_si >= 0.9 * orig_vol,
        "filleted volume should be at least 90% of original: got {vol_si}, original {orig_vol}"
    );

    // (b) face_modified non-empty: the parent box faces are trimmed by the fillets.
    assert!(
        !history.face_modified.is_empty(),
        "fillet history.face_modified should be non-empty for a 10mm box — \
         got {} records",
        history.face_modified.len()
    );

    // (c) face_generated non-empty: each filleted edge generates a curved fillet face.
    assert!(
        !history.face_generated.is_empty(),
        "fillet history.face_generated should be non-empty for a 10mm box — \
         got {} records",
        history.face_generated.len()
    );

    // (d) Every record has parent_index == 0 (single parent: the box).
    for r in &history.face_modified {
        assert_eq!(
            r.parent_index, 0,
            "fillet face_modified records always have parent_index=0, got {}",
            r.parent_index
        );
    }
    for r in &history.face_generated {
        assert_eq!(
            r.parent_index, 0,
            "fillet face_generated records always have parent_index=0, got {}",
            r.parent_index
        );
    }

    // (e) face_modified.parent_subshape_index < 6 (box has exactly 6 faces).
    for r in &history.face_modified {
        assert!(
            r.parent_subshape_index < 6,
            "face_modified parent_subshape_index {} out of range for a 6-face box",
            r.parent_subshape_index
        );
    }

    // (f) face_generated.parent_subshape_index < 12 (fillet lateral faces are generated
    //     FROM parent edges; a 10mm box has exactly 12 edges).
    for r in &history.face_generated {
        assert!(
            r.parent_subshape_index < 12,
            "face_generated parent_subshape_index {} out of range for a 12-edge box \
             (generated faces come from edges)",
            r.parent_subshape_index
        );
    }

    // (g) Every result_subshape_index is in-range for the result shape's face list.
    let result_faces = kernel
        .extract_faces(result_id)
        .expect("extract_faces on the fillet result should succeed");
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
}
