//! Tests for `ParamDefaultTypeMismatch` — param declared-type vs initializer-dimension
//! mismatch detected at the declaration site.
//!
//! Step 1: RED (top-level + guard). Tests compile against current main without
//! referencing `DiagnosticCode` (that variant does not exist yet).
//!
//! Step 3 (appended later): RED (port-member). References `DiagnosticCode::ParamDefaultTypeMismatch`
//! which is introduced in step 2.
//!
//! Amendment pass: additional coverage tests for:
//!   - Int-literal guard (`param x : Length = 1` must NOT error)
//!   - Real-literal guard (`param x : Length = 0.5` must NOT error — extended in amendment)
//!   - Cross-dimension scalar mismatch (`param x : Length = 5kg` MUST error)
//!   - Reciprocal-dimension non-literal expression (literal-only guard → active error)

use reify_core::DiagnosticCode;
use reify_test_support::{compile_source, errors_only};

/// A structure with `param drum_d : Real = rope_dia * d_ratio` where `rope_dia` has
/// declared type `Length` (Scalar[m]) must produce an error anchored at the param
/// declaration, not at a downstream consumer.
///
/// RED until step-2 wires the check at the top-level structure-param arm.
#[test]
fn top_level_param_dimension_mismatch_errors_at_declaration() {
    let source = r#"
structure T {
    param rope_dia : Length = 6mm
    param d_ratio : Real = 8.0
    param drum_d : Real = rope_dia * d_ratio
}
"#;

    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for param dimension mismatch, got none; diagnostics: {:?}",
        module.diagnostics
    );

    // Some error must mention drum_d, Real, and Scalar[m].
    let mismatch_diag = errors.iter().find(|d| {
        d.message.contains("drum_d")
            && d.message.contains("Real")
            && d.message.contains("Scalar[m]")
    });
    assert!(
        mismatch_diag.is_some(),
        "expected an error mentioning drum_d, Real, and Scalar[m]; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // The diagnostic must be anchored at the param declaration (first label span
    // contains "drum_d").
    let diag = mismatch_diag.unwrap();
    assert!(
        !diag.labels.is_empty(),
        "expected diagnostic to have at least one label; diag: {:?}",
        diag
    );
    let span = diag.labels[0].span;
    let sliced = &source[span.start as usize..span.end as usize];
    assert!(
        sliced.contains("drum_d"),
        "expected label span to cover the param declaration containing 'drum_d', \
         but span covers: {:?}",
        sliced
    );
}

/// A structure with valid param annotations — including Int→Real widening, undef
/// default, and correct declared dimensions — must NOT produce any
/// "declared … initializer" diagnostics.
///
/// This guard test passes throughout all steps.
#[test]
fn valid_and_widening_param_annotations_do_not_error() {
    let source = r#"
structure Valid {
    param a : Real = 8.0
    param b : Length = 6mm
    param c : Length = b * 2.0
    param n : Real = 8
    param u : Real = undef
}
"#;

    let module = compile_source(source);
    let errors = errors_only(&module);

    // No error should carry both "declared" and "initializer" in its message.
    let false_positive = errors.iter().find(|d| {
        d.message.contains("declared") && d.message.contains("initializer")
    });
    assert!(
        false_positive.is_none(),
        "unexpected 'declared/initializer' error on valid param annotations: {:?}",
        false_positive
    );
}

/// A port-member `param d : Real = rope_dia * 2.0` where `rope_dia : Length` must
/// produce a `ParamDefaultTypeMismatch` error anchored at the port-member param
/// declaration ("`param d`"), not at a downstream consumer.
///
/// RED until step-4 wires the check at the port-member param arm (site 2).
#[test]
fn port_member_param_dimension_mismatch_errors_at_declaration() {
    let source = r#"
trait T { param d : Real }
structure S {
    param rope_dia : Length = 6mm
    port p : out T { param d : Real = rope_dia * 2.0 }
}
"#;

    let module = compile_source(source);
    let errors = errors_only(&module);

    // Must have at least one error with code ParamDefaultTypeMismatch.
    let mismatch_diag = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        mismatch_diag.is_some(),
        "expected a ParamDefaultTypeMismatch error for port-member param 'd'; got: {:?}",
        errors.iter().map(|d| (&d.message, &d.code)).collect::<Vec<_>>()
    );

    let diag = mismatch_diag.unwrap();
    assert!(
        diag.message.contains("Real") && diag.message.contains("Scalar[m]"),
        "expected error message to mention 'Real' and 'Scalar[m]'; got: {:?}",
        diag.message
    );

    // The label span must cover the port-member param declaration.
    assert!(
        !diag.labels.is_empty(),
        "expected diagnostic to have at least one label; diag: {:?}",
        diag
    );
    let span = diag.labels[0].span;
    let sliced = &source[span.start as usize..span.end as usize];
    assert!(
        sliced.contains("param d"),
        "expected label span to cover the port-member param declaration containing \
         'param d', but span covers: {:?}",
        sliced
    );
}

// ─── Amendment-pass coverage tests ───────────────────────────────────────────

/// The Int-literal and Real-literal idioms for dimensioned Scalar params must
/// NOT produce `ParamDefaultTypeMismatch`:
///
///   `param x : Length = 0`   — Int literal, accepted (whole-number idiom).
///   `param x : Length = 1`   — Int literal, accepted (whole-number idiom).
///   `param x : Length = 0.5` — Real literal, accepted (fractional idiom,
///                               extended in the amendment-pass guard fix).
///   `param x : Length = 70.0` — Real literal, accepted.
///
/// These cover the most novel branch in `check_param_default_type` — the
/// Int-for-Scalar early-return — and its new Real-for-Scalar extension.
#[test]
fn param_int_and_real_literal_on_dimensioned_scalar_do_not_error() {
    let source = r#"
structure S {
    param zero_int   : Length = 0
    param one_int    : Length = 1
    param half_real  : Length = 0.5
    param large_real : Length = 70.0
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let false_pos = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        false_pos.is_none(),
        "unexpected ParamDefaultTypeMismatch for Int/Real literal on Length param; \
         Int and dimensionless-Real literals must be accepted for any dimensioned Scalar; \
         got: {:?}",
        false_pos
    );
}

/// Negative numeric literals (`-5.0`, `-1`) must NOT produce `ParamDefaultTypeMismatch`
/// for dimensioned Scalar params.
///
/// The compiler lowers `-5.0` to `UnOp { Neg, Literal(5.0) }` rather than a bare
/// `Literal`, so the literal-only guard must also cover negated literals.  Without
/// this, `param z : Length = -5.0` would false-positive as a type mismatch.
#[test]
fn param_negative_literal_on_dimensioned_scalar_does_not_error() {
    let source = r#"
structure S {
    param neg_real : Length = -5.0
    param neg_int  : Length = -1
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let false_pos = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        false_pos.is_none(),
        "unexpected ParamDefaultTypeMismatch for negated literal on Length param; \
         negative numeric literals (-5.0, -1) must be accepted for any dimensioned Scalar; \
         got: {:?}",
        false_pos
    );
}

/// A param whose declared type is a dimensioned Scalar but whose initializer
/// evaluates to a *different* dimensioned Scalar (e.g. `Length = 5kg`) MUST
/// produce `ParamDefaultTypeMismatch` — this is the primary intended catch of
/// the check.  Scalar[m] and Scalar[kg] are incompatible under `type_compatible`.
#[test]
fn param_different_scalar_dimensions_error() {
    let source = r#"
structure S {
    param bad_mass : Length = 5kg
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        mismatch.is_some(),
        "expected ParamDefaultTypeMismatch for 'param bad_mass : Length = 5kg' \
         (Scalar[m] ≠ Scalar[kg]); got: {:?}",
        errors.iter().map(|d| (&d.message, &d.code)).collect::<Vec<_>>()
    );

    let diag = mismatch.unwrap();
    assert!(
        diag.message.contains("bad_mass"),
        "error message should mention 'bad_mass'; got: {:?}",
        diag.message
    );
}

// ─── Step-6 regression: untyped params must NOT trigger ParamDefaultTypeMismatch ──

/// An untyped port-member param with an enum default must NOT produce a
/// `ParamDefaultTypeMismatch` diagnostic.  For an untyped param the compiler
/// assigns a `Type::Real` inference fallback (entity.rs port-member first pass),
/// NOT a user-declared type, so the declared-vs-initializer cross-check must be
/// suppressed entirely.
///
/// This is the reviewer's exact locus (enum-valued port-param override).
/// RED until step-7 gates `check_param_default_type` on `has_explicit_type`.
#[test]
fn untyped_port_member_param_with_enum_default_does_not_error() {
    let source = r#"
enum FluidType { Liquid, Gas }
trait T { param fluid_type : FluidType }
structure S {
    port p : out T { param fluid_type = FluidType.Liquid }
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let false_positive = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        false_positive.is_none(),
        "untyped port-member param 'fluid_type = FluidType.Liquid' must NOT produce \
         ParamDefaultTypeMismatch (no explicit type annotation; cell_type is only a \
         Type::Real inference fallback, not a user contract); got: {:?}",
        false_positive
    );
}

/// An untyped top-level structure param with an enum default must NOT produce a
/// `ParamDefaultTypeMismatch` diagnostic.
#[test]
fn untyped_top_level_param_with_enum_default_does_not_error() {
    let source = r#"
enum Color { Red, Green }
structure S {
    param c = Color.Red
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let false_positive = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        false_positive.is_none(),
        "untyped top-level param 'c = Color.Red' must NOT produce ParamDefaultTypeMismatch \
         (no explicit type annotation; Type::Real is only an inference fallback, not a \
         user-declared type); got: {:?}",
        false_positive
    );
}

/// An untyped top-level structure param with a dimensioned-unit default must NOT
/// produce a `ParamDefaultTypeMismatch` diagnostic.
#[test]
fn untyped_top_level_param_with_dimensioned_default_does_not_error() {
    let source = r#"
structure S {
    param x = 5mm
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let false_positive = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        false_positive.is_none(),
        "untyped top-level param 'x = 5mm' must NOT produce ParamDefaultTypeMismatch \
         (no explicit type annotation; Type::Real is only an inference fallback, not a \
         user contract for Length); got: {:?}",
        false_positive
    );
}

/// A non-literal initializer (`1.0 / 1m` is a BinOp Div, not a
/// `CompiledExprKind::Literal`) for a dimensioned Scalar param (`Length`) MUST
/// produce `ParamDefaultTypeMismatch`.
///
/// **Why this works (the literal-guard mechanism):** the Scalar guard in
/// `check_param_default_type` is restricted to `CompiledExprKind::Literal` nodes.
/// `1.0 / 1m` is a binary division expression — not a literal — so the guard does
/// NOT fire regardless of the expression's inferred result_type.  The check falls
/// through to `type_compatible(Scalar[m], <inferred>)`, which returns `false`
/// whether `1.0/1m` is inferred as dimensionless (`Scalar[]`) or as the more
/// precise `Scalar[1/m]` — the mismatch is flagged either way.
///
/// Note: a future inference improvement that tracks `1.0/1m` as `Scalar[1/m]`
/// rather than dimensionless would only refine the printed dimension string in the
/// error message; it would NOT affect whether an error fires.  The inference gap
/// is therefore genuinely out of scope and is NOT a prerequisite for this test.
///
/// This test is the active regression proof for the literal-only guard (S1): it
/// would regress to a false-negative if the guard were ever relaxed back to keying
/// on result_type alone (which would silently accept any dimensionless expression).
#[test]
fn param_reciprocal_dim_mismatch_errors() {
    let source = r#"
structure S {
    // Length (Scalar[m]) ≠ reciprocal-dimension expression (non-literal BinOp).
    // The literal-only guard does NOT fire; falls through to type_compatible → error.
    param bad_dim : Length = 1.0 / 1m
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        mismatch.is_some(),
        "expected ParamDefaultTypeMismatch for 'param bad_dim : Length = 1.0/1m' \
         (non-literal initializer bypasses the literal guard and falls through to \
         type_compatible(Scalar[m], _) which is false)"
    );
}

// ─── S1 amendment: canonical literal-only guard proof ────────────────────────

/// A non-literal compound expression that *happens to infer a dimensionless
/// result* MUST still produce `ParamDefaultTypeMismatch` when assigned to a
/// dimensioned Scalar param.
///
/// `ratio * 2.0` is a `BinOp(Mul)` expression — NOT a `CompiledExprKind::Literal`
/// — that infers `Scalar[dimensionless]` (product of a dimensionless Real and a
/// scalar 2.0).  Without the literal-only guard the Scalar early-return would fire
/// on the dimensionless `result_type` and silently accept the mismatch.  With the
/// literal-only guard in place `BinOp(Mul)` is NOT a literal, the guard does NOT
/// fire, and the check falls through to
/// `type_compatible(Scalar[m], Scalar[dimensionless])` which returns false →
/// `ParamDefaultTypeMismatch` is correctly emitted.
///
/// This is the canonical S1 proof: it would regress to a false-negative if the
/// guard were ever relaxed back to keying on `result_type` alone (which would
/// silently accept any dimensionless expression, literal or compound).
#[test]
fn param_nonliteral_dimensionless_compound_on_dimensioned_scalar_errors() {
    let source = r#"
structure S {
    param ratio : Real   = 8.0
    param x     : Length = ratio * 2.0
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        mismatch.is_some(),
        "expected ParamDefaultTypeMismatch for 'param x : Length = ratio * 2.0' \
         (ratio*2.0 infers dimensionless Scalar but is a BinOp, not a literal; \
         the literal-only guard does NOT fire; falls through to \
         type_compatible(Scalar[m], dimensionless) = false); got: {:?}",
        errors.iter().map(|d| (&d.message, &d.code)).collect::<Vec<_>>()
    );
}

// ─── S2 amendment: Int-arm + anti-cascade coverage ───────────────────────────

/// A param declared `Int` with a dimensioned initializer (`5kg`) MUST produce
/// `ParamDefaultTypeMismatch`.  The `Int` arm of the declared-type guard is not
/// affected by the Scalar literal early-return, so it falls directly through to
/// `type_compatible(Int, Scalar[kg])` which returns false.
#[test]
fn param_int_declared_with_dimensioned_initializer_errors() {
    let source = r#"
structure S {
    param x : Int = 5kg
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        mismatch.is_some(),
        "expected ParamDefaultTypeMismatch for 'param x : Int = 5kg' \
         (Int ≠ Scalar[kg]); got: {:?}",
        errors.iter().map(|d| (&d.message, &d.code)).collect::<Vec<_>>()
    );
}

/// A param declared `Int` with a fractional Real initializer (`0.5`) MUST produce
/// `ParamDefaultTypeMismatch`.  `type_compatible(Int, Scalar[dimensionless])` returns
/// false because the Int→dimensionless-scalar widening coercion is one-directional:
/// it allows `Int` where `Scalar[dimensionless]` is declared, NOT the reverse.
#[test]
fn param_int_declared_with_real_initializer_errors() {
    let source = r#"
structure S {
    param x : Int = 0.5
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        mismatch.is_some(),
        "expected ParamDefaultTypeMismatch for 'param x : Int = 0.5' \
         (Int ≠ Real/dimensionless-scalar); got: {:?}",
        errors.iter().map(|d| (&d.message, &d.code)).collect::<Vec<_>>()
    );
}

/// A `Real`-declared param with a non-literal reciprocal-dimension initializer
/// (`1.0/1m`) MUST produce `ParamDefaultTypeMismatch`.
///
/// **Why this works:** `1.0/1m` correctly infers as `Scalar[1/m]` (reciprocal
/// dimension) — no inference gap exists here.  The literal-only guard does NOT
/// fire (BinOp Div is not a literal), so the check falls through to
/// `type_compatible(Real/dimensionless, Scalar[1/m])` which returns false
/// (distinct Scalar types: dimensionless ≠ 1/m).
///
/// Note: this is the *declared=Real* side of the S3 asymmetry, complementary
/// to `param_reciprocal_dim_mismatch_errors` which covers the *declared=Length*
/// side.  Both cases fire because `type_compatible` has no Scalar→Scalar
/// widening rule: only the `Int→dimensionless` coercion and the identity
/// short-circuit exist.
#[test]
fn param_real_declared_with_reciprocal_dim_initializer_errors() {
    let source = r#"
structure S {
    // Real (dimensionless) declared, but 1.0/1m is a reciprocal-dimension expression.
    // 1.0/1m correctly infers as Scalar[1/m]; type_compatible(Real, Scalar[1/m]) = false.
    param bad_dim : Real = 1.0 / 1m
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        mismatch.is_some(),
        "expected ParamDefaultTypeMismatch for 'param bad_dim : Real = 1.0/1m' \
         (Real/dimensionless ≠ Scalar[1/m]; 1.0/1m correctly infers as reciprocal dimension); \
         got: {:?}",
        errors.iter().map(|d| (&d.message, &d.code)).collect::<Vec<_>>()
    );
}

/// A param whose *declared type* is unresolvable (`Bogus`) produces TWO errors:
/// an UnresolvedType root-cause error AND a secondary `ParamDefaultTypeMismatch`.
///
/// **Why two errors (interim behaviour):** unknown name `Bogus` currently resolves
/// to `Type::dimensionless_scalar()` (i.e. `Type::Real`), NOT `Type::Error`, so
/// the `declared.is_error()` anti-cascade guard in `check_param_default_type` does
/// NOT fire.  The declared type is effectively `Real`, the initializer `5kg` has
/// type `Scalar[kg]`, and `type_compatible(Real, Scalar[kg])` is false → a
/// `ParamDefaultTypeMismatch` is correctly emitted as a second diagnostic.
///
/// This is an **interim state**; once unknown-name resolution returns `Type::Error`
/// instead of `Type::Real`, the anti-cascade guard WILL fire and this test will need
/// to be updated back to expect exactly ONE error (UnresolvedType only, no secondary
/// ParamDefaultTypeMismatch).
#[test]
fn param_unresolved_declared_type_anti_cascade_no_secondary_error() {
    let source = r#"
structure S {
    param p : Bogus = 5kg
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // There must be at least one error (unresolved type `Bogus`).
    assert!(
        !errors.is_empty(),
        "expected at least one error for unresolved type 'Bogus'; got none"
    );

    // A secondary ParamDefaultTypeMismatch IS present because unknown-name
    // 'Bogus' resolves to Type::Real (not Type::Error), so the anti-cascade
    // guard does not fire.  Both errors are expected until the unknown-name→Error
    // root bug is fixed.
    let mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ParamDefaultTypeMismatch));
    assert!(
        mismatch.is_some(),
        "expected a ParamDefaultTypeMismatch for 'param p : Bogus = 5kg' \
         (Bogus resolves to Real, Real ≠ Scalar[kg], so a secondary mismatch IS emitted); \
         got: {:?}",
        errors.iter().map(|d| (&d.message, &d.code)).collect::<Vec<_>>()
    );
}
