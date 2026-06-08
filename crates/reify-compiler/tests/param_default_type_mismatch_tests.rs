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
//!   - Known inference gap for reciprocal-dimension expressions (documented as #[ignore])

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

/// Known inference limitation: `1.0 / 1m` is inferred as `Type::Real`
/// (dimensionless) rather than `Type::Scalar[1/m]` (reciprocal-length).
///
/// The Real-literal guard in `check_param_default_type` (extended in this
/// amendment pass) silently accepts this as "dimensionless literal on dimensioned
/// param", preventing a false-positive error.  However, the *correct* behavior
/// once inference is fixed would be to *emit* `ParamDefaultTypeMismatch` because
/// `Length (Scalar[m]) ≠ reciprocal-length (Scalar[1/m])`.
///
/// This `#[ignore]` test asserts the future-correct behavior: that a
/// `param x : Length = 1.0 / 1m` declaration IS an error.  It currently FAILS
/// (no error is produced, because the Real-literal guard skips the check).
/// When the compiler correctly infers `1.0 / 1m` as `Scalar[1/m]`, the
/// Real-literal guard no longer applies and `type_compatible(Scalar[m], Scalar[1/m])`
/// returns `false` → error → this test passes.
///
/// To verify the fix: remove the `Type::Real` arm from the Scalar guard in
/// `check_param_default_type` and confirm this test passes with `--include-ignored`.
#[test]
#[ignore = "inference gap: 1.0/1m infers Real not Scalar[1/m]; unignore when inference fixed"]
fn param_reciprocal_dim_mismatch_detected_after_inference_fix() {
    let source = r#"
structure S {
    // Length (Scalar[m]) ≠ reciprocal-length (Scalar[1/m]).
    // When inference tracks 1.0/1m as Scalar[1/m], this MUST be a mismatch error.
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
        "expected ParamDefaultTypeMismatch for 'Length = 1.0/1m' once inference is fixed; \
         currently passes via the Real-literal guard (1.0/1m infers Real, not Scalar[1/m])"
    );
}
