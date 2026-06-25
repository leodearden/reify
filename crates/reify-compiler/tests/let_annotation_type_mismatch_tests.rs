//! Tests for `LetAnnotationTypeMismatch` — let binding declared-type vs
//! initializer-type mismatch detected at the declaration site.
//!
//! This is the let analogue of `ParamDefaultTypeMismatch` (#4318).
//!
//! Step 1: RED (headline + positive guard). Tests compile against current main
//! without referencing `DiagnosticCode` (that variant does not exist yet).
//! The headline test is RED today (silent accept); the guard already passes.
//!
//! Step 3 (appended): References `DiagnosticCode::LetAnnotationTypeMismatch`
//! introduced in step 2; adds numeric-idiom (RED until step-4), code assertion,
//! Int-arm, wrong-type, anti-cascade, scope-guard, and unresolved-annotation tests.
//!
//! Step 5 (appended later): RED for port-member let site (site 2).

use reify_core::DiagnosticCode;
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

// ─── Step-3 tests (appended) ──────────────────────────────────────────────────

/// The numeric-literal idiom — whole-number, fractional, and negated literals
/// for a dimensioned Scalar let — must NOT produce `LetAnnotationTypeMismatch`.
///
///   `let x : Length = 5`   — Int literal, accepted.
///   `let y : Length = 0.5` — dimensionless Real literal, accepted.
///   `let z : Length = -5.0` — negated Real literal, accepted.
///
/// RED until step-4 adds the numeric-literal guard to `check_let_annotation_type`.
/// Mirrors `param_int_and_real_literal_on_dimensioned_scalar_do_not_error` and
/// `param_negative_literal_on_dimensioned_scalar_does_not_error`.
#[test]
fn let_annotation_int_and_real_literal_on_dimensioned_scalar_do_not_error() {
    let source = r#"
structure S {
    let x : Length = 5
    let y : Length = 0.5
    let z : Length = -5.0
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let false_pos = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::LetAnnotationTypeMismatch));
    assert!(
        false_pos.is_none(),
        "unexpected LetAnnotationTypeMismatch for Int/Real literal on Length let; \
         Int and dimensionless-Real literals must be accepted for any dimensioned Scalar; \
         got: {:?}",
        false_pos
    );
}

/// The headline `let a : Length = 5kg` must carry
/// `code == DiagnosticCode::LetAnnotationTypeMismatch`.
///
/// Already GREEN after step-2. Registers the canonical code assertion.
#[test]
fn let_annotation_mismatch_carries_correct_diagnostic_code() {
    let source = r#"
structure S {
    let a : Length = 5kg
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::LetAnnotationTypeMismatch));
    assert!(
        mismatch.is_some(),
        "expected LetAnnotationTypeMismatch error for 'let a : Length = 5kg'; got: {:?}",
        errors.iter().map(|d| (&d.message, &d.code)).collect::<Vec<_>>()
    );

    let diag = mismatch.unwrap();
    assert!(
        diag.message.contains('a'),
        "error message should mention the let name 'a'; got: {:?}",
        diag.message
    );
}

/// A let declared `Int` with a fractional Real initializer (`0.5`) MUST produce
/// `LetAnnotationTypeMismatch`. `type_compatible(Int, Scalar[dimensionless])` is false
/// (one-directional widening: Int→dimensionless, not the reverse).
#[test]
fn let_int_declared_with_real_initializer_errors() {
    let source = r#"
structure S {
    let i : Int = 0.5
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::LetAnnotationTypeMismatch));
    assert!(
        mismatch.is_some(),
        "expected LetAnnotationTypeMismatch for 'let i : Int = 0.5' (Int ≠ Real); got: {:?}",
        errors.iter().map(|d| (&d.message, &d.code)).collect::<Vec<_>>()
    );
}

/// A let declared `Int` with a dimensioned initializer (`5kg`) MUST produce
/// `LetAnnotationTypeMismatch`. `type_compatible(Int, Scalar[kg])` is false.
#[test]
fn let_int_declared_with_dimensioned_initializer_errors() {
    let source = r#"
structure S {
    let j : Int = 5kg
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::LetAnnotationTypeMismatch));
    assert!(
        mismatch.is_some(),
        "expected LetAnnotationTypeMismatch for 'let j : Int = 5kg' (Int ≠ Scalar[kg]); \
         got: {:?}",
        errors.iter().map(|d| (&d.message, &d.code)).collect::<Vec<_>>()
    );
}

/// A let declared `Length` (scalar) with a Bool initializer (`true`) MUST produce
/// `LetAnnotationTypeMismatch`. `type_compatible(Scalar[m], Bool)` is false.
/// Tests the "wrong-type" path (non-scalar RHS with scalar declared type).
#[test]
fn let_scalar_declared_with_bool_initializer_errors() {
    let source = r#"
structure S {
    let a : Length = true
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::LetAnnotationTypeMismatch));
    assert!(
        mismatch.is_some(),
        "expected LetAnnotationTypeMismatch for 'let a : Length = true' \
         (scalar declared, Bool RHS); got: {:?}",
        errors.iter().map(|d| (&d.message, &d.code)).collect::<Vec<_>>()
    );
}

/// Anti-cascade regression lock: `let a : Length = []` must produce EXACTLY ONE error
/// and its code must be `CollectionLiteralKindMismatch` (from β/#4702),
/// NOT `LetAnnotationTypeMismatch`.
///
/// β owns collection-literal-vs-annotation. The `rhs_is_collection_literal` skip in
/// `check_let_annotation_type` prevents the double-fire.
#[test]
fn let_annotation_empty_list_anti_cascade_single_collection_error() {
    let source = r#"
structure S {
    let a : Length = []
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // Must have exactly one error.
    assert!(
        errors.len() == 1,
        "expected exactly one error for 'let a : Length = []'; got: {:?}",
        errors.iter().map(|d| (&d.message, &d.code)).collect::<Vec<_>>()
    );

    // That error must be CollectionLiteralKindMismatch (β's code), NOT LetAnnotationTypeMismatch.
    assert!(
        errors[0].code == Some(DiagnosticCode::CollectionLiteralKindMismatch),
        "expected CollectionLiteralKindMismatch for 'let a : Length = []'; \
         got code: {:?}, message: {:?}",
        errors[0].code,
        errors[0].message
    );

    let no_let_mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::LetAnnotationTypeMismatch));
    assert!(
        no_let_mismatch.is_none(),
        "LetAnnotationTypeMismatch must NOT fire for a collection-literal RHS \
         (β owns this via CollectionLiteralKindMismatch); got: {:?}",
        no_let_mismatch
    );
}

/// Scope-guard (bullet-B deferred): `let xs : List<Length> = [1N]` must produce
/// NO `LetAnnotationTypeMismatch`. The declared type is `List<Length>` — not scalar —
/// so the scalar restriction in `check_let_annotation_type` skips the check.
/// Non-empty collection element conformance is the filed follow-up (#4705 §B).
#[test]
fn let_annotation_list_type_declared_no_let_annotation_mismatch() {
    let source = r#"
structure S {
    let xs : List<Length> = [1N]
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let let_mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::LetAnnotationTypeMismatch));
    assert!(
        let_mismatch.is_none(),
        "LetAnnotationTypeMismatch must NOT fire for 'let xs : List<Length> = [1N]' \
         (declared is List — not scalar; scalar restriction skips); got: {:?}",
        let_mismatch
    );
}

/// Unresolved-annotation guard: `let a : Bogus = 5` must produce NO
/// `LetAnnotationTypeMismatch`. When the annotation type fails to resolve,
/// `expected_ty` is `None` and the check is skipped entirely — preserving
/// current behavior for unresolved let annotations.
#[test]
fn let_annotation_unresolved_type_no_let_annotation_mismatch() {
    let source = r#"
structure S {
    let a : Bogus = 5
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // There may be an UnresolvedType error, but there must be NO LetAnnotationTypeMismatch.
    let let_mismatch = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::LetAnnotationTypeMismatch));
    assert!(
        let_mismatch.is_none(),
        "LetAnnotationTypeMismatch must NOT fire for 'let a : Bogus = 5' \
         (unresolved annotation -> expected_ty=None -> check skipped); got: {:?}",
        let_mismatch
    );
}
