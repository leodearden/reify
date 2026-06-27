/// Task 4654 (R3a): carried-topology bundle for result values.
///
/// Defines [`CarriedTopology`] — the kernel-free, selector-resolvable topology
/// bundle that result values (ModalResult, ElasticResult, …) carry so the
/// eval-path resolver (R3b, a separate task — the CONSUMER) can resolve
/// `faces_by_normal(part,+Z,tol)` against baked data, never OCCT.
///
/// # Design decisions
///
/// * **PER-FACE normals** (Q3): keyed by `GeometryHandleId`, same as
///   `BoundaryAssociation::OnFace`. Only per-face normals reproduce
///   `faces_by_normal`'s kernel-side selection — a node on two faces has an
///   ambiguous per-node normal (PRD §7.2/§9).
///
/// * **One shared type**: `CarriedTopology` is not modal-only; the same
///   `from_realized_mesh` builder serves modal and FEA result models (DD-3).
///
/// * **Value round-trip via synthetic StructureInstance** (type_id u32::MAX):
///   synthetic result values bypass field-vs-.ri-def validation, matching the
///   `warm_started`/other undeclared-field precedents.

#[cfg(test)]
mod tests {
    use reify_core::identity::RealizationNodeId;
    use reify_ir::boundary_attachment::{BoundaryAssociation, NodeAttachment};
    use reify_ir::geometry::GeometryHandleId;
    use reify_ir::value::GeometryHandleRef;

    use super::*;

    /// Build a representative `CarriedTopology` fixture with fully-finite data,
    /// covering all three `NodeAttachment` variants.
    fn make_fixture() -> CarriedTopology {
        let part = GeometryHandleRef {
            realization_ref: RealizationNodeId::new("body", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: None,
        };

        // Flat XYZ for 4 nodes: (0,0,0), (1,0,0), (0,1,0), (0,0,1)
        let node_coords: Vec<f32> = vec![
            0.0, 0.0, 0.0,
            1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
            0.0, 0.0, 1.0,
        ];

        // Two per-face normals keyed by GeometryHandleId
        let face_normals = vec![
            (GeometryHandleId(1), [0.0_f64, 0.0, 1.0]),
            (GeometryHandleId(2), [1.0_f64, 0.0, 0.0]),
        ];

        // BoundaryAssociation covering all 3 attachment variants
        let mut boundary = BoundaryAssociation::default();
        boundary.associate(4, NodeAttachment::OnFace(GeometryHandleId(1)));
        boundary.associate(8, NodeAttachment::OnFace(GeometryHandleId(2)));
        boundary.associate(0, NodeAttachment::OnEdge(GeometryHandleId(1)));

        CarriedTopology { part, node_coords, face_normals, boundary }
    }

    /// RED: CarriedTopology, to_value, and from_value do not exist yet.
    ///
    /// Tests construction via accessors and lossless Value round-trip.
    #[test]
    fn carried_topology_construction_and_round_trip() {
        let topo = make_fixture();

        // ── Accessor checks ──────────────────────────────────────────────────
        assert_eq!(topo.part().realization_ref.entity, "body");
        assert_eq!(topo.part().realization_ref.index, 0);
        assert!(topo.part().kernel_handle.is_none());

        assert_eq!(topo.node_coords().len(), 12, "4 nodes × 3 floats");
        assert_eq!(topo.face_normals().len(), 2);
        assert_eq!(topo.face_normals()[0].0, GeometryHandleId(1));
        assert_eq!(topo.face_normals()[0].1, [0.0, 0.0, 1.0]);
        assert_eq!(topo.face_normals()[1].0, GeometryHandleId(2));
        assert_eq!(topo.face_normals()[1].1, [1.0, 0.0, 0.0]);

        assert_eq!(topo.boundary().len(), 3);
        assert_eq!(
            topo.boundary().get(4),
            Some(NodeAttachment::OnFace(GeometryHandleId(1)))
        );
        assert_eq!(
            topo.boundary().get(8),
            Some(NodeAttachment::OnFace(GeometryHandleId(2)))
        );
        assert_eq!(
            topo.boundary().get(0),
            Some(NodeAttachment::OnEdge(GeometryHandleId(1)))
        );

        // ── Lossless round-trip ──────────────────────────────────────────────
        let encoded = topo.to_value();
        let decoded = CarriedTopology::from_value(&encoded);
        assert!(decoded.is_some(), "from_value must succeed on a valid encoding");
        assert_eq!(decoded.unwrap(), topo, "round-trip must be exact (PartialEq)");
    }
}
