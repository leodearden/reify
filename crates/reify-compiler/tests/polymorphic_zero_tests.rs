//! Compile-breadth tests for the polymorphic-zero comparison coercion (task 4485/β, §7.2).
//!
//! Tests verify that comparison expressions `member > 0` / `0 < member` produce
//! NO error diagnostics for every dimension family touched by the stdlib migration:
//! - Base dimensions: Length, Mass
//! - Compound-product: MomentOfInertia (kg·m²)
//! - Compound-quotient: Stiffness (N/m), Velocity (m/s)
//!
//! All comparison tests compile without error diagnostics today (infer_binop_type
//! returns Bool unconditionally for comparisons), so they serve as a regression net.
//! The eval signal (step-3a in polymorphic_zero_eval.rs) is what is RED today.
//!
//! Step-5 tests (additive position + edge/negative cases) are added in the same
//! file: additive assertions are RED today (the Add/Sub dimension guard at
//! expr.rs:1131 emits an error for dimensioned + dimensionless).

use reify_test_support::{assert_no_error_diagnostics, compile_source_with_stdlib};

// ────────────────────────────────────────────────────────────────────────────
// Step-3 (b): comparison-position breadth
// ────────────────────────────────────────────────────────────────────────────

/// `member > 0` — base dimension Length (right-is-zero form).
#[test]
fn length_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param len : Length = 1m
    constraint len > 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "length > 0 comparison");
}

/// `0 < member` — base dimension Length (left-is-zero form).
#[test]
fn zero_lt_length_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param len : Length = 1m
    constraint 0 < len
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "0 < length comparison");
}

/// `member > 0` — base dimension Mass.
#[test]
fn mass_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param mass : Mass = 1kg
    constraint mass > 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "mass > 0 comparison");
}

/// `member > 0` — compound-product dimension MomentOfInertia (kg·m²).
#[test]
fn moment_of_inertia_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param moi : MomentOfInertia = 1kg * 1m * 1m
    constraint moi > 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "moment_of_inertia > 0 comparison");
}

/// `member > 0` — compound-quotient dimension Stiffness (N/m).
#[test]
fn stiffness_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param k : Stiffness = 1N / 1m
    constraint k > 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "stiffness > 0 comparison");
}

/// `member > 0` — compound-quotient dimension Velocity (m/s).
#[test]
fn velocity_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param v : Velocity = 1m / 1s
    constraint v > 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "velocity > 0 comparison");
}

// ────────────────────────────────────────────────────────────────────────────
// Step-5: additive-position + edge/negative cases
// ────────────────────────────────────────────────────────────────────────────

/// `mass + 0` — additive-position zero coercion (right-is-zero form).
///
/// RED today: the Add/Sub dimension guard at expr.rs:1131 emits an error
/// "incompatible types in addition: Mass vs Int" before the coercion lands.
#[test]
fn mass_add_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param mass : Mass = 3kg
    let m : Mass = mass + 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "mass + 0 additive coercion");
}

/// `mass - 0` — additive-position zero coercion (subtract zero form).
///
/// RED today: same dimension guard error as mass + 0.
#[test]
fn mass_sub_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param mass : Mass = 3kg
    let m : Mass = mass - 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "mass - 0 subtractive coercion");
}

/// `constraint mass > -0` — unary-neg zero form in comparison (right-is-zero via UnOp{"-"}).
///
/// `is_syntactic_zero_literal` recurses through `-0` → should coerce like `> 0`.
/// RED today: same as mass > 0 (the coercion is not yet implemented).
/// After step-4 impl: no error diagnostic.
#[test]
fn mass_gt_neg_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param mass : Mass = 1kg
    constraint mass > -0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "mass > -0 unary-neg zero comparison");
}

/// `constraint mass > -0.0` — unary-neg real zero form.
#[test]
fn mass_gt_neg_zero_real_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param mass : Mass = 1kg
    constraint mass > -0.0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "mass > -0.0 unary-neg real zero");
}

// ────────────────────────────────────────────────────────────────────────────
// Step-5 negative/no-op cases (must NOT change current behaviour)
// ────────────────────────────────────────────────────────────────────────────

/// `0 > 0` — both-zero: no coercion; compiles without error, both operands stay Int.
///
/// The gating predicate requires the OTHER operand to be Scalar<D> with
/// !D.is_dimensionless(). When both are zero/dimensionless, no adoption occurs.
#[test]
fn both_zero_comparison_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    constraint 0 > 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "0 > 0 both-zero should compile");
}

/// `0 > 1.0` — dimensionless sibling: no coercion; compiles without error.
///
/// The sibling is dimensionless_scalar (D.is_dimensionless()), so no adoption.
#[test]
fn dimensionless_sibling_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    constraint 0 > 1.0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "0 > 1.0 dimensionless sibling");
}

/// `mass > 1 - 1` — constant-folded zero: NOT a syntactic literal, not coerced.
///
/// `1 - 1` is `ExprKind::BinOp`, not `NumberLiteral`, so `is_syntactic_zero_literal`
/// returns false. Today this emits no error (comparisons accept any types);
/// after coercion lands it should still emit no error (the predicate correctly
/// excludes it from coercion, and comparison type inference returns Bool regardless).
#[test]
fn constant_folded_zero_not_coerced_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param mass : Mass = 1kg
    constraint mass > 1 - 1
}
"#,
    );
    assert_no_error_diagnostics(
        &compiled.diagnostics,
        "mass > 1-1 constant-folded zero no extra error",
    );
}
