//! Integration test for the curated per-edge chamfer path —
//! `OcctKernelHandle::chamfer_edges_with_history` (task 4185, step-1/step-2).
//!
//! Pins the curated edge-SELECTION chamfer: `BRepFilletAPI_MakeChamfer::Add(d, edge)`
//! applied to a CURATED SUBSET of a solid's edges (vs `chamfer_with_history`, which
//! chamfers all 12). The persistent-naming seam (face Modified/Generated history)
//! must survive the per-edge path identically to the all-edges path.
//!
//! The exact chamfer parallel of `per_edge_fillet.rs` (task 3205 / α).
//!
//! Gated on `OCCT_AVAILABLE` and `#![cfg(has_occt)]` so non-OCCT builds skip
//! without linker errors (mirrors `per_edge_fillet.rs`).
//!
//! Compilation/linkage of this test pins step-2: it will FAIL TO BUILD until
//! the `chamfer_edges_with_history` FFI primitive + handle method ship (a valid
//! RED for a method-existence + behavior pin, exactly as `per_edge_fillet.rs`
//! does for `fillet_edges_with_history`).

#![cfg(has_occt)]

use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};

/// Chamfer setback: 1 mm. Small enough that a 1 mm chamfer on every edge of a
/// 20×10×15 mm box (10 mm minimum dimension) stays geometrically valid, yet
/// large enough that the curated-subset vs all-edges volume difference is
/// well above floating-point noise.
const CHAMFER_DISTANCE_M: f64 = 1.0e-3;

// Box dimensions in metres (the kernel stores lengths in SI metres — see the
// `Value::Real(BOX_SIDE_M)` usage in `tests/common/mod.rs`).
const BOX_W_M: f64 = 20.0e-3;
const BOX_H_M: f64 = 10.0e-3;
const BOX_D_M: f64 = 15.0e-3;

/// Build a 20×10×15 mm box and return its handle id.
fn build_box(kernel: &OcctKernelHandle) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(BOX_W_M),
            height: Value::Real(BOX_H_M),
            depth: Value::Real(BOX_D_M),
        })
        .expect("box should build")
        .id
}

/// Query the SI volume (m³) of a solid handle.
fn volume_of(kernel: &OcctKernelHandle, id: GeometryHandleId) -> f64 {
    kernel
        .query(&GeometryQuery::Volume(id))
        .expect("volume query should succeed")
        .as_f64()
        .expect("volume value should be numeric")
}

/// (i)/(ii)/(iii): a curated 4-edge chamfer produces a valid non-empty Solid,
/// whose volume sits strictly between the all-edges chamfer (removes the most
/// material) and the unchamfered box (removes none) — an ORDERING inequality,
/// not a numeric tolerance — and whose history populates the persistent-naming
/// face Generated/Modified seam.
#[test]
fn chamfer_edges_with_history_curated_subset_volume_ordering_and_seam() {
    if !OCCT_AVAILABLE {
        return;
    }
    let kernel = OcctKernelHandle::spawn();

    let box_id = build_box(&kernel);
    let vol_box = volume_of(&kernel, box_id);
    assert!(vol_box > 0.0, "box volume must be positive, got {vol_box}");

    // Extract all 12 edges of the box; curate a subset of 4.
    let edges = kernel
        .extract_edges(box_id)
        .expect("extract_edges should succeed on a solid box");
    assert_eq!(
        edges.len(),
        12,
        "a 6-face box has exactly 12 edges, got {}",
        edges.len()
    );
    let four_edges: Vec<GeometryHandleId> = edges.iter().take(4).copied().collect();

    // (i) Curated per-edge chamfer returns a valid, non-empty Solid + history.
    let (four_edge_id, history) = kernel
        .chamfer_edges_with_history(box_id, CHAMFER_DISTANCE_M, &four_edges)
        .expect("4-edge curated chamfer should succeed on a 20×10×15 box");
    let vol_4edge = volume_of(&kernel, four_edge_id);
    assert!(
        vol_4edge > 0.0,
        "4-edge chamfer result must have positive volume, got {vol_4edge}"
    );

    // All-edges reference via the EXISTING all-edges primitive
    // `chamfer_with_history` (NOT `GeometryOp::Chamfer{edges: vec![]}` — the IR
    // `edges` field is not added until step-6, so this KERNEL test stays
    // GREEN-able by step-2 alone).
    let (all_edges_id, _all_history) = kernel
        .chamfer_with_history(box_id, CHAMFER_DISTANCE_M)
        .expect("all-edges chamfer_with_history should succeed");
    let vol_all = volume_of(&kernel, all_edges_id);

    // (ii) Volume ordering — chamfering MORE edges removes MORE material:
    //   vol(all 12 edges) < vol(4-edge subset) < vol(box).
    // This is an ORDERING inequality (the curated subset removes a strict
    // subset of the material the all-edges path removes), NOT a tight
    // numeric tolerance — see plan "RED-PREMISE ACHIEVABILITY".
    assert!(
        vol_all < vol_4edge,
        "all-edges chamfer must remove MORE material than the 4-edge subset: \
         vol_all={vol_all}, vol_4edge={vol_4edge}"
    );
    assert!(
        vol_4edge < vol_box,
        "4-edge chamfer must remove SOME material vs the unchamfered box: \
         vol_4edge={vol_4edge}, vol_box={vol_box}"
    );

    // (iii) Persistent-naming seam survives the curated path: per-edge history
    // populates face_generated AND/OR face_modified (the curated path must emit
    // the SAME class of LocalFeatureOpHistory records as the all-edges path).
    assert!(
        !history.face_generated.is_empty() || !history.face_modified.is_empty(),
        "per-edge chamfer history must populate face_generated and/or \
         face_modified (persistent-naming seam): generated={}, modified={}",
        history.face_generated.len(),
        history.face_modified.len()
    );
}
