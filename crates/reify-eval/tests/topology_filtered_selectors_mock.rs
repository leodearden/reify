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

#[test]
fn faces_by_normal_zero_target_returns_query_failed() {
    // normalize3 rejects vectors with magnitude below f64::EPSILON; the selector
    // must surface a QueryFailed before touching the kernel at all.
    let parent = GeometryHandleId(1);
    let mut kernel = MockGeometryKernel::new();

    let result = topology_selectors::faces_by_normal(&mut kernel, parent, [0.0, 0.0, 0.0], 0.1);
    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("non-zero and finite"),
                "error should mention 'non-zero and finite', got: {msg:?}"
            );
        }
        other => panic!("expected Err(QueryFailed) for zero target, got {:?}", other),
    }
}

#[test]
fn faces_by_normal_nan_target_returns_query_failed() {
    // The !mag.is_finite() guard catches NaN before the mag < f64::EPSILON
    // check would (any comparison with NaN is false, so NaN would otherwise
    // slip through as "not too small").
    let parent = GeometryHandleId(1);
    let mut kernel = MockGeometryKernel::new();

    let result =
        topology_selectors::faces_by_normal(&mut kernel, parent, [f64::NAN, 0.0, 1.0], 0.1);
    assert!(
        matches!(result, Err(QueryError::QueryFailed(_))),
        "NaN target must produce Err(QueryFailed), got {:?}",
        result
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// edges_parallel_to
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn edges_parallel_to_anti_parallel_tangent_is_accepted() {
    // edges_parallel_to is orientation-agnostic: the kernel may return either
    // direction along an edge, so an anti-parallel tangent must be accepted.
    // This is enforced via abs(dot) in the predicate (unlike faces_by_normal).
    let parent = GeometryHandleId(1);
    let edge = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![edge])
        .with_edge_tangent_result(
            edge,
            Value::String("{\"x\":-1.0,\"y\":0.0,\"z\":0.0}".into()), // -x tangent
        );

    // Target axis is +x with 0.1 rad tolerance; anti-parallel -x tangent is accepted.
    let result =
        topology_selectors::edges_parallel_to(&mut kernel, parent, [1.0, 0.0, 0.0], 0.1)
            .expect("edges_parallel_to should succeed");
    assert_eq!(
        result,
        vec![edge],
        "anti-parallel tangent (-x) must be accepted when axis is +x (orientation-agnostic)"
    );
}

#[test]
fn edges_parallel_to_zero_axis_returns_query_failed() {
    let parent = GeometryHandleId(1);
    let mut kernel = MockGeometryKernel::new();

    let result =
        topology_selectors::edges_parallel_to(&mut kernel, parent, [0.0, 0.0, 0.0], 0.1);
    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("non-zero and finite"),
                "error should mention 'non-zero and finite', got: {msg:?}"
            );
        }
        other => panic!("expected Err(QueryFailed) for zero axis, got {:?}", other),
    }
}

#[test]
fn edges_parallel_to_nan_axis_returns_query_failed() {
    // Mirrors faces_by_normal_nan_target: the !mag.is_finite() guard in
    // normalize3 is what catches the NaN-poisoned axis.
    let parent = GeometryHandleId(1);
    let mut kernel = MockGeometryKernel::new();

    let result =
        topology_selectors::edges_parallel_to(&mut kernel, parent, [f64::NAN, 0.0, 1.0], 0.1);
    assert!(
        matches!(result, Err(QueryError::QueryFailed(_))),
        "NaN axis must produce Err(QueryFailed), got {:?}",
        result
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// edges_at_height
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn edges_at_height_z_window_inclusive_at_endpoints() {
    // Predicate: (zmin - z_m).abs() <= tol_m && (zmax - z_m).abs() <= tol_m
    // (both endpoints inclusive via <=).
    //
    // Layout (all edges are horizontal so zmin == zmax):
    //   e_low       z = 0.000   (exactly tol below target when tol = 0.005)
    //   e_at_target z = 0.005   (on the target plane)
    //   e_high      z = 0.010   (exactly tol above target when tol = 0.005)
    let parent = GeometryHandleId(1);
    let e_low = GeometryHandleId(2);
    let e_at_target = GeometryHandleId(3);
    let e_high = GeometryHandleId(4);

    let bbox_json = |z: f64| {
        Value::String(format!(
            "{{\"xmin\":0.0,\"ymin\":0.0,\"zmin\":{z},\
              \"xmax\":1.0,\"ymax\":1.0,\"zmax\":{z}}}"
        ))
    };

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e_low, e_at_target, e_high])
        .with_bbox_result(e_low, bbox_json(0.000))
        .with_bbox_result(e_at_target, bbox_json(0.005))
        .with_bbox_result(e_high, bbox_json(0.010));

    // Full tolerance tol=0.005 — both boundary edges are exactly at distance
    // tol from the target plane, so the <= predicate must include them.
    let all =
        topology_selectors::edges_at_height(&mut kernel, parent, 0.005, 0.005)
            .expect("edges_at_height should succeed with tol=0.005");
    assert_eq!(
        all,
        vec![e_low, e_at_target, e_high],
        "all three edges should be included when tol equals their z-distance from target"
    );

    // Tighter tolerance tol=0.0049 — boundary edges are now just outside the
    // window (0.005 > 0.0049) and only the on-target edge survives.
    let at_target_only =
        topology_selectors::edges_at_height(&mut kernel, parent, 0.005, 0.0049)
            .expect("edges_at_height should succeed with tol=0.0049");
    assert_eq!(
        at_target_only,
        vec![e_at_target],
        "only the on-target edge should survive when tol=0.0049 is just below the z-distance"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Additional error-path coverage (reviewer suggestion: fill gaps so always-on
// CI exercises branches that are OCCT-gated in topology_filtered_selectors.rs)
// ─────────────────────────────────────────────────────────────────────────────

/// faces_by_area mirrors the non-Real contract guard that edges_by_length tests
/// for EdgeLength — SurfaceArea must also be Value::Real.
#[test]
fn faces_by_area_returns_query_failed_when_surface_area_is_int() {
    let parent = GeometryHandleId(1);
    let f = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f])
        .with_surface_area_result(f, Value::Int(5)); // intentionally wrong type

    let result = topology_selectors::faces_by_area(&mut kernel, parent, 0.0, 100.0);
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

/// Per-sub-shape query errors propagate through selectors via `?`.
/// Configuring extraction without the area result exercises the Err path
/// (mock returns QueryFailed for unconfigured typed queries).
#[test]
fn faces_by_area_propagates_err_from_surface_area_query() {
    let parent = GeometryHandleId(1);
    let f = GeometryHandleId(2);

    // Extraction is configured but the SurfaceArea result is NOT — the mock
    // will return Err(QueryFailed("no mock result for ...")) for the query.
    let mut kernel = MockGeometryKernel::new().with_extracted_faces(parent, vec![f]);

    let result = topology_selectors::faces_by_area(&mut kernel, parent, 0.0, 1.0);
    assert!(
        matches!(result, Err(QueryError::QueryFailed(_))),
        "Err from SurfaceArea query should propagate through faces_by_area, got {:?}",
        result
    );
}

/// Malformed JSON in the FaceNormal payload must produce QueryFailed.
#[test]
fn faces_by_normal_malformed_json_returns_query_failed() {
    let parent = GeometryHandleId(1);
    let f = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f])
        .with_face_normal_result(f, Value::String("not-valid-json".into()));

    let result =
        topology_selectors::faces_by_normal(&mut kernel, parent, [0.0, 0.0, 1.0], 0.1);
    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("malformed JSON") || msg.contains("FaceNormal"),
                "error should mention malformed JSON or FaceNormal, got: {msg:?}"
            );
        }
        other => panic!("expected Err(QueryFailed) for malformed JSON, got {:?}", other),
    }
}

/// A well-formed FaceNormal JSON that omits the `z` key must produce
/// QueryFailed. This pins the per-key Some/None branch of `parse_xyz_json`:
/// `x` and `y` are set but `z?` short-circuits to `None`, so
/// `parse_xyz_value` surfaces "FaceNormal returned malformed JSON Point3".
#[test]
fn faces_by_normal_well_formed_xyz_missing_z_returns_query_failed() {
    let parent = GeometryHandleId(1);
    let f = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f])
        .with_face_normal_result(f, Value::String("{\"x\":1.0,\"y\":0.0}".into()));

    let result =
        topology_selectors::faces_by_normal(&mut kernel, parent, [0.0, 0.0, 1.0], 0.1);
    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("malformed JSON") || msg.contains("FaceNormal"),
                "error should mention malformed JSON or FaceNormal, got: {msg:?}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed) for missing-z face normal JSON, got {:?}",
            other
        ),
    }
}

/// A face whose normal parses as the zero vector must produce QueryFailed.
/// This exercises the normalize3 degenerate-face guard (distinct from the
/// zero-target guard tested by faces_by_normal_zero_target_returns_query_failed).
#[test]
fn faces_by_normal_degenerate_face_normal_returns_query_failed() {
    let parent = GeometryHandleId(1);
    let f = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f])
        .with_face_normal_result(
            f,
            Value::String("{\"x\":0.0,\"y\":0.0,\"z\":0.0}".into()),
        );

    let result =
        topology_selectors::faces_by_normal(&mut kernel, parent, [0.0, 0.0, 1.0], 0.1);
    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("degenerate"),
                "error should mention 'degenerate' for a zero face normal, got: {msg:?}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed) for zero face normal, got {:?}",
            other
        ),
    }
}

/// Malformed JSON in the EdgeTangent payload must produce QueryFailed.
#[test]
fn edges_parallel_to_malformed_tangent_json_returns_query_failed() {
    let parent = GeometryHandleId(1);
    let e = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e])
        .with_edge_tangent_result(e, Value::String("{bad json}".into()));

    let result =
        topology_selectors::edges_parallel_to(&mut kernel, parent, [1.0, 0.0, 0.0], 0.1);
    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("malformed JSON") || msg.contains("EdgeTangent"),
                "error should mention malformed JSON or EdgeTangent, got: {msg:?}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed) for malformed tangent JSON, got {:?}",
            other
        ),
    }
}

/// Malformed JSON in the BoundingBox payload must produce QueryFailed.
#[test]
fn edges_at_height_malformed_bbox_json_returns_query_failed() {
    let parent = GeometryHandleId(1);
    let e = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e])
        .with_bbox_result(e, Value::String("this is not json".into()));

    let result = topology_selectors::edges_at_height(&mut kernel, parent, 0.0, 1.0);
    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("malformed JSON") || msg.contains("BoundingBox"),
                "error should mention malformed JSON or BoundingBox, got: {msg:?}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed) for malformed bbox JSON, got {:?}",
            other
        ),
    }
}

/// A structurally-valid bbox JSON that omits `zmin`/`zmax` must produce
/// QueryFailed. This drives `parse_flat_number_object` through every
/// iteration (xmin/xmax/ymin/ymax are all tolerated at line 458 of
/// topology_selectors.rs) and only fails at the final `Some((zmin?, zmax?))`
/// — the per-key Some/None branch of `parse_bbox_z_extents_json` that the
/// malformed-string fixture above does NOT exercise.
#[test]
fn edges_at_height_well_formed_bbox_missing_zmin_zmax_returns_query_failed() {
    let parent = GeometryHandleId(1);
    let e = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e])
        .with_bbox_result(
            e,
            Value::String("{\"xmin\":0.0,\"xmax\":1.0,\"ymin\":0.0,\"ymax\":1.0}".into()),
        );

    let result = topology_selectors::edges_at_height(&mut kernel, parent, 0.0, 1.0);
    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("malformed JSON") || msg.contains("BoundingBox"),
                "error should mention malformed JSON or BoundingBox, got: {msg:?}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed) for bbox missing zmin/zmax, got {:?}",
            other
        ),
    }
}
