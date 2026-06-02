//! Mock-kernel unit tests for the three new `*_with_tags` tagged variants of the
//! filtered topology selectors (task 2329).
//!
//! These tests are always-on (no OCCT runtime required). They use
//! `MockGeometryKernel` with pre-configured builders to exercise:
//!   - `edges_by_length_with_tags` (step 1/2)
//!   - `faces_by_area_with_tags` (step 3/4)
//!   - `edges_parallel_to_with_tags` (step 5/6)
//!
//! Each test asserts a four-part contract:
//!   (a) tagged variant's filtered output equals the baseline's filtered output;
//!   (b) every extracted sub-shape (pre-filter) has a recorded FeatureTag;
//!   (c) recorded tag.step_kind and tag.source_span match the parent_tag;
//!   (d) recorded sub_index values are {0,1,2} (canonical enumerate order, one per extracted sub-shape).
//!
//! Handle id convention: id=1 is the parent solid, id=2..N are sub-shape handles
//! returned by extraction.

use reify_core::SourceSpan;
use reify_eval::topology_selectors;
use reify_ir::{FeatureTag, FeatureTagTable, GeometryHandleId, QueryError, StepKind, Value};
use reify_test_support::MockGeometryKernel;

// ─── edges_by_length_with_tags ────────────────────────────────────────────────

/// `edges_by_length_with_tags` must:
/// (a) return vec![GeometryHandleId(3)] — the same filtered output as the baseline;
/// (b) table.len() == 3 (every extracted edge tagged, pre-filter);
/// (c) each recorded tag carries step_kind == Primitive AND source_span from parent_tag;
/// (d) recorded sub_index values are {0,1,2} (one per extracted edge, canonical enumerate order).
#[test]
fn edges_by_length_with_tags_matches_baseline_and_records_per_edge_tags() {
    let parent = GeometryHandleId(1);
    let e2 = GeometryHandleId(2); // length 0.005 m — below window
    let e3 = GeometryHandleId(3); // length 0.010 m — inside window [0.008, 0.012]
    let e4 = GeometryHandleId(4); // length 0.015 m — above window

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e2, e3, e4])
        .with_edge_length_result(e2, Value::Real(0.005))
        .with_edge_length_result(e3, Value::Real(0.010))
        .with_edge_length_result(e4, Value::Real(0.015));

    let parent_tag = FeatureTag {
        source_span: SourceSpan::new(10, 30),
        step_kind: StepKind::Primitive,
        sub_index: 0,
    };

    // (a) baseline and tagged variant must return the same filtered vec.
    let baseline = topology_selectors::edges_by_length(&mut kernel, parent, 0.008, 0.012)
        .expect("edges_by_length should succeed");
    assert_eq!(baseline, vec![e3], "baseline: expected only the 10mm edge");

    let mut table = FeatureTagTable::default();
    let tagged = topology_selectors::edges_by_length_with_tags(
        &mut kernel,
        &mut table,
        parent,
        parent_tag,
        0.008,
        0.012,
    )
    .expect("edges_by_length_with_tags should succeed");
    assert_eq!(
        tagged, baseline,
        "tagged variant must return the same filtered vec as baseline"
    );

    // (b) every extracted edge (pre-filter) has a tag.
    assert_eq!(
        table.len(),
        3,
        "table must have one entry per extracted edge"
    );
    for &id in &[e2, e3, e4] {
        assert!(
            table.lookup(id).is_some(),
            "extracted edge {:?} must have a FeatureTag recorded",
            id
        );
    }

    // (c) step_kind and source_span match parent_tag for each recorded tag.
    for &id in &[e2, e3, e4] {
        let tag = table.lookup(id).unwrap();
        assert_eq!(
            tag.step_kind, parent_tag.step_kind,
            "tag.step_kind for {:?} must match parent",
            id
        );
        assert_eq!(
            tag.source_span, parent_tag.source_span,
            "tag.source_span for {:?} must match parent",
            id
        );
    }

    // (d) sub_index values are {0,1,2} (canonical enumerate order, one per extracted edge).
    let mut sub_indices: Vec<u32> = [e2, e3, e4]
        .iter()
        .map(|&id| table.lookup(id).unwrap().sub_index)
        .collect();
    sub_indices.sort_unstable();
    assert_eq!(
        sub_indices,
        vec![0u32, 1, 2],
        "sub_index values must be the enumerate positions {{0,1,2}}"
    );
}

// ─── faces_by_area_with_tags ──────────────────────────────────────────────────

/// `faces_by_area_with_tags` must:
/// (a) return vec![GeometryHandleId(3)] — same as baseline;
/// (b) table.len() == 3 (every face tagged, pre-filter);
/// (c) recorded tags carry step_kind == Boolean AND source_span == parent_tag.source_span
///     (parent_tag.sub_index == 7 must NOT be inherited — sub_index is overwritten per child);
/// (d) recorded sub_index values are {0,1,2} (one per extracted face, canonical order).
#[test]
fn faces_by_area_with_tags_matches_baseline_and_records_per_face_tags() {
    let parent = GeometryHandleId(1);
    let f2 = GeometryHandleId(2); // area 1.0e-4 m^2 — below window
    let f3 = GeometryHandleId(3); // area 4.0e-4 m^2 — inside window [2.0e-4, 5.0e-4]
    let f4 = GeometryHandleId(4); // area 9.0e-4 m^2 — above window

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f2, f3, f4])
        .with_surface_area_result(f2, Value::Real(1.0e-4))
        .with_surface_area_result(f3, Value::Real(4.0e-4))
        .with_surface_area_result(f4, Value::Real(9.0e-4));

    // Deliberately non-zero sub_index (7) to prove sub_index is overwritten per child.
    let parent_tag = FeatureTag {
        source_span: SourceSpan::new(50, 80),
        step_kind: StepKind::Boolean,
        sub_index: 7,
    };

    // (a) baseline and tagged variant must return the same filtered vec.
    let baseline = topology_selectors::faces_by_area(&mut kernel, parent, 2.0e-4, 5.0e-4)
        .expect("faces_by_area should succeed");
    assert_eq!(
        baseline,
        vec![f3],
        "baseline: expected only the 4.0e-4 m^2 face"
    );

    let mut table = FeatureTagTable::default();
    let tagged = topology_selectors::faces_by_area_with_tags(
        &mut kernel,
        &mut table,
        parent,
        parent_tag,
        2.0e-4,
        5.0e-4,
    )
    .expect("faces_by_area_with_tags should succeed");
    assert_eq!(
        tagged, baseline,
        "tagged variant must return the same filtered vec as baseline"
    );

    // (b) every extracted face (pre-filter) has a tag.
    assert_eq!(
        table.len(),
        3,
        "table must have one entry per extracted face"
    );
    for &id in &[f2, f3, f4] {
        assert!(
            table.lookup(id).is_some(),
            "extracted face {:?} must have a FeatureTag recorded",
            id
        );
    }

    // (c) step_kind and source_span match parent_tag; parent's sub_index (7) NOT inherited.
    for &id in &[f2, f3, f4] {
        let tag = table.lookup(id).unwrap();
        assert_eq!(
            tag.step_kind, parent_tag.step_kind,
            "tag.step_kind for {:?} must match parent",
            id
        );
        assert_eq!(
            tag.source_span, parent_tag.source_span,
            "tag.source_span for {:?} must match parent",
            id
        );
        // sub_index must NOT be parent's sub_index (7).
        assert_ne!(
            tag.sub_index, 7,
            "child sub_index must be overwritten (enumerate position), not inherited from parent"
        );
    }

    // (d) sub_index values are {0,1,2} (canonical order, one per extracted face).
    let mut sub_indices: Vec<u32> = [f2, f3, f4]
        .iter()
        .map(|&id| table.lookup(id).unwrap().sub_index)
        .collect();
    sub_indices.sort_unstable();
    assert_eq!(
        sub_indices,
        vec![0u32, 1, 2],
        "sub_index values must be the enumerate positions {{0,1,2}}"
    );
}

// ─── edges_parallel_to_with_tags ─────────────────────────────────────────────

/// `edges_parallel_to_with_tags` must:
/// (a) return vec![GeometryHandleId(2), GeometryHandleId(3)] — same as baseline
///     (+x and -x edges accepted sign-tolerantly; +y rejected);
/// (b) table.len() == 3 (every edge tagged, including the rejected +y edge);
/// (c) recorded tags carry step_kind == Sweep AND source_span == parent_tag.source_span;
/// (d) sub_index values are {0,1,2}.
#[test]
fn edges_parallel_to_with_tags_matches_baseline_and_records_per_edge_tags() {
    let parent = GeometryHandleId(1);
    let e2 = GeometryHandleId(2); // tangent +x — accepted
    let e3 = GeometryHandleId(3); // tangent -x — accepted (sign-tolerant)
    let e4 = GeometryHandleId(4); // tangent +y — rejected

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e2, e3, e4])
        .with_edge_tangent_result(e2, Value::String("{\"x\":1,\"y\":0,\"z\":0}".into()))
        .with_edge_tangent_result(e3, Value::String("{\"x\":-1,\"y\":0,\"z\":0}".into()))
        .with_edge_tangent_result(e4, Value::String("{\"x\":0,\"y\":1,\"z\":0}".into()));

    let parent_tag = FeatureTag {
        source_span: SourceSpan::new(100, 130),
        step_kind: StepKind::Sweep,
        sub_index: 0,
    };

    // (a) baseline and tagged variant must return the same filtered vec.
    let baseline = topology_selectors::edges_parallel_to(
        &mut kernel,
        parent,
        [1.0, 0.0, 0.0],
        1f64.to_radians(),
    )
    .expect("edges_parallel_to should succeed");
    assert_eq!(
        baseline,
        vec![e2, e3],
        "baseline: expected both ±x edges (sign-tolerant); +y rejected"
    );

    let mut table = FeatureTagTable::default();
    let tagged = topology_selectors::edges_parallel_to_with_tags(
        &mut kernel,
        &mut table,
        parent,
        parent_tag,
        [1.0, 0.0, 0.0],
        1f64.to_radians(),
    )
    .expect("edges_parallel_to_with_tags should succeed");
    assert_eq!(
        tagged, baseline,
        "tagged variant must return the same filtered vec as baseline"
    );

    // (b) every extracted edge (pre-filter, including rejected +y) has a tag.
    assert_eq!(
        table.len(),
        3,
        "table must have one entry per extracted edge"
    );
    for &id in &[e2, e3, e4] {
        assert!(
            table.lookup(id).is_some(),
            "extracted edge {:?} must have a FeatureTag recorded (even if rejected by filter)",
            id
        );
    }

    // (c) step_kind and source_span match parent_tag for each recorded tag.
    for &id in &[e2, e3, e4] {
        let tag = table.lookup(id).unwrap();
        assert_eq!(
            tag.step_kind, parent_tag.step_kind,
            "tag.step_kind for {:?} must match parent",
            id
        );
        assert_eq!(
            tag.source_span, parent_tag.source_span,
            "tag.source_span for {:?} must match parent",
            id
        );
    }

    // (d) sub_index values are {0,1,2}.
    let mut sub_indices: Vec<u32> = [e2, e3, e4]
        .iter()
        .map(|&id| table.lookup(id).unwrap().sub_index)
        .collect();
    sub_indices.sort_unstable();
    assert_eq!(
        sub_indices,
        vec![0u32, 1, 2],
        "sub_index values must be the enumerate positions {{0,1,2}}"
    );
}

// ─── edges_parallel_to_with_tags: 'fail before kernel touch' contract ─────────

/// `edges_parallel_to_with_tags` with a zero axis must return
/// `Err(QueryFailed)` *before* calling `extract_edges` or mutating `table`.
///
/// This pins the 'fail before kernel touch' contract documented in the
/// function's rustdoc: `normalize3(axis)` is the first operation, so a
/// degenerate axis never reaches the kernel or the tag-recording step.
#[test]
fn edges_parallel_to_with_tags_zero_axis_errors_before_table_mutation() {
    let parent = GeometryHandleId(1);
    // No edges configured — the kernel must not be touched at all.
    let mut kernel = MockGeometryKernel::new();
    let mut table = FeatureTagTable::default();
    let parent_tag = FeatureTag {
        source_span: SourceSpan::new(0, 10),
        step_kind: StepKind::Primitive,
        sub_index: 0,
    };

    let result = topology_selectors::edges_parallel_to_with_tags(
        &mut kernel,
        &mut table,
        parent,
        parent_tag,
        [0.0, 0.0, 0.0], // zero axis — degenerate
        1f64.to_radians(),
    );

    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("non-zero and finite"),
                "error should mention 'non-zero and finite', got: {msg:?}"
            );
        }
        other => panic!("expected Err(QueryFailed) for zero axis, got {:?}", other),
    }
    // Table must be untouched: axis validation fires before any extract / record.
    assert_eq!(
        table.len(),
        0,
        "table must remain empty: zero axis must error before any kernel or table touch"
    );
}

/// Shared assertion fixture for the three angular-tolerance boundary tests
/// below.  Calls `edges_parallel_to_with_tags` with a valid axis and an
/// empty kernel + empty table, then asserts:
///   - The result is `Err(QueryFailed)` with `"angular_tol_rad"` in the
///     message.
///   - `table.len() == 0` — tol validation fired before any kernel or table
///     touch.
fn assert_tol_rejected(tol: f64) {
    let parent = GeometryHandleId(1);
    let mut kernel = MockGeometryKernel::new();
    let mut table = FeatureTagTable::default();
    let parent_tag = FeatureTag {
        source_span: SourceSpan::new(0, 10),
        step_kind: StepKind::Primitive,
        sub_index: 0,
    };
    let result = topology_selectors::edges_parallel_to_with_tags(
        &mut kernel,
        &mut table,
        parent,
        parent_tag,
        [1.0, 0.0, 0.0],
        tol,
    );
    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("angular_tol_rad"),
                "error should mention 'angular_tol_rad', got: {msg:?}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed) for tol {:?}, got {:?}",
            tol, other
        ),
    }
    assert_eq!(
        table.len(),
        0,
        "table must remain empty: tol validation must error before any kernel or table touch"
    );
}

#[test]
fn edges_parallel_to_with_tags_negative_tol_errors_before_table_mutation() {
    assert_tol_rejected(-0.1);
}

#[test]
fn edges_parallel_to_with_tags_tol_above_half_pi_errors_before_table_mutation() {
    assert_tol_rejected(std::f64::consts::FRAC_PI_2 + 1e-3);
}

#[test]
fn edges_parallel_to_with_tags_nan_tol_errors_before_table_mutation() {
    assert_tol_rejected(f64::NAN);
}

// ─── negative tests: post-extraction error paths ──────────────────────────────

/// `edges_by_length_with_tags` must propagate `Err(QueryFailed)` when the
/// kernel returns a non-`Real` value for `EdgeLength`.
///
/// Tags are recorded **before** the per-subshape query loop (in
/// `record_subshape_tags`), so the table IS populated even when the query
/// reply is invalid. This test pins that contract: callers can inspect the
/// table after catching the error and still see which sub-shapes were
/// extracted before the failure.
#[test]
fn edges_by_length_with_tags_bad_query_reply_propagates_error() {
    let parent = GeometryHandleId(1);
    let e = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e])
        .with_edge_length_result(e, Value::Int(5)); // intentionally wrong type

    let mut table = FeatureTagTable::default();
    let parent_tag = FeatureTag {
        source_span: SourceSpan::new(10, 30),
        step_kind: StepKind::Primitive,
        sub_index: 0,
    };

    let result = topology_selectors::edges_by_length_with_tags(
        &mut kernel,
        &mut table,
        parent,
        parent_tag,
        0.0,
        100.0,
    );

    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("non-real value"),
                "error message should mention 'non-real value', got: {msg:?}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed) for non-real EdgeLength, got {:?}",
            other
        ),
    }
    // Tags are recorded pre-filter (before query_per_subshape runs), so the
    // table IS populated even on a query error.
    assert_eq!(
        table.len(),
        1,
        "table should contain the tag recorded before the query error"
    );
}

/// `faces_by_area_with_tags` must propagate `Err(QueryFailed)` when the
/// kernel returns a non-`Real` value for `SurfaceArea`.
///
/// Tags are recorded before the query loop, so the table IS populated
/// even when the query reply is invalid (same contract as
/// `edges_by_length_with_tags`).
#[test]
fn faces_by_area_with_tags_bad_query_reply_propagates_error() {
    let parent = GeometryHandleId(1);
    let f = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(parent, vec![f])
        .with_surface_area_result(f, Value::Int(5)); // intentionally wrong type

    let mut table = FeatureTagTable::default();
    let parent_tag = FeatureTag {
        source_span: SourceSpan::new(50, 80),
        step_kind: StepKind::Boolean,
        sub_index: 0,
    };

    let result = topology_selectors::faces_by_area_with_tags(
        &mut kernel,
        &mut table,
        parent,
        parent_tag,
        0.0,
        100.0,
    );

    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("non-real value"),
                "error message should mention 'non-real value', got: {msg:?}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed) for non-real SurfaceArea, got {:?}",
            other
        ),
    }
    // Tags are recorded pre-filter, so table IS populated even on query error.
    assert_eq!(
        table.len(),
        1,
        "table should contain the tag recorded before the query error"
    );
}

/// `edges_parallel_to_with_tags` must propagate `Err(QueryFailed)` when
/// an extracted edge returns a degenerate (near-zero) tangent.
///
/// Unlike the zero-axis case, a degenerate *tangent* is detected in the
/// predicate loop **after** `record_subshape_tags` has already run — so the
/// table IS populated even when the tangent normalisation fails. This test
/// pins that ordering contract explicitly.
#[test]
fn edges_parallel_to_with_tags_degenerate_tangent_errors_after_tag_recording() {
    let parent = GeometryHandleId(1);
    let e = GeometryHandleId(2);

    let mut kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e])
        // A zero vector: normalize3 returns None → QueryFailed("degenerate").
        .with_edge_tangent_result(e, Value::String("{\"x\":0,\"y\":0,\"z\":0}".into()));

    let mut table = FeatureTagTable::default();
    let parent_tag = FeatureTag {
        source_span: SourceSpan::new(100, 130),
        step_kind: StepKind::Sweep,
        sub_index: 0,
    };

    let result = topology_selectors::edges_parallel_to_with_tags(
        &mut kernel,
        &mut table,
        parent,
        parent_tag,
        [1.0, 0.0, 0.0],
        0.1,
    );

    match result {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("degenerate"),
                "error should mention 'degenerate' for a zero tangent, got: {msg:?}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed) for degenerate tangent, got {:?}",
            other
        ),
    }
    // record_subshape_tags runs before the predicate loop, so the tag IS recorded
    // even when the tangent normalisation fails mid-loop.
    assert_eq!(
        table.len(),
        1,
        "table should contain the tag recorded before the tangent normalisation error"
    );
}
