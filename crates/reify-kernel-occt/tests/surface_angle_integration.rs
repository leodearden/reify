//! Integration tests for `OcctKernel::surface_angle` — returns the dihedral
//! angle (in radians) between two `TopoDS_Face` shapes via face-normal dot.
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
use reify_types::{GeometryHandleId, GeometryOp, GeometryQuery, QueryError, Value};

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
fn neighbors_of(
    kernel: &OcctKernel,
    shape: GeometryHandleId,
    face_index: usize,
) -> HashSet<usize> {
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

    let neighbors: Vec<HashSet<usize>> = (0..6)
        .map(|i| neighbors_of(&kernel, box_id, i))
        .collect();

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
            .unwrap_or_else(|e| {
                panic!("surface_angle(face[{i}], face[{j}]) returned Err: {e:?}")
            });
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
        Err(QueryError::InvalidHandle(id)) => panic!(
            "expected InvalidHandle({unknown:?}), got InvalidHandle({id:?})"
        ),
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
        Err(QueryError::InvalidHandle(id)) => panic!(
            "expected InvalidHandle({unknown:?}), got InvalidHandle({id:?})"
        ),
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
