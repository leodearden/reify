//! LSP end-to-end integration tests for decl-level match-block surface (task #3567).
//!
//! Drives `textDocument/didOpen` + `textDocument/hover` through the real
//! [`ReifyLanguageServer`] and asserts the two user-observable leaf signals:
//!
//!   1. `lsp_hover_on_cluster_member_resolves_union` (hover): hovering on a
//!      match-arm cluster member shows `Union<HexHead | SocketHead>` — locks
//!      the `find_match_arm_group_union` fallback path wired by step-4.
//!
//!   2. `lsp_publish_diagnostics_names_offending_arm_for_missing_field`
//!      (diagnostics): opening a doc whose match cluster + arm-specific-field
//!      access is invalid in one arm produces exactly one diagnostic naming
//!      both the field name and the offending arm type — regression lock over
//!      the `compiled.diagnostics → convert` path (no new implementation
//!      required).
//!
//! Harness mirrors `crates/reify-lsp/tests/shadowing_lsp_tests.rs`:
//! `LspService` + `ReifyLanguageServer::with_sink(NoOpSink)`, initialize,
//! `did_open`, then inspect server state or issue a hover request.

use std::sync::Arc;

use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService};

use reify_lsp::server::{NoOpSink, ReifyLanguageServer};

// ── helpers ───────────────────────────────────────────────────────────────────

fn make_service() -> (LspService<ReifyLanguageServer>, tower_lsp::ClientSocket) {
    LspService::new(|client| ReifyLanguageServer::with_sink(client, Arc::new(NoOpSink)))
}

async fn initialize_server(server: &ReifyLanguageServer) {
    server
        .initialize(InitializeParams::default())
        .await
        .unwrap();
    server.initialized(InitializedParams {}).await;
}

async fn open_doc(server: &ReifyLanguageServer, uri: Url, source: &str) {
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: "reify".into(),
                version: 1,
                text: source.into(),
            },
        })
        .await;
}

// ── test 1: hover resolves union ──────────────────────────────────────────────

/// Hovering on a match-arm cluster member resolves the union type.
///
/// Opens a `Bolt` doc with `match head_type { Hex => sub head : HexHead, Socket => sub head : SocketHead }`,
/// issues hover at the `head` token (line 5, col 33), and asserts the hover
/// markdown contains `Union<` plus both arm type names `HexHead` and `SocketHead`.
///
/// Locks the `find_match_arm_group_union` fallback path wired by step-4 of
/// task #3567 (analysis.rs + hover.rs).
#[tokio::test]
async fn lsp_hover_on_cluster_member_resolves_union() {
    let (service, _socket) = make_service();
    let server = service.inner();
    initialize_server(server).await;

    // Source layout (0-indexed lines):
    // Line 0: "enum HeadType { Hex, Socket }"
    // Line 1: "structure HexHead { }"
    // Line 2: "structure SocketHead { }"
    // Line 3: "structure Bolt {"
    // Line 4: "    param head_type : HeadType = HeadType.Hex"
    // Line 5: "    match head_type { Hex => sub head : HexHead, Socket => sub head : SocketHead }"
    // Line 6: "}"
    //
    // On line 5: "    match head_type { Hex => sub head : ..."
    //   cols: 0..3=spaces, 4..8=match, 10..18=head_type, 20={, 22..24=Hex,
    //   26..27=>=, 29..31=sub, 33..36=head
    let source = "\
enum HeadType { Hex, Socket }
structure HexHead { }
structure SocketHead { }
structure Bolt {
    param head_type : HeadType = HeadType.Hex
    match head_type { Hex => sub head : HexHead, Socket => sub head : SocketHead }
}";
    let uri = Url::parse("file:///bolt_hover.ri").unwrap();
    open_doc(server, uri.clone(), source).await;

    // Issue hover at line 5, col 33 — the 'h' of the first 'head' token.
    let hover_result = server
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(5, 33),
            },
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap();

    let hover = hover_result.expect(
        "hover on match-arm cluster member 'head' must return Some \
         (find_match_arm_group_union fallback, task #3567)",
    );

    let md = match hover.contents {
        HoverContents::Markup(MarkupContent { value, .. }) => value,
        other => panic!("expected Markup hover contents, got: {other:?}"),
    };

    assert!(
        md.contains("Union<"),
        "hover markdown must contain 'Union<' for cluster member, got: {md}"
    );
    assert!(
        md.contains("HexHead"),
        "hover markdown must name HexHead arm type, got: {md}"
    );
    assert!(
        md.contains("SocketHead"),
        "hover markdown must name SocketHead arm type, got: {md}"
    );
}

// ── test 2: missing-field diagnostic names offending arm ─────────────────────

/// `publishDiagnostics` carries the missing-field Error naming the offending arm.
///
/// Opens a `Bolt` doc whose `let probe = self.head.recess_depth` access is
/// invalid for the `SocketHead` arm (which lacks `recess_depth`).  Reads
/// `state.last_diagnostics_for(&uri)` and asserts exactly one diagnostic with
/// `source == Some("reify")` whose message names both `recess_depth` and
/// `SocketHead`.
///
/// Regression lock over the `compiled.diagnostics → convert_diagnostic` path
/// (diagnostics.rs:96-98); no new implementation required — the compiler
/// already emits the diagnostic and the conversion path already publishes it.
#[tokio::test]
async fn lsp_publish_diagnostics_names_offending_arm_for_missing_field() {
    let (service, _socket) = make_service();
    let server = service.inner();
    initialize_server(server).await;

    // Source: arm-specific field `recess_depth` present in RecessedHead only.
    // `let probe = self.head.recess_depth` triggers a missing-field Error for SocketHead.
    let source = "\
enum HeadKind { Hex, Socket }
structure RecessedHead {
    param recess_depth : Real = 5
    param across_flats : Real = 10
}
structure SocketHead {
    param across_flats : Real = 8
}
structure Bolt {
    param head_kind : HeadKind = HeadKind.Hex
    match head_kind {
        Hex => sub head : RecessedHead,
        Socket => sub head : SocketHead
    }
    let probe = self.head.recess_depth
}";
    let uri = Url::parse("file:///bolt_diag.ri").unwrap();
    open_doc(server, uri.clone(), source).await;

    // Retrieve captured diagnostics.
    let state = server.state().read().await;
    let diags = state
        .last_diagnostics_for(&uri)
        .expect("did_open must capture publishDiagnostics payload")
        .clone();
    drop(state);

    // Assert exactly one diagnostic naming both 'recess_depth' and 'SocketHead'.
    let field_diags: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.source.as_deref() == Some("reify")
                && d.message.contains("recess_depth")
                && d.message.contains("SocketHead")
        })
        .collect();

    assert_eq!(
        field_diags.len(),
        1,
        "expected exactly one diagnostic naming 'recess_depth' + 'SocketHead' \
         (missing-field Error for SocketHead arm, task #3567 AC2), \
         got {} diag(s): {diags:#?}",
        field_diags.len(),
    );

    // Assert it is an ERROR severity.
    let d = field_diags[0];
    assert_eq!(
        d.severity,
        Some(DiagnosticSeverity::ERROR),
        "missing-field diagnostic must have ERROR severity, got: {:?}",
        d.severity
    );
}
