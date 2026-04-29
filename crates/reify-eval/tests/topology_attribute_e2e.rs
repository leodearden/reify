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

use reify_eval::{
    AttributeQuery, AttributeResolution, propagate_attributes_via_brepalgoapi_history,
    resolve_unique_by_attribute,
};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernelHandle};
use reify_types::{
    BooleanOpParents, DiagnosticCode, FeatureId, GeometryHandleId, GeometryOp, ModEntry,
    RealizationNodeId, Role, SourceSpan, TopologyAttribute, TopologyAttributeTable, Value,
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

    // The fuse op's FeatureId is passed as `splitting_feature_id` and
    // stamped onto each `ModEntry` appended on splits. The integration
    // test only seeds parent-face attributes; it does NOT exercise the
    // resolver's AmbiguousAfterSplit path here (that's the dedicated
    // mod-history e2e test below).
    let fuse_feature_id = FeatureId::new("Fuse#realization[0]");
    propagate_attributes_via_brepalgoapi_history(
        &mut table,
        &parents,
        &result_face_handles,
        &result_edge_handles,
        &history,
        &fuse_feature_id,
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
    // its feature_id matches the originating parent, and its parent-key
    // fields propagate unchanged. `mod_history` is augmented per the v0.2
    // task-3 contract: split parents (count > 1 across same-kind Modified
    // ∪ Generated) get a fresh `ModEntry { splitting_feature_id, split_index }`
    // appended; single-result parents remain pure pass-through.
    let face_child_counts: HashMap<(u8, u32), usize> = {
        let mut counts: HashMap<(u8, u32), usize> = HashMap::new();
        for rec in history
            .face_modified
            .iter()
            .chain(history.face_generated.iter())
        {
            *counts
                .entry((rec.parent_index, rec.parent_subshape_index))
                .or_insert(0) += 1;
        }
        counts
    };
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
        // Find the parent that wrote this propagated entry. The
        // last-write-wins iteration matches the propagation walk, so for the
        // mod_history assertion we look up the last record's parent key.
        // For non-split parents we expect mod_history empty; for split
        // parents we expect a non-empty mod_history whose tail entry's
        // splitting_feature_id matches the fuse_feature_id passed to
        // propagation. (The dedicated mod-history e2e test pins the
        // per-child split_index ordering.)
        let parent_key_for_last_record = history
            .face_modified
            .iter()
            .chain(history.face_generated.iter())
            .filter(|rec| rec.result_subshape_index == result_subshape_index)
            .last()
            .map(|rec| (rec.parent_index, rec.parent_subshape_index))
            .expect("touched index must originate from at least one record");
        let parent_count = face_child_counts
            .get(&parent_key_for_last_record)
            .copied()
            .unwrap_or(0);
        if parent_count > 1 {
            assert!(
                !propagated.mod_history.is_empty(),
                "split parent {:?} (count={parent_count}) should propagate a non-empty mod_history",
                parent_key_for_last_record
            );
            let tail = propagated
                .mod_history
                .last()
                .expect("non-empty mod_history must have a tail");
            assert_eq!(
                tail.splitting_feature_id, fuse_feature_id,
                "split-induced ModEntry must stamp the propagation's splitting_feature_id"
            );
        } else {
            assert!(
                propagated.mod_history.is_empty(),
                "non-split parent (count={parent_count}) propagation must leave mod_history empty (got {:?})",
                propagated.mod_history
            );
        }
        assert_eq!(
            propagated.user_label, None,
            "propagation preserves user_label = None from the seeded parents"
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

/// step-15 (task #2653) — end-to-end mod_history threading and resolver
/// AmbiguousAfterSplit clustering.
///
/// Reuses the existing two-cube fuse fixture and seeds parent face
/// attributes the same way as the previous test. After propagation:
///
///   (a) For each parent with count > 1 across face_modified ∪
///       face_generated: each child carries a `mod_history` whose tail
///       entry is `ModEntry { splitting_feature_id == fuse_feature_id,
///       split_index = i }` for i = 0..count, in records-encounter order
///       (Modified records first, then Generated). The child's
///       parent-key fields (feature_id, role, local_index, user_label)
///       inherit verbatim from the parent.
///   (b) For each parent with count == 1: the single child's
///       `mod_history.is_empty()` (pure pass-through, no ModEntry).
///   (c) Pick the FIRST parent with count > 1. Build an `AttributeQuery`
///       from its `(feature_id, role, local_index)` and pass
///       `result_face_handles` as candidates. The resolver must return
///       `AttributeResolution::AmbiguousAfterSplit { children }` whose
///       handles match the SET of children we identified in (a). A
///       `TopologyAttributeStale` diagnostic with the "split children"
///       message sub-form must accompany the resolution.
///
/// If OCCT's actual fuse output for two cubes offset by half-width
/// produces NO parent face splits — possible for an aligned fuse where
/// every overlapping parent face is either fully Modified into one
/// result face, fully Deleted, or absent from history — sub-clauses (a)
/// and (c) gracefully no-op (with eprintln so the skip is visible in
/// CI). Sub-clause (b) ALWAYS runs; non-split parents must always have
/// empty mod_history regardless of OCCT's particular split topology.
/// Step-16's orthogonal-slab variant covers the explicit-split path
/// when this fixture doesn't naturally exercise it.
#[test]
fn mod_history_threading_through_propagation_and_resolver_end_to_end() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    // ─── Setup mirrors the existing fixture ───────────────────────────
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
    let left_face_handles = kernel.extract_faces(left).unwrap();
    let right_face_handles = kernel.extract_faces(right).unwrap();
    let left_edge_handles = kernel.extract_edges(left).unwrap();
    let right_edge_handles = kernel.extract_edges(right).unwrap();
    let left_feature_id = FeatureId::from(&RealizationNodeId::new("Left", 0));
    let right_feature_id = FeatureId::from(&RealizationNodeId::new("Right", 0));
    let mut table = TopologyAttributeTable::default();
    seed_face_attributes(&mut table, &left_face_handles, &left_feature_id);
    seed_face_attributes(&mut table, &right_face_handles, &right_feature_id);

    let (result_handle, history) = kernel
        .boolean_fuse_with_history(left, right)
        .expect("boolean_fuse_with_history should succeed");
    let result_face_handles = kernel.extract_faces(result_handle).unwrap();
    let result_edge_handles = kernel.extract_edges(result_handle).unwrap();

    let parents = BooleanOpParents::Binary {
        faces: [&left_face_handles, &right_face_handles],
        edges: [&left_edge_handles, &right_edge_handles],
    };
    let fuse_feature_id = FeatureId::new("Fuse#realization[0]");
    propagate_attributes_via_brepalgoapi_history(
        &mut table,
        &parents,
        &result_face_handles,
        &result_edge_handles,
        &history,
        &fuse_feature_id,
    )
    .expect("propagation should succeed");

    // ─── Build per-parent child enumeration in records-encounter order ─
    // Walk face_modified.iter().chain(face_generated.iter()) in the same
    // order the propagator did, accumulating each parent's children with
    // their assigned split_index (0, 1, 2, …).
    //
    // Caveat: a single result_subshape_index can appear in records for
    // MULTIPLE parents (e.g. an internal face that was deleted from one
    // operand and "modified" from the other, OCCT's history may emit
    // both). Last-write-wins propagation means the table's entry for
    // that result face reflects only the LAST parent to write. We track
    // the LAST writing parent per result_subshape_index here so the
    // assertions below can skip non-authoritative writes — those
    // shared-result-face stamps are the table's contract under
    // last-write-wins (which the previous test pinned), not a regression
    // of mod_history threading.
    let mut children_per_parent: HashMap<(u8, u32), Vec<u32>> = HashMap::new();
    let mut last_writer_for_result: HashMap<u32, (u8, u32)> = HashMap::new();
    for rec in history
        .face_modified
        .iter()
        .chain(history.face_generated.iter())
    {
        children_per_parent
            .entry((rec.parent_index, rec.parent_subshape_index))
            .or_default()
            .push(rec.result_subshape_index);
        last_writer_for_result.insert(
            rec.result_subshape_index,
            (rec.parent_index, rec.parent_subshape_index),
        );
    }

    let parent_face_slices = parents.face_slices();
    let mut split_parent_with_children: Option<((u8, u32), Vec<u32>)> = None;

    // ─── (a) + (b): mod_history per child for split vs non-split ─────
    for (&parent_key, child_result_indices) in children_per_parent.iter() {
        let count = child_result_indices.len();
        let parent_handle = parent_face_slices[parent_key.0 as usize][parent_key.1 as usize];
        let parent_attr = table.lookup(parent_handle).expect(
            "seeded parent face must still be in the table after propagation \
             (parents are never removed, only result entries are added)",
        );
        let parent_feature_id = parent_attr.feature_id.clone();
        let parent_role = parent_attr.role;
        let parent_local_index = parent_attr.local_index;
        let parent_user_label = parent_attr.user_label.clone();
        let parent_prior_history = parent_attr.mod_history.clone();
        if count > 1 {
            // (a) Split parent: each child carries a fresh ModEntry whose
            // split_index follows records-encounter order. Parent-key
            // fields inherit verbatim. Skip children where ANOTHER parent
            // was the last writer (last-write-wins) — those are pinned by
            // the previous integration test.
            let mut authoritative_children: Vec<u32> = Vec::new();
            for (split_index, &result_subshape_index) in child_result_indices.iter().enumerate() {
                if last_writer_for_result.get(&result_subshape_index) != Some(&parent_key) {
                    // Another parent is the authoritative writer for this
                    // result face. The split_index this parent assigned
                    // is overwritten in the table; skip per-entry
                    // assertions for this child.
                    continue;
                }
                authoritative_children.push(result_subshape_index);
                let child_handle = result_face_handles[result_subshape_index as usize];
                let child_attr = table.lookup(child_handle).unwrap_or_else(|| {
                    panic!(
                        "split child (parent={:?}, result_subshape_index={}) must have a \
                         propagated entry",
                        parent_key, result_subshape_index
                    )
                });
                assert_eq!(
                    child_attr.feature_id, parent_feature_id,
                    "split child inherits parent feature_id verbatim"
                );
                assert_eq!(
                    child_attr.role, parent_role,
                    "split child inherits parent role verbatim"
                );
                assert_eq!(
                    child_attr.local_index, parent_local_index,
                    "split child inherits parent local_index verbatim"
                );
                assert_eq!(
                    child_attr.user_label, parent_user_label,
                    "split child inherits parent user_label verbatim"
                );
                let expected_tail = ModEntry {
                    splitting_feature_id: fuse_feature_id.clone(),
                    split_index: split_index as u32,
                };
                let actual_tail = child_attr
                    .mod_history
                    .last()
                    .expect("split child mod_history must be non-empty");
                assert_eq!(
                    actual_tail, &expected_tail,
                    "split child {} of parent {:?} must carry tail {:?}",
                    split_index, parent_key, expected_tail
                );
                // mod_history prefix must equal the parent's prior history
                // (preserved verbatim; new ModEntry is APPENDED).
                let prefix_len = child_attr.mod_history.len() - 1;
                assert_eq!(
                    &child_attr.mod_history[..prefix_len],
                    parent_prior_history.as_slice(),
                    "split child mod_history prefix must equal parent's prior history"
                );
            }
            // Remember the FIRST split parent that has ≥ 2 authoritative
            // children (i.e. children this parent actually owns in the
            // table) for the resolver query in clause (c). Without ≥ 2
            // authoritative children the resolver cannot witness the
            // cluster — every entry in the table for those children
            // attributes them to a DIFFERENT parent.
            if split_parent_with_children.is_none() && authoritative_children.len() >= 2 {
                split_parent_with_children = Some((parent_key, authoritative_children));
            }
        } else {
            // (b) Non-split parent: child's mod_history is the parent's
            // mod_history verbatim — no new ModEntry appended. For the
            // existing fixture (no prior splits), this is empty.
            // Skip if another parent is the authoritative writer
            // (last-write-wins overwrote this parent's pass-through).
            let result_subshape_index = child_result_indices[0];
            if last_writer_for_result.get(&result_subshape_index) != Some(&parent_key) {
                continue;
            }
            let child_handle = result_face_handles[result_subshape_index as usize];
            let child_attr = table.lookup(child_handle).unwrap_or_else(|| {
                panic!(
                    "non-split child (parent={:?}, result_subshape_index={}) must have a \
                     propagated entry",
                    parent_key, result_subshape_index
                )
            });
            assert_eq!(
                child_attr.mod_history, parent_prior_history,
                "non-split child mod_history must equal parent's prior history (no new \
                 ModEntry appended; count=1 means pure pass-through)"
            );
        }
    }

    // ─── (c) Resolver clustering on the first split parent ───────────
    let Some((split_parent_key, child_result_indices)) = split_parent_with_children else {
        eprintln!(
            "note: this OCCT output had no face splits (every parent has count==1 across \
             face_modified ∪ face_generated); resolver clustering not exercised by this fixture. \
             See `mod_history_threading_with_orthogonal_slabs` for explicit-split coverage."
        );
        return;
    };

    let split_parent_handle =
        parent_face_slices[split_parent_key.0 as usize][split_parent_key.1 as usize];
    let split_parent_attr = table
        .lookup(split_parent_handle)
        .expect("split-parent attribute must round-trip");
    let query = AttributeQuery {
        user_label: split_parent_attr.user_label.clone(),
        role_and_index: Some((split_parent_attr.role, split_parent_attr.local_index)),
        feature_id: Some(split_parent_attr.feature_id.clone()),
    };
    let mut diagnostics = Vec::new();
    let resolution = resolve_unique_by_attribute(
        &table,
        &result_face_handles,
        &query,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    let expected_children: HashSet<GeometryHandleId> = child_result_indices
        .iter()
        .map(|&i| result_face_handles[i as usize])
        .collect();
    match &resolution {
        AttributeResolution::AmbiguousAfterSplit { children } => {
            let actual: HashSet<GeometryHandleId> = children.iter().copied().collect();
            assert_eq!(
                actual, expected_children,
                "AmbiguousAfterSplit children must equal the propagated child set for the \
                 split parent {:?}",
                split_parent_key
            );
        }
        other => panic!(
            "expected AmbiguousAfterSplit for split parent {:?}, got {:?}",
            split_parent_key, other
        ),
    }
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one TopologyAttributeStale diagnostic for the split-children resolution"
    );
    let diag = &diagnostics[0];
    assert_eq!(diag.code, Some(DiagnosticCode::TopologyAttributeStale));
    assert!(
        diag.message.contains("split children"),
        "diagnostic message must mention 'split children', got: {}",
        diag.message
    );
}
