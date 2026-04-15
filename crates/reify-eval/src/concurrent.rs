// Concurrent edit support — structs and Engine methods for prepare/apply/rollback/resolve.
//
// This module will hold ConcurrentEditSetup, ConcurrentNodeResult,
// ConcurrentEditResult structs and their associated Engine methods, extracted
// from lib.rs.

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time assertion: ConcurrentEditSetup is accessible from this module.
    #[test]
    fn concurrent_edit_setup_accessible() {
        let _: fn() -> String = || {
            format!("{}", std::mem::size_of::<ConcurrentEditSetup>());
            String::new()
        };
    }

    /// Compile-time assertion: ConcurrentNodeResult is accessible from this module.
    #[test]
    fn concurrent_node_result_accessible() {
        let _: fn() -> String = || {
            format!("{}", std::mem::size_of::<ConcurrentNodeResult>());
            String::new()
        };
    }

    /// Compile-time assertion: ConcurrentEditResult is accessible from this module.
    #[test]
    fn concurrent_edit_result_accessible() {
        let _: fn() -> String = || {
            format!("{}", std::mem::size_of::<ConcurrentEditResult>());
            String::new()
        };
    }
}
