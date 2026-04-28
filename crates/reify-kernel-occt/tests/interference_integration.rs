//! Integration tests for interference and clearance queries:
//! `shapes_intersect` and `min_clearance`.
//!
//! These tests verify that:
//! - `shapes_intersect` returns true for overlapping boxes and false for disjoint boxes.
//! - `min_clearance` returns the correct gap distance for disjoint boxes and ~0 for overlapping.
//! - Invalid handles return `QueryError::InvalidHandle`.
//!
//! Fixture: two 10×10×10 boxes — `box_a` centered at the origin (x∈[-5,5]),
//! `box_b` translated along X by `dx` (x∈[dx-5, dx+5]).
//! - `dx=50.0`: 40-unit gap (mirrors the `distance_between_shapes` lib.rs test at line 3340).
//! - `dx=5.0`: 50% overlap (x∈[0,5] is the common slab).

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_types::{GeometryHandleId, GeometryOp, QueryError, Value};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Build a kernel with two 10×10×10 boxes.
///
/// `box_a` is centered at the origin (x∈[-5,5]).
/// `box_b` is translated by `dx` along X (x∈[dx-5, dx+5]).
///
/// Returns `(kernel, box_a_id, box_b_id)`.
fn two_box_kernel(dx: f64) -> (OcctKernel, GeometryHandleId, GeometryHandleId) {
    let mut kernel = OcctKernel::new();

    let box_a = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("box_a creation should succeed");

    let box_b_raw = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("box_b creation should succeed");

    let box_b = kernel
        .execute(&GeometryOp::Translate {
            target: box_b_raw.id,
            dx,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("box_b translate should succeed");

    (kernel, box_a.id, box_b.id)
}

// ---------------------------------------------------------------------------
// shapes_intersect — happy path
// ---------------------------------------------------------------------------

/// Two boxes with 50% X-overlap (dx=5.0, common slab x∈[0,5]) should intersect.
#[test]
fn shapes_intersect_returns_true_for_overlapping_boxes() {
    let (kernel, box_a_id, box_b_id) = two_box_kernel(5.0);
    let result = kernel.shapes_intersect(box_a_id, box_b_id);
    match result {
        Ok(true) => {}
        Ok(false) => panic!("overlapping boxes should intersect, got Ok(false)"),
        Err(e) => panic!("overlapping boxes should intersect, got Err({e:?})"),
    }
}

/// Two boxes with a 40-unit gap (dx=50.0, box_a x∈[-5,5], box_b x∈[45,55]) should not intersect.
#[test]
fn shapes_intersect_returns_false_for_disjoint_boxes() {
    let (kernel, box_a_id, box_b_id) = two_box_kernel(50.0);
    let result = kernel.shapes_intersect(box_a_id, box_b_id);
    match result {
        Ok(false) => {}
        Ok(true) => panic!("disjoint boxes should not intersect, got Ok(true)"),
        Err(e) => panic!("disjoint boxes should not intersect, got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// min_clearance — happy path
// ---------------------------------------------------------------------------

/// Disjoint boxes (dx=50.0): box_a x∈[-5,5], box_b x∈[45,55] → gap = 40.
/// Mirrors the `distance_between_shapes` lib.rs test (line ~3367).
#[test]
fn min_clearance_between_disjoint_boxes_matches_gap() {
    let (kernel, box_a_id, box_b_id) = two_box_kernel(50.0);
    match kernel.min_clearance(box_a_id, box_b_id) {
        Ok(d) => assert!(
            (d - 40.0).abs() < 1e-6,
            "expected clearance ~40.0, got {d}"
        ),
        Err(e) => panic!("expected Ok(~40.0), got Err({e:?})"),
    }
}

/// Overlapping boxes (dx=5.0): volumes intersect → min clearance is 0.
#[test]
fn min_clearance_between_overlapping_boxes_is_zero() {
    let (kernel, box_a_id, box_b_id) = two_box_kernel(5.0);
    match kernel.min_clearance(box_a_id, box_b_id) {
        Ok(d) => assert!(
            d.abs() < 1e-9,
            "expected clearance ~0.0 for overlapping boxes, got {d}"
        ),
        Err(e) => panic!("expected Ok(~0.0), got Err({e:?})"),
    }
}

// ---------------------------------------------------------------------------
// Error path — invalid handle
// ---------------------------------------------------------------------------

/// An unknown handle should return `QueryError::InvalidHandle` from `shapes_intersect`.
#[test]
fn shapes_intersect_with_unknown_handle_returns_invalid_handle() {
    let (kernel, box_id, _) = two_box_kernel(50.0);
    let unknown = GeometryHandleId(999);
    match kernel.shapes_intersect(box_id, unknown) {
        Err(QueryError::InvalidHandle(id)) if id == unknown => {}
        Err(QueryError::InvalidHandle(id)) => panic!(
            "expected InvalidHandle({:?}), got InvalidHandle({:?})",
            unknown, id
        ),
        other => panic!("expected Err(InvalidHandle({unknown:?})), got {other:?}"),
    }
}

/// An unknown handle should return `QueryError::InvalidHandle` from `min_clearance`.
#[test]
fn min_clearance_with_unknown_handle_returns_invalid_handle() {
    let (kernel, box_id, _) = two_box_kernel(50.0);
    let unknown = GeometryHandleId(999);
    match kernel.min_clearance(box_id, unknown) {
        Err(QueryError::InvalidHandle(id)) if id == unknown => {}
        Err(QueryError::InvalidHandle(id)) => panic!(
            "expected InvalidHandle({:?}), got InvalidHandle({:?})",
            unknown, id
        ),
        other => panic!("expected Err(InvalidHandle({unknown:?})), got {other:?}"),
    }
}
