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

    fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
        todo!()
    }

    fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        todo!()
    }

    fn tessellate(
        &self,
        _handle: GeometryHandleId,
        _tolerance: f64,
    ) -> Result<Mesh, TessError> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use reify_test_support::MockGeometryKernel;
    use reify_types::{
        GeometryError, GeometryHandleId, GeometryKernel, GeometryOp, ReprKind, Value,
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
}
