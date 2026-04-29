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
        BooleanOpHistoryRecords, BooleanOpParents, CapKind, FeatureId, GeometryHandleId,
        HistoryRecord, QueryError, Role, SweepOpHistoryRecords, TopologyAttributeTable,
    };

    use super::{populate_extrude_attributes, propagate_attributes_via_brepalgoapi_history};

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
        let parents = BooleanOpParents::nary(&[], &[]);
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

    #[test]
    fn no_records_binary_succeeds() {
        // Smoke-test: Binary variant + empty history must succeed and leave
        // the table empty — exercises the Binary accessor path through
        // propagation without hitting any error branch.
        let mut table = TopologyAttributeTable::default();
        let layout = minimal_parent_result_layout();
        let parents = BooleanOpParents::Binary {
            faces: [&layout.parent_faces[0], &layout.parent_faces[1]],
            edges: [&layout.parent_edges[0], &layout.parent_edges[1]],
        };
        let history = BooleanOpHistoryRecords::default();

        propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parents,
            &layout.result_faces,
            &layout.result_edges,
            &history,
        )
        .expect("empty history with Binary parents should propagate without error");
        assert!(table.is_empty(), "no-op propagation must not write entries");
    }

    // -- populate_extrude_attributes tests (task 5a, step-11) --
    //
    // The helper originates new attributes for an extrude result: cap faces
    // get `Role::Cap(CapKind::Top|Bottom)` with local_index 0; lateral faces
    // get `Role::Side` with sequential 0-based local_index in face_generated
    // order. Profile face/edge slices are passed in for defense-in-depth
    // index-range validation.

    /// Profile + result handle vectors for a 1-parent extrude layout.
    /// Owned so the test fn can borrow slices without temporary-lifetime
    /// issues.
    struct ExtrudeLayout {
        profile_faces: Vec<GeometryHandleId>,
        profile_edges: Vec<GeometryHandleId>,
        result_faces: Vec<GeometryHandleId>,
        result_edges: Vec<GeometryHandleId>,
    }

    /// Layout for a rect-face extrude: 1 profile face, 4 profile edges,
    /// 9 result faces (indices 0..=8 → 5 = start cap, 6 = end cap, 7/8
    /// = side faces), 12 result edges.
    fn extrude_layout_for_step11() -> ExtrudeLayout {
        ExtrudeLayout {
            profile_faces: vec![GeometryHandleId(101)],
            profile_edges: vec![
                GeometryHandleId(201),
                GeometryHandleId(202),
                GeometryHandleId(203),
                GeometryHandleId(204),
            ],
            result_faces: (0..9).map(|i| GeometryHandleId(1000 + i)).collect(),
            result_edges: (0..12).map(|i| GeometryHandleId(2000 + i)).collect(),
        }
    }

    /// Synthetic SweepOpHistoryRecords matching the step-11 spec:
    /// start_cap = [5], end_cap = [6], face_generated = [(0,0,7), (0,1,8)],
    /// every other vector empty.
    fn step11_extrude_history() -> SweepOpHistoryRecords {
        SweepOpHistoryRecords {
            face_generated: vec![
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 0,
                    result_subshape_index: 7,
                },
                HistoryRecord {
                    parent_index: 0,
                    parent_subshape_index: 1,
                    result_subshape_index: 8,
                },
            ],
            start_cap_face_indices: vec![5],
            end_cap_face_indices: vec![6],
            ..Default::default()
        }
    }

    #[test]
    fn populate_extrude_writes_cap_top_for_start_cap_index() {
        let mut table = TopologyAttributeTable::default();
        let layout = extrude_layout_for_step11();
        let feature_id = FeatureId::new("Bracket#realization[0]");
        let history = step11_extrude_history();

        populate_extrude_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
        )
        .expect("step-11 history is well-formed");

        let attr = table
            .lookup(layout.result_faces[5])
            .expect("start_cap_face_indices[0] = 5 should have an entry");
        assert_eq!(attr.role, Role::Cap(CapKind::Top));
        assert_eq!(attr.local_index, 0);
        assert_eq!(attr.feature_id, feature_id);
        assert!(attr.user_label.is_none());
        assert!(attr.mod_history.is_empty());
    }

    #[test]
    fn populate_extrude_writes_cap_bottom_for_end_cap_index() {
        let mut table = TopologyAttributeTable::default();
        let layout = extrude_layout_for_step11();
        let feature_id = FeatureId::new("Bracket#realization[0]");
        let history = step11_extrude_history();

        populate_extrude_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
        )
        .expect("step-11 history is well-formed");

        let attr = table
            .lookup(layout.result_faces[6])
            .expect("end_cap_face_indices[0] = 6 should have an entry");
        assert_eq!(attr.role, Role::Cap(CapKind::Bottom));
        assert_eq!(attr.local_index, 0);
        assert_eq!(attr.feature_id, feature_id);
        assert!(attr.user_label.is_none());
        assert!(attr.mod_history.is_empty());
    }

    #[test]
    fn populate_extrude_writes_side_with_sequential_local_index_for_face_generated() {
        let mut table = TopologyAttributeTable::default();
        let layout = extrude_layout_for_step11();
        let feature_id = FeatureId::new("Bracket#realization[0]");
        let history = step11_extrude_history();

        populate_extrude_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
        )
        .expect("step-11 history is well-formed");

        let side_a = table
            .lookup(layout.result_faces[7])
            .expect("face_generated[0].result_subshape_index = 7 should have an entry");
        assert_eq!(side_a.role, Role::Side);
        assert_eq!(side_a.local_index, 0);
        assert_eq!(side_a.feature_id, feature_id);
        assert!(side_a.mod_history.is_empty());
        assert!(side_a.user_label.is_none());

        let side_b = table
            .lookup(layout.result_faces[8])
            .expect("face_generated[1].result_subshape_index = 8 should have an entry");
        assert_eq!(side_b.role, Role::Side);
        assert_eq!(side_b.local_index, 1);
        assert_eq!(side_b.feature_id, feature_id);
        assert!(side_b.mod_history.is_empty());
        assert!(side_b.user_label.is_none());
    }

    #[test]
    fn populate_extrude_does_not_write_to_result_face_indices_not_in_records() {
        let mut table = TopologyAttributeTable::default();
        let layout = extrude_layout_for_step11();
        let feature_id = FeatureId::new("Bracket#realization[0]");
        let history = step11_extrude_history();

        populate_extrude_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
        )
        .expect("step-11 history is well-formed");

        // Only indices 5, 6, 7, 8 are referenced; 0..=4 must remain unkeyed.
        for unkeyed_idx in [0_usize, 1, 2, 3, 4] {
            assert!(
                table.lookup(layout.result_faces[unkeyed_idx]).is_none(),
                "result face index {unkeyed_idx} should have no attribute entry",
            );
        }
        assert_eq!(
            table.len(),
            4,
            "only the 2 cap faces and 2 side faces should be keyed",
        );
    }

    #[test]
    fn populate_extrude_returns_query_failed_when_start_cap_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = extrude_layout_for_step11();
        let feature_id = FeatureId::new("Bracket#realization[0]");
        let history = SweepOpHistoryRecords {
            start_cap_face_indices: vec![99], // result has only 9 faces.
            ..Default::default()
        };

        let err = populate_extrude_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
        )
        .expect_err("expected QueryFailed for out-of-range start_cap index");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("99"),
                    "error should mention out-of-range index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn populate_extrude_returns_query_failed_when_face_generated_result_index_out_of_range() {
        let mut table = TopologyAttributeTable::default();
        let layout = extrude_layout_for_step11();
        let feature_id = FeatureId::new("Bracket#realization[0]");
        let history = SweepOpHistoryRecords {
            face_generated: vec![HistoryRecord {
                parent_index: 0,
                parent_subshape_index: 0,
                result_subshape_index: 42, // > result faces (9).
            }],
            ..Default::default()
        };

        let err = populate_extrude_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
        )
        .expect_err("expected QueryFailed for out-of-range result_subshape_index");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("42"),
                    "error should mention out-of-range index, got {msg:?}",
                );
            }
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }

    #[test]
    fn populate_extrude_empty_history_is_a_noop() {
        let mut table = TopologyAttributeTable::default();
        let layout = extrude_layout_for_step11();
        let feature_id = FeatureId::new("Bracket#realization[0]");
        let history = SweepOpHistoryRecords::default();

        populate_extrude_attributes(
            &mut table,
            &feature_id,
            &layout.profile_faces,
            &layout.profile_edges,
            &layout.result_faces,
            &layout.result_edges,
            &history,
        )
        .expect("empty history is a no-op");
        assert!(table.is_empty());
    }
}
