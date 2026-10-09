//! Tests for the LspBridge Tauri integration.

use std::sync::Arc;

use serde_json::json;

use crate::lsp_bridge::{LspBridge, lsp_request_impl};
use crate::tests::test_helpers::init_and_open;
use reify_lsp::test_support::RecordingSink;

#[tokio::test]
async fn lsp_bridge_can_be_constructed_and_initialized() {
    let bridge = LspBridge::new();
    let result = lsp_request_impl(
        &bridge,
        "initialize",
        reify_test_support::MINIMAL_INIT_PARAMS_JSON.to_string(),
    )
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
    lsp_request_impl(
        bridge,
        "initialize",
        reify_test_support::MINIMAL_INIT_PARAMS_JSON.to_string(),
    )
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

    lsp_request_impl(
        &bridge,
        "initialize",
        reify_test_support::MINIMAL_INIT_PARAMS_JSON.to_string(),
    )
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

    lsp_request_impl(
        &bridge,
        "initialize",
        reify_test_support::MINIMAL_INIT_PARAMS_JSON.to_string(),
    )
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
    let bridge = LspBridge::new();
    for case in ["not json", "", "{", "\"unterminated"] {
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
async fn lsp_request_impl_null_literal_passes_json_parse_step() {
    let bridge = LspBridge::new();
    // Invariant: `null` IS valid JSON (RFC 8259), so the JSON parse step in
    // `lsp_request_impl` must accept it. Whether the downstream handler accepts
    // or rejects null is outside this test's scope — we only assert that the
    // "invalid JSON params" prefix is NOT emitted (that prefix is emitted only
    // by the JSON parse step, not by any handler).
    let result = lsp_request_impl(&bridge, "initialize", "null".to_string()).await;
    assert!(
        !matches!(&result, Err(e) if e.contains("invalid JSON params")),
        "null literal should not trigger a JSON parse error, got: {result:?}"
    );
}

// ── Task 5772: the LSP large-stack seam ──────────────────────────────────────
//
// `lsp_request` reaches `reify-syntax`'s CST-to-AST walk (no `stacker` guard, no
// depth cap) and `reify-compiler`'s recursive compile, on a tokio worker's
// default ~2 MiB stack, at keystroke frequency. `lsp_request_on_worker` routes
// that dispatch onto the persistent 256 MiB LSP lane.
//
// `main.rs::lsp_request` takes `tauri::State` and cannot be constructed
// headlessly, so — exactly as the task-5357 and step-8 guards do for the engine
// commands — these test the COMPOSITION the wrapper performs, not `main.rs`
// source text.
//
// SCOPE: all fourteen of `InProcessLsp::handle_request`'s arms get the lane's
// stack. Ten run inline in `handle_request`; the other four
// (`textDocument/definition`, `prepareRename`, `rename`, `references`) run their
// blocking work on the thread polling `handle_request`, because the bridge picks
// `BlockingWorkPlacement::CallingThread` — pinned by
// `lsp_bridge_answers_the_blocking_work_methods_with_the_blocking_pool_saturated`.
//
// Since task 6517 the fourteen arms are also split across two lanes — six
// ordered + every unrecognised method on `LSP_LANE`, eight read-only queries on
// `LSP_POOL`. That is orthogonal to the stack every arm now gets; the parity
// table in (b) spans every pooled arm plus one ordered-lane arm, because those
// are exactly the arms whose lane changed.

/// Compile-time proof that `T` satisfies the bound the lane rests on. Never
/// runs; naming the type is the assertion.
fn assert_send_sync_static<T: Send + Sync + 'static>() {}

/// (a) `Arc<LspBridge>` is `Send + Sync + 'static` — the bound
/// `run_on_lsp_worker`'s `'static` closure requires.
///
/// It must already be true: `main.rs` `app.manage`s the bridge, and Tauri
/// requires managed state to be `Send + Sync + 'static`. Pinned HERE so the
/// migration does not silently depend on that staying true — if a future field
/// makes `LspBridge` non-`Sync`, this fails in the lib test target rather than as
/// a puzzling error in `main.rs`, which only builds under `--features gui`.
#[test]
fn lsp_bridge_arc_is_send_sync_and_static() {
    assert_send_sync_static::<Arc<LspBridge>>();
    assert_send_sync_static::<LspBridge>();
}

/// (b) RESULT PARITY — for each covered method, the value returned THROUGH the
/// lane equals the value `lsp_request_impl` returns directly, against an
/// equivalently-driven bridge.
///
/// This is the load-bearing migration guard: the lane hop must be invisible to
/// the frontend. Two independently-constructed bridges are driven identically,
/// so equal responses mean the routing changed nothing observable.
///
/// # Why the table spans the blocking-work arms too
///
/// `hover` / `completion` / `documentSymbol` are the keystroke/cursor-frequency
/// arms. `definition` and `references` are two of the four blocking-work arms,
/// whose parse/compile work the bridge runs on the thread polling
/// `handle_request` rather than on tokio's blocking pool (see
/// `lsp_bridge_answers_the_blocking_work_methods_with_the_blocking_pool_saturated`),
/// so through the lane they compute ON the lane. Including them pins that the
/// deepest work the lane carries is just as invisible to the frontend.
///
/// The `must_resolve` column is the anti-vacuity guard for exactly those two: a
/// `null == null` comparison would satisfy the parity assertion while proving
/// nothing ran, so they additionally have to produce a real answer.
///
/// # (m) Task 6517: the table now spans BOTH LANES, and every pooled arm
///
/// This is the migration guard the ordered-lane/query-pool split rests on, so
/// it has to cover the arms whose LANE CHANGED — which is precisely what the
/// five original rows did not. `documentHighlight`, `prepareRename` and
/// `rename` are added, completing the eight methods `lane_for_method` routes to
/// the pool, and `didChange` is added as an ORDERED-lane row so the table spans
/// both destinations rather than silently testing one.
///
/// `must_resolve` is set for the three added arms, two of which
/// (`prepareRename`, `rename`) are blocking-work arms: a `null == null`
/// comparison is exactly what an arm that stopped running looks like, and these
/// are the ones whose routing moved.
///
/// `didChange` is LAST on purpose. It mutates both bridges — identically, so
/// parity would hold either way — but running it earlier would silently change
/// the text every later row queries, making a failure hard to attribute.
#[tokio::test]
async fn lsp_request_on_worker_matches_direct_results_for_covered_methods() {
    use crate::lsp_bridge::lsp_request_on_worker;

    const URI: &str = "file:///parity.ri";

    let direct = LspBridge::new();
    init_and_open(&direct, URI).await;

    let worker = Arc::new(LspBridge::new());
    init_and_open(&worker, URI).await;

    // (method, params, must_resolve): `must_resolve` demands a non-`null`
    // response, so the case cannot pass by both sides answering "nothing".
    let cases = [
        // Inline arms — a hover and a completion are the two the task
        // description names as firing on effectively every keystroke and cursor
        // move.
        (
            "textDocument/hover",
            json!({
                "textDocument": { "uri": URI },
                "position": { "line": 1, "character": 4 }
            }),
            false,
        ),
        (
            "textDocument/completion",
            json!({
                "textDocument": { "uri": URI },
                "position": { "line": 1, "character": 0 }
            }),
            false,
        ),
        (
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": URI } }),
            false,
        ),
        // Blocking-work arms — computed on the lane itself. Positions match
        // reify-lsp's own handler tests: `thickness` in a constraint (line 9)
        // and the `width` declaration token (line 1).
        (
            "textDocument/definition",
            json!({
                "textDocument": { "uri": URI },
                "position": { "line": 9, "character": 15 }
            }),
            true,
        ),
        (
            "textDocument/references",
            json!({
                "textDocument": { "uri": URI },
                "position": { "line": 1, "character": 10 },
                "context": { "includeDeclaration": true }
            }),
            true,
        ),
        // ── task 6517: the remaining POOL-routed arms ────────────────────────
        // Inline arm, but on the `width` declaration token (line 1) it produces
        // real highlights, so it can carry `must_resolve`.
        (
            "textDocument/documentHighlight",
            json!({
                "textDocument": { "uri": URI },
                "position": { "line": 1, "character": 10 }
            }),
            true,
        ),
        // Blocking-work arms, on the same `width` declaration token
        // `references` uses.
        (
            "textDocument/prepareRename",
            json!({
                "textDocument": { "uri": URI },
                "position": { "line": 1, "character": 10 }
            }),
            true,
        ),
        (
            "textDocument/rename",
            json!({
                "textDocument": { "uri": URI },
                "position": { "line": 1, "character": 10 },
                "newName": "span"
            }),
            true,
        ),
        // ── task 6517: an ORDERED-lane arm, so the table spans both lanes ────
        // A notification: it answers `Null`, hence `must_resolve: false`. LAST,
        // because it MUTATES both bridges — identically, so parity holds either
        // way, but an earlier position would change the text every later row
        // queries.
        (
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": URI, "version": 2 },
                "contentChanges": [
                    { "text": reify_test_support::bracket_source_with_width("123mm") }
                ]
            }),
            false,
        ),
    ];

    for (method, params, must_resolve) in cases {
        let expected = lsp_request_impl(&direct, method, params.to_string())
            .await
            .unwrap_or_else(|e| panic!("direct {method} should succeed: {e}"));

        let actual = lsp_request_on_worker(
            Arc::clone(&worker),
            method.to_string(),
            params.to_string(),
        )
        .await
        .unwrap_or_else(|e| panic!("{method} through the lane should succeed: {e}"));

        assert_eq!(
            actual, expected,
            "{method} through the LSP lane must return exactly what a direct \
             call returns — the lane hop must be invisible to the frontend"
        );

        if must_resolve {
            let parsed: serde_json::Value = serde_json::from_str(&actual)
                .unwrap_or_else(|e| panic!("{method} response must be JSON: {e}"));
            assert!(
                !parsed.is_null(),
                "{method} must produce a real answer through the lane, not \
                 `null` — a null-vs-null comparison would satisfy the parity \
                 assertion while proving the arm never ran"
            );
        }
    }
}

/// The bridge's PLACEMENT: `definition`, `prepareRename`, `rename` and
/// `references` answer with tokio's blocking pool saturated, so their
/// parse/compile work runs on whatever thread polls `handle_request` rather than
/// on a ~2 MiB blocking-pool thread. In production that is the query-pool
/// consumer that drives it: the production entry point reaches a lane thread per
/// `lsp_lane_routing_tests`' (q), and its (i) routes these four to `LSP_POOL`.
///
/// Driven through [`lsp_request_impl`], not `lsp_request_on_worker`: the
/// property under test is the bridge's placement, and parking the
/// process-global lane behind a test-owned saturated runtime would couple this
/// test to every other lane user. Both constructors are covered: `new` (the
/// tests' bridge) and `with_sink` (the one `main.rs` builds).
#[test]
fn lsp_bridge_answers_the_blocking_work_methods_with_the_blocking_pool_saturated() {
    use std::time::Duration;

    use reify_lsp::blocking_work::test_support::{SaturatedBlockingPool, blocking_work_requests};

    const URI: &str = "file:///placement.ri";
    /// Turns a request stuck behind the saturated pool into a failure; a
    /// request that never touches the pool answers without timing anything.
    const HANG_BOUND: Duration = Duration::from_secs(60);

    let requests = blocking_work_requests(URI);

    let bridges = [
        ("LspBridge::new", LspBridge::new()),
        (
            "LspBridge::with_sink",
            LspBridge::with_sink(Arc::new(reify_lsp::server::NoOpSink)),
        ),
    ];
    for (constructor, bridge) in bridges {
        let pool = SaturatedBlockingPool::new();
        pool.block_on(async {
            init_and_open(&bridge, URI).await;
            for (method, params) in &requests {
                let answer = tokio::time::timeout(
                    HANG_BOUND,
                    lsp_request_impl(&bridge, method, params.to_string()),
                )
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "{constructor}: {method} waited on the blocking pool, so its \
                         compiler work would run on a ~2 MiB pool thread, not the lane"
                    )
                })
                .unwrap_or_else(|e| panic!("{constructor}: {method} failed: {e}"));
                let parsed: serde_json::Value = serde_json::from_str(&answer).unwrap_or_else(|e| {
                    panic!("{constructor}: {method} response must be JSON: {e}")
                });
                assert!(
                    !parsed.is_null(),
                    "{constructor}: {method} answered null at a position the bracket \
                     fixture resolves"
                );
            }
        });
    }
}

/// (c) The ERROR path is preserved: the lane hop must not turn an `Err` into a
/// panic (which would unwind the Tauri command and leave the frontend's
/// `invoke` promise unresolved — a silently dead editor pane).
#[tokio::test]
async fn lsp_request_on_worker_preserves_the_error_path() {
    use crate::lsp_bridge::lsp_request_on_worker;

    let bridge = Arc::new(LspBridge::new());

    // Malformed params: rejected by the JSON parse step in `lsp_request_impl`.
    let err = lsp_request_on_worker(
        Arc::clone(&bridge),
        "initialize".to_string(),
        "not json".to_string(),
    )
    .await
    .expect_err("malformed JSON params must still return Err through the lane");
    assert!(
        err.contains("invalid JSON params"),
        "the lane must forward the original parse error verbatim, got: {err}"
    );

    // Unsupported method: rejected by `handle_request`'s fallthrough arm.
    let err = lsp_request_on_worker(
        Arc::clone(&bridge),
        "textDocument/notAThing".to_string(),
        "{}".to_string(),
    )
    .await
    .expect_err("an unsupported method must still return Err through the lane");
    assert!(
        !err.is_empty(),
        "the unsupported-method error must survive the lane hop with a message"
    );
}

/// (d) The ORDERED-lane helper `run_on_lsp_worker` genuinely runs its future on
/// the lane thread — not inline on the awaiting tokio worker.
///
/// SCOPE, corrected in the task-6517 amendment pass: this probes
/// `crate::large_stack::run_on_lsp_worker`, which since the routing change has
/// no production caller — `lsp_request_on_worker` is
/// `lsp_request_on_lane(lane_for_method(&method), ..)`. So the claim this doc
/// used to make ("if `lsp_request_on_worker` were quietly awaiting
/// `lsp_request_impl` directly, only this test would notice") is no longer this
/// test's to make; it belongs to `lsp_lane_routing_tests`' (q), which asserts
/// it against the production entry point through a thread-recording sink. What
/// (d) still pins, and what it is kept for, is the ORDERED lane's own dispatch
/// mechanism: the helper several tests use for work that must be ordered
/// against the notification stream puts that work on `LSP_WORKER_THREAD_NAME`
/// rather than awaiting it inline.
#[tokio::test]
async fn the_lsp_lane_runs_its_work_off_the_awaiting_runtime_thread() {
    use crate::large_stack::{LSP_WORKER_THREAD_NAME, run_on_lsp_worker};

    let caller = std::thread::current().id();
    let (name, id) = run_on_lsp_worker(async {
        (
            std::thread::current().name().map(str::to_owned),
            std::thread::current().id(),
        )
    })
    .await;

    assert_eq!(
        name.as_deref(),
        Some(LSP_WORKER_THREAD_NAME),
        "LSP work must land on the named LSP lane thread"
    );
    assert_ne!(
        id, caller,
        "LSP work must not run inline on the awaiting tokio worker"
    );
}

/// (e) END-TO-END deep nesting: a real `.ri` document with deeply-nested
/// expressions is opened, hovered and searched for references THROUGH the
/// lane, and every request succeeds with a well-formed response.
///
/// This is the regression case the routing exists for — the keystroke-frequency
/// compiler-adjacent path (`reify-syntax`'s CST-to-AST walk, which has neither a
/// `stacker` guard nor a depth cap, then `reify-compiler`'s recursive compile)
/// driven over genuinely nested source rather than over a synthetic recursion.
///
/// The nesting depth is chosen to stay well under `reify-compiler`'s
/// `MAX_COMPILE_RECURSION_DEPTH` (256) so the request SUCCEEDS rather than being
/// refused by the depth cap — a refusal would make the test pass without ever
/// exercising a deep walk. The synthetic ~16 MiB assertion lives in
/// `large_stack_tests.rs`; this one proves the real path is wired to the same
/// lane.
///
/// EXERCISES BOTH ARM SHAPES, ACROSS BOTH LANES. `initialize`/`initialized`/
/// `didOpen` travel the ordered `LSP_LANE`; `hover` and `references` travel the
/// query `LSP_POOL`. `didOpen` and `hover` run inline in `handle_request`, and
/// `references` is a blocking-work arm the bridge runs on the polling thread, so
/// all of them compute on a lane's large stack.
#[tokio::test]
async fn deeply_nested_source_opens_hovers_and_finds_references_through_the_lane() {
    use crate::lsp_bridge::lsp_request_on_worker;

    /// Comfortably under `MAX_COMPILE_RECURSION_DEPTH` (256), and far above the
    /// 128 of "realistic-nesting headroom" the compiler's guard is sized for.
    const NESTING: usize = 100;

    let uri = "file:///deeply_nested.ri";
    let expr = format!("{}1mm{}", "(".repeat(NESTING), ")".repeat(NESTING));
    let source = format!("structure Deep {{\n    param width: Length = {expr}\n}}");

    let bridge = Arc::new(LspBridge::new());
    lsp_request_on_worker(
        Arc::clone(&bridge),
        "initialize".to_string(),
        reify_test_support::MINIMAL_INIT_PARAMS_JSON.to_string(),
    )
    .await
    .expect("initialize through the lane");
    lsp_request_on_worker(
        Arc::clone(&bridge),
        "initialized".to_string(),
        "{}".to_string(),
    )
    .await
    .expect("initialized through the lane");

    // didOpen drives the full parse + compile of the nested source.
    lsp_request_on_worker(
        Arc::clone(&bridge),
        "textDocument/didOpen".to_string(),
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "reify",
                "version": 1,
                "text": source
            }
        })
        .to_string(),
    )
    .await
    .expect("didOpen of deeply-nested source through the lane");

    // hover on the `width` param — the per-cursor-move request.
    let hovered = lsp_request_on_worker(
        Arc::clone(&bridge),
        "textDocument/hover".to_string(),
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 1, "character": 10 }
        })
        .to_string(),
    )
    .await
    .expect("hover over deeply-nested source through the lane");

    serde_json::from_str::<serde_json::Value>(&hovered)
        .expect("hover over deeply-nested source must return a well-formed JSON response");

    // references on the same `width` param — a blocking-work arm.
    let referenced = lsp_request_on_worker(
        Arc::clone(&bridge),
        "textDocument/references".to_string(),
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": 1, "character": 10 },
            "context": { "includeDeclaration": true }
        })
        .to_string(),
    )
    .await
    .expect("references over deeply-nested source through the lane");

    serde_json::from_str::<serde_json::Value>(&referenced)
        .expect("references over deeply-nested source must return a well-formed JSON response");
}

/// (f) The DEGRADED arm of the LSP routing, driven by the REAL production
/// composition rather than by a stand-in closure.
///
/// `dispatch_async`'s `None` arm is what runs when the OS refuses the 256 MiB
/// mapping. The generic guard for it — `large_stack_tests`'
/// `async_dispatch_without_a_lane_runs_inline_and_still_resolves` — submits
/// `|| (77u32, thread::current().id())`, a body that needs no runtime and so
/// cannot detect the hazard the PRODUCTION body carries: the only real caller
/// pre-bakes a [`tokio::runtime::Handle::block_on`], and `block_on` called from
/// inside a runtime panics "Cannot start a runtime from within a runtime". The
/// degraded arm therefore has to be exercised through the SAME function body
/// `lsp_request_on_worker` delegates to, or the test rots into testing a COPY of
/// the composition rather than the composition.
///
/// The claim is RESOLVING WITH THE RIGHT VALUE, not merely "did not hang": a
/// degraded arm that panics unwinds the Tauri command and leaves the frontend's
/// `invoke` promise unresolved — precisely the silently-dead-editor-pane outcome
/// the routing exists to prevent.
#[tokio::test]
async fn lsp_request_on_lane_without_a_lane_still_resolves_to_the_right_value() {
    use crate::lsp_bridge::lsp_request_on_lane;

    const URI: &str = "file:///degraded.ri";

    let direct = LspBridge::new();
    init_and_open(&direct, URI).await;

    let degraded = Arc::new(LspBridge::new());
    init_and_open(&degraded, URI).await;

    let params = json!({
        "textDocument": { "uri": URI },
        "position": { "line": 1, "character": 4 }
    })
    .to_string();

    let expected = lsp_request_impl(&direct, "textDocument/hover", params.clone())
        .await
        .expect("a direct hover must succeed");

    // `None` is exactly what `LSP_LANE.sender()` yields once the OS has refused
    // the 256 MiB mapping — the state this arm exists for.
    let actual = lsp_request_on_lane(
        None,
        Arc::clone(&degraded),
        "textDocument/hover".to_string(),
        params,
    )
    .await
    .expect("the degraded arm must RESOLVE to Ok, not panic and unwind the command");

    assert_eq!(
        actual, expected,
        "with no lane, the LSP seam must still return exactly what a direct \
         `lsp_request_impl` call returns — degradation is a stack downgrade, not \
         a behaviour change"
    );
}

/// (g) The lane-path counterpart of (f): the SAME seam, handed a REAL lane,
/// returns the SAME payload.
///
/// (f) and (g) together pin that the degradation is BEHAVIOUR-PRESERVING rather
/// than merely non-crashing — and they keep (f) honest in the other direction
/// too. A future change that silently sent every request down the degraded arm
/// would satisfy (f) alone; it fails (d)'s off-thread assertion, which submits
/// through the same lane API this seam uses.
#[tokio::test]
async fn lsp_request_on_lane_with_a_lane_returns_the_same_payload() {
    use crate::lsp_bridge::lsp_request_on_lane;

    const URI: &str = "file:///lane_parity.ri";

    let direct = LspBridge::new();
    init_and_open(&direct, URI).await;

    let laned = Arc::new(LspBridge::new());
    init_and_open(&laned, URI).await;

    let params = json!({
        "textDocument": { "uri": URI },
        "position": { "line": 1, "character": 4 }
    })
    .to_string();

    let expected = lsp_request_impl(&direct, "textDocument/hover", params.clone())
        .await
        .expect("a direct hover must succeed");

    let actual = lsp_request_on_lane(
        crate::large_stack::LSP_LANE.sender(),
        Arc::clone(&laned),
        "textDocument/hover".to_string(),
        params,
    )
    .await
    .expect("the lane arm must resolve to Ok");

    assert_eq!(
        actual, expected,
        "through the real lane the LSP seam must return exactly what a direct \
         `lsp_request_impl` call returns — the lane hop must be invisible"
    );
}

/// The lane drives its futures INSIDE a runtime context.
///
/// `dispatch_async` drives a lane job with `Handle::block_on` rather than a bare
/// executor because a future may need the runtime — `InProcessLsp`'s default
/// `BlockingPool` placement calls `tokio::task::spawn_blocking`. The bridge's
/// `CallingThread` placement means no LSP arm in this file reaches for it any
/// more, so the driver contract is pinned here directly.
#[tokio::test]
async fn the_lsp_lane_drives_its_futures_inside_a_runtime_context() {
    use crate::large_stack::run_on_lsp_worker;

    let answer = run_on_lsp_worker(async {
        tokio::task::spawn_blocking(|| 7u32)
            .await
            .expect("blocking work submitted from the lane completes")
    })
    .await;

    assert_eq!(answer, 7);
}

#[tokio::test]
async fn lsp_request_impl_valid_json_passes_json_parse_step() {
    // Table-driven: each entry is valid JSON that serde_json::from_str accepts.
    // `lsp_request_impl` must NOT return "invalid JSON params" for any of these
    // (that error is emitted only by the JSON parse step, not by any handler).
    let bridge = LspBridge::new();
    for case in ["{}", "[]", "42", "true", "null"] {
        let result = lsp_request_impl(&bridge, "initialize", case.to_string()).await;
        assert!(
            !matches!(&result, Err(e) if e.contains("invalid JSON params")),
            "valid JSON case {case:?} should not trigger a JSON parse error, got: {result:?}"
        );
    }
}
