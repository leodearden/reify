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
    BooleanOpHistoryRecords, GeometryHandleId, HistoryRecord, QueryError, TopologyAttributeTable,
};

/// Propagate parent topology attributes onto the result of a `BRepAlgoAPI`
/// boolean operation, using the Modified / Generated / Deleted records the
/// algorithm exposes.
///
/// Inputs:
/// - `table`: the `TopologyAttributeTable` to update in place. Parent
///   entries are read; new entries are written for each Modified/Generated
///   result sub-shape whose corresponding parent had an attribute.
/// - `parent_face_handles`: per-parent face-handle vectors (i.e.
///   `[left_faces, right_faces]`) in canonical TopExp order. Caller
///   extracts these via `kernel.extract_faces(parent)` once and reuses
///   the same vectors as the table-seeding keys.
/// - `parent_edge_handles`: as above, but for edges.
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
/// guarantees in-range indices, so this is a defense-in-depth path.
///
/// Cross-references PRD docs/prds/v0_2/persistent-naming-v2.md (a)+(c)+(d)
/// of decomposition-plan task 1 (lines 89-103).
pub fn propagate_attributes_via_brepalgoapi_history(
    table: &mut TopologyAttributeTable,
    parent_face_handles: &[Vec<GeometryHandleId>; 2],
    parent_edge_handles: &[Vec<GeometryHandleId>; 2],
    result_face_handles: &[GeometryHandleId],
    result_edge_handles: &[GeometryHandleId],
    history: &BooleanOpHistoryRecords,
) -> Result<(), QueryError> {
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
    parent_handles: &[Vec<GeometryHandleId>; 2],
    result_handles: &[GeometryHandleId],
    record: &HistoryRecord,
    kind: &str,
) -> Result<(), QueryError> {
    let parent_idx = record.parent_index as usize;
    if parent_idx >= parent_handles.len() {
        return Err(QueryError::QueryFailed(format!(
            "BRepAlgoAPI history {kind} record has parent_index {parent_idx} \
             but only 2 parents are tracked",
        )));
    }
    let parent_vec = &parent_handles[parent_idx];
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
    use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
    use reify_types::{
        FeatureId, GeometryHandleId, GeometryKernel, GeometryOp, RealizationNodeId, Role,
        TopologyAttribute, TopologyAttributeTable, Value,
    };

    use super::propagate_attributes_via_brepalgoapi_history;

    /// 10×10×10 mm box, expressed in SI metres at the kernel boundary.
    const BOX_SIDE_M: f64 = 10.0e-3;

    fn ten_mm_box_op() -> GeometryOp {
        GeometryOp::Box {
            width: Value::Real(BOX_SIDE_M),
            height: Value::Real(BOX_SIDE_M),
            depth: Value::Real(BOX_SIDE_M),
        }
    }

    /// Seed `table` with one `TopologyAttribute` per provided parent face
    /// handle, rooted at `feature_id`. Each face gets `Role::Side` and a
    /// `local_index` matching its position in the input slice.
    fn seed_face_attributes(
        table: &mut TopologyAttributeTable,
        face_handles: &[GeometryHandleId],
        feature_id: &FeatureId,
    ) {
        for (idx, &face_id) in face_handles.iter().enumerate() {
            let attr = TopologyAttribute {
                feature_id: feature_id.clone(),
                role: Role::Side,
                local_index: idx as u32,
                user_label: None,
                mod_history: Vec::new(),
            };
            table.record(face_id, attr);
        }
    }

    /// Core post-condition test for the propagation helper.
    ///
    /// Steps:
    /// 1. Build two overlapping 10mm boxes via `OcctKernelHandle`.
    /// 2. Extract each parent's face/edge handles ONCE and seed the
    ///    `TopologyAttributeTable` with one entry per face: left's get
    ///    `FeatureId::from(&RealizationNodeId::new("L", 0))`, right's get
    ///    `FeatureId::from(&RealizationNodeId::new("R", 0))`.
    /// 3. Call `boolean_fuse_with_history(left, right)`.
    /// 4. Extract result face/edge handles ONCE and pass them, along
    ///    with the parent vectors, to
    ///    `propagate_attributes_via_brepalgoapi_history(...)`.
    /// 5. Assert: every result-face referenced in `face_modified` or
    ///    `face_generated` has a `lookup`-able entry whose `feature_id`
    ///    matches the originating parent (via the record's `parent_index`).
    /// 6. Assert: each propagated entry's `mod_history` is empty and
    ///    `user_label` is None — task 1 keeps clones unchanged.
    #[test]
    fn propagation_clones_parent_attribute_onto_modified_and_generated_result_faces() {
        if !OCCT_AVAILABLE {
            return;
        }

        let mut kernel = OcctKernelHandle::spawn();

        // Two overlapping cubes (right offset by +5mm in X).
        let left = kernel
            .execute(&ten_mm_box_op())
            .expect("left box should build")
            .id;
        let right_origin = kernel
            .execute(&ten_mm_box_op())
            .expect("right box should build")
            .id;
        let right = kernel
            .execute(&GeometryOp::Translate {
                target: right_origin,
                dx: 5.0e-3,
                dy: 0.0,
                dz: 0.0,
            })
            .expect("right translate should build")
            .id;

        // Extract each parent's face/edge handles ONCE and reuse the
        // same vectors as both seeding keys and propagation inputs.
        let left_face_handles = kernel
            .extract_faces(left)
            .expect("extract_faces(left) should succeed");
        let right_face_handles = kernel
            .extract_faces(right)
            .expect("extract_faces(right) should succeed");
        let left_edge_handles = kernel
            .extract_edges(left)
            .expect("extract_edges(left) should succeed");
        let right_edge_handles = kernel
            .extract_edges(right)
            .expect("extract_edges(right) should succeed");

        let left_feature_id = FeatureId::from(&RealizationNodeId::new("L", 0));
        let right_feature_id = FeatureId::from(&RealizationNodeId::new("R", 0));

        let mut table = TopologyAttributeTable::default();
        seed_face_attributes(&mut table, &left_face_handles, &left_feature_id);
        seed_face_attributes(&mut table, &right_face_handles, &right_feature_id);

        let seeded_count = table.len();
        assert_eq!(
            seeded_count,
            left_face_handles.len() + right_face_handles.len(),
            "seeding should add one entry per parent face"
        );

        let (result_handle, history) = kernel
            .boolean_fuse_with_history(left, right)
            .expect("boolean_fuse_with_history should succeed for overlapping boxes");

        // Extract result face/edge handles ONCE, then pass them to
        // propagation. Subsequent extract_* calls would allocate fresh
        // ids (kernel does not dedupe), so we capture once and reuse.
        let result_face_handles = kernel
            .extract_faces(result_handle)
            .expect("extract_faces(result) should succeed");
        let result_edge_handles = kernel
            .extract_edges(result_handle)
            .expect("extract_edges(result) should succeed");

        let parent_face_handles = [left_face_handles.clone(), right_face_handles.clone()];
        let parent_edge_handles = [left_edge_handles.clone(), right_edge_handles.clone()];

        propagate_attributes_via_brepalgoapi_history(
            &mut table,
            &parent_face_handles,
            &parent_edge_handles,
            &result_face_handles,
            &result_edge_handles,
            &history,
        )
        .expect("propagation should succeed for a well-formed history");

        // (d) Table now contains entries for at least some result-face
        //     handles (those touched by Modified/Generated records).
        assert!(
            table.len() > seeded_count,
            "propagation should record additional entries for result faces \
             (had {seeded_count} seeded, table now has {})",
            table.len()
        );

        // (e) Walk the history in iteration order and remember the
        //     LAST record that mentioned each result face. The
        //     propagated entry's `feature_id` must match the parent of
        //     that last record (last-write-wins per
        //     `TopologyAttributeTable::record`'s overwrite semantics —
        //     a result face shared between left and right gets the
        //     parent that was written last).
        use std::collections::HashMap;
        let mut last_face_record: HashMap<u32, u8> = HashMap::new();
        for record in history
            .face_modified
            .iter()
            .chain(history.face_generated.iter())
        {
            last_face_record.insert(record.result_subshape_index, record.parent_index);
        }

        for (&result_subshape_index, &expected_parent_index) in last_face_record.iter() {
            let result_face_id = result_face_handles[result_subshape_index as usize];
            let propagated = table.lookup(result_face_id).unwrap_or_else(|| {
                panic!(
                    "result face {:?} (subshape index {}) should have a propagated attribute",
                    result_face_id, result_subshape_index
                )
            });
            let expected_feature_id = match expected_parent_index {
                0 => &left_feature_id,
                1 => &right_feature_id,
                other => panic!("unexpected parent_index {other} in face history record"),
            };
            assert_eq!(
                &propagated.feature_id, expected_feature_id,
                "result face index {} should carry feature_id {} (last-write-wins from parent {})",
                result_subshape_index, expected_feature_id, expected_parent_index,
            );
            // (f) mod_history empty, user_label None — task-1 invariant.
            assert!(
                propagated.mod_history.is_empty(),
                "task-1 propagation leaves mod_history empty (got {:?})",
                propagated.mod_history
            );
            assert_eq!(
                propagated.user_label, None,
                "task-1 propagation leaves user_label as None"
            );
        }

        // Sanity-check the iteration covered something.
        assert!(
            !last_face_record.is_empty(),
            "history should contain at least one face Modified/Generated record"
        );

        // Repeat the per-record assertion for edges.
        for record in history
            .edge_modified
            .iter()
            .chain(history.edge_generated.iter())
        {
            let result_edge_id =
                result_edge_handles[record.result_subshape_index as usize];
            // Edges weren't seeded in this test, so propagation should
            // be a no-op for them (parent edge has no entry to clone).
            assert!(
                table.lookup(result_edge_id).is_none(),
                "edges weren't seeded → propagation should not write entries \
                 for result edges (got {:?})",
                table.lookup(result_edge_id),
            );
        }
    }
}
