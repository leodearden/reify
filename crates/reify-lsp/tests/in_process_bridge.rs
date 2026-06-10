//! Integration tests for the InProcessLsp bridge.

use reify_lsp::bridge::InProcessLsp;
use reify_lsp::bridge::error_prefix;
use reify_test_support::assert_warn_count;
use reify_test_support::warn_counting_guard;
use serde_json::json;
use std::time::Duration;

/// Assert that calling `handle_request` with `method` and `json!(42)` (a canonical
/// malformed payload) returns an `Err` whose message contains `fragment`.
///
/// All malformed-params tests share this identical assertion triple.
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

/// Assert that calling `handle_request("shutdown", params)` returns exactly `Ok(Value::Null)`.
///
/// The `shutdown` bridge arm ignores params entirely, so every `params` value must
/// produce the same result. The failure messages embed `params` so that a regression
/// in the helper is immediately attributable to the specific params value that broke.
///
/// The caller is responsible for constructing `lsp` (either `InProcessLsp::new()` for
/// pre-handshake tests or `initialized_lsp().await` for post-handshake tests).
async fn assert_shutdown_returns_null(lsp: &InProcessLsp, params: &serde_json::Value) {
    let result = lsp.handle_request("shutdown", params.clone()).await;
    let val = result
        .unwrap_or_else(|e| panic!("shutdown(params={params}) should return Ok, got Err: {e}"));
    assert_eq!(
        val,
        serde_json::Value::Null,
        "shutdown(params={params}) should return exactly Ok(Value::Null)"
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
    lsp.handle_request("initialize", reify_test_support::minimal_init_params())
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
        did_open_params("file:///test.ri", reify_test_support::bracket_source()),
    )
    .await
    .expect("open_bracket_doc: didOpen should succeed");
}

/// Extract the completion items slice from a serialized [`CompletionResponse`] value.
///
/// The bridge serializes `Option<CompletionResponse>` to `serde_json::Value`, producing
/// one of two shapes depending on which variant the server returned:
///
/// - `CompletionResponse::Array` → JSON array `[...]`
/// - `CompletionResponse::List`  → JSON object `{"isIncomplete": bool, "items": [...]}`
///
/// This helper handles both shapes so that tests remain valid regardless of which
/// variant the server returns.  Panics with an actionable message (including the actual
/// value received) if neither shape matches.
fn completion_items(response: &serde_json::Value) -> &[serde_json::Value] {
    if let Some(arr) = response.as_array() {
        return arr;
    }
    if let Some(obj) = response.as_object()
        && let Some(items_val) = obj.get("items")
    {
        if let Some(arr) = items_val.as_array() {
            return arr;
        }
        panic!("CompletionResponse::List has non-array 'items' field: {response}");
    }
    panic!(
        "completion response should be CompletionResponse::Array (JSON array) or \
         CompletionResponse::List (JSON object with \"items\" field), got: {response}"
    );
}

/// Default hang-guard timeout in seconds, applied to all async bridge tests via the `hang_guard!` macro.
///
/// Five seconds is long enough to be unreachable for any fast local I/O and short
/// enough that a genuine hang surfaces quickly.  All call sites reference this
/// constant so that a single change here adjusts the timeout project-wide.
const HANG_GUARD_SECS: u64 = 5;

/// Ergonomic wrapper around [`with_hang_guard`] that supplies the default timeout.
///
/// `hang_guard!(future)` expands to `with_hang_guard(HANG_GUARD_SECS, future).await`.
/// `hang_guard!(secs, future)` expands to `with_hang_guard(secs, future).await`.
///
/// The two-arg form exists for tests that need a custom timeout; most tests should
/// use the single-arg form, which references the project-wide [`HANG_GUARD_SECS`]
/// constant so that all default-timeout sites can be tuned in one place.
macro_rules! hang_guard {
    ($future:expr) => {
        with_hang_guard(HANG_GUARD_SECS, $future).await
    };
    ($secs:expr, $future:expr) => {
        with_hang_guard($secs, $future).await
    };
}

/// Wrap `f` in a timeout and panic with a descriptive message if the timeout fires.
///
/// This helper guards all async bridge tests against infinite hangs: a future
/// that blocks forever (e.g. waiting on a channel that never receives) will be
/// detected within `seconds` seconds and surface as a panic rather than causing
/// the test suite to hang silently.
///
/// Most tests use this via the [`hang_guard!`] macro, which supplies the default
/// [`HANG_GUARD_SECS`] timeout and includes `.await`. Call this function directly
/// only when a custom timeout is needed (e.g. `panics_on_timeout`).
///
/// The `#[track_caller]` attribute auto-derives the caller's file:line location,
/// which appears in the panic message to make hangs immediately attributable to
/// the specific test that timed out.
///
/// # Panics
///
/// Panics with `"{caller} must not hang (timed out after {seconds}s)"` if
/// `f` does not complete within `seconds` seconds, where `{caller}` is the
/// auto-derived file:line of the call site.
async fn with_hang_guard<F: std::future::Future<Output = ()>>(seconds: u64, f: F) {
    let caller = std::panic::Location::caller();
    tokio::time::timeout(Duration::from_secs(seconds), f)
        .await
        .unwrap_or_else(|_| panic!("{caller} must not hang (timed out after {seconds}s)"));
}

/// Assert that `result` is either `Ok(val)` or a well-defined non-empty `Err`.
///
/// Returns `Some(val)` when `result` is `Ok(val)` — the caller may inspect `val`
/// further.  Returns `None` when `result` is `Err(e)` and `!e.is_empty()` — the
/// error is well-formed; nothing more to check.
///
/// # Panics
///
/// Panics with a message containing `"non-empty"` and `ctx` when `result` is
/// `Err(e)` and `e.is_empty()` — an empty error string is a bug (it provides no
/// actionable information to the caller).
///
/// This helper is used by `downstream_ops_after_malformed_initialize_without_initialized`
/// to honour the dual-outcome spec documented in that test's doc comment: downstream
/// calls may either succeed (outcome a) or return a non-empty Err (outcome b).
fn assert_ok_or_nonempty_err(
    result: Result<serde_json::Value, String>,
    ctx: &str,
) -> Option<serde_json::Value> {
    match result {
        Ok(val) => Some(val),
        Err(e) if !e.is_empty() => None,
        Err(e) => panic!(
            "assert_ok_or_nonempty_err({ctx}): expected Ok or non-empty Err, got empty Err: {e:?}"
        ),
    }
}

/// Regression guard: the set_default guard pattern must capture WARN events when
/// running on a current_thread tokio runtime.
///
/// set_default installs a *thread-local* guard; multi-thread runtimes push tasks
/// to other threads that don't have the guard.  current_thread avoids this.
#[tokio::test(flavor = "current_thread")]
async fn set_default_guard_captures_warn_on_current_thread() {
    // Inoculate against tracing's per-callsite Interest cache — see
    // `prime_tracing_callsite_cache` in reify-test-support for why.
    reify_test_support::prime_tracing_callsite_cache();
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
    hang_guard!(async {
        let lsp = InProcessLsp::new();

        let result = lsp
            .handle_request("initialize", reify_test_support::minimal_init_params())
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
    });
}

#[tokio::test]
async fn did_open_and_completion_returns_items() {
    hang_guard!(async {
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

        // Should return completion items in either CompletionResponse variant.
        let items = completion_items(&result);
        assert!(
            !items.is_empty(),
            "completion should return non-empty items for bracket source"
        );
    });
}

#[tokio::test]
async fn hover_returns_info_for_known_symbol() {
    hang_guard!(async {
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
    });
}

#[tokio::test]
async fn hover_on_documented_structure_shows_doc_via_bridge() {
    hang_guard!(async {
        let lsp = initialized_lsp().await;

        let source = "/// A bracket.\nstructure Bracket {\n    param width: Length = 80mm\n}";
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
    });
}

#[tokio::test]
async fn goto_definition_returns_location() {
    hang_guard!(async {
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
    });
}

#[tokio::test]
async fn diagnostics_captured_after_did_open_with_syntax_error() {
    hang_guard!(async {
        let lsp = initialized_lsp().await;

        // Open a document with a syntax error
        let broken_source = "structure {";
        let uri = "file:///broken.ri";
        lsp.handle_request("textDocument/didOpen", did_open_params(uri, broken_source))
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
    });
}

/// Malformed (non-object) params should return an Err containing
/// [`error_prefix::INITIALIZE_PARAMS`].
#[tokio::test]
async fn initialize_with_malformed_params_returns_error() {
    hang_guard!(async {
        let lsp = InProcessLsp::new();
        assert_malformed_params_returns_error(&lsp, "initialize", error_prefix::INITIALIZE_PARAMS)
            .await;
    });
}

/// Notifications with malformed params should propagate deserialization errors as Err,
/// not silently succeed with Ok(Value::Null).
///
/// This documents that the notification arms of `handle_request` are not "fire and forget"
/// — deserialization failures are surfaced to the caller even for one-way messages.
#[tokio::test]
async fn notification_with_malformed_params_returns_error() {
    hang_guard!(async {
        let lsp = InProcessLsp::new();
        assert_malformed_params_returns_error(
            &lsp,
            "textDocument/didOpen",
            error_prefix::DID_OPEN_PARAMS,
        )
        .await;
    });
}

/// An unknown/unsupported method name should return Err, not panic or silently succeed.
///
/// This documents the `other => Err(...)` arm of `handle_request`'s match expression.
#[tokio::test]
async fn unsupported_method_returns_error() {
    hang_guard!(async {
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
    });
}

/// Wrong field type within a valid-looking object should return Err containing
/// "initialize params error".
#[tokio::test]
async fn initialize_with_invalid_field_type_returns_error() {
    hang_guard!(async {
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
    });
}

/// Missing required `capabilities` field should return Err containing
/// [`error_prefix::INITIALIZE_PARAMS`].
///
/// `json!({})` is a valid JSON object, but `InitializeParams` requires a `capabilities`
/// field. This tests that strict serde deserialization surfaces the missing-field error
/// rather than silently substituting a default.
#[tokio::test]
async fn initialize_with_missing_capabilities_returns_error() {
    hang_guard!(async {
        let lsp = InProcessLsp::new();

        let result = lsp.handle_request("initialize", json!({})).await;

        assert!(
            result.is_err(),
            "server should return Err when capabilities field is missing"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains(error_prefix::INITIALIZE_PARAMS),
            "error message should contain '{}', got: {err}",
            error_prefix::INITIALIZE_PARAMS
        );
    });
}

/// A failed initialize call should not corrupt server state — a subsequent valid
/// initialize call on the same instance should succeed.
///
/// Uses `json!(42)` as the failing payload (wrong type entirely) to verify that
/// deserialization failure in the initialize arm leaves the server in a recoverable
/// pre-initialized state.
#[tokio::test]
async fn initialize_error_does_not_corrupt_server_state() {
    hang_guard!(async {
        let lsp = InProcessLsp::new();

        // First call: should fail.
        let err_result = lsp.handle_request("initialize", json!(42)).await;
        assert!(
            err_result.is_err(),
            "initialize with json!(42) should return Err, got: {:?}",
            err_result
        );

        // Second call on the same instance: should succeed and return a well-formed response.
        let ok_result = lsp
            .handle_request("initialize", reify_test_support::minimal_init_params())
            .await;
        let val = ok_result.expect("initialize after a failed attempt should succeed");
        assert!(
            val.get("capabilities").is_some(),
            "recovery response should contain capabilities"
        );
    });
}

/// The `initialized` notification should return exactly `Ok(Value::Null)`.
///
/// Documents the `Ok(Value::Null)` contract for the `initialized` arm of
/// `handle_request`. Uses initialize-only setup (no prior `initialized`) so
/// the `initialized` notification in the body is the first and only one,
/// directly testing the first-time Ok(Value::Null) contract. The paired
/// `did_open_returns_ok_null` test below covers the `textDocument/didOpen`
/// arm separately.
#[tokio::test]
async fn initialized_returns_ok_null() {
    hang_guard!(async {
        let lsp = InProcessLsp::new();
        lsp.handle_request("initialize", reify_test_support::minimal_init_params())
            .await
            .expect("initialize should succeed before testing initialized");

        let result = lsp.handle_request("initialized", json!({})).await;

        let val = result.expect("initialized should return Ok");
        assert_eq!(
            val,
            serde_json::Value::Null,
            "initialized should return exactly Ok(Value::Null)"
        );
    });
}

/// A valid `textDocument/didOpen` notification should return exactly `Ok(Value::Null)`.
///
/// Documents the `Ok(Value::Null)` contract for the didOpen arm of `handle_request`.
/// Uses a fully initialized server to match the realistic call-site where
/// didOpen is sent after the initialize/initialized handshake.
#[tokio::test]
async fn did_open_returns_ok_null() {
    hang_guard!(async {
        let lsp = initialized_lsp().await;

        let result = lsp
            .handle_request(
                "textDocument/didOpen",
                did_open_params("file:///test.ri", reify_test_support::bracket_source()),
            )
            .await
            .expect("didOpen should return Ok");

        assert_eq!(
            result,
            serde_json::Value::Null,
            "didOpen should return exactly Ok(Value::Null)"
        );
    });
}

/// Malformed (non-object) params for `initialized` should return an Err
/// containing "initialized params error", not silently succeed.
///
/// This documents that the `initialized` arm follows the same strict
/// deserialization contract as all other notification arms — bad params are
/// surfaced to the caller rather than silently ignored.
#[tokio::test]
async fn initialized_with_malformed_params_returns_error() {
    hang_guard!(async {
        let lsp = InProcessLsp::new();
        assert_malformed_params_returns_error(
            &lsp,
            "initialized",
            error_prefix::INITIALIZED_PARAMS,
        )
        .await;
    });
}

/// LSP clients (e.g. some VS Code extensions, Neovim) may send `params: null` for
/// notifications with no meaningful payload. `InitializedParams` is an empty interface
/// (`{}` in the LSP spec), so `null` should be treated as equivalent to `{}`.
///
/// This test documents that `initialized` with `Value::Null` returns `Ok(Value::Null)`
/// rather than a deserialization error.
#[tokio::test]
async fn initialized_with_null_params_returns_ok() {
    hang_guard!(async {
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
    });
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
    hang_guard!(async {
        let lsp = initialized_lsp().await;
        assert_malformed_params_returns_error(
            &lsp,
            "textDocument/didOpen",
            error_prefix::DID_OPEN_PARAMS,
        )
        .await;
    });
}

/// Malformed params for `textDocument/didChange` should return an Err
/// containing "didChange params error".
///
/// Documents that the didChange arm performs strict deserialization — bad
/// params are surfaced to the caller rather than silently ignored.
#[tokio::test]
async fn did_change_with_malformed_params_returns_error() {
    hang_guard!(async {
        let lsp = initialized_lsp().await;
        assert_malformed_params_returns_error(
            &lsp,
            "textDocument/didChange",
            error_prefix::DID_CHANGE_PARAMS,
        )
        .await;
    });
}

/// A valid `textDocument/didChange` notification should return exactly `Ok(Value::Null)`.
///
/// Documents the `Ok(Value::Null)` contract for the didChange arm of `handle_request`.
/// The document is opened first so the change applies to a live entry, exercising the
/// realistic open → change lifecycle rather than coupling to lenient missing-URI behavior.
#[tokio::test]
async fn did_change_returns_ok_null() {
    hang_guard!(async {
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
    });
}

/// Malformed params for `textDocument/didClose` should return an Err
/// containing "didClose params error".
///
/// Documents that the didClose arm performs strict deserialization — bad
/// params are surfaced to the caller rather than silently ignored.
#[tokio::test]
async fn did_close_with_malformed_params_returns_error() {
    hang_guard!(async {
        let lsp = initialized_lsp().await;
        assert_malformed_params_returns_error(
            &lsp,
            "textDocument/didClose",
            error_prefix::DID_CLOSE_PARAMS,
        )
        .await;
    });
}

/// A valid `textDocument/didClose` notification should return exactly `Ok(Value::Null)`.
///
/// Documents the `Ok(Value::Null)` contract for the didClose arm of `handle_request`.
/// The document is opened first so the close applies to a live entry, exercising the
/// realistic open → close lifecycle rather than coupling to lenient missing-URI behavior.
#[tokio::test]
async fn did_close_returns_ok_null() {
    hang_guard!(async {
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
    });
}

/// Malformed params for `textDocument/completion` should return an Err
/// containing "completion params error".
///
/// Documents that the completion arm performs strict deserialization — bad
/// params are surfaced to the caller rather than silently ignored.
/// Uses a fully initialized server to match the realistic call-site where
/// completion is sent after the initialize/initialized handshake.
#[tokio::test]
async fn completion_with_malformed_params_returns_error() {
    hang_guard!(async {
        let lsp = initialized_lsp().await;
        assert_malformed_params_returns_error(
            &lsp,
            "textDocument/completion",
            error_prefix::COMPLETION_PARAMS,
        )
        .await;
    });
}

/// Malformed params for `textDocument/hover` should return an Err
/// containing "hover params error".
///
/// Documents that the hover arm performs strict deserialization — bad
/// params are surfaced to the caller rather than silently ignored.
#[tokio::test]
async fn hover_with_malformed_params_returns_error() {
    hang_guard!(async {
        let lsp = initialized_lsp().await;
        assert_malformed_params_returns_error(
            &lsp,
            "textDocument/hover",
            error_prefix::HOVER_PARAMS,
        )
        .await;
    });
}

/// Malformed params for `textDocument/definition` should return an Err
/// containing "definition params error".
///
/// Documents that the definition arm performs strict deserialization — bad
/// params are surfaced to the caller rather than silently ignored.
#[tokio::test]
async fn definition_with_malformed_params_returns_error() {
    hang_guard!(async {
        let lsp = initialized_lsp().await;
        assert_malformed_params_returns_error(
            &lsp,
            "textDocument/definition",
            error_prefix::DEFINITION_PARAMS,
        )
        .await;
    });
}

/// The `shutdown` request should return exactly `Ok(Value::Null)`.
///
/// This documents that the shutdown arm follows the same `Ok(Value::Null)` contract
/// as successfully-processed notifications, and provides coverage for an arm that
/// was previously untested.
#[tokio::test]
async fn shutdown_returns_ok_null() {
    hang_guard!(async {
        let lsp = initialized_lsp().await;
        assert_shutdown_returns_null(&lsp, &json!({})).await;
    });
}

/// The `shutdown` request with `null` params should return exactly `Ok(Value::Null)`.
///
/// Per the LSP spec, `shutdown` is a parameterless request — `null` (or omitted params)
/// is the canonical JSON-RPC representation. This test exercises that spec-correct path
/// and guards against the `json!({})` variant above failing "for the wrong reason" if a
/// future change adds strict param validation to the shutdown arm (e.g. rejecting `{}`
/// while accepting `null`).
///
/// The naming follows the `initialized_with_null_params_returns_ok` pattern already
/// established for the analogous null-params case in the `initialized` arm.
#[tokio::test]
async fn shutdown_with_null_params_returns_ok_null() {
    hang_guard!(async {
        let lsp = initialized_lsp().await;
        assert_shutdown_returns_null(&lsp, &json!(null)).await;
    });
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
    hang_guard!(async {
        let lsp = InProcessLsp::new();
        assert_shutdown_returns_null(&lsp, &json!({})).await;
    });
}

/// Calling `shutdown` with `null` params on a bare [`InProcessLsp`] before the
/// initialize/initialized handshake should not panic and should return `Ok(Value::Null)`.
///
/// This covers the canonical parameterless shutdown call (`params: null` per the LSP spec)
/// on a pre-handshake server, paralleling the `json!({})` variant in
/// `shutdown_before_initialize` above. Both variants must be safe on the pre-handshake
/// path because the shutdown arm in the bridge ignores params entirely.
#[tokio::test]
async fn shutdown_before_initialize_with_null_params() {
    hang_guard!(async {
        let lsp = InProcessLsp::new();
        assert_shutdown_returns_null(&lsp, &json!(null)).await;
    });
}

/// The `shutdown` bridge arm ignores params entirely — it never deserializes or
/// validates them. This test locks in that permissive contract by sending two
/// unexpected values: an object with extra fields (`{"foo": 42}`) and a wrong JSON
/// type entirely (`"oops"`). Both must return `Ok(Value::Null)`.
///
/// The `"shutdown"` arm in `InProcessLsp::handle_request` calls `server.shutdown().await`
/// and returns `Ok(Value::Null)` without touching `params`. If a future change adds
/// strict param validation, this test will fail — making the behavior change
/// inescapable rather than accidental.
///
/// Uses `InProcessLsp::new()` (pre-handshake) to avoid handshake overhead; the
/// shutdown arm does not consult initialization state.
#[tokio::test]
async fn shutdown_ignores_unexpected_params() {
    hang_guard!(async {
        // Object with unexpected extra fields — bridge must not reject this.
        let lsp = InProcessLsp::new();
        assert_shutdown_returns_null(&lsp, &json!({"foo": 42})).await;
        // Wrong JSON type entirely — bridge must not reject this either.
        let lsp = InProcessLsp::new();
        assert_shutdown_returns_null(&lsp, &json!("oops")).await;
    });
}

/// Mirror of `shutdown_ignores_unexpected_params` for the post-handshake path.
///
/// Guards the invariant that the `"shutdown"` arm in `InProcessLsp::handle_request`
/// ignores params regardless of initialization state. Both unexpected values
/// (`{"foo": 42}` and `"oops"`) must return `Ok(Value::Null)` after the
/// initialize/initialized handshake, just as they do before it.
///
/// Each payload gets a fresh `initialized_lsp().await` instance so that the two
/// assertions are isolated from any state the prior shutdown call may have left.
#[tokio::test]
async fn shutdown_ignores_unexpected_params_after_initialize() {
    hang_guard!(async {
        // Object with unexpected extra fields — bridge must not reject this post-handshake.
        let lsp = initialized_lsp().await;
        assert_shutdown_returns_null(&lsp, &json!({"foo": 42})).await;
        // Wrong JSON type entirely — bridge must not reject this post-handshake either.
        let lsp = initialized_lsp().await;
        assert_shutdown_returns_null(&lsp, &json!("oops")).await;
    });
}

/// Unit tests for the `completion_items` helper function.
///
/// These tests exercise `completion_items` in isolation using raw `serde_json::Value`
/// fixtures, without going through the LSP bridge.
mod completion_items_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn completion_items_extracts_from_array_response() {
        let val = json!([{"label": "foo"}]);
        let items = completion_items(&val);
        assert_eq!(items.len(), 1, "expected one item, got {}", items.len());
        assert_eq!(
            items[0]["label"], "foo",
            "first item label should be 'foo', got: {}",
            items[0]["label"]
        );
    }

    #[test]
    fn completion_items_extracts_from_list_response() {
        let val = json!({"isIncomplete": false, "items": [{"label": "bar"}]});
        let items = completion_items(&val);
        assert_eq!(items.len(), 1, "expected one item, got {}", items.len());
        assert_eq!(
            items[0]["label"], "bar",
            "first item label should be 'bar', got: {}",
            items[0]["label"]
        );
    }

    #[test]
    fn completion_items_returns_empty_slice_for_empty_array_response() {
        let val = json!([]);
        let items = completion_items(&val);
        assert!(
            items.is_empty(),
            "expected empty slice for json!([]), got {} items",
            items.len()
        );
    }

    #[test]
    fn completion_items_returns_empty_slice_for_empty_list_response() {
        let val = json!({"isIncomplete": false, "items": []});
        let items = completion_items(&val);
        assert!(
            items.is_empty(),
            "expected empty slice for json!({{\"isIncomplete\": false, \"items\": []}}), got {} items",
            items.len()
        );
    }

    #[test]
    #[should_panic(expected = "CompletionResponse::Array")]
    fn completion_items_panics_on_unexpected_shape() {
        completion_items(&json!(42));
    }

    #[test]
    #[should_panic(expected = "\"items\":42")]
    fn completion_items_panics_on_non_array_items_field() {
        completion_items(&json!({"items": 42}));
    }

    #[test]
    #[should_panic(expected = "CompletionResponse::Array")]
    fn completion_items_panics_on_list_missing_items_field() {
        completion_items(&json!({"isIncomplete": false}));
    }
}

/// Each `error_prefix` constant must actually appear in the error message
/// returned when the corresponding method receives malformed params.
///
/// This test serves as a compile-time anchor: if a format-string prefix in
/// bridge.rs is renamed, the constant definition must be updated too —
/// otherwise this test will fail at runtime, catching the drift immediately.
#[tokio::test]
async fn error_prefix_constants_match_actual_errors() {
    hang_guard!(async {
        let lsp = InProcessLsp::new();

        // Verify each constant is contained in the error for its matching method.
        assert_malformed_params_returns_error(&lsp, "initialize", error_prefix::INITIALIZE_PARAMS)
            .await;
        assert_malformed_params_returns_error(
            &lsp,
            "initialized",
            error_prefix::INITIALIZED_PARAMS,
        )
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
        assert_malformed_params_returns_error(
            &lsp,
            "textDocument/completion",
            error_prefix::COMPLETION_PARAMS,
        )
        .await;
        assert_malformed_params_returns_error(
            &lsp,
            "textDocument/hover",
            error_prefix::HOVER_PARAMS,
        )
        .await;
        assert_malformed_params_returns_error(
            &lsp,
            "textDocument/definition",
            error_prefix::DEFINITION_PARAMS,
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
    });
}

/// Document the server behavior when `initialize` fails (malformed params) and the
/// `initialized` notification is never sent, but downstream `textDocument/didOpen` and
/// `textDocument/completion` calls are made anyway.
///
/// **Permitted outcomes** (per the task description):
/// - The downstream calls succeed with well-formed responses — the bridge does not
///   gate them on the handshake state, so `did_open` and `completion` run normally.
/// - The downstream calls return a well-defined `Err` — a future change may add a
///   pre-handshake guard; that is also an acceptable outcome provided the error is
///   non-empty.  The code below honors both outcomes via `assert_ok_or_nonempty_err`.
///
/// **Regressions this test guards against:**
/// - A panic or hang on `textDocument/didOpen` / `textDocument/completion` when the
///   handshake was never completed.
/// - An ill-formed (e.g. empty string) error being returned on the pre-handshake path.
/// - A future permissive-to-strict change that silently swallows the error instead of
///   surfacing it.
///
/// Mirrors the setup of `initialize_error_does_not_corrupt_server_state` (same failing
/// payload, same `InProcessLsp::new()` start), but replaces the "recovery initialize"
/// assertion with downstream `didOpen` + `completion` calls.
#[tokio::test]
async fn downstream_ops_after_malformed_initialize_without_initialized() {
    hang_guard!(async {
        let lsp = InProcessLsp::new();

        // Step 1: send initialize with a completely wrong payload type.
        // Uses json!(42) — the canonical malformed payload in this file — to guarantee
        // serde rejects it before server.initialize() is ever called, so workspace_root
        // and stdlib_path remain at their defaults.
        assert_malformed_params_returns_error(&lsp, "initialize", error_prefix::INITIALIZE_PARAMS)
            .await;

        // Step 2: intentionally skip the `initialized` notification.
        // The `initialized()` handler is a no-op, so skipping it produces no server-side
        // state change — but this test documents that the server also does not require it
        // before accepting downstream calls.

        // Step 3: send textDocument/didOpen on the same (never-handshook) instance.
        // bracket_source() does not require stdlib resolution to parse, so compilation
        // proceeds even without the workspace_root that initialize() would have set.
        let did_open_result = lsp
            .handle_request(
                "textDocument/didOpen",
                did_open_params("file:///test.ri", reify_test_support::bracket_source()),
            )
            .await;
        if let Some(val) = assert_ok_or_nonempty_err(did_open_result, "didOpen before initialized")
        {
            assert_eq!(
                val,
                serde_json::Value::Null,
                "didOpen should return exactly Ok(Value::Null) when it succeeds, got: {val}"
            );
        }

        // Step 4: send textDocument/completion at the same position used by the happy-path
        // reference test did_open_and_completion_returns_items (line 1, character 0).
        // The bridge dispatches textDocument/completion regardless of initialization state:
        // the match arm in bridge.rs has no init-state guard.  If didOpen failed earlier
        // and the DocumentStore entry was never created, the completion response will be
        // Ok(Value::Null) (no completions) rather than an item list.
        let completion_result = lsp
            .handle_request(
                "textDocument/completion",
                json!({
                    "textDocument": { "uri": "file:///test.ri" },
                    "position": { "line": 1, "character": 0 }
                }),
            )
            .await;
        if let Some(val) =
            assert_ok_or_nonempty_err(completion_result, "completion before initialized")
            && !val.is_null()
        {
            let _items = completion_items(&val);
        }
    });
}

/// Unit tests for the `with_hang_guard` helper.
mod hang_guard_tests {
    use super::*;

    /// A future that completes immediately should not panic.
    #[tokio::test]
    async fn completes_fast_future() {
        hang_guard!(async {});
    }

    /// A future that exceeds the timeout should panic with "must not hang".
    #[tokio::test]
    #[should_panic(expected = "must not hang")]
    async fn panics_on_timeout() {
        with_hang_guard(1, tokio::time::sleep(Duration::from_secs(60))).await;
    }

    /// The panic message must include the caller's file name so hangs are
    /// immediately attributable to a specific test location.
    ///
    /// Uses `#[should_panic(expected = "in_process_bridge.rs")]` to verify
    /// that the auto-derived caller location appears in the panic message.
    #[tokio::test]
    #[should_panic(expected = "in_process_bridge.rs")]
    async fn timeout_panic_reports_caller_location() {
        with_hang_guard(1, tokio::time::sleep(Duration::from_secs(60))).await;
    }
}

/// Unit tests for the `assert_ok_or_nonempty_err` helper.
///
/// These tests exercise the helper in isolation with synthetic `Result` values,
/// without going through the LSP bridge.
mod assert_ok_or_nonempty_err_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ok_returns_some() {
        let result: Result<serde_json::Value, String> = Ok(json!(42));
        let out = assert_ok_or_nonempty_err(result, "ok_returns_some");
        assert_eq!(
            out,
            Some(json!(42)),
            "Ok(val) should return Some(val), got: {out:?}"
        );
    }

    #[test]
    fn nonempty_err_returns_none() {
        let result: Result<serde_json::Value, String> = Err("some error".into());
        let out = assert_ok_or_nonempty_err(result, "nonempty_err_returns_none");
        assert_eq!(out, None, "non-empty Err should return None, got: {out:?}");
    }

    #[test]
    #[should_panic(expected = "non-empty")]
    fn empty_err_panics() {
        let result: Result<serde_json::Value, String> = Err(String::new());
        assert_ok_or_nonempty_err(result, "empty_err_panics");
    }
}
