//! RED behavioural contract for α (task 4373): delete Type::Real, canonicalize
//! type layer to Scalar{DIMENSIONLESS}.
//!
//! Every assertion in this file **compiles today** (none references Type::Real
//! directly) but the integration tests fail at runtime until step-4 lands the
//! atomic deletion.  The unit tests in type_compat.rs and reify-eval/src/lib.rs
//! mod tests are companion contracts for the same migration.

use reify_core::Severity;
use reify_test_support::compile_source;

// ── (a) Real + Dimensionless addition must compile clean ─────────────────────
//
// Today: `a : Real` has Type::Real; `b : Dimensionless` has Type::Scalar{DL}.
// The Add/Sub arm `(Type::Int | Type::Real, Type::Scalar { .. })` fires and
// emits "incompatible types in addition: Real vs Real".
//
// After α: both `a` and `b` are Type::Scalar{DL}; the arm doesn't fire.
// RED today → GREEN after step-4.
#[test]
fn mixed_real_dimensionless_add_compiles_clean() {
    let source = r"
occurrence def Widget {
    param a : Real
    param b : Dimensionless
    let c = a + b
}
";
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for Real + Dimensionless; got: {:?}",
        errors
    );
}

// ── (b) Dimensionless + Int must compile clean ────────────────────────────────
//
// Today: `b : Dimensionless` has Type::Scalar{DL}; `1` has Type::Int.
// The arm `(Type::Scalar { .. }, Type::Int | Type::Real)` fires and emits
// "incompatible types in addition: Real vs Int".
//
// After α + the !is_dimensionless() guard: the arm is skipped.
// RED today → GREEN after step-4.
#[test]
fn dimensionless_plus_int_compiles_clean() {
    let source = r"
occurrence def Widget {
    param b : Dimensionless
    let c = b + 1
}
";
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for Dimensionless + Int; got: {:?}",
        errors
    );
}

// ── (c) Real + Int stays clean (regression guard) ────────────────────────────
//
// Today: `a : Real` → Type::Real; `1` → Type::Int. Neither Add/Sub arm
// matches `(Type::Real, Type::Int)`, so no error.
//
// After α: `a : Real` → Type::Scalar{DL}. The !is_dimensionless() guard MUST
// be in place so the arm is still skipped for dimensionless + Int.
// GREEN today AND after step-4 (regression guard).
#[test]
fn real_plus_int_stays_clean() {
    let source = r"
occurrence def Widget {
    param a : Real
    let c = a + 1
}
";
    let compiled = compile_source(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics for Real + Int; got: {:?}",
        errors
    );
}

// ── (d) Genuine dimension mismatch prints "Real" for the DL operand ──────────
//
// Today (after step-2 Display change): `r : Dimensionless` has Type::Scalar{DL}
// which now displays as "Real". `L : Length` has Type::Scalar{LENGTH} which
// displays as "Scalar[m]". The dimension-mismatch diagnostic fires and says
// "dimension mismatch in addition: Scalar[m] vs Real".
//
// This test checks: (1) error is emitted, (2) message contains "Real" for the DL side.
// GREEN after step-2 (diagnostic already mentions "Real"); regression guard for step-4.
#[test]
fn genuine_dimension_mismatch_prints_real() {
    let source = r"
occurrence def Widget {
    param length_val : Length
    param r : Dimensionless
    let c = length_val + r
}
";
    let compiled = compile_source(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "expected at least one Error diagnostic for Length + Dimensionless; got none"
    );

    let any_mentions_real = errors.iter().any(|d| d.message.contains("Real"));
    assert!(
        any_mentions_real,
        "expected a dimension-mismatch diagnostic mentioning 'Real' for the DL operand; got: {:?}",
        errors
    );
}
