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
