//! Mock-kernel and pure-Rust unit tests for the v0.2 selector vocabulary
//! (`reify_eval::selector_vocabulary_v2`), task 2658 (PRD task 10).
//!
//! These tests are always-on (no OCCT runtime required) and complement the
//! OCCT-backed integration tests in `selector_vocabulary_v2_e2e.rs` which
//! skip at runtime when OCCT is unavailable.
//!
//! Convention: handle id=1 is the parent solid, id=2..N are the sub-shape
//! (edge / face) handles returned by the configured extraction. This
//! mirrors `topology_filtered_selectors_mock.rs`.

use reify_eval::selector_vocabulary_v2::{
    Axis, ExtremalSense, adjacent_to_face, ancestor_faces_of_edge, complement, created_by_feature,
    edges_by_curve_kind, edges_perpendicular_to, except, extremal_by_bbox, extremal_by_centroid,
    faces_by_surface_kind, faces_perpendicular_to, geom_universal, has_user_label, intersect,
    owner_body_of, siblings_of_face, split_by_feature, union, user_label_eq,
};
use reify_test_support::MockGeometryKernel;
use reify_ir::{CapKind, EdgeCurveKind, FaceSurfaceKind, FeatureId, GeometryHandleId, ModEntry, QueryError, Role, TopologyAttribute, TopologyAttributeTable, Value};

// ─────────────────────────────────────────────────────────────────────────────
// intersect — set intersection over Vec<GeometryHandleId>
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn intersect_keeps_left_encounter_order_for_common_elements() {
    let a = vec![
        GeometryHandleId(10),
        GeometryHandleId(20),
        GeometryHandleId(30),
        GeometryHandleId(40),
    ];
    let b = vec![
        GeometryHandleId(40),
        GeometryHandleId(20),
        GeometryHandleId(99),
    ];

    // Both 20 and 40 are in both; order is the LEFT operand's order
    // (so 20 before 40, not 40 before 20 as `b` would suggest).
    assert_eq!(
        intersect(&a, &b),
        vec![GeometryHandleId(20), GeometryHandleId(40)],
        "intersect must preserve left-operand encounter order"
    );
}

#[test]
fn intersect_dedupes_duplicates_in_left_operand() {
    // The left operand contains duplicates; intersect must emit each
    // common element at most once, at its first encounter position.
    let a = vec![
        GeometryHandleId(10),
        GeometryHandleId(20),
        GeometryHandleId(20),
        GeometryHandleId(30),
        GeometryHandleId(10),
    ];
    let b = vec![GeometryHandleId(10), GeometryHandleId(20)];

    assert_eq!(
        intersect(&a, &b),
        vec![GeometryHandleId(10), GeometryHandleId(20)],
        "intersect must dedupe on first-seen even when LHS has duplicates"
    );
}

#[test]
fn intersect_with_disjoint_inputs_is_empty() {
    let a = vec![GeometryHandleId(1), GeometryHandleId(2)];
    let b = vec![GeometryHandleId(3), GeometryHandleId(4)];
    assert!(intersect(&a, &b).is_empty());
}

#[test]
fn intersect_with_empty_inputs_is_empty() {
    let a: Vec<GeometryHandleId> = vec![];
    let b = vec![GeometryHandleId(1)];
    assert!(intersect(&a, &b).is_empty());
    assert!(intersect(&b, &a).is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// union — set union with left-then-right encounter order
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn union_returns_left_then_right_only_new_elements() {
    let a = vec![
        GeometryHandleId(10),
        GeometryHandleId(20),
        GeometryHandleId(30),
    ];
    let b = vec![
        GeometryHandleId(20), // already in a; skip
        GeometryHandleId(40), // new
        GeometryHandleId(10), // already in a; skip
        GeometryHandleId(50), // new
    ];

    assert_eq!(
        union(&a, &b),
        vec![
            GeometryHandleId(10),
            GeometryHandleId(20),
            GeometryHandleId(30),
            GeometryHandleId(40),
            GeometryHandleId(50),
        ],
        "union returns a in encounter order, then elements of b not in a in encounter order"
    );
}

#[test]
fn union_dedupes_duplicates_within_either_operand() {
    let a = vec![
        GeometryHandleId(1),
        GeometryHandleId(1),
        GeometryHandleId(2),
    ];
    let b = vec![
        GeometryHandleId(2),
        GeometryHandleId(3),
        GeometryHandleId(3),
    ];

    assert_eq!(
        union(&a, &b),
        vec![
            GeometryHandleId(1),
            GeometryHandleId(2),
            GeometryHandleId(3)
        ],
        "union must dedupe on first-seen even when either operand has duplicates"
    );
}

#[test]
fn union_with_empty_left_returns_dedupe_of_right() {
    let a: Vec<GeometryHandleId> = vec![];
    let b = vec![
        GeometryHandleId(1),
        GeometryHandleId(2),
        GeometryHandleId(1),
    ];
    assert_eq!(
        union(&a, &b),
        vec![GeometryHandleId(1), GeometryHandleId(2)],
    );
}

#[test]
fn union_with_empty_right_returns_dedupe_of_left() {
    let a = vec![
        GeometryHandleId(1),
        GeometryHandleId(2),
        GeometryHandleId(1),
    ];
    let b: Vec<GeometryHandleId> = vec![];
    assert_eq!(
        union(&a, &b),
        vec![GeometryHandleId(1), GeometryHandleId(2)],
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// complement — set difference (universe \ exclude), preserving universe order
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn complement_returns_universe_elements_not_in_exclude() {
    let universe = vec![
        GeometryHandleId(10),
        GeometryHandleId(20),
        GeometryHandleId(30),
        GeometryHandleId(40),
    ];
    let exclude = vec![GeometryHandleId(20), GeometryHandleId(40)];
    assert_eq!(
        complement(&universe, &exclude),
        vec![GeometryHandleId(10), GeometryHandleId(30)],
        "complement must return universe elements not in exclude, in universe order"
    );
}

#[test]
fn complement_empty_universe_is_empty() {
    let universe: Vec<GeometryHandleId> = vec![];
    let exclude = vec![GeometryHandleId(1)];
    assert!(complement(&universe, &exclude).is_empty());
}

#[test]
fn complement_empty_exclude_returns_dedupe_of_universe() {
    let universe = vec![
        GeometryHandleId(1),
        GeometryHandleId(2),
        GeometryHandleId(1), // duplicate — should dedupe to first
        GeometryHandleId(3),
    ];
    let exclude: Vec<GeometryHandleId> = vec![];
    assert_eq!(
        complement(&universe, &exclude),
        vec![
            GeometryHandleId(1),
            GeometryHandleId(2),
            GeometryHandleId(3)
        ],
        "with empty exclude, complement = universe with dedup-on-first-seen"
    );
}

#[test]
fn complement_full_overlap_is_empty() {
    let universe = vec![GeometryHandleId(1), GeometryHandleId(2)];
    let exclude = vec![GeometryHandleId(1), GeometryHandleId(2)];
    assert!(complement(&universe, &exclude).is_empty());
}

#[test]
fn complement_partial_overlap_with_duplicates_dedupes() {
    let universe = vec![
        GeometryHandleId(1),
        GeometryHandleId(2),
        GeometryHandleId(2),
        GeometryHandleId(3),
        GeometryHandleId(1),
    ];
    let exclude = vec![GeometryHandleId(2), GeometryHandleId(2)];
    assert_eq!(
        complement(&universe, &exclude),
        vec![GeometryHandleId(1), GeometryHandleId(3)],
        "complement dedupes universe duplicates and tolerates exclude duplicates"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// except — alias for complement (PRD line 79 names them distinctly)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn except_matches_complement_on_simple_inputs() {
    let a = vec![
        GeometryHandleId(1),
        GeometryHandleId(2),
        GeometryHandleId(3),
    ];
    let b = vec![GeometryHandleId(2)];
    assert_eq!(
        except(&a, &b),
        complement(&a, &b),
        "except is currently identical to complement"
    );
    assert_eq!(
        except(&a, &b),
        vec![GeometryHandleId(1), GeometryHandleId(3)],
    );
}

#[test]
fn except_with_full_overlap_is_empty() {
    let a = vec![GeometryHandleId(1), GeometryHandleId(2)];
    let b = vec![GeometryHandleId(2), GeometryHandleId(1)];
    assert!(except(&a, &b).is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// faces_perpendicular_to — `#axis` direction filter for faces (PRD line 76)
//
// A face's normal `n` is perpendicular to `axis` iff n ⟂ axis, i.e. the
// projection |n · axis| is small. Sign-tolerant: ±axis both qualify.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn faces_perpendicular_to_keeps_faces_orthogonal_to_axis() {
    // Three faces with normals exactly +X, +Y, +Z.
    // For axis = +X, perpendicular faces are those with n · X = 0 → +Y, +Z.
    // The +X face has n · X = 1, so it must be dropped.
    let parent = GeometryHandleId(1);
    let f_x = GeometryHandleId(2);
    let f_y = GeometryHandleId(3);
    let f_z = GeometryHandleId(4);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f_x, f_y, f_z])
        .with_face_normal_result(f_x, Value::String("{\"x\":1.0,\"y\":0.0,\"z\":0.0}".into()))
        .with_face_normal_result(f_y, Value::String("{\"x\":0.0,\"y\":1.0,\"z\":0.0}".into()))
        .with_face_normal_result(f_z, Value::String("{\"x\":0.0,\"y\":0.0,\"z\":1.0}".into()));

    let result = faces_perpendicular_to(&mut kernel, parent, [1.0, 0.0, 0.0], 1.0_f64.to_radians())
        .expect("faces_perpendicular_to should succeed for axis-aligned normals");
    assert_eq!(
        result,
        vec![f_y, f_z],
        "faces with normals ⟂ X (i.e. +Y, +Z) survive; +X face is dropped"
    );
}

#[test]
fn faces_perpendicular_to_is_sign_tolerant() {
    // A face with normal -X is "parallel to X axis" (anti-parallel) and
    // therefore NOT perpendicular. The selector treats ±axis equivalently —
    // both contribute to the "parallel" side and are excluded.
    let parent = GeometryHandleId(1);
    let f_neg_x = GeometryHandleId(2);
    let f_y = GeometryHandleId(3);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f_neg_x, f_y])
        .with_face_normal_result(
            f_neg_x,
            Value::String("{\"x\":-1.0,\"y\":0.0,\"z\":0.0}".into()),
        )
        .with_face_normal_result(f_y, Value::String("{\"x\":0.0,\"y\":1.0,\"z\":0.0}".into()));

    let result = faces_perpendicular_to(&mut kernel, parent, [1.0, 0.0, 0.0], 1.0_f64.to_radians())
        .expect("faces_perpendicular_to should succeed");
    assert_eq!(
        result,
        vec![f_y],
        "anti-parallel face (-X) is parallel to X (sign-tolerant); only ⟂ face survives"
    );
}

#[test]
fn faces_perpendicular_to_zero_axis_returns_query_failed() {
    let parent = GeometryHandleId(1);
    let mut kernel = MockGeometryKernel::new();
    let result = faces_perpendicular_to(&mut kernel, parent, [0.0, 0.0, 0.0], 0.1);
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

fn assert_faces_perpendicular_to_tol_rejected(tol: f64) {
    let parent = GeometryHandleId(1);
    let mut kernel = MockGeometryKernel::new();
    let result = faces_perpendicular_to(&mut kernel, parent, [1.0, 0.0, 0.0], tol);
    match result {
        Err(QueryError::QueryFailed(msg)) => assert!(
            msg.contains("angular_tol_rad"),
            "error should mention 'angular_tol_rad', got: {msg:?}"
        ),
        other => panic!(
            "expected Err(QueryFailed) for tol {:?}, got {:?}",
            tol, other
        ),
    }
}

#[test]
fn faces_perpendicular_to_negative_tol_returns_query_failed() {
    assert_faces_perpendicular_to_tol_rejected(-0.1);
}

#[test]
fn faces_perpendicular_to_tol_above_half_pi_returns_query_failed() {
    assert_faces_perpendicular_to_tol_rejected(std::f64::consts::FRAC_PI_2 + 1e-3);
}

#[test]
fn faces_perpendicular_to_nan_tol_returns_query_failed() {
    assert_faces_perpendicular_to_tol_rejected(f64::NAN);
}

fn assert_faces_perpendicular_to_tol_accepted_at_boundaries() {
    let parent = GeometryHandleId(1);
    let face = GeometryHandleId(2);

    // Lower bound — tol=0.0: a face with normal exactly perpendicular to the axis has
    // |dot(n, axis)| = 0 = sin(0), satisfying `|dot| <= 0` (tests `<=` not `<`).
    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![face])
        .with_face_normal_result(
            face,
            Value::String("{\"x\":0.0,\"y\":1.0,\"z\":0.0}".into()),
        );
    let result =
        faces_perpendicular_to(&mut kernel, parent, [1.0, 0.0, 0.0], 0.0).unwrap_or_else(|e| {
            panic!(
                "faces_perpendicular_to must accept tol=0.0 and include an exactly-perpendicular \
                 face, got Err: {e:?}"
            )
        });
    assert_eq!(
        result,
        vec![face],
        "face with |dot(n,axis)|=0=sin(0) must be included at tol=0 (inclusive lower bound)"
    );

    // Upper bound — tol=π/2: a face with normal exactly parallel to the axis has
    // |dot(n, axis)| = 1 = sin(π/2), satisfying `|dot| <= 1` (tests the inclusive
    // upper bound: `<=` not `<`; sin(π/2) = 1.0 exactly in f64).
    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![face])
        .with_face_normal_result(
            face,
            Value::String("{\"x\":1.0,\"y\":0.0,\"z\":0.0}".into()),
        );
    let result = faces_perpendicular_to(
        &mut kernel,
        parent,
        [1.0, 0.0, 0.0],
        std::f64::consts::FRAC_PI_2,
    )
    .unwrap_or_else(|e| {
        panic!(
            "faces_perpendicular_to must accept tol=π/2 and include a face parallel to axis, \
             got Err: {e:?}"
        )
    });
    assert_eq!(
        result,
        vec![face],
        "face with |dot(n,axis)|=1=sin(π/2) must be included at tol=π/2 (inclusive upper bound)"
    );
}

#[test]
fn faces_perpendicular_to_inclusive_boundaries_zero_and_half_pi_are_accepted() {
    assert_faces_perpendicular_to_tol_accepted_at_boundaries();
}

#[test]
fn faces_perpendicular_to_degenerate_normal_returns_query_failed() {
    // A face that reports a zero normal (degenerate face) must surface a
    // QueryFailed rather than slipping through with NaN-poisoned arithmetic.
    let parent = GeometryHandleId(1);
    let f = GeometryHandleId(2);
    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f])
        .with_face_normal_result(f, Value::String("{\"x\":0.0,\"y\":0.0,\"z\":0.0}".into()));
    let result = faces_perpendicular_to(&mut kernel, parent, [1.0, 0.0, 0.0], 0.1);
    assert!(
        matches!(result, Err(QueryError::QueryFailed(_))),
        "degenerate normal must produce QueryFailed, got {:?}",
        result
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// edges_perpendicular_to — `#axis` direction filter for edges (PRD line 76)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn edges_perpendicular_to_keeps_edges_orthogonal_to_axis() {
    // Three edges with tangents +X, +Y, +Z. For axis = +Z the edges with
    // tangents perpendicular to Z (i.e. +X, +Y) survive; the +Z edge is
    // dropped because its tangent is parallel to Z.
    let parent = GeometryHandleId(1);
    let e_x = GeometryHandleId(2);
    let e_y = GeometryHandleId(3);
    let e_z = GeometryHandleId(4);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e_x, e_y, e_z])
        .with_edge_tangent_result(e_x, Value::String("{\"x\":1.0,\"y\":0.0,\"z\":0.0}".into()))
        .with_edge_tangent_result(e_y, Value::String("{\"x\":0.0,\"y\":1.0,\"z\":0.0}".into()))
        .with_edge_tangent_result(e_z, Value::String("{\"x\":0.0,\"y\":0.0,\"z\":1.0}".into()));

    let result = edges_perpendicular_to(&mut kernel, parent, [0.0, 0.0, 1.0], 1.0_f64.to_radians())
        .expect("edges_perpendicular_to should succeed");
    assert_eq!(
        result,
        vec![e_x, e_y],
        "edges with tangents ⟂ Z (i.e. +X, +Y) survive; +Z edge dropped"
    );
}

#[test]
fn edges_perpendicular_to_zero_axis_returns_query_failed() {
    let parent = GeometryHandleId(1);
    let mut kernel = MockGeometryKernel::new();
    let result = edges_perpendicular_to(&mut kernel, parent, [0.0, 0.0, 0.0], 0.1);
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

fn assert_edges_perpendicular_to_tol_rejected(tol: f64) {
    let parent = GeometryHandleId(1);
    let mut kernel = MockGeometryKernel::new();
    let result = edges_perpendicular_to(&mut kernel, parent, [0.0, 0.0, 1.0], tol);
    match result {
        Err(QueryError::QueryFailed(msg)) => assert!(
            msg.contains("angular_tol_rad"),
            "error should mention 'angular_tol_rad', got: {msg:?}"
        ),
        other => panic!(
            "expected Err(QueryFailed) for tol {:?}, got {:?}",
            tol, other
        ),
    }
}

#[test]
fn edges_perpendicular_to_negative_tol_returns_query_failed() {
    assert_edges_perpendicular_to_tol_rejected(-0.1);
}

#[test]
fn edges_perpendicular_to_tol_above_half_pi_returns_query_failed() {
    assert_edges_perpendicular_to_tol_rejected(std::f64::consts::FRAC_PI_2 + 1e-3);
}

#[test]
fn edges_perpendicular_to_nan_tol_returns_query_failed() {
    assert_edges_perpendicular_to_tol_rejected(f64::NAN);
}

fn assert_edges_perpendicular_to_tol_accepted_at_boundaries() {
    let parent = GeometryHandleId(1);
    let edge = GeometryHandleId(2);

    // Lower bound — tol=0.0: a tangent exactly perpendicular to the axis has
    // |dot(t, axis)| = 0 = sin(0), satisfying `|dot| <= 0` (tests `<=` not `<`).
    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![edge])
        .with_edge_tangent_result(
            edge,
            Value::String("{\"x\":1.0,\"y\":0.0,\"z\":0.0}".into()),
        );
    let result =
        edges_perpendicular_to(&mut kernel, parent, [0.0, 0.0, 1.0], 0.0).unwrap_or_else(|e| {
            panic!(
                "edges_perpendicular_to must accept tol=0.0 and include an exactly-perpendicular \
                 edge, got Err: {e:?}"
            )
        });
    assert_eq!(
        result,
        vec![edge],
        "edge with |dot(t,axis)|=0=sin(0) must be included at tol=0 (inclusive lower bound)"
    );

    // Upper bound — tol=π/2: a tangent exactly parallel to the axis has
    // |dot(t, axis)| = 1 = sin(π/2), satisfying `|dot| <= 1` (tests the inclusive
    // upper bound: `<=` not `<`; sin(π/2) = 1.0 exactly in f64).
    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![edge])
        .with_edge_tangent_result(
            edge,
            Value::String("{\"x\":0.0,\"y\":0.0,\"z\":1.0}".into()),
        );
    let result = edges_perpendicular_to(
        &mut kernel,
        parent,
        [0.0, 0.0, 1.0],
        std::f64::consts::FRAC_PI_2,
    )
    .unwrap_or_else(|e| {
        panic!(
            "edges_perpendicular_to must accept tol=π/2 and include an edge parallel to axis, \
             got Err: {e:?}"
        )
    });
    assert_eq!(
        result,
        vec![edge],
        "edge with |dot(t,axis)|=1=sin(π/2) must be included at tol=π/2 (inclusive upper bound)"
    );
}

#[test]
fn edges_perpendicular_to_inclusive_boundaries_zero_and_half_pi_are_accepted() {
    assert_edges_perpendicular_to_tol_accepted_at_boundaries();
}

#[test]
fn edges_perpendicular_to_degenerate_tangent_returns_query_failed() {
    let parent = GeometryHandleId(1);
    let e = GeometryHandleId(2);
    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e])
        .with_edge_tangent_result(e, Value::String("{\"x\":0.0,\"y\":0.0,\"z\":0.0}".into()));
    let result = edges_perpendicular_to(&mut kernel, parent, [0.0, 0.0, 1.0], 0.1);
    assert!(
        matches!(result, Err(QueryError::QueryFailed(_))),
        "degenerate tangent must produce QueryFailed, got {:?}",
        result
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// extremal_by_bbox — `>axis` extremal selector by BoundingBox extents (PRD line 77)
//
// Returns the cluster of candidates whose extent along the requested axis
// (using bbox.max[axis] for `Max` and bbox.min[axis] for `Min`) is within
// `tol_m` of the global extreme. Tie cluster is returned in input order;
// dedup-on-first-seen is preserved.
// ─────────────────────────────────────────────────────────────────────────────

/// Helper to format a bbox JSON payload in the kernel's canonical encoding.
fn bbox_json(min: [f64; 3], max: [f64; 3]) -> Value {
    Value::String(format!(
        "{{\"xmin\":{},\"ymin\":{},\"zmin\":{},\"xmax\":{},\"ymax\":{},\"zmax\":{}}}",
        min[0], min[1], min[2], max[0], max[1], max[2]
    ))
}

#[test]
fn extremal_by_bbox_unique_max_along_y_returns_single_face() {
    // Stage four faces with bbox ymax = 5e-3, 8e-3, 8.0001e-3, 12e-3.
    // The 12e-3 face is the unique global maximum; with a tight tol_m=1e-6
    // it is the only face in the result cluster.
    let f1 = GeometryHandleId(2); // ymax = 5e-3
    let f2 = GeometryHandleId(3); // ymax = 8e-3
    let f3 = GeometryHandleId(4); // ymax = 8.0001e-3
    let f4 = GeometryHandleId(5); // ymax = 12e-3 (unique max)
    let candidates = vec![f1, f2, f3, f4];

    let mut kernel = MockGeometryKernel::new()
        .with_bbox_result(f1, bbox_json([0.0, 0.0, 0.0], [1.0, 0.005, 1.0]))
        .with_bbox_result(f2, bbox_json([0.0, 0.0, 0.0], [1.0, 0.008, 1.0]))
        .with_bbox_result(f3, bbox_json([0.0, 0.0, 0.0], [1.0, 0.0080001, 1.0]))
        .with_bbox_result(f4, bbox_json([0.0, 0.0, 0.0], [1.0, 0.012, 1.0]));

    let result = extremal_by_bbox(&mut kernel, &candidates, Axis::Y, ExtremalSense::Max, 1e-6)
        .expect("extremal_by_bbox should succeed for unique max");
    assert_eq!(
        result,
        vec![f4],
        "unique max along Y (tol=1e-6) should be only the 12e-3 face"
    );
}

#[test]
fn extremal_by_bbox_tied_cluster_returned_in_input_order() {
    // Three faces with ymax = 5e-3, 8e-3, 8.0001e-3. Global max is 8.0001e-3;
    // with tol_m = 1e-3 the cluster around the max captures both 8e-3 and
    // 8.0001e-3 (their absolute difference is 1e-7 ≪ tol). The cluster is
    // returned in input order.
    let f1 = GeometryHandleId(2); // ymax = 5e-3
    let f2 = GeometryHandleId(3); // ymax = 8e-3
    let f3 = GeometryHandleId(4); // ymax = 8.0001e-3 (global max)
    let candidates = vec![f1, f2, f3];

    let mut kernel = MockGeometryKernel::new()
        .with_bbox_result(f1, bbox_json([0.0, 0.0, 0.0], [1.0, 0.005, 1.0]))
        .with_bbox_result(f2, bbox_json([0.0, 0.0, 0.0], [1.0, 0.008, 1.0]))
        .with_bbox_result(f3, bbox_json([0.0, 0.0, 0.0], [1.0, 0.0080001, 1.0]));

    let result = extremal_by_bbox(&mut kernel, &candidates, Axis::Y, ExtremalSense::Max, 1e-3)
        .expect("extremal_by_bbox should succeed for tied cluster");
    assert_eq!(
        result,
        vec![f2, f3],
        "tied cluster around max along Y (tol=1e-3) returned in input order"
    );
}

#[test]
fn extremal_by_bbox_min_sense_uses_bbox_min_for_axis() {
    // Three faces with ymin = 0.0, 5e-3, 1e-2. Min sense → global min is 0.0;
    // with tol = 1e-6 the result is the unique f1.
    let f1 = GeometryHandleId(2);
    let f2 = GeometryHandleId(3);
    let f3 = GeometryHandleId(4);
    let candidates = vec![f1, f2, f3];

    let mut kernel = MockGeometryKernel::new()
        .with_bbox_result(f1, bbox_json([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]))
        .with_bbox_result(f2, bbox_json([0.0, 0.005, 0.0], [1.0, 1.0, 1.0]))
        .with_bbox_result(f3, bbox_json([0.0, 0.01, 0.0], [1.0, 1.0, 1.0]));

    let result = extremal_by_bbox(&mut kernel, &candidates, Axis::Y, ExtremalSense::Min, 1e-6)
        .expect("extremal_by_bbox should succeed for min sense");
    assert_eq!(
        result,
        vec![f1],
        "Min sense along Y picks the candidate with the smallest ymin"
    );
}

#[test]
fn extremal_by_bbox_axis_x_picks_along_x_only() {
    // Two faces with the same ymax/zmax but different xmax — only the
    // xmax differentiates. Confirms axis selection is wired correctly.
    let f1 = GeometryHandleId(2);
    let f2 = GeometryHandleId(3);
    let candidates = vec![f1, f2];

    let mut kernel = MockGeometryKernel::new()
        .with_bbox_result(f1, bbox_json([0.0, 0.0, 0.0], [0.003, 1.0, 1.0]))
        .with_bbox_result(f2, bbox_json([0.0, 0.0, 0.0], [0.009, 1.0, 1.0]));

    let result = extremal_by_bbox(&mut kernel, &candidates, Axis::X, ExtremalSense::Max, 1e-6)
        .expect("extremal_by_bbox should succeed along X");
    assert_eq!(
        result,
        vec![f2],
        "Max along X picks the candidate with xmax=0.009"
    );
}

#[test]
fn extremal_by_bbox_axis_z_picks_along_z_only() {
    // Confirms Axis::Z routes through bbox zmin/zmax (the existing axis
    // already covered by parse_bbox_z_extents, but exercised end-to-end here
    // through the new generalised parse_bbox_axis_extents helper).
    let f1 = GeometryHandleId(2);
    let f2 = GeometryHandleId(3);
    let candidates = vec![f1, f2];

    let mut kernel = MockGeometryKernel::new()
        .with_bbox_result(f1, bbox_json([0.0, 0.0, 0.0], [1.0, 1.0, 0.002]))
        .with_bbox_result(f2, bbox_json([0.0, 0.0, 0.0], [1.0, 1.0, 0.011]));

    let result = extremal_by_bbox(&mut kernel, &candidates, Axis::Z, ExtremalSense::Max, 1e-6)
        .expect("extremal_by_bbox should succeed along Z");
    assert_eq!(
        result,
        vec![f2],
        "Max along Z picks the candidate with zmax=0.011"
    );
}

#[test]
fn extremal_by_bbox_empty_candidates_returns_empty() {
    let mut kernel = MockGeometryKernel::new();
    let candidates: Vec<GeometryHandleId> = vec![];
    let result = extremal_by_bbox(&mut kernel, &candidates, Axis::Z, ExtremalSense::Max, 1e-6)
        .expect("extremal_by_bbox on empty candidates should succeed");
    assert!(
        result.is_empty(),
        "empty candidate slice yields empty cluster"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// extremal_by_centroid — `>>axis` extremal selector by Centroid (PRD line 77)
//
// Centroid-based counterpart of `extremal_by_bbox`. Differs from the
// bbox version on non-flat faces: the centroid of a curved face can lie
// inside the bbox interior even though the bbox extent reaches further.
// PRD lines 76-77 list `>` (by-bounds) and `>>` (by-center) as distinct
// selectors precisely to cover this divergence.
// ─────────────────────────────────────────────────────────────────────────────

/// Helper to format a centroid JSON payload in the kernel's canonical
/// `{"x":..,"y":..,"z":..}` encoding.
fn xyz_json(p: [f64; 3]) -> Value {
    Value::String(format!("{{\"x\":{},\"y\":{},\"z\":{}}}", p[0], p[1], p[2]))
}

#[test]
fn extremal_by_centroid_unique_max_along_y_returns_single_face() {
    // Stage four faces with centroid Y values 0.0, 5e-3, 5e-3, 10e-3.
    // With Axis::Y / Max / tol=1e-6 the unique top is the 10e-3 face.
    let f1 = GeometryHandleId(2);
    let f2 = GeometryHandleId(3);
    let f3 = GeometryHandleId(4);
    let f4 = GeometryHandleId(5);
    let candidates = vec![f1, f2, f3, f4];

    let mut kernel = MockGeometryKernel::new()
        .with_centroid_result(f1, xyz_json([0.0, 0.0, 0.0]))
        .with_centroid_result(f2, xyz_json([0.0, 0.005, 0.0]))
        .with_centroid_result(f3, xyz_json([0.0, 0.005, 0.0]))
        .with_centroid_result(f4, xyz_json([0.0, 0.010, 0.0]));

    let result = extremal_by_centroid(&mut kernel, &candidates, Axis::Y, ExtremalSense::Max, 1e-6)
        .expect("extremal_by_centroid should succeed for unique max");
    assert_eq!(
        result,
        vec![f4],
        "unique centroid-y max (tol=1e-6) is the 10e-3 face"
    );
}

#[test]
fn extremal_by_centroid_min_sense_returns_tied_cluster_in_input_order() {
    // Fixture: four faces with centroid Y values 0.0, 5e-4, 5e-4, 10e-3.
    // With Min sense and tol=1e-3 the cluster around the global min (0.0)
    // captures three distinct handles (f1, f2, f3) whose centroid Y is
    // within 1e-3 of 0.0; f4 at 10e-3 is excluded (|0.010 - 0| > 1e-3).
    // Result is in input order — f2 and f3 are distinct handles with the
    // same centroid value, so dedup-on-first-seen does NOT collapse them.
    let f1 = GeometryHandleId(2);
    let f2 = GeometryHandleId(3);
    let f3 = GeometryHandleId(4);
    let f4 = GeometryHandleId(5);
    let candidates = vec![f1, f2, f3, f4];

    let mut kernel = MockGeometryKernel::new()
        .with_centroid_result(f1, xyz_json([0.0, 0.0, 0.0]))
        .with_centroid_result(f2, xyz_json([0.0, 0.0005, 0.0]))
        .with_centroid_result(f3, xyz_json([0.0, 0.0005, 0.0]))
        .with_centroid_result(f4, xyz_json([0.0, 0.010, 0.0]));

    let result = extremal_by_centroid(&mut kernel, &candidates, Axis::Y, ExtremalSense::Min, 1e-3)
        .expect("extremal_by_centroid should succeed for tie cluster");
    assert_eq!(
        result,
        vec![f1, f2, f3],
        "Min cluster (tol=1e-3 around 0.0) captures the three near-zero faces in input order"
    );
}

#[test]
fn extremal_by_centroid_distinguishes_by_axis() {
    // Two faces whose centroids tie on Y but differ on X — Axis::X must
    // pick the larger-X face; Axis::Y must return both as a tie cluster.
    let f1 = GeometryHandleId(2);
    let f2 = GeometryHandleId(3);
    let candidates = vec![f1, f2];

    let mut kernel = MockGeometryKernel::new()
        .with_centroid_result(f1, xyz_json([0.0, 0.005, 0.0]))
        .with_centroid_result(f2, xyz_json([0.010, 0.005, 0.0]));

    let max_x = extremal_by_centroid(&mut kernel, &candidates, Axis::X, ExtremalSense::Max, 1e-6)
        .expect("extremal_by_centroid X/Max should succeed");
    assert_eq!(max_x, vec![f2], "Max along X picks the f2 (x=0.010)");

    let max_y = extremal_by_centroid(&mut kernel, &candidates, Axis::Y, ExtremalSense::Max, 1e-6)
        .expect("extremal_by_centroid Y/Max should succeed");
    assert_eq!(
        max_y,
        vec![f1, f2],
        "tied Y centroids both qualify as the global max in input order"
    );
}

#[test]
fn extremal_by_centroid_empty_candidates_returns_empty() {
    let mut kernel = MockGeometryKernel::new();
    let candidates: Vec<GeometryHandleId> = vec![];
    let result = extremal_by_centroid(&mut kernel, &candidates, Axis::Z, ExtremalSense::Max, 1e-6)
        .expect("extremal_by_centroid on empty candidates should succeed");
    assert!(
        result.is_empty(),
        "empty candidate slice yields empty cluster"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// faces_by_surface_kind / edges_by_curve_kind / geom_universal — geometry-type
// filters (PRD line 78). The `%Plane`/`%Cylinder`/etc. slot is implemented by
// dispatching the new GeometryQuery::FaceSurfaceKind / EdgeCurveKind variants
// (added in step-14) and parsing the canonical name string back into the typed
// enum. `%Geom` is the identity passthrough — any handle that is `Geometry` is
// retained, so the function clones the input slice unchanged.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn faces_by_surface_kind_keeps_only_matching_kind() {
    // Three faces classified as Plane / Cylinder / Sphere; filtering on
    // Plane retains only the planar face (the canonical box top-face case).
    let parent = GeometryHandleId(1);
    let f_plane = GeometryHandleId(2);
    let f_cyl = GeometryHandleId(3);
    let f_sph = GeometryHandleId(4);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f_plane, f_cyl, f_sph])
        .with_face_surface_kind_result(f_plane, Value::String("Plane".into()))
        .with_face_surface_kind_result(f_cyl, Value::String("Cylinder".into()))
        .with_face_surface_kind_result(f_sph, Value::String("Sphere".into()));

    let result = faces_by_surface_kind(&mut kernel, parent, FaceSurfaceKind::Plane)
        .expect("faces_by_surface_kind should succeed");
    assert_eq!(
        result,
        vec![f_plane],
        "Plane filter retains only the planar face"
    );
}

#[test]
fn faces_by_surface_kind_returns_all_when_all_match() {
    // Two planar faces — both should survive the Plane filter, in
    // input order.
    let parent = GeometryHandleId(1);
    let f1 = GeometryHandleId(2);
    let f2 = GeometryHandleId(3);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f1, f2])
        .with_face_surface_kind_result(f1, Value::String("Plane".into()))
        .with_face_surface_kind_result(f2, Value::String("Plane".into()));

    let result = faces_by_surface_kind(&mut kernel, parent, FaceSurfaceKind::Plane)
        .expect("faces_by_surface_kind should succeed");
    assert_eq!(result, vec![f1, f2]);
}

#[test]
fn faces_by_surface_kind_returns_empty_when_no_match() {
    // No planar face — the Plane filter returns an empty slice.
    let parent = GeometryHandleId(1);
    let f_cyl = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f_cyl])
        .with_face_surface_kind_result(f_cyl, Value::String("Cylinder".into()));

    let result = faces_by_surface_kind(&mut kernel, parent, FaceSurfaceKind::Plane)
        .expect("faces_by_surface_kind should succeed");
    assert!(result.is_empty());
}

#[test]
fn faces_by_surface_kind_unknown_kind_string_returns_query_failed() {
    // The canonical name list is bounded by OCCT's GeomAbs_* enum; an
    // unrecognised string from a misbehaving kernel must surface as
    // QueryFailed (decode-side defence-in-depth).
    let parent = GeometryHandleId(1);
    let f = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f])
        .with_face_surface_kind_result(f, Value::String("NotAKind".into()));

    let result = faces_by_surface_kind(&mut kernel, parent, FaceSurfaceKind::Plane);
    assert!(
        matches!(result, Err(QueryError::QueryFailed(_))),
        "unknown kind name must surface QueryFailed, got {:?}",
        result
    );
}

#[test]
fn edges_by_curve_kind_keeps_only_matching_kind() {
    // Three edges classified as Line / Circle / BSplineCurve; filtering on
    // Line retains only the linear edge.
    let parent = GeometryHandleId(1);
    let e_line = GeometryHandleId(2);
    let e_circ = GeometryHandleId(3);
    let e_bspl = GeometryHandleId(4);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e_line, e_circ, e_bspl])
        .with_edge_curve_kind_result(e_line, Value::String("Line".into()))
        .with_edge_curve_kind_result(e_circ, Value::String("Circle".into()))
        .with_edge_curve_kind_result(e_bspl, Value::String("BSplineCurve".into()));

    let result = edges_by_curve_kind(&mut kernel, parent, EdgeCurveKind::Line)
        .expect("edges_by_curve_kind should succeed");
    assert_eq!(
        result,
        vec![e_line],
        "Line filter retains only the linear edge"
    );
}

#[test]
fn edges_by_curve_kind_handles_circle_filter() {
    let parent = GeometryHandleId(1);
    let e_line = GeometryHandleId(2);
    let e_circ = GeometryHandleId(3);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e_line, e_circ])
        .with_edge_curve_kind_result(e_line, Value::String("Line".into()))
        .with_edge_curve_kind_result(e_circ, Value::String("Circle".into()));

    let result = edges_by_curve_kind(&mut kernel, parent, EdgeCurveKind::Circle)
        .expect("edges_by_curve_kind should succeed");
    assert_eq!(result, vec![e_circ]);
}

// ─────────────────────────────────────────────────────────────────────────────
// geom_universal — `%Geom` no-op identity (PRD line 78)
//
// The universal filter retains every handle (every Geometry trivially
// satisfies the `kind == Geom` predicate). It must not call into the
// kernel, must not dedupe, and must preserve input order verbatim — it's
// a syntactic identity for chain composition.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn geom_universal_returns_input_slice_unchanged() {
    // Universal filter is the identity — order, content, and length all
    // preserved. The duplicates here exercise that geom_universal is NOT a
    // dedup combinator (unlike intersect/union/complement).
    let handles = vec![
        GeometryHandleId(10),
        GeometryHandleId(20),
        GeometryHandleId(20), // duplicate is preserved
        GeometryHandleId(30),
    ];
    let result = geom_universal(&handles);
    assert_eq!(
        result, handles,
        "geom_universal must return the input slice unchanged (no dedup, no reorder)"
    );
}

#[test]
fn geom_universal_empty_input_returns_empty() {
    let handles: Vec<GeometryHandleId> = vec![];
    assert!(geom_universal(&handles).is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// created_by_feature — `qCreatedBy(feature_id)` history selector (PRD line 80)
//
// Returns the candidates whose `attr.feature_id == feature_id` — the
// origin-feature of the topology entity. Pure-Rust; consults a
// `TopologyAttributeTable` rather than the kernel.
// ─────────────────────────────────────────────────────────────────────────────

/// Build a fixture with three handles:
/// - A (id=10) produced by F1, no mod_history
/// - B (id=20) produced by F2, mod_history = [F3 split=0]
/// - C (id=30) produced by F2, mod_history = [F3 split=1, F4 split=0]
fn fixture_history_table() -> (
    TopologyAttributeTable,
    GeometryHandleId,
    GeometryHandleId,
    GeometryHandleId,
    FeatureId,
    FeatureId,
    FeatureId,
    FeatureId,
) {
    let f1 = FeatureId::new("F1");
    let f2 = FeatureId::new("F2");
    let f3 = FeatureId::new("F3");
    let f4 = FeatureId::new("F4");

    let a = GeometryHandleId(10);
    let b = GeometryHandleId(20);
    let c = GeometryHandleId(30);

    let mut table = TopologyAttributeTable::default();
    table.record(
        a,
        TopologyAttribute {
            feature_id: f1.clone(),
            role: Role::Cap(CapKind::Top),
            local_index: 0,
            user_label: None,
            mod_history: Vec::new(),
        },
    );
    table.record(
        b,
        TopologyAttribute {
            feature_id: f2.clone(),
            role: Role::Side,
            local_index: 0,
            user_label: None,
            mod_history: vec![ModEntry {
                splitting_feature_id: f3.clone(),
                split_index: 0,
            }],
        },
    );
    table.record(
        c,
        TopologyAttribute {
            feature_id: f2.clone(),
            role: Role::Side,
            local_index: 1,
            user_label: None,
            mod_history: vec![
                ModEntry {
                    splitting_feature_id: f3.clone(),
                    split_index: 1,
                },
                ModEntry {
                    splitting_feature_id: f4.clone(),
                    split_index: 0,
                },
            ],
        },
    );
    (table, a, b, c, f1, f2, f3, f4)
}

#[test]
fn created_by_feature_returns_handles_whose_origin_feature_matches() {
    let (table, a, b, c, f1, f2, _f3, _f4) = fixture_history_table();
    let candidates = vec![a, b, c];

    // F1 produced only A.
    assert_eq!(
        created_by_feature(&table, &candidates, &f1),
        vec![a],
        "created_by_feature(F1) must return only handles whose origin is F1"
    );
    // F2 produced both B and C — order follows the candidate slice.
    assert_eq!(
        created_by_feature(&table, &candidates, &f2),
        vec![b, c],
        "created_by_feature(F2) must return [B, C] in candidate order"
    );
}

#[test]
fn created_by_feature_dedupes_duplicate_candidates() {
    let (table, a, b, c, _f1, f2, _f3, _f4) = fixture_history_table();
    // Duplicate B in the candidate list — dedup must drop the second copy.
    let candidates = vec![b, c, b, b];

    assert_eq!(
        created_by_feature(&table, &candidates, &f2),
        vec![b, c],
        "duplicate candidates must dedupe on first-seen"
    );
    // Sanity: A is not in the candidate slice, so requesting F1 returns empty.
    let f1 = FeatureId::new("F1");
    let _ = a;
    let candidates_no_a = vec![b, c];
    assert!(
        created_by_feature(&table, &candidates_no_a, &f1).is_empty(),
        "candidates not produced by F1 must yield empty"
    );
}

#[test]
fn created_by_feature_unknown_feature_returns_empty() {
    let (table, a, b, c, _f1, _f2, _f3, _f4) = fixture_history_table();
    let candidates = vec![a, b, c];

    let f99 = FeatureId::new("F99-never-existed");
    assert!(
        created_by_feature(&table, &candidates, &f99).is_empty(),
        "unknown feature id must yield empty"
    );
}

#[test]
fn created_by_feature_handles_missing_table_entries() {
    // A handle with no entry in the table simply does not match — should
    // not panic, should not appear in the result.
    let table = TopologyAttributeTable::default();
    let h = GeometryHandleId(42);
    let f = FeatureId::new("F1");
    assert!(
        created_by_feature(&table, &[h], &f).is_empty(),
        "missing table entry must yield empty result, not panic"
    );
}

#[test]
fn created_by_feature_empty_candidates_returns_empty() {
    let (table, _a, _b, _c, f1, _f2, _f3, _f4) = fixture_history_table();
    let candidates: Vec<GeometryHandleId> = vec![];
    assert!(
        created_by_feature(&table, &candidates, &f1).is_empty(),
        "empty candidate slice must yield empty"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// split_by_feature — `qSplitBy(feature_id)` history selector (PRD line 80)
//
// Returns the candidates whose `mod_history` contains an entry naming
// `feature_id` as the splitting feature, **at any position** (not just the
// most recent). Aligns with OnShape's `qSplitBy` semantics: a topology
// entity that was split-by-F3-then-split-by-F4 should still match
// `split_by_feature(F3)` because F3 is part of its lineage.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn split_by_feature_matches_any_position_in_mod_history() {
    let (table, a, b, c, _f1, _f2, f3, f4) = fixture_history_table();
    let candidates = vec![a, b, c];

    // F3 split both B and C (B's only entry; C's first entry).
    assert_eq!(
        split_by_feature(&table, &candidates, &f3),
        vec![b, c],
        "split_by_feature(F3) matches any-position in mod_history"
    );
    // F4 split only C (its second mod_history entry).
    assert_eq!(
        split_by_feature(&table, &candidates, &f4),
        vec![c],
        "split_by_feature(F4) matches the deeper mod_history entry"
    );
}

#[test]
fn split_by_feature_unmatched_feature_returns_empty() {
    let (table, a, b, c, f1, _f2, _f3, _f4) = fixture_history_table();
    let candidates = vec![a, b, c];

    // F1 is the origin of A but never appears as a splitting feature in
    // any mod_history — split_by_feature(F1) must be empty.
    assert!(
        split_by_feature(&table, &candidates, &f1).is_empty(),
        "F1 never splits, so split_by_feature(F1) is empty"
    );
}

#[test]
fn split_by_feature_handle_with_empty_mod_history_does_not_match() {
    let (table, a, _b, _c, _f1, _f2, f3, _f4) = fixture_history_table();
    // A has no mod_history — split_by_feature(F3) over [A] alone is empty.
    assert!(
        split_by_feature(&table, &[a], &f3).is_empty(),
        "handle with empty mod_history must not match any split_by query"
    );
}

#[test]
fn split_by_feature_dedupes_duplicate_candidates() {
    let (table, _a, b, c, _f1, _f2, f3, _f4) = fixture_history_table();
    let candidates = vec![b, c, b, b, c];
    assert_eq!(
        split_by_feature(&table, &candidates, &f3),
        vec![b, c],
        "duplicate candidates must dedupe on first-seen"
    );
}

#[test]
fn split_by_feature_empty_table_returns_empty() {
    let table = TopologyAttributeTable::default();
    let f = FeatureId::new("F1");
    assert!(
        split_by_feature(&table, &[GeometryHandleId(1)], &f).is_empty(),
        "empty table must yield empty result"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// has_user_label / user_label_eq — attribute primitives (PRD line 82)
//
// `has_user_label(table, candidates)` retains handles whose attribute carries
// a `user_label = Some(_)` (i.e. the user has assigned a stable name).
// `user_label_eq(table, candidates, label)` retains handles whose attribute
// carries `user_label = Some(label)` exactly. Both are pure-Rust readers
// over `TopologyAttributeTable`; they implement the `has_attribute(key)` /
// `attribute_eq(key, value)` PRD slots where the only currently-supported
// "key" in v0.2 is `user_label` (other attribute fields like `feature_id`,
// `role`, `local_index`, `mod_history` are positional and addressed by the
// named selectors above).
// ─────────────────────────────────────────────────────────────────────────────

/// Build a fixture with three handles:
/// - A (id=10) `user_label = None`
/// - B (id=20) `user_label = Some("top")`
/// - C (id=30) `user_label = Some("bottom")`
fn fixture_label_table() -> (
    TopologyAttributeTable,
    GeometryHandleId,
    GeometryHandleId,
    GeometryHandleId,
) {
    let f1 = FeatureId::new("F-label");
    let a = GeometryHandleId(10);
    let b = GeometryHandleId(20);
    let c = GeometryHandleId(30);

    let mut table = TopologyAttributeTable::default();
    table.record(
        a,
        TopologyAttribute {
            feature_id: f1.clone(),
            role: Role::Side,
            local_index: 0,
            user_label: None,
            mod_history: Vec::new(),
        },
    );
    table.record(
        b,
        TopologyAttribute {
            feature_id: f1.clone(),
            role: Role::Cap(CapKind::Top),
            local_index: 0,
            user_label: Some("top".into()),
            mod_history: Vec::new(),
        },
    );
    table.record(
        c,
        TopologyAttribute {
            feature_id: f1.clone(),
            role: Role::Cap(CapKind::Bottom),
            local_index: 0,
            user_label: Some("bottom".into()),
            mod_history: Vec::new(),
        },
    );
    (table, a, b, c)
}

#[test]
fn has_user_label_returns_only_handles_with_some_label() {
    let (table, a, b, c) = fixture_label_table();
    let candidates = vec![a, b, c];
    assert_eq!(
        has_user_label(&table, &candidates),
        vec![b, c],
        "has_user_label retains only handles whose user_label is Some(_)"
    );
}

#[test]
fn has_user_label_dedupes_duplicate_candidates() {
    let (table, a, b, c) = fixture_label_table();
    let candidates = vec![b, b, c, b];
    assert_eq!(
        has_user_label(&table, &candidates),
        vec![b, c],
        "duplicate candidates must dedupe on first-seen"
    );
    let _ = a;
}

#[test]
fn has_user_label_handle_with_no_table_entry_does_not_match() {
    // A handle missing from the table cannot have a user_label by
    // construction — it must not appear in the result.
    let (table, _a, _b, _c) = fixture_label_table();
    let unknown = GeometryHandleId(999);
    assert!(
        has_user_label(&table, &[unknown]).is_empty(),
        "missing table entry must yield empty result"
    );
}

#[test]
fn has_user_label_empty_candidates_returns_empty() {
    let (table, _a, _b, _c) = fixture_label_table();
    let candidates: Vec<GeometryHandleId> = vec![];
    assert!(has_user_label(&table, &candidates).is_empty());
}

#[test]
fn user_label_eq_returns_only_exact_match() {
    let (table, a, b, c) = fixture_label_table();
    let candidates = vec![a, b, c];

    // Exact match → only B.
    assert_eq!(
        user_label_eq(&table, &candidates, "top"),
        vec![b],
        "user_label_eq(\"top\") must return only B"
    );
    // Different exact match → only C.
    assert_eq!(
        user_label_eq(&table, &candidates, "bottom"),
        vec![c],
        "user_label_eq(\"bottom\") must return only C"
    );
}

#[test]
fn user_label_eq_unknown_label_returns_empty() {
    let (table, a, b, c) = fixture_label_table();
    let candidates = vec![a, b, c];
    assert!(
        user_label_eq(&table, &candidates, "nope").is_empty(),
        "an unrecognised label must yield empty"
    );
}

#[test]
fn user_label_eq_is_case_sensitive() {
    // The user_label is stored as-typed; equality is exact (no
    // case-folding). "Top" must NOT match the entry storing "top".
    let (table, a, b, c) = fixture_label_table();
    let candidates = vec![a, b, c];
    assert!(
        user_label_eq(&table, &candidates, "Top").is_empty(),
        "user_label_eq must be case-sensitive"
    );
}

#[test]
fn user_label_eq_dedupes_duplicate_candidates() {
    let (table, _a, b, _c) = fixture_label_table();
    let candidates = vec![b, b, b];
    assert_eq!(
        user_label_eq(&table, &candidates, "top"),
        vec![b],
        "duplicate candidates must dedupe on first-seen"
    );
}

#[test]
fn user_label_eq_handle_without_label_does_not_match() {
    // A handle whose attribute has user_label = None must not match any
    // user_label_eq query, regardless of the requested label.
    let (table, a, _b, _c) = fixture_label_table();
    assert!(
        user_label_eq(&table, &[a], "top").is_empty(),
        "user_label = None must not match any non-empty query"
    );
    assert!(
        user_label_eq(&table, &[a], "").is_empty(),
        "even an empty-string query must not match user_label = None"
    );
}

#[test]
fn user_label_eq_empty_candidates_returns_empty() {
    let (table, _a, _b, _c) = fixture_label_table();
    let candidates: Vec<GeometryHandleId> = vec![];
    assert!(user_label_eq(&table, &candidates, "top").is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// adjacent_to_face — `adjacent_to(face)` topological walk (PRD line 81)
//
// Composes `extract_faces(parent)` (to recover the canonical face list and
// thereby map a face_handle → 0-based index) with `GeometryQuery::AdjacentFaces`
// (which returns a `Value::List(Vec<Value::Int>)` of global face indices).
// The selector maps the index reply back through the canonical face list to
// surface `Vec<GeometryHandleId>` results.
//
// Errors: passing a `face_handle` that is not in `extract_faces(parent)`
// must surface `QueryError::QueryFailed` with "not a child of parent" in
// the message — the selector cannot map a foreign handle into the kernel's
// 0-based face index space without a re-extraction.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn adjacent_to_face_returns_neighbours_in_kernel_order() {
    // Box has 6 faces; face 0 is adjacent to faces 1, 2, 3, 4 (the four
    // side faces of the canonical TopExp_Explorer order).
    let parent = GeometryHandleId(1);
    let f0 = GeometryHandleId(2);
    let f1 = GeometryHandleId(3);
    let f2 = GeometryHandleId(4);
    let f3 = GeometryHandleId(5);
    let f4 = GeometryHandleId(6);
    let f5 = GeometryHandleId(7);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f0, f1, f2, f3, f4, f5])
        .with_adjacent_faces_result(
            parent,
            0,
            Value::List(vec![
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Int(4),
            ]),
        );

    let result = adjacent_to_face(&mut kernel, parent, f0)
        .expect("adjacent_to_face should succeed for a child face");
    assert_eq!(
        result,
        vec![f1, f2, f3, f4],
        "adjacent face handles must be returned in kernel index order"
    );
}

#[test]
fn adjacent_to_face_with_non_child_handle_returns_query_failed() {
    // `foreign` is not in `extract_faces(parent)` — the selector cannot
    // map it into a 0-based face index, so it must error rather than
    // silently returning an empty result (which would mask the bug).
    let parent = GeometryHandleId(1);
    let f0 = GeometryHandleId(2);
    let foreign = GeometryHandleId(99);

    let mut kernel = MockGeometryKernel::new().with_extracted_faces(parent, vec![f0]);

    let result = adjacent_to_face(&mut kernel, parent, foreign);
    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("not a child of parent"),
                "error must mention 'not a child of parent', got: {msg:?}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed) for non-child face, got {:?}",
            other
        ),
    }
}

#[test]
fn adjacent_to_face_with_no_neighbours_returns_empty() {
    // A degenerate box-with-one-face: the AdjacentFaces query returns an
    // empty list, and the selector surfaces that as an empty Vec.
    let parent = GeometryHandleId(1);
    let f0 = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f0])
        .with_adjacent_faces_result(parent, 0, Value::List(vec![]));

    let result = adjacent_to_face(&mut kernel, parent, f0)
        .expect("adjacent_to_face should succeed for an empty neighbour list");
    assert!(result.is_empty());
}

#[test]
fn adjacent_to_face_propagates_extract_faces_error() {
    // `extract_faces` errors must propagate rather than producing a panic
    // or a misleading "not a child of parent" message.
    let parent = GeometryHandleId(1);
    let f0 = GeometryHandleId(2);
    let mut kernel = MockGeometryKernel::new();
    // No `with_extracted_faces` configured → extract_faces fails.
    let result = adjacent_to_face(&mut kernel, parent, f0);
    assert!(matches!(result, Err(QueryError::QueryFailed(_))));
}

#[test]
fn adjacent_to_face_indexes_face_handle_correctly() {
    // The face_handle is the second face (index 1), and AdjacentFaces is
    // staged for that index. Confirms the selector picks the right
    // face_index for the query, not just always 0.
    let parent = GeometryHandleId(1);
    let f0 = GeometryHandleId(10);
    let f1 = GeometryHandleId(20);
    let f2 = GeometryHandleId(30);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f0, f1, f2])
        .with_adjacent_faces_result(parent, 1, Value::List(vec![Value::Int(0), Value::Int(2)]));

    let result = adjacent_to_face(&mut kernel, parent, f1)
        .expect("adjacent_to_face should succeed for index 1");
    assert_eq!(
        result,
        vec![f0, f2],
        "selector must use the face_handle's own index for the AdjacentFaces query"
    );
}

#[test]
fn adjacent_to_face_returns_query_failed_on_non_list_payload() {
    // The kernel reports a non-List payload (e.g. an integer) — the
    // selector must surface this as QueryFailed rather than panicking on
    // the type mismatch.
    let parent = GeometryHandleId(1);
    let f0 = GeometryHandleId(2);
    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f0])
        .with_adjacent_faces_result(parent, 0, Value::Int(42));
    let result = adjacent_to_face(&mut kernel, parent, f0);
    assert!(matches!(result, Err(QueryError::QueryFailed(_))));
}

#[test]
fn adjacent_to_face_returns_query_failed_on_out_of_range_index() {
    // The kernel returns an index outside the extract_faces array — the
    // selector cannot map it back to a handle, so it errors rather than
    // panicking on a slice index.
    let parent = GeometryHandleId(1);
    let f0 = GeometryHandleId(2);
    let f1 = GeometryHandleId(3);
    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f0, f1])
        .with_adjacent_faces_result(parent, 0, Value::List(vec![Value::Int(99)]));
    let result = adjacent_to_face(&mut kernel, parent, f0);
    assert!(matches!(result, Err(QueryError::QueryFailed(_))));
}

// ─────────────────────────────────────────────────────────────────────────────
// ancestor_faces_of_edge — `ancestors(edge) on faces` topological walk
// (PRD line 81)
//
// Composes `extract_edges(parent)` (to map a caller-supplied edge_handle
// → 0-based edge_index) with `GeometryQuery::AncestorFacesOfEdge` (which
// returns a `Value::List(Vec<Value::Int>)` of global face indices into
// the canonical `extract_faces(parent)` order). The selector maps each
// returned index back through `extract_faces(parent)` to surface
// `Vec<GeometryHandleId>` results.
//
// In a 12-edge box, every edge is shared between exactly two faces; the
// selector must return both face handles in the order the kernel
// reports them.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn ancestor_faces_of_edge_returns_owning_faces_in_kernel_order() {
    // 12-edge / 6-face box: edge 3 belongs to faces 0 and 2 (canonical
    // TopExp_Explorer ordering). The ordering of the returned vec must
    // match the kernel's `Value::List` ordering — we assert
    // `[f0, f2]`, not the sorted set.
    let parent = GeometryHandleId(1);
    let e0 = GeometryHandleId(2);
    let e1 = GeometryHandleId(3);
    let e2 = GeometryHandleId(4);
    let e3 = GeometryHandleId(5);
    let e4 = GeometryHandleId(6);
    let e5 = GeometryHandleId(7);
    let e6 = GeometryHandleId(8);
    let e7 = GeometryHandleId(9);
    let e8 = GeometryHandleId(10);
    let e9 = GeometryHandleId(11);
    let e10 = GeometryHandleId(12);
    let e11 = GeometryHandleId(13);
    let f0 = GeometryHandleId(14);
    let f1 = GeometryHandleId(15);
    let f2 = GeometryHandleId(16);
    let f3 = GeometryHandleId(17);
    let f4 = GeometryHandleId(18);
    let f5 = GeometryHandleId(19);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(
            parent,
            vec![e0, e1, e2, e3, e4, e5, e6, e7, e8, e9, e10, e11],
        )
        .with_extracted_faces(parent, vec![f0, f1, f2, f3, f4, f5])
        .with_ancestor_faces_result(parent, 3, Value::List(vec![Value::Int(0), Value::Int(2)]));

    let result = ancestor_faces_of_edge(&mut kernel, parent, e3)
        .expect("ancestor_faces_of_edge should succeed for a child edge");
    assert_eq!(
        result,
        vec![f0, f2],
        "owning face handles must be returned in kernel index order"
    );
}

#[test]
fn ancestor_faces_of_edge_with_non_child_handle_returns_query_failed() {
    // `foreign` is not in `extract_edges(parent)` — the selector cannot
    // map it into a 0-based edge index, so it must error rather than
    // silently returning an empty result.
    let parent = GeometryHandleId(1);
    let e0 = GeometryHandleId(2);
    let f0 = GeometryHandleId(10);
    let foreign = GeometryHandleId(99);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e0])
        .with_extracted_faces(parent, vec![f0]);

    let result = ancestor_faces_of_edge(&mut kernel, parent, foreign);
    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("not a child of parent"),
                "error must mention 'not a child of parent', got: {msg:?}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed) for non-child edge, got {:?}",
            other
        ),
    }
}

#[test]
fn ancestor_faces_of_edge_returns_query_failed_on_non_list_payload() {
    // The kernel reports a non-List payload — the selector must surface
    // this as QueryFailed rather than panicking on the type mismatch.
    let parent = GeometryHandleId(1);
    let e0 = GeometryHandleId(2);
    let f0 = GeometryHandleId(10);
    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e0])
        .with_extracted_faces(parent, vec![f0])
        .with_ancestor_faces_result(parent, 0, Value::Int(42));
    let result = ancestor_faces_of_edge(&mut kernel, parent, e0);
    assert!(matches!(result, Err(QueryError::QueryFailed(_))));
}

#[test]
fn ancestor_faces_of_edge_returns_query_failed_on_out_of_range_index() {
    // The kernel returns an index outside the extract_faces array — the
    // selector cannot map it back to a face handle, so it errors rather
    // than panicking on a slice index.
    let parent = GeometryHandleId(1);
    let e0 = GeometryHandleId(2);
    let f0 = GeometryHandleId(10);
    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e0])
        .with_extracted_faces(parent, vec![f0])
        .with_ancestor_faces_result(parent, 0, Value::List(vec![Value::Int(99)]));
    let result = ancestor_faces_of_edge(&mut kernel, parent, e0);
    assert!(matches!(result, Err(QueryError::QueryFailed(_))));
}

// ─────────────────────────────────────────────────────────────────────────────
// siblings_of_face — `siblings(face)` topological walk (PRD line 81)
//
// Pure-Rust composition of `extract_faces(parent)` and `except`: returns
// every face of `parent` other than `face_handle`, preserving canonical
// face order. A `face_handle` not in `extract_faces(parent)` errors with
// "not a child of parent" — symmetric with `adjacent_to_face`.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn siblings_of_face_returns_all_other_faces_in_canonical_order() {
    // 6-face box, drop f2 — expect the remaining five in canonical order.
    let parent = GeometryHandleId(1);
    let f0 = GeometryHandleId(2);
    let f1 = GeometryHandleId(3);
    let f2 = GeometryHandleId(4);
    let f3 = GeometryHandleId(5);
    let f4 = GeometryHandleId(6);
    let f5 = GeometryHandleId(7);

    let mut kernel =
        MockGeometryKernel::new().with_extracted_faces(parent, vec![f0, f1, f2, f3, f4, f5]);

    let result = siblings_of_face(&mut kernel, parent, f2)
        .expect("siblings_of_face should succeed for a child face");
    assert_eq!(
        result,
        vec![f0, f1, f3, f4, f5],
        "siblings must contain every other face in canonical order, with f2 omitted"
    );
}

#[test]
fn siblings_of_face_with_non_child_handle_returns_query_failed() {
    let parent = GeometryHandleId(1);
    let f0 = GeometryHandleId(2);
    let foreign = GeometryHandleId(99);

    let mut kernel = MockGeometryKernel::new().with_extracted_faces(parent, vec![f0]);

    let result = siblings_of_face(&mut kernel, parent, foreign);
    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("not a child of parent"),
                "error must mention 'not a child of parent', got: {msg:?}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed) for non-child face, got {:?}",
            other
        ),
    }
}

#[test]
fn siblings_of_face_propagates_extract_faces_error() {
    // No `with_extracted_faces` configured → extract_faces fails. The
    // error must propagate (not be masked by the not-a-child check).
    let parent = GeometryHandleId(1);
    let f0 = GeometryHandleId(2);
    let mut kernel = MockGeometryKernel::new();
    let result = siblings_of_face(&mut kernel, parent, f0);
    assert!(matches!(result, Err(QueryError::QueryFailed(_))));
}

#[test]
fn siblings_of_face_with_single_face_parent_returns_empty() {
    // Degenerate: parent has exactly one face (the queried face itself).
    // siblings_of_face must return an empty vec, not error.
    let parent = GeometryHandleId(1);
    let f0 = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new().with_extracted_faces(parent, vec![f0]);

    let result = siblings_of_face(&mut kernel, parent, f0)
        .expect("siblings_of_face should succeed when the only face is the queried one");
    assert!(result.is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// owner_body_of — `owner_body(sub_handle)` topological walk (PRD line 81)
//
// Issues a single `GeometryQuery::OwnerBody(sub_handle)` query (a pure read —
// the selector takes `&K`, not `&mut K`) and unwraps the `Value::Int(parent_id)`
// reply into a typed `GeometryHandleId`. The kernel records parent provenance
// on every `extract_edges` / `extract_faces` call so any sub-handle can answer
// "what solid did I come from?" without re-extraction.
//
// `MockGeometryKernel::with_owner_body_result(child, parent)` stages the
// mapping; an unstaged child surfaces as `QueryFailed` rather than a panic.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn owner_body_of_returns_recorded_parent_for_staged_handle() {
    let parent = GeometryHandleId(1);
    let child = GeometryHandleId(2);
    let kernel = MockGeometryKernel::new().with_owner_body_result(child, parent);

    let result = owner_body_of(&kernel, child)
        .expect("owner_body_of should succeed for a staged child handle");
    assert_eq!(result, parent, "child must resolve to its recorded parent");
}

#[test]
fn owner_body_of_with_unstaged_handle_returns_query_failed() {
    let foreign = GeometryHandleId(99);
    let kernel = MockGeometryKernel::new();

    let result = owner_body_of(&kernel, foreign);
    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("owner_body") && msg.contains("no recorded parent"),
                "error must mention 'owner_body' and 'no recorded parent', got: {msg:?}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed) for an unstaged handle, got {:?}",
            other
        ),
    }
}

#[test]
fn owner_body_of_returns_query_failed_on_non_int_payload() {
    // Defence-in-depth: if a future kernel ever returns a non-Int payload
    // for OwnerBody, the selector must surface it as QueryFailed rather
    // than panicking on the type mismatch.
    let child = GeometryHandleId(2);
    let kernel =
        MockGeometryKernel::new().with_owner_body_value(child, Value::String("not an int".into()));

    let result = owner_body_of(&kernel, child);
    assert!(matches!(result, Err(QueryError::QueryFailed(_))));
}

#[test]
fn owner_body_of_distinguishes_distinct_children_of_same_parent() {
    // Two children of the same parent — the selector must return the
    // same parent for both, demonstrating the recorded-parent lookup is
    // per-child rather than per-shape.
    let parent = GeometryHandleId(1);
    let f0 = GeometryHandleId(2);
    let f1 = GeometryHandleId(3);
    let kernel = MockGeometryKernel::new()
        .with_owner_body_result(f0, parent)
        .with_owner_body_result(f1, parent);

    assert_eq!(
        owner_body_of(&kernel, f0).expect("f0 should resolve"),
        parent
    );
    assert_eq!(
        owner_body_of(&kernel, f1).expect("f1 should resolve"),
        parent
    );
}
