//! End-to-end LSP regression lock for the specialization-scope forbidden-decl squiggle (task 3573).
//!
//! Drives `textDocument/didOpen` via the live `compute_diagnostics_with_state` path and
//! asserts the published diagnostic:
//!
//! - Test A (`lsp_didopen_surfaces_specialization_forbidden_decl_squiggle_on_keyword_name_span`):
//!   a `param x : Length` inside a `sub motor : Base { ... }` body publishes exactly one
//!   `SpecializationForbiddenDecl` Error/source=reify diagnostic whose LSP range covers only
//!   the `param x` line — single-line, NOT the multi-line `{ body }` block (PRD AC 6).
//!
//! - Test B (`lsp_didopen_emits_zero_specialization_forbidden_decl_for_permitted_only_body`):
//!   a body containing only permitted members (`let` + `constraint`) publishes zero
//!   `SpecializationForbiddenDecl` diagnostics (PRD AC 4).
//!
//! Diagnostics are filtered by `code == "SpecializationForbiddenDecl"` so unrelated diagnostics
//! (e.g. unresolved-type noise for `MechanicalPort`, `Length`) do not affect assertions.
//! Mirrors the harness pattern from `crates/reify-lsp/tests/shadowing_lsp_tests.rs`.
//!
//! PRD reference: docs/prds/phase-3-grammar-fiction-triage-log.md §B3.
//! Compiler-side coverage: crates/reify-compiler/tests/specialization_scope_e2e_tests.rs.

use std::sync::Arc;

use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};

use reify_lsp::server::{NoOpSink, ReifyLanguageServer};

/// Test A — PRD AC 6: the `param x : Length` forbidden member inside a
/// `sub motor : Base { ... }` body must publish exactly one `SpecializationForbiddenDecl`
/// diagnostic with:
/// - severity ERROR, source "reify"
/// - range non-degenerate (start != end)
/// - range entirely on the `param x` line (both start.line and end.line equal
///   the line of `param x`) — proving the squiggle is on the keyword+name span,
///   NOT the multi-line `{ body }` block.
///
/// Source layout (0-indexed lines):
/// 0: "structure Base { param size : Length = 10mm }"
/// 1: "structure Assembly {"
/// 2: "    sub motor : Base {"
/// 3: "        param x : Length"
/// 4: "    }"
/// 5: "}"
#[tokio::test]
async fn lsp_didopen_surfaces_specialization_forbidden_decl_squiggle_on_keyword_name_span() {
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

    // --- 3. Open synthetic source with one forbidden `param x` ---
    let uri = Url::parse("file:///spec_forbidden.ri").unwrap();
    let source = "structure Base { param size : Length = 10mm }\nstructure Assembly {\n    sub motor : Base {\n        param x : Length\n    }\n}\n";
    // Line mapping (0-indexed):
    // 0: "structure Base { param size : Length = 10mm }"
    // 1: "structure Assembly {"
    // 2: "    sub motor : Base {"
    // 3: "        param x : Length"
    // 4: "    }"
    // 5: "}"
    let param_x_line: u32 = 3;

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

    // --- 5. Filter for SpecializationForbiddenDecl ---
    let forbidden: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.code == Some(NumberOrString::String(
                "SpecializationForbiddenDecl".to_string(),
            ))
        })
        .collect();

    assert_eq!(
        forbidden.len(),
        1,
        "expected exactly one SpecializationForbiddenDecl diagnostic for `param x`, \
         got {} diag(s): {diags:#?}",
        forbidden.len(),
    );

    // --- 6. Assert diagnostic shape ---
    let d = forbidden[0];

    assert_eq!(
        d.severity,
        Some(DiagnosticSeverity::ERROR),
        "SpecializationForbiddenDecl diagnostic must be ERROR severity"
    );

    assert_eq!(
        d.source.as_deref(),
        Some("reify"),
        "diagnostic source must be 'reify'"
    );

    // Range must be non-degenerate.
    assert_ne!(
        d.range.start, d.range.end,
        "SpecializationForbiddenDecl diagnostic must have a non-degenerate range \
         (keyword+name span, not a zero-width point)"
    );

    // Range must be entirely on the `param x` line — single-line and strictly
    // narrower than the multi-line `{ body }` block (PRD AC 6).
    assert_eq!(
        d.range.start.line, param_x_line,
        "SpecializationForbiddenDecl range.start.line must be on the `param x` line ({param_x_line}), \
         got {} — squiggle must point at the forbidden keyword+name, NOT the body block",
        d.range.start.line
    );
    assert_eq!(
        d.range.end.line, param_x_line,
        "SpecializationForbiddenDecl range.end.line must be on the `param x` line ({param_x_line}), \
         got {} — squiggle must not span multiple lines (the `{{ body }}` block spans lines {}-{})",
        d.range.end.line, param_x_line - 1, param_x_line + 1
    );
}

/// Test B — PRD AC 4: a `sub motor : Base { let m = 1.0  constraint m > 0.0 }` body
/// contains only permitted members; the validator must publish zero
/// `SpecializationForbiddenDecl` diagnostics.
///
/// Source layout:
/// "structure Base { param size : Length = 10mm }\nstructure Assembly {\n    sub motor : Base { let m = 1.0  constraint m > 0.0 }\n}\n"
#[tokio::test]
async fn lsp_didopen_emits_zero_specialization_forbidden_decl_for_permitted_only_body() {
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

    // --- 3. Open synthetic source with permitted-only body ---
    let uri = Url::parse("file:///spec_permitted.ri").unwrap();
    // Only permitted body members: let + constraint (no param/port/sub).
    let source = "structure Base { param size : Length = 10mm }\nstructure Assembly {\n    sub motor : Base { let m = 1.0  constraint m > 0.0 }\n}\n";

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

    // --- 5. Assert zero SpecializationForbiddenDecl diagnostics ---
    let forbidden: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.code == Some(NumberOrString::String(
                "SpecializationForbiddenDecl".to_string(),
            ))
        })
        .collect();

    assert!(
        forbidden.is_empty(),
        "permitted-only body must publish zero SpecializationForbiddenDecl diagnostics \
         (let + constraint are permitted in a specialization-scope body); \
         got {}: {forbidden:#?}",
        forbidden.len()
    );
}
