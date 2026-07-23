//! Integration tests for `contains` — tests whether a 3D point lies inside or
//! on the boundary of a closed solid using `BRepClass3d_SolidClassifier`.
//!
//! Algorithm: `BRepClass3d_SolidClassifier(shape).Perform(gp_Pnt, tolerance)`;
//! returns `State() == TopAbs_IN || State() == TopAbs_ON`.
//!
//! Semantics: `contains` returns `true` for points that are strictly inside the
//! solid (TopAbs_IN) *or* on its boundary surface (TopAbs_ON), and `false` for
//! points strictly outside (TopAbs_OUT). This is the conventional closed-solid
//! membership predicate per PRD §8.1.
//!
//! Fixture: a 10×10×10 box centered at the origin (x∈[-5,5], y∈[-5,5], z∈[-5,5]).
//! Coordinates use the same unit system as `Value::Real(10.0)` = 10mm, so the
//! face is at ±5.0.
//!
//! Tests:
//! - Center (0,0,0) → Ok(true) (TopAbs_IN).
//! - Face center (5,0,0) → Ok(true) (TopAbs_ON).
//! - Corner vertex (5,5,5) → Ok(true) (TopAbs_ON).
//! - Far outside (20,0,0) → Ok(false) (TopAbs_OUT).
//! - Unknown handle → `Err(QueryError::InvalidHandle)`.
//! - Negative tolerance → `Err(QueryError::QueryFailed)` (tolerance precondition).

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_ir::{GeometryHandleId, GeometryOp, QueryError, Value};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Build a kernel with a single 10×10×10 box centered at the origin
/// (x∈[-5,5], y∈[-5,5], z∈[-5,5]).
///
/// Returns `(kernel, box_id)`.
fn box_kernel() -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("box creation should succeed");
    (kernel, handle.id)
}

// ---------------------------------------------------------------------------
// Happy path — points inside or on the surface
// ---------------------------------------------------------------------------

/// Center of the box (0,0,0) is strictly inside → TopAbs_IN → true.
#[test]
fn contains_center_returns_true() {
    let (kernel, box_id) = box_kernel();
    match kernel.contains(box_id, 0.0, 0.0, 0.0, 1e-7) {
        Ok(true) => {}
        Ok(false) => panic!("expected true for center (0,0,0), got false"),
        Err(e) => panic!("expected Ok(true), got Err({e:?})"),
    }
}

/// Face center (5,0,0) lies exactly on the +X face → TopAbs_ON → true.
#[test]
fn contains_face_center_returns_true() {
    let (kernel, box_id) = box_kernel();
    match kernel.contains(box_id, 5.0, 0.0, 0.0, 1e-7) {
        Ok(true) => {}
        Ok(false) => panic!("expected true for face center (5,0,0), got false"),
        Err(e) => panic!("expected Ok(true), got Err({e:?})"),
    }
}

/// Corner vertex (5,5,5) is on the boundary → TopAbs_ON → true.
#[test]
fn contains_corner_returns_true() {
    let (kernel, box_id) = box_kernel();
    match kernel.contains(box_id, 5.0, 5.0, 5.0, 1e-7) {
        Ok(true) => {}
        Ok(false) => panic!("expected true for corner (5,5,5), got false"),
        Err(e) => panic!("expected Ok(true), got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// Outside points
// ---------------------------------------------------------------------------

/// Far outside (20,0,0) — well outside the [-5,5] box → TopAbs_OUT → false.
#[test]
fn contains_far_outside_returns_false() {
    let (kernel, box_id) = box_kernel();
    match kernel.contains(box_id, 20.0, 0.0, 0.0, 1e-7) {
        Ok(false) => {}
        Ok(true) => panic!("expected false for far-outside (20,0,0), got true"),
        Err(e) => panic!("expected Ok(false), got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// Error conditions
// ---------------------------------------------------------------------------

/// Unknown handle → `Err(QueryError::InvalidHandle)`.
#[test]
fn contains_invalid_handle_returns_err() {
    let (kernel, _) = box_kernel();
    let unknown = GeometryHandleId(9999);
    match kernel.contains(unknown, 0.0, 0.0, 0.0, 1e-7) {
        Err(QueryError::InvalidHandle(_)) => {}
        other => panic!("expected Err(InvalidHandle), got {:?}", other),
    }
}

/// Negative tolerance → `Err(QueryError::QueryFailed)` (tolerance precondition).
#[test]
fn contains_negative_tolerance_returns_err() {
    let (kernel, box_id) = box_kernel();
    match kernel.contains(box_id, 0.0, 0.0, 0.0, -1e-7) {
        Err(QueryError::QueryFailed(_)) => {}
        other => panic!("expected Err(QueryFailed) for negative tolerance, got {:?}", other),
    }
}

/// NaN tolerance → `Err(QueryError::QueryFailed)` (non-finite tolerance
/// precondition). The C++ guard is `std::isfinite(tolerance) && tolerance >= 0.0`;
/// `f64::NAN` fails `isfinite`, so this is an independent branch from the
/// negative-tolerance test above.
#[test]
fn contains_nan_tolerance_returns_err() {
    let (kernel, box_id) = box_kernel();
    match kernel.contains(box_id, 0.0, 0.0, 0.0, f64::NAN) {
        Err(QueryError::QueryFailed(_)) => {}
        other => panic!("expected Err(QueryFailed) for NaN tolerance, got {:?}", other),
    }
}

/// Infinite tolerance → `Err(QueryError::QueryFailed)` (non-finite tolerance
/// precondition). `f64::INFINITY` also fails `isfinite`.
#[test]
fn contains_infinite_tolerance_returns_err() {
    let (kernel, box_id) = box_kernel();
    match kernel.contains(box_id, 0.0, 0.0, 0.0, f64::INFINITY) {
        Err(QueryError::QueryFailed(_)) => {}
        other => panic!("expected Err(QueryFailed) for infinite tolerance, got {:?}", other),
    }
}
