//! Integration tests for Money-vs-Force dimension-mismatch diagnostics.
//!
//! Tests that `25USD + 5N` (and related expressions) produce:
//! - At least one error diagnostic
//! - `code == Some(DiagnosticCode::DimensionMismatch)`
//! - At least one label whose message contains both "Money" and "Force"
//!
//! Tests will fail until `expr.rs` delegates to `format_dimension_mismatch_diagnostic`
//! (step-8 for binary-op, step-10 for range).

use reify_test_support::{compile_source_with_stdlib, errors_only};
use reify_core::DiagnosticCode;

/// Helper: assert exactly one error diagnostic carries `code == DimensionMismatch`.
///
/// Checking `count() == 1` (rather than `any()`) catches a class of regression
/// where the dimension-mismatch diagnostic is emitted twice — e.g. once from the
/// binary-op site and once from a wrapper coercion — which would otherwise pass
/// silently.
fn has_dimension_mismatch_code(errors: &[&reify_core::Diagnostic]) -> bool {
    errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::DimensionMismatch))
        .count()
        == 1
}

/// Helper: check if the labels of any error diagnostic mention both "Money" and "Force".
///
/// Uses `any()` (existence check) rather than `count()` because having the hint
/// appear in more than one label is redundant but not incorrect — the primary and
/// secondary labels may both be informative.
fn has_money_and_force_label(errors: &[&reify_core::Diagnostic]) -> bool {
    errors.iter().any(|d| {
        d.labels
            .iter()
            .any(|l| l.message.contains("Money") && l.message.contains("Force"))
    })
}

/// Helper: assert no error diagnostic mentions "duplicate unit declaration".
///
/// Used to catch regressions where a test source redeclares a unit that is already
/// present in the stdlib prelude (e.g. `pub unit USD : Money` in
/// `crates/reify-compiler/stdlib/units.ri`).
///
/// Substring-based rather than code-based because the duplicate-unit producer in
/// `compile_builder/units_phase.rs` does not currently attach a `DiagnosticCode`.
fn has_no_duplicate_unit_declaration(errors: &[&reify_core::Diagnostic]) -> bool {
    !errors
        .iter()
        .any(|d| d.message.contains("duplicate unit declaration"))
}

/// `25USD + 5N` should produce a DimensionMismatch error with "Money" and "Force" in labels.
#[test]
fn money_plus_force_has_dimension_mismatch_code() {
    // USD comes from crates/reify-compiler/stdlib/units.ri (added in task 2378).
    let source = r#"
structure def S {
    param p : Money = 25USD + 5N
}
"#;
    let module = compile_source_with_stdlib(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for 25USD + 5N, got: {:?}",
        module.diagnostics
    );

    assert!(
        has_dimension_mismatch_code(&errors),
        "expected code == DimensionMismatch for 25USD + 5N, got codes: {:?}",
        errors.iter().map(|d| d.code).collect::<Vec<_>>()
    );

    assert!(
        has_money_and_force_label(&errors),
        "expected a label containing both 'Money' and 'Force' for 25USD + 5N, labels: {:?}",
        errors
            .iter()
            .flat_map(|d| d.labels.iter().map(|l| &l.message))
            .collect::<Vec<_>>()
    );

    assert!(
        has_no_duplicate_unit_declaration(&errors),
        "unexpected 'duplicate unit declaration' error in test source: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // Non-empty span on the first dimension-mismatch label
    let dim_err = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::DimensionMismatch))
        .unwrap();
    assert!(
        !dim_err.labels.is_empty(),
        "expected at least one label on the DimensionMismatch diagnostic"
    );
    assert!(
        !dim_err.labels[0].span.is_empty(),
        "expected non-empty span on the DimensionMismatch diagnostic"
    );
}

/// `25USD - 5N` (subtraction) should produce the same enriched diagnostic.
#[test]
fn money_minus_force_has_dimension_mismatch_code() {
    let source = r#"
structure def S {
    param p : Money = 25USD - 5N
}
"#;
    let module = compile_source_with_stdlib(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for 25USD - 5N"
    );
    assert!(
        has_dimension_mismatch_code(&errors),
        "expected DimensionMismatch code for 25USD - 5N"
    );
    assert!(
        has_money_and_force_label(&errors),
        "expected label with 'Money' and 'Force' for 25USD - 5N"
    );
    assert!(
        has_no_duplicate_unit_declaration(&errors),
        "unexpected 'duplicate unit declaration' error in test source: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// `5N + 25USD` (reverse polarity) should also produce the enriched diagnostic.
#[test]
fn force_plus_money_has_dimension_mismatch_code() {
    let source = r#"
structure def S {
    param p : Force = 5N + 25USD
}
"#;
    let module = compile_source_with_stdlib(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for 5N + 25USD"
    );
    assert!(
        has_dimension_mismatch_code(&errors),
        "expected DimensionMismatch code for 5N + 25USD"
    );
    assert!(
        has_money_and_force_label(&errors),
        "expected label with 'Money' and 'Force' for 5N + 25USD"
    );
    assert!(
        has_no_duplicate_unit_declaration(&errors),
        "unexpected 'duplicate unit declaration' error in test source: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// `25USD..5N` (range) should produce a DimensionMismatch error with "Money" and "Force" in labels.
///
/// This test will fail until the range site in `expr.rs` delegates to
/// `format_dimension_mismatch_diagnostic` (step-10).
#[test]
fn money_range_force_has_dimension_mismatch_code() {
    let source = r#"
structure def S {
    let r = 25USD..5N
}
"#;
    let module = compile_source_with_stdlib(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for 25USD..5N, got: {:?}",
        module.diagnostics
    );

    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("dimension mismatch in range")),
        "expected 'dimension mismatch in range' message for 25USD..5N, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    assert!(
        has_dimension_mismatch_code(&errors),
        "expected code == DimensionMismatch for 25USD..5N, got codes: {:?}",
        errors.iter().map(|d| d.code).collect::<Vec<_>>()
    );

    assert!(
        has_money_and_force_label(&errors),
        "expected a label containing both 'Money' and 'Force' for 25USD..5N, labels: {:?}",
        errors
            .iter()
            .flat_map(|d| d.labels.iter().map(|l| &l.message))
            .collect::<Vec<_>>()
    );

    assert!(
        has_no_duplicate_unit_declaration(&errors),
        "unexpected 'duplicate unit declaration' error in test source: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // Non-empty span
    let dim_err = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::DimensionMismatch))
        .unwrap();
    assert!(
        !dim_err.labels.is_empty(),
        "expected at least one label on the range DimensionMismatch diagnostic"
    );
    assert!(
        !dim_err.labels[0].span.is_empty(),
        "expected non-empty span on the range DimensionMismatch diagnostic"
    );
}
