//! Integration tests for `point_on_shape` — tests whether a 3D point lies on
//! a `TopoDS_Shape`'s BREP boundary (face/edge/vertex) within a given tolerance.
//!
//! Algorithm: `BRepExtrema_DistShapeShape(shape, vertex)` where the vertex is
//! built from the query point.  Returns `dist.Value() <= tolerance`.
//!
//! **Interior solid points — OCCT overlap behavior:** when the query vertex is
//! inside a `TopoDS_Solid`, `BRepExtrema_DistShapeShape` considers the shapes to
//! overlap and returns `dist.Value() = 0` (not the distance to the nearest
//! boundary face).  Therefore `point_on_shape` returns `true` for interior solid
//! points with any positive tolerance.  This means the primitive cannot distinguish
//! on-surface from inside-solid for `TopoDS_Solid` shapes; see escalation esc-2829-6.
//!
//! Fixture: a 10×10×10 box centered at the origin (x∈[-5,5], y∈[-5,5], z∈[-5,5]).
//!
//! Tests:
//! - Face center (5,0,0) → true.
//! - Edge midpoint (5,5,0) → true.
//! - Corner vertex (5,5,5) → true.
//! - External point (10,0,0) → false.
//! - Interior point (0,0,0) with tolerance 1e-3 → true (OCCT dist=0 for solid interior).
//! - Point within tolerance of surface (5 + 5e-4, 0, 0) with tol=1e-3 → true.
//! - Point outside tolerance of surface (5 + 2e-3, 0, 0) with tol=1e-3 → false.
//! - Unknown handle → `QueryError::InvalidHandle`.

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_types::{GeometryHandleId, GeometryOp, QueryError, Value};

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
// Happy path — points on the surface
// ---------------------------------------------------------------------------

/// Query point (5.0, 0.0, 0.0) lies exactly on the +X face center.
/// With tolerance 1e-7 (≈ Precision::Confusion), the result should be true.
#[test]
fn point_on_shape_face_center_returns_true() {
    let (kernel, box_id) = box_kernel();
    match kernel.point_on_shape(box_id, 5.0, 0.0, 0.0, 1e-7) {
        Ok(true) => {}
        Ok(false) => panic!("expected true for face-center point (5,0,0), got false"),
        Err(e) => panic!("expected Ok(true), got Err({e:?})"),
    }
}

/// Query point (5.0, 5.0, 0.0) lies exactly on the +X/+Y edge midpoint.
/// With tolerance 1e-7, the result should be true.
#[test]
fn point_on_shape_edge_midpoint_returns_true() {
    let (kernel, box_id) = box_kernel();
    match kernel.point_on_shape(box_id, 5.0, 5.0, 0.0, 1e-7) {
        Ok(true) => {}
        Ok(false) => panic!("expected true for edge-midpoint (5,5,0), got false"),
        Err(e) => panic!("expected Ok(true), got Err({e:?})"),
    }
}

/// Query point (5.0, 5.0, 5.0) lies exactly on the +X/+Y/+Z corner vertex.
/// With tolerance 1e-7, the result should be true.
#[test]
fn point_on_shape_corner_returns_true() {
    let (kernel, box_id) = box_kernel();
    match kernel.point_on_shape(box_id, 5.0, 5.0, 5.0, 1e-7) {
        Ok(true) => {}
        Ok(false) => panic!("expected true for corner vertex (5,5,5), got false"),
        Err(e) => panic!("expected Ok(true), got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// External point — outside the box
// ---------------------------------------------------------------------------

/// Query point (10.0, 0.0, 0.0) is 5 units outside the +X face.
/// With tolerance 1e-7, the result should be false.
#[test]
fn point_on_shape_external_point_returns_false() {
    let (kernel, box_id) = box_kernel();
    match kernel.point_on_shape(box_id, 10.0, 0.0, 0.0, 1e-7) {
        Ok(false) => {}
        Ok(true) => panic!("expected false for external point (10,0,0), got true"),
        Err(e) => panic!("expected Ok(false), got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// Interior point — BRepExtrema solid-overlap behavior regression lock-in
// ---------------------------------------------------------------------------

/// Query point (0.0, 0.0, 0.0) lies strictly inside the 10×10×10 box.
///
/// **Regression lock-in for OCCT solid-overlap behavior:** when the query vertex
/// is inside a `TopoDS_Solid`, `BRepExtrema_DistShapeShape` considers the shapes
/// to overlap and returns `dist.Value() = 0` (not the distance to the nearest
/// boundary face).  Therefore `point_on_shape` returns `true` for interior solid
/// points, because `0.0 <= tolerance` for any positive tolerance.
///
/// This behavior means `point_on_shape` **cannot distinguish between a point on
/// the BREP surface and a point inside the solid** when the shape is a
/// `TopoDS_Solid`.  Callers that need strict surface-only membership should apply
/// a solid-classifier pre-filter (e.g. `BRepClass3d_SolidClassifier`) — that is
/// out of scope for this FFI primitive.  See escalation esc-2829-6 and parent
/// task 2324 for stdlib wiring decisions.
#[test]
fn point_on_shape_interior_solid_point_returns_true() {
    let (kernel, box_id) = box_kernel();
    // OCCT returns dist.Value() = 0 for interior solid points (overlap).
    // 0.0 <= 1e-3 → true.
    match kernel.point_on_shape(box_id, 0.0, 0.0, 0.0, 1e-3) {
        Ok(true) => {}
        Ok(false) => panic!(
            "expected true for interior solid point (0,0,0) with tol=1e-3 \
             (OCCT BRepExtrema returns dist=0 for solid interior, not 5.0), got false"
        ),
        Err(e) => panic!("expected Ok(true), got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// Tolerance boundary tests
// ---------------------------------------------------------------------------

/// Point (5.0 + 5e-4, 0.0, 0.0) is 5e-4 away from the +X face.
/// With tolerance 1e-3, the point is within tolerance → true.
#[test]
fn point_on_shape_within_tolerance_returns_true() {
    let (kernel, box_id) = box_kernel();
    let tol = 1e-3_f64;
    let px = 5.0 + 0.5 * tol; // 5.0005 — within tol=1e-3
    match kernel.point_on_shape(box_id, px, 0.0, 0.0, tol) {
        Ok(true) => {}
        Ok(false) => panic!(
            "expected true for point ({px}, 0, 0) with tol={tol} (distance ≈ 5e-4 ≤ tol), got false"
        ),
        Err(e) => panic!("expected Ok(true), got Err({e:?})"),
    }
}

/// Point (5.0 + 2e-3, 0.0, 0.0) is 2e-3 away from the +X face.
/// With tolerance 1e-3, the point is outside tolerance → false.
#[test]
fn point_on_shape_outside_tolerance_returns_false() {
    let (kernel, box_id) = box_kernel();
    let tol = 1e-3_f64;
    let px = 5.0 + 2.0 * tol; // 5.002 — outside tol=1e-3
    match kernel.point_on_shape(box_id, px, 0.0, 0.0, tol) {
        Ok(false) => {}
        Ok(true) => panic!(
            "expected false for point ({px}, 0, 0) with tol={tol} (distance ≈ 2e-3 > tol), got true"
        ),
        Err(e) => panic!("expected Ok(false), got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// Error path — invalid handle
// ---------------------------------------------------------------------------

/// An unknown handle should return `QueryError::InvalidHandle`.
#[test]
fn point_on_shape_unknown_handle_returns_invalid_handle() {
    let (kernel, _box_id) = box_kernel();
    let unknown = GeometryHandleId(999);
    match kernel.point_on_shape(unknown, 0.0, 0.0, 0.0, 1e-7) {
        Err(QueryError::InvalidHandle(id)) if id == unknown => {}
        Err(QueryError::InvalidHandle(id)) => panic!(
            "expected InvalidHandle({:?}), got InvalidHandle({:?})",
            unknown, id
        ),
        other => panic!("expected Err(InvalidHandle({unknown:?})), got {other:?}"),
    }
}
