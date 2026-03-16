use reify_types::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value,
};

/// A dispatch planner that wraps an optional geometry kernel.
///
/// For M1, holds a single optional kernel and delegates all
/// `GeometryKernel` calls to it. When no kernel is registered,
/// each method returns the appropriate error.
pub struct DispatchPlanner {
    kernel: Option<Box<dyn GeometryKernel>>,
}

impl DispatchPlanner {
    /// Create a new `DispatchPlanner` with no kernel registered.
    pub fn new() -> Self {
        Self { kernel: None }
    }

    /// Register a geometry kernel for dispatch.
    pub fn register_kernel(&mut self, kernel: Box<dyn GeometryKernel>) {
        self.kernel = Some(kernel);
    }

    /// Returns `true` if a kernel has been registered.
    pub fn has_kernel(&self) -> bool {
        self.kernel.is_some()
    }
}

impl GeometryKernel for DispatchPlanner {
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

    fn tessellate(
        &self,
        handle: GeometryHandleId,
        tolerance: f64,
    ) -> Result<Mesh, TessError> {
        match self.kernel.as_ref() {
            Some(k) => k.tessellate(handle, tolerance),
            None => Err(TessError::TessellationFailed(
                "no geometry kernel registered".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use reify_test_support::MockGeometryKernel;
    use reify_test_support::mm3;
    use reify_types::{
        ExportError, ExportFormat, GeometryError, GeometryHandleId, GeometryKernel, GeometryOp,
        GeometryQuery, QueryError, ReprKind, TessError, Value,
    };

    use super::*;

    #[test]
    fn new_planner_has_no_kernel() {
        let planner = DispatchPlanner::new();
        assert!(!planner.has_kernel());
    }

    #[test]
    fn register_kernel_sets_has_kernel_true() {
        let mut planner = DispatchPlanner::new();
        let mock = MockGeometryKernel::new();
        planner.register_kernel(Box::new(mock));
        assert!(planner.has_kernel());
    }

    #[test]
    fn execute_no_kernel_returns_error() {
        let mut planner = DispatchPlanner::new();
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
        let mut planner = DispatchPlanner::new();
        planner.register_kernel(Box::new(MockGeometryKernel::new()));

        let op = GeometryOp::Box {
            width: Value::length(0.01),
            height: Value::length(0.01),
            depth: Value::length(0.01),
        };
        let handle = planner.execute(&op).expect("execute should succeed");
        assert_eq!(handle.id, GeometryHandleId(1));
        assert_eq!(handle.repr, ReprKind::Solid);
    }

    #[test]
    fn query_no_kernel_returns_error() {
        let planner = DispatchPlanner::new();
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
        let mut planner = DispatchPlanner::new();
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
        let planner = DispatchPlanner::new();
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
        let mut planner = DispatchPlanner::new();
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
        let planner = DispatchPlanner::new();
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
        let mut planner = DispatchPlanner::new();
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
}
