//! End-to-end smoke test for `curvature(Surface, Point3<Length>)` and
//! `curvature(Curve, Point3<Length>)` (task 3621, PRD
//! `docs/prds/v0_3/kernel-geometry-queries.md` §9 KGQ-μ).
//!
//! The fixture `examples/kernel_queries/curvature_smoke.ri` contains:
//!
//! ```ri
//! structure def CurvatureSmoke {
//!     let s       = sphere(5mm)
//!     let cyl     = cylinder(10mm, 20mm)
//!     let pt_s    = point3(5mm, 0mm, 0mm)
//!     let pt_c    = point3(10mm, 0mm, 0mm)
//!     let k_surf  = curvature(s, pt_s)
//!     let k_curve = curvature(cyl, pt_c)
//! }
//! ```
//!
//! Two assertions:
//!
//! 1. **COMPILE-LEVEL** (always) — `curvature_smoke.ri` parses and compiles
//!    with no error-severity diagnostics. `curvature` is registered in
//!    `units.rs` under `GEOMETRY_QUERY_NAMES` (task 3621, KGQ-μ), so the cell
//!    type resolves to `Scalar<Curvature>`.  At DSL eval time both cells
//!    resolve to `Value::Undef` because solid handles fail the
//!    face/edge kernel queries (pre-Phase-3 sub-handle chaining); only
//!    Warning diagnostics are emitted — not Errors — so the compile assertion
//!    holds.
//!
//! 2. **OCCT-BACKED RUNTIME** (gated on `reify_kernel_occt::OCCT_AVAILABLE`) —
//!    Spawn a real `OcctKernelHandle`, build the geometry directly at the kernel
//!    level, and call `kernel.query()` to confirm the `OcctKernel::query`
//!    dispatch arms are live:
//!
//!    - `GeometryQuery::SurfaceCurvatureAt` on a face of `sphere(5mm)` returns
//!      a 2×2 `Value::List` matrix whose trace/2 = mean curvature
//!      H ≈ -1/r = -200 m⁻¹ (|H| within 1e-6 relative of 1/0.005).
//!
//!    - `GeometryQuery::CurveCurvatureAt` on a circular edge of radius 10mm
//!      (built via `GeometryOp::Arc`, full circle) returns `Value::Real(κ)`
//!      with |κ| ≈ 1/r = 100 m⁻¹ (within 1e-6 relative of 1/0.01).
//!
//! Modelled on `kernel_queries_normal_smoke.rs` (same compile + OcctKernelHandle
//! direct-query pattern) and `curve_curvature_integration.rs` (sphere/circle
//! fixture shapes and tolerance conventions).

use reify_ir::{GeometryOp, GeometryQuery, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};
use std::f64::consts::{PI, TAU};

const CURVATURE_SMOKE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/curvature_smoke.ri"
);

/// Pins the user-observable signal for KGQ-μ:
///
/// - `CurvatureSmoke.k_surf` and `CurvatureSmoke.k_curve` compile with no
///   error diagnostics, confirming `curvature` is registered in `units.rs`.
/// - `GeometryQuery::SurfaceCurvatureAt` on a sphere face and
///   `GeometryQuery::CurveCurvatureAt` on a circle edge through the real OCCT
///   kernel return the correct curvature values, confirming the
///   `OcctKernel::query()` dispatch and FFI chain are live.
///
/// Skips the OCCT-backed assertions cleanly when OCCT is not available.
#[test]
fn curvature_smoke_compiles_and_occt_query_chain_live() {
    // ── assertion 1: fixture exists and compiles with no ERROR diagnostics ────

    let source = std::fs::read_to_string(CURVATURE_SMOKE_PATH)
        .expect("examples/kernel_queries/curvature_smoke.ri should exist (task 3621 step-7)");

    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/kernel_queries/curvature_smoke.ri should compile with no \
         error-severity diagnostics (Warnings from solid-handle Undef eval are \
         acceptable pre-Phase-3), got:\n{:#?}",
        errors_only(&compiled)
    );

    // ── assertion 2: real-OCCT OcctKernel::query dispatch arms are live ───────

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return;
    }

    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();

    // ── 2a: SurfaceCurvatureAt on sphere(5mm) face ────────────────────────────

    // r = 5mm in SI metres.
    let r_surf = 0.005_f64; // 5mm
    let sphere_handle = kernel
        .execute(&GeometryOp::Sphere {
            radius: Value::Real(r_surf),
        })
        .expect("sphere(5mm) should build successfully");

    let faces = kernel
        .extract_faces(sphere_handle.id)
        .expect("extract_faces should succeed on sphere");
    assert!(!faces.is_empty(), "sphere should have at least one face");

    // Parametric point (u=π, v=0): safe interior point away from poles,
    // consistent with curve_curvature_integration.rs.
    let surf_reply = kernel
        .query(&GeometryQuery::SurfaceCurvatureAt {
            handle: faces[0],
            u: PI,
            v: 0.0,
        })
        .expect("SurfaceCurvatureAt on sphere face should succeed");

    // Decode the 2×2 principal-curvature matrix reply.
    let rows = match surf_reply {
        Value::List(ref rows) if rows.len() == 2 => rows,
        other => panic!(
            "SurfaceCurvatureAt on sphere face should return Value::List(2 rows), \
             got: {other:?}"
        ),
    };
    let row0 = match &rows[0] {
        Value::List(r) if r.len() == 2 => r,
        other => panic!("row0 should be Value::List(2), got: {other:?}"),
    };
    let row1 = match &rows[1] {
        Value::List(r) if r.len() == 2 => r,
        other => panic!("row1 should be Value::List(2), got: {other:?}"),
    };
    let kappa_max = match row0[0] {
        Value::Real(v) => v,
        ref other => panic!("row0[0] should be Value::Real, got: {other:?}"),
    };
    let kappa_min = match row1[1] {
        Value::Real(v) => v,
        ref other => panic!("row1[1] should be Value::Real, got: {other:?}"),
    };
    // Mean curvature H = (kappa_max + kappa_min) / 2 = -1/r (OCCT: convex → negative).
    let mean_surf = (kappa_max + kappa_min) / 2.0;
    let expected_surf = 1.0 / r_surf; // 200 m⁻¹
    let rel_err_surf = (mean_surf.abs() - expected_surf).abs() / expected_surf;
    assert!(
        rel_err_surf < 1e-6,
        "SurfaceCurvatureAt sphere(5mm): expected |mean| = 1/r = {expected_surf} m⁻¹, \
         got mean = {mean_surf} (|mean| = {}, rel_err = {rel_err_surf})",
        mean_surf.abs()
    );

    // ── 2b: CurveCurvatureAt on circular arc of radius 10mm ──────────────────

    // r = 10mm in SI metres.
    let r_curve = 0.01_f64; // 10mm
    let arc_handle = kernel
        .execute(&GeometryOp::Arc {
            center: [0.0, 0.0, 0.0],
            radius: r_curve,
            start_angle: 0.0,
            end_angle: TAU, // full circle
            axis: [0.0, 0.0, 1.0],
        })
        .expect("Arc (full circle r=10mm) should build successfully");

    let edges = kernel
        .extract_edges(arc_handle.id)
        .expect("extract_edges should succeed on circle arc");
    assert_eq!(edges.len(), 1, "full-circle arc should have exactly 1 edge");

    // Probe point: (r, 0, 0) — on the circle in the XY plane.
    let curve_reply = kernel
        .query(&GeometryQuery::CurveCurvatureAt {
            handle: edges[0],
            px: r_curve,
            py: 0.0,
            pz: 0.0,
        })
        .expect("CurveCurvatureAt on circle edge should succeed");

    let kappa = match curve_reply {
        Value::Real(v) => v,
        other => {
            panic!("CurveCurvatureAt on circle edge should return Value::Real(κ), got: {other:?}")
        }
    };
    let expected_curve = 1.0 / r_curve; // 100 m⁻¹
    let rel_err_curve = (kappa.abs() - expected_curve).abs() / expected_curve;
    assert!(
        rel_err_curve < 1e-6,
        "CurveCurvatureAt circle r=10mm: expected |κ| = 1/r = {expected_curve} m⁻¹, \
         got κ = {kappa} (|κ| = {}, rel_err = {rel_err_curve})",
        kappa.abs()
    );
}
