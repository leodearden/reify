//! End-to-end LSP regression lock for the W_SHADOW (DiagnosticCode::Shadowing) warning.
//!
//! Drives `textDocument/didOpen` for a synthetic source with a lambda-shadows-entity-param
//! case and asserts the published diagnostic carries `code = NumberOrString::String("Shadowing")`,
//! severity Warning, a non-zero range covering the lambda's `|x|` line, and
//! `related_information` whose single entry locates the shadowed `param x` declaration with
//! the literal label `"originally declared here"`.
//!
//! PRD reference: docs/prds/shadowing-warning.md §8.5.
//! Compiler-side coverage: crates/reify-compiler/tests/shadowing_warning_tests.rs.

use std::sync::Arc;

use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};

use reify_lsp::server::{NoOpSink, ReifyLanguageServer};

#[tokio::test]
async fn lsp_publish_diagnostics_surfaces_w_shadow_warning_for_lambda_param_shadow() {
    // --- 1. Create service ---
    let (service, _socket) =
        LspService::new(|client| ReifyLanguageServer::with_sink(client, Arc::new(NoOpSink)));
    let server = service.inner();

    // --- 2. Initialize ---
    server
        .initialize(InitializeParams::default())
        .await
        .unwrap();
    server.initialized(InitializedParams {}).await;

    // --- 3. Open a synthetic source with lambda-shadows-entity-param ---
    // Line 0: "structure S {"
    // Line 1: "    param x : Real = 1"
    // Line 2: "    let f = |x| x * 2"
    // Line 3: "}"
    let uri = Url::parse("file:///shadow.ri").unwrap();
    let source = "structure S {\n    param x : Real = 1\n    let f = |x| x * 2\n}\n";

    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "reify".into(),
                version: 1,
                text: source.into(),
            },
        })
        .await;

    // --- 4. Retrieve captured diagnostics ---
    let state = server.state().read().await;
    let diags = state
        .last_diagnostics_for(&uri)
        .expect("did_open must capture publishDiagnostics payload")
        .clone();
    drop(state);

    // --- 5. Filter for W_SHADOW code ---
    let shadow: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Some(NumberOrString::String("Shadowing".to_string())))
        .collect();
    assert_eq!(
        shadow.len(),
        1,
        "expected exactly one Shadowing diagnostic, got {} diag(s): {diags:#?}",
        shadow.len(),
    );

    // --- 6. Assert diagnostic shape ---
    let d = shadow[0];

    assert_eq!(
        d.severity,
        Some(DiagnosticSeverity::WARNING),
        "Shadowing diagnostic must be a Warning"
    );

    assert_eq!(
        d.source.as_deref(),
        Some("reify"),
        "diagnostic source must be 'reify'"
    );

    // Range must be non-empty and on line 2 (the `let f = |x| x * 2` line).
    assert_ne!(
        d.range.start, d.range.end,
        "Shadowing diagnostic must have a non-empty range"
    );
    assert_eq!(
        d.range.start.line, 2,
        "Shadowing range must start on line 2 (the `let f = |x| x * 2` line)"
    );

    // related_information: exactly one entry pointing to the param x declaration.
    let related = d
        .related_information
        .as_ref()
        .expect("Shadowing diagnostic must carry related_information");
    assert_eq!(
        related.len(),
        1,
        "expected exactly one related_information entry, got {}: {related:#?}",
        related.len()
    );

    let ri = &related[0];
    assert_eq!(
        ri.location.uri, uri,
        "related_information location must reference the same document"
    );
    assert_eq!(
        ri.message, "originally declared here",
        "related_information message must be 'originally declared here'"
    );
    assert_eq!(
        ri.location.range.start.line, 1,
        "related_information must point to line 1 (the `param x` declaration)"
    );
}
