//! Integration tests for `closest_point_on_shape` — returns the closest point
//! on a `TopoDS_Shape` to a given query point.
//!
//! Fixture: a 10×10×10 box centered at the origin (x∈[-5,5], y∈[-5,5], z∈[-5,5]).
//!
//! Tests:
//! - External point along +X axis → nearest face at x=5.
//! - External point along +Y axis → nearest face at y=5.
//! - Point already on the +X face → distance to returned witness ≤ 1e-6.
//! - Off-center interior point at (1,0,0) → OCCT returns nearest face point (5,0,0) at distance 4.0 (regression sentinel).
//! - Oblique external (10,10,10) → corner witness (5,5,5) at distance 5√3.
//! - Non-solid Face sub-shape input → "any TopoDS_Shape" contract holds (Ok with witness on face, distance ≈ 5.0).
//! - NaN query coords → `Err(QueryError::QueryFailed(_))` (regression sentinel, OCCT rejects NaN vertex).
//! - Unknown handle → `QueryError::InvalidHandle`.
//! - Interior origin (0,0,0) → shell-fallback witness on box face at distance ≈5.0 (regression sentinel for `dist < 1e-10` path).

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery, QueryError, Value};

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
            assert!((x - 5.0).abs() < 1e-6, "expected x≈5.0, got {x}");
            assert!(y.abs() < 1e-6, "expected y≈0.0, got {y}");
            assert!(z.abs() < 1e-6, "expected z≈0.0, got {z}");
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
            assert!(x.abs() < 1e-6, "expected x≈0.0, got {x}");
            assert!((y - 5.0).abs() < 1e-6, "expected y≈5.0, got {y}");
            assert!(z.abs() < 1e-6, "expected z≈0.0, got {z}");
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
///
/// When the query is on the surface, `BRepExtrema_DistShapeShape` reports
/// distance ≈ 0, which triggers the C++ wrapper's `dist < 1e-10`
/// shell-fallback: the wrapper re-runs extrema against the first shell via
/// `TopExp_Explorer` (`TopAbs_SHELL`) and returns the shell result.  This
/// query happens to enter the shell-fallback branch; the test verifies the
/// externally-visible contract regardless of which branch ran.
#[test]
fn closest_point_when_point_lies_on_face() {
    let (kernel, box_id) = box_kernel();
    match kernel.closest_point_on_shape(box_id, 5.0, 0.0, 0.0) {
        Ok([x, y, z]) => {
            let dist = ((x - 5.0).powi(2) + y.powi(2) + z.powi(2)).sqrt();
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
/// Query (0.0, 0.0, 0.0) is chosen because each face plane of a centred
/// 10×10×10 box is at distance 5 from the origin, and the perpendicular foot
/// of the origin onto each face plane lies within the face's [-5,5]² bounds —
/// so the closest point on any face from the origin is at distance 5.0,
/// independent of which face `MapShapes` returns as `faces[0]`. This keeps
/// the test deterministic without face-identification logic.
#[test]
fn closest_point_on_face_subshape_satisfies_any_shape_contract() {
    let (mut kernel, box_id) = box_kernel(); // mut required for extract_faces
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces should succeed for a valid box");
    assert!(!faces.is_empty(), "box should have at least one face");

    for &fid in &faces {
        match kernel.closest_point_on_shape(fid, 0.0, 0.0, 0.0) {
            Ok([x, y, z]) => {
                let dist = (x * x + y * y + z * z).sqrt();
                assert!(
                    (dist - 5.0).abs() < 1e-6,
                    "each face plane of a centred 10×10×10 box is at distance 5 from the origin \
                     and the perpendicular foot lies within the face bounds, so the closest point \
                     on any face from the origin is at distance 5.0; got ({x}, {y}, {z}), dist={dist}"
                );
            }
            Err(e) => panic!(
                "closest_point_on_shape on a Face sub-shape should satisfy the \
                 'any TopoDS_Shape' contract, got Err({e:?})"
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Interior point — degenerate case
// ---------------------------------------------------------------------------

/// Query point (1.0, 0.0, 0.0) lies strictly inside the 10×10×10 box.
///
/// `BRepExtrema_DistShapeShape` has no inside/outside knowledge — it returns
/// the distance to the nearest BREP boundary face.  For this query the
/// reported distance is 4.0 (X-gap between (1,0,0) and the +X face at x=5),
/// so the primary `PointOnShape1(1)` path in the C++ wrapper returns
/// (5.0, 0.0, 0.0) directly.  The C++ wrapper's defensive `dist < 1e-10`
/// shell-fallback is *not* entered for this query — it fires only for
/// on-surface or coincident queries where `BRepExtrema` reports distance ≈ 0;
/// see `closest_point_when_point_lies_on_face` for a test that exercises that
/// branch.  Regression sentinel — pin the exact returned coordinates within
/// 1e-6 so a future OCCT/cxx upgrade that changes this behaviour is caught.
///
/// Observed against the OCCT version in use at task 2849.
#[test]
fn closest_point_for_offcenter_interior_point() {
    let (kernel, box_id) = box_kernel();
    // BRepExtrema returns distance 4.0 for this interior query and the
    // perpendicular foot on the +X face directly; the shell-search detour is
    // not entered (it fires only when dist < 1e-10).
    match kernel.closest_point_on_shape(box_id, 1.0, 0.0, 0.0) {
        Ok([x, y, z]) => {
            assert!(
                (x - 5.0).abs() < 1e-6,
                "expected x≈5.0 (nearest face surface for interior query at (1,0,0)), got {x}"
            );
            assert!(y.abs() < 1e-6, "expected y≈0.0, got {y}");
            assert!(z.abs() < 1e-6, "expected z≈0.0, got {z}");
        }
        Err(e) => panic!(
            "expected Ok([5.0, 0.0, 0.0]) for off-centre interior query at (1,0,0), got Err({e:?})"
        ),
    }
}

/// Query point (0.0, 0.0, 0.0) lies at the centre of the 10×10×10 box.
///
/// For a `TopoDS_Solid`, `BRepExtrema_DistShapeShape` considers the query
/// vertex to overlap the solid and returns `dist.Value() = 0`.  The C++
/// wrapper's `dist < 1e-10` shell-fallback in `closest_point_on_shape`
/// (introduced by task 2780) then fires: it re-runs `BRepExtrema_DistShapeShape`
/// against the first shell (`TopExp_Explorer(shape, TopAbs_SHELL)`) and
/// returns a boundary witness from the shell.
///
/// This test verifies the shell-fallback contract:
/// - The returned witness is at distance ≈ 5.0 from the origin (each face
///   plane of a centred 10×10×10 box is 5 units away).
/// - At least one coordinate of the witness is ≈ ±5, confirming the witness
///   lies on a face plane rather than being an arbitrary interior point.
///
/// Complementary to `closest_point_for_offcenter_interior_point` which
/// queries `(1,0,0)` and exercises the *primary* BRepExtrema path where the
/// shell-fallback is NOT entered (dist=4.0 > 1e-10).
///
/// **If this test fails, that is a real bug in the shell-recompute path —
/// do NOT weaken the assertion; escalate instead.**
#[test]
fn closest_point_for_interior_origin_returns_face_witness_after_shell_recompute() {
    let (kernel, box_id) = box_kernel();
    match kernel.closest_point_on_shape(box_id, 0.0, 0.0, 0.0) {
        Ok([x, y, z]) => {
            let dist = (x * x + y * y + z * z).sqrt();
            assert!(
                (dist - 5.0).abs() < 1e-6,
                "expected shell-fallback witness at distance ≈5.0 from origin, \
                 got ({x}, {y}, {z}), dist={dist}"
            );
            let on_face = (x.abs() - 5.0).abs() < 1e-6
                || (y.abs() - 5.0).abs() < 1e-6
                || (z.abs() - 5.0).abs() < 1e-6;
            assert!(
                on_face,
                "expected witness on a box face after shell-recompute \
                 (at least one coord ≈ ±5), got ({x}, {y}, {z})"
            );
        }
        Err(e) => panic!(
            "expected Ok([...]) for interior origin query after shell-fallback, got Err({e:?})"
        ),
    }
}

// ---------------------------------------------------------------------------
// Degenerate inputs — regression sentinels
// ---------------------------------------------------------------------------

/// Regression sentinel for `BRepBuilderAPI_MakeVertex(gp_Pnt(NAN, 0, 0))`
/// behaviour in `closest_point_on_shape`'s NaN-vertex handling.
///
/// OCCT does not validate vertex constructor inputs: a NaN coordinate may
/// succeed at vertex construction but produce `BRepExtrema_DistShapeShape`
/// failure (`IsDone() == false` or `NbSolution() < 1`), yielding
/// `Err(QueryError::QueryFailed(_))` in the Rust wrapper.  The exact observed
/// outcome is pinned below so a future OCCT/cxx upgrade that flips this
/// behaviour is caught.
///
/// Observed against the OCCT version in use at task 2849.
#[test]
fn closest_point_for_nan_query_coords_locks_current_behavior() {
    let (kernel, box_id) = box_kernel();
    // Behaviour observed against OCCT at task 2849; flip this assertion when
    // the upgrade ticket lands.
    let result = kernel.closest_point_on_shape(box_id, f64::NAN, 0.0, 0.0);
    match result {
        Err(QueryError::QueryFailed(_)) => {
            // Expected: OCCT rejects the NaN vertex — lock in this branch.
        }
        Ok([x, y, z]) => panic!(
            "expected Err(QueryError::QueryFailed(_)) for NaN query coords, \
             got Ok([{x}, {y}, {z}]) — if OCCT changed behaviour, pin the Ok branch instead"
        ),
        Err(e) => {
            panic!("expected Err(QueryError::QueryFailed(_)) for NaN query coords, got Err({e:?})")
        }
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

// ---------------------------------------------------------------------------
// query() round-trip — task 2324 stdlib wiring
// ---------------------------------------------------------------------------

/// Round-trip the new `GeometryQuery::ClosestPointOnShape` variant via the
/// generic `kernel.query(...)` dispatch. The wire format is the same JSON-Point3
/// `{"x":_,"y":_,"z":_}` that `Centroid` / `FaceNormal` / `EdgeTangent` emit,
/// so the eval-side dispatcher can reuse `parse_xyz_value`.
///
/// External point (10, 0, 0) → closest point on +X face ≈ (5, 0, 0).
#[test]
fn query_closest_point_on_shape_returns_xyz_json_for_external_point() {
    let (kernel, box_id) = box_kernel();
    let value = kernel
        .query(&GeometryQuery::ClosestPointOnShape {
            handle: box_id,
            px: 10.0,
            py: 0.0,
            pz: 0.0,
        })
        .expect("query(ClosestPointOnShape) should succeed for valid box handle");

    let json = match value {
        Value::String(s) => s,
        other => panic!(
            "query(ClosestPointOnShape) must return Value::String for cross-arm \
             format equality with Centroid/FaceNormal/EdgeTangent, got {other:?}"
        ),
    };

    // Manually parse the {"x":_,"y":_,"z":_} payload — keep the test
    // independent of the eval-side parse_xyz_value helper.
    let parse = |key: &str| -> f64 {
        let needle = format!("\"{key}\":");
        let start = json
            .find(&needle)
            .unwrap_or_else(|| panic!("expected key {key:?} in {json:?}"))
            + needle.len();
        let tail = &json[start..];
        let end = tail.find([',', '}']).unwrap_or(tail.len());
        tail[..end]
            .trim()
            .parse::<f64>()
            .unwrap_or_else(|e| panic!("parse {key} from {json:?}: {e}"))
    };
    let (x, y, z) = (parse("x"), parse("y"), parse("z"));

    assert!(
        (x - 5.0).abs() < 1e-6,
        "expected x≈5.0 (closest +X-face hit), got {x}"
    );
    assert!(y.abs() < 1e-6, "expected y≈0.0, got {y}");
    assert!(z.abs() < 1e-6, "expected z≈0.0, got {z}");
}
