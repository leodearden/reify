//! Integration tests for β: expected-type pushdown at the let-binding position.
//!
//! PRD: docs/prds/expected-type-pushdown.md §7 (let-position signal).
//!
//! RED for step-3 (tests #1/#2/#3/#7/#7b fail until step-4 wires the let annotation):
//!   - Positive (annotated matching-kind empty literals must NOT warn after impl).
//!   - Negative (annotated non-matching-kind literals must error after impl).
//! GREEN invariant guards (#4/#8) must stay green both before and after the impl.

use reify_core::DiagnosticCode;
use reify_test_support::{compile_source, errors_only, warnings_only};

// ── positive: annotated matching-kind literals (#1/#2/#3) ────────────────────

/// `let xs : List<Length> = []` must NOT emit a "cannot infer element type"
/// warning because the annotation resolves the element type.
///
/// RED until step-4 wires the let path to push the annotation as expected_type.
#[test]
fn let_annotated_list_empty_literal_no_cannot_infer_warning() {
    let source = r#"
structure S {
    let xs : List<Length> = []
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for `let xs : List<Length> = []`, got: {:?}",
        errors
    );
    let warnings = warnings_only(&module);
    let has_infer_warning = warnings
        .iter()
        .any(|d| d.message.contains("cannot infer") || d.message.contains("empty list"));
    assert!(
        !has_infer_warning,
        "expected NO cannot-infer warning for annotated empty list literal, got: {:?}",
        warnings
    );
}

/// `let s : Set<Length> = set {}` must NOT emit a "cannot infer element type"
/// warning because the annotation resolves the element type.
///
/// RED until step-4 wires the let path to push the annotation as expected_type.
#[test]
fn let_annotated_set_empty_literal_no_cannot_infer_warning() {
    let source = r#"
structure S {
    let s : Set<Length> = set {}
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for `let s : Set<Length> = set {{}}`, got: {:?}",
        errors
    );
    let warnings = warnings_only(&module);
    let has_infer_warning = warnings
        .iter()
        .any(|d| d.message.contains("cannot infer") || d.message.contains("empty set"));
    assert!(
        !has_infer_warning,
        "expected NO cannot-infer warning for annotated empty set literal, got: {:?}",
        warnings
    );
}

/// `let m : Map<String, Length> = map {}` must NOT emit a "cannot infer" warning
/// because the annotation resolves the key/value types.
///
/// RED until step-4 wires the let path to push the annotation as expected_type.
#[test]
fn let_annotated_map_empty_literal_no_cannot_infer_warning() {
    let source = r#"
structure S {
    let m : Map<String, Length> = map {}
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for `let m : Map<String, Length> = map {{}}`, got: {:?}",
        errors
    );
    let warnings = warnings_only(&module);
    let has_infer_warning = warnings
        .iter()
        .any(|d| d.message.contains("cannot infer") || d.message.contains("empty map"));
    assert!(
        !has_infer_warning,
        "expected NO cannot-infer warning for annotated empty map literal, got: {:?}",
        warnings
    );
}

/// `let xss : List<List<Length>> = [[]]` must NOT emit a "cannot infer element
/// type" warning for the inner `[]` because the outer annotation propagates
/// `List<Length>` as the expected element type.
///
/// RED until step-4 wires the let path; the inner `[]` currently warns.
#[test]
fn let_annotated_nested_list_no_cannot_infer_warning() {
    let source = r#"
structure S {
    let xss : List<List<Length>> = [[]]
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "expected no errors for `let xss : List<List<Length>> = [[]]`, got: {:?}",
        errors
    );
    let warnings = warnings_only(&module);
    let has_infer_warning = warnings
        .iter()
        .any(|d| d.message.contains("cannot infer") || d.message.contains("empty list"));
    assert!(
        !has_infer_warning,
        "expected NO cannot-infer warning for annotated nested empty list literal, got: {:?}",
        warnings
    );
}

// ── negative: annotated non-matching-kind literals (#7/#7b) ─────────────────

/// `let a : Length = []` must error with `CollectionLiteralKindMismatch` because
/// the annotation `Length` (a scalar) disagrees with the list literal kind.
///
/// RED until step-4 wires the let path to push the annotation as expected_type.
#[test]
fn let_scalar_annotation_with_list_literal_errors_kind_mismatch() {
    let source = r#"
structure S {
    let a : Length = []
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    let mismatch_err = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::CollectionLiteralKindMismatch));
    assert!(
        mismatch_err.is_some(),
        "expected CollectionLiteralKindMismatch error for `let a : Length = []`, got: {:?}",
        module.diagnostics
    );
    let diag = mismatch_err.unwrap();
    assert!(
        diag.message.contains("cannot satisfy annotation"),
        "error message should contain 'cannot satisfy annotation', got: {:?}",
        diag.message
    );
}

/// `let xs : Set<Length> = [1mm]` must error with `CollectionLiteralKindMismatch`
/// because the annotation `Set<Length>` disagrees with the list literal kind.
///
/// RED until step-4 wires the let path to push the annotation as expected_type.
#[test]
fn let_set_annotation_with_list_literal_errors_kind_mismatch() {
    let source = r#"
structure S {
    let xs : Set<Length> = [1mm]
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    let mismatch_err = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::CollectionLiteralKindMismatch));
    assert!(
        mismatch_err.is_some(),
        "expected CollectionLiteralKindMismatch error for `let xs : Set<Length> = [1mm]`, got: {:?}",
        module.diagnostics
    );
    let diag = mismatch_err.unwrap();
    assert!(
        diag.message.contains("cannot satisfy annotation"),
        "error message should contain 'cannot satisfy annotation', got: {:?}",
        diag.message
    );
}

// ── non-regression: invariant guards (#4/#8) ─────────────────────────────────

/// `let xs = []` (no annotation) must STILL emit a "cannot infer element type"
/// warning AND must NOT emit a CollectionLiteralKindMismatch error.
///
/// This guard must stay green both before and after step-4.
#[test]
fn let_unannotated_empty_list_still_warns_no_kind_mismatch_error() {
    let source = r#"
structure S {
    let xs = []
}
"#;
    let module = compile_source(source);
    // Must still warn — the unannotated path is unchanged.
    let warnings = warnings_only(&module);
    let has_infer_warning = warnings
        .iter()
        .any(|d| d.message.contains("cannot infer") || d.message.contains("empty list"));
    assert!(
        has_infer_warning,
        "unannotated empty list must still emit a cannot-infer warning, got: {:?}",
        warnings
    );
    // Must NOT error with CollectionLiteralKindMismatch.
    let errors = errors_only(&module);
    let mismatch_err = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::CollectionLiteralKindMismatch));
    assert!(
        mismatch_err.is_none(),
        "unannotated empty list must NOT produce CollectionLiteralKindMismatch, got: {:?}",
        errors
    );
}

/// `let xs : List<Length> = [1N]` (matching kind, mismatched element type) must
/// NOT emit a CollectionLiteralKindMismatch error — element-type conformance is
/// out of scope for β (PRD §11).
///
/// This guard must stay green both before and after step-4.
#[test]
fn let_matching_kind_mismatched_element_no_kind_mismatch_error() {
    let source = r#"
structure S {
    let xs : List<Length> = [1N]
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);
    let mismatch_err = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::CollectionLiteralKindMismatch));
    assert!(
        mismatch_err.is_none(),
        "matching-kind let annotation must NOT produce CollectionLiteralKindMismatch (element conformance is β §11 out-of-scope), got: {:?}",
        errors
    );
}
