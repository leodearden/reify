//! Tests for the LspBridge Tauri integration.

use std::sync::Arc;

use serde_json::json;

use crate::lsp_bridge::{LspBridge, lsp_request_impl};
use reify_lsp::test_support::RecordingSink;

#[tokio::test]
async fn lsp_bridge_can_be_constructed_and_initialized() {
    let bridge = LspBridge::new();
    let result = lsp_request_impl(&bridge, "initialize", r#"{"capabilities":{}}"#.to_string())
        .await
        .expect("initialize should succeed");

    // Parse the response — should contain capabilities
    let parsed: serde_json::Value =
        serde_json::from_str(&result).expect("result should be valid JSON");
    assert!(
        parsed["capabilities"].is_object(),
        "should contain capabilities"
    );
}

/// Helper: initialize the bridge and open a document with bracket source.
async fn setup_bridge_with_document(bridge: &LspBridge) {
    lsp_request_impl(bridge, "initialize", r#"{"capabilities":{}}"#.to_string())
        .await
        .expect("initialize");
    lsp_request_impl(bridge, "initialized", "{}".to_string())
        .await
        .expect("initialized");

    let source = reify_test_support::bracket_source();
    let did_open_params = json!({
        "textDocument": {
            "uri": "file:///test.ri",
            "languageId": "reify",
            "version": 1,
            "text": source
        }
    });
    lsp_request_impl(
        bridge,
        "textDocument/didOpen",
        serde_json::to_string(&did_open_params).unwrap(),
    )
    .await
    .expect("didOpen");
}

#[tokio::test]
async fn lsp_request_impl_completion_returns_items() {
    let bridge = LspBridge::new();
    setup_bridge_with_document(&bridge).await;

    let completion_params = json!({
        "textDocument": { "uri": "file:///test.ri" },
        "position": { "line": 1, "character": 0 }
    });
    let result = lsp_request_impl(
        &bridge,
        "textDocument/completion",
        serde_json::to_string(&completion_params).unwrap(),
    )
    .await
    .expect("completion should succeed");

    let parsed: serde_json::Value =
        serde_json::from_str(&result).expect("result should be valid JSON");
    let items = parsed
        .as_array()
        .expect("completion should return an array");
    assert!(
        !items.is_empty(),
        "completion should return non-empty items"
    );
}

#[tokio::test]
async fn lsp_bridge_diagnostics_after_syntax_error() {
    let bridge = LspBridge::new();

    lsp_request_impl(&bridge, "initialize", r#"{"capabilities":{}}"#.to_string())
        .await
        .expect("initialize");
    lsp_request_impl(&bridge, "initialized", "{}".to_string())
        .await
        .expect("initialized");

    // Open a document with a syntax error
    let broken_source = "structure {";
    let uri = "file:///broken.ri";
    let did_open_params = json!({
        "textDocument": {
            "uri": uri,
            "languageId": "reify",
            "version": 1,
            "text": broken_source
        }
    });
    lsp_request_impl(
        &bridge,
        "textDocument/didOpen",
        serde_json::to_string(&did_open_params).unwrap(),
    )
    .await
    .expect("didOpen");

    // Get diagnostics through the bridge (async to properly await the RwLock)
    let diags = bridge.get_diagnostics(uri).await;
    assert!(
        !diags.is_empty(),
        "should have diagnostics for broken source"
    );

    // Verify diagnostics can be serialized to JSON (for Tauri event emission)
    let serialized =
        serde_json::to_string(&diags).expect("diagnostics should be serializable to JSON");
    assert!(
        serialized.len() > 2,
        "serialized diagnostics should be non-trivial"
    );

    // At least one diagnostic should be an error (severity 1)
    let has_error = diags.iter().any(|d| {
        d.get("severity")
            .and_then(|s| s.as_u64())
            .map(|s| s == 1)
            .unwrap_or(false)
    });
    assert!(has_error, "should have at least one error diagnostic");
}

#[tokio::test]
async fn lsp_bridge_with_sink_routes_diagnostics() {
    let sink = Arc::new(RecordingSink::default());
    let bridge = LspBridge::with_sink(sink.clone());

    lsp_request_impl(&bridge, "initialize", r#"{"capabilities":{}}"#.to_string())
        .await
        .expect("initialize");
    lsp_request_impl(&bridge, "initialized", "{}".to_string())
        .await
        .expect("initialized");

    // Use broken source so we get error diagnostics — proves the sink is wired
    let broken_source = "structure {";
    let uri = "file:///sink_test.ri";
    let did_open_params = json!({
        "textDocument": {
            "uri": uri,
            "languageId": "reify",
            "version": 1,
            "text": broken_source
        }
    });
    lsp_request_impl(
        &bridge,
        "textDocument/didOpen",
        serde_json::to_string(&did_open_params).unwrap(),
    )
    .await
    .expect("didOpen should succeed");

    // RecordingSink should have captured at least one publish_diagnostics call
    let calls = sink.take_calls();
    assert!(
        !calls.is_empty(),
        "RecordingSink should have received at least one publish_diagnostics call"
    );

    // Verify the call has the correct URI
    assert_eq!(
        calls[0].0.as_str(),
        uri,
        "sink should receive diagnostics for the correct URI"
    );

    // Verify the diagnostics include an error (broken source)
    let has_error = calls[0]
        .1
        .iter()
        .any(|d| d.severity == Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR));
    assert!(
        has_error,
        "broken source should produce error diagnostics through the sink"
    );
}

#[tokio::test]
async fn lsp_request_impl_rejects_malformed_json_params() {
    // Table-driven: each entry is a string that is not valid JSON.
    // serde_json::from_str rejects all of them, so `lsp_request_impl` must
    // return Err with the "invalid JSON params" prefix (from lsp_bridge.rs).
    for case in ["not json", "", "{", "\"unterminated"] {
        let bridge = LspBridge::new();
        let result = lsp_request_impl(&bridge, "initialize", case.to_string()).await;
        assert!(
            result.is_err(),
            "malformed JSON case {case:?} should return Err"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("invalid JSON params"),
            "case {case:?}: error should contain 'invalid JSON params', got: {err}"
        );
    }
}

#[tokio::test]
async fn lsp_request_impl_accepts_valid_json_null_literal() {
    let bridge = LspBridge::new();
    // "null" is valid JSON — parsing must succeed; the error (if any) must NOT
    // come from the JSON parse step itself.
    let result = lsp_request_impl(&bridge, "initialize", "null".to_string()).await;
    // The LSP initialize handler rejects null params (initialize requires a structured
    // params object), so result must be Err. But the error must not say "invalid JSON
    // params" — that would mean the JSON parse step itself rejected it, which is wrong
    // because null IS valid JSON.
    let err = result.unwrap_err();
    assert!(
        !err.contains("invalid JSON params"),
        "null literal should not trigger a JSON parse error, got: {err}"
    );
}
