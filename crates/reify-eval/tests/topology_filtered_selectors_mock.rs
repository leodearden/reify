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
