//! Integration tests for the interactive edit loop through the LSP server.
//!
//! Proves the stateful diagnostics pipeline: open → edit → diagnostics update.

use tower_lsp::lsp_types::{DiagnosticSeverity, Url};

use reify_lsp::diagnostics::{compute_diagnostics_with_state, DiagnosticsResult, EvalState};

fn test_uri() -> Url {
    Url::parse("file:///test.ri").unwrap()
}

#[test]
fn lsp_stateful_diagnostics_initial_eval() {
    let mut state = EvalState::new();
    let source = reify_test_support::bracket_source();

    let result = compute_diagnostics_with_state(&mut state, source, &test_uri());

    // Valid bracket source should produce no error-severity diagnostics
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();
    assert!(
        errors.is_empty(),
        "valid bracket source should have no errors via stateful pipeline, got: {errors:?}"
    );
}

#[test]
fn lsp_stateful_diagnostics_after_edit_detects_violation() {
    let mut state = EvalState::new();

    // First call: valid bracket source (no errors)
    let result1 = compute_diagnostics_with_state(
        &mut state,
        reify_test_support::bracket_source(),
        &test_uri(),
    );
    let errors1: Vec<_> = result1
        .diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();
    assert!(errors1.is_empty(), "initial valid source should have no errors");

    // Second call: violating bracket source (thickness=1mm violates thickness > 2mm)
    let violating_source = reify_test_support::bracket_source_violating();
    let result2 = compute_diagnostics_with_state(&mut state, &violating_source, &test_uri());

    // Should have at least one error-severity diagnostic for the constraint violation
    let errors2: Vec<_> = result2
        .diagnostics
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();
    assert!(
        !errors2.is_empty(),
        "violating source should produce error diagnostics, got: {:?}",
        result2.diagnostics
    );

    // At least one error should mention constraint violation
    let has_violation = errors2.iter().any(|d| {
        d.message.contains("violated") || d.message.contains("constraint")
    });
    assert!(
        has_violation,
        "should have a diagnostic mentioning constraint violation, got: {errors2:?}"
    );
}
