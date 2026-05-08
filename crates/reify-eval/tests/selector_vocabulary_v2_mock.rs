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
    Axis, ExtremalSense, complement, edges_perpendicular_to, except, extremal_by_bbox,
    extremal_by_centroid, faces_perpendicular_to, intersect, union,
};
use reify_test_support::MockGeometryKernel;
use reify_types::{GeometryHandleId, QueryError, Value};

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
        vec![GeometryHandleId(1), GeometryHandleId(2), GeometryHandleId(3)],
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
        .with_face_normal_result(
            f_x,
            Value::String("{\"x\":1.0,\"y\":0.0,\"z\":0.0}".into()),
        )
        .with_face_normal_result(
            f_y,
            Value::String("{\"x\":0.0,\"y\":1.0,\"z\":0.0}".into()),
        )
        .with_face_normal_result(
            f_z,
            Value::String("{\"x\":0.0,\"y\":0.0,\"z\":1.0}".into()),
        );

    let result =
        faces_perpendicular_to(&mut kernel, parent, [1.0, 0.0, 0.0], 1.0_f64.to_radians())
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
        .with_face_normal_result(
            f_y,
            Value::String("{\"x\":0.0,\"y\":1.0,\"z\":0.0}".into()),
        );

    let result =
        faces_perpendicular_to(&mut kernel, parent, [1.0, 0.0, 0.0], 1.0_f64.to_radians())
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

#[test]
fn faces_perpendicular_to_degenerate_normal_returns_query_failed() {
    // A face that reports a zero normal (degenerate face) must surface a
    // QueryFailed rather than slipping through with NaN-poisoned arithmetic.
    let parent = GeometryHandleId(1);
    let f = GeometryHandleId(2);
    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f])
        .with_face_normal_result(
            f,
            Value::String("{\"x\":0.0,\"y\":0.0,\"z\":0.0}".into()),
        );
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
        .with_edge_tangent_result(
            e_x,
            Value::String("{\"x\":1.0,\"y\":0.0,\"z\":0.0}".into()),
        )
        .with_edge_tangent_result(
            e_y,
            Value::String("{\"x\":0.0,\"y\":1.0,\"z\":0.0}".into()),
        )
        .with_edge_tangent_result(
            e_z,
            Value::String("{\"x\":0.0,\"y\":0.0,\"z\":1.0}".into()),
        );

    let result =
        edges_perpendicular_to(&mut kernel, parent, [0.0, 0.0, 1.0], 1.0_f64.to_radians())
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

#[test]
fn edges_perpendicular_to_degenerate_tangent_returns_query_failed() {
    let parent = GeometryHandleId(1);
    let e = GeometryHandleId(2);
    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e])
        .with_edge_tangent_result(
            e,
            Value::String("{\"x\":0.0,\"y\":0.0,\"z\":0.0}".into()),
        );
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
    assert_eq!(result, vec![f2], "Max along X picks the candidate with xmax=0.009");
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
    assert_eq!(result, vec![f2], "Max along Z picks the candidate with zmax=0.011");
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
    Value::String(format!(
        "{{\"x\":{},\"y\":{},\"z\":{}}}",
        p[0], p[1], p[2]
    ))
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

    let result =
        extremal_by_centroid(&mut kernel, &candidates, Axis::Y, ExtremalSense::Max, 1e-6)
            .expect("extremal_by_centroid should succeed for unique max");
    assert_eq!(
        result,
        vec![f4],
        "unique centroid-y max (tol=1e-6) is the 10e-3 face"
    );
}

#[test]
fn extremal_by_centroid_min_sense_returns_tied_cluster_in_input_order() {
    // Stage four faces with centroid Y values 0.0, 5e-3, 5e-3, 10e-3.
    // With Min sense and tol=1e-3 the cluster around the global min (0.0)
    // captures both 0.0 and 5e-3 (their absolute difference is 5e-3 ≪ 1e-3? NO).
    // Re-check: |0.005 - 0.0| = 5e-3 > 1e-3 → 5e-3 is OUT. Reset fixture.
    //
    // Use centroid Y values 0.0, 5e-4, 5e-4, 10e-3 so the cluster around
    // the global min (0.0) at tol=1e-3 captures both 0.0 and 5e-4 (twice).
    // Dedup-on-first-seen yields [f1, f2] (the two distinct handles whose
    // centroid Y is within 1e-3 of 0.0), in input order.
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

    let result =
        extremal_by_centroid(&mut kernel, &candidates, Axis::Y, ExtremalSense::Min, 1e-3)
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

    let max_x =
        extremal_by_centroid(&mut kernel, &candidates, Axis::X, ExtremalSense::Max, 1e-6)
            .expect("extremal_by_centroid X/Max should succeed");
    assert_eq!(max_x, vec![f2], "Max along X picks the f2 (x=0.010)");

    let max_y =
        extremal_by_centroid(&mut kernel, &candidates, Axis::Y, ExtremalSense::Max, 1e-6)
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
    let result =
        extremal_by_centroid(&mut kernel, &candidates, Axis::Z, ExtremalSense::Max, 1e-6)
            .expect("extremal_by_centroid on empty candidates should succeed");
    assert!(
        result.is_empty(),
        "empty candidate slice yields empty cluster"
    );
}
