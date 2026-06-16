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

/// Asymmetric chamfer setbacks: 1 mm on one adjacent face, 2 mm on the other.
const D1_M: f64 = 1.0e-3;
const D2_M: f64 = 2.0e-3;

/// Parse the JSON `Value::String` produced by `BoundingBox` queries into
/// `(xmin, ymin, zmin, xmax, ymax, zmax)` (mirrors `topology_extract_integration.rs`).
fn parse_bbox(v: &Value) -> (f64, f64, f64, f64, f64, f64) {
    let s = match v {
        Value::String(s) => s,
        other => panic!("expected Value::String, got {:?}", other),
    };
    let parsed: serde_json::Value =
        serde_json::from_str(s).unwrap_or_else(|e| panic!("failed to parse {:?} as JSON: {e}", s));
    let xmin = parsed["xmin"].as_f64().expect("missing xmin");
    let ymin = parsed["ymin"].as_f64().expect("missing ymin");
    let zmin = parsed["zmin"].as_f64().expect("missing zmin");
    let xmax = parsed["xmax"].as_f64().expect("missing xmax");
    let ymax = parsed["ymax"].as_f64().expect("missing ymax");
    let zmax = parsed["zmax"].as_f64().expect("missing zmax");
    (xmin, ymin, zmin, xmax, ymax, zmax)
}

/// Query a handle's `BoundingBox` and return the parsed extents tuple.
fn bbox_of(kernel: &OcctKernelHandle, id: GeometryHandleId) -> (f64, f64, f64, f64, f64, f64) {
    let v = kernel
        .query(&GeometryQuery::BoundingBox(id))
        .expect("BoundingBox query should succeed");
    parse_bbox(&v)
}

/// Identify the 4 TOP edges of `box_id`: horizontal (z-extent ≈ 0) AND sitting
/// on the top z-plane (distinguishes them from the 4 bottom edges). Asserts
/// exactly 4 are found. Shared by the direct-call and execute-routed asymmetric
/// chamfer tests so both select the SAME edge set.
fn top4_edges(kernel: &OcctKernelHandle, box_id: GeometryHandleId) -> Vec<GeometryHandleId> {
    // Top z-plane of the box (orientation-agnostic; we only assume the kernel's
    // width→x / height→y / depth→z axis mapping, pinned by topology tests).
    let (_, _, _, _, _, box_zmax) = bbox_of(kernel, box_id);
    // Flatness / position tolerance: OCCT's BRepBndLib enlarges the bbox by the
    // shape's stored tolerance (~1e-7), so 1e-6 m comfortably resolves a flat
    // axis (extent ≈ 0) and a face/edge sitting on the top z-plane.
    let flat_tol = 1e-6;
    let pos_tol = 1e-6;
    let edges = kernel
        .extract_edges(box_id)
        .expect("extract_edges should succeed on a solid box");
    let mut top4: Vec<GeometryHandleId> = Vec::new();
    for e in &edges {
        let (_, _, zmin, _, _, zmax) = bbox_of(kernel, *e);
        if (zmax - zmin).abs() < flat_tol && (zmax - box_zmax).abs() < pos_tol {
            top4.push(*e);
        }
    }
    assert_eq!(
        top4.len(),
        4,
        "a box has exactly 4 top edges, got {}",
        top4.len()
    );
    top4
}

/// Measure the chamfer setbacks on a result whose 4 TOP edges were chamfered.
/// Each measurement is one component of an edge's {d1, d2} split:
///   - the shrunken horizontal top face loses 2·(top setback) in x AND in y
///     (each of its 4 edges is chamfered) → /2 recovers the top setback;
///   - each vertical side face has ONLY its top edge chamfered, so its z-extent
///     shrinks by exactly that edge's side setback.
/// Pooling top-derived and side-derived values guarantees a ~1 mm and a ~2 mm
/// entry regardless of which adjacent face the kernel picks as the reference F.
fn measure_top_chamfer_setbacks(
    kernel: &OcctKernelHandle,
    box_id: GeometryHandleId,
    result_id: GeometryHandleId,
) -> Vec<f64> {
    let (_, _, _, _, _, box_zmax) = bbox_of(kernel, box_id);
    let flat_tol = 1e-6;
    let pos_tol = 1e-6;
    // A genuine vertical side face spans most of the 15 mm depth (≈13–14 mm after
    // its top edge is trimmed); a chamfer bevel face spans only ≈1–2 mm in z.
    // 5 mm cleanly separates the two classes.
    let side_min = 5.0e-3;
    let faces = kernel
        .extract_faces(result_id)
        .expect("extract_faces should succeed on the chamfered result");
    let mut setbacks: Vec<f64> = Vec::new();
    for f in &faces {
        let (xmin, ymin, zmin, xmax, ymax, zmax) = bbox_of(kernel, *f);
        let dx = xmax - xmin;
        let dy = ymax - ymin;
        let dz = zmax - zmin;
        if dz < flat_tol && (zmax - box_zmax).abs() < pos_tol {
            // Shrunken top face (horizontal, on the top z-plane).
            setbacks.push((BOX_W_M - dx) / 2.0);
            setbacks.push((BOX_H_M - dy) / 2.0);
        } else if dy < flat_tol && dz > side_min {
            // y-normal vertical side face: top trimmed → z shrink = side setback.
            setbacks.push(BOX_D_M - dz);
        } else if dx < flat_tol && dz > side_min {
            // x-normal vertical side face: z shrink = side setback.
            setbacks.push(BOX_D_M - dz);
        }
    }
    assert!(
        setbacks.len() >= 2,
        "expected at least two setback measurements (top + sides), got {setbacks:?}"
    );
    setbacks
}

/// Assert a pool of measured setbacks reveals the unordered pair ≈ {1 mm, 2 mm}
/// (min ≈ D1_M, max ≈ D2_M within 5%) with ratio ≈ 2.0 — proving the d1:d2 = 1:2
/// asymmetry independent of which adjacent face the kernel picks as F.
/// Exact-by-construction (OCCT `MakeChamfer::Add(d1, d2, E, F)`); 5% absorbs
/// only measurement margin.
fn assert_setbacks_1_2(setbacks: &[f64]) {
    let min = setbacks.iter().copied().fold(f64::INFINITY, f64::min);
    let max = setbacks.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (min - D1_M).abs() / D1_M < 0.05,
        "minimum setback must be ≈1 mm: min={min}, setbacks={setbacks:?}"
    );
    assert!(
        (max - D2_M).abs() / D2_M < 0.05,
        "maximum setback must be ≈2 mm: max={max}, setbacks={setbacks:?}"
    );
    assert!(
        ((max / min) - 2.0).abs() < 0.10,
        "setback ratio must be ≈2.0: max/min={}, setbacks={setbacks:?}",
        max / min
    );
}

/// Asymmetric per-edge chamfer applies DISTINCT setbacks (d1=1 mm, d2=2 mm) to
/// the two faces adjacent to each selected edge — `MakeChamfer::Add(d1, d2, E, F)`
/// puts d1 on the reference face F and d2 on the other. Chamfering the 4 TOP
/// edges of the box and measuring the resulting face setbacks must reveal the
/// unordered pair ≈ {1 mm, 2 mm}, proving the 1:2 asymmetry — robustly to which
/// adjacent face the kernel picks as F (the measurement pool always contains a
/// ~1 mm value and a ~2 mm value under any reference-face assignment).
#[test]
fn chamfer_asymmetric_distinct_setbacks() {
    if !OCCT_AVAILABLE {
        return;
    }
    let kernel = OcctKernelHandle::spawn();
    let box_id = build_box(&kernel);
    let top4 = top4_edges(&kernel, box_id);

    // Asymmetric chamfer: d1 = 1 mm, d2 = 2 mm on the 4 top edges.
    let (result_id, _history) = kernel
        .chamfer_asymmetric_edges_with_history(box_id, D1_M, D2_M, &top4)
        .expect("asymmetric chamfer should succeed on the 4 top edges of a 20×10×15 box");

    let setbacks = measure_top_chamfer_setbacks(&kernel, box_id, result_id);
    assert_setbacks_1_2(&setbacks);
}

/// `GeometryOp::ChamferAsymmetric{edges: <non-empty>, d1, d2}` through `execute`
/// must route to the asymmetric per-edge path
/// (`chamfer_asymmetric_edges_with_history`), reproducing the distinct setbacks
/// ≈ {1 mm, 2 mm} on the 4 top edges. Proves execute dispatches the new variant
/// to the asymmetric primitive (not symmetric chamfer or the all-edges path).
#[test]
fn chamfer_asymmetric_execute_routes_distinct_setbacks() {
    if !OCCT_AVAILABLE {
        return;
    }
    let kernel = OcctKernelHandle::spawn();
    let box_id = build_box(&kernel);
    let top4 = top4_edges(&kernel, box_id);

    // Asymmetric chamfer via execute (must route to chamfer_asymmetric_edges_with_history).
    let result_id = kernel
        .execute(&GeometryOp::ChamferAsymmetric {
            target: box_id,
            edges: top4,
            d1: Value::Real(D1_M),
            d2: Value::Real(D2_M),
        })
        .expect("asymmetric chamfer via execute should succeed on the 4 top edges")
        .id;

    let setbacks = measure_top_chamfer_setbacks(&kernel, box_id, result_id);
    assert_setbacks_1_2(&setbacks);
}

/// A zero (or non-positive) setback must be rejected on BOTH d1 and d2 — the
/// same finite-positive contract the symmetric path enforces.
#[test]
fn chamfer_asymmetric_zero_distance_errors() {
    if !OCCT_AVAILABLE {
        return;
    }
    let kernel = OcctKernelHandle::spawn();
    let box_id = build_box(&kernel);
    let edges = kernel
        .extract_edges(box_id)
        .expect("extract_edges should succeed on a solid box");
    let one: Vec<GeometryHandleId> = edges.iter().take(1).copied().collect();

    // d1 = 0 → Err.
    assert!(
        kernel
            .chamfer_asymmetric_edges_with_history(box_id, 0.0, D2_M, &one)
            .is_err(),
        "asymmetric chamfer with d1=0 must error (distance must be finite positive)"
    );
    // d2 = 0 → Err.
    assert!(
        kernel
            .chamfer_asymmetric_edges_with_history(box_id, D1_M, 0.0, &one)
            .is_err(),
        "asymmetric chamfer with d2=0 must error (distance must be finite positive)"
    );
}

/// CONTROL (2-arg back-compat unchanged): the empty-selection dispatch
/// `GeometryOp::Chamfer{edges: vec![], ..}` must produce the SAME volume as the
/// canonical all-edges chamfer (`chamfer_with_history` over all 12 edges).
/// Proves step-6's `branch-on-edges.is_empty()` keeps empty selection ==
/// all-edges, so the legacy 2-arg chamfer path is bit-for-bit unchanged.
/// β parallel of `per_edge_fillet.rs::fillet_empty_edges_matches_all_edges_back_compat`.
#[test]
fn chamfer_empty_edges_matches_all_edges_back_compat() {
    if !OCCT_AVAILABLE {
        return;
    }
    let kernel = OcctKernelHandle::spawn();
    let box_id = build_box(&kernel);

    // Empty-edges dispatch (the back-compat all-edges path).
    let empty_id = kernel
        .execute(&GeometryOp::Chamfer {
            target: box_id,
            edges: vec![],
            distance: Value::Real(CHAMFER_DISTANCE_M),
        })
        .expect("empty-edges chamfer should succeed")
        .id;
    let vol_empty = volume_of(&kernel, empty_id);

    // Canonical all-edges chamfer via `chamfer_with_history`.
    let (all_id, _history) = kernel
        .chamfer_with_history(box_id, CHAMFER_DISTANCE_M)
        .expect("all-edges chamfer_with_history should succeed");
    let vol_all = volume_of(&kernel, all_id);

    // Same operation (all 12 edges, same distance) → matching volume within a
    // loose relative tolerance (1e-6), distinguishing "same all-edges op" from
    // "different op" while tolerating incidental floating-point drift between
    // the two all-edges FFI entry points.
    assert!(vol_all > 0.0, "all-edges volume must be positive, got {vol_all}");
    let rel_err = (vol_empty - vol_all).abs() / vol_all;
    assert!(
        rel_err < 1e-6,
        "empty-edges chamfer must match the all-edges volume (back-compat): \
         vol_empty={vol_empty}, vol_all={vol_all}, rel_err={rel_err}"
    );
}

/// `GeometryOp::Chamfer{edges: <non-empty>, ..}` through `execute` must route to
/// the curated per-edge path (`chamfer_edges_with_history`), reproducing the
/// step-1 volume ordering: vol(all 12) < vol(4-edge subset) < vol(box). Proves
/// execute's `branch-on-edges.is_empty()` sends a non-empty selection to the
/// curated primitive, not the all-edges fall-through.
#[test]
fn chamfer_execute_routes_curated_edges_volume_ordering() {
    if !OCCT_AVAILABLE {
        return;
    }
    let kernel = OcctKernelHandle::spawn();
    let box_id = build_box(&kernel);
    let vol_box = volume_of(&kernel, box_id);

    let edges = kernel
        .extract_edges(box_id)
        .expect("extract_edges should succeed on a solid box");
    let four_edges: Vec<GeometryHandleId> = edges.iter().take(4).copied().collect();

    // Curated 4-edge chamfer via execute (must route to chamfer_edges_with_history).
    let four_edge_id = kernel
        .execute(&GeometryOp::Chamfer {
            target: box_id,
            edges: four_edges,
            distance: Value::Real(CHAMFER_DISTANCE_M),
        })
        .expect("curated 4-edge chamfer via execute should succeed")
        .id;
    let vol_4edge = volume_of(&kernel, four_edge_id);

    // All-edges baseline via the existing all-edges primitive.
    let (all_id, _history) = kernel
        .chamfer_with_history(box_id, CHAMFER_DISTANCE_M)
        .expect("all-edges chamfer_with_history should succeed");
    let vol_all = volume_of(&kernel, all_id);

    assert!(
        vol_all < vol_4edge,
        "all-edges chamfer must remove MORE material than the 4-edge subset: \
         vol_all={vol_all}, vol_4edge={vol_4edge}"
    );
    assert!(
        vol_4edge < vol_box,
        "curated 4-edge chamfer via execute must remove SOME material vs the box: \
         vol_4edge={vol_4edge}, vol_box={vol_box}"
    );
}
