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
//! - Zero tolerance with exact face point (5,0,0) → true (dist=0 exactly).
//! - Off-face coplanar point (5,100,100): on plane x=5 but outside face extent → false.
//! - Non-solid (wire) fixture: point 1 unit off the wire returns false (no OCCT overlap).
//! - Negative tolerance → `Err(QueryError::QueryFailed(_))` (regression sentinel for tolerance precondition throw).
//! - NaN tolerance → `Err(QueryError::QueryFailed(_))` (regression sentinel for tolerance precondition throw).
//! - Infinite tolerance → `Err(QueryError::QueryFailed(_))` (regression sentinel for tolerance precondition throw).

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_ir::{DEFAULT_POINT_ON_SHAPE_TOLERANCE_M, GeometryHandleId, GeometryOp, GeometryQuery, QueryError, Value};

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
/// With tolerance `DEFAULT_POINT_ON_SHAPE_TOLERANCE_M` (≈ Precision::Confusion), the result should be true.
#[test]
fn point_on_shape_face_center_returns_true() {
    let (kernel, box_id) = box_kernel();
    match kernel.point_on_shape(box_id, 5.0, 0.0, 0.0, DEFAULT_POINT_ON_SHAPE_TOLERANCE_M) {
        Ok(true) => {}
        Ok(false) => panic!("expected true for face-center point (5,0,0), got false"),
        Err(e) => panic!("expected Ok(true), got Err({e:?})"),
    }
}

/// Query point (5.0, 5.0, 0.0) lies exactly on the +X/+Y edge midpoint.
/// With tolerance `DEFAULT_POINT_ON_SHAPE_TOLERANCE_M`, the result should be true.
#[test]
fn point_on_shape_edge_midpoint_returns_true() {
    let (kernel, box_id) = box_kernel();
    match kernel.point_on_shape(box_id, 5.0, 5.0, 0.0, DEFAULT_POINT_ON_SHAPE_TOLERANCE_M) {
        Ok(true) => {}
        Ok(false) => panic!("expected true for edge-midpoint (5,5,0), got false"),
        Err(e) => panic!("expected Ok(true), got Err({e:?})"),
    }
}

/// Query point (5.0, 5.0, 5.0) lies exactly on the +X/+Y/+Z corner vertex.
/// With tolerance `DEFAULT_POINT_ON_SHAPE_TOLERANCE_M`, the result should be true.
#[test]
fn point_on_shape_corner_returns_true() {
    let (kernel, box_id) = box_kernel();
    match kernel.point_on_shape(box_id, 5.0, 5.0, 5.0, DEFAULT_POINT_ON_SHAPE_TOLERANCE_M) {
        Ok(true) => {}
        Ok(false) => panic!("expected true for corner vertex (5,5,5), got false"),
        Err(e) => panic!("expected Ok(true), got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// External point — outside the box
// ---------------------------------------------------------------------------

/// Query point (10.0, 0.0, 0.0) is 5 units outside the +X face.
/// With tolerance `DEFAULT_POINT_ON_SHAPE_TOLERANCE_M`, the result should be false.
#[test]
fn point_on_shape_external_point_returns_false() {
    let (kernel, box_id) = box_kernel();
    match kernel.point_on_shape(box_id, 10.0, 0.0, 0.0, DEFAULT_POINT_ON_SHAPE_TOLERANCE_M) {
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
    match kernel.point_on_shape(unknown, 0.0, 0.0, 0.0, DEFAULT_POINT_ON_SHAPE_TOLERANCE_M) {
        Err(QueryError::InvalidHandle(id)) if id == unknown => {}
        Err(QueryError::InvalidHandle(id)) => panic!(
            "expected InvalidHandle({:?}), got InvalidHandle({:?})",
            unknown, id
        ),
        other => panic!("expected Err(InvalidHandle({unknown:?})), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Zero-tolerance test — exact boundary coincidence
// ---------------------------------------------------------------------------

/// Point (5.0, 0.0, 0.0) lies exactly on the +X face center.
///
/// With `tolerance = 0.0`, the test exercises the `dist.Value() <= 0.0` boundary.
/// For a parameterically-exact BREP boundary point, `BRepExtrema_DistShapeShape`
/// reports `dist.Value() = 0.0` exactly, so the result should be `true`.
/// This verifies that callers do not need to pass a positive tolerance just
/// to handle the "exactly on surface" case.
#[test]
fn point_on_shape_zero_tolerance_exact_face_point_returns_true() {
    let (kernel, box_id) = box_kernel();
    match kernel.point_on_shape(box_id, 5.0, 0.0, 0.0, 0.0) {
        Ok(true) => {}
        Ok(false) => {
            panic!("expected true for exact face-center point (5,0,0) with tol=0.0, got false")
        }
        Err(e) => panic!("expected Ok(true), got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// Off-face coplanar point — face extent vs. plane equation
// ---------------------------------------------------------------------------

/// Point (5.0, 100.0, 100.0) is coplanar with the +X face (plane x=5.0)
/// but far outside the face's spatial extent (y∈[-5,5], z∈[-5,5]).
///
/// The nearest point on the box is the corner (5, 5, 5), at distance
/// `sqrt((100−5)² + (100−5)²) ≈ 134.4` units.  With tight tolerance
/// (`DEFAULT_POINT_ON_SHAPE_TOLERANCE_M`), the result must be `false`.
///
/// This locks in that `BRepExtrema_DistShapeShape` queries the actual BREP
/// face geometry — not just the infinite plane containing the face — so a
/// coplanar-but-off-face point is correctly rejected.
#[test]
fn point_on_shape_off_face_coplanar_point_returns_false() {
    let (kernel, box_id) = box_kernel();
    match kernel.point_on_shape(
        box_id,
        5.0,
        100.0,
        100.0,
        DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
    ) {
        Ok(false) => {}
        Ok(true) => panic!(
            "expected false for coplanar-but-off-face point (5,100,100) with tol=DEFAULT_POINT_ON_SHAPE_TOLERANCE_M, got true"
        ),
        Err(e) => panic!("expected Ok(false), got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// Non-solid shape — no OCCT solid-overlap behavior
// ---------------------------------------------------------------------------

/// Build a kernel with a single line-segment wire from (-5,0,0) to (5,0,0).
///
/// A `TopoDS_Wire` is NOT a solid. `BRepExtrema_DistShapeShape` does not
/// apply the solid-overlap shortcut, so it returns the actual geometric distance.
fn wire_kernel() -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let handle = kernel
        .execute(&GeometryOp::LineSegment {
            x1: -5.0,
            y1: 0.0,
            z1: 0.0,
            x2: 5.0,
            y2: 0.0,
            z2: 0.0,
        })
        .expect("line segment creation should succeed");
    (kernel, handle.id)
}

// ---------------------------------------------------------------------------
// Tolerance precondition — early-validation throw site regression sentinels
// ---------------------------------------------------------------------------

/// Regression sentinel for the early-validation throw in
/// `point_on_shape`'s tolerance precondition.
///
/// The C++ wrapper validates the tolerance argument before calling
/// `BRepExtrema_DistShapeShape`:
///
/// ```cpp
/// if (tolerance < 0.0 || !std::isfinite(tolerance))
///     throw std::runtime_error(
///         "point_on_shape: tolerance must be a non-negative finite value");
/// ```
///
/// The contract is documented at all five layers: `cpp/occt_wrapper.cpp`,
/// `cpp/occt_wrapper.h`, `crates/reify-kernel-occt/src/ffi.rs`,
/// `crates/reify-kernel-occt/src/lib.rs`, and
/// `crates/reify-types/src/stubs.rs`.  This test locks the throw site:
/// if it starts returning `Ok(_)`, the early check was silently removed.
#[test]
fn point_on_shape_negative_tolerance_returns_error() {
    let (kernel, box_id) = box_kernel();
    let result = kernel.point_on_shape(box_id, 5.0, 0.0, 0.0, -1.0);
    match result {
        Err(QueryError::QueryFailed(_)) => {}
        Ok(v) => {
            panic!("expected Err(QueryError::QueryFailed(_)) for tolerance=-1.0, got Ok({v:?})")
        }
        Err(e) => {
            panic!("expected Err(QueryError::QueryFailed(_)) for tolerance=-1.0, got Err({e:?})")
        }
    }
}

/// Regression sentinel for the early-validation throw in
/// `point_on_shape`'s tolerance precondition, NaN branch.
///
/// `!std::isfinite(NaN) == true` triggers the same throw site as a negative
/// tolerance.  This test locks the IEEE-754 NaN-comparison footgun
/// (`dist <= NaN` is always `false` in C++) at the boundary: rather than
/// silently producing `false` for every call when `tolerance` is NaN, the
/// wrapper rejects it with a clear error.
///
/// The contract is documented at all five layers: `cpp/occt_wrapper.cpp`,
/// `cpp/occt_wrapper.h`, `crates/reify-kernel-occt/src/ffi.rs`,
/// `crates/reify-kernel-occt/src/lib.rs`, and
/// `crates/reify-types/src/stubs.rs`.
#[test]
fn point_on_shape_nan_tolerance_returns_error() {
    let (kernel, box_id) = box_kernel();
    let result = kernel.point_on_shape(box_id, 5.0, 0.0, 0.0, f64::NAN);
    match result {
        Err(QueryError::QueryFailed(_)) => {}
        Ok(v) => {
            panic!("expected Err(QueryError::QueryFailed(_)) for tolerance=NaN, got Ok({v:?})")
        }
        Err(e) => {
            panic!("expected Err(QueryError::QueryFailed(_)) for tolerance=NaN, got Err({e:?})")
        }
    }
}

/// Regression sentinel for the early-validation throw in
/// `point_on_shape`'s tolerance precondition, ±∞ branch.
///
/// The C++ wrapper checks `!std::isfinite(tolerance)`, which is `true` for
/// both `+∞` and `-∞`.  Without the `isfinite` guard a refactor to
/// `tol >= 0.0` alone would silently pass `+∞` (since `+∞ >= 0.0` is `true`
/// in IEEE-754).  This test locks the ∞-rejection path complementary to
/// the negative-finite and NaN sentinels.
///
/// The contract is documented at all five layers: `cpp/occt_wrapper.cpp`,
/// `cpp/occt_wrapper.h`, `crates/reify-kernel-occt/src/ffi.rs`,
/// `crates/reify-kernel-occt/src/lib.rs`, and
/// `crates/reify-types/src/stubs.rs`.
#[test]
fn point_on_shape_infinite_tolerance_returns_error() {
    let (kernel, box_id) = box_kernel();
    let result = kernel.point_on_shape(box_id, 5.0, 0.0, 0.0, f64::INFINITY);
    match result {
        Err(QueryError::QueryFailed(_)) => {}
        Ok(v) => panic!("expected Err(QueryError::QueryFailed(_)) for tolerance=+∞, got Ok({v:?})"),
        Err(e) => {
            panic!("expected Err(QueryError::QueryFailed(_)) for tolerance=+∞, got Err({e:?})")
        }
    }
}

/// Query point (0.0, 1.0, 0.0) is 1.0 unit away from the wire (which lies on
/// the X axis from (−5,0,0) to (5,0,0)).
///
/// For a non-solid shape, `BRepExtrema_DistShapeShape` returns the actual
/// geometric distance — there is no OCCT solid-overlap shortcut.  With
/// tolerance `DEFAULT_POINT_ON_SHAPE_TOLERANCE_M`, the result must be `false`
/// (1.0 >> 1e-7).
///
/// Contrasts with `point_on_shape_interior_solid_point_returns_true` which
/// shows that for a `TopoDS_Solid`, interior points return `dist=0` regardless
/// of geometric distance to the boundary.
#[test]
fn point_on_shape_non_solid_off_wire_returns_false() {
    let (kernel, wire_id) = wire_kernel();
    // (0, 1, 0): distance to the X-axis wire is 1.0 unit, far above DEFAULT_POINT_ON_SHAPE_TOLERANCE_M.
    match kernel.point_on_shape(wire_id, 0.0, 1.0, 0.0, DEFAULT_POINT_ON_SHAPE_TOLERANCE_M) {
        Ok(false) => {}
        Ok(true) => panic!(
            "expected false for point (0,1,0) 1.0 unit from wire with tol=DEFAULT_POINT_ON_SHAPE_TOLERANCE_M, got true"
        ),
        Err(e) => panic!("expected Ok(false), got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// query() round-trip — task 2324 stdlib wiring
// ---------------------------------------------------------------------------

/// Round-trip `GeometryQuery::PointOnShape` via the generic `kernel.query(...)`
/// dispatch and confirm the kernel emits a `Value::Bool`.
///
/// Face-center query (5,0,0) on the +X face → true.
#[test]
fn query_point_on_shape_returns_true_for_face_center_point() {
    let (kernel, box_id) = box_kernel();
    let value = kernel
        .query(&GeometryQuery::PointOnShape {
            handle: box_id,
            px: 5.0,
            py: 0.0,
            pz: 0.0,
            tolerance: DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
        })
        .expect("query(PointOnShape) should succeed for valid box handle");
    assert_eq!(
        value,
        Value::Bool(true),
        "expected Value::Bool(true) for on-face point (5,0,0)"
    );
}

/// External point (10, 0, 0) is well outside the +X face → false.
#[test]
fn query_point_on_shape_returns_false_for_external_point() {
    let (kernel, box_id) = box_kernel();
    let value = kernel
        .query(&GeometryQuery::PointOnShape {
            handle: box_id,
            px: 10.0,
            py: 0.0,
            pz: 0.0,
            tolerance: DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
        })
        .expect("query(PointOnShape) should succeed for valid box handle");
    assert_eq!(
        value,
        Value::Bool(false),
        "expected Value::Bool(false) for external point (10,0,0)"
    );
}
