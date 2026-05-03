//! Shared assertion helpers for local-feature (fillet/chamfer) integration tests.
//!
//! Extracted from `fillet_with_history_integration.rs` and
//! `chamfer_with_history_integration.rs` to eliminate byte-for-byte duplication
//! of the edge-buffer assertion blocks (h)–(l). Future history-record additions
//! (e.g. result_subshape_index sentinels) require only a single edit here rather
//! than dual edits with drift risk.
//!
//! Block (g) mirrors the silent_drop_count invariant from
//! `boolean_op_history_integration.rs` (parity, not extraction).

#![cfg(has_occt)]

use reify_kernel_occt::{LocalFeatureOpHistoryRecords, OcctKernelHandle};
use reify_types::{GeometryError, GeometryHandleId};

/// Assert well-formedness of all edge-related history buffers produced by a
/// local-feature operation (fillet or chamfer) on a 10 mm box.
///
/// Covers assertion blocks (g)–(l) from the integration-test spec:
///
/// - **(g)** `silent_drop_count == 0`: every Modified/Generated child must be
///   resolvable in the result map. Mirrors the same invariant in
///   `boolean_op_history_integration.rs`.
/// - **(g2)** `face_generated` per-edge coverage: the set of distinct
///   `parent_subshape_index` values in `face_generated` must equal 12, one per
///   parent edge of the 10mm cube. Uses `HashSet` deduplication so the check is
///   independent of OCCT's per-edge face count (esc-2655-26 suggestion #1 /
///   task 2821 amendment).
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

    // (g2) face_generated per-edge coverage (esc-2655-26 suggestion #1 / task 2821 amendment).
    //
    // Collect the distinct parent_subshape_index values from face_generated.  For a
    // clean 10mm cube + fillet/chamfer every one of the 12 parent edges must produce
    // at least one generated face, so the HashSet size must equal 12.
    //
    // Using a HashSet rather than `len()` or `saturating_sub` arithmetic makes the
    // check independent of OCCT's per-edge face count: it tests the actual claim
    // ("all 12 edges are covered") without relying on the specific decomposition
    // (6 modified + 8 corner + 12 lateral) that the prior formula encoded.
    let generated_edge_parents: std::collections::HashSet<u32> = history
        .face_generated
        .iter()
        .map(|r| r.parent_subshape_index)
        .collect();
    assert_eq!(
        generated_edge_parents.len(),
        12,
        "{op_name} face_generated must cover all 12 parent edges of the cube; \
         got {} distinct parent_subshape_index values (face_generated.len()={})",
        generated_edge_parents.len(),
        history.face_generated.len()
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

/// Assert that a local-feature op (`fillet_with_history` or `chamfer_with_history`)
/// rejects a non-`BRepKind::Solid` input handle with a descriptive
/// `GeometryError::OperationFailed` message.
///
/// Pass the raw `Result` returned by the op, the human-readable kind label
/// (e.g. `"BRepKind::Face"`, `"BRepKind::Edge"`), and the op name for failure
/// messages.
///
/// # Panics
///
/// Panics unless `result` is `Err(GeometryError::OperationFailed(msg))` where
/// `msg` contains `"Solid"` or `"BRepKind"`.
///
/// Used by `fillet_with_history_rejects_non_solid_input` and
/// `chamfer_with_history_rejects_non_solid_input` (esc-2655-26 suggestions #2/#5 /
/// task 2821 amendment) to eliminate byte-for-byte duplication and to exercise
/// multiple non-Solid kinds (Face and Edge) from a single test body.
#[allow(dead_code)] // only called from has_occt integration-test binaries
pub fn assert_local_feature_rejects_non_solid_input<T>(
    result: Result<T, GeometryError>,
    kind_label: &str,
    op_name: &str,
) {
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!(
            "{op_name} should have rejected a {kind_label} input handle but returned Ok"
        ),
    };
    match &err {
        GeometryError::OperationFailed(msg) => {
            assert!(
                msg.contains("Solid") || msg.contains("BRepKind"),
                "{op_name} OperationFailed message should mention 'Solid' or 'BRepKind' \
                 when rejecting a {kind_label} input: {msg}"
            );
        }
        other => panic!(
            "{op_name} expected GeometryError::OperationFailed for {kind_label} input, \
             got {other:?}"
        ),
    }
}
