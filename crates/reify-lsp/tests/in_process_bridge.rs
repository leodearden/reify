//! Integration tests for the InProcessLsp bridge.

use std::sync::Arc;

use reify_lsp::bridge::InProcessLsp;
use serde_json::json;

/// Build a tracing subscriber that counts WARN-level events.
/// Returns the subscriber and a clone of the counter for assertions.
fn warn_counting_subscriber() -> (
    impl tracing::Subscriber,
    Arc<std::sync::atomic::AtomicUsize>,
) {
    use std::sync::atomic::AtomicUsize;

    let count = Arc::new(AtomicUsize::new(0));
    let count_clone = Arc::clone(&count);

    struct WarnCounter(Arc<AtomicUsize>);

    impl tracing::Subscriber for WarnCounter {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            metadata.level() <= &tracing::Level::WARN
        }

        fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

        fn record_follows_from(
            &self,
            _span: &tracing::span::Id,
            _follows: &tracing::span::Id,
        ) {
        }

        fn event(&self, event: &tracing::Event<'_>) {
            if event.metadata().level() == &tracing::Level::WARN {
                self.0
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }

        fn enter(&self, _span: &tracing::span::Id) {}

        fn exit(&self, _span: &tracing::span::Id) {}
    }

    (WarnCounter(count_clone), count)
}

/// Malformed (non-object) params should not crash the server and should emit a
/// tracing WARN event.
#[tokio::test]
async fn initialize_with_malformed_params_emits_warning() {
    let (subscriber, warn_count) = warn_counting_subscriber();
    // set_default returns a DefaultGuard — compatible with async/await on a
    // single-threaded tokio runtime because all .await points stay on the same
    // thread where the guard is active.
    let _guard = tracing::subscriber::set_default(subscriber);

    let lsp = InProcessLsp::new();

    // json!(42) is clearly malformed for InitializeParams (expects an object)
    let result = lsp.handle_request("initialize", json!(42)).await;

    assert!(result.is_ok(), "server should not crash on malformed params");
    assert!(
        warn_count.load(std::sync::atomic::Ordering::Relaxed) > 0,
        "expected a WARN to be emitted for malformed InitializeParams"
    );
}

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
        caps["hoverProvider"].as_bool().unwrap_or(false) || caps["hoverProvider"].is_object(),
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
    let items = result
        .as_array()
        .expect("completion should return an array");
    assert!(
        !items.is_empty(),
        "completion should return non-empty items for bracket source"
    );
}

#[tokio::test]
async fn hover_returns_info_for_known_symbol() {
    let lsp = InProcessLsp::new();

    lsp.handle_request("initialize", json!({})).await.unwrap();
    lsp.handle_request("initialized", json!({})).await.unwrap();

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
        result["contents"].is_object()
            || result["contents"].is_string()
            || result["contents"].is_array(),
        "hover result should have contents"
    );
}

#[tokio::test]
async fn hover_on_documented_structure_shows_doc_via_bridge() {
    let lsp = InProcessLsp::new();

    lsp.handle_request("initialize", json!({})).await.unwrap();
    lsp.handle_request("initialized", json!({})).await.unwrap();

    let source = "/// A bracket.\nstructure Bracket {\n    param width: Scalar = 80mm\n}";
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
                "position": { "line": 1, "character": 12 }
            }),
        )
        .await
        .expect("hover should succeed");

    // Hover should return non-null info containing doc comment
    assert!(
        !result.is_null(),
        "hover should return info for documented structure, got null"
    );
    let contents = &result["contents"];
    let hover_text = contents["value"]
        .as_str()
        .unwrap_or_else(|| contents.as_str().unwrap_or(""));
    assert!(
        hover_text.contains("A bracket."),
        "hover should contain doc comment 'A bracket.', got: {hover_text}"
    );
}

#[tokio::test]
async fn goto_definition_returns_location() {
    let lsp = InProcessLsp::new();

    lsp.handle_request("initialize", json!({})).await.unwrap();
    lsp.handle_request("initialized", json!({})).await.unwrap();

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

#[tokio::test]
async fn diagnostics_captured_after_did_open_with_syntax_error() {
    let lsp = InProcessLsp::new();

    lsp.handle_request("initialize", json!({})).await.unwrap();
    lsp.handle_request("initialized", json!({})).await.unwrap();

    // Open a document with a syntax error
    let broken_source = "structure {";
    let uri = "file:///broken.ri";
    lsp.handle_request(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "reify",
                "version": 1,
                "text": broken_source
            }
        }),
    )
    .await
    .unwrap();

    // Get diagnostics from the bridge (async to properly await the RwLock)
    let diags = lsp.get_diagnostics(uri).await;
    assert!(
        !diags.is_empty(),
        "should have diagnostics for broken source"
    );

    // Check at least one is an error
    let has_error = diags.iter().any(|d| {
        d.get("severity")
            .and_then(|s| s.as_u64())
            .map(|s| s == 1) // DiagnosticSeverity::ERROR = 1
            .unwrap_or(false)
    });
    assert!(has_error, "should have at least one error diagnostic");
}
