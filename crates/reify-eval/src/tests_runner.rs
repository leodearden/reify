use reify_types::{Diagnostic, Satisfaction};

use crate::ConstraintCheckEntry;

/// Overall status of a single `@test` entity run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestStatus {
    Pass,
    Fail,
    Indeterminate,
}

/// Compute the overall test status from per-constraint satisfaction entries.
///
/// - Empty → `Pass` (vacuously satisfied).
/// - Any `Violated` → `Fail` (violations dominate).
/// - Else any `Indeterminate` → `Indeterminate`.
/// - Else → `Pass`.
fn compute_status(results: &[ConstraintCheckEntry]) -> TestStatus {
    let mut has_indeterminate = false;
    for entry in results {
        match entry.satisfaction {
            Satisfaction::Violated => return TestStatus::Fail,
            Satisfaction::Indeterminate => has_indeterminate = true,
            Satisfaction::Satisfied => {}
        }
    }
    if has_indeterminate {
        TestStatus::Indeterminate
    } else {
        TestStatus::Pass
    }
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

    fn entry(sat: reify_types::Satisfaction) -> crate::ConstraintCheckEntry {
        use reify_types::ConstraintNodeId;
        crate::ConstraintCheckEntry {
            id: ConstraintNodeId::new("E", 0),
            label: None,
            satisfaction: sat,
        }
    }

    #[test]
    fn compute_status_empty_returns_pass() {
        use super::compute_status;
        assert_eq!(compute_status(&[]), super::TestStatus::Pass);
    }

    #[test]
    fn compute_status_all_satisfied_returns_pass() {
        use reify_types::Satisfaction;
        use super::compute_status;
        let entries = vec![entry(Satisfaction::Satisfied), entry(Satisfaction::Satisfied)];
        assert_eq!(compute_status(&entries), super::TestStatus::Pass);
    }

    #[test]
    fn compute_status_any_violated_returns_fail() {
        use reify_types::Satisfaction;
        use super::compute_status;
        let entries = vec![entry(Satisfaction::Satisfied), entry(Satisfaction::Violated)];
        assert_eq!(compute_status(&entries), super::TestStatus::Fail);
    }

    #[test]
    fn compute_status_only_indeterminate_returns_indeterminate() {
        use reify_types::Satisfaction;
        use super::compute_status;
        let entries = vec![entry(Satisfaction::Indeterminate)];
        assert_eq!(compute_status(&entries), super::TestStatus::Indeterminate);
    }

    #[test]
    fn compute_status_mix_satisfied_indeterminate_returns_indeterminate() {
        use reify_types::Satisfaction;
        use super::compute_status;
        let entries = vec![entry(Satisfaction::Satisfied), entry(Satisfaction::Indeterminate)];
        assert_eq!(compute_status(&entries), super::TestStatus::Indeterminate);
    }

    #[test]
    fn compute_status_violated_dominates_indeterminate_returns_fail() {
        use reify_types::Satisfaction;
        use super::compute_status;
        let entries = vec![entry(Satisfaction::Indeterminate), entry(Satisfaction::Violated)];
        assert_eq!(compute_status(&entries), super::TestStatus::Fail);
    }

    #[test]
    fn compute_status_violated_dominates_satisfied_and_indeterminate_returns_fail() {
        use reify_types::Satisfaction;
        use super::compute_status;
        let entries = vec![
            entry(Satisfaction::Satisfied),
            entry(Satisfaction::Indeterminate),
            entry(Satisfaction::Violated),
        ];
        assert_eq!(compute_status(&entries), super::TestStatus::Fail);
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
