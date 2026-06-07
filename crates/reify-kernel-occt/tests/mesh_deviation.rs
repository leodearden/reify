//! Integration tests for `OcctKernel::measure_mesh_deviation` and
//! `OcctKernelHandle::measure_mesh_deviation` — the sampled max
//! facet-chord deviation metric (SI metres, task 4198 Determinacy β).
//!
//! All tests require a real OCCT build (`cfg(has_occt)`).
//!
//! # Invariants under test
//!
//! - **B1** (planar box ≈ 0): planar-face triangulation places every interior
//!   sample point exactly in the face plane (convex combo of coplanar f32
//!   vertices), so projected distance ≈ f32 quantization (~1e-6 m at unit
//!   scale). Asserted `≤ 1e-5 m` at both COARSE and FINE deflections.
//!
//! - **B2** (curved monotone): OCCT's linear-deflection bound means coarser
//!   deflection → larger facets → interior samples farther from the true
//!   surface → strictly larger measured deviation. Sphere and cylinder both
//!   satisfy `deviation(coarse) > deviation(fine)` strictly, and
//!   `deviation(fine) > 1e-7 m` (non-zero, above f32 noise floor).
//!
//! - **B3-numeric**: every returned value is `finite` and `≥ 0`.
//!
//! # CRITICAL: no tolerance argument
//!
//! `measure_mesh_deviation` receives no tolerance/deflection argument — it
//! cannot echo the configured deflection (structural anti-circularity per
//! PRD §8.3 / task CRITICAL). The measured value may legitimately exceed
//! the requested deflection when OCCT clamps (MinSize / angular-deflection
//! domination / mesh failure).

#![cfg(has_occt)]

use reify_ir::{GeometryKernel, GeometryOp, Value};
use reify_kernel_occt::{OcctKernel, OcctKernelHandle};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Build a kernel and execute a single op, returning `(kernel, handle_id)`.
fn make_shape(op: GeometryOp) -> (OcctKernel, reify_ir::GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let handle = kernel
        .execute(&op)
        .expect("geometry op should succeed");
    (kernel, handle.id)
}

fn sphere_op(radius_m: f64) -> GeometryOp {
    GeometryOp::Sphere {
        radius: Value::Real(radius_m),
    }
}

fn box_op(w: f64, h: f64, d: f64) -> GeometryOp {
    GeometryOp::Box {
        width: Value::Real(w),
        height: Value::Real(h),
        depth: Value::Real(d),
    }
}

fn cylinder_op(radius_m: f64, height_m: f64) -> GeometryOp {
    GeometryOp::Cylinder {
        radius: Value::Real(radius_m),
        height: Value::Real(height_m),
    }
}

// ── B1: planar box — deviation ≈ 0 at both deflections ───────────────────────

/// B1-coarse: a unit box tessellated at coarse deflection (5e-2 m) has
/// max facet-chord deviation ≤ 1e-5 m.
///
/// Planar-face interior samples are exact convex combinations of coplanar
/// f32 vertices; projected distance = pure f32 quantization (~1e-6 m at
/// unit scale). 1e-5 m clears that ~10× and sits well below curved fine
/// deviation (~1e-4 m), giving clean B1/B2 separation.
#[test]
fn b1_box_coarse_deviation_is_near_zero() {
    let (kernel, box_id) = make_shape(box_op(1.0, 1.0, 1.0));
    let mesh = kernel
        .tessellate(box_id, 5e-2)
        .expect("box tessellation should succeed");
    let dev = kernel
        .measure_mesh_deviation(box_id, &mesh)
        .expect("measure_mesh_deviation should return Ok for a valid handle");
    assert!(
        dev >= 0.0,
        "B3-numeric: deviation must be ≥ 0, got {dev}"
    );
    assert!(
        dev.is_finite(),
        "B3-numeric: deviation must be finite, got {dev}"
    );
    assert!(
        dev <= 1e-5,
        "B1: box (planar faces) deviation at coarse deflection must be ≤ 1e-5 m, got {dev}"
    );
}

/// B1-fine: same box at fine deflection (5e-4 m) — still ≤ 1e-5 m.
#[test]
fn b1_box_fine_deviation_is_near_zero() {
    let (kernel, box_id) = make_shape(box_op(1.0, 1.0, 1.0));
    let mesh = kernel
        .tessellate(box_id, 5e-4)
        .expect("box tessellation should succeed");
    let dev = kernel
        .measure_mesh_deviation(box_id, &mesh)
        .expect("measure_mesh_deviation should return Ok for a valid handle");
    assert!(
        dev >= 0.0,
        "B3-numeric: deviation must be ≥ 0, got {dev}"
    );
    assert!(
        dev.is_finite(),
        "B3-numeric: deviation must be finite, got {dev}"
    );
    assert!(
        dev <= 1e-5,
        "B1: box (planar faces) deviation at fine deflection must be ≤ 1e-5 m, got {dev}"
    );
}

// ── B2: curved sphere — coarse deviation > fine deviation (monotone) ─────────

/// B2-sphere: coarse (5e-2 m) deviation > fine (5e-4 m) deviation strictly;
/// fine deviation > 1e-7 m (non-zero above f32 noise floor).
///
/// OCCT's linear-deflection chord bound guarantees: coarser deflection →
/// larger facets → interior samples farther from the true sphere surface →
/// strictly larger measured deviation. Choose well-separated deflections so
/// coarse_dev ≈ 100× fine_dev and fine_dev (~1e-4 m) >> 1e-7 noise floor.
#[test]
fn b2_sphere_deviation_is_monotone_in_deflection() {
    // R=1 m sphere: coarse deflection 5e-2 m, fine deflection 5e-4 m.
    let coarse_tol = 5e-2_f64;
    let fine_tol = 5e-4_f64;

    let (kernel_c, sphere_id_c) = make_shape(sphere_op(1.0));
    let mesh_c = kernel_c
        .tessellate(sphere_id_c, coarse_tol)
        .expect("sphere coarse tessellation should succeed");
    let dev_c = kernel_c
        .measure_mesh_deviation(sphere_id_c, &mesh_c)
        .expect("measure_mesh_deviation should return Ok");

    let (kernel_f, sphere_id_f) = make_shape(sphere_op(1.0));
    let mesh_f = kernel_f
        .tessellate(sphere_id_f, fine_tol)
        .expect("sphere fine tessellation should succeed");
    let dev_f = kernel_f
        .measure_mesh_deviation(sphere_id_f, &mesh_f)
        .expect("measure_mesh_deviation should return Ok");

    // B3-numeric
    assert!(dev_c.is_finite() && dev_c >= 0.0, "coarse dev must be finite ≥ 0, got {dev_c}");
    assert!(dev_f.is_finite() && dev_f >= 0.0, "fine dev must be finite ≥ 0, got {dev_f}");

    // B2: curved surface → non-zero deviation even at fine deflection
    assert!(
        dev_f > 1e-7,
        "B2: sphere fine deviation must be > 1e-7 m (above f32 noise floor), got {dev_f}"
    );

    // B2: monotone — coarser tessellation yields larger deviation
    assert!(
        dev_c > dev_f,
        "B2: sphere deviation must be strictly monotone: coarse ({dev_c}) > fine ({dev_f})"
    );
}

// ── B2: curved cylinder — coarse deviation > fine deviation (monotone) ───────

/// B2-cylinder: same monotonicity assertion for a cylinder (curved lateral
/// face + flat top/bottom caps). Coarse > fine strictly; fine > 1e-7 m.
#[test]
fn b2_cylinder_deviation_is_monotone_in_deflection() {
    let coarse_tol = 5e-2_f64;
    let fine_tol = 5e-4_f64;

    let (kernel_c, cyl_id_c) = make_shape(cylinder_op(1.0, 2.0));
    let mesh_c = kernel_c
        .tessellate(cyl_id_c, coarse_tol)
        .expect("cylinder coarse tessellation should succeed");
    let dev_c = kernel_c
        .measure_mesh_deviation(cyl_id_c, &mesh_c)
        .expect("measure_mesh_deviation should return Ok");

    let (kernel_f, cyl_id_f) = make_shape(cylinder_op(1.0, 2.0));
    let mesh_f = kernel_f
        .tessellate(cyl_id_f, fine_tol)
        .expect("cylinder fine tessellation should succeed");
    let dev_f = kernel_f
        .measure_mesh_deviation(cyl_id_f, &mesh_f)
        .expect("measure_mesh_deviation should return Ok");

    // B3-numeric
    assert!(dev_c.is_finite() && dev_c >= 0.0, "coarse dev must be finite ≥ 0, got {dev_c}");
    assert!(dev_f.is_finite() && dev_f >= 0.0, "fine dev must be finite ≥ 0, got {dev_f}");

    // B2
    assert!(
        dev_f > 1e-7,
        "B2: cylinder fine deviation must be > 1e-7 m (above f32 noise floor), got {dev_f}"
    );
    assert!(
        dev_c > dev_f,
        "B2: cylinder deviation must be strictly monotone: coarse ({dev_c}) > fine ({dev_f})"
    );
}

// ── B3: invalid handle → Err ──────────────────────────────────────────────────

/// B3-handle: an unknown `GeometryHandleId` returns `Err(InvalidHandle)`,
/// not a misleading 0.0 or panic.
#[test]
fn b3_invalid_handle_returns_err() {
    let kernel = OcctKernel::new();
    // Use a handle that was never stored in this kernel instance.
    let bad_id = reify_ir::GeometryHandleId(9_999_999);
    let dummy_mesh = reify_ir::Mesh {
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        indices: vec![0, 1, 2],
        normals: None,
    };
    let result = kernel.measure_mesh_deviation(bad_id, &dummy_mesh);
    assert!(
        result.is_err(),
        "B3: invalid handle should return Err, got Ok({:?})",
        result.ok()
    );
}

// ── Step-3: actor-channel parity + dyn-trait reachability ────────────────────
//
// These tests verify that `OcctKernelHandle::measure_mesh_deviation` routes the
// measurement request correctly through the actor channel and returns the SAME
// value as a direct `OcctKernel::measure_mesh_deviation` call on the same shape
// and mesh.
//
// They also verify that the method is reachable through `&dyn GeometryKernel`,
// returning `Some` (the OCCT handle overrides the default-absent trait method).
//
// These tests are RED because `OcctRequest::MeasureMeshDeviation`, the actor
// handler, the `OcctKernelHandle` inherent method, and the trait override do
// not yet exist (step-4 will implement them).

/// Actor-channel parity: `OcctKernelHandle::measure_mesh_deviation` returns the
/// same value as `OcctKernel::measure_mesh_deviation` for the same shape and mesh,
/// within 1e-9 m (channel round-trip carries vertices/indices faithfully).
#[test]
fn handle_deviation_matches_direct_kernel() {
    let coarse_tol = 5e-2_f64;

    // --- Direct kernel (reference value) ---
    let (direct_kernel, direct_sphere_id) = make_shape(sphere_op(1.0));
    let direct_mesh = direct_kernel
        .tessellate(direct_sphere_id, coarse_tol)
        .expect("direct kernel: tessellation should succeed");
    let direct_dev = direct_kernel
        .measure_mesh_deviation(direct_sphere_id, &direct_mesh)
        .expect("direct kernel: measure_mesh_deviation should return Ok");

    // --- Actor handle (must match) ---
    let handle = OcctKernelHandle::spawn();
    let sphere_gh = handle
        .execute(&sphere_op(1.0))
        .expect("actor handle: execute should succeed");
    let handle_mesh = handle
        .tessellate(sphere_gh.id, coarse_tol)
        .expect("actor handle: tessellation should succeed");
    // OcctKernelHandle::measure_mesh_deviation does not exist yet (step-4
    // implements it) — this call makes the test RED.
    let handle_dev = handle
        .measure_mesh_deviation(sphere_gh.id, &handle_mesh)
        .expect("actor handle: measure_mesh_deviation should return Some");

    assert!(
        (handle_dev - direct_dev).abs() < 1e-9,
        "actor-channel parity: handle_dev ({handle_dev}) and direct_dev ({direct_dev}) \
         must agree within 1e-9 m"
    );
}

/// Dyn-trait reachability: through `&dyn GeometryKernel`, the trait method
/// `measure_mesh_deviation` returns `Some` for an `OcctKernelHandle`.
///
/// This confirms the trait override (step-4) is wired and the engine's
/// `&dyn GeometryKernel` call site will get a real measurement (not the
/// default `None`).
#[test]
fn dyn_trait_measure_mesh_deviation_returns_some() {
    let handle = OcctKernelHandle::spawn();
    let sphere_gh = handle
        .execute(&sphere_op(1.0))
        .expect("dyn-trait test: execute should succeed");
    let mesh = handle
        .tessellate(sphere_gh.id, 5e-2)
        .expect("dyn-trait test: tessellation should succeed");

    // Cast to &dyn GeometryKernel — exercises the trait dispatch path.
    let dyn_kernel: &dyn GeometryKernel = &handle;
    let result = dyn_kernel.measure_mesh_deviation(sphere_gh.id, &mesh);

    assert!(
        result.is_some(),
        "dyn-trait: measure_mesh_deviation should return Some for OcctKernelHandle, got None"
    );
    let dev = result.unwrap();
    assert!(
        dev.is_finite() && dev >= 0.0,
        "dyn-trait: deviation must be finite ≥ 0, got {dev}"
    );
    assert!(
        dev > 1e-7,
        "dyn-trait: sphere deviation (coarse) must be > 1e-7 m (curved surface), got {dev}"
    );
}
