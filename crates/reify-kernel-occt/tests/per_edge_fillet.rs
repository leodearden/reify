//! Integration test for the curated per-edge fillet path —
//! `OcctKernelHandle::fillet_edges_with_history` (task 3205, step-11/step-12).
//!
//! Pins the curated edge-SELECTION fillet: `BRepFilletAPI_MakeFillet::Add(r, edge)`
//! applied to a CURATED SUBSET of a solid's edges (vs `fillet_all_edges`, which
//! fillets all 12). The persistent-naming seam (face Modified/Generated history)
//! must survive the per-edge path identically to the all-edges path.
//!
//! Gated on `OCCT_AVAILABLE` and `#![cfg(has_occt)]` so non-OCCT builds skip
//! without linker errors (mirrors `fillet_with_history_integration.rs`).
//!
//! Compilation/linkage of this test pins step-12: it will FAIL TO BUILD until
//! the `fillet_edges_with_history` FFI primitive + handle method ship (a valid
//! RED for a method-existence + behavior pin, exactly as
//! `fillet_with_history_integration.rs` does for `fillet_with_history`).

#![cfg(has_occt)]

use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};

/// Fillet radius: 2 mm. Small enough that a 2 mm fillet on every edge of a
/// 20×10×15 mm box (10 mm minimum dimension) stays geometrically valid, yet
/// large enough that the curated-subset vs all-edges volume difference is
/// well above floating-point noise.
const FILLET_RADIUS_M: f64 = 2.0e-3;

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

/// (i)/(ii)/(iii): a curated 4-edge fillet produces a valid non-empty Solid,
/// whose volume sits strictly between the all-edges fillet (removes the most
/// material) and the unfilleted box (removes none) — an ORDERING inequality,
/// not a numeric tolerance — and whose history populates the persistent-naming
/// face Generated/Modified seam.
#[test]
fn fillet_edges_with_history_curated_subset_volume_ordering_and_seam() {
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

    // (i) Curated per-edge fillet returns a valid, non-empty Solid + history.
    let (four_edge_id, history) = kernel
        .fillet_edges_with_history(box_id, FILLET_RADIUS_M, &four_edges)
        .expect("4-edge curated fillet should succeed on a 20×10×15 box");
    let vol_4edge = volume_of(&kernel, four_edge_id);
    assert!(
        vol_4edge > 0.0,
        "4-edge fillet result must have positive volume, got {vol_4edge}"
    );

    // All-edges reference via the back-compat empty-edges dispatch
    // (`GeometryOp::Fillet{edges: vec![], ..}` → `fillet_all_edges`).
    let all_edges_id = kernel
        .execute(&GeometryOp::Fillet {
            target: box_id,
            edges: vec![],
            radius: Value::Real(FILLET_RADIUS_M),
        })
        .expect("all-edges (empty selection) fillet should succeed")
        .id;
    let vol_all = volume_of(&kernel, all_edges_id);

    // (ii) Volume ordering — filleting MORE edges removes MORE material:
    //   vol(all 12 edges) < vol(4-edge subset) < vol(box).
    // This is an ORDERING inequality (the curated subset removes a strict
    // subset of the material the all-edges path removes), NOT a tight
    // numeric tolerance — see plan "RED-PREMISE ACHIEVABILITY".
    assert!(
        vol_all < vol_4edge,
        "all-edges fillet must remove MORE material than the 4-edge subset: \
         vol_all={vol_all}, vol_4edge={vol_4edge}"
    );
    assert!(
        vol_4edge < vol_box,
        "4-edge fillet must remove SOME material vs the unfilleted box: \
         vol_4edge={vol_4edge}, vol_box={vol_box}"
    );

    // (iii) Persistent-naming seam survives the curated path: per-edge history
    // populates face_generated AND/OR face_modified (the curated path must emit
    // the SAME class of LocalFeatureOpHistory records as the all-edges path).
    assert!(
        !history.face_generated.is_empty() || !history.face_modified.is_empty(),
        "per-edge fillet history must populate face_generated and/or \
         face_modified (persistent-naming seam): generated={}, modified={}",
        history.face_generated.len(),
        history.face_modified.len()
    );
}

/// CONTROL (2-arg back-compat unchanged): the empty-selection dispatch
/// `GeometryOp::Fillet{edges: vec![], ..}` must produce the SAME volume as the
/// canonical all-edges fillet (`fillet_with_history` over all 12 edges).
/// Proves step-12's `branch-on-edges.is_empty()` keeps empty selection ==
/// all-edges, so the legacy 2-arg fillet path is bit-for-bit unchanged.
#[test]
fn fillet_empty_edges_matches_all_edges_back_compat() {
    if !OCCT_AVAILABLE {
        return;
    }
    let kernel = OcctKernelHandle::spawn();
    let box_id = build_box(&kernel);

    // Empty-edges dispatch (the back-compat all-edges path).
    let empty_id = kernel
        .execute(&GeometryOp::Fillet {
            target: box_id,
            edges: vec![],
            radius: Value::Real(FILLET_RADIUS_M),
        })
        .expect("empty-edges fillet should succeed")
        .id;
    let vol_empty = volume_of(&kernel, empty_id);

    // Canonical all-edges fillet via `fillet_with_history`.
    let (all_id, _history) = kernel
        .fillet_with_history(box_id, FILLET_RADIUS_M)
        .expect("all-edges fillet_with_history should succeed");
    let vol_all = volume_of(&kernel, all_id);

    // Same operation (all 12 edges, same radius) → matching volume. A loose
    // relative tolerance (1e-6) tightly distinguishes "same all-edges op" from
    // "different op" while tolerating any incidental floating-point drift
    // between the two all-edges FFI entry points.
    assert!(vol_all > 0.0, "all-edges volume must be positive, got {vol_all}");
    let rel_err = (vol_empty - vol_all).abs() / vol_all;
    assert!(
        rel_err < 1e-6,
        "empty-edges fillet must match the all-edges volume (back-compat): \
         vol_empty={vol_empty}, vol_all={vol_all}, rel_err={rel_err}"
    );
}
