use reify_ir::{AttributeHistory, ExportError, ExportFormat, ExportOptions, ExportWarning, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, KernelAttributeHook, Mesh, QueryError, SampledField, TessError, Value};

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

// INVARIANT: `SingleKernelHolder` must delegate EVERY `GeometryKernel` method to
// its inner kernel. The trait's defaulted capability methods (extract_edges,
// execute_split, ingest_mesh, attribute_hook, densify_grid_to_sampled, …) and
// the forward-through-core defaults (execute_with_history, query_many,
// export_with_options) silently mask missing delegation — returning
// None/not-supported (or routing through the holder's own core methods) instead
// of the inner kernel's real result. Whenever a new `GeometryKernel` method is
// added, add a delegating override below whose `None` arm reproduces the trait
// default's no-kernel output. This is guarded by the parity test
// `tests::delegates_all_capability_methods_to_inner_kernel`, which invokes every
// method through the holder and asserts the inner kernel saw each call.
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
            // Mirror the trait default's no-kernel message; type_name::<Self>()
            // resolves to SingleKernelHolder here, exactly as the default would.
            None => Err(GeometryError::OperationFailed(format!(
                "{} does not accept Mesh inputs",
                std::any::type_name::<Self>()
            ))),
        }
    }

    fn attribute_hook(&self) -> Option<&dyn KernelAttributeHook> {
        match self.kernel.as_ref() {
            Some(k) => k.attribute_hook(),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use reify_test_support::MockGeometryKernel;
    use reify_test_support::mm3;
    use reify_ir::{AttributeHistory, BRepKind, ExportError, ExportFormat, ExportOptions, ExportWarning, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, KernelAttributeHook, Mesh, QueryError, SampledField, TessError, Value};
    use std::sync::{Arc, Mutex};

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

    /// A self-contained `GeometryKernel` that records the name of every method
    /// invoked on it into a shared log. Used by
    /// `delegates_all_capability_methods_to_inner_kernel` to prove that
    /// `SingleKernelHolder` forwards each `GeometryKernel` method to its inner
    /// kernel rather than silently masking it with the trait default.
    ///
    /// Every method — including the defaulted capability methods — is overridden
    /// so the inner kernel logs its own name when (and only when) the holder
    /// actually delegates. Return values are minimal; the parity check inspects
    /// the call log, not the results (so `densify_grid_to_sampled` logs and
    /// returns `Err` rather than constructing a heavyweight `SampledField`).
    struct RecordingKernel {
        log: Arc<Mutex<Vec<&'static str>>>,
    }

    impl RecordingKernel {
        fn new() -> (Self, Arc<Mutex<Vec<&'static str>>>) {
            let log = Arc::new(Mutex::new(Vec::new()));
            (Self { log: log.clone() }, log)
        }

        fn record(&self, name: &'static str) {
            self.log.lock().unwrap().push(name);
        }

        fn handle() -> GeometryHandle {
            GeometryHandle { id: GeometryHandleId(1), repr: Some(BRepKind::Solid) }
        }
    }

    impl GeometryKernel for RecordingKernel {
        fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
            self.record("execute");
            Ok(Self::handle())
        }

        fn execute_with_history(
            &mut self,
            _op: &GeometryOp,
        ) -> Result<(GeometryHandle, AttributeHistory), GeometryError> {
            self.record("execute_with_history");
            Ok((Self::handle(), AttributeHistory::None))
        }

        fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
            self.record("query");
            Ok(Value::length(0.0))
        }

        fn query_many(&self, queries: &[GeometryQuery]) -> Result<Vec<Value>, QueryError> {
            self.record("query_many");
            Ok(queries.iter().map(|_| Value::length(0.0)).collect())
        }

        fn export(
            &self,
            _handle: GeometryHandleId,
            _format: ExportFormat,
            _writer: &mut dyn std::io::Write,
        ) -> Result<(), ExportError> {
            self.record("export");
            Ok(())
        }

        fn export_with_options(
            &self,
            _handle: GeometryHandleId,
            _format: ExportFormat,
            _options: &ExportOptions,
            _writer: &mut dyn std::io::Write,
        ) -> Result<Vec<ExportWarning>, ExportError> {
            self.record("export_with_options");
            Ok(Vec::new())
        }

        fn tessellate(&self, _handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
            self.record("tessellate");
            Ok(Mesh { vertices: vec![], indices: vec![], normals: None })
        }

        fn extract_edges(
            &mut self,
            _handle: GeometryHandleId,
        ) -> Result<Vec<GeometryHandleId>, QueryError> {
            self.record("extract_edges");
            Ok(vec![])
        }

        fn extract_faces(
            &mut self,
            _handle: GeometryHandleId,
        ) -> Result<Vec<GeometryHandleId>, QueryError> {
            self.record("extract_faces");
            Ok(vec![])
        }

        fn extract_vertices(
            &mut self,
            _handle: GeometryHandleId,
        ) -> Result<Vec<GeometryHandleId>, QueryError> {
            self.record("extract_vertices");
            Ok(vec![])
        }

        fn densify_grid_to_sampled(
            &mut self,
            _handle: GeometryHandleId,
        ) -> Result<SampledField, QueryError> {
            // Log then return Err — the parity check inspects the call log, not
            // the value, so we avoid constructing a heavyweight SampledField.
            self.record("densify_grid_to_sampled");
            Err(QueryError::QueryFailed("recorded".into()))
        }

        fn execute_split(
            &mut self,
            _op: &GeometryOp,
        ) -> Result<Vec<GeometryHandleId>, GeometryError> {
            self.record("execute_split");
            Ok(vec![])
        }

        fn make_compound(
            &mut self,
            _handles: &[GeometryHandleId],
        ) -> Result<GeometryHandle, GeometryError> {
            self.record("make_compound");
            Ok(Self::handle())
        }

        fn ingest_mesh(&mut self, _mesh: &Mesh) -> Result<GeometryHandle, GeometryError> {
            self.record("ingest_mesh");
            Ok(Self::handle())
        }

        fn attribute_hook(&self) -> Option<&dyn KernelAttributeHook> {
            self.record("attribute_hook");
            None
        }

        fn measure_mesh_deviation(&self, _handle: GeometryHandleId, _mesh: &Mesh) -> Option<f64> {
            self.record("measure_mesh_deviation");
            Some(0.0)
        }
    }

    /// Parity guard: `SingleKernelHolder` must delegate EVERY `GeometryKernel`
    /// method to its inner kernel. The trait's defaulted capability methods
    /// (extract_*, execute_split, ingest_mesh, attribute_hook,
    /// densify_grid_to_sampled) and the forward-through-core defaults
    /// (execute_with_history, query_many, export_with_options) silently mask the
    /// inner kernel's real implementation when the holder fails to override them
    /// — the exact failure the lib.rs FIXME warns about. This invokes every
    /// method through the holder and asserts the inner `RecordingKernel` saw
    /// each call.
    #[test]
    fn delegates_all_capability_methods_to_inner_kernel() {
        let (kernel, log) = RecordingKernel::new();
        let mut holder = SingleKernelHolder::new();
        holder.register_kernel(Box::new(kernel));

        let op = GeometryOp::Box {
            width: Value::length(0.01),
            height: Value::length(0.01),
            depth: Value::length(0.01),
        };
        let mesh = Mesh { vertices: vec![], indices: vec![], normals: None };
        let mut buf = Vec::new();

        // Invoke every GeometryKernel method through the holder.
        let _ = holder.execute(&op);
        let _ = holder.execute_with_history(&op);
        let _ = holder.query(&GeometryQuery::Volume(GeometryHandleId(1)));
        let _ = holder.query_many(&[GeometryQuery::Volume(GeometryHandleId(1))]);
        let _ = holder.export(GeometryHandleId(1), ExportFormat::Step, &mut buf);
        let _ = holder.export_with_options(
            GeometryHandleId(1),
            ExportFormat::Step,
            &ExportOptions::default(),
            &mut buf,
        );
        let _ = holder.tessellate(GeometryHandleId(1), 0.1);
        let _ = holder.extract_edges(GeometryHandleId(1));
        let _ = holder.extract_faces(GeometryHandleId(1));
        let _ = holder.extract_vertices(GeometryHandleId(1));
        let _ = holder.densify_grid_to_sampled(GeometryHandleId(1));
        let _ = holder.execute_split(&op);
        let _ = holder.make_compound(&[GeometryHandleId(1)]);
        let _ = holder.ingest_mesh(&mesh);
        let _ = holder.attribute_hook();
        let _ = holder.measure_mesh_deviation(GeometryHandleId(1), &mesh);

        let recorded = log.lock().unwrap();
        for method in [
            "execute",
            "execute_with_history",
            "query",
            "query_many",
            "export",
            "export_with_options",
            "tessellate",
            "extract_edges",
            "extract_faces",
            "extract_vertices",
            "densify_grid_to_sampled",
            "execute_split",
            "make_compound",
            "ingest_mesh",
            "attribute_hook",
            "measure_mesh_deviation",
        ] {
            assert!(
                recorded.contains(&method),
                "SingleKernelHolder did not delegate `{method}` to the inner kernel; \
                 recorded calls: {recorded:?}"
            );
        }
    }
}
