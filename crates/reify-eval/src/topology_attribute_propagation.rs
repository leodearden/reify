//! Topology-attribute propagation through BRepAlgoAPI history records
//! (v0.2 persistent-naming-v2, task 2590).
//!
//! After a constructive boolean op (Fuse / Cut / Common) the result shape
//! contains a mix of:
//!
//! - parent faces/edges that survived unchanged (Modified maps a parent
//!   sub-shape onto the equivalent result sub-shape);
//! - parent faces/edges that were split or transformed (also Modified, but
//!   1-to-many);
//! - newly-created faces/edges along the cut/seam (Generated, with an
//!   imaginary parent sub-shape — represented in our flat record format
//!   via a `parent_subshape_index` of the surviving parent boundary that
//!   sponsored the new sub-shape);
//! - parent faces/edges that disappeared (Deleted; no result entry).
//!
//! [`propagate_attributes_via_brepalgoapi_history`] takes the per-parent
//! attribute table populated by tasks 5-8 (or, in the foundational task 1
//! integration test, hand-seeded) and copies the parent attribute onto
//! each Modified/Generated result handle. Deleted records are skipped.
//!
//! Per task-1 design decision: the parent's attribute is cloned
//! **unchanged** — `role`, `local_index`, `mod_history`, `user_label`
//! are all preserved. Per-op transformation rules (e.g. "boolean cut's
//! generated faces always carry Role::NewEdge") are deferred to tasks
//! 5-8, which will add per-op variants of this helper.

use reify_types::{
    BooleanOpHistoryRecords, BooleanOpParents, GeometryHandleId, HistoryRecord, QueryError,
    TopologyAttributeTable,
};

/// Propagate parent topology attributes onto the result of a `BRepAlgoAPI`
/// boolean operation, using the Modified / Generated / Deleted records the
/// algorithm exposes.
///
/// Inputs:
/// - `table`: the `TopologyAttributeTable` to update in place. Parent
///   entries are read; new entries are written for each Modified/Generated
///   result sub-shape whose corresponding parent had an attribute.
/// - `parents`: typed wrapper carrying the per-parent face/edge handle
///   slices in canonical TopExp order. Use [`BooleanOpParents::Binary`]
///   for binary booleans (fuse / cut / common), where `parent_index` 0 is
///   the left operand and 1 is the right operand (matching
///   [`HistoryRecord::parent_index`] semantics). Use
///   [`BooleanOpParents::NAry`] for multi-input fuse
///   (`BRepAlgoAPI_BuilderAlgo`).
/// - `result_face_handles`: the result shape's faces in canonical
///   TopExp order. Indexed by `record.result_subshape_index`. The
///   propagation writes entries to these handle ids.
/// - `result_edge_handles`: as above, but for edges.
/// - `history`: the records emitted by the FFI primitive
///   (`OcctKernelHandle::boolean_fuse_with_history`).
///
/// Why pre-extracted vectors?
///
/// `kernel.extract_faces(handle)` / `extract_edges(handle)` allocate
/// fresh `GeometryHandleId`s on each call (the kernel does not dedupe
/// by face-equality). To make the parent attribute lookup by handle id
/// work, the caller must seed the table using the same handle vectors
/// it later passes to the propagation. Likewise, the result-face write
/// keys are the caller's chosen result-face ids — passing them in keeps
/// the function pure with respect to id allocation and lets a downstream
/// consumer (test, task-5-8 auto-population) use the same vectors to
/// inspect what was written.
///
/// Behaviour:
/// - For every Modified or Generated record (faces and edges), if the
///   parent sub-shape has an entry in `table`, clone it onto the
///   corresponding result sub-shape's handle. The clone is **unchanged**
///   — `role`, `local_index`, `mod_history`, `user_label` are all
///   preserved (per-op transformation is task-5-8 scope per PRD).
/// - Deleted records are skipped: a deleted parent has no result entry
///   to write, and the parent's own table entry is left untouched (its
///   handle still resolves; tasks 3/4 will add diagnostics for accidental
///   rebinds).
///
/// Returns `Err(QueryError::QueryFailed)` if any record references an
/// out-of-bounds parent or result sub-shape index — the FFI primitive
/// guarantees in-range indices, so this is a defense-in-depth path
/// pinned by the unit tests below.
///
/// Cross-references PRD docs/prds/v0_2/persistent-naming-v2.md (a)+(c)+(d)
/// of decomposition-plan task 1 (lines 89-103).
pub fn propagate_attributes_via_brepalgoapi_history(
    table: &mut TopologyAttributeTable,
    parents: &BooleanOpParents<'_>,
    result_face_handles: &[GeometryHandleId],
    result_edge_handles: &[GeometryHandleId],
    history: &BooleanOpHistoryRecords,
) -> Result<(), QueryError> {
    let parent_face_handles = parents.face_slices();
    let parent_edge_handles = parents.edge_slices();

    // Faces: Modified ∪ Generated.
    for record in history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
    {
        propagate_one(
            table,
            parent_face_handles,
            result_face_handles,
            record,
            "face",
        )?;
    }

    // Edges: Modified ∪ Generated.
    for record in history
        .edge_modified
        .iter()
        .chain(history.edge_generated.iter())
    {
        propagate_one(
            table,
            parent_edge_handles,
            result_edge_handles,
            record,
            "edge",
        )?;
    }

    // Deleted records are intentionally skipped: no result sub-shape
    // exists to receive the attribute, and parents' existing table
    // entries remain valid (task 3 / task 4 will add diagnostics).
    Ok(())
}

/// Look up the parent attribute via `record.parent_index` /
/// `record.parent_subshape_index`, and if present, clone it onto the
/// result sub-shape at `record.result_subshape_index`.
///
/// Returns `Err(QueryError::QueryFailed)` if any index is out of range.
fn propagate_one(
    table: &mut TopologyAttributeTable,
    parent_handles: &[&[GeometryHandleId]],
    result_handles: &[GeometryHandleId],
    record: &HistoryRecord,
    kind: &str,
) -> Result<(), QueryError> {
    let parent_idx = record.parent_index as usize;
    if parent_idx >= parent_handles.len() {
        return Err(QueryError::QueryFailed(format!(
            "BRepAlgoAPI history {kind} record has parent_index {parent_idx} \
             but only {} parents are tracked",
            parent_handles.len()
        )));
    }
    let parent_vec = parent_handles[parent_idx];
    let parent_subshape_idx = record.parent_subshape_index as usize;
    if parent_subshape_idx >= parent_vec.len() {
        return Err(QueryError::QueryFailed(format!(
            "BRepAlgoAPI history {kind} record has parent_subshape_index {} \
             but parent {} has only {} {kind}s",
            parent_subshape_idx,
            parent_idx,
            parent_vec.len()
        )));
    }
    let parent_handle = parent_vec[parent_subshape_idx];

    let result_subshape_idx = record.result_subshape_index as usize;
    if result_subshape_idx >= result_handles.len() {
        return Err(QueryError::QueryFailed(format!(
            "BRepAlgoAPI history {kind} record has result_subshape_index {} \
             but result has only {} {kind}s",
            result_subshape_idx,
            result_handles.len()
        )));
    }
    let result_handle = result_handles[result_subshape_idx];

    // If the parent had no attribute (e.g. tasks 5-8 only auto-populate
    // for some op kinds; task-1 tests hand-seed only faces), there's
    // nothing to clone — silently skip. The end-to-end test asserts
    // that explicitly-seeded parents propagate.
    if let Some(parent_attr) = table.lookup(parent_handle) {
        let attr_clone = parent_attr.clone();
        table.record(result_handle, attr_clone);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Unit tests focused on the `Err(QueryError::QueryFailed(...))`
    //! defense-in-depth branches of [`propagate_one`]. The happy-path
    //! (parent → result attribute clone via Modified/Generated records)
    //! is fully covered by the PRD-line-93 single integration test in
    //! `tests/topology_attribute_e2e.rs`; duplicating it here would just
    //! double the maintenance surface as the propagation contract evolves.
    //!
    //! These error branches are pure given inputs, so they need no OCCT
    //! kernel — we hand-build a malformed `BooleanOpHistoryRecords` and
    //! check that each variant surfaces as `QueryFailed`.
    use reify_types::{
        BooleanOpHistoryRecords, BooleanOpParents, GeometryHandleId, HistoryRecord, QueryError,
        TopologyAttributeTable,
    };

    use super::propagate_attributes_via_brepalgoapi_history;

    /// Build a `BooleanOpHistoryRecords` with `rec` as the sole
    /// `face_modified` entry and every other vector empty.
    fn history_with_single_face_modified(rec: HistoryRecord) -> BooleanOpHistoryRecords {
        BooleanOpHistoryRecords {
            face_modified: vec![rec],
            ..Default::default()
        }
    }

    /// Build a `BooleanOpHistoryRecords` with `rec` as the sole
    /// `edge_modified` entry and every other vector empty.
    fn history_with_single_edge_modified(rec: HistoryRecord) -> BooleanOpHistoryRecords {
        BooleanOpHistoryRecords {
            edge_modified: vec![rec],
            ..Default::default()
        }
    }

    /// Parent + result handle vectors for a 2-parent, 1-result layout
    /// — owned so the test fn can borrow slices into them without
    /// running afoul of intermediate-temporary lifetime issues.
    struct MinimalLayout {
        parent_faces: [Vec<GeometryHandleId>; 2],
        parent_edges: [Vec<GeometryHandleId>; 2],
        result_faces: Vec<GeometryHandleId>,
        result_edges: Vec<GeometryHandleId>,
    }

    /// One face/edge per parent + one result face/edge — the minimum
    /// shape needed to exercise out-of-range index error paths without
    /// tripping earlier guards.
    fn minimal_parent_result_layout() -> MinimalLayout {
        MinimalLayout {
            parent_faces: [vec![GeometryHandleId(1)], vec![GeometryHandleId(2)]],
            parent_edges: [vec![GeometryHandleId(3)], vec![GeometryHandleId(4)]],
            result_faces: vec![GeometryHandleId(11)],
            result_edges: vec![GeometryHandleId(12)],
        }
    }

    #[test]
    fn propagate_returns_query_failed_when_face_record_has_parent_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = minimal_parent_result_layout();
        let parents = BooleanOpParents::Binary {
            faces: [&layout.parent_faces[0], &layout.parent_faces[1]],
            edges: [&layout.parent_edges[0], &layout.parent_edges[1]],
        };

        // 5 >= 2 parents tracked.
        let history = history_with_single_face_modified(HistoryRecord {
            parent_index: 5,
            parent_subshape_index: 0,
            result_subshape_index: 0,
        });

        let err = propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &layout.result_faces,
            &layout.result_edges,
            &history,
        )
        .expect_err("expected QueryFailed for parent_index out of range");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("parent_index 5"),
                    "error message should mention the offending parent_index, got {msg:?}",
                );
                assert!(
                    msg.contains("face"),
                    "error message should identify face record, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn propagate_returns_query_failed_when_face_record_has_parent_subshape_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = minimal_parent_result_layout();
        let parents = BooleanOpParents::Binary {
            faces: [&layout.parent_faces[0], &layout.parent_faces[1]],
            edges: [&layout.parent_edges[0], &layout.parent_edges[1]],
        };

        // Parent 0 has only 1 face, so subshape 99 is out of range.
        let history = history_with_single_face_modified(HistoryRecord {
            parent_index: 0,
            parent_subshape_index: 99,
            result_subshape_index: 0,
        });

        let err = propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &layout.result_faces,
            &layout.result_edges,
            &history,
        )
        .expect_err("expected QueryFailed for parent_subshape_index out of range");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("parent_subshape_index 99"),
                    "error message should mention the offending parent_subshape_index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn propagate_returns_query_failed_when_face_record_has_result_subshape_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = minimal_parent_result_layout();
        let parents = BooleanOpParents::Binary {
            faces: [&layout.parent_faces[0], &layout.parent_faces[1]],
            edges: [&layout.parent_edges[0], &layout.parent_edges[1]],
        };

        // Result has only 1 face, so subshape 7 is out of range.
        let history = history_with_single_face_modified(HistoryRecord {
            parent_index: 0,
            parent_subshape_index: 0,
            result_subshape_index: 7,
        });

        let err = propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &layout.result_faces,
            &layout.result_edges,
            &history,
        )
        .expect_err("expected QueryFailed for result_subshape_index out of range");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("result_subshape_index 7"),
                    "error message should mention the offending result_subshape_index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn propagate_returns_query_failed_when_edge_record_has_parent_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = minimal_parent_result_layout();
        let parents = BooleanOpParents::Binary {
            faces: [&layout.parent_faces[0], &layout.parent_faces[1]],
            edges: [&layout.parent_edges[0], &layout.parent_edges[1]],
        };

        // Edge equivalent of the parent_index check — confirms the kind
        // arg is threaded into the error message.
        let history = history_with_single_edge_modified(HistoryRecord {
            parent_index: 4,
            parent_subshape_index: 0,
            result_subshape_index: 0,
        });

        let err = propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &layout.result_faces,
            &layout.result_edges,
            &history,
        )
        .expect_err("expected QueryFailed for edge parent_index out of range");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("edge"),
                    "edge-record error message should identify edge kind, got {msg:?}",
                );
                assert!(
                    msg.contains("parent_index 4"),
                    "error message should mention the offending parent_index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn propagate_succeeds_silently_on_empty_history() {
        // No records — propagation is a no-op and must not error even
        // when parent/result handle slices are empty.
        let mut table = TopologyAttributeTable::default();
        let parents = BooleanOpParents::NAry {
            faces: &[],
            edges: &[],
        };
        let result_handles: Vec<GeometryHandleId> = Vec::new();
        let history = BooleanOpHistoryRecords::default();

        propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &result_handles,
            &result_handles,
            &history,
        )
        .expect("empty history should propagate without error");
        assert!(table.is_empty(), "no-op propagation must not write entries");
    }
}
