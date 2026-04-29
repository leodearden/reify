//! End-to-end integration test for the v0.2 persistent-naming-v2
//! attribute data model + BRepAlgoAPI propagation pipeline (task 2590).
//!
//! PRD docs/prds/v0_2/persistent-naming-v2.md line 93 mandates a
//! single integration test that exercises the data model + propagation
//! end-to-end via the public API. This file is that test.
//!
//! What it covers:
//!
//! 1. Construction of every public v0.2 attribute primitive
//!    (`FeatureId`, `Role`, `CapKind`, `ModEntry`, `TopologyAttribute`,
//!    `TopologyAttributeTable`) using only the `reify_types` re-exports.
//! 2. The `OcctKernelHandle::boolean_fuse_with_history` FFI primitive
//!    returning a `(GeometryHandleId, BooleanOpHistoryRecords)` pair
//!    populated with Modified / Generated / Deleted records.
//! 3. The `reify_eval::propagate_attributes_via_brepalgoapi_history`
//!    helper cloning each parent attribute onto every result face/edge
//!    referenced by a Modified or Generated record, leaving deleted
//!    parents and untouched result sub-shapes alone.
//!
//! Out of scope (per PRD task-1 boundaries documented in
//! `topology_attribute_propagation.rs`):
//!
//! - Selector resolution against attributes (task 2 / #2570).
//! - `mod_history` threading on splits (task 3 / #2571).
//! - Per-op `Role` transformation rules (tasks 5-8).
//! - Auto-population during `Engine::execute_realization_ops` (tasks 5-8).
//!
//! The test is gated on `OCCT_AVAILABLE` mirroring `feature_tag_e2e.rs`
//! and other OCCT-dependent integration tests.

use std::collections::{HashMap, HashSet};

use reify_eval::propagate_attributes_via_brepalgoapi_history;
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_types::{
    BooleanOpParents, FeatureId, GeometryHandleId, GeometryOp,
    RealizationNodeId, Role, TopologyAttribute, TopologyAttributeTable, Value,
};

/// 10×10×10 mm box, expressed in SI metres at the kernel boundary.
/// Same convention as `feature_tag_e2e.rs` and the other OCCT tests.
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

/// PRD-line-93 single integration test.
///
/// Per step-17:
///
/// (1) Spawn an `OcctKernelHandle`, build a left 10mm cube at origin
///     and a right 10mm cube offset (+5,0,0) so the fuse has both
///     shared and outer faces.
/// (2) Extract each box's faces; seed a `TopologyAttributeTable` with
///     one `TopologyAttribute` per parent face (left → "Left#realization[0]",
///     right → "Right#realization[0]", role = `Role::Side`,
///     local_index = the TopExp 0-based index, user_label = None,
///     mod_history empty).
/// (3) Call `kernel.boolean_fuse_with_history(left, right)` →
///     `(result_handle, history)`.
/// (4) Pre-condition: assert every seeded attribute round-trips via
///     `lookup`; `Role::Side` and `FeatureId::From` impls work;
///     `Vec::new()` mod_history is accepted.
/// (5) Run `propagate_attributes_via_brepalgoapi_history(...)`.
/// (6) Post-condition assertions exercising propagation:
///     (a) `table.len()` increased.
///     (b) Every result-face referenced in `face_modified` or
///         `face_generated` has a `lookup`-able entry.
///     (c) Each propagated entry's `feature_id` equals the FeatureId
///         for the originating parent (last-write-wins via the table's
///         overwrite semantics — see the unit test for the same
///         iteration-order rationale).
///     (d) `mod_history` is still empty and `user_label` is still None
///         on propagated entries (task-1 invariant).
///     (e) For at least one Deleted face record, the parent face
///         handle still resolves in the table (the parent entry isn't
///         removed) AND no entry exists in the table for any result-face
///         handle that doesn't appear in Modified or Generated.
#[test]
fn attribute_data_model_and_brepalgoapi_propagation_end_to_end() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    // ─── (1) Build two overlapping cubes via OcctKernelHandle ────────
    // `OcctKernelHandle`'s methods take `&self` (the kernel-thread
    // channel handles all the mutation), so no `mut` needed.
    let kernel = OcctKernelHandle::spawn();

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

    // Pre-extract parent face/edge handles ONCE: the kernel allocates
    // fresh GeometryHandleIds on each `extract_*` call, so we must reuse
    // these vectors as both seeding keys and propagation inputs.
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

    assert_eq!(
        left_face_handles.len(),
        6,
        "a 10mm box should have exactly 6 faces"
    );
    assert_eq!(
        right_face_handles.len(),
        6,
        "a translated 10mm box should still have exactly 6 faces"
    );

    // ─── (2) Seed a TopologyAttributeTable from the public API ───────
    let left_feature_id = FeatureId::from(&RealizationNodeId::new("Left", 0));
    let right_feature_id = FeatureId::from(&RealizationNodeId::new("Right", 0));
    assert_eq!(
        format!("{}", left_feature_id),
        "Left#realization[0]",
        "FeatureId::from(&RealizationNodeId) must produce the PRD-§6.5 path"
    );
    assert_eq!(
        format!("{}", right_feature_id),
        "Right#realization[0]",
        "FeatureId::from(&RealizationNodeId) must produce the PRD-§6.5 path"
    );

    let mut table = TopologyAttributeTable::default();
    assert!(
        table.is_empty(),
        "TopologyAttributeTable::default() should be empty"
    );

    seed_face_attributes(&mut table, &left_face_handles, &left_feature_id);
    seed_face_attributes(&mut table, &right_face_handles, &right_feature_id);

    let seeded_count = table.len();
    assert_eq!(
        seeded_count,
        left_face_handles.len() + right_face_handles.len(),
        "seeding should add one entry per parent face (left {} + right {})",
        left_face_handles.len(),
        right_face_handles.len()
    );

    // ─── (4) Pre-condition smoke tests on the data model ─────────────
    // (Run before history extraction so a data-model regression is
    // caught even if the FFI primitive panics afterwards.)

    // Every seeded attribute round-trips via lookup with the expected
    // feature_id and role.
    for (idx, &face_id) in left_face_handles.iter().enumerate() {
        let attr = table
            .lookup(face_id)
            .expect("seeded left face must round-trip via lookup");
        assert_eq!(attr.feature_id, left_feature_id);
        assert_eq!(attr.role, Role::Side);
        assert_eq!(attr.local_index, idx as u32);
        assert_eq!(attr.user_label, None);
        assert!(attr.mod_history.is_empty());
    }
    for (idx, &face_id) in right_face_handles.iter().enumerate() {
        let attr = table
            .lookup(face_id)
            .expect("seeded right face must round-trip via lookup");
        assert_eq!(attr.feature_id, right_feature_id);
        assert_eq!(attr.role, Role::Side);
        assert_eq!(attr.local_index, idx as u32);
        assert_eq!(attr.user_label, None);
        assert!(attr.mod_history.is_empty());
    }

    // ─── (3) Run boolean_fuse_with_history ───────────────────────────
    let (result_handle, history) = kernel
        .boolean_fuse_with_history(left, right)
        .expect("boolean_fuse_with_history should succeed for overlapping boxes");

    // Pre-extract result face/edge handles ONCE for the same reason
    // we pre-extract parent handles.
    let result_face_handles = kernel
        .extract_faces(result_handle)
        .expect("extract_faces(result) should succeed");
    let result_edge_handles = kernel
        .extract_edges(result_handle)
        .expect("extract_edges(result) should succeed");

    // History must be populated for an overlapping-box fuse.
    assert!(
        !history.face_modified.is_empty() || !history.face_generated.is_empty(),
        "history.face_modified ∪ face_generated should be non-empty for an overlapping-box fuse",
    );

    // ─── (5) Run propagation ─────────────────────────────────────────
    // BooleanOpParents::Binary documents the binary-fuse expectation:
    // `parent_index` 0 == left operand, 1 == right operand.
    let parents = BooleanOpParents::Binary {
        faces: [&left_face_handles, &right_face_handles],
        edges: [&left_edge_handles, &right_edge_handles],
    };

    propagate_attributes_via_brepalgoapi_history(
        &mut table,
        &parents,
        &result_face_handles,
        &result_edge_handles,
        &history,
    )
    .expect("propagation should succeed for a well-formed history");

    // ─── (6) Post-condition assertions ───────────────────────────────

    // (a) table.len() increased.
    assert!(
        table.len() > seeded_count,
        "propagation should record additional entries for result faces \
         (had {seeded_count} seeded, table now has {})",
        table.len()
    );

    // Walk the history in iteration order and remember the LAST record
    // that mentioned each result face. The propagated entry's feature_id
    // must match the parent of that last record (last-write-wins per
    // `TopologyAttributeTable::record`'s overwrite semantics).
    let mut last_face_record: HashMap<u32, u8> = HashMap::new();
    let mut touched_result_face_indices: HashSet<u32> = HashSet::new();
    for record in history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
    {
        last_face_record.insert(record.result_subshape_index, record.parent_index);
        touched_result_face_indices.insert(record.result_subshape_index);
    }

    // (b) + (c) + (d) — every touched result-face has a lookupable entry,
    // its feature_id matches the originating parent, and its
    // mod_history/user_label are unchanged.
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

    // (e) For at least one Deleted face record, the parent face handle
    // still resolves in the table (i.e. parent entries aren't removed
    // when their faces are deleted from the result). Then assert that
    // result faces NOT in Modified ∪ Generated have no entry — this
    // pins the "no spurious entries" invariant.
    if !history.face_deleted.is_empty() {
        let parent_face_slices = parents.face_slices();
        for deleted in history.face_deleted.iter() {
            let parent_idx = deleted.parent_index as usize;
            let parent_subshape_idx = deleted.parent_subshape_index as usize;
            let parent_handle = parent_face_slices[parent_idx][parent_subshape_idx];
            assert!(
                table.lookup(parent_handle).is_some(),
                "parent face handle for deleted record (parent {}, subshape {}) \
                 must still resolve in the table — parents aren't removed by \
                 propagation, only result entries are added",
                parent_idx,
                parent_subshape_idx,
            );
        }
    }

    // No spurious entries: result faces NOT in Modified ∪ Generated
    // have no entry in the table.
    for (idx, &result_face_id) in result_face_handles.iter().enumerate() {
        if touched_result_face_indices.contains(&(idx as u32)) {
            continue;
        }
        assert!(
            table.lookup(result_face_id).is_none(),
            "result face {:?} (index {}) was not in Modified/Generated \
             history, so propagation should NOT have written an entry for it",
            result_face_id,
            idx,
        );
    }

    // Edges: the integration test only seeds faces, so propagation
    // should not write any entries for result edges.
    for &result_edge_id in result_edge_handles.iter() {
        assert!(
            table.lookup(result_edge_id).is_none(),
            "result edge {:?} should not have an entry — only faces were seeded",
            result_edge_id,
        );
    }
}
