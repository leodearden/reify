//! Tests for `ParamDefaultTypeMismatch` — param declared-type vs initializer-dimension
//! mismatch detected at the declaration site.
//!
//! Step 1: RED (top-level + guard). Tests compile against current main without
//! referencing `DiagnosticCode` (that variant does not exist yet).
//!
//! Step 3 (appended later): RED (port-member). References `DiagnosticCode::ParamDefaultTypeMismatch`
//! which is introduced in step 2.

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
