use reify_ir::{ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value};

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
}

// FIXME: Every new optional `GeometryKernel` capability method (extract_edges, // ptodo:allow interface-tracking note, no specific task
// make_compound, ingest_mesh, measure_mesh_deviation, attribute_hook, ...) MUST be
// manually added and delegated here. The trait's default implementation silently
// masks missing delegation — returning None/not-supported instead of the inner
// kernel's real result.
impl GeometryKernel for SingleKernelHolder {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        match self.kernel.as_mut() {
            Some(k) => k.execute(op),
            None => Err(GeometryError::OperationFailed(
                "no geometry kernel registered".to_string(),
            )),
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
                "no geometry kernel registered".to_string(),
            )),
        }
    }

    fn measure_mesh_deviation(&self, handle: GeometryHandleId, mesh: &Mesh) -> Option<f64> {
        match self.kernel.as_ref() {
            Some(k) => k.measure_mesh_deviation(handle, mesh),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use reify_test_support::MockGeometryKernel;
    use reify_test_support::mm3;
    use reify_ir::{BRepKind, ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value};

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

        // MockGeometryKernel returns a single triangle
        assert_eq!(mesh.vertices.len(), 9); // 3 vertices * 3 coords
        assert_eq!(mesh.indices.len(), 3); // 1 triangle
        assert!(mesh.normals.is_some());
        assert_eq!(mesh.normals.unwrap().len(), 9); // 3 normals * 3 coords
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

    /// Minimal in-test stub kernel that only supports `measure_mesh_deviation`.
    /// Implements the four required trait methods as `unimplemented!()` stubs,
    /// following the CountingKernel minimal-stub pattern from reify-ir/src/geometry.rs.
    ///
    /// Returns a value derived from both forwarded arguments (handle ID + vertex
    /// count) so a delegation bug that passes the wrong handle or mesh produces a
    /// different result and the assertion catches it.
    struct DeviationStubKernel;

    impl GeometryKernel for DeviationStubKernel {
        fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
            unimplemented!("DeviationStubKernel only supports measure_mesh_deviation") // ptodo:allow exhaustiveness/stub arm - not tracked debt
        }

        fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
            unimplemented!("DeviationStubKernel only supports measure_mesh_deviation") // ptodo:allow exhaustiveness/stub arm - not tracked debt
        }

        fn export(
            &self,
            _handle: GeometryHandleId,
            _format: ExportFormat,
            _writer: &mut dyn std::io::Write,
        ) -> Result<(), ExportError> {
            unimplemented!("DeviationStubKernel only supports measure_mesh_deviation") // ptodo:allow exhaustiveness/stub arm - not tracked debt
        }

        fn tessellate(&self, _handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
            unimplemented!("DeviationStubKernel only supports measure_mesh_deviation") // ptodo:allow exhaustiveness/stub arm - not tracked debt
        }

        fn measure_mesh_deviation(&self, handle: GeometryHandleId, mesh: &Mesh) -> Option<f64> {
            // Encode both forwarded arguments into the return value so the delegation
            // test can verify the correct handle and mesh were passed through — not just
            // that some value was returned.  A wrong handle or wrong mesh produces a
            // different f64 and the assertion catches it.
            Some(handle.0 as f64 + mesh.vertices.len() as f64)
        }
    }

    #[test]
    fn measure_mesh_deviation_delegates_to_registered_kernel() {
        let mut planner = SingleKernelHolder::new();
        planner.register_kernel(Box::new(DeviationStubKernel));
        // Non-trivial handle id (5) and mesh with 3 vertex floats so the expected
        // encoded value is unambiguous: 5.0 (handle) + 3.0 (vertex count) = 8.0.
        // Passing a wrong handle or an empty/default mesh produces a different
        // result and the assertion catches the forwarding bug.
        let mesh = Mesh { vertices: vec![0.0_f32, 1.0, 2.0], indices: vec![], normals: None };
        let result = planner.measure_mesh_deviation(GeometryHandleId(5), &mesh);
        assert_eq!(result, Some(8.0));
    }

    #[test]
    fn measure_mesh_deviation_no_kernel_returns_none() {
        let planner = SingleKernelHolder::new();
        let mesh = Mesh { vertices: vec![], indices: vec![], normals: None };
        let result = planner.measure_mesh_deviation(GeometryHandleId(1), &mesh);
        assert!(result.is_none());
    }
}
