//! Integration tests for filtered topology selectors in
//! `reify_eval::topology_selectors` (task 318).
//!
//! These pure-Rust selectors compose `extract_edges` / `extract_faces` with
//! filter predicates over per-edge / per-face property queries
//! (`EdgeLength`, `SurfaceArea`, `FaceNormal`, `EdgeTangent`, `BoundingBox`).
//! They live in reify-eval (not reify-stdlib) because they need `&mut dyn
//! GeometryKernel` for handle allocation; the .ri language-surface wiring is
//! deferred to a future task.
//!
//! Tests skip at runtime if OCCT is unavailable
//! (`reify_kernel_occt::OCCT_AVAILABLE == false`), matching the established
//! reify-eval convention (the `has_occt` cfg is only set inside
//! reify-kernel-occt's own build script and is not visible here).
//!
//! `OcctKernelHandle` is used (not `OcctKernel`) because the selectors are
//! generic over `K: GeometryKernel`, and only `OcctKernelHandle` implements
//! that trait — `OcctKernel` exposes its operations as inherent methods.

use reify_eval::topology_selectors;
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_types::{GeometryHandleId, GeometryOp, Value};

/// Helper: spawn a kernel handle, build a single box of the given mm
/// dimensions (converted to SI metres at the kernel boundary so length
/// filters operate in m and area filters in m²), and return the handle +
/// its box id.
fn box_handle(
    width_mm: f64,
    height_mm: f64,
    depth_mm: f64,
) -> (OcctKernelHandle, GeometryHandleId) {
    let kernel = OcctKernelHandle::spawn();
    let h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(width_mm * 1e-3),
            height: Value::Real(height_mm * 1e-3),
            depth: Value::Real(depth_mm * 1e-3),
        })
        .expect("Box creation should succeed");
    (kernel, h.id)
}

#[test]
fn edges_by_length_box_10x20x30_filters_to_x_axis_edges() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    // 10×20×30 mm box has 12 edges in three axis-aligned groups of 4:
    //   - 4 edges of length 10mm (= 0.010 m, the x-axis edges)
    //   - 4 edges of length 20mm (= 0.020 m)
    //   - 4 edges of length 30mm (= 0.030 m)
    //
    // Filtering by [9.5e-3, 10.5e-3] m must select exactly the four x-edges.
    let (mut kernel, box_id) = box_handle(10.0, 20.0, 30.0);

    let result = topology_selectors::edges_by_length(&mut kernel, box_id, 9.5e-3, 10.5e-3)
        .expect("edges_by_length on a valid box should succeed");

    assert_eq!(
        result.len(),
        4,
        "edges_by_length(9.5e-3, 10.5e-3) on a 10x20x30 box should return the 4 x-axis edges, got {}",
        result.len()
    );

    // The four returned ids should be distinct and none should equal the
    // source box handle.
    let mut seen = std::collections::HashSet::new();
    for id in &result {
        assert_ne!(*id, box_id, "filtered id must differ from the source box");
        assert!(seen.insert(*id), "duplicate filtered id {:?}", id);
    }
}

#[test]
fn edges_at_height_box_top_z_5mm_returns_four() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    // 10×10×10 mm box centered at origin → z extent is [-5e-3, +5e-3].
    // The four edges that lie entirely at z = +5e-3 form the top
    // rectangle; similarly there are four at z = -5e-3 (bottom) and
    // four vertical edges spanning [-5e-3, +5e-3]. Filtering by
    // (z = +5e-3, tol = 1e-9) must select exactly the top four.
    let (mut kernel, box_id) = box_handle(10.0, 10.0, 10.0);

    let result = topology_selectors::edges_at_height(&mut kernel, box_id, 5e-3, 1e-9)
        .expect("edges_at_height on a valid box should succeed");

    assert_eq!(
        result.len(),
        4,
        "edges_at_height(z=+5e-3, tol=1e-9) on a 10x10x10 box should return the 4 top edges, got {}",
        result.len()
    );

    let mut seen = std::collections::HashSet::new();
    for id in &result {
        assert_ne!(*id, box_id, "filtered id must differ from the source box");
        assert!(seen.insert(*id), "duplicate filtered id {:?}", id);
    }
}

#[test]
fn edges_parallel_to_box_x_axis_one_degree_tolerance_returns_four() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    // 10×20×30 mm box has 12 edges in three axis-aligned groups of 4:
    //   - 4 edges parallel to x (length 10mm)
    //   - 4 edges parallel to y (length 20mm)
    //   - 4 edges parallel to z (length 30mm)
    //
    // Filtering for parallelism to x-axis with 1° tolerance must select
    // all four x-aligned edges, regardless of which direction along ±x
    // the kernel's tangent vector happens to point (tangent direction is
    // sign-arbitrary, unlike face normal).
    let (mut kernel, box_id) = box_handle(10.0, 20.0, 30.0);

    let result = topology_selectors::edges_parallel_to(
        &mut kernel,
        box_id,
        [1.0, 0.0, 0.0],
        (1.0_f64).to_radians(),
    )
    .expect("edges_parallel_to on a valid box should succeed");

    assert_eq!(
        result.len(),
        4,
        "edges_parallel_to([1,0,0], 1°) on a 10x20x30 box should return the 4 x-axis edges, got {}",
        result.len()
    );

    let mut seen = std::collections::HashSet::new();
    for id in &result {
        assert_ne!(*id, box_id, "filtered id must differ from the source box");
        assert!(seen.insert(*id), "duplicate filtered id {:?}", id);
    }
}

#[test]
fn faces_by_normal_box_top_face_one_degree_tolerance_returns_one() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    // 10×10×10 mm box has 6 faces with axis-aligned normals (±x, ±y, ±z).
    // Filtering by the +z direction with a 1-degree angular tolerance
    // should select exactly one face (the top), since the next nearest
    // normal is the bottom face at 180° (which is *also* parallel to z
    // and so is correctly accepted under the parity-tolerant predicate
    // — the test calls for "exactly one" though, so the impl must NOT
    // accept anti-parallel directions for `faces_by_normal`).
    //
    // i.e. unlike `edges_parallel_to`, `faces_by_normal` distinguishes
    // outward vs inward — only one face has its outward normal aligned
    // with +z.
    let (mut kernel, box_id) = box_handle(10.0, 10.0, 10.0);

    let result = topology_selectors::faces_by_normal(
        &mut kernel,
        box_id,
        [0.0, 0.0, 1.0],
        (1.0_f64).to_radians(),
    )
    .expect("faces_by_normal on a valid box should succeed");

    assert_eq!(
        result.len(),
        1,
        "faces_by_normal([0,0,1], 1°) on a 10x10x10 box should return exactly the top face, got {} ids",
        result.len()
    );
    assert_ne!(result[0], box_id, "filtered id must differ from the source box");
}

#[test]
fn faces_by_area_box_10x20x30_filters_to_pair_with_target_area() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    // 10×20×30 mm box has 6 faces in three axis-aligned pairs:
    //   - 2 faces of area 10mm × 20mm = 200e-6 m²
    //   - 2 faces of area 10mm × 30mm = 300e-6 m²
    //   - 2 faces of area 20mm × 30mm = 600e-6 m²
    //
    // Filtering by [199e-6, 201e-6] m² should select exactly the two
    // 10×20 faces.
    let (mut kernel, box_id) = box_handle(10.0, 20.0, 30.0);

    let result = topology_selectors::faces_by_area(&mut kernel, box_id, 199e-6, 201e-6)
        .expect("faces_by_area on a valid box should succeed");

    assert_eq!(
        result.len(),
        2,
        "faces_by_area([199e-6, 201e-6]) on a 10x20x30 box should return the two 10x20 faces, got {}",
        result.len()
    );

    let mut seen = std::collections::HashSet::new();
    for id in &result {
        assert_ne!(*id, box_id, "filtered id must differ from the source box");
        assert!(seen.insert(*id), "duplicate filtered id {:?}", id);
    }
}
