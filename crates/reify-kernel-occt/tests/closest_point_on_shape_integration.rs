//! Integration tests for `closest_point_on_shape` — returns the closest point
//! on a `TopoDS_Shape` to a given query point.
//!
//! Fixture: a 10×10×10 box centered at the origin (x∈[-5,5], y∈[-5,5], z∈[-5,5]).
//!
//! Tests:
//! - External point along +X axis → nearest face at x=5.
//! - External point along +Y axis → nearest face at y=5.
//! - Point already on the +X face → distance to returned witness ≤ 1e-6.
//! - Off-center interior point at (1,0,0) → OCCT returns query point (1,0,0) at distance 0 (regression sentinel).
//! - Oblique external (10,10,10) → corner witness (5,5,5) at distance 5√3.
//! - Non-solid Face sub-shape input → "any TopoDS_Shape" contract holds (Ok with witness on face, distance ≈ 5.0).
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

/// Query point (10.0, 10.0, 10.0) lies along the body-diagonal of the +X+Y+Z
/// octant outside the centred 10×10×10 box. The unique closest point on the
/// box is the corner vertex (5.0, 5.0, 5.0) at distance 5·√3 ≈ 8.6602540378
/// — a corner-witness branch of `BRepExtrema_DistShapeShape` that the
/// axis-aligned external-point tests do not cover.
#[test]
fn closest_point_for_oblique_external_point_resolves_to_corner_witness() {
    let (kernel, box_id) = box_kernel();
    match kernel.closest_point_on_shape(box_id, 10.0, 10.0, 10.0) {
        Ok([x, y, z]) => {
            assert!(
                (x - 5.0).abs() < 1e-6,
                "expected x≈5.0 (corner witness), got {x}"
            );
            assert!(
                (y - 5.0).abs() < 1e-6,
                "expected y≈5.0 (corner witness), got {y}"
            );
            assert!(
                (z - 5.0).abs() < 1e-6,
                "expected z≈5.0 (corner witness), got {z}"
            );
            let d = ((x - 10.0).powi(2) + (y - 10.0).powi(2) + (z - 10.0).powi(2)).sqrt();
            assert!(
                (d - (75.0_f64).sqrt()).abs() < 1e-6,
                "expected distance 5√3≈{}, got {d}",
                (75.0_f64).sqrt()
            );
        }
        Err(e) => panic!("expected Ok([5.0, 5.0, 5.0]), got Err({e:?})"),
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
// Non-solid TopoDS_Shape input
// ---------------------------------------------------------------------------

/// The C++ wrapper accepts "any TopoDS_Shape" per its docstring — not just
/// Solid boxes. This test verifies that a `TopoDS_Face` sub-shape extracted
/// from the box returns a valid witness, exercising the non-solid path.
///
/// Query (0.0, 0.0, 0.0) is chosen because every face of a centred 10×10×10
/// box has its centroid exactly 5 units from the origin. The nearest point
/// on any face from the origin is at distance 5.0, independent of which face
/// `MapShapes` returns as `faces[0]`. This keeps the test deterministic
/// without face-identification logic.
#[test]
fn closest_point_on_face_subshape_satisfies_any_shape_contract() {
    let (mut kernel, box_id) = box_kernel(); // mut required for extract_faces
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces should succeed for a valid box");
    assert!(!faces.is_empty(), "box should have at least one face");

    match kernel.closest_point_on_shape(faces[0], 0.0, 0.0, 0.0) {
        Ok([x, y, z]) => {
            let dist = (x * x + y * y + z * z).sqrt();
            assert!(
                (dist - 5.0).abs() < 1e-6,
                "any face of a centred 10×10×10 box has its centroid 5 units from origin, \
                 so the closest point on a face from origin is at distance 5.0; got ({x}, {y}, {z}), dist={dist}"
            );
        }
        Err(e) => panic!(
            "closest_point_on_shape on a Face sub-shape should satisfy the \
             'any TopoDS_Shape' contract, got Err({e:?})"
        ),
    }
}

// ---------------------------------------------------------------------------
// Interior point — degenerate case
// ---------------------------------------------------------------------------

/// Query point (1.0, 0.0, 0.0) lies strictly inside the 10×10×10 box.
///
/// When the query vertex is inside the solid, `BRepExtrema_DistShapeShape`
/// reports distance 0 (the shapes overlap) and places the witness
/// `PointOnShape1` at the query location itself. This is OCCT's observed
/// behaviour: the interior of the solid is part of shape1, so the nearest
/// point on shape1 to the query vertex is the vertex itself. Regression
/// sentinel — pin the exact returned coordinates within 1e-6 so a future
/// OCCT/cxx upgrade that changes this behaviour is caught.
///
/// Observed against the OCCT version in use at task 2849.
#[test]
fn closest_point_for_offcenter_interior_point() {
    let (kernel, box_id) = box_kernel();
    // For an interior query, OCCT returns the query point itself (distance=0).
    match kernel.closest_point_on_shape(box_id, 1.0, 0.0, 0.0) {
        Ok([x, y, z]) => {
            assert!(
                (x - 1.0).abs() < 1e-6,
                "expected x≈1.0 (query point returned for interior query), got {x}"
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
        Err(e) => panic!("expected Ok([1.0, 0.0, 0.0]) for off-centre interior query at (1,0,0), got Err({e:?})"),
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
