//! Integration tests for `geo_equiv` — tests whether two shapes are
//! geometrically equivalent within a tolerance by (1) topology-count
//! matching and (2) sampled-vertex distance checking.
//!
//! Algorithm:
//!   (1) Topology check: compare per-kind (vertex/edge/face) counts via
//!       `TopExp::MapShapes` for both shapes; mismatch → false immediately.
//!   (2) Sampling: for each face / edge in canonical order, evaluate `N`
//!       uniform parameter points on both shapes and require every
//!       `|p_a − p_b| < tolerance`.
//!
//! Semantics: `geo_equiv(a, b, tol)` returns `true` iff both shapes have
//! identical topology counts AND every sampled point is within `tol`.
//! This is the conventional "same shape within tolerance" predicate per
//! PRD §5.1 (KGQ-δ).
//!
//! Fixture units: boxes/cylinders are built with `Value::Real(...)` which
//! maps directly to the kernel's coordinate system (10.0 = 10 mm, etc.).
//! Faces of a `Value::Real(10.0)` box lie at ±5.0.  Tolerance values are
//! in the same raw coordinate units.
//!
//! Tests:
//! - box(10,10,10) vs box(10,10,10), tol=1e-6 → Ok(true) [identical].
//! - box(10,10,10) vs box(10+2e-7,10,10), tol=1e-6 → Ok(true) [within-tol:
//!   face displacement = 1e-7 < tol].
//! - box(10,10,10) vs box(10.02,10,10), tol=1e-6 → Ok(false) [gross:
//!   displacement = 0.01 >> tol; topology matches so the SAMPLE arm fails].
//! - box(10,10,10) vs cylinder(5,10), tol=1e-6 → Ok(false) [topology
//!   mismatch: 6 faces vs 3 faces].
//! - Unknown handle → `Err(QueryError::InvalidHandle)`.
//! - Negative tolerance → `Err(QueryError::QueryFailed)`.

#![cfg(has_occt)]

use reify_ir::{GeometryHandleId, GeometryOp, QueryError, Value};
use reify_kernel_occt::OcctKernel;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Build a kernel with a single box primitive.
///
/// Returns `(kernel, handle_id)`.
fn box_kernel(width: f64, height: f64, depth: f64) -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(width),
            height: Value::Real(height),
            depth: Value::Real(depth),
        })
        .expect("box creation should succeed");
    (kernel, handle.id)
}

/// Build a kernel with two primitives: a box and a cylinder.
///
/// Returns `(kernel, box_id, cyl_id)`.
fn box_and_cylinder_kernel() -> (OcctKernel, GeometryHandleId, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let box_handle = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("box creation should succeed");
    let cyl_handle = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(5.0),
            height: Value::Real(10.0),
        })
        .expect("cylinder creation should succeed");
    (kernel, box_handle.id, cyl_handle.id)
}

// ---------------------------------------------------------------------------
// Topology match + sampling — happy path
// ---------------------------------------------------------------------------

/// Identical boxes (same dimensions) → topology matches, all sampled points
/// coincide (displacement 0 < tol) → Ok(true).
#[test]
fn geo_equiv_identical_boxes_returns_true() {
    let (mut kernel, a) = box_kernel(10.0, 10.0, 10.0);
    let b = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("second box creation should succeed")
        .id;
    match kernel.geo_equiv(a, b, 1e-6) {
        Ok(true) => {}
        Ok(false) => panic!("expected true for identical boxes, got false"),
        Err(e) => panic!("expected Ok(true) for identical boxes, got Err({e:?})"),
    }
}

/// Within-tolerance: box(10,10,10) vs box(10+2e-7,10,10).
/// Face displacement along width axis = (2e-7)/2 = 1e-7 < tol=1e-6.
/// Topology matches (6F/12E/8V each) → sample arm runs → all |p_a−p_b| < tol.
/// Expected: Ok(true).
#[test]
fn geo_equiv_within_tol_boxes_returns_true() {
    let (mut kernel, a) = box_kernel(10.0, 10.0, 10.0);
    let b = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0 + 2e-7),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("slightly-wider box creation should succeed")
        .id;
    match kernel.geo_equiv(a, b, 1e-6) {
        Ok(true) => {}
        Ok(false) => panic!("expected true for within-tol boxes, got false"),
        Err(e) => panic!("expected Ok(true) for within-tol boxes, got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// Sample arm fails (gross displacement)
// ---------------------------------------------------------------------------

/// Gross displacement: box(10,10,10) vs box(10.02,10,10).
/// Face displacement along width = 0.01 >> tol=1e-6.
/// Topology matches (6F/12E/8V each) so we reach the sampling arm, which fails.
/// Expected: Ok(false).
#[test]
fn geo_equiv_gross_displacement_boxes_returns_false() {
    let (mut kernel, a) = box_kernel(10.0, 10.0, 10.0);
    let b = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.02),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("wider box creation should succeed")
        .id;
    match kernel.geo_equiv(a, b, 1e-6) {
        Ok(false) => {}
        Ok(true) => panic!("expected false for grossly displaced boxes, got true"),
        Err(e) => panic!("expected Ok(false) for grossly displaced boxes, got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// Topology mismatch arm
// ---------------------------------------------------------------------------

/// Topology mismatch: box(10,10,10) vs cylinder(5,10).
/// Box: 6 faces / 12 edges / 8 vertices.
/// Cylinder: 3 faces / 3 edges / 2 vertices.
/// Face-count mismatch → topology arm fires immediately → false.
/// Expected: Ok(false).
#[test]
fn geo_equiv_box_vs_cylinder_returns_false() {
    let (kernel, box_id, cyl_id) = box_and_cylinder_kernel();
    match kernel.geo_equiv(box_id, cyl_id, 1e-6) {
        Ok(false) => {}
        Ok(true) => panic!("expected false for box vs cylinder (topology mismatch), got true"),
        Err(e) => panic!(
            "expected Ok(false) for box vs cylinder, got Err({e:?})"
        ),
    }
}

// ---------------------------------------------------------------------------
// Error conditions
// ---------------------------------------------------------------------------

/// Unknown handle → `Err(QueryError::InvalidHandle)`.
#[test]
fn geo_equiv_invalid_handle_returns_err() {
    let (kernel, box_id, _) = box_and_cylinder_kernel();
    let unknown = GeometryHandleId(9999);
    match kernel.geo_equiv(box_id, unknown, 1e-6) {
        Err(QueryError::InvalidHandle(_)) => {}
        other => panic!("expected Err(InvalidHandle) for unknown handle, got {:?}", other),
    }
}

/// Negative tolerance → `Err(QueryError::QueryFailed)` (tolerance precondition).
#[test]
fn geo_equiv_negative_tolerance_returns_err() {
    let (kernel, box_id, cyl_id) = box_and_cylinder_kernel();
    match kernel.geo_equiv(box_id, cyl_id, -1e-6) {
        Err(QueryError::QueryFailed(_)) => {}
        other => panic!(
            "expected Err(QueryFailed) for negative tolerance, got {:?}",
            other
        ),
    }
}

/// NaN tolerance → `Err(QueryError::QueryFailed)` (isfinite precondition).
///
/// Pins the `std::isfinite` half of the precondition guard alongside the
/// existing negative-tolerance test.
#[test]
fn geo_equiv_nan_tolerance_returns_err() {
    let (kernel, box_id, cyl_id) = box_and_cylinder_kernel();
    match kernel.geo_equiv(box_id, cyl_id, f64::NAN) {
        Err(QueryError::QueryFailed(_)) => {}
        other => panic!(
            "expected Err(QueryFailed) for NaN tolerance, got {:?}",
            other
        ),
    }
}

/// Zero tolerance → `Err(QueryError::QueryFailed)` (must be strictly positive).
///
/// With tol_sq == 0 the comparison `pa.SquareDistance(pb) >= tol_sq` would always
/// be true (0 >= 0), so even identical shapes would return false.  The precondition
/// rejects zero to prevent this silent trap.
#[test]
fn geo_equiv_zero_tolerance_returns_err() {
    let (kernel, box_id, cyl_id) = box_and_cylinder_kernel();
    match kernel.geo_equiv(box_id, cyl_id, 0.0) {
        Err(QueryError::QueryFailed(_)) => {}
        other => panic!(
            "expected Err(QueryFailed) for zero tolerance, got {:?}",
            other
        ),
    }
}
