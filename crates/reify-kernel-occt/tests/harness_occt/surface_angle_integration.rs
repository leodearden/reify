//! Integration tests for `OcctKernel::surface_angle` — returns the angle
//! (in radians) between two `TopoDS_Face` outward normals via face-normal dot.
//!
//! Fixture: a 10×10×10 box centered at the origin (x∈[-5,5], y∈[-5,5], z∈[-5,5]),
//! with 6 faces extracted via `extract_faces`.
//!
//! Tests:
//! - Adjacent box-face pairs → angle ≈ π/2.
//! - Opposite box-face pairs → angle ≈ π.
//! - Same face handle passed twice → angle ≈ 0 (degenerate-coplanar case,
//!   exercises the dot-clamp path because `n·n` may equal 1.0 + tiny FP error).
//! - Unknown handle → `QueryError::InvalidHandle`.
//! - Non-face solid handle → `QueryError::QueryFailed` with "not a face".

#![cfg(has_occt)]

use std::collections::HashSet;
use std::f64::consts::PI;

use reify_kernel_occt::OcctKernel;
use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery, QueryError, Value};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Build a kernel with a single 10×10×10 box centered at the origin.
/// Returns `(kernel, box_id)` where `kernel` is `mut` (required for
/// `extract_faces`).
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

/// Query `AdjacentFaces` for `face_index` and return the neighbor indices
/// as a `HashSet<usize>`.
fn neighbors_of(kernel: &OcctKernel, shape: GeometryHandleId, face_index: usize) -> HashSet<usize> {
    let result = kernel
        .query(&GeometryQuery::AdjacentFaces { shape, face_index })
        .unwrap_or_else(|e| panic!("AdjacentFaces({face_index}) returned Err: {e:?}"));
    match result {
        Value::List(items) => items
            .into_iter()
            .map(|v| match v {
                Value::Int(i) => i as usize,
                other => panic!("expected Value::Int neighbor, got {other:?}"),
            })
            .collect(),
        other => panic!("expected Value::List from AdjacentFaces, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Happy path — π/2 (adjacent faces)
// ---------------------------------------------------------------------------

/// Every pair of adjacent box faces is perpendicular: surface_angle ≈ π/2.
///
/// Derives adjacency via `GeometryQuery::AdjacentFaces` so the test is
/// robust against changes in TopExp face-ordering.
#[test]
fn box_adjacent_faces_yield_pi_over_two() {
    let (mut kernel, box_id) = box_kernel();
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces should succeed");
    assert_eq!(faces.len(), 6, "expected 6 faces for a box");

    for i in 0..6 {
        let neighbors = neighbors_of(&kernel, box_id, i);
        assert_eq!(
            neighbors.len(),
            4,
            "box face {i} should have exactly 4 adjacent faces"
        );
        for &j in &neighbors {
            let angle = kernel
                .surface_angle(faces[i], faces[j])
                .unwrap_or_else(|e| {
                    panic!("surface_angle(face[{i}], face[{j}]) returned Err: {e:?}")
                });
            assert!(
                (angle - PI / 2.0).abs() < 1e-9,
                "adjacent box faces ({i}, {j}): expected π/2 ≈ {:.10}, got {angle:.10}",
                PI / 2.0
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Happy path — π (opposite faces)
// ---------------------------------------------------------------------------

/// Each pair of opposite box faces (parallel, anti-normal): surface_angle ≈ π.
///
/// The opposite face is the unique index in 0..6 that is neither the face
/// itself nor in its adjacency set — mirroring the pattern in
/// `topology_selectors_integration::box_opposite_faces_share_no_edges`.
#[test]
fn box_opposite_faces_yield_pi() {
    let (mut kernel, box_id) = box_kernel();
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces should succeed");
    assert_eq!(faces.len(), 6, "expected 6 faces for a box");

    let neighbors: Vec<HashSet<usize>> = (0..6).map(|i| neighbors_of(&kernel, box_id, i)).collect();

    for i in 0..6usize {
        let opposite_candidates: Vec<usize> = (0..6)
            .filter(|&j| j != i && !neighbors[i].contains(&j))
            .collect();
        assert_eq!(
            opposite_candidates.len(),
            1,
            "expected exactly 1 opposite face for face {i}, got {opposite_candidates:?}"
        );
        let j = opposite_candidates[0];

        let angle = kernel
            .surface_angle(faces[i], faces[j])
            .unwrap_or_else(|e| panic!("surface_angle(face[{i}], face[{j}]) returned Err: {e:?}"));
        assert!(
            (angle - PI).abs() < 1e-9,
            "opposite box faces ({i}, {j}): expected π ≈ {PI:.10}, got {angle:.10}"
        );
    }
}

// ---------------------------------------------------------------------------
// Happy path — 0 (degenerate-coplanar: same face handle)
// ---------------------------------------------------------------------------

/// Passing the same face handle for both arguments yields angle ≈ 0.
///
/// For a unit normal `n`, `dot(n, n) = 1.0` exactly under IEEE 754 for
/// exact unit vectors, or `1.0 + ε` for FP-rounded ones.  The dot-clamp
/// path inside the C++ implementation guards against NaN from `acos(1+ε)`.
/// This test exercises that guard.
#[test]
fn same_face_yields_zero() {
    let (mut kernel, box_id) = box_kernel();
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces should succeed");
    assert!(!faces.is_empty(), "box should have at least one face");

    // Use the first face; the choice is arbitrary — all box faces are planar
    // and yield a well-defined unit normal.
    let face = faces[0];
    let angle = kernel
        .surface_angle(face, face)
        .expect("surface_angle(f, f) should return Ok(0)");
    assert!(
        angle.abs() < 1e-9,
        "same face: expected angle ≈ 0, got {angle:.10}"
    );
}

// ---------------------------------------------------------------------------
// Error path — invalid handle
// ---------------------------------------------------------------------------

/// An unknown handle for `face_a` yields `QueryError::InvalidHandle` carrying
/// the unknown id.
#[test]
fn unknown_face_a_handle_returns_invalid_handle() {
    let (mut kernel, box_id) = box_kernel();
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces should succeed");
    let valid = faces[0];
    let unknown = GeometryHandleId(9999);

    match kernel.surface_angle(unknown, valid) {
        Err(QueryError::InvalidHandle(id)) if id == unknown => {}
        Err(QueryError::InvalidHandle(id)) => {
            panic!("expected InvalidHandle({unknown:?}), got InvalidHandle({id:?})")
        }
        other => panic!("expected Err(InvalidHandle({unknown:?})), got {other:?}"),
    }
}

/// An unknown handle for `face_b` yields `QueryError::InvalidHandle` carrying
/// the unknown id.
#[test]
fn unknown_face_b_handle_returns_invalid_handle() {
    let (mut kernel, box_id) = box_kernel();
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces should succeed");
    let valid = faces[0];
    let unknown = GeometryHandleId(9999);

    match kernel.surface_angle(valid, unknown) {
        Err(QueryError::InvalidHandle(id)) if id == unknown => {}
        Err(QueryError::InvalidHandle(id)) => {
            panic!("expected InvalidHandle({unknown:?}), got InvalidHandle({id:?})")
        }
        other => panic!("expected Err(InvalidHandle({unknown:?})), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Error path — non-face shape (solid handle)
// ---------------------------------------------------------------------------

/// Passing the box's solid handle directly (not an extracted face) for `face_a`
/// yields `QueryError::QueryFailed` with a message containing "not a face".
#[test]
fn non_face_shape_for_face_a_returns_query_failed() {
    let (mut kernel, box_id) = box_kernel();
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces should succeed");
    let valid_face = faces[0];

    match kernel.surface_angle(box_id, valid_face) {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("not a face"),
                "expected error message to contain 'not a face', got: {msg}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed(\"...not a face...\")) when passing a solid handle for face_a, \
             got {other:?}"
        ),
    }
}

/// Passing the box's solid handle directly for `face_b` yields
/// `QueryError::QueryFailed` with a message containing "not a face".
#[test]
fn non_face_shape_for_face_b_returns_query_failed() {
    let (mut kernel, box_id) = box_kernel();
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces should succeed");
    let valid_face = faces[0];

    match kernel.surface_angle(valid_face, box_id) {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("not a face"),
                "expected error message to contain 'not a face', got: {msg}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed(\"...not a face...\")) when passing a solid handle for face_b, \
             got {other:?}"
        ),
    }
}

// ---------------------------------------------------------------------------
// Coverage — curved face (cylinder)
// ---------------------------------------------------------------------------

/// `surface_angle` returns a finite value in `[0, π]` for every face pair of a
/// cylinder, including the curved side face.
///
/// A cylinder has 3 faces: bottom cap, top cap, and the curved lateral surface.
/// This test exercises the centroid-sampling path on the curved face and
/// confirms no errors are raised. It additionally verifies that the two end
/// caps (flat, with normals ≈ ±Z) yield angle ≈ π, and that calling the
/// function with the same curved-face handle twice yields angle ≈ 0.
///
/// Curved-face caveat: the area centroid of the cylindrical side surface lies
/// on the cylinder axis (not on the surface itself). `ShapeAnalysis_Surface
/// ::ValueOfUV` projects it to the nearest surface point, producing a radially
/// outward normal at that point. The exact azimuth is implementation-defined,
/// but the normal is always perpendicular to the Z axis, so:
///   `surface_angle(side, cap) ≈ π/2` regardless of which azimuth is chosen.
#[test]
fn cylinder_curved_face_returns_finite_angle() {
    let mut kernel = OcctKernel::new();
    let cyl_id = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(5.0),
            height: Value::Real(10.0),
        })
        .expect("cylinder creation should succeed")
        .id;

    let faces = kernel
        .extract_faces(cyl_id)
        .expect("extract_faces should succeed for cylinder");
    assert_eq!(
        faces.len(),
        3,
        "expected 3 faces for a cylinder (bottom, top, side)"
    );

    // Every pair must return Ok with a finite angle in [0, π].
    for i in 0..faces.len() {
        for j in 0..faces.len() {
            let angle = kernel
                .surface_angle(faces[i], faces[j])
                .unwrap_or_else(|e| panic!("surface_angle(face[{i}], face[{j}]) failed: {e:?}"));
            assert!(
                angle.is_finite() && (0.0..=PI + 1e-9).contains(&angle),
                "surface_angle(face[{i}], face[{j}]) = {angle:.10}, \
                 expected finite value in [0, π]"
            );
            // Same-face degenerate case: angle must be ≈ 0.
            if i == j {
                assert!(
                    angle.abs() < 1e-9,
                    "surface_angle(face[{i}], face[{i}]) = {angle:.10}, expected ≈ 0"
                );
            }
        }
    }

    // Identify the two flat caps: their outward normals are (anti-)parallel to Z,
    // so |n_z| ≈ 1. Use the typed helper to avoid hand-parsing FaceNormal JSON.
    let cap_indices: Vec<usize> = (0..3)
        .filter(|&i| {
            kernel
                .face_outward_unit_normal_for_test(faces[i])
                .expect("face_outward_unit_normal_for_test should succeed")[2]
                .abs()
                > 0.99
        })
        .collect();
    assert_eq!(
        cap_indices.len(),
        2,
        "expected exactly 2 flat cap faces with |n_z| ≈ 1, found {cap_indices:?}"
    );

    // The two caps have anti-parallel normals (+Z and −Z): angle ≈ π.
    let (ca, cb) = (cap_indices[0], cap_indices[1]);
    let cap_angle = kernel
        .surface_angle(faces[ca], faces[cb])
        .expect("surface_angle(cap, cap) should succeed");
    assert!(
        (cap_angle - PI).abs() < 1e-9,
        "cylinder caps: expected angle ≈ π ≈ {PI:.10}, got {cap_angle:.10}"
    );

    // The side face is the one that is not a cap.
    let side_idx = (0..3).find(|i| !cap_indices.contains(i)).unwrap();
    // Side vs each cap: the lateral normal is radially outward (⊥ Z), so
    // angle with either cap normal (which is ±Z) ≈ π/2.
    for &cap_idx in &cap_indices {
        let angle = kernel
            .surface_angle(faces[side_idx], faces[cap_idx])
            .expect("surface_angle(side, cap) should succeed");
        assert!(
            (angle - PI / 2.0).abs() < 1e-6,
            "cylinder side vs cap[{cap_idx}]: expected ≈ π/2 ≈ {:.10}, got {angle:.10}",
            PI / 2.0
        );
    }
}

// ---------------------------------------------------------------------------
// query() round-trip — task 2324 stdlib wiring
// ---------------------------------------------------------------------------

/// Round-trip `GeometryQuery::SurfaceAngle` via the generic `kernel.query(...)`
/// dispatch. Picks one perpendicular box-face pair via the existing adjacency
/// helper and asserts the kernel emits `Value::Real(rad)` ≈ π/2.
///
/// Mirrors the structure of `box_adjacent_faces_yield_pi_over_two` but goes
/// through the typed-query interface that the eval-side dispatcher uses.
#[test]
fn query_surface_angle_returns_pi_over_two_for_adjacent_faces() {
    let (mut kernel, box_id) = box_kernel();
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces should succeed");
    assert_eq!(faces.len(), 6, "expected 6 faces for a box");

    // Pick face[0] and one of its 4 adjacent neighbours — perpendicular by
    // construction in a box.
    let neighbors = neighbors_of(&kernel, box_id, 0);
    let &j = neighbors
        .iter()
        .next()
        .expect("face 0 must have at least one adjacent face");

    let value = kernel
        .query(&GeometryQuery::SurfaceAngle {
            face_a: faces[0],
            face_b: faces[j],
        })
        .expect("query(SurfaceAngle) should succeed for valid face handles");
    let rad = match value {
        Value::Real(r) => r,
        other => panic!("expected Value::Real from SurfaceAngle, got {other:?}"),
    };
    assert!(
        (rad - PI / 2.0).abs() < 1e-9,
        "adjacent box faces (0, {j}): expected π/2 ≈ {:.10}, got {rad:.10}",
        PI / 2.0
    );
}
