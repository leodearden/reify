// Stub — M1 implementation pending

#[cfg(test)]
mod tests {
    use reify_test_support::MockGeometryKernel;

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
}
