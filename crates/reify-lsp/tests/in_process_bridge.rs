//! Integration tests for the InProcessLsp bridge.

use reify_lsp::bridge::InProcessLsp;
use serde_json::json;

#[tokio::test]
async fn initialize_returns_server_capabilities() {
    let lsp = InProcessLsp::new();

    let result = lsp
        .handle_request("initialize", json!({}))
        .await
        .expect("initialize should succeed");

    // Should return ServerCapabilities with our providers
    let caps = &result["capabilities"];
    assert!(
        caps["hoverProvider"].as_bool().unwrap_or(false)
            || caps["hoverProvider"].is_object(),
        "should advertise hover provider"
    );
    assert!(
        caps["definitionProvider"].as_bool().unwrap_or(false)
            || caps["definitionProvider"].is_object(),
        "should advertise definition provider"
    );
    assert!(
        caps["completionProvider"].is_object(),
        "should advertise completion provider"
    );
    assert!(
        caps["textDocumentSync"].is_number() || caps["textDocumentSync"].is_object(),
        "should advertise text document sync"
    );
}

#[tokio::test]
async fn did_open_and_completion_returns_items() {
    let lsp = InProcessLsp::new();

    // Initialize first
    lsp.handle_request("initialize", json!({}))
        .await
        .expect("initialize should succeed");
    lsp.handle_request("initialized", json!({}))
        .await
        .expect("initialized should succeed");

    // Open a document with bracket source
    let source = reify_test_support::bracket_source();
    lsp.handle_request(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": "file:///test.ri",
                "languageId": "reify",
                "version": 1,
                "text": source
            }
        }),
    )
    .await
    .expect("didOpen should succeed");

    // Request completions
    let result = lsp
        .handle_request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": "file:///test.ri" },
                "position": { "line": 1, "character": 0 }
            }),
        )
        .await
        .expect("completion should succeed");

    // Should return an array of completion items
    let items = result.as_array().expect("completion should return an array");
    assert!(
        !items.is_empty(),
        "completion should return non-empty items for bracket source"
    );
}

#[tokio::test]
async fn hover_returns_info_for_known_symbol() {
    let lsp = InProcessLsp::new();

    lsp.handle_request("initialize", json!({}))
        .await
        .unwrap();
    lsp.handle_request("initialized", json!({}))
        .await
        .unwrap();

    let source = reify_test_support::bracket_source();
    lsp.handle_request(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": "file:///test.ri",
                "languageId": "reify",
                "version": 1,
                "text": source
            }
        }),
    )
    .await
    .unwrap();

    let result = lsp
        .handle_request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": "file:///test.ri" },
                "position": { "line": 1, "character": 10 }
            }),
        )
        .await
        .expect("hover should succeed");

    // Hover should return non-null info for 'width' parameter
    assert!(
        !result.is_null(),
        "hover should return info for known symbol, got null"
    );
    assert!(
        result["contents"].is_object() || result["contents"].is_string() || result["contents"].is_array(),
        "hover result should have contents"
    );
}

#[tokio::test]
async fn goto_definition_returns_location() {
    let lsp = InProcessLsp::new();

    lsp.handle_request("initialize", json!({}))
        .await
        .unwrap();
    lsp.handle_request("initialized", json!({}))
        .await
        .unwrap();

    let source = reify_test_support::bracket_source();
    lsp.handle_request(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": "file:///test.ri",
                "languageId": "reify",
                "version": 1,
                "text": source
            }
        }),
    )
    .await
    .unwrap();

    // Position on 'thickness' in a constraint line (line 9, col 15)
    let result = lsp
        .handle_request(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": "file:///test.ri" },
                "position": { "line": 9, "character": 15 }
            }),
        )
        .await
        .expect("goto-definition should succeed");

    // Should return a location (or null if not found — but for thickness it should find it)
    assert!(
        !result.is_null(),
        "goto-definition should return a location for 'thickness'"
    );
    assert!(
        result["uri"].is_string() || result["targetUri"].is_string(),
        "goto-definition result should have a URI"
    );
}
