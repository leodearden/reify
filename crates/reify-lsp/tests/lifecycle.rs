use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};

use reify_lsp::server::ReifyLanguageServer;

#[tokio::test]
async fn full_lifecycle_initialize_open_change_close() {
    // 1. Create service
    let (service, _socket) = LspService::new(ReifyLanguageServer::new);
    let server = service.inner();

    // 2. Initialize
    let init_result = server.initialize(InitializeParams::default()).await.unwrap();
    match init_result.capabilities.text_document_sync {
        Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)) => {}
        other => panic!("Expected FULL sync, got {other:?}"),
    }

    server.initialized(InitializedParams {}).await;

    // 3. Open with valid bracket source
    let uri = Url::parse("file:///bracket.ri").unwrap();
    let source = reify_test_support::bracket_source();

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

    // Verify diagnostics are computable (no panics) by calling compute_diagnostics directly
    let diags = reify_lsp::diagnostics::compute_diagnostics(source, &uri);
    let errors: Vec<_> = diags
        .iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .collect();
    assert!(
        errors.is_empty(),
        "valid bracket source should have no errors, got: {errors:?}"
    );

    // 4. Change to broken source
    let broken = "structure {";
    server
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: broken.to_string(),
            }],
        })
        .await;

    // Verify broken source produces error diagnostics
    let diags = reify_lsp::diagnostics::compute_diagnostics(broken, &uri);
    assert!(
        diags
            .iter()
            .any(|d| d.severity == Some(DiagnosticSeverity::ERROR)),
        "broken source should produce error diagnostics"
    );

    // 5. Close
    server
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        })
        .await;

    // 6. Shutdown
    server.shutdown().await.unwrap();
}
