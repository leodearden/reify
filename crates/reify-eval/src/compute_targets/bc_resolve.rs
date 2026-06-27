//! Pure boundary-condition resolution helpers (task 4092 — FEA face-selector
//! boundary conditions).
//!
//! Three helpers bridge a typed predicate topology [`SelectorValue`] and the
//! realized tet [`reify_ir::VolumeMesh`]'s per-node
//! [`reify_ir::BoundaryAssociation`]:
//!
//! * [`resolve_selector_faces`] — BUILD-time (live kernel): a thin reuse-wrapper
//!   over [`crate::topology_selectors::resolve`] that turns a predicate selector
//!   into the matching `Vec<GeometryHandleId>` face handles on the realized body.
//! * [`build_face_anchors`] — BUILD-time (live kernel): assembles the
//!   `(face_handle, centroid)` anchor list the realization edge feeds to
//!   `GeometryKernel::mesh_surface_to_volume_attributed`.
//! * [`boundary_node_set`] — PURE (kernel-less): maps resolved face handles to
//!   the sorted node-index set attributed `OnFace(handle)` on the realized mesh.
//!
//! The kernel-bearing halves run where a live [`GeometryKernel`] is available
//! (`geometry_ops.rs` / the realization edge); the kernel-less
//! [`boundary_node_set`] runs in the compute trampoline (which receives only
//! value + realization inputs, no kernel). This split is why the trampoline
//! consumes already-resolved face handles + the realized boundary rather than a
//! [`SelectorValue`].

#[cfg(test)]
mod tests {
    use reify_core::identity::RealizationNodeId;
    use reify_core::ty::SelectorKind;
    use reify_core::Diagnostic;
    use reify_ir::value::{GeometryHandleRef, LeafQuery, SelectorValue};
    use reify_ir::{
        BoundaryAssociation, ExportError, ExportFormat, GeometryError, GeometryHandle,
        GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, NodeAttachment,
        QueryError, TessError, Value,
    };
    use std::collections::HashMap;

    fn h(n: u64) -> GeometryHandleId {
        GeometryHandleId(n)
    }

    /// Minimal in-test `GeometryKernel`: `extract_faces` returns a fixed list,
    /// `query` replies from a staged `(id → Value)` map (for `FaceNormal` /
    /// `Centroid`), and an id absent from the map yields `InvalidHandle` so a
    /// per-face Centroid failure is exercisable. All other trait methods that
    /// have no default body are stubbed.
    struct FakeKernel {
        faces: Vec<GeometryHandleId>,
        responses: HashMap<GeometryHandleId, Value>,
    }

    impl FakeKernel {
        fn id_of(query: &GeometryQuery) -> Option<GeometryHandleId> {
            match query {
                GeometryQuery::FaceNormal(id) | GeometryQuery::Centroid(id) => Some(*id),
                _ => None,
            }
        }
    }

    impl GeometryKernel for FakeKernel {
        fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
            Err(GeometryError::OperationFailed("FakeKernel: execute unsupported".into()))
        }
        fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
            let id = Self::id_of(query)
                .ok_or_else(|| QueryError::QueryFailed("FakeKernel: unsupported query".into()))?;
            self.responses.get(&id).cloned().ok_or(QueryError::InvalidHandle(id))
        }
        fn export(
            &self,
            _handle: GeometryHandleId,
            _format: ExportFormat,
            _writer: &mut dyn std::io::Write,
        ) -> Result<(), ExportError> {
            Err(ExportError::FormatError("FakeKernel: export unsupported".into()))
        }
        fn tessellate(&self, _handle: GeometryHandleId, _tol: f64) -> Result<Mesh, TessError> {
            Err(TessError::TessellationFailed("FakeKernel: tessellate unsupported".into()))
        }
        fn extract_faces(
            &mut self,
            _handle: GeometryHandleId,
        ) -> Result<Vec<GeometryHandleId>, QueryError> {
            Ok(self.faces.clone())
        }
    }

    fn xyz(x: f64, y: f64, z: f64) -> Value {
        Value::String(format!("{{\"x\":{x},\"y\":{y},\"z\":{z}}}"))
    }

    /// Build a `Face` `ByNormal` predicate selector targeting `body`.
    fn by_normal_face_selector(body: GeometryHandleId, dir: [f64; 3]) -> SelectorValue {
        let target = GeometryHandleRef {
            realization_ref: RealizationNodeId::new("body", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle: Some(body),
        };
        SelectorValue::leaf(SelectorKind::Face, target, LeafQuery::ByNormal { dir, tol_rad: 0.1 })
            .expect("valid Face/ByNormal leaf")
    }

    // ── (a) boundary_node_set ────────────────────────────────────────────────

    #[test]
    fn boundary_node_set_keeps_only_onface_nodes_of_requested_handles() {
        let mut b = BoundaryAssociation::default();
        // Nodes 5,2 on face h1; node 7 on face h2; node 1 OnEdge; node 9 OnVertex.
        b.associate(5, NodeAttachment::OnFace(h(1)));
        b.associate(2, NodeAttachment::OnFace(h(1)));
        b.associate(7, NodeAttachment::OnFace(h(2)));
        b.associate(1, NodeAttachment::OnEdge(h(1)));
        b.associate(9, NodeAttachment::OnVertex(h(1)));

        // Requesting only h1 → sorted [2,5]; excludes h2's node, the edge & vertex.
        assert_eq!(super::boundary_node_set(&b, &[h(1)]), vec![2u32, 5]);
        // Requesting both faces → sorted union [2,5,7].
        assert_eq!(super::boundary_node_set(&b, &[h(1), h(2)]), vec![2u32, 5, 7]);
        // Unmatched handle → empty.
        assert!(super::boundary_node_set(&b, &[h(999)]).is_empty());
        // Empty face slice → empty.
        assert!(super::boundary_node_set(&b, &[]).is_empty());
    }

    // ── (b) resolve_selector_faces ───────────────────────────────────────────

    #[test]
    fn resolve_selector_faces_returns_predicate_matched_faces() {
        let body = h(100);
        let mut responses = HashMap::new();
        // Both faces normal ≈ +Z → both within tol of dir [0,0,1].
        responses.insert(h(1), xyz(0.0, 0.0, 1.0));
        responses.insert(h(2), xyz(0.0, 0.0, 1.0));
        let mut kernel = FakeKernel { faces: vec![h(1), h(2)], responses };

        let selector = by_normal_face_selector(body, [0.0, 0.0, 1.0]);
        let mut diags: Vec<Diagnostic> = Vec::new();
        let faces = super::resolve_selector_faces(&selector, &mut kernel, &mut diags)
            .expect("resolve must succeed");
        assert_eq!(faces, vec![h(1), h(2)]);
    }

    // ── (c) compose (b) → (a) ────────────────────────────────────────────────

    #[test]
    fn resolve_then_node_set_yields_nonempty_set() {
        let body = h(100);
        let mut responses = HashMap::new();
        responses.insert(h(1), xyz(0.0, 0.0, 1.0));
        responses.insert(h(2), xyz(0.0, 0.0, 1.0));
        let mut kernel = FakeKernel { faces: vec![h(1), h(2)], responses };

        let selector = by_normal_face_selector(body, [0.0, 0.0, 1.0]);
        let mut diags: Vec<Diagnostic> = Vec::new();
        let faces = super::resolve_selector_faces(&selector, &mut kernel, &mut diags).unwrap();

        // A realized boundary attributing node 4 to h1 and node 8 to h2.
        let mut boundary = BoundaryAssociation::default();
        boundary.associate(4, NodeAttachment::OnFace(h(1)));
        boundary.associate(8, NodeAttachment::OnFace(h(2)));
        boundary.associate(0, NodeAttachment::OnEdge(h(1)));

        let nodes = super::boundary_node_set(&boundary, &faces);
        assert_eq!(nodes, vec![4u32, 8], "composed selector→node-set must be the OnFace union");
    }
}
