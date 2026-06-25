//! Tests for `LetAnnotationTypeMismatch` — let binding declared-type vs
//! initializer-type mismatch detected at the declaration site.
//!
//! This is the let analogue of `ParamDefaultTypeMismatch` (#4318).
//!
//! Step 1: RED (headline + positive guard). Tests compile against current main
//! without referencing `DiagnosticCode` (that variant does not exist yet).
//! The headline test is RED today (silent accept); the guard already passes.
//!
//! Step 3 (appended later): References `DiagnosticCode::LetAnnotationTypeMismatch`
//! introduced in step 2; adds numeric-idiom, code assertion, Int-arm, wrong-type,
//! anti-cascade, scope-guard, and unresolved-annotation tests.
//!
//! Step 5 (appended later): RED for port-member let site (site 2).

use reify_test_support::{compile_source, errors_only};

// ─── Step-1 tests ─────────────────────────────────────────────────────────────

/// A structure-body `let a : Length = 5kg` must produce ≥1 error whose message
/// mentions the let name `a`, the word "declared", and the word "initializer";
/// the first label span must cover the `let a` declaration.
///
/// RED until step-2 wires `check_let_annotation_type` at the structure-body let site.
#[test]
fn let_annotation_dimension_mismatch_errors_at_declaration() {
    let source = r#"
structure S {
    let a : Length = 5kg
}
"#;

    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for let annotation dimension mismatch, got none; \
         diagnostics: {:?}",
        module.diagnostics
    );

    // Some error must mention the let name 'a', "declared", and "initializer".
    let mismatch_diag = errors.iter().find(|d| {
        d.message.contains('a')
            && d.message.contains("declared")
            && d.message.contains("initializer")
    });
    assert!(
        mismatch_diag.is_some(),
        "expected an error mentioning 'a', 'declared', and 'initializer'; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // The first label span must cover the `let a` declaration.
    let diag = mismatch_diag.unwrap();
    assert!(
        !diag.labels.is_empty(),
        "expected diagnostic to have at least one label; diag: {:?}",
        diag
    );
    let span = diag.labels[0].span;
    let sliced = &source[span.start as usize..span.end as usize];
    assert!(
        sliced.contains("let a"),
        "expected label span to cover the let declaration containing 'let a', \
         but span covers: {:?}",
        sliced
    );
}

/// Cross-dimension variant: `let a : Length = 5N` (Newtons ≠ meters) must also
/// produce ≥1 "declared"/"initializer" error anchored at the `let a` declaration.
///
/// RED until step-2.
#[test]
fn let_annotation_dimension_mismatch_force_unit_errors_at_declaration() {
    let source = r#"
structure S {
    let a : Length = 5N
}
"#;

    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for 'let a : Length = 5N' (force ≠ length), got none; \
         diagnostics: {:?}",
        module.diagnostics
    );

    let mismatch_diag = errors.iter().find(|d| {
        d.message.contains('a')
            && d.message.contains("declared")
            && d.message.contains("initializer")
    });
    assert!(
        mismatch_diag.is_some(),
        "expected an error mentioning 'a', 'declared', and 'initializer' for 5N case; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let diag = mismatch_diag.unwrap();
    assert!(
        !diag.labels.is_empty(),
        "expected diagnostic to have at least one label for 5N case; diag: {:?}",
        diag
    );
    let span = diag.labels[0].span;
    let sliced = &source[span.start as usize..span.end as usize];
    assert!(
        sliced.contains("let a"),
        "expected label span to cover 'let a' for 5N case, but span covers: {:?}",
        sliced
    );
}

/// Valid let bindings with compatible annotations must NOT produce any
/// "declared … initializer" diagnostics.
///
/// - `let v : Length = 6mm`    — exact match (ok)
/// - `let c : Length = b * 2.0` — Length*Real = Length via algebra (ok)
/// - `let r : Real = 8.0`      — exact Real match (ok)
/// - `let n : Real = 8`        — Int→Real widening via type_compatible (ok)
///
/// This positive guard passes throughout all steps.
#[test]
fn valid_let_annotations_do_not_produce_declared_initializer_error() {
    let source = r#"
structure S {
    param b : Length = 3mm
    let v : Length = 6mm
    let c : Length = b * 2.0
    let r : Real = 8.0
    let n : Real = 8
}
"#;

    let module = compile_source(source);
    let errors = errors_only(&module);

    // No error should carry both "declared" and "initializer" in its message.
    let false_positive = errors
        .iter()
        .find(|d| d.message.contains("declared") && d.message.contains("initializer"));
    assert!(
        false_positive.is_none(),
        "unexpected 'declared/initializer' error on valid let annotations: {:?}",
        false_positive
    );
}
