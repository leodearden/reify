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

    /// Seed `table` with one `TopologyAttribute` per face of `shape`,
    /// rooted at `feature_id`. Returns the parent face handle vector
    /// (in TopExp order) so the test can later look up how propagation
    /// routed each parent's entry onto the result.
    fn seed_face_attributes(
        kernel: &mut OcctKernelHandle,
        table: &mut TopologyAttributeTable,
        shape: GeometryHandleId,
        feature_id: FeatureId,
    ) -> Vec<GeometryHandleId> {
        let face_handles = kernel
            .extract_faces(shape)
            .expect("extract_faces on a box should succeed");
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
        face_handles
    }

    /// Core post-condition test for the propagation helper.
    ///
    /// Steps:
    /// 1. Build two overlapping 10mm boxes via `OcctKernelHandle`.
    /// 2. Hand-seed the `TopologyAttributeTable` with one entry per
    ///    parent face: left's get `FeatureId::from(&RealizationNodeId::new("L", 0))`,
    ///    right's get `FeatureId::from(&RealizationNodeId::new("R", 0))`.
    /// 3. Call `boolean_fuse_with_history(left, right)`.
    /// 4. Call `propagate_attributes_via_brepalgoapi_history(...)`.
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

        let left_feature_id = FeatureId::from(&RealizationNodeId::new("L", 0));
        let right_feature_id = FeatureId::from(&RealizationNodeId::new("R", 0));

        let mut table = TopologyAttributeTable::default();
        let left_face_handles =
            seed_face_attributes(&mut kernel, &mut table, left, left_feature_id.clone());
        let right_face_handles =
            seed_face_attributes(&mut kernel, &mut table, right, right_feature_id.clone());

        let seeded_count = table.len();
        assert_eq!(
            seeded_count,
            left_face_handles.len() + right_face_handles.len(),
            "seeding should add one entry per parent face"
        );

        let (result_handle, history) = kernel
            .boolean_fuse_with_history(left, right)
            .expect("boolean_fuse_with_history should succeed for overlapping boxes");

        propagate_attributes_via_brepalgoapi_history(
            &mut kernel,
            &mut table,
            &[left, right],
            result_handle,
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

        // Re-extract result faces in canonical order so we can resolve
        // each `result_subshape_index` back to the right handle id.
        let result_face_handles = kernel
            .extract_faces(result_handle)
            .expect("extract_faces on the fused result should succeed");

        // (e) Each propagated entry's feature_id matches the parent
        //     index in the originating history record.
        for record in history
            .face_modified
            .iter()
            .chain(history.face_generated.iter())
        {
            let result_face_id =
                result_face_handles[record.result_subshape_index as usize];
            let propagated = table.lookup(result_face_id).unwrap_or_else(|| {
                panic!(
                    "result face {:?} (record {:?}) should have a propagated attribute",
                    result_face_id, record
                )
            });
            let expected_feature_id = match record.parent_index {
                0 => &left_feature_id,
                1 => &right_feature_id,
                other => panic!("unexpected parent_index {other} in face history record"),
            };
            assert_eq!(
                &propagated.feature_id, expected_feature_id,
                "result face from parent {} should carry feature_id {} (record {:?})",
                record.parent_index, expected_feature_id, record
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
    }
}
