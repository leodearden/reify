//! Compile-breadth tests for the polymorphic-zero comparison coercion (task 4485/β, §7.2).
//!
//! Tests verify that comparison expressions `member > 0` / `0 < member` produce
//! NO error diagnostics for every dimension family touched by the stdlib migration:
//! - Base dimensions: Length, Mass
//! - Compound-product: MomentOfInertia (kg·m²)
//! - Compound-quotient: Stiffness (N/m), Velocity (m/s)
//!
//! All comparison tests compile without error diagnostics (infer_binop_type
//! returns Bool unconditionally for comparisons), so they serve as a regression net.
//! The eval signal (polymorphic_zero_eval.rs) proves the coercion fires at runtime
//! and produces Satisfaction::Satisfied, including for compound dimensions (Stiffness).
//!
//! Step-5 tests (additive position + edge/negative cases) are added in the same
//! file: the additive tests confirm the coercion fires before the Add/Sub dimension
//! guard, so `dimensioned ± 0` compiles without error.

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
/// The compile-time rewrite promotes `0` to `Scalar<Mass>(0.0)` before the
/// Add/Sub dimension guard runs, so no "incompatible types" error is emitted.
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
/// Zero-coercion promotes `0` to `Scalar<Mass>(0.0)`; the dimension guard sees
/// matching types on both sides and emits no error.
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
/// `is_syntactic_zero_literal` recurses through the `UnOp{"-"}` wrapper, treating
/// `-0` as a syntactic zero; the coercion adopts the Mass dimension and no error
/// diagnostic is emitted.
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

/// `mass > 1 - 1` — constant-folded zero: not a *syntactic* literal, but a
/// dimensionless constant expression that folds to exactly `0`.
///
/// `1 - 1` is `ExprKind::BinOp`, not `NumberLiteral`, so `is_syntactic_zero_literal`
/// returns false — but `coerce_zero_operand` also recognizes constant-folded zeros
/// (via `const_numeric_value`, task 4490): a dimensionless folded zero adopts the
/// dimensioned sibling (`Mass`) and the comparison compiles clean. (A *dimensioned*
/// folded zero like `mass > 1m - 1m` is NOT coerced and still errors.)
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

// ────────────────────────────────────────────────────────────────────────────
// task 6038 — operand shapes asserted by the corrected stdlib esc-3115-112
// comments
//
// The stdlib comment sweep restates several `> 0N` / `> 0Hz` / `>= 0kg` /
// `> 0 * 1N * 1s` constraint literals as a readability CONVENTION rather than a
// requirement, on the grounds that a bare `0` would compile too. These cases
// pin that claim for the dimension families and the operator those sites use
// but which the Step-3/Step-5 sections above do not exercise, so a regression
// cannot silently re-stale the corrected comments.
// ────────────────────────────────────────────────────────────────────────────

/// `member > 0` — base-ish named dimension Force (N).
///
/// Backs the corrected notes on `StepForce.magnitude > 0N` and
/// `HarmonicForce.amplitude > 0N` (modal_analysis.ri) and
/// `JointLimit.max_force` (trajectory.ri).
#[test]
fn force_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param magnitude : Force = 1N
    constraint magnitude > 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "force > 0 comparison");
}

/// `member > 0` — named dimension Frequency (Hz).
///
/// Backs the corrected notes on `HarmonicForce.frequency > 0Hz`
/// (modal_analysis.ri) and the three `target_frequency > 0Hz` shaper
/// constraints (ZVShaper / ZVDShaper / EIShaper, trajectory.ri).
#[test]
fn frequency_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param frequency : Frequency = 1Hz
    constraint frequency > 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "frequency > 0 comparison");
}

/// `member >= 0` — the `>=` operator, which no case above exercises.
///
/// `coerce_zero_operand` runs in `compile_binop` before `infer_binop_type`, so
/// it is operator-agnostic; this pins that for the one stdlib site that uses a
/// non-strict bound, `MassProperties`' `constraint mass >= 0kg` (dynamics.ri).
#[test]
fn mass_ge_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param mass : Mass = 1kg
    constraint mass >= 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "mass >= 0 comparison");
}

/// `member > 0` — compound-product dimension Impulse (N·s = kg·m·s⁻¹).
///
/// Backs the corrected note on `ImpulseForce.impulse > 0 * 1N * 1s`
/// (modal_analysis.ri), whose stale text claimed the dimensioned-zero form was
/// needed "because polymorphic-zero has not landed".
#[test]
fn impulse_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param impulse : Impulse = 1N * 1s
    constraint impulse > 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "impulse > 0 comparison");
}
