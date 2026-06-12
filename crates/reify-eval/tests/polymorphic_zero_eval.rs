//! Eval-signal tests for the polymorphic-zero comparison coercion (task 4485/β, §7.2).
//!
//! These tests verify the USER-OBSERVABLE SIGNAL: `constraint mass > 0` where
//! `mass : Mass` should produce `Satisfaction::Satisfied` (not `Indeterminate`)
//! after the compile-time literal rewrite lands.
//!
//! RED today: the compiler does not rewrite the literal `0` (type Int) to
//! `Scalar<Mass>(0.0)` before emitting the comparison, so `eval_cmp` sees a
//! dimensioned Scalar vs a plain Int — which returns `Undef` and the constraint
//! checker reports `Indeterminate`.
//!
//! GREEN after step-4 (comparison-position coercion impl): the compiler rewrites
//! `0` → `Scalar<Mass>(0.0)`, `eval_cmp` evaluates `5kg > 0kg` → true, and the
//! constraint reports `Satisfied`.

use reify_ir::Satisfaction;
use reify_test_support::check_source;

/// `param mass : Mass = 5kg; constraint mass > 0` → Satisfied.
///
/// The compile-time zero-coercion must promote the literal `0` (Int, dimensionless)
/// to `Scalar<Mass>(0.0)` so eval_cmp compares two same-dimension Scalars:
/// `5kg > 0kg` → true → Satisfied.
#[test]
fn mass_gt_zero_satisfied_when_positive() {
    let result = check_source(
        r#"
structure S {
    param mass : Mass = 5kg
    constraint mass > 0
}
"#,
    );
    assert_eq!(
        result.constraint_results.len(),
        1,
        "expected exactly 1 constraint result, got {}",
        result.constraint_results.len()
    );
    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "mass (5kg) > 0 should be Satisfied after zero-coercion, got {:?}",
        result.constraint_results[0].satisfaction
    );
}

/// `param mass : Mass = 0kg; constraint mass > 0` → Violated.
///
/// Boundary case: `0kg > 0kg` is false → Violated.
/// Confirms the coercion does not suppress correct violation detection.
#[test]
fn mass_gt_zero_violated_when_zero() {
    let result = check_source(
        r#"
structure S {
    param mass : Mass = 0kg
    constraint mass > 0
}
"#,
    );
    assert_eq!(
        result.constraint_results.len(),
        1,
        "expected exactly 1 constraint result, got {}",
        result.constraint_results.len()
    );
    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Violated,
        "mass (0kg) > 0 should be Violated (0 > 0 is false), got {:?}",
        result.constraint_results[0].satisfaction
    );
}

/// `param len : Length = 3mm; constraint len > 0` → Satisfied.
///
/// Confirms the coercion works for Length (the most common dimension in stdlib
/// constraints), not just Mass.
#[test]
fn length_gt_zero_satisfied_when_positive() {
    let result = check_source(
        r#"
structure S {
    param len : Length = 3mm
    constraint len > 0
}
"#,
    );
    assert_eq!(
        result.constraint_results.len(),
        1,
        "expected exactly 1 constraint result, got {}",
        result.constraint_results.len()
    );
    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "len (3mm) > 0 should be Satisfied after zero-coercion, got {:?}",
        result.constraint_results[0].satisfaction
    );
}

/// Mirrored form: `0 < mass` (literal on the left) → Satisfied when mass > 0kg.
///
/// Covers the left-is-zero coercion path (symmetric to the right-is-zero case above).
#[test]
fn zero_lt_mass_satisfied_when_positive() {
    let result = check_source(
        r#"
structure S {
    param mass : Mass = 2kg
    constraint 0 < mass
}
"#,
    );
    assert_eq!(
        result.constraint_results.len(),
        1,
        "expected exactly 1 constraint result, got {}",
        result.constraint_results.len()
    );
    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "0 < mass (2kg) should be Satisfied after zero-coercion, got {:?}",
        result.constraint_results[0].satisfaction
    );
}
