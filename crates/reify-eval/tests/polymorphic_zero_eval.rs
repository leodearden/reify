//! Eval-signal tests for the polymorphic-zero comparison and additive coercion
//! (task 4485/β, §7.2).
//!
//! These tests verify the USER-OBSERVABLE SIGNAL: `constraint mass > 0` where
//! `mass : Mass` produces `Satisfaction::Satisfied` (not `Indeterminate`).
//!
//! The compile-time zero-coercion promotes the literal `0` (type Int) to
//! `Scalar<Mass>(0.0)` before emitting the comparison, so `eval_cmp` evaluates
//! `5kg > 0kg` → true and the constraint reports `Satisfied`.
//!
//! Coverage spans:
//! - Base dimensions (Mass, Length) — comparison-position
//! - Compound-quotient dimension (Stiffness = N/m) — comparison-position
//! - Additive position (`mass + 0`, `mass - 0`) — value-preservation check

use reify_ir::Satisfaction;
use reify_test_support::{check_source, check_source_with_stdlib};

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

// ────────────────────────────────────────────────────────────────────────────
// Compound-dimension eval signal (Suggestion 1 — covers dimensions beyond Mass/Length)
// ────────────────────────────────────────────────────────────────────────────

/// `param k : Stiffness = 5N/m; constraint k > 0` → Satisfied.
///
/// Verifies the zero-coercion works for compound-quotient dimensions
/// (Stiffness = N/m, a product-of-powers dimension vector).  The compound
/// dimension must be correctly adopted into the zero literal — a coercion bug
/// that, for example, adopted the wrong dimension would cause eval_cmp to Undef
/// and return Indeterminate instead.
///
/// Uses check_source_with_stdlib because Stiffness is a stdlib type alias.
#[test]
fn stiffness_gt_zero_satisfied_when_positive() {
    let result = check_source_with_stdlib(
        r#"
structure S {
    param k : Stiffness = 5N / 1m
    constraint k > 0
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
        "k (5N/m) > 0 should be Satisfied after zero-coercion, got {:?}",
        result.constraint_results[0].satisfaction
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Additive-position value-preservation (Suggestion 3)
// ────────────────────────────────────────────────────────────────────────────

/// `let m : Mass = mass + 0` where mass = 3kg → m equals mass (value preserved).
///
/// Confirms that the additive zero-coercion not only avoids a type error but also
/// computes the correct numeric result: `3kg + 0` evaluates to `3kg`, not 0 or
/// some spurious magnitude.  The equality constraint would fail if the coercion
/// introduced a wrong value.
#[test]
fn mass_add_zero_preserves_value() {
    let result = check_source(
        r#"
structure S {
    param mass : Mass = 3kg
    let m : Mass = mass + 0
    constraint m == mass
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
        "mass + 0 should yield mass (3kg == 3kg), got {:?}",
        result.constraint_results[0].satisfaction
    );
}

/// `let m : Mass = mass - 0` where mass = 3kg → m equals mass (value preserved).
///
/// Symmetric to mass_add_zero_preserves_value: subtraction of zero must not
/// negate or corrupt the original quantity.
#[test]
fn mass_sub_zero_preserves_value() {
    let result = check_source(
        r#"
structure S {
    param mass : Mass = 3kg
    let m : Mass = mass - 0
    constraint m == mass
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
        "mass - 0 should yield mass (3kg == 3kg), got {:?}",
        result.constraint_results[0].satisfaction
    );
}
