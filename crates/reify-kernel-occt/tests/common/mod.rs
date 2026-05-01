//! Shared assertion helpers for local-feature (fillet/chamfer) integration tests.
//!
//! Extracted from `fillet_with_history_integration.rs` and
//! `chamfer_with_history_integration.rs` to eliminate byte-for-byte duplication
//! of the edge-buffer assertion blocks (g)–(l). Future history-record additions
//! (e.g. result_subshape_index sentinels) require only a single edit here rather
//! than dual edits with drift risk.

#![cfg(has_occt)]

use reify_kernel_occt::{LocalFeatureOpHistoryRecords, OcctKernelHandle};
use reify_types::GeometryHandleId;

/// Assert well-formedness of all edge-related history buffers produced by a
/// local-feature operation (fillet or chamfer) on a 10 mm box.
///
/// Covers assertion blocks (g)–(l) from the integration-test spec:
///
/// - **(g)** `silent_drop_count == 0`: every Modified/Generated child must be
///   resolvable in the result map. Mirrors the same invariant in
///   `boolean_op_history_integration.rs`.
/// - **(h)** Extracts `result_edge_count` via `kernel.extract_edges(result_id)`.
/// - **(i)** `edge_modified` per-record well-formedness: `parent_index == 0`,
///   `parent_subshape_index < 12` (box edges), `result_subshape_index < result_edge_count`.
///   No non-empty assertion — OCCT may route parent edges through Generated/Deleted.
/// - **(j)** `edge_generated` per-record well-formedness: `parent_index == 0`,
///   `parent_subshape_index < 8` (box **vertices** — `edge_generated` is keyed by
///   the parent VERTEX map, not the edge map), `result_subshape_index < result_edge_count`.
/// - **(k)** `face_deleted.is_empty()`: the operation does not consume any parent face.
/// - **(l)** `!edge_deleted.is_empty()`: OCCT marks all 12 parent edges as `IsDeleted()`
///   (they are subsumed by the generated fillet/chamfer surfaces). Per-record bounds:
///   `parent_index == 0`, `parent_subshape_index < 12`.
///
/// `op_name` is included in every failure message (e.g. `"fillet"` or `"chamfer"`).
///
/// # Panics
///
/// Panics with a descriptive message if any assertion fails.
#[allow(dead_code)] // only called from has_occt integration-test binaries
pub fn assert_local_feature_history_well_formed(
    kernel: &OcctKernelHandle,
    result_id: GeometryHandleId,
    history: &LocalFeatureOpHistoryRecords,
    op_name: &str,
) {
    // (g) silent_drop_count must be zero for a well-formed clean local-feature op:
    //     every Modified/Generated child must be resolvable in the result map.
    //     Mirrors the same invariant in boolean_op_history_integration.rs.
    assert_eq!(
        history.silent_drop_count,
        0,
        "{op_name} should not silently drop any history record on a clean 10mm-box op; \
         got {} drops",
        history.silent_drop_count
    );

    // (h) Derive result_edge_count for index-bounds checks.
    let result_edges = kernel
        .extract_edges(result_id)
        .expect("extract_edges on the local-feature result should succeed");
    let result_edge_count = result_edges.len() as u32;

    // (i) edge_modified per-record well-formedness.
    // No non-empty assertion: for a fully-filleted/chamfered box, OCCT may route
    // parent edges through Generated() or IsDeleted() rather than Modified()
    // (see plan design decision).
    for r in &history.edge_modified {
        assert_eq!(
            r.parent_index, 0,
            "{op_name} edge_modified records always have parent_index=0, got {}",
            r.parent_index
        );
        assert!(
            r.parent_subshape_index < 12,
            "{op_name} edge_modified parent_subshape_index {} out of range for a 12-edge box",
            r.parent_subshape_index
        );
        assert!(
            r.result_subshape_index < result_edge_count,
            "{op_name} edge_modified result_subshape_index {} out of range; result has {} edges",
            r.result_subshape_index,
            result_edge_count
        );
    }

    // (j) edge_generated per-record well-formedness.
    // parent_subshape_index is into the VERTEX map (box has 8 vertices), not
    // the edge map, because edge_generated is populated via
    // emit_sweep_generated_cross_type(shape_vertex_map, result_edge_map, TopAbs_EDGE).
    for r in &history.edge_generated {
        assert_eq!(
            r.parent_index, 0,
            "{op_name} edge_generated records always have parent_index=0, got {}",
            r.parent_index
        );
        assert!(
            r.parent_subshape_index < 8,
            "{op_name} edge_generated parent_subshape_index {} out of range for an 8-vertex box \
             (edge_generated is keyed by parent VERTEX, not parent edge)",
            r.parent_subshape_index
        );
        assert!(
            r.result_subshape_index < result_edge_count,
            "{op_name} edge_generated result_subshape_index {} out of range; result has {} edges",
            r.result_subshape_index,
            result_edge_count
        );
    }

    // (k) face_deleted must be empty for a clean local-feature op on a convex box.
    assert!(
        history.face_deleted.is_empty(),
        "clean {op_name} must not delete any parent face; got {} face_deleted records",
        history.face_deleted.len()
    );

    // (l) edge_deleted must be non-empty: BRepFilletAPI_Make{Fillet,Chamfer} marks
    // all 12 parent edges as IsDeleted() because they are fully subsumed by the
    // generated fillet/chamfer surfaces. A regression that stops emitting
    // edge_deleted records (e.g. a broken IsDeleted() walk or index-map mismatch)
    // would produce an empty vec — caught here.
    assert!(
        !history.edge_deleted.is_empty(),
        "{op_name} edge_deleted must be non-empty: OCCT marks all parent edges IsDeleted(); \
         got 0 records (regression in edge-deleted emit loop?)"
    );
    for d in &history.edge_deleted {
        assert_eq!(
            d.parent_index, 0,
            "{op_name} edge_deleted records always have parent_index=0, got {}",
            d.parent_index
        );
        assert!(
            d.parent_subshape_index < 12,
            "{op_name} edge_deleted parent_subshape_index {} out of range for a 12-edge box",
            d.parent_subshape_index
        );
    }
}

/// Verify that the helper panics with a message containing "silently drop"
/// when `silent_drop_count` is non-zero.
///
/// This test is expected to FAIL before the `silent_drop_count == 0` assertion
/// is added to `assert_local_feature_history_well_formed` (step-2). Without
/// that assertion, the helper panics elsewhere (at `extract_edges` or the `(l)`
/// `edge_deleted` check) with a message that does NOT contain "silently drop",
/// so `#[should_panic(expected = "silently drop")]` correctly reports failure.
///
/// After step-2 adds the assertion at the TOP of the helper, it fires first
/// (before `extract_edges` is ever called) and its message contains
/// "silently drop" — this test then passes.
#[test]
#[should_panic(expected = "silently drop")]
fn helper_panics_when_silent_drop_count_nonzero() {
    let kernel = OcctKernelHandle::spawn();
    let history = LocalFeatureOpHistoryRecords {
        silent_drop_count: 1,
        ..Default::default()
    };
    // GeometryHandleId(0) is a deliberately bogus id. It is fine because the
    // new assertion (step-2) fires at the TOP of the helper, before
    // `extract_edges` is called — so the kernel is never actually queried.
    assert_local_feature_history_well_formed(&kernel, GeometryHandleId(0), &history, "test_op");
}
