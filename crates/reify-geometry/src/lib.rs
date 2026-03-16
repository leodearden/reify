use reify_types::GeometryKernel;

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

#[cfg(test)]
mod tests {
    use reify_test_support::MockGeometryKernel;
    use reify_types::{GeometryError, GeometryKernel, GeometryOp, Value};

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
}
