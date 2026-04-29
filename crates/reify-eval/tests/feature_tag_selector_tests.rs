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
//!   (d) recorded sub_index values are unique across all extracted sub-shapes.
//!
//! Handle id convention: id=1 is the parent solid, id=2..N are sub-shape handles
//! returned by extraction.

use reify_eval::topology_selectors;
use reify_test_support::MockGeometryKernel;
use reify_types::{FeatureTag, FeatureTagTable, GeometryHandleId, SourceSpan, StepKind, Value};

// ─── edges_by_length_with_tags ────────────────────────────────────────────────

/// `edges_by_length_with_tags` must:
/// (a) return vec![GeometryHandleId(3)] — the same filtered output as the baseline;
/// (b) table.len() == 3 (every extracted edge tagged, pre-filter);
/// (c) each recorded tag carries step_kind == Primitive AND source_span from parent_tag;
/// (d) the three recorded sub_index values are unique (sorted-deduped len == 3).
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
    let tagged =
        topology_selectors::edges_by_length_with_tags(&mut kernel, &mut table, parent, parent_tag, 0.008, 0.012)
            .expect("edges_by_length_with_tags should succeed");
    assert_eq!(
        tagged, baseline,
        "tagged variant must return the same filtered vec as baseline"
    );

    // (b) every extracted edge (pre-filter) has a tag.
    assert_eq!(table.len(), 3, "table must have one entry per extracted edge");
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

    // (d) sub_index values are unique across all extracted edges.
    let mut sub_indices: Vec<u32> = [e2, e3, e4]
        .iter()
        .map(|&id| table.lookup(id).unwrap().sub_index)
        .collect();
    let original_len = sub_indices.len();
    sub_indices.sort_unstable();
    sub_indices.dedup();
    assert_eq!(
        sub_indices.len(),
        original_len,
        "sub_index values must be unique across all extracted edges"
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
    assert_eq!(baseline, vec![f3], "baseline: expected only the 4.0e-4 m^2 face");

    let mut table = FeatureTagTable::default();
    let tagged =
        topology_selectors::faces_by_area_with_tags(&mut kernel, &mut table, parent, parent_tag, 2.0e-4, 5.0e-4)
            .expect("faces_by_area_with_tags should succeed");
    assert_eq!(
        tagged, baseline,
        "tagged variant must return the same filtered vec as baseline"
    );

    // (b) every extracted face (pre-filter) has a tag.
    assert_eq!(table.len(), 3, "table must have one entry per extracted face");
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
    let original_len = sub_indices.len();
    sub_indices.sort_unstable();
    sub_indices.dedup();
    assert_eq!(
        sub_indices.len(),
        original_len,
        "sub_index values must be unique across all extracted faces"
    );
    // Also verify the actual values are {0,1,2}.
    assert_eq!(
        sub_indices,
        vec![0u32, 1, 2],
        "sub_index values must be the enumerate positions {0,1,2}"
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
    let baseline =
        topology_selectors::edges_parallel_to(&mut kernel, parent, [1.0, 0.0, 0.0], 1f64.to_radians())
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
    assert_eq!(table.len(), 3, "table must have one entry per extracted edge");
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
    let original_len = sub_indices.len();
    sub_indices.sort_unstable();
    sub_indices.dedup();
    assert_eq!(
        sub_indices.len(),
        original_len,
        "sub_index values must be unique across all extracted edges"
    );
    assert_eq!(
        sub_indices,
        vec![0u32, 1, 2],
        "sub_index values must be the enumerate positions {0,1,2}"
    );
}
