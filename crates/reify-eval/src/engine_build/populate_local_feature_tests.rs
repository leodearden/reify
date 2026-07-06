    use reify_ir::{
        AttributeHistory, FeatureId, GeometryHandleId, GeometryOp, HistoryRecord,
        LocalFeatureOpHistoryRecords, QueryError, Role, TopologyAttribute, TopologyAttributeTable,
        Value,
    };
    use reify_test_support::mocks::MockGeometryKernel;

    use super::populate_attribute_history;

    fn fillet_fid() -> FeatureId {
        FeatureId::realization("Fillet", 0)
    }

    fn make_attr(fid: &FeatureId, role: Role, local_index: u32) -> TopologyAttribute {
        TopologyAttribute {
            feature_id: fid.clone(),
            role,
            local_index,
            user_label: None,
            mod_history: vec![],
        }
    }

    fn hrec(parent_subshape_index: u32, result_subshape_index: u32) -> HistoryRecord {
        HistoryRecord {
            parent_index: 0,
            parent_subshape_index,
            result_subshape_index,
        }
    }

    // -----------------------------------------------------------------------
    // Fillet: face_generated cross-kind origination (1 parent edge → 2
    // originated result faces). Option B (esc-4832-140): generated faces
    // are owned by the creating (fillet) feature, not cloned from the
    // parent edge — the parent_edge attribute seeded below is a distractor
    // that must NOT leak into the result.
    // -----------------------------------------------------------------------
    #[test]
    fn fillet_local_feature_dispatches_and_originates_face_generated() {
        // Handles
        let target = GeometryHandleId(1);
        let result = GeometryHandleId(100);
        let parent_face = GeometryHandleId(10);
        let parent_edge = GeometryHandleId(20);
        let parent_vertex = GeometryHandleId(30);
        let result_face_a = GeometryHandleId(110);
        let result_face_b = GeometryHandleId(111);
        let result_edge = GeometryHandleId(120);

        // Mock: target extraction + result extraction
        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(target, vec![parent_face])
            .with_extracted_edges(target, vec![parent_edge])
            .with_extracted_vertices(target, vec![parent_vertex])
            .with_extracted_faces(result, vec![result_face_a, result_face_b])
            .with_extracted_edges(result, vec![result_edge]);

        // Seed: parent_edge has an attribute (its Role::NewEdge propagates to 2 result faces)
        let fid = FeatureId::realization("Box", 0);
        let splitting_fid = fillet_fid();
        let mut table = TopologyAttributeTable::default();
        table.record(parent_face, make_attr(&fid, Role::Side, 0));
        table.record(parent_edge, make_attr(&fid, Role::NewEdge, 5));

        // History: one parent edge → two result faces (cross-kind split)
        let history = LocalFeatureOpHistoryRecords {
            face_generated: vec![hrec(0, 0), hrec(0, 1)],
            ..Default::default()
        };
        let attr_history = AttributeHistory::LocalFeature(history);

        let geom_op = GeometryOp::Fillet {
            target,
            edges: vec![],
            radius: Value::Real(0.001),
        };

        populate_attribute_history(
            &mut table,
            &mut kernel,
            &splitting_fid,
            &geom_op,
            result,
            &attr_history,
        )
        .expect("fillet LocalFeature dispatch should succeed");

        // Both result faces are ORIGINATED with a fresh attribute owned by
        // the fillet — not cloned from parent_edge's Box/NewEdge/5 attr.
        for (handle, expected_local_index) in [(result_face_a, 0u32), (result_face_b, 1u32)] {
            let attr = table
                .lookup(handle)
                .unwrap_or_else(|| panic!("{handle:?} must have attr after fillet propagation"));
            assert_eq!(
                attr.feature_id, splitting_fid,
                "generated face must be owned by the creating feature, not the parent edge's"
            );
            assert_eq!(attr.role, Role::LocalFeatureFace);
            assert_eq!(
                attr.local_index, expected_local_index,
                "local_index must run 0..n within the face_generated stream"
            );
            assert!(
                attr.mod_history.is_empty(),
                "originated faces are created, not split; mod_history must be empty, got {:?}",
                attr.mod_history
            );
        }
    }

    // -----------------------------------------------------------------------
    // Chamfer: edge_generated cross-kind pass-through (1 parent edge → 1 result edge)
    // -----------------------------------------------------------------------
    #[test]
    fn chamfer_local_feature_dispatches_and_propagates_edge_modified_passthrough() {
        let target = GeometryHandleId(2);
        let result = GeometryHandleId(200);
        let parent_face = GeometryHandleId(11);
        let parent_edge = GeometryHandleId(21);
        let parent_vertex = GeometryHandleId(31);
        let result_face = GeometryHandleId(210);
        let result_edge = GeometryHandleId(220);

        let mut kernel = MockGeometryKernel::new()
            .with_extracted_faces(target, vec![parent_face])
            .with_extracted_edges(target, vec![parent_edge])
            .with_extracted_vertices(target, vec![parent_vertex])
            .with_extracted_faces(result, vec![result_face])
            .with_extracted_edges(result, vec![result_edge]);

        let fid = FeatureId::realization("Box", 0);
        let splitting_fid = fillet_fid();
        let mut table = TopologyAttributeTable::default();
        table.record(parent_edge, make_attr(&fid, Role::NewEdge, 3));

        // edge_modified: 1 parent edge → 1 result edge (pure pass-through)
        let history = LocalFeatureOpHistoryRecords {
            edge_modified: vec![hrec(0, 0)],
            ..Default::default()
        };
        let attr_history = AttributeHistory::LocalFeature(history);

        let geom_op = GeometryOp::Chamfer {
            target,
            edges: vec![],
            distance: Value::Real(0.001),
        };

        populate_attribute_history(
            &mut table,
            &mut kernel,
            &splitting_fid,
            &geom_op,
            result,
            &attr_history,
        )
        .expect("chamfer LocalFeature dispatch should succeed");

        let attr = table
            .lookup(result_edge)
            .expect("result_edge must have attr after chamfer propagation");
        assert_eq!(attr.feature_id, fid);
        assert_eq!(attr.role, Role::NewEdge);
        assert_eq!(attr.local_index, 3);
        assert!(
            attr.mod_history.is_empty(),
            "1→1 pass-through must not add ModEntry; got {:?}",
            attr.mod_history
        );
    }

    // -----------------------------------------------------------------------
    // Guard: non-Fillet/Chamfer GeometryOp with AttributeHistory::LocalFeature
    //        must return Err(QueryError::QueryFailed).
    // -----------------------------------------------------------------------
    #[test]
    fn local_feature_with_non_fillet_chamfer_geom_op_returns_query_failed() {
        let profile = GeometryHandleId(4);
        let result = GeometryHandleId(300);

        // Empty mock — the error must fire before any kernel extraction.
        let mut kernel = MockGeometryKernel::new();
        let mut table = TopologyAttributeTable::default();
        let fid = fillet_fid();

        let attr_history = AttributeHistory::LocalFeature(LocalFeatureOpHistoryRecords::default());

        // Use Extrude (not Fillet/Chamfer) as the mismatched op.
        let geom_op = GeometryOp::Extrude {
            profile,
            distance: Value::Real(0.01),
        };

        let err = populate_attribute_history(
            &mut table,
            &mut kernel,
            &fid,
            &geom_op,
            result,
            &attr_history,
        )
        .expect_err("non-Fillet/Chamfer op with LocalFeature history must return QueryFailed");

        match err {
            QueryError::QueryFailed(_) => {}
            other => panic!("expected QueryError::QueryFailed, got {other:?}"),
        }
    }
