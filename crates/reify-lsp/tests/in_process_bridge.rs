//! Integration tests for the InProcessLsp bridge.

use reify_lsp::bridge::error_prefix;
use reify_lsp::bridge::InProcessLsp;
use reify_test_support::assert_warn_count;
use reify_test_support::warn_counting_guard;
use serde_json::json;

/// Assert that calling `handle_request` with `method` and `json!(42)` (a canonical
/// malformed payload) returns an `Err` whose message contains `fragment`.
///
/// All five "malformed params" tests share this identical assertion triple.
/// The caller is responsible for constructing `lsp` (uninitialized via
/// `InProcessLsp::new()` or fully handshook via `init_lsp()` / `initialized_lsp()`).
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

/// Perform the standard two-step LSP handshake on an existing [`InProcessLsp`] instance.
///
/// 1. `initialize` — the client advertises its capabilities and receives the
///    server's [`InitializeResult`].
/// 2. `initialized` — the client sends a one-way notification confirming it
///    has processed the result.
///
/// Panics if the handshake fails.  Prefer [`initialized_lsp()`] for tests that
/// only need a ready server; use this helper directly when you need to configure
/// the instance before or after initialization.
async fn init_lsp(lsp: &InProcessLsp) {
    lsp.handle_request("initialize", json!({"capabilities": {}}))
        .await
        .expect("init_lsp: initialize should succeed");
    lsp.handle_request("initialized", json!({}))
        .await
        .expect("init_lsp: initialized should succeed");
}

/// Create a fully initialized [`InProcessLsp`] server, ready to receive document
/// requests and notifications.
///
/// Delegates to [`init_lsp()`] for the two-step LSP handshake, then returns the
/// ready server instance.
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
    init_lsp(&lsp).await;
    lsp
}

/// Build a `textDocument/didOpen` params value for `uri` and `text`.
///
/// All four inline `textDocument/didOpen` JSON blocks in this file share the
/// same `{ "textDocument": { "uri", "languageId": "reify", "version": 1, "text" } }`
/// structure; this helper eliminates the repetition while keeping the LSP-specific
/// logic local to this test file (it is not useful outside the bridge interface).
fn did_open_params(uri: &str, text: &str) -> serde_json::Value {
    json!({
        "textDocument": {
            "uri": uri,
            "languageId": "reify",
            "version": 1,
            "text": text
        }
    })
}

/// Open the standard bracket document in `lsp` as `file:///test.ri`.
///
/// Sends a `textDocument/didOpen` notification with [`reify_test_support::bracket_source()`]
/// as the document text.  All tests that need a pre-populated document for the
/// standard bracket fixture should call this helper rather than repeating the
/// 8-line JSON payload inline.
///
/// Panics if the notification fails — callers that exercise error paths should
/// build the `didOpen` payload directly rather than using this helper.
///
/// Mirrors the `open_bracket_source` helper in `server.rs` tests, adapted for
/// the [`InProcessLsp`] bridge interface.
async fn open_bracket_doc(lsp: &InProcessLsp) {
    lsp.handle_request(
        "textDocument/didOpen",
        did_open_params("file:///test.ri", &reify_test_support::bracket_source()),
    )
    .await
    .expect("open_bracket_doc: didOpen should succeed");
}

/// Regression guard: the set_default guard pattern must capture WARN events when
/// running on a current_thread tokio runtime.
///
/// set_default installs a *thread-local* guard; multi-thread runtimes push tasks
/// to other threads that don't have the guard.  current_thread avoids this.
#[tokio::test(flavor = "current_thread")]
async fn set_default_guard_captures_warn_on_current_thread() {
    let (_guard, warn_count) = warn_counting_guard();

    tracing::warn!("test");

    assert_warn_count(
        &warn_count,
        1,
        "set_default guard must capture exactly one WARN event on current_thread runtime",
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

    open_bracket_doc(&lsp).await;

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

    open_bracket_doc(&lsp).await;

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
        did_open_params("file:///test.ri", source),
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

    open_bracket_doc(&lsp).await;

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
        did_open_params(uri, broken_source),
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
///
/// Uses initialize-only setup (no prior `initialized`) so the `initialized`
/// notification in the body is the first and only one, directly testing the
/// first-time Ok(Value::Null) contract.
#[tokio::test]
async fn valid_notification_returns_ok_null() {
    let lsp = InProcessLsp::new();
    lsp.handle_request("initialize", json!({"capabilities": {}}))
        .await
        .expect("initialize should succeed before testing notification");

    let result = lsp.handle_request("initialized", json!({})).await;

    let val = result.expect("valid notification should return Ok");
    assert_eq!(
        val,
        serde_json::Value::Null,
        "valid notification should return exactly Ok(Value::Null)"
    );
}

/// A valid `textDocument/didOpen` notification should return exactly `Ok(Value::Null)`.
///
/// Documents the `Ok(Value::Null)` contract for the didOpen arm of `handle_request`.
/// Uses a fully initialized server to match the realistic call-site where
/// didOpen is sent after the initialize/initialized handshake.
#[tokio::test]
async fn did_open_returns_ok_null() {
    let lsp = initialized_lsp().await;

    let result = lsp
        .handle_request(
            "textDocument/didOpen",
            did_open_params("file:///test.ri", &reify_test_support::bracket_source()),
        )
        .await
        .expect("didOpen should return Ok");

    assert_eq!(
        result,
        serde_json::Value::Null,
        "didOpen should return exactly Ok(Value::Null)"
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

    let val = result.expect("initialized with null params should return Ok");
    assert_eq!(
        val,
        serde_json::Value::Null,
        "initialized with null params should return exactly Ok(Value::Null)"
    );
}

/// Malformed params for `textDocument/didOpen` should return an Err
/// containing "didOpen params error".
///
/// Documents that the didOpen arm performs strict deserialization — bad
/// params are surfaced to the caller rather than silently ignored.
/// Uses a fully initialized server to match the realistic call-site where
/// didOpen is sent after the initialize/initialized handshake.
#[tokio::test]
async fn did_open_with_malformed_params_returns_error() {
    let lsp = initialized_lsp().await;
    assert_malformed_params_returns_error(
        &lsp,
        "textDocument/didOpen",
        error_prefix::DID_OPEN_PARAMS,
    )
    .await;
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

/// A valid `textDocument/didChange` notification should return exactly `Ok(Value::Null)`.
///
/// Documents the `Ok(Value::Null)` contract for the didChange arm of `handle_request`.
/// The document is opened first so the change applies to a live entry, exercising the
/// realistic open → change lifecycle rather than coupling to lenient missing-URI behavior.
#[tokio::test]
async fn did_change_returns_ok_null() {
    let lsp = initialized_lsp().await;

    open_bracket_doc(&lsp).await;

    let result = lsp
        .handle_request(
            "textDocument/didChange",
            json!({
                "textDocument": {
                    "uri": "file:///test.ri",
                    "version": 2
                },
                "contentChanges": [{ "text": reify_test_support::bracket_source() }]
            }),
        )
        .await
        .expect("didChange should return Ok");

    assert_eq!(
        result,
        serde_json::Value::Null,
        "didChange should return exactly Ok(Value::Null)"
    );
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

/// A valid `textDocument/didClose` notification should return exactly `Ok(Value::Null)`.
///
/// Documents the `Ok(Value::Null)` contract for the didClose arm of `handle_request`.
/// The document is opened first so the close applies to a live entry, exercising the
/// realistic open → close lifecycle rather than coupling to lenient missing-URI behavior.
#[tokio::test]
async fn did_close_returns_ok_null() {
    let lsp = initialized_lsp().await;

    open_bracket_doc(&lsp).await;

    let result = lsp
        .handle_request(
            "textDocument/didClose",
            json!({
                "textDocument": {
                    "uri": "file:///test.ri"
                }
            }),
        )
        .await
        .expect("didClose should return Ok");

    assert_eq!(
        result,
        serde_json::Value::Null,
        "didClose should return exactly Ok(Value::Null)"
    );
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

    let val = result.expect("shutdown should return Ok");
    assert_eq!(
        val,
        serde_json::Value::Null,
        "shutdown should return exactly Ok(Value::Null)"
    );
}

/// Calling `shutdown` on a bare [`InProcessLsp`] before the initialize/initialized
/// handshake should not panic and should return `Ok(Value::Null)`.
///
/// The `shutdown` match arm in the bridge calls `server.shutdown().await` without
/// any initialization-state guard, so the pre-handshake path must be safe to call.
/// This test documents that contract and guards against future regressions such as
/// a panic or an unexpected error being introduced on this path.
#[tokio::test]
async fn shutdown_before_initialize() {
    let lsp = InProcessLsp::new();

    let result = lsp.handle_request("shutdown", json!({})).await;

    assert!(
        result.is_ok(),
        "shutdown before initialize should return Ok, got: {:?}",
        result
    );
    assert_eq!(
        result.unwrap(),
        serde_json::Value::Null,
        "shutdown before initialize should return exactly Ok(Value::Null)"
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
