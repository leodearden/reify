//! Integration test for `OcctKernelHandle::chamfer_with_history` —
//! the v0.2 persistent-naming-v2 local-feature history-tracking primitive
//! for `BRepFilletAPI_MakeChamfer` (task 2655, step-5/step-6).
//!
//! Exercises the FFI primitive that wraps `BRepFilletAPI_MakeChamfer::Modified()`,
//! `Generated()`, and `IsDeleted()` and exposes the per-parent face/edge
//! correspondence for face and edge topology.
//!
//! Mirrors the structure of `fillet_with_history_integration.rs` (and
//! `boolean_op_history_integration.rs`): gated on `OCCT_AVAILABLE` and
//! `#![cfg(has_occt)]` so non-OCCT builds skip without linker errors.

#![cfg(has_occt)]

use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_types::{GeometryOp, GeometryQuery, Value};

/// 10×10×10 mm box, expressed in SI metres at the kernel boundary.
const BOX_SIDE_M: f64 = 10.0e-3;

/// Chamfer distance: 1 mm. Small enough that every edge gets a chamfer face
/// without geometric collapse on a 10mm cube.
const CHAMFER_DISTANCE_M: f64 = 1.0e-3;

/// Build the `GeometryOp::Box` for a 10mm cube.
fn ten_mm_box_op() -> GeometryOp {
    GeometryOp::Box {
        width: Value::Real(BOX_SIDE_M),
        height: Value::Real(BOX_SIDE_M),
        depth: Value::Real(BOX_SIDE_M),
    }
}

/// `BRepFilletAPI_MakeChamfer` history exposes Modified/Generated/Deleted for
/// each parent face and edge. The test:
///
/// - builds a 10×10×10mm box;
/// - calls `chamfer_with_history(box_id, 1mm)` (1mm chamfer distance);
/// - asserts result volume positive and < original (chamfer removes material);
/// - asserts `face_modified` non-empty (every box face is trimmed by adjacent chamfers);
/// - asserts `face_generated` non-empty (each chamfered edge generates a chamfer face);
/// - asserts every record has `parent_index == 0`;
/// - asserts `face_modified.parent_subshape_index < 6` (box has 6 faces);
/// - asserts `face_generated.parent_subshape_index < 12` (chamfer lateral faces
///   are generated FROM edges; box has 12 edges);
/// - asserts every `result_subshape_index` is in-range for the result shape;
/// - asserts `edge_modified` per-record well-formedness (parent_index == 0,
///   parent_subshape_index < 12, result_subshape_index < result_edge_count);
/// - asserts `edge_generated` per-record bounds: parent_index == 0,
///   parent_subshape_index < 8 (box VERTICES — generated FROM vertices, not edges),
///   result_subshape_index < result_edge_count (OCCT's chamfer algorithm does not
///   guarantee vertex-generated edges are populated; bounds-only pin like edge_modified);
/// - asserts `face_deleted.is_empty()` (chamfer does not consume any parent face);
/// - asserts `edge_deleted` per-record bounds: parent_index == 0,
///   parent_subshape_index < 12 (BRepFilletAPI_MakeChamfer marks parent edges as
///   IsDeleted; bounds-only pin, no emptiness assertion).
///
/// Compilation/linkage of this test pins step-6: it would fail to build
/// until the FFI primitive + Rust handle method ship (already done in step-2).
#[test]
fn chamfer_with_history_reports_face_records() {
    if !OCCT_AVAILABLE {
        return;
    }

    let kernel = OcctKernelHandle::spawn();

    let box_handle = kernel
        .execute(&ten_mm_box_op())
        .expect("box should build");

    let (result_id, history) = kernel
        .chamfer_with_history(box_handle.id, CHAMFER_DISTANCE_M)
        .expect("chamfer_with_history should succeed for a 10mm box with 1mm distance");

    // (a) Result volume is positive and strictly less than the original box.
    // Original box: 10mm × 10mm × 10mm = 1000 mm³ = 1.0e-6 m³.
    // Chamfering removes material from every edge and corner, so result < original.
    let orig_vol = BOX_SIDE_M * BOX_SIDE_M * BOX_SIDE_M; // 1e-6 m³
    let vol = kernel
        .query(&GeometryQuery::Volume(result_id))
        .expect("volume query on the chamfer result should succeed");
    let vol_si = vol.as_f64().expect("volume value should be numeric");
    assert!(
        vol_si > 0.0,
        "chamfered result must have positive volume, got {vol_si}"
    );
    assert!(
        vol_si < orig_vol,
        "chamfered volume must be strictly less than the original (chamfers remove material): \
         got {vol_si}, original {orig_vol}"
    );
    // Allow up to 10% material removal for a small 1mm chamfer on a 10mm cube.
    assert!(
        vol_si >= 0.9 * orig_vol,
        "chamfered volume should be at least 90% of original: got {vol_si}, original {orig_vol}"
    );

    // (b) face_modified non-empty: the parent box faces are trimmed by the chamfers.
    assert!(
        !history.face_modified.is_empty(),
        "chamfer history.face_modified should be non-empty for a 10mm box — \
         got {} records",
        history.face_modified.len()
    );

    // (c) face_generated non-empty: each chamfered edge generates a flat chamfer face.
    assert!(
        !history.face_generated.is_empty(),
        "chamfer history.face_generated should be non-empty for a 10mm box — \
         got {} records",
        history.face_generated.len()
    );

    // (d) Every record has parent_index == 0 (single parent: the box).
    for r in &history.face_modified {
        assert_eq!(
            r.parent_index, 0,
            "chamfer face_modified records always have parent_index=0, got {}",
            r.parent_index
        );
    }
    for r in &history.face_generated {
        assert_eq!(
            r.parent_index, 0,
            "chamfer face_generated records always have parent_index=0, got {}",
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

    // (f) face_generated.parent_subshape_index < 12 (chamfer lateral faces are generated
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
        .expect("extract_faces on the chamfer result should succeed");
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

    // (h) Compute result_edge_count for use in the edge-buffer bounds assertions.
    let result_edges = kernel
        .extract_edges(result_id)
        .expect("extract_edges on the chamfer result should succeed");
    let result_edge_count = result_edges.len() as u32;

    // (i) edge_modified per-record well-formedness.
    // No non-empty assertion: for a fully-chamfered box OCCT may route parent edges
    // through Generated/Deleted rather than Modified.
    for r in &history.edge_modified {
        assert_eq!(
            r.parent_index, 0,
            "chamfer edge_modified records always have parent_index=0, got {}",
            r.parent_index
        );
        assert!(
            r.parent_subshape_index < 12,
            "chamfer edge_modified parent_subshape_index {} out of range for a 12-edge box",
            r.parent_subshape_index
        );
        assert!(
            r.result_subshape_index < result_edge_count,
            "chamfer edge_modified result_subshape_index {} out of range; result has {} edges",
            r.result_subshape_index,
            result_edge_count
        );
    }

    // (j) edge_generated per-record well-formedness. No non-empty assertion:
    // BRepFilletAPI_MakeChamfer::Generated() does not populate vertex-generated
    // edges for this topology (same conservative treatment as edge_modified).
    // parent_subshape_index is into the VERTEX map (box has 8 vertices), not
    // the edge map.
    for r in &history.edge_generated {
        assert_eq!(
            r.parent_index, 0,
            "chamfer edge_generated records always have parent_index=0, got {}",
            r.parent_index
        );
        assert!(
            r.parent_subshape_index < 8,
            "chamfer edge_generated parent_subshape_index {} out of range for an 8-vertex box \
             (edge_generated is keyed by parent VERTEX, not parent edge)",
            r.parent_subshape_index
        );
        assert!(
            r.result_subshape_index < result_edge_count,
            "chamfer edge_generated result_subshape_index {} out of range; result has {} edges",
            r.result_subshape_index,
            result_edge_count
        );
    }

    // (k) face_deleted must be empty for a clean chamfer on a simple convex box.
    assert!(
        history.face_deleted.is_empty(),
        "clean chamfer must not delete any parent face; got {} face_deleted records",
        history.face_deleted.len()
    );

    // (l) edge_deleted per-record bounds. No emptiness assertion: for a fully-chamfered
    // box, BRepFilletAPI_MakeChamfer marks all parent edges as IsDeleted() (they are
    // replaced by chamfer surfaces). parent_subshape_index must be in-range for a
    // 12-edge box; parent_index must be 0 (single parent).
    for d in &history.edge_deleted {
        assert_eq!(
            d.parent_index, 0,
            "chamfer edge_deleted records always have parent_index=0, got {}",
            d.parent_index
        );
        assert!(
            d.parent_subshape_index < 12,
            "chamfer edge_deleted parent_subshape_index {} out of range for a 12-edge box",
            d.parent_subshape_index
        );
    }
}
