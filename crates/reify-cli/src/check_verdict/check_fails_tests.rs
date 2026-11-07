use super::{check_fails, ConstraintOutcome};

// ── step-1: RED unit tests for check_fails ────────────────────────────────

#[test]
fn check_fails_all_satisfied_is_false_regardless_of_strict() {
    assert!(
        !check_fails(&ConstraintOutcome::AllSatisfied, false),
        "AllSatisfied + strict=false should be false"
    );
    assert!(
        !check_fails(&ConstraintOutcome::AllSatisfied, true),
        "AllSatisfied + strict=true should be false"
    );
}

#[test]
fn check_fails_some_violated_is_true_regardless_of_strict() {
    assert!(
        check_fails(&ConstraintOutcome::SomeViolated, false),
        "SomeViolated + strict=false should be true"
    );
    assert!(
        check_fails(&ConstraintOutcome::SomeViolated, true),
        "SomeViolated + strict=true should be true"
    );
}

#[test]
fn check_fails_some_indeterminate_false_when_not_strict() {
    assert!(
        !check_fails(&ConstraintOutcome::SomeIndeterminate(1), false),
        "SomeIndeterminate + strict=false should be false (indeterminate is not a failure without --strict)"
    );
    assert!(
        !check_fails(&ConstraintOutcome::SomeIndeterminate(3), false),
        "SomeIndeterminate(3) + strict=false should be false"
    );
}

#[test]
fn check_fails_some_indeterminate_true_when_strict() {
    assert!(
        check_fails(&ConstraintOutcome::SomeIndeterminate(1), true),
        "SomeIndeterminate + strict=true should be true (--strict promotes indeterminate to failure)"
    );
    assert!(
        check_fails(&ConstraintOutcome::SomeIndeterminate(2), true),
        "SomeIndeterminate(2) + strict=true should be true"
    );
}

// ── end step-1 ────────────────────────────────────────────────────────────
