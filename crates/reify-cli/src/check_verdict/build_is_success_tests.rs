use super::{build_is_success, ConstraintOutcome};

/// (AllSatisfied, no error diagnostic) → success.
#[test]
fn all_satisfied_no_error_is_success() {
    assert!(build_is_success(&ConstraintOutcome::AllSatisfied, false));
}

/// (AllSatisfied, has error diagnostic) → failure.
/// Mirrors cmd_eval's Severity::Error gate (task 4458 fix (c)).
#[test]
fn all_satisfied_with_error_is_failure() {
    assert!(!build_is_success(&ConstraintOutcome::AllSatisfied, true));
}

/// (SomeIndeterminate, no error diagnostic) → success.
/// Indeterminate constraints do not gate build; geometry is still written.
#[test]
fn some_indeterminate_no_error_is_success() {
    assert!(build_is_success(&ConstraintOutcome::SomeIndeterminate(2), false));
}

/// (SomeIndeterminate, has error diagnostic) → failure.
#[test]
fn some_indeterminate_with_error_is_failure() {
    assert!(!build_is_success(&ConstraintOutcome::SomeIndeterminate(2), true));
}

/// (SomeViolated, no error diagnostic) → failure.
#[test]
fn some_violated_no_error_is_failure() {
    assert!(!build_is_success(&ConstraintOutcome::SomeViolated, false));
}

/// (SomeViolated, has error diagnostic) → failure.
#[test]
fn some_violated_with_error_is_failure() {
    assert!(!build_is_success(&ConstraintOutcome::SomeViolated, true));
}
