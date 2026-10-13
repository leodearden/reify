use reify_core::hash::ContentHash;
use reify_ir::{AttributeHistory, ElementOrderTag, ExportError, ExportFormat, ExportOptions, ExportWarning, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, KernelAttributeHook, Mesh, QueryError, SampledField, TessError, Value, VolumeMesh, VoxelResolution};

/// A single-kernel holder that wraps an optional geometry kernel.
///
/// Holds a single optional kernel and delegates all `GeometryKernel` calls to
/// it. When no kernel is registered, each method returns the appropriate error.
///
/// `SingleKernelHolder` implements [`GeometryKernel`] itself, so it can be
/// used as a transparent drop-in wherever `Box<dyn GeometryKernel>` is
/// expected (e.g., the `reify-eval` test suite). Production binaries use
/// [`Engine::with_registered_kernel`](reify_eval::Engine::with_registered_kernel)
/// instead of constructing a `SingleKernelHolder` directly.
pub struct SingleKernelHolder {
    kernel: Option<Box<dyn GeometryKernel>>,
}

// Compile-time check: SingleKernelHolder is Send + Sync (required by GeometryKernel).
const _: fn() = || {
    fn must_be_send_sync<T: Send + Sync>() {}
    must_be_send_sync::<SingleKernelHolder>();
};

impl Default for SingleKernelHolder {
    fn default() -> Self {
        Self::new()
    }
}

impl SingleKernelHolder {
    /// Create a new `SingleKernelHolder` with no kernel registered.
    pub fn new() -> Self {
        Self { kernel: None }
    }

    /// Register a geometry kernel.
    pub fn register_kernel(&mut self, kernel: Box<dyn GeometryKernel>) {
        self.kernel = Some(kernel);
    }

    /// Returns `true` if a kernel has been registered.
    pub fn has_kernel(&self) -> bool {
        self.kernel.is_some()
    }

    /// The no-kernel outcome shared by every Mesh-ingest entry point.
    ///
    /// `GeometryKernel::ingest_mesh`, `ingest_mesh_at_resolution` and
    /// `register_mesh_handle` all reproduce one trait-default message on an
    /// empty holder (the latter two defaults route through `ingest_mesh`);
    /// producing it once means the three arms cannot drift.
    /// `type_name::<Self>()` resolves to `SingleKernelHolder` here exactly as
    /// it does in the trait default.
    fn no_mesh_kernel_error() -> GeometryError {
        GeometryError::OperationFailed(Self::type_named_message("does not accept Mesh inputs"))
    }

    /// A no-kernel message prefixed with `type_name::<Self>()`, as the trait
    /// defaults that name their kernel produce it.
    fn type_named_message(tail: &str) -> String {
        format!("{} {tail}", std::any::type_name::<Self>())
    }
}

// INVARIANT: every `GeometryKernel` method is forwarded to the inner kernel, and
// each `None` arm returns exactly what the trait default returns for this
// holder. A missing forward is silent: the trait default answers in place of
// the inner kernel, or reroutes to a different method of it (as
// `register_mesh_handle`'s default does through `ingest_mesh`). Guarded by
// `tests/forwards_every_kernel_method.rs`, which enumerates the trait's methods
// from reify-ir's source, so a new trait method fails it until it is forwarded
// here.
impl GeometryKernel for SingleKernelHolder {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        match self.kernel.as_mut() {
            Some(k) => k.execute(op),
            None => Err(GeometryError::OperationFailed(
                "no geometry kernel registered".to_string(),
            )),
        }
    }

    /// Delegating override for the whole-file-reload teardown (task 5212).
    ///
    /// Required by this impl's delegate-EVERY-method invariant: the trait
    /// default is a silent no-op, so without this the holder would swallow the
    /// reload reset and leave the inner kernel's native shapes resident — the
    /// exact unbounded growth `GeometryKernel::reset` exists to bound. The
    /// `None` arm reproduces the trait default's no-kernel output (nothing to
    /// reset).
    fn reset(&mut self) {
        if let Some(k) = self.kernel.as_mut() {
            k.reset();
        }
    }

    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        match self.kernel.as_ref() {
            Some(k) => k.query(query),
            None => Err(QueryError::QueryFailed(
                "no geometry kernel registered".to_string(),
            )),
        }
    }

    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        match self.kernel.as_ref() {
            Some(k) => k.export(handle, format, writer),
            None => Err(ExportError::FormatError(
                "no geometry kernel registered".to_string(),
            )),
        }
    }

    fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        match self.kernel.as_ref() {
            Some(k) => k.tessellate(handle, tolerance),
            None => Err(TessError::TessellationFailed(
                "no geometry kernel registered".to_string(),
            )),
        }
    }

    fn make_compound(
        &mut self,
        handles: &[GeometryHandleId],
    ) -> Result<reify_ir::GeometryHandle, GeometryError> {
        match self.kernel.as_mut() {
            Some(k) => k.make_compound(handles),
            None => Err(GeometryError::OperationFailed(
                "make_compound not supported by this kernel".into(),
            )),
        }
    }

    fn measure_mesh_deviation(&self, handle: GeometryHandleId, mesh: &Mesh) -> Option<f64> {
        match self.kernel.as_ref() {
            Some(k) => k.measure_mesh_deviation(handle, mesh),
            None => None,
        }
    }

    fn execute_with_history(
        &mut self,
        op: &GeometryOp,
    ) -> Result<(GeometryHandle, AttributeHistory), GeometryError> {
        match self.kernel.as_mut() {
            Some(k) => k.execute_with_history(op),
            // Mirrors the trait default's no-kernel output: the default calls
            // self.execute(op), which returns this exact error when no kernel
            // is registered.
            None => Err(GeometryError::OperationFailed(
                "no geometry kernel registered".to_string(),
            )),
        }
    }

    fn query_many(&self, queries: &[GeometryQuery]) -> Result<Vec<Value>, QueryError> {
        match self.kernel.as_ref() {
            Some(k) => k.query_many(queries),
            // No kernel: reproduce the trait default's per-element fallback so
            // no-kernel behavior (including empty-input → Ok(vec![])) is
            // unchanged; self.query yields the no-kernel error per element.
            None => queries.iter().map(|q| self.query(q)).collect(),
        }
    }

    fn export_with_options(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        options: &ExportOptions,
        writer: &mut dyn std::io::Write,
    ) -> Result<Vec<ExportWarning>, ExportError> {
        match self.kernel.as_ref() {
            Some(k) => k.export_with_options(handle, format, options, writer),
            // No kernel: reproduce the trait default (ignore options, delegate
            // to export, empty warnings) so no-kernel behavior is unchanged.
            None => self.export(handle, format, writer).map(|()| Vec::new()),
        }
    }

    fn extract_edges(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        match self.kernel.as_mut() {
            Some(k) => k.extract_edges(handle),
            None => Err(QueryError::QueryFailed(
                "topology extraction not supported by this kernel".into(),
            )),
        }
    }

    fn extract_faces(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        match self.kernel.as_mut() {
            Some(k) => k.extract_faces(handle),
            None => Err(QueryError::QueryFailed(
                "topology extraction not supported by this kernel".into(),
            )),
        }
    }

    fn extract_vertices(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        match self.kernel.as_mut() {
            Some(k) => k.extract_vertices(handle),
            None => Err(QueryError::QueryFailed(
                "topology extraction not supported by this kernel".into(),
            )),
        }
    }

    fn densify_grid_to_sampled(
        &mut self,
        handle: GeometryHandleId,
    ) -> Result<SampledField, QueryError> {
        match self.kernel.as_mut() {
            Some(k) => k.densify_grid_to_sampled(handle),
            None => Err(QueryError::QueryFailed(
                "densify_grid_to_sampled not supported by this kernel".into(),
            )),
        }
    }

    fn execute_split(
        &mut self,
        op: &GeometryOp,
    ) -> Result<Vec<GeometryHandleId>, GeometryError> {
        match self.kernel.as_mut() {
            Some(k) => k.execute_split(op),
            None => Err(GeometryError::OperationFailed(
                "execute_split not supported by this kernel".into(),
            )),
        }
    }

    fn ingest_mesh(&mut self, mesh: &Mesh) -> Result<GeometryHandle, GeometryError> {
        match self.kernel.as_mut() {
            Some(k) => k.ingest_mesh(mesh),
            None => Err(Self::no_mesh_kernel_error()),
        }
    }

    /// Delegating override for the resolution-carrying mesh ingest (task 6560).
    ///
    /// Required by this impl's delegate-EVERY-method invariant, and uniquely
    /// harmful to omit: the trait default drops `resolution` and routes
    /// through `self.ingest_mesh`, which this holder also overrides, so the
    /// call still reaches the inner kernel — just via its `ingest_mesh` arm,
    /// at `VoxelResolution::HonestFloor`. The caller's request is therefore
    /// discarded SILENTLY, surfacing as a quietly coarser grid rather than an
    /// error: precisely the sub-voxel-feature failure `VoxelResolution` was
    /// introduced to prevent. Delegating verbatim is the whole point of the
    /// seam.
    ///
    /// The `None` arm reproduces the trait default's no-kernel output via the
    /// shared `SingleKernelHolder::no_mesh_kernel_error`, which `ingest_mesh`
    /// above also calls — so the two arms cannot drift.
    fn ingest_mesh_at_resolution(
        &mut self,
        mesh: &Mesh,
        resolution: VoxelResolution,
    ) -> Result<GeometryHandle, GeometryError> {
        match self.kernel.as_mut() {
            Some(k) => k.ingest_mesh_at_resolution(mesh, resolution),
            None => Err(Self::no_mesh_kernel_error()),
        }
    }

    fn register_mesh_handle(&mut self, mesh: &Mesh) -> Result<GeometryHandle, GeometryError> {
        match self.kernel.as_mut() {
            Some(k) => k.register_mesh_handle(mesh),
            None => Err(Self::no_mesh_kernel_error()),
        }
    }

    fn attribute_hook(&self) -> Option<&dyn KernelAttributeHook> {
        match self.kernel.as_ref() {
            Some(k) => k.attribute_hook(),
            None => None,
        }
    }

    fn volume_mesh(&self, handle: GeometryHandleId) -> Result<VolumeMesh, QueryError> {
        match self.kernel.as_ref() {
            Some(k) => k.volume_mesh(handle),
            None => Err(QueryError::QueryFailed(
                "volume_mesh projection not supported by this kernel".into(),
            )),
        }
    }

    fn mesh_surface_to_volume(
        &self,
        surface: &Mesh,
        element_order: ElementOrderTag,
    ) -> Result<VolumeMesh, GeometryError> {
        match self.kernel.as_ref() {
            Some(k) => k.mesh_surface_to_volume(surface, element_order),
            None => Err(GeometryError::OperationFailed(Self::type_named_message(
                "does not produce volume meshes",
            ))),
        }
    }

    fn mesh_surface_to_volume_attributed(
        &self,
        surface: &Mesh,
        element_order: ElementOrderTag,
        face_anchors: &[(GeometryHandleId, [f64; 3])],
        match_tolerance: f64,
    ) -> Result<VolumeMesh, GeometryError> {
        match self.kernel.as_ref() {
            Some(k) => k.mesh_surface_to_volume_attributed(
                surface,
                element_order,
                face_anchors,
                match_tolerance,
            ),
            None => Err(GeometryError::OperationFailed(Self::type_named_message(
                "does not produce attributed volume meshes",
            ))),
        }
    }

    fn store_volume_mesh(&self, vm: VolumeMesh) -> Result<GeometryHandleId, GeometryError> {
        match self.kernel.as_ref() {
            Some(k) => k.store_volume_mesh(vm),
            None => Err(GeometryError::OperationFailed(Self::type_named_message(
                "does not store volume meshes",
            ))),
        }
    }

    fn closest_point_on_shape(
        &self,
        handle: GeometryHandleId,
        point: [f64; 3],
    ) -> Result<[f64; 3], QueryError> {
        match self.kernel.as_ref() {
            Some(k) => k.closest_point_on_shape(handle, point),
            None => Err(QueryError::QueryFailed(Self::type_named_message(
                "does not support closest-point projection",
            ))),
        }
    }

    fn vertex_point(&self, handle: GeometryHandleId) -> Result<[f64; 3], QueryError> {
        match self.kernel.as_ref() {
            Some(k) => k.vertex_point(handle),
            None => Err(QueryError::QueryFailed(Self::type_named_message(
                "does not support vertex-position queries",
            ))),
        }
    }

    fn realize_mesh_from_voxel(
        &self,
        handle: GeometryHandleId,
        iso_level: f64,
        adaptive: bool,
    ) -> Result<Mesh, GeometryError> {
        match self.kernel.as_ref() {
            Some(k) => k.realize_mesh_from_voxel(handle, iso_level, adaptive),
            None => Err(GeometryError::OperationFailed(Self::type_named_message(
                "does not support voxel surfacing (marching cubes)",
            ))),
        }
    }

    fn surface_options_content_hash(&self, iso_level: f64, adaptive: bool) -> ContentHash {
        match self.kernel.as_ref() {
            Some(k) => k.surface_options_content_hash(iso_level, adaptive),
            None => ContentHash(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use reify_test_support::MockGeometryKernel;
    use reify_test_support::mm3;
    use reify_ir::{BRepKind, ExportError, ExportFormat, GeometryError, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, QueryError, TessError, Value};

    use super::*;

    #[test]
    fn new_holder_has_no_kernel() {
        let planner = SingleKernelHolder::new();
        assert!(!planner.has_kernel());
    }

    #[test]
    fn register_kernel_sets_has_kernel_true() {
        let mut planner = SingleKernelHolder::new();
        let mock = MockGeometryKernel::new();
        planner.register_kernel(Box::new(mock));
        assert!(planner.has_kernel());
    }

    #[test]
    fn execute_no_kernel_returns_error() {
        let mut planner = SingleKernelHolder::new();
        let op = GeometryOp::Box {
            width: Value::length(0.01),
            height: Value::length(0.01),
            depth: Value::length(0.01),
        };
        let result = planner.execute(&op);
        assert!(result.is_err());
        match result.unwrap_err() {
            GeometryError::OperationFailed(msg) => {
                assert!(
                    msg.contains("no geometry kernel registered"),
                    "unexpected error message: {}",
                    msg
                );
            }
            other => panic!("expected OperationFailed, got {:?}", other),
        }
    }

    #[test]
    fn execute_delegates_to_registered_kernel() {
        let mut planner = SingleKernelHolder::new();
        planner.register_kernel(Box::new(MockGeometryKernel::new()));

        let op = GeometryOp::Box {
            width: Value::length(0.01),
            height: Value::length(0.01),
            depth: Value::length(0.01),
        };
        let handle = planner.execute(&op).expect("execute should succeed");
        assert_eq!(handle.id, GeometryHandleId(1));
        assert_eq!(handle.repr, Some(BRepKind::Solid));
    }

    #[test]
    fn query_no_kernel_returns_error() {
        let planner = SingleKernelHolder::new();
        let query = GeometryQuery::Volume(GeometryHandleId(1));
        let result = planner.query(&query);
        assert!(result.is_err());
        match result.unwrap_err() {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("no geometry kernel registered"),
                    "unexpected error message: {}",
                    msg
                );
            }
            other => panic!("expected QueryFailed, got {:?}", other),
        }
    }

    #[test]
    fn query_delegates_to_registered_kernel() {
        let mut planner = SingleKernelHolder::new();
        let expected_volume = mm3(1000.0); // 1000 mm³
        let mock = MockGeometryKernel::new()
            .with_query_result(GeometryHandleId(1), expected_volume.clone());
        planner.register_kernel(Box::new(mock));

        // Execute an op first to create handle 1
        let op = GeometryOp::Box {
            width: Value::length(0.01),
            height: Value::length(0.01),
            depth: Value::length(0.01),
        };
        planner.execute(&op).unwrap();

        let query = GeometryQuery::Volume(GeometryHandleId(1));
        let result = planner.query(&query).expect("query should succeed");
        assert_eq!(result, expected_volume);
    }

    #[test]
    fn export_no_kernel_returns_error() {
        let planner = SingleKernelHolder::new();
        let mut buf = Vec::new();
        let result = planner.export(GeometryHandleId(1), ExportFormat::Step, &mut buf);
        assert!(result.is_err());
        match result.unwrap_err() {
            ExportError::FormatError(msg) => {
                assert!(
                    msg.contains("no geometry kernel registered"),
                    "unexpected error message: {}",
                    msg
                );
            }
            other => panic!("expected FormatError, got {:?}", other),
        }
    }

    #[test]
    fn export_delegates_to_registered_kernel() {
        let mut planner = SingleKernelHolder::new();
        planner.register_kernel(Box::new(MockGeometryKernel::new()));

        // Execute an op to create a handle
        let op = GeometryOp::Box {
            width: Value::length(0.01),
            height: Value::length(0.01),
            depth: Value::length(0.01),
        };
        planner.execute(&op).unwrap();

        let mut buf = Vec::new();
        planner
            .export(GeometryHandleId(1), ExportFormat::Step, &mut buf)
            .expect("export should succeed");
        assert_eq!(buf, b"MOCK_EXPORT_DATA");
    }

    #[test]
    fn tessellate_no_kernel_returns_error() {
        let planner = SingleKernelHolder::new();
        let result = planner.tessellate(GeometryHandleId(1), 0.1);
        assert!(result.is_err());
        match result.unwrap_err() {
            TessError::TessellationFailed(msg) => {
                assert!(
                    msg.contains("no geometry kernel registered"),
                    "unexpected error message: {}",
                    msg
                );
            }
            other => panic!("expected TessellationFailed, got {:?}", other),
        }
    }

    #[test]
    fn tessellate_delegates_to_registered_kernel() {
        let mut planner = SingleKernelHolder::new();
        planner.register_kernel(Box::new(MockGeometryKernel::new()));

        // Execute an op to create a handle
        let op = GeometryOp::Box {
            width: Value::length(0.01),
            height: Value::length(0.01),
            depth: Value::length(0.01),
        };
        planner.execute(&op).unwrap();

        let mesh = planner
            .tessellate(GeometryHandleId(1), 0.1)
            .expect("tessellate should succeed");

        // MockGeometryKernel returns `mocks::minimal_valid_mesh(true)`: a
        // closed unit tetrahedron. It used to be a single open triangle, which
        // `Mesh::validate` rejects under `MeshInvariant::Closed` — see task
        // #5105 (INV-GEO-1) for why the fixture had to become contract-valid.
        // This test only asserts the delegation reaches the registered kernel,
        // so the counts are re-grounded on the tetra rather than pinned to the
        // old shape.
        assert_eq!(mesh.vertices.len(), 12); // 4 vertices * 3 coords
        assert_eq!(mesh.indices.len(), 12); // 4 triangles
        assert!(mesh.normals.is_some());
        assert_eq!(mesh.normals.unwrap().len(), 12); // 4 normals * 3 coords
    }

    #[test]
    fn multi_operation_sequence_dispatched() {
        let mock = MockGeometryKernel::new();
        let ops_ref = mock.operations_ref();

        let mut planner = SingleKernelHolder::new();
        planner.register_kernel(Box::new(mock));

        // Create a box
        let box_op = GeometryOp::Box {
            width: Value::length(0.01),
            height: Value::length(0.02),
            depth: Value::length(0.03),
        };
        let box_handle = planner.execute(&box_op).expect("box should succeed");
        assert_eq!(box_handle.id, GeometryHandleId(1));

        // Translate the box
        let translate_op = GeometryOp::Translate {
            target: box_handle.id,
            dx: 0.1,
            dy: 0.0,
            dz: 0.0,
        };
        let translate_handle = planner
            .execute(&translate_op)
            .expect("translate should succeed");
        assert_eq!(translate_handle.id, GeometryHandleId(2));

        // Verify both operations were recorded
        let ops = ops_ref.lock().unwrap();
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].result_handle, GeometryHandleId(1));
        assert_eq!(ops[1].result_handle, GeometryHandleId(2));

        // Verify op types
        assert!(matches!(ops[0].op, GeometryOp::Box { .. }));
        assert!(matches!(ops[1].op, GeometryOp::Translate { .. }));
    }

    #[test]
    fn single_kernel_holder_usable_as_boxed_geometry_kernel() {
        let mock = MockGeometryKernel::new();
        let mut planner = SingleKernelHolder::new();
        planner.register_kernel(Box::new(mock));

        // Box the planner as a trait object — this is how reify-eval uses it
        let kernel: Box<dyn GeometryKernel> = Box::new(planner);

        // Use through trait interface
        let mut kernel = kernel;
        let op = GeometryOp::Sphere {
            radius: Value::length(0.05),
        };
        let handle = kernel
            .execute(&op)
            .expect("execute through trait object should succeed");
        assert_eq!(handle.id, GeometryHandleId(1));
        assert_eq!(handle.repr, Some(BRepKind::Solid));
    }
}
