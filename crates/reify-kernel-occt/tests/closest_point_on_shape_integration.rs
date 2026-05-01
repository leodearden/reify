//! Integration tests for `closest_point_on_shape` — returns the closest point
//! on a `TopoDS_Shape` to a given query point.
//!
//! Fixture: a 10×10×10 box centered at the origin (x∈[-5,5], y∈[-5,5], z∈[-5,5]).
//!
//! Tests:
//! - External point along +X axis → nearest face at x=5.
//! - External point along +Y axis → nearest face at y=5.
//! - Point already on the +X face → distance to returned witness ≤ 1e-6.
//! - Interior point at origin → witness on box surface at distance ≈ 5.0.
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
// Happy path — external points
// ---------------------------------------------------------------------------

/// Query point (10.0, 0.0, 0.0) is outside the box along +X.
/// The closest point on the box surface must be (5.0, 0.0, 0.0).
#[test]
fn closest_point_for_external_point_on_x_axis() {
    let (kernel, box_id) = box_kernel();
    match kernel.closest_point_on_shape(box_id, 10.0, 0.0, 0.0) {
        Ok([x, y, z]) => {
            assert!(
                (x - 5.0).abs() < 1e-6,
                "expected x≈5.0, got {x}"
            );
            assert!(
                y.abs() < 1e-6,
                "expected y≈0.0, got {y}"
            );
            assert!(
                z.abs() < 1e-6,
                "expected z≈0.0, got {z}"
            );
        }
        Err(e) => panic!("expected Ok([5.0, 0.0, 0.0]), got Err({e:?})"),
    }
}

/// Query point (0.0, 7.0, 0.0) is outside the box along +Y.
/// The closest point on the box surface must be (0.0, 5.0, 0.0).
#[test]
fn closest_point_for_external_point_on_y_axis() {
    let (kernel, box_id) = box_kernel();
    match kernel.closest_point_on_shape(box_id, 0.0, 7.0, 0.0) {
        Ok([x, y, z]) => {
            assert!(
                x.abs() < 1e-6,
                "expected x≈0.0, got {x}"
            );
            assert!(
                (y - 5.0).abs() < 1e-6,
                "expected y≈5.0, got {y}"
            );
            assert!(
                z.abs() < 1e-6,
                "expected z≈0.0, got {z}"
            );
        }
        Err(e) => panic!("expected Ok([0.0, 5.0, 0.0]), got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// Happy path — point already on the surface
// ---------------------------------------------------------------------------

/// Query point (5.0, 0.0, 0.0) lies exactly on the +X face.
/// The returned witness point must be within 1e-6 of the query point.
#[test]
fn closest_point_when_point_lies_on_face() {
    let (kernel, box_id) = box_kernel();
    match kernel.closest_point_on_shape(box_id, 5.0, 0.0, 0.0) {
        Ok([x, y, z]) => {
            let dist =
                ((x - 5.0).powi(2) + y.powi(2) + z.powi(2)).sqrt();
            assert!(
                dist < 1e-6,
                "expected witness within 1e-6 of (5.0, 0.0, 0.0), got ({x}, {y}, {z}), dist={dist}"
            );
        }
        Err(e) => panic!("expected Ok near (5.0, 0.0, 0.0), got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// Interior point — degenerate case
// ---------------------------------------------------------------------------

/// Query point (0.0, 0.0, 0.0) lies strictly inside the 10×10×10 box.
///
/// `BRepExtrema_DistShapeShape` reports distance 0 and the query point
/// itself when the point is inside a solid.  The C++ wrapper detects this
/// (dist < 1e-10) and re-runs extrema against the outer shell, returning a
/// proper boundary witness instead.  This test locks in that contract: the
/// call must succeed without error and the returned witness must lie on the
/// box surface at distance ≈ 5.0 from the origin (the nearest face distance
/// for a box centred at origin with half-width 5).
#[test]
fn closest_point_for_interior_point_at_origin() {
    let (kernel, box_id) = box_kernel();
    match kernel.closest_point_on_shape(box_id, 0.0, 0.0, 0.0) {
        Ok([x, y, z]) => {
            // Witness must land on the nearest face (distance 5.0 from origin).
            let dist = (x * x + y * y + z * z).sqrt();
            assert!(
                (dist - 5.0).abs() < 1e-6,
                "expected witness at distance ≈5.0 from origin (nearest box face), \
                 got ({x}, {y}, {z}), dist={dist}"
            );
            // At least one coordinate ≈ ±5.0 confirms the witness is on a face.
            let on_face = (x.abs() - 5.0).abs() < 1e-6
                || (y.abs() - 5.0).abs() < 1e-6
                || (z.abs() - 5.0).abs() < 1e-6;
            assert!(
                on_face,
                "expected one coord ≈ ±5.0 (witness on a box face), got ({x}, {y}, {z})"
            );
        }
        Err(e) => panic!("expected Ok for interior query at origin, got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// Error path — invalid handle
// ---------------------------------------------------------------------------

/// An unknown handle should return `QueryError::InvalidHandle`.
#[test]
fn closest_point_on_shape_unknown_handle_returns_invalid_handle() {
    let (kernel, _box_id) = box_kernel();
    let unknown = GeometryHandleId(999);
    match kernel.closest_point_on_shape(unknown, 0.0, 0.0, 0.0) {
        Err(QueryError::InvalidHandle(id)) if id == unknown => {}
        Err(QueryError::InvalidHandle(id)) => panic!(
            "expected InvalidHandle({:?}), got InvalidHandle({:?})",
            unknown, id
        ),
        other => panic!("expected Err(InvalidHandle({unknown:?})), got {other:?}"),
    }
}
