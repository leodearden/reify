use reify_types::Diagnostic;

use crate::ConstraintCheckEntry;

/// Overall status of a single `@test` entity run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestStatus {
    Pass,
    Fail,
    Indeterminate,
}

/// Result of running a single `@test` entity.
#[derive(Debug, Clone)]
pub struct TestResult {
    /// Name of the test template (from `TopologyTemplate::name`).
    pub name: String,
    /// Overall test status: Pass/Fail/Indeterminate.
    pub status: TestStatus,
    /// Diagnostics emitted by constraint checking during the test run.
    pub diagnostics: Vec<Diagnostic>,
    /// Per-constraint satisfaction entries from the test template.
    pub constraint_results: Vec<ConstraintCheckEntry>,
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_status_variants_exist_and_are_distinct() {
        use super::TestStatus;
        assert_ne!(TestStatus::Pass, TestStatus::Fail);
        assert_ne!(TestStatus::Pass, TestStatus::Indeterminate);
        assert_ne!(TestStatus::Fail, TestStatus::Indeterminate);
    }

    #[test]
    fn test_result_constructs_with_required_fields() {
        use super::{TestResult, TestStatus};
        let tr = TestResult {
            name: "TestFoo".to_string(),
            status: TestStatus::Pass,
            diagnostics: Vec::new(),
            constraint_results: Vec::new(),
        };
        assert_eq!(tr.name, "TestFoo");
        assert_eq!(tr.status, TestStatus::Pass);
        assert!(tr.diagnostics.is_empty());
        assert!(tr.constraint_results.is_empty());
    }
}
