//! Integration tests for the InProcessLsp bridge.

use std::sync::atomic::Ordering;

use reify_lsp::bridge::error_prefix;
use reify_lsp::bridge::InProcessLsp;
use reify_test_support::warn_counting_subscriber;
use serde_json::json;

/// Assert that calling `handle_request` with `method` and `json!(42)` (a canonical
/// malformed payload) returns an `Err` whose message contains `fragment`.
///
/// All five "malformed params" tests share this identical assertion triple.
/// The caller is responsible for constructing `lsp` (uninitialized via
/// `InProcessLsp::new()` or fully handshook via `initialized_lsp()`).
async fn assert_malformed_params_returns_error(lsp: &InProcessLsp, method: &str, fragment: &str) {
    let result = lsp.handle_request(method, json!(42)).await;
    assert!(
        result.is_err(),
        "{method} with malformed params should return Err, got: {result:?}"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains(fragment),
        "error message should contain '{fragment}', got: {err}"
    );
}

/// Create a fully initialized [`InProcessLsp`] server, ready to receive document
/// requests and notifications.
///
/// Performs the standard two-step LSP handshake:
///
/// 1. `initialize` — the client advertises its capabilities and receives the
///    server's [`InitializeResult`].
/// 2. `initialized` — the client sends a one-way notification confirming it
///    has processed the result.  `initialized` is a **notification**, not a
///    request: the LSP spec does not define a response payload for it.
///    `handle_request` therefore returns `Ok(Value::Null)` — the sentinel for
///    a successfully processed notification — rather than a meaningful JSON
///    object.  The `.expect()` on that call documents the protocol contract
///    (the notification must not fail), not a guard against a server-level
///    error.
///
/// Panics if the handshake fails — all tests that need a ready server should
/// use this helper rather than repeating the setup inline.
///
/// **Note for error-path tests**: tests that exercise param deserialization
/// failures (e.g. `initialize_with_malformed_params_returns_error`) should
/// call [`InProcessLsp::new()`] directly to avoid paying the two-round-trip
/// handshake overhead when the server state after handshake is irrelevant.
async fn initialized_lsp() -> InProcessLsp {
    let lsp = InProcessLsp::new();
    lsp.handle_request("initialize", json!({"capabilities": {}}))
        .await
        .expect("initialized_lsp: initialize should succeed");
    lsp.handle_request("initialized", json!({}))
        .await
        .expect("initialized_lsp: initialized should succeed");
    lsp
}

/// Regression guard: the set_default guard pattern must capture WARN events when
/// running on a current_thread tokio runtime.
///
/// set_default installs a *thread-local* guard; multi-thread runtimes push tasks
/// to other threads that don't have the guard.  current_thread avoids this.
#[tokio::test(flavor = "current_thread")]
async fn set_default_guard_captures_warn_on_current_thread() {
    let (subscriber, warn_count) = warn_counting_subscriber();
    let _guard = tracing::subscriber::set_default(subscriber);

    tracing::warn!("test");

    assert_eq!(
        warn_count.load(Ordering::Relaxed),
        1,
        "set_default guard must capture exactly one WARN event on current_thread runtime"
    );
}

#[tokio::test]
async fn initialize_returns_server_capabilities() {
    let lsp = InProcessLsp::new();

    let result = lsp
        .handle_request("initialize", json!({"capabilities": {}}))
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
    let lsp = initialized_lsp().await;

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
    let lsp = initialized_lsp().await;

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
    let lsp = initialized_lsp().await;

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
    let lsp = initialized_lsp().await;

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
    let lsp = initialized_lsp().await;

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

/// Malformed (non-object) params should return an Err containing
/// [`error_prefix::INITIALIZE_PARAMS`].
#[tokio::test]
async fn initialize_with_malformed_params_returns_error() {
    let lsp = InProcessLsp::new();
    assert_malformed_params_returns_error(&lsp, "initialize", error_prefix::INITIALIZE_PARAMS)
        .await;
}

/// Notifications with malformed params should propagate deserialization errors as Err,
/// not silently succeed with Ok(Value::Null).
///
/// This documents that the notification arms of `handle_request` are not "fire and forget"
/// — deserialization failures are surfaced to the caller even for one-way messages.
#[tokio::test]
async fn notification_with_malformed_params_returns_error() {
    let lsp = InProcessLsp::new();
    assert_malformed_params_returns_error(
        &lsp,
        "textDocument/didOpen",
        error_prefix::DID_OPEN_PARAMS,
    )
    .await;
}

/// An unknown/unsupported method name should return Err, not panic or silently succeed.
///
/// This documents the `other => Err(...)` arm of `handle_request`'s match expression.
#[tokio::test]
async fn unsupported_method_returns_error() {
    let lsp = InProcessLsp::new();

    let result = lsp.handle_request("textDocument/foobar", json!({})).await;

    assert!(
        result.is_err(),
        "unsupported method should return Err, got: {:?}",
        result
    );
    let err = result.unwrap_err();
    // The constant covers the prefix; the method name appears as a suffix after the colon.
    assert!(
        err.contains(error_prefix::UNSUPPORTED_METHOD),
        "error message should contain '{}', got: {err}",
        error_prefix::UNSUPPORTED_METHOD
    );
}

/// Wrong field type within a valid-looking object should return Err containing
/// "initialize params error".
#[tokio::test]
async fn initialize_with_invalid_field_type_returns_error() {
    let lsp = InProcessLsp::new();

    // processId is an Option<u32> in InitializeParams — passing a string makes
    // deserialization fail, exercising the error-propagation path.
    let result = lsp
        .handle_request("initialize", json!({ "processId": "not_a_number" }))
        .await;

    assert!(
        result.is_err(),
        "server should return Err on invalid field type"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains(error_prefix::INITIALIZE_PARAMS),
        "error message should contain '{}', got: {err}",
        error_prefix::INITIALIZE_PARAMS
    );
}

/// A valid notification should return exactly `Ok(Value::Null)`, not an error and not
/// any JSON payload.
///
/// This documents the `Ok(Value::Null)` contract for successfully processed
/// one-way LSP messages (initialized, didOpen, didChange, didClose).
#[tokio::test]
async fn valid_notification_returns_ok_null() {
    let lsp = initialized_lsp().await;

    // Sending `initialized` again is valid — the server accepts multiple
    // notifications and returns Ok(Value::Null) each time.
    let result = lsp.handle_request("initialized", json!({})).await;

    assert!(
        result.is_ok(),
        "valid notification should return Ok, got: {:?}",
        result
    );
    assert_eq!(
        result.unwrap(),
        serde_json::Value::Null,
        "valid notification should return exactly Ok(Value::Null)"
    );
}

/// Malformed (non-object) params for `initialized` should return an Err
/// containing "initialized params error", not silently succeed.
///
/// This documents that the `initialized` arm follows the same strict
/// deserialization contract as all other notification arms — bad params are
/// surfaced to the caller rather than silently ignored.
#[tokio::test]
async fn initialized_with_malformed_params_returns_error() {
    let lsp = InProcessLsp::new();
    assert_malformed_params_returns_error(&lsp, "initialized", error_prefix::INITIALIZED_PARAMS)
        .await;
}

/// LSP clients (e.g. some VS Code extensions, Neovim) may send `params: null` for
/// notifications with no meaningful payload. `InitializedParams` is an empty interface
/// (`{}` in the LSP spec), so `null` should be treated as equivalent to `{}`.
///
/// This test documents that `initialized` with `Value::Null` returns `Ok(Value::Null)`
/// rather than a deserialization error.
#[tokio::test]
async fn initialized_with_null_params_returns_ok() {
    let lsp = initialized_lsp().await;

    // Value::Null simulates a JSON-RPC client that omits params or sends null
    // for a notification with no meaningful payload.
    let result = lsp
        .handle_request("initialized", serde_json::Value::Null)
        .await;

    assert!(
        result.is_ok(),
        "initialized with null params should return Ok, got: {:?}",
        result
    );
    assert_eq!(
        result.unwrap(),
        serde_json::Value::Null,
        "initialized with null params should return exactly Ok(Value::Null)"
    );
}

/// Malformed params for `textDocument/didChange` should return an Err
/// containing "didChange params error".
///
/// Documents that the didChange arm performs strict deserialization — bad
/// params are surfaced to the caller rather than silently ignored.
#[tokio::test]
async fn did_change_with_malformed_params_returns_error() {
    let lsp = initialized_lsp().await;
    assert_malformed_params_returns_error(
        &lsp,
        "textDocument/didChange",
        error_prefix::DID_CHANGE_PARAMS,
    )
    .await;
}

/// Malformed params for `textDocument/didClose` should return an Err
/// containing "didClose params error".
///
/// Documents that the didClose arm performs strict deserialization — bad
/// params are surfaced to the caller rather than silently ignored.
#[tokio::test]
async fn did_close_with_malformed_params_returns_error() {
    let lsp = initialized_lsp().await;
    assert_malformed_params_returns_error(
        &lsp,
        "textDocument/didClose",
        error_prefix::DID_CLOSE_PARAMS,
    )
    .await;
}

/// The `shutdown` request should return exactly `Ok(Value::Null)`.
///
/// This documents that the shutdown arm follows the same `Ok(Value::Null)` contract
/// as successfully-processed notifications, and provides coverage for an arm that
/// was previously untested.
#[tokio::test]
async fn shutdown_returns_ok_null() {
    let lsp = initialized_lsp().await;

    let result = lsp.handle_request("shutdown", json!({})).await;

    assert!(
        result.is_ok(),
        "shutdown should return Ok, got: {:?}",
        result
    );
    assert_eq!(
        result.unwrap(),
        serde_json::Value::Null,
        "shutdown should return exactly Ok(Value::Null)"
    );
}

/// Each `error_prefix` constant must actually appear in the error message
/// returned when the corresponding method receives malformed params.
///
/// This test serves as a compile-time anchor: if a format-string prefix in
/// bridge.rs is renamed, the constant definition must be updated too —
/// otherwise this test will fail at runtime, catching the drift immediately.
#[tokio::test]
async fn error_prefix_constants_match_actual_errors() {
    let lsp = InProcessLsp::new();

    // Verify each constant is contained in the error for its matching method.
    assert_malformed_params_returns_error(&lsp, "initialize", error_prefix::INITIALIZE_PARAMS)
        .await;
    assert_malformed_params_returns_error(&lsp, "initialized", error_prefix::INITIALIZED_PARAMS)
        .await;
    assert_malformed_params_returns_error(
        &lsp,
        "textDocument/didOpen",
        error_prefix::DID_OPEN_PARAMS,
    )
    .await;
    assert_malformed_params_returns_error(
        &lsp,
        "textDocument/didChange",
        error_prefix::DID_CHANGE_PARAMS,
    )
    .await;
    assert_malformed_params_returns_error(
        &lsp,
        "textDocument/didClose",
        error_prefix::DID_CLOSE_PARAMS,
    )
    .await;

    // The unsupported-method constant covers the prefix portion of the error.
    let result = lsp.handle_request("textDocument/foobar", json!({})).await;
    assert!(result.is_err(), "unsupported method should return Err");
    let err = result.unwrap_err();
    assert!(
        err.contains(error_prefix::UNSUPPORTED_METHOD),
        "error should contain '{}', got: {err}",
        error_prefix::UNSUPPORTED_METHOD
    );
}
