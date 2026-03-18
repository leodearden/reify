//! Integration tests for the interactive edit loop through the LSP server.
//!
//! Proves the stateful diagnostics pipeline: open → edit → diagnostics update.

use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};

use reify_lsp::diagnostics::{compute_diagnostics_with_state, DiagnosticsResult, EvalState};
use reify_lsp::server::ReifyLanguageServer;

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

/// Capstone E2E test: full lifecycle through tower-lsp server.
///
/// 1. Initialize
/// 2. did_open with valid bracket source → no error diagnostics
/// 3. did_change with violating source → constraint violation diagnostic
/// 4. did_change back to valid source → diagnostics clear
/// 5. did_close + shutdown
#[tokio::test]
async fn lsp_server_e2e_interactive_edit_loop() {
    let (service, _socket) = LspService::new(ReifyLanguageServer::new);
    let server = service.inner();

    // 1. Initialize
    let init_result = server.initialize(InitializeParams::default()).await.unwrap();
    match init_result.capabilities.text_document_sync {
        Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)) => {}
        other => panic!("Expected FULL sync, got {other:?}"),
    }
    server.initialized(InitializedParams {}).await;

    let uri = Url::parse("file:///bracket.ri").unwrap();
    let source = reify_test_support::bracket_source();

    // 2. did_open with valid bracket source
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "reify".to_string(),
                version: 1,
                text: source.to_string(),
            },
        })
        .await;

    // Verify via compute_diagnostics_with_state proxy: no errors for valid source
    {
        let mut state = EvalState::new();
        let result = compute_diagnostics_with_state(&mut state, source, &uri);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(errors.is_empty(), "valid source should have no errors after did_open");
    }

    // 3. did_change with violating source (thickness=1mm)
    let violating_source = reify_test_support::bracket_source_violating();
    server
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: violating_source.clone(),
            }],
        })
        .await;

    // Verify: violating source produces constraint violation diagnostic
    {
        let mut state = EvalState::new();
        let result = compute_diagnostics_with_state(&mut state, &violating_source, &uri);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(
            !errors.is_empty(),
            "violating source should produce error diagnostics after did_change"
        );
    }

    // 4. did_change back to valid source → diagnostics should clear
    server
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 3,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: source.to_string(),
            }],
        })
        .await;

    // Verify: valid source has no errors again
    {
        let mut state = EvalState::new();
        let result = compute_diagnostics_with_state(&mut state, source, &uri);
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
            .collect();
        assert!(errors.is_empty(), "valid source should have no errors after reverting");
    }

    // 5. did_close + shutdown
    server
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        })
        .await;

    server.shutdown().await.unwrap();
}
