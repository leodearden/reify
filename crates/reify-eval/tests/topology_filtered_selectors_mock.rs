//! Mock-kernel unit tests for `reify_eval::topology_selectors` (task 2511).
//!
//! These tests are always-on (no OCCT runtime required) and complement the
//! OCCT-backed integration tests in `topology_filtered_selectors.rs` which
//! skip at runtime when OCCT is unavailable.
//!
//! Fixtures use `MockGeometryKernel` populated with pre-configured
//! `with_extracted_edges` / `with_extracted_faces` / `with_edge_length_result`
//! / `with_edge_tangent_result` / `with_face_normal_result` / `with_bbox_result`
//! / `with_surface_area_result` builders introduced in task 2511.
//!
//! Handle ids are pre-allocated by convention: id=1 is the parent solid,
//! id=2…N are the sub-shape (edge / face) handles returned by the configured
//! extraction.

use reify_eval::topology_selectors;
use reify_test_support::MockGeometryKernel;
use reify_types::{GeometryHandleId, QueryError, Value};

// ─────────────────────────────────────────────────────────────────────────────
// edges_by_length
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn edges_by_length_inclusive_at_min_and_max_endpoints() {
    let parent = GeometryHandleId(1);
    let e_low = GeometryHandleId(2);
    let e_mid = GeometryHandleId(3);
    let e_high = GeometryHandleId(4);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e_low, e_mid, e_high])
        .with_edge_length_result(e_low, Value::Real(0.010))
        .with_edge_length_result(e_mid, Value::Real(0.020))
        .with_edge_length_result(e_high, Value::Real(0.030));

    // Full window [0.010, 0.030] — both endpoints are inclusive (>= / <=).
    let all = topology_selectors::edges_by_length(&mut kernel, parent, 0.010, 0.030)
        .expect("edges_by_length should succeed with full window");
    assert_eq!(
        all,
        vec![e_low, e_mid, e_high],
        "all three edges should be included when window covers all lengths exactly"
    );

    // Tighter window [0.011, 0.029] — just-outside endpoints are excluded.
    let mid_only = topology_selectors::edges_by_length(&mut kernel, parent, 0.011, 0.029)
        .expect("edges_by_length should succeed with tighter window");
    assert_eq!(
        mid_only,
        vec![e_mid],
        "only the middle edge should survive when min/max endpoints are just outside"
    );
}

#[test]
fn edges_by_length_returns_query_failed_when_edge_length_is_int() {
    // Kernels are expected to return Value::Real for EdgeLength queries.
    // If a kernel incorrectly returns Value::Int the selector must surface
    // a QueryFailed rather than silently skipping or panicking.
    let parent = GeometryHandleId(1);
    let e = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e])
        .with_edge_length_result(e, Value::Int(5)); // intentionally wrong type

    let result = topology_selectors::edges_by_length(&mut kernel, parent, 0.0, 100.0);
    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("non-real value"),
                "error message should mention 'non-real value', got: {msg:?}"
            );
        }
        other => panic!("expected Err(QueryFailed), got {:?}", other),
    }
}

#[test]
fn edges_by_length_propagates_invalid_handle_from_extract_edges() {
    let parent = GeometryHandleId(1);

    let mut kernel = MockGeometryKernel::new()
        .with_extract_edges_error(parent, QueryError::InvalidHandle(parent));

    let result = topology_selectors::edges_by_length(&mut kernel, parent, 0.0, 1.0);
    assert!(
        matches!(result, Err(QueryError::InvalidHandle(h)) if h == parent),
        "InvalidHandle from extract_edges should propagate unchanged, got {:?}",
        result
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// faces_by_area
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn faces_by_area_inclusive_at_min_and_max_endpoints() {
    let parent = GeometryHandleId(1);
    let f_small = GeometryHandleId(2);
    let f_mid = GeometryHandleId(3);
    let f_big = GeometryHandleId(4);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f_small, f_mid, f_big])
        .with_surface_area_result(f_small, Value::Real(1.0e-4))
        .with_surface_area_result(f_mid, Value::Real(4.0e-4))
        .with_surface_area_result(f_big, Value::Real(9.0e-4));

    // Full window [1.0e-4, 9.0e-4] — both endpoints inclusive.
    let all = topology_selectors::faces_by_area(&mut kernel, parent, 1.0e-4, 9.0e-4)
        .expect("faces_by_area should succeed with full window");
    assert_eq!(
        all,
        vec![f_small, f_mid, f_big],
        "all three faces should be included when window covers all areas exactly"
    );

    // Tighter window [2.0e-4, 5.0e-4] — just-outside endpoints are excluded.
    let mid_only = topology_selectors::faces_by_area(&mut kernel, parent, 2.0e-4, 5.0e-4)
        .expect("faces_by_area should succeed with tighter window");
    assert_eq!(
        mid_only,
        vec![f_mid],
        "only the middle face should survive when min/max endpoints are just outside"
    );
}

#[test]
fn faces_by_area_propagates_invalid_handle_from_extract_faces() {
    let parent = GeometryHandleId(1);

    let mut kernel = MockGeometryKernel::new()
        .with_extract_faces_error(parent, QueryError::InvalidHandle(parent));

    let result = topology_selectors::faces_by_area(&mut kernel, parent, 0.0, 1.0);
    assert!(
        matches!(result, Err(QueryError::InvalidHandle(h)) if h == parent),
        "InvalidHandle from extract_faces should propagate unchanged, got {:?}",
        result
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// faces_by_normal
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn faces_by_normal_exactly_aligned_face_at_zero_tolerance_is_accepted() {
    // Smoke-test for the clamp(-1.0, 1.0) path: when target and normal are
    // exactly aligned the dot product can computationally exceed 1.0 by a few
    // ULPs. Without the clamp, acos would return NaN and the face would be
    // silently dropped. The clamp turns the risk into exact-zero acceptance.
    let parent = GeometryHandleId(1);
    let face = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![face])
        .with_face_normal_result(
            face,
            Value::String("{\"x\":1.0,\"y\":0.0,\"z\":0.0}".into()),
        );

    let result = topology_selectors::faces_by_normal(&mut kernel, parent, [1.0, 0.0, 0.0], 0.0)
        .expect("faces_by_normal should succeed for aligned face");
    assert_eq!(result, vec![face], "exactly-aligned face must be accepted at zero tolerance");
}

#[test]
fn faces_by_normal_anti_parallel_target_is_rejected() {
    // faces_by_normal is orientation-aware: a face whose normal is anti-parallel
    // to the target (180° off) must be rejected even at a generous tolerance,
    // distinguishing it from edges_parallel_to which accepts anti-parallel.
    let parent = GeometryHandleId(1);
    let face = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![face])
        .with_face_normal_result(
            face,
            Value::String("{\"x\":0.0,\"y\":0.0,\"z\":-1.0}".into()), // -z normal
        );

    // Target is +z; tolerance 0.1 rad; anti-parallel -z is ~π rad away → rejected.
    let result = topology_selectors::faces_by_normal(&mut kernel, parent, [0.0, 0.0, 1.0], 0.1)
        .expect("faces_by_normal should succeed");
    assert_eq!(
        result,
        vec![],
        "anti-parallel face (-z normal, +z target) must be rejected even at 0.1 rad tolerance"
    );
}
