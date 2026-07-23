//! Integration tests for `OcctKernel::curve_curvature_at` and the
//! `GeometryQuery::CurveCurvatureAt` / `GeometryQuery::SurfaceCurvatureAt`
//! dispatch arms.
//!
//! Ground truth:
//! - **Circle r=0.01m (10mm)**: curvature κ = 1/r = 100 m⁻¹ (exact).
//! - **Sphere r=5m**: surface mean curvature H = -1/r = -0.2 m⁻¹ (OCCT
//!   sign convention: negative for convex outward normal).  2×2 principal
//!   matrix trace/2 = H; kappa_max = kappa_min = -1/r (umbilical).
//!
//! Circular-edge fixture is built via `GeometryOp::Arc` (start=0, end=2π)
//! so we stay inside the Rust OcctKernel API rather than calling OCCT
//! constructors directly.

#![cfg(all(has_occt, feature = "test-fixtures"))]

use std::f64::consts::TAU;

use reify_kernel_occt::OcctKernel;
use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery, QueryError, Value};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Build a kernel with a full-circle arc (edge) of the given radius in the
/// XY plane.  The wire returned by `Arc(start=0, end=TAU)` contains exactly
/// one circular edge; we extract and return that edge handle.
fn circle_edge(kernel: &mut OcctKernel, radius: f64) -> GeometryHandleId {
    let wire = kernel
        .execute(&GeometryOp::Arc {
            center: [0.0, 0.0, 0.0],
            radius,
            start_angle: 0.0,
            end_angle: TAU,
            axis: [0.0, 0.0, 1.0],
        })
        .expect("Arc should create a full-circle wire");
    let edges = kernel
        .extract_edges(wire.id)
        .expect("extract_edges should succeed on circle wire");
    assert_eq!(edges.len(), 1, "full-circle wire should have exactly 1 edge");
    edges[0]
}

/// Build a kernel with a sphere of the given radius.
/// Returns `(kernel, sphere_handle_id)`.
fn sphere_kernel(radius: f64) -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Sphere {
            radius: Value::Real(radius),
        })
        .expect("sphere creation should succeed");
    (kernel, handle.id)
}

// ---------------------------------------------------------------------------
// OcctKernel::curve_curvature_at — happy-path: circular edge
// ---------------------------------------------------------------------------

/// `OcctKernel::curve_curvature_at` on a circular edge of radius r=0.01m
/// returns κ ≈ 1/r = 100 m⁻¹ within 1e-6 relative error.
///
/// Curvature of a circle is the exact value 1/r — no floating-point
/// uncertainty, only OCCT's internal double precision.
///
/// Point chosen on the circle: (r, 0, 0), the closest XY-plane intercept.
#[test]
fn curve_curvature_at_on_circle_edge_yields_one_over_r() {
    let r = 0.01_f64; // 10 mm in SI metres
    let mut kernel = OcctKernel::new();
    let edge = circle_edge(&mut kernel, r);

    // Query point: (r, 0, 0) — on the circle in the XY plane.
    let kappa = kernel
        .curve_curvature_at(edge, r, 0.0, 0.0)
        .expect("curve_curvature_at should succeed for a circular edge");

    let expected = 1.0 / r; // = 100 m⁻¹
    let rel_err = (kappa.abs() - expected).abs() / expected;
    assert!(
        rel_err < 1e-6,
        "circle curvature: expected |κ| = 1/r = {expected}, got {kappa} (rel_err = {rel_err})"
    );
}

// ---------------------------------------------------------------------------
// OcctKernel::curve_curvature_at — error path: invalid handle
// ---------------------------------------------------------------------------

/// `OcctKernel::curve_curvature_at` returns `Err(QueryError::InvalidHandle)`
/// for a handle that was never registered.
#[test]
fn curve_curvature_at_on_invalid_handle_returns_error() {
    let kernel = OcctKernel::new();
    let bogus = GeometryHandleId(999_999);
    let result = kernel.curve_curvature_at(bogus, 0.0, 0.0, 0.0);
    match result {
        Err(QueryError::InvalidHandle(_)) | Err(QueryError::QueryFailed(_)) => {}
        Ok(v) => panic!("expected Err for invalid handle, got Ok({v:?})"),
        Err(e) => panic!("expected InvalidHandle or QueryFailed, got {e:?}"),
    }
}

// ---------------------------------------------------------------------------
// GeometryQuery::CurveCurvatureAt dispatch through kernel.query()
// ---------------------------------------------------------------------------

/// `kernel.query(&GeometryQuery::CurveCurvatureAt{..})` returns
/// `Value::Real(κ)` where |κ| ≈ 1/r within 1e-6 relative.
///
/// This exercises the OcctKernel::query() dispatch arm added by KGQ-μ.
#[test]
fn geometry_query_curve_curvature_at_returns_value_real() {
    let r = 0.01_f64;
    let mut kernel = OcctKernel::new();
    let edge = circle_edge(&mut kernel, r);

    let reply = kernel
        .query(&GeometryQuery::CurveCurvatureAt {
            handle: edge,
            px: r,
            py: 0.0,
            pz: 0.0,
        })
        .expect("GeometryQuery::CurveCurvatureAt should succeed on circle edge");

    let kappa = match reply {
        Value::Real(v) => v,
        other => panic!("CurveCurvatureAt reply should be Value::Real, got {other:?}"),
    };

    let expected = 1.0 / r;
    let rel_err = (kappa.abs() - expected).abs() / expected;
    assert!(
        rel_err < 1e-6,
        "CurveCurvatureAt: expected |κ| = 1/r = {expected}, got {kappa} (rel_err = {rel_err})"
    );
}

// ---------------------------------------------------------------------------
// GeometryQuery::SurfaceCurvatureAt dispatch through kernel.query()
// ---------------------------------------------------------------------------

/// `kernel.query(&GeometryQuery::SurfaceCurvatureAt{..})` on a sphere face
/// returns a `Value::List([[kappa_max, 0], [0, kappa_min]])` 2×2 matrix.
///
/// For a sphere of radius r=5:
///   kappa_max = kappa_min = -1/r = -0.2 (umbilical; OCCT sign: negative for
///   convex outward normal).
///   trace/2 = (kappa_max + kappa_min) / 2 = H = -0.2.
///
/// Test asserts |trace/2 + 1/r| < 1e-9.
#[test]
fn geometry_query_surface_curvature_at_on_sphere_returns_2x2_principal_matrix() {
    use std::f64::consts::PI;

    let r = 5.0_f64;
    let (mut kernel, sphere_id) = sphere_kernel(r);
    let faces = kernel
        .extract_faces(sphere_id)
        .expect("extract_faces should succeed for sphere");
    let face = faces[0];

    // (u=π, v=0): safe interior parametric point away from poles.
    let reply = kernel
        .query(&GeometryQuery::SurfaceCurvatureAt {
            handle: face,
            u: PI,
            v: 0.0,
        })
        .expect("GeometryQuery::SurfaceCurvatureAt should succeed on sphere face");

    // Decode outer Value::List (rows).
    let rows = match reply {
        Value::List(rows) => rows,
        other => panic!("SurfaceCurvatureAt reply should be Value::List, got {other:?}"),
    };
    assert_eq!(rows.len(), 2, "2×2 matrix should have 2 rows");

    // Decode each row.
    let row0 = match &rows[0] {
        Value::List(r) => r.clone(),
        other => panic!("row0 should be Value::List, got {other:?}"),
    };
    let row1 = match &rows[1] {
        Value::List(r) => r.clone(),
        other => panic!("row1 should be Value::List, got {other:?}"),
    };
    assert_eq!(row0.len(), 2, "row0 should have 2 elements");
    assert_eq!(row1.len(), 2, "row1 should have 2 elements");

    let kappa_max = match row0[0] {
        Value::Real(v) => v,
        ref other => panic!("row0[0] should be Value::Real, got {other:?}"),
    };
    let off_01 = match row0[1] {
        Value::Real(v) => v,
        ref other => panic!("row0[1] should be Value::Real, got {other:?}"),
    };
    let off_10 = match row1[0] {
        Value::Real(v) => v,
        ref other => panic!("row1[0] should be Value::Real, got {other:?}"),
    };
    let kappa_min = match row1[1] {
        Value::Real(v) => v,
        ref other => panic!("row1[1] should be Value::Real, got {other:?}"),
    };

    // Off-diagonal elements must be 0.0 (diagonal encoding).
    assert_eq!(off_01, 0.0, "off-diagonal [0,1] should be 0.0");
    assert_eq!(off_10, 0.0, "off-diagonal [1,0] should be 0.0");

    // Sphere mean curvature H = (kappa_max + kappa_min) / 2 = -1/r (OCCT sign convention).
    let mean = (kappa_max + kappa_min) / 2.0;
    let tol = 1e-9;
    assert!(
        (mean + 1.0 / r).abs() < tol,
        "sphere surface mean curvature: expected H = -1/r = {}, got H = {mean}",
        -1.0 / r
    );
}
