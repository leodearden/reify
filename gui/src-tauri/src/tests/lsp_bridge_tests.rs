//! Tests for the LspBridge Tauri integration.

use std::sync::Arc;

use serde_json::json;

use crate::lsp_bridge::{LspBridge, lsp_request_impl};
use reify_lsp::server::NotificationSink;
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
// SCOPE, stated honestly and pinned by no assertion here to the contrary: of
// `InProcessLsp::handle_request`'s fourteen arms, four (`textDocument/definition`,
// `prepareRename`, `rename`, `references`) hop to `tokio::task::spawn_blocking`,
// so their compiler work runs on tokio's BLOCKING POOL at the std ~2 MiB default
// regardless of what thread `handle_request` itself is on. Putting the dispatch
// on a 256 MiB thread gives the big stack only to that thread's own frames. The
// arms this seam DOES cover are the other ten — including `didOpen`,
// `didChange`, `hover`, `completion`, `documentSymbol`, `documentHighlight` —
// which are precisely the keystroke/cursor-frequency ones. Closing the other
// four needs `crates/reify-lsp/src/server.rs`, outside this task's scope
// (task #6195).
//
// That stack cut is UNCHANGED by task 6517, and is a different cut from the LANE
// routing the section further down adds. Since 6517 the fourteen arms are split
// across two lanes — six ordered + everything unrecognised on `LSP_LANE`, eight
// read-only queries on `LSP_POOL` — which is orthogonal to which four get the
// big stack. Three of the pooled arms (`documentHighlight`, `prepareRename`,
// `rename`) were covered by NO test in this file before 6517; the parity table
// in (b) now spans every pooled arm plus one ordered-lane arm, because those are
// exactly the arms whose lane changed.

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

/// Drive a bridge to the same state the parity test needs: `initialize`,
/// `initialized`, and a `didOpen` of the shared bracket fixture.
async fn init_and_open(bridge: &LspBridge, uri: &str) {
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
    lsp_request_impl(
        bridge,
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "reify",
                "version": 1,
                "text": reify_test_support::bracket_source()
            }
        })
        .to_string(),
    )
    .await
    .expect("didOpen");
}

/// (b) RESULT PARITY — for each covered method, the value returned THROUGH the
/// lane equals the value `lsp_request_impl` returns directly, against an
/// equivalently-driven bridge.
///
/// This is the load-bearing migration guard: the lane hop must be invisible to
/// the frontend. Two independently-constructed bridges are driven identically,
/// so equal responses mean the routing changed nothing observable.
///
/// # Why the table spans BOTH arm shapes
///
/// `hover` / `completion` / `documentSymbol` run INLINE inside `handle_request`,
/// so the lane thread's own stack carries them. `definition` and `references`
/// instead hop to [`tokio::task::spawn_blocking`], whose first statement is
/// `Handle::current()` — and driving them is the entire reason
/// `dispatch_async` captures a [`tokio::runtime::Handle`] on the submitter and
/// uses `Handle::block_on` rather than a bare executor such as
/// `futures::executor::block_on`, which would panic "there is no reactor
/// running". That justification is stated three times across this module's docs
/// and was asserted nowhere: an inline-arms-only table leaves a bare-executor
/// refactor, or a runtime-flavour change, shipping green.
///
/// The `must_resolve` column is the anti-vacuity guard for exactly those two: a
/// `null == null` comparison would satisfy the parity assertion while proving
/// nothing ran, so the spawn_blocking cases additionally have to produce a real
/// answer. (No claim is made that those two get the LARGE STACK — their compiler
/// work runs on the blocking pool's ~2 MiB threads, see this file's header note
/// and task #6195. What is claimed is that they RESOLVE through the lane.)
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
/// (`prepareRename`, `rename`) hop to `spawn_blocking`: a `null == null`
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
        // `spawn_blocking` arms — reached from inside `Handle::block_on` on a
        // NON-runtime thread, the interaction the lane's driver choice exists
        // for. Positions match reify-lsp's own handler tests: `thickness` in a
        // constraint (line 9) and the `width` declaration token (line 1).
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
        // `spawn_blocking` arms, on the same `width` declaration token
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
/// test's to make; it belongs to (q), which asserts it against the production
/// entry point through a thread-recording sink. What (d) still pins, and what it
/// is kept for, is the ORDERED lane's own dispatch mechanism: the helper several
/// tests use for work that must be ordered against the notification stream puts
/// that work on `LSP_WORKER_THREAD_NAME` rather than awaiting it inline.
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
/// expressions is opened and hovered THROUGH the lane, and both requests
/// succeed with well-formed responses.
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
/// EXERCISES THE INLINE ARMS, ACROSS BOTH LANES. `didOpen` and `hover` both run
/// inline inside `handle_request`, so they genuinely get a lane's large stack —
/// but since task 6517 they get DIFFERENT lanes: `initialize`, `initialized` and
/// `didOpen` travel the ordered `LSP_LANE`, while the `hover` travels the query
/// `LSP_POOL`. The test name's "the lane" predates that split and is kept for
/// continuity; read it as "a lane". `definition`, `prepareRename`, `rename` and
/// `references` hop to `spawn_blocking` and get no large stack on either lane —
/// no assertion here claims otherwise.
#[tokio::test]
async fn deeply_nested_source_opens_and_hovers_through_the_lane() {
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

// ── Task 6517: the ordered lane / query pool split ───────────────────────────
//
// Task 5772's section above closes by naming its own limit: a lane has one
// consumer, so every `lsp_request` serializes against every other. Task 6517
// bounds that by routing LSP work over TWO lanes instead of one.
//
// The classification key is LSP PROTOCOL semantics — does this method mutate
// server-side document/session state? — and deliberately NOT `reify-lsp`'s
// internal choice of which arms hop to `spawn_blocking`. Keying on the latter
// would have been the narrower change (it is exactly the four arms whose lane
// occupancy hurts most), but it couples `gui/src-tauri` to an implementation
// detail of another crate that this crate cannot observe or test, and that would
// rot silently the day `reify-lsp` moved an arm. The pool subsumes it without
// the coupling: work that gains nothing from the big stack merely occupies one
// of N consumers instead of the only one.
//
// * ORDERED lane (`LSP_LANE`, size 1, unchanged): the six state-mutating and
//   lifecycle methods — plus, conservatively, ANY unrecognised method.
// * QUERY pool (`LSP_POOL`, size `LSP_POOL_SIZE`): the eight read-only queries.
//
// The membership is spelled out ONCE per direction of the check: authoritatively
// in `lane_for_method`'s `matches!` arm, and as the `ORDERED_METHODS` /
// `QUERY_METHODS` constants below, which (j) relates to `HANDLE_REQUEST_ARMS`
// and executes against the real dispatcher. A third prose copy here would be a
// list nothing checks, so this comment carries only the counts.
//
// Ordering among NOTIFICATIONS is therefore preserved exactly — one FIFO
// consumer, which is what `didChange` correctness rests on, pinned by (k). The
// only ordering given up is query-vs-notification, which is precisely the
// pre-5772 behaviour on the multi-threaded tauri runtime and which `reify-lsp`'s
// own `RwLock`/`Mutex` already serialise for safety: a query can read older
// text — staleness, never corruption.
//
// SCOPE UNCHANGED by this split: the four `spawn_blocking` arms still run their
// compiler work on tokio's blocking pool at the std ~2 MiB default, whichever
// lane submitted them (task #6195). What changes is that they no longer occupy
// the ONLY LSP consumer while doing it.

/// The order-sensitive / lifecycle methods, which must keep a single FIFO
/// consumer. `initialize` / `initialized` / `shutdown` are session lifecycle;
/// the three `did*` notifications mutate the server's document set, and
/// `didOpen`/`didChange` additionally hold `reify-lsp`'s `eval_state` mutex
/// across a synchronous diagnostics eval.
///
/// Reordering two of these against each other is CORRUPTION, not staleness —
/// applying edit N+1 before edit N yields text neither the client nor the server
/// ever had — which is why the pool must not carry them.
const ORDERED_METHODS: [&str; 6] = [
    "initialize",
    "initialized",
    "textDocument/didOpen",
    "textDocument/didChange",
    "textDocument/didClose",
    "shutdown",
];

/// The read-only query methods, which may run concurrently.
///
/// None mutates server-side state: each takes `state.read().await`, clones what
/// it needs and drops the guard (four of them then hop to `spawn_blocking`).
/// Reordering these against a notification can only make one read older text.
const QUERY_METHODS: [&str; 8] = [
    "textDocument/completion",
    "textDocument/hover",
    "textDocument/definition",
    "textDocument/documentSymbol",
    "textDocument/documentHighlight",
    "textDocument/prepareRename",
    "textDocument/rename",
    "textDocument/references",
];

/// Strings `InProcessLsp::handle_request` does NOT accept — they fall through to
/// its `other => Err(UNSUPPORTED_METHOD)` arm.
///
/// These are the load-bearing cases of (h), not filler. They pin the
/// CONSERVATIVE default: a method added to `reify-lsp` tomorrow — which may well
/// mutate state — must NOT silently acquire concurrency here by virtue of not
/// being listed.
const UNRECOGNISED_METHODS: [&str; 2] = ["textDocument/notAThing", ""];

/// Every arm string `InProcessLsp::handle_request` accepts, transcribed from
/// `crates/reify-lsp/src/bridge.rs`'s `match method` (the fourteen arms before
/// its `other =>` fallthrough).
///
/// Transcribed by hand, and therefore NOT self-validating: (j) drives every
/// entry through the real dispatcher rather than trusting the transcription, so
/// an arm renamed or deleted in `reify-lsp` reds this file instead of drifting
/// out of it silently. An earlier revision of this comment claimed the
/// transcription itself was the cross-check; it was not — three hand-maintained
/// `const` arrays in one file can only ever prove each other consistent.
const HANDLE_REQUEST_ARMS: [&str; 14] = [
    "initialize",
    "initialized",
    "textDocument/didOpen",
    "textDocument/didChange",
    "textDocument/didClose",
    "textDocument/completion",
    "textDocument/hover",
    "textDocument/definition",
    "textDocument/documentSymbol",
    "textDocument/documentHighlight",
    "textDocument/prepareRename",
    "textDocument/rename",
    "textDocument/references",
    "shutdown",
];

/// (h) Order-sensitive methods — and every UNRECOGNISED method — route to the
/// ORDERED lane.
///
/// Compared by POINTER IDENTITY rather than by any observable behaviour, because
/// that is the only comparison that cannot be satisfied by coincidence: two
/// distinct lanes both answer a probe correctly, and both run it off the
/// caller's thread. Only pointer equality says "the same queue".
///
/// The unknown-method rows are the ones worth having. A `matches!` over the
/// CONCURRENCY-SAFE set with `_ => ordered` makes the safe direction structural;
/// the opposite spelling (list the ordered set, default to the pool) would look
/// identical in review and would hand concurrency to every future method by
/// default. These rows are what tells the two apart.
#[test]
fn order_sensitive_methods_route_to_the_ordered_lane() {
    use crate::large_stack::LSP_LANE;
    use crate::lsp_bridge::lane_for_method;

    let ordered = LSP_LANE.sender().map(std::ptr::from_ref);
    assert!(
        ordered.is_some(),
        "precondition: the ordered lane must have started, or every row below \
         would compare `None` to `None` and pass vacuously"
    );

    for method in ORDERED_METHODS {
        assert_eq!(
            lane_for_method(method).map(std::ptr::from_ref),
            ordered,
            "{method} mutates server-side state or is session lifecycle, so it \
             must travel the single-consumer ORDERED lane — reordering two of \
             these is corruption, not staleness"
        );
    }

    for method in UNRECOGNISED_METHODS {
        assert_eq!(
            lane_for_method(method).map(std::ptr::from_ref),
            ordered,
            "{method:?} is not an arm `handle_request` accepts, so it must \
             default to the ORDERED lane. A future state-mutating method must \
             not acquire concurrency merely by not being listed."
        );
    }
}

/// (i) The eight read-only query methods route to the QUERY POOL.
///
/// The non-vacuity assertion is what stops (h) and (i) both passing against a
/// single lane: if `LSP_POOL.sender()` and `LSP_LANE.sender()` were the same
/// pointer, every row of both tables would hold while nothing had been split.
#[test]
fn query_methods_route_to_the_query_pool() {
    use crate::large_stack::{LSP_LANE, LSP_POOL};
    use crate::lsp_bridge::lane_for_method;

    let pool = LSP_POOL.sender().map(std::ptr::from_ref);
    let ordered = LSP_LANE.sender().map(std::ptr::from_ref);
    assert!(
        pool.is_some(),
        "precondition: the query pool must have started, or every row below \
         would compare `None` to `None` and pass vacuously"
    );
    assert_ne!(
        pool, ordered,
        "the pool and the ordered lane must be DIFFERENT queues, or (h) and (i) \
         would both hold with nothing actually split"
    );

    for method in QUERY_METHODS {
        assert_eq!(
            lane_for_method(method).map(std::ptr::from_ref),
            pool,
            "{method} only reads server-side state, so it must travel the query \
             pool — that is what bounds head-of-line blocking among queries"
        );
    }
}

/// (j) The classification is TOTAL over what the bridge can dispatch: the union
/// of the ordered and query tables is exactly `handle_request`'s arm set.
///
/// Without this, a method that exists in `reify-lsp` but appears in neither
/// table would simply take `lane_for_method`'s conservative fallthrough and no
/// test would ever mention it. That is safe but silent — and silence is how a
/// keystroke-frequency method ends up on the ordered lane by accident and stays
/// there. This is the test that makes adding an arm to `reify-lsp` a decision
/// here rather than a default.
///
/// Disjointness is asserted too: a method listed in BOTH tables would make (h)
/// and (i) contradictory, and whichever ran second would look like a routing bug
/// rather than a table bug.
///
/// # The arm list is EXECUTED against the real dispatcher, not just compared
///
/// The set half above relates three hand-maintained `const` arrays in this file
/// to each other, which proves them internally consistent and nothing more:
/// rename or delete an arm in `crates/reify-lsp/src/bridge.rs` and every set
/// assertion still holds, because `HANDLE_REQUEST_ARMS` is a copy, not an
/// observation. So the second half calls [`lsp_request_impl`] for every entry of
/// both tables and reads the answer through `reify-lsp`'s OWN public constant,
/// `bridge::error_prefix::UNSUPPORTED_METHOD` — the exact string its `other =>`
/// fallthrough emits:
///
/// * every `HANDLE_REQUEST_ARMS` entry must NOT come back unsupported (it may
///   come back `Ok`, or `Err` from its own params parse — both mean the arm is
///   there), and
/// * every `UNRECOGNISED_METHODS` entry MUST come back unsupported.
///
/// That is what turns "adding an arm to `reify-lsp` is a decision here" from a
/// claim into a mechanism, in both directions: a NEW arm nobody classified is
/// caught by the set half only once it is transcribed, but a REMOVED or RENAMED
/// arm — the drift that no assertion could previously see — now reds this test
/// on the first run.
///
/// `"null"` is the params payload for every row because the question is which
/// ARM was reached, never whether it liked its arguments. Each arm is driven on
/// its OWN bridge so nothing (notably `shutdown`) can leave state that changes a
/// later row's answer.
#[tokio::test]
async fn the_classification_covers_every_dispatchable_method() {
    use reify_lsp::bridge::error_prefix::UNSUPPORTED_METHOD;
    use std::collections::BTreeSet;

    let ordered: BTreeSet<&str> = ORDERED_METHODS.into_iter().collect();
    let queries: BTreeSet<&str> = QUERY_METHODS.into_iter().collect();
    let arms: BTreeSet<&str> = HANDLE_REQUEST_ARMS.into_iter().collect();

    let overlap: Vec<&str> = ordered.intersection(&queries).copied().collect();
    assert!(
        overlap.is_empty(),
        "no method may be classified BOTH order-sensitive and concurrency-safe, \
         got {overlap:?}"
    );

    let classified: BTreeSet<&str> = ordered.union(&queries).copied().collect();
    assert_eq!(
        classified, arms,
        "every arm `InProcessLsp::handle_request` accepts must be classified \
         exactly once. A method present in `arms` but not in `classified` would \
         silently take the conservative fallthrough with no test naming it; one \
         present in `classified` but not in `arms` is a stale entry in this file."
    );

    // ── The executable half: every row is driven through the real dispatcher ──

    for method in HANDLE_REQUEST_ARMS {
        let bridge = LspBridge::new();
        if let Err(e) = lsp_request_impl(&bridge, method, "null".to_string()).await {
            assert!(
                !e.starts_with(UNSUPPORTED_METHOD),
                "`{method}` is listed in HANDLE_REQUEST_ARMS but \
                 `InProcessLsp::handle_request` answered with its \
                 unsupported-method fallthrough ({e:?}). Either the arm was \
                 renamed or removed in `crates/reify-lsp/src/bridge.rs` — in \
                 which case ORDERED_METHODS / QUERY_METHODS still route a \
                 method that no longer exists — or this list has a typo. Both \
                 are classification bugs, which is why this row executes rather \
                 than merely comparing strings."
            );
        }
    }

    for method in UNRECOGNISED_METHODS {
        let bridge = LspBridge::new();
        let err = lsp_request_impl(&bridge, method, "null".to_string())
            .await
            .expect_err(&format!(
                "`{method:?}` is used by (h) as an UNRECOGNISED method — the row \
                 that pins `lane_for_method`'s conservative default — so \
                 `handle_request` must reject it. If it now succeeds, it is a \
                 real arm and must be classified, not treated as unknown."
            ));
        assert!(
            err.starts_with(UNSUPPORTED_METHOD),
            "`{method:?}` must be rejected by the `other =>` fallthrough \
             specifically, not by some arm's params parse — otherwise (h)'s \
             unknown-method rows would be pinning the default against a method \
             that is actually dispatchable. Got: {err:?}"
        );
    }
}

/// (k) The ORDERED lane still has EXACTLY ONE consumer after the `Lane`
/// generalisation.
///
/// This is the invariant `didChange` correctness rests on, and it is the one the
/// pool mechanism could most easily take away by accident — `Lane::pool` and
/// `Lane::new` share a receive loop, so a default that spawned more than one
/// consumer would leave every other test green while making concurrent edits
/// applicable out of order.
///
/// A size-N lane would fail this rather than pass it by luck: the shared
/// receiver lock is handed off after each dequeue, so consumers ROTATE across
/// sequential submissions even when only one job is in flight at a time.
///
/// Its twin for the other lane is (p): this pins `LSP_LANE` at exactly 1, and
/// (p) pins `LSP_POOL` at `LSP_POOL_SIZE`. Both counts are load-bearing and in
/// opposite directions, so neither can be left to the other.
#[test]
fn the_ordered_lane_still_has_exactly_one_consumer() {
    use crate::large_stack::LSP_LANE;
    use crate::tests::test_helpers::post_and_wait;
    use std::collections::HashSet;

    let ids: HashSet<_> = (0..16)
        .map(|_| post_and_wait(LSP_LANE.sender(), || std::thread::current().id()))
        .collect();
    assert_eq!(
        ids.len(),
        1,
        "the ordered lane must keep exactly ONE consumer: notifications are \
         order-sensitive against each other, and a second consumer would let a \
         `didChange` overtake an earlier one. (A degraded lane fails here too: \
         `post` then spawns a fresh thread per job.) Saw {ids:?}"
    );
}

/// (l) END-TO-END: a real `textDocument/hover` completes while another consumer
/// of the same lane is OCCUPIED — the head-of-line-blocking property, driven
/// through the REAL production composition.
///
/// `large_stack_tests`' (aa) measures the mechanism with synthetic jobs; this
/// measures the composition `lsp_request_on_worker` actually performs, via the
/// `lsp_request_on_lane` seam whose only variable is the lane. A test that
/// rebuilt the `dispatch_async(pool, lsp_request_future(..))` composition itself
/// would only prove its own copy is concurrent.
///
/// # Why a TEST-LOCAL pool and not `LSP_POOL`
///
/// Proving occupancy means PARKING a consumer, and `LSP_POOL` is a process-wide
/// `static` that every test in this binary shares while cargo runs them
/// concurrently. Parking one of its consumers would starve whichever other test
/// is using it — hanging the suite instead of failing it. The local pool is
/// size 2 for the same reason: park one, leave exactly one free, so the property
/// is deterministic rather than a race.
///
/// Against a SINGLE-consumer lane this fails as a clean `tokio::time::timeout`
/// elapse, not a hang, and the probe is released on every exit path.
#[tokio::test]
async fn a_query_does_not_queue_behind_an_occupied_lane_consumer() {
    use crate::large_stack::{Lane, post};
    use crate::lsp_bridge::lsp_request_on_lane;
    use std::sync::mpsc;
    use std::time::Duration;

    const URI: &str = "file:///pool_head_of_line.ri";
    const PREFIX: &str = "t6517-hol-";
    static TEST_POOL: Lane = Lane::pool(PREFIX, 2);

    let direct = LspBridge::new();
    init_and_open(&direct, URI).await;
    let pooled = Arc::new(LspBridge::new());
    init_and_open(&pooled, URI).await;

    let params = json!({
        "textDocument": { "uri": URI },
        "position": { "line": 1, "character": 4 }
    })
    .to_string();

    let expected = lsp_request_impl(&direct, "textDocument/hover", params.clone())
        .await
        .expect("a direct hover must succeed");

    // Park exactly ONE of the two consumers. Posting never waits, so the
    // runtime this test is on is never itself blocked.
    let (parked_tx, parked_rx) = mpsc::channel::<Option<String>>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let (finished_tx, finished_rx) = mpsc::channel::<()>();
    post(
        TEST_POOL.sender(),
        Box::new(move || {
            let _ = parked_tx.send(std::thread::current().name().map(str::to_owned));
            // Parks until the test drops `release_tx`, which it does on EVERY
            // exit path below — `recv` then returns `Err` and the job ends.
            let _ = release_rx.recv();
            let _ = finished_tx.send(());
        }),
    )
    .expect("posting the probe must succeed");

    let parked_on = parked_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the probe job must reach a consumer and report where it parked");
    assert!(
        parked_on.as_deref().is_some_and(|n| n.starts_with(PREFIX)),
        "the probe must occupy a real POOL consumer; a lane that degraded to a \
         spawned thread would leave both consumers free and make this test \
         vacuous. Parked on {parked_on:?}"
    );

    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        lsp_request_on_lane(
            TEST_POOL.sender(),
            Arc::clone(&pooled),
            "textDocument/hover".to_string(),
            params,
        ),
    )
    .await;

    // Read occupancy BEFORE releasing: the probe cannot have finished, because
    // only dropping `release_tx` — which this frame still holds — can end it.
    let probe_still_parked = finished_rx.try_recv().is_err();
    drop(release_tx);

    let actual = outcome
        .expect(
            "a hover must not queue behind an occupied consumer: it timed out, \
             which is what a single-consumer lane does here",
        )
        .expect("the hover must resolve to Ok through the pool");

    assert!(
        probe_still_parked,
        "the hover must have completed WHILE the other consumer was occupied — \
         if the probe had already finished, this would prove nothing about \
         concurrency"
    );
    assert_eq!(
        actual, expected,
        "a hover served by a free pool consumer must return exactly what a \
         direct `lsp_request_impl` call returns — the pool hop must be invisible \
         to the frontend"
    );
}

/// (n) An AWAITED sequence still observes its own edits, across the lane split.
///
/// This is the cross-lane ordering guard the split must not break, driven the
/// way `gui/src/editor/lspClient.ts` drives a single request: each `invoke`
/// awaited before the next is issued. The sequence spans BOTH destinations —
/// `initialize` / `initialized` / `didOpen` / `didChange` travel the ordered
/// lane, and the `hover` that reads the result travels the POOL, on a different
/// OS thread.
///
/// It pins the AWAITING client, which is not the whole of the shipped app, and
/// the scope is worth stating so this test is not read as covering more than it
/// does: `Editor.tsx` fires `didChange` from a debounced `setTimeout` that
/// nothing sequences on, and CodeMirror issues completion/hover/highlight from
/// independent sources, so a real query CAN overtake a real `didChange`. That
/// interleaving is disclosed on [`crate::large_stack::Lane`] as reachable
/// staleness; it is deliberately not asserted here, because the only property
/// available to assert about it — that neither answer is self-inconsistent — is
/// weaker than what (n) already establishes and would race on the scheduler.
///
/// What it pins is a happens-before that survives only because the client awaits
/// AND because `LSP_LANE` stays single-consumer: the `didChange` job has
/// returned (which is what resolved the awaited promise) before the `hover` is
/// ever submitted, so the pool consumer cannot observe pre-change text. A split
/// that had let notifications run concurrently would make this a race even for a
/// client that awaits.
///
/// The hover payload carries the parameter's DEFAULT VALUE (`param width:
/// Scalar[m] = 0.08 m`), which is what makes "the answer differs before and
/// after" a real observation rather than a hope: the pre-change assertion is a
/// stated precondition, and the post-change one asserts the new value is present
/// AND the old one is gone.
#[tokio::test]
async fn an_awaited_sequence_still_observes_its_own_edits() {
    use crate::lsp_bridge::lsp_request_on_worker;

    const URI: &str = "file:///awaited_sequence.ri";

    let bridge = Arc::new(LspBridge::new());

    lsp_request_on_worker(
        Arc::clone(&bridge),
        "initialize".to_string(),
        reify_test_support::MINIMAL_INIT_PARAMS_JSON.to_string(),
    )
    .await
    .expect("initialize");
    lsp_request_on_worker(
        Arc::clone(&bridge),
        "initialized".to_string(),
        "{}".to_string(),
    )
    .await
    .expect("initialized");
    lsp_request_on_worker(
        Arc::clone(&bridge),
        "textDocument/didOpen".to_string(),
        json!({
            "textDocument": {
                "uri": URI,
                "languageId": "reify",
                "version": 1,
                "text": reify_test_support::bracket_source()
            }
        })
        .to_string(),
    )
    .await
    .expect("didOpen");

    // On the `width` declaration token — the position reify-lsp's own hover
    // tests use.
    let hover_params = json!({
        "textDocument": { "uri": URI },
        "position": { "line": 1, "character": 10 }
    })
    .to_string();

    let before = lsp_request_on_worker(
        Arc::clone(&bridge),
        "textDocument/hover".to_string(),
        hover_params.clone(),
    )
    .await
    .expect("hover before the edit");
    assert!(
        before.contains("0.08 m"),
        "precondition: the pre-change hover must report the ORIGINAL default, or \
         the post-change assertion below proves nothing. Got: {before}"
    );

    lsp_request_on_worker(
        Arc::clone(&bridge),
        "textDocument/didChange".to_string(),
        json!({
            "textDocument": { "uri": URI, "version": 2 },
            "contentChanges": [
                { "text": reify_test_support::bracket_source_with_width("123mm") }
            ]
        })
        .to_string(),
    )
    .await
    .expect("didChange");

    let after = lsp_request_on_worker(
        Arc::clone(&bridge),
        "textDocument/hover".to_string(),
        hover_params,
    )
    .await
    .expect("hover after the edit");

    assert!(
        after.contains("0.123 m"),
        "a hover issued AFTER an awaited didChange must read the POST-change \
         text, even though it travels a different lane on a different thread. \
         Got: {after}"
    );
    assert!(
        !after.contains("0.08 m"),
        "the post-change hover must not still report the old default — that \
         would mean the pool consumer read text the ordered lane had already \
         replaced. Got: {after}"
    );
}

/// What `abandoned_didopen_outcome` observed. (o) and (o2) assert OPPOSITE
/// polarities on the same two fields, which is the whole difference between
/// them.
struct AbandonedOutcome {
    /// Every URI the recording sink saw diagnostics published for, in arrival
    /// order. Carried as `String` rather than `Url` so the callers' failure
    /// messages can print it without a second conversion.
    published: Vec<String>,
    /// Whether the abandoned URI has server-side state — a document, hence
    /// diagnostics — once the lane has had its chance at the job.
    abandoned_has_state: bool,
}

/// The shared body of (o) and (o2): manufacture an abandoned
/// `textDocument/didOpen` on `lane`, then report what the SERVER actually
/// observed.
///
/// (o) and (o2) differ in exactly two things — the destination's declared
/// `OnAbandon` policy, and the polarity of the conclusion — so the ~150 lines
/// they share are written ONCE here. Every part of that shape is load-bearing
/// and would be a drift hazard duplicated: the recording bridge and its
/// take-and-discard of the setup publishes, the probe that parks the lane's
/// single consumer, the elapsing timeout that PERFORMS the abandonment, the
/// release-on-every-exit-path discipline, and the live request that doubles as
/// the FIFO barrier. A fix applied to one copy would silently not reach the
/// other. Same reason `large_stack_tests`' `observe_concurrent_arrivals`
/// exists, and its docstring says so in as many words.
///
/// # What is asserted here, and what is left to the caller
///
/// Asserted here: the PRECONDITIONS of the measurement, which belong to neither
/// test's claim. The probe really occupied a real lane consumer (a lane that
/// degraded to a spawned thread would leave the queue free and make both
/// callers vacuous); the request really was abandoned (the elapse *is* the
/// abandonment); the live request really resolved, and answered `null` as a
/// notification must.
///
/// Left to the caller: everything about the ABANDONED job. Both the
/// server-side-effect field and the published list come back untouched, so each
/// test states its own conclusion — and the non-vacuity check on the LIVE uri
/// stays with the caller too, since it is what makes that test's own absence or
/// presence assertion mean something.
///
/// # Why a size-1 lane, and why test-local
///
/// Size 1 because the ABANDONMENT has to be forced: the request must sit in the
/// queue while its awaiting side is dropped, which means every consumer must be
/// occupied. Test-local because parking the process-wide `LSP_LANE` would
/// starve every concurrently-running test in this binary.
///
/// Ordering is deterministic rather than timed: the queue is FIFO with one
/// consumer, so the abandoned job is dequeued strictly before the live one, and
/// by the time the live request resolves the abandoned one has already had its
/// chance to publish.
///
/// # Why a `didOpen`, on lanes production routes one to only half of
///
/// Because the assertion has to be a SERVER-SIDE EFFECT, and `didOpen` is the
/// method with the loudest one. It is a probe here, not a claim about routing:
/// production sends `didOpen` to the ordered lane, which is `OnAbandon::Run`
/// precisely so a discard cannot happen to it. Reading (o) as "an abandoned
/// `didOpen` is dropped" inverts it — the discard follows from the LANE being
/// declared `Lane::cancelling_pool`, which is (o)'s to declare and (o2)'s to
/// contradict.
///
/// Asserting the server-side effect rather than merely "the lane recovered" is
/// the point of both. A lane that ran the abandoned job to completion and then
/// carried on would satisfy "recovered" perfectly while doing exactly the
/// wasted work (o) is about; equally, a lane that DROPPED it would satisfy
/// "recovered" while losing the document (o2) is about. Only the publish
/// distinguishes them.
async fn abandoned_didopen_outcome(
    lane: &'static crate::large_stack::Lane,
    lane_name: &str,
    abandoned_uri: &str,
    live_uri: &str,
) -> AbandonedOutcome {
    use crate::large_stack::post;
    use crate::lsp_bridge::lsp_request_on_lane;
    use std::sync::mpsc;
    use std::time::Duration;

    /// Broken source, so `didOpen` is GUARANTEED to publish error diagnostics
    /// through the sink — "no publish" then means "never ran", not "ran and had
    /// nothing to say".
    const BROKEN: &str = "structure {";

    let sink = Arc::new(RecordingSink::default());
    let bridge = Arc::new(LspBridge::with_sink(sink.clone()));

    // Session setup runs DIRECTLY, not through the lane, so the parked consumer
    // is the only thing between the abandoned request and its execution.
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
    // Discard any setup publishes so the caller's assertions speak only about
    // the two requests under test.
    let _ = sink.take_calls();

    let did_open = |uri: &str| {
        json!({
            "textDocument": {
                "uri": uri,
                "languageId": "reify",
                "version": 1,
                "text": BROKEN
            }
        })
        .to_string()
    };

    // Occupy the lane's single consumer.
    let (parked_tx, parked_rx) = mpsc::channel::<Option<String>>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    post(
        lane.sender(),
        Box::new(move || {
            let _ = parked_tx.send(std::thread::current().name().map(str::to_owned));
            let _ = release_rx.recv();
        }),
    )
    .expect("posting the probe must succeed");
    let parked_on = parked_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the probe job must reach the consumer and report where it parked");
    assert_eq!(
        parked_on.as_deref(),
        Some(lane_name),
        "the probe must occupy the real lane consumer; a lane that degraded to \
         a spawned thread would leave the queue free and make this test vacuous"
    );

    // Enqueue a real `didOpen` and then ABANDON it: the only consumer is parked,
    // so the timeout necessarily elapses and drops the submitted future.
    let abandoned = tokio::time::timeout(
        Duration::from_millis(200),
        lsp_request_on_lane(
            lane.sender(),
            Arc::clone(&bridge),
            "textDocument/didOpen".to_string(),
            did_open(abandoned_uri),
        ),
    )
    .await;
    assert!(
        abandoned.is_err(),
        "precondition: with the only consumer parked the request must not have \
         completed — that elapse is what abandons it"
    );

    // Release the consumer; it now dequeues the abandoned job.
    drop(release_tx);

    // A LIVE request through the same lane, which is also the FIFO barrier: by
    // the time it resolves the abandoned job has already been dequeued and has
    // had whatever chance the destination's policy gives it.
    let live = tokio::time::timeout(
        Duration::from_secs(10),
        lsp_request_on_lane(
            lane.sender(),
            Arc::clone(&bridge),
            "textDocument/didOpen".to_string(),
            did_open(live_uri),
        ),
    )
    .await
    .expect("the lane must keep serving after an abandoned request")
    .expect("the live didOpen must resolve to Ok");
    assert_eq!(
        live, "null",
        "`didOpen` is a notification, so it answers Null through the lane"
    );

    AbandonedOutcome {
        published: sink
            .take_calls()
            .iter()
            .map(|(uri, ..)| uri.as_str().to_owned())
            .collect(),
        abandoned_has_state: !bridge.get_diagnostics(abandoned_uri).await.is_empty(),
    }
}

/// (o) An ABANDONED request to a DISCARDING destination produces no server-side
/// effect and does not keep a lane consumer — the end-to-end counterpart of
/// `large_stack_tests`' (ah), through the REAL composition.
///
/// (ah) proves the mechanism with a synthetic sender and an `AtomicBool`. This
/// proves it against a real lane, a real `InProcessLsp` and a real notification
/// sink: the abandoned request never reaches the server, so it publishes no
/// diagnostics and leaves no document behind.
///
/// The shape is `abandoned_didopen_outcome` above — shared verbatim with (o2),
/// which asserts the opposite polarity on the same two fields. Only the lane's
/// declared policy differs, which is what makes the pair a contrast rather than
/// two similar-looking tests.
#[tokio::test]
async fn an_abandoned_request_does_not_occupy_a_lane_consumer() {
    use crate::large_stack::Lane;

    const LANE_NAME: &str = "t6517-canc";
    // `cancelling_pool`, i.e. `OnAbandon::Discard` — the policy the real
    // `LSP_POOL` carries. See the helper's doc for why a `didOpen` is
    // nonetheless the payload.
    static CANCEL_LANE: Lane = Lane::cancelling_pool(LANE_NAME, 1);

    const ABANDONED_URI: &str = "file:///abandoned.ri";
    const LIVE_URI: &str = "file:///still_live.ri";

    let outcome =
        abandoned_didopen_outcome(&CANCEL_LANE, LANE_NAME, ABANDONED_URI, LIVE_URI).await;

    assert!(
        outcome.published.iter().any(|uri| uri == LIVE_URI),
        "non-vacuity: the LIVE didOpen must have published diagnostics, or the \
         absence assertion below would hold for a sink that records nothing. \
         Recorded: {:?}",
        outcome.published
    );
    assert!(
        outcome.published.iter().all(|uri| uri != ABANDONED_URI),
        "the abandoned didOpen must never have reached `InProcessLsp`: it \
         published diagnostics, so the lane drove work whose awaiting side was \
         already gone. Recorded: {:?}",
        outcome.published
    );
    assert!(
        !outcome.abandoned_has_state,
        "the abandoned request must have left NO server-side state — no \
         document, and therefore no diagnostics, for its URI"
    );
}

/// (o2) An abandoned `textDocument/didOpen` on an ORDERED (`OnAbandon::Run`)
/// destination IS STILL APPLIED.
///
/// (o)'s inverse, and the end-to-end statement of why cancel-at-the-lane is a
/// property of the destination rather than a blanket rule. It runs the SAME
/// `abandoned_didopen_outcome` body as (o) — same parked consumer, same
/// manufactured abandonment, same sink — with the lane's declared policy as the
/// only difference, so a regression to the blanket `if reply_tx.is_closed()`
/// reds exactly here. Sharing the body is what makes "everything is identical
/// except the policy" literally true rather than true by resemblance.
///
/// # What the regression costs, which is why this is asserted end-to-end
///
/// Not a lost notification. A lost DOCUMENT. If the queued `didOpen` is
/// discarded, `InProcessLsp` never learns the URI exists; a subsequent
/// `didChange` then takes `ReifyLanguageServer::did_change`'s `didChange for
/// unknown URI` branch and silently applies nothing, and every query handler —
/// `hover`, `goto_definition`, `completion`, `document_symbol`,
/// `document_highlight`, `prepare_rename`, `rename`, `references` — returns
/// `Ok(None)` from its `documents.get(&uri)` miss arm. The pane
/// stays dark to hover, completion and diagnostics for the rest of the session,
/// with no error anywhere. Asserting the PUBLISH (a real server-side effect)
/// rather than "the lane recovered" is what distinguishes that outcome from a
/// healthy one — a lane that dropped the job would satisfy "recovered"
/// perfectly.
///
/// The second `didOpen` is the non-vacuity twin, exactly as in (o): without it
/// a sink that recorded everything twice would also pass.
#[tokio::test]
async fn an_abandoned_request_on_the_ordered_lane_is_still_applied() {
    use crate::large_stack::Lane;

    const LANE_NAME: &str = "t6517-ord";
    // `Lane::pool`, i.e. `OnAbandon::Run` — the policy `LSP_LANE` carries.
    static ORDERED_LANE: Lane = Lane::pool(LANE_NAME, 1);

    const ABANDONED_URI: &str = "file:///abandoned_ordered.ri";
    const LIVE_URI: &str = "file:///still_live_ordered.ri";

    let outcome =
        abandoned_didopen_outcome(&ORDERED_LANE, LANE_NAME, ABANDONED_URI, LIVE_URI).await;

    assert!(
        outcome.published.iter().any(|uri| uri == LIVE_URI),
        "non-vacuity: the LIVE didOpen must have published diagnostics. \
         Recorded: {:?}",
        outcome.published
    );
    assert!(
        outcome.published.iter().any(|uri| uri == ABANDONED_URI),
        "the ABANDONED didOpen must still have reached `InProcessLsp`. It did \
         not — so on the ordered lane a request whose caller stopped listening \
         now silently loses the document: `didChange` for that URI applies \
         nothing and every query answers None, for the rest of the session. \
         Recorded: {:?}",
        outcome.published
    );
    assert!(
        outcome.abandoned_has_state,
        "the abandoned request must have left its server-side state behind — a \
         document, and diagnostics for it"
    );
}

/// (p) The PRODUCTION query pool runs the consumers it DECLARES — the twin of
/// (k), for the lane whose consumer count is the whole point of task 6517.
///
/// (k) pins `LSP_LANE` at exactly one consumer, because a second one would let a
/// `didChange` overtake an earlier one. This pins the opposite direction, and
/// nothing else in either test file does: `large_stack_tests`' (aa)/(ab)/(ad)/
/// (ae) measure the mechanism against test-local `Lane::pool(..)` instances,
/// (l) above deliberately uses a test-local size-2 pool, and (i) only compares
/// `lane_for_method`'s returned POINTER with `LSP_POOL.sender()`. So before this
/// test, rebuilding the static as `Lane::new(LSP_POOL_THREAD_PREFIX)` — or
/// letting `LSP_POOL_SIZE` fall to 1 — left every other test in this binary
/// green while restoring total head-of-line blocking among LSP queries, which is
/// exactly the regression the task exists to prevent.
///
/// # Two assertions, because neither alone is enough
///
/// STRUCTURAL, via `Lane::size()`. `LSP_POOL.size() == LSP_POOL_SIZE` catches a
/// static rebuilt with `Lane::new` or with a stray literal; `LSP_POOL_SIZE == 4`
/// catches the constant itself being lowered. The second is a VALUE assertion
/// rather than a `>= 2` range check on purpose — `LSP_POOL_SIZE`'s own docs
/// justify a fixed constant by promising "the same number on every machine —
/// directly assertable from a test", and a range check would let 4 drift to 2
/// unremarked, which is precisely the silent narrowing this guards.
///
/// BEHAVIOURAL, via thread names. A declared size means nothing if `Lane::sender`
/// does not act on it, so a probe is dispatched through the real pool and the
/// thread it lands on is named. This is a sharp check rather than a soft one
/// because of `Lane`'s naming rule: a size-1 lane names its consumer EXACTLY
/// `name`, with no index (that is what keeps `reify-lsp-w` byte-identical across
/// the pool generalisation), so BOTH regressions above produce the bare
/// `reify-lsp-p` — which is not in the expected set and reds here.
///
/// # Why the observed set is NOT asserted to be the full set
///
/// That would be a scheduling bet dressed as a property. Consumers do rotate
/// across sequential submissions — the shared receiver lock is released before
/// each job body — but WHICH waiter wins the freed lock is the OS's choice, so
/// "all four names appear within N submissions" can fail on a loaded machine
/// with nothing broken. The alternative — PARKING four consumers to observe them
/// at once — is the starvation hazard (l) documents from the other side: any
/// concurrently-running test in this binary holding one pool consumer would make
/// the parked count fall short and RED a healthy pool. So the count is pinned
/// structurally and the naming behaviourally, and neither assertion depends on
/// the scheduler. This test never parks a consumer for longer than a
/// `ThreadId` read.
#[test]
fn the_query_pool_runs_the_consumers_it_declares() {
    use crate::large_stack::{LSP_LANE, LSP_POOL, LSP_POOL_SIZE, LSP_POOL_THREAD_PREFIX};
    use crate::tests::test_helpers::post_and_wait;
    use std::collections::HashSet;

    assert_eq!(
        LSP_POOL_SIZE, 4,
        "`LSP_POOL_SIZE` is a FIXED constant so the head-of-line bound is the \
         same number on every machine and in every bug report. Changing it is a \
         legitimate decision — but a deliberate one, which is what this line \
         makes it."
    );
    assert_eq!(
        LSP_POOL.size(),
        LSP_POOL_SIZE,
        "the query pool must be declared with `LSP_POOL_SIZE` consumers. A \
         static rebuilt as `Lane::new(LSP_POOL_THREAD_PREFIX)`, or with a \
         literal that drifted from the constant, serializes every LSP query \
         again while leaving every other test green."
    );
    assert_eq!(
        LSP_LANE.size(),
        1,
        "the ordered lane must stay single-consumer — the structural twin of \
         (k)'s behavioural check, and the invariant `didChange` ordering rests on"
    );

    let expected: HashSet<String> = (0..LSP_POOL_SIZE)
        .map(|i| format!("{LSP_POOL_THREAD_PREFIX}{i}"))
        .collect();
    let caller = std::thread::current().name().map(str::to_owned);

    let mut seen: HashSet<String> = HashSet::new();
    for _ in 0..32 {
        let landed_on = post_and_wait(LSP_POOL.sender(), || {
            std::thread::current().name().map(str::to_owned)
        })
        .expect(
            "a pool job must run on a NAMED lane thread; an unnamed thread means \
             the lane degraded and the job ran somewhere this test cannot vouch \
             for",
        );
        assert!(
            expected.contains(&landed_on),
            "a pool job landed on {landed_on:?}, which is not one of {expected:?}. \
             A size-1 lane names its consumer exactly \
             `{LSP_POOL_THREAD_PREFIX}` with no index, so that bare name here is \
             the signature of the pool having been collapsed to a single \
             consumer."
        );
        seen.insert(landed_on);
    }

    assert!(
        !seen.is_empty(),
        "non-vacuity: the loop must actually have dispatched, or every \
         assertion inside it held over nothing"
    );
    assert!(
        caller.is_none_or(|c| !seen.contains(&c)),
        "the pool must run its jobs on lane threads, never on the caller — a \
         job answering from the caller's own thread would make the naming \
         assertions meaningless. Saw {seen:?}"
    );

    // REALISED, via `Lane::started()`, and placed after the loop because the
    // count is meaningful only once `sender()` has run. The two assertions above
    // are both about what the pool DECLARES; `Lane::sender` warns and continues
    // on a partial spawn failure, so a pool that started 1 of 4 consumers passes
    // every one of them while serializing LSP queries again. This is the only
    // line in either file that can tell those apart.
    //
    // It stays a strict equality, and the diagnostic does the work instead.
    // Weakening it to `>= 1` would delete the whole point — the silent
    // narrowing is exactly a count between 1 and `LSP_POOL_SIZE`, so a liveness
    // check cannot see it. But the shortfall has TWO possible causes and they
    // belong to different subsystems: a code defect here, or an OS that refused
    // one of this binary's many concurrent 256 MiB mappings. This binary
    // declares a lot of them at once (`LSP_POOL`'s four, `LSP_LANE`,
    // `ENGINE_LANE`, plus the test-local lanes of every concurrently-running
    // `#[test]`), and verify runs under CPU/memory admission control alongside
    // other cargo waves, so the environment cause is real rather than
    // theoretical. The message names the discriminator so a red here is
    // triaged at the right subsystem rather than at this crate by default.
    let started = LSP_POOL.started();
    assert_eq!(
        started, LSP_POOL_SIZE,
        "the query pool started {started} of {LSP_POOL_SIZE} consumers. \
         TRIAGE THE ENVIRONMENT FIRST, and the discriminator is on stderr: \
         `Lane::sender` warns and continues on a partial spawn failure, \
         printing `failed to spawn {LSP_POOL_THREAD_PREFIX} lane consumer <i> \
         of {LSP_POOL_SIZE}` with the OS error whenever the mapping was \
         refused. A restrictive `RLIMIT_AS`, `vm.overcommit_memory=2`, a low \
         `vm.max_map_count` or a container memory cap all produce that, and \
         `Lane::sender` is written to survive it — so with such a warning \
         present this is an environment shortfall and NOT a code defect. With \
         NO such warning the shortfall IS a code defect: the advertised \
         head-of-line bound is not the one in force, and nothing else in this \
         binary can observe that. Either way the number above is the realised \
         bound, not the declared one."
    );
    // The anti-vacuity twin — that `started()` reports 0 for a lane nobody has
    // used — is `large_stack_tests`' (al), on a TEST-LOCAL pool. It cannot live
    // here: every `static` in this module is process-wide, and any of the ~dozen
    // concurrently-running tests in this binary may have created `LSP_LANE`
    // already, so "0 before use" is not a property this test can observe.
}

/// A [`NotificationSink`] that records the NAME of the thread each
/// `publish_diagnostics` call arrives on.
///
/// `reify_lsp::test_support::RecordingSink` records the CALL but not its thread,
/// and it lives in `crates/reify-lsp` — outside this task's scope — so the
/// thread observation is made with a local sink rather than by widening that
/// one. Being a sink, it is also the ONLY hook `reify-lsp` exposes to this
/// crate that can report where server-side work ran, which is why (q) below can
/// make the observation for a notification and not for a query.
#[derive(Default)]
struct ThreadNameSink {
    threads: std::sync::Mutex<Vec<Option<String>>>,
}

impl NotificationSink for ThreadNameSink {
    fn publish_diagnostics(
        &self,
        _uri: tower_lsp::lsp_types::Url,
        _diagnostics: Vec<tower_lsp::lsp_types::Diagnostic>,
        _version: Option<i32>,
    ) {
        self.threads
            .lock()
            .unwrap()
            .push(std::thread::current().name().map(str::to_owned));
    }

    fn log_message(&self, _line: reify_lsp::server::LogLine) {}
}

impl ThreadNameSink {
    /// Drain the recorded thread names, so a later assertion speaks only about
    /// the requests issued since the last drain.
    fn take(&self) -> Vec<Option<String>> {
        std::mem::take(&mut self.threads.lock().unwrap())
    }
}

/// (q) The PRODUCTION entry point genuinely reaches a lane thread — asserted
/// against `lsp_request_on_worker` itself, not against a lane helper beside it.
///
/// (d) makes the same claim, but since the routing change it makes it about
/// `crate::large_stack::run_on_lsp_worker`, which production no longer calls at
/// all: `lsp_request_on_worker` is now
/// `lsp_request_on_lane(lane_for_method(&method), ..)`. So (d) went vacuous with
/// respect to its own stated purpose — gutting `lsp_request_on_worker` to a
/// plain `lsp_request_impl(..).await` would leave (d), (h), (i), (l), (n) and
/// (o) all green, because each of those either drives the `lsp_request_on_lane`
/// seam directly, inspects `lane_for_method` in isolation, or only checks a
/// returned VALUE — and a value is exactly what a gutted wrapper still gets
/// right. This restores the property against the real entry point.
///
/// The observation is the sink: `did_open` calls `publish_diagnostics`
/// synchronously, on whatever thread is running the handler, so the recorded
/// name IS the thread `lsp_request_on_worker` put the work on. Broken source
/// guarantees a publish, so "no record" means "never ran" rather than "ran and
/// had nothing to say".
///
/// # What this covers for the QUERY pool, and how
///
/// Not by the same observation: no read-only query publishes anything, so the
/// sink is silent for all eight of them, and PARKING pool consumers to watch
/// them instead is the starvation hazard (l) and (p) document. It is covered by
/// COMPOSITION, which since task 6517 is one line — `lsp_request_on_worker` IS
/// `lsp_request_on_lane(lane_for_method(&method), ..)`, for every method at
/// once. This test proves that line reaches a lane rather than awaiting inline;
/// (i) proves `lane_for_method` returns `LSP_POOL`'s sender for each of the
/// eight queries; (p) proves those consumers are real, indexed pool threads.
/// The gutting this exists to catch removes the lane hop for EVERY method
/// simultaneously, so catching it on one is catching it.
#[tokio::test]
async fn the_production_entry_point_runs_its_work_on_a_lane_thread() {
    use crate::large_stack::LSP_WORKER_THREAD_NAME;
    use crate::lsp_bridge::lsp_request_on_worker;

    const URI: &str = "file:///production_entry_lane.ri";
    /// Broken source, so `didOpen` is GUARANTEED to publish error diagnostics.
    const BROKEN: &str = "structure {";

    assert_ne!(
        std::thread::current().name(),
        Some(LSP_WORKER_THREAD_NAME),
        "precondition: this test must not itself be running on the lane thread, \
         or the assertion below would hold for a wrapper that awaited inline"
    );

    let sink = Arc::new(ThreadNameSink::default());
    let bridge = Arc::new(LspBridge::with_sink(sink.clone()));

    lsp_request_on_worker(
        Arc::clone(&bridge),
        "initialize".to_string(),
        reify_test_support::MINIMAL_INIT_PARAMS_JSON.to_string(),
    )
    .await
    .expect("initialize");
    lsp_request_on_worker(
        Arc::clone(&bridge),
        "initialized".to_string(),
        "{}".to_string(),
    )
    .await
    .expect("initialized");
    // Discard any setup publishes, so the assertions speak only about the
    // `didOpen` below.
    let _ = sink.take();

    lsp_request_on_worker(
        Arc::clone(&bridge),
        "textDocument/didOpen".to_string(),
        json!({
            "textDocument": {
                "uri": URI,
                "languageId": "reify",
                "version": 1,
                "text": BROKEN
            }
        })
        .to_string(),
    )
    .await
    .expect("didOpen");

    let threads = sink.take();
    assert!(
        !threads.is_empty(),
        "non-vacuity: the `didOpen` must have published diagnostics, or the \
         thread assertion below would hold over an empty list"
    );
    assert!(
        threads
            .iter()
            .all(|t| t.as_deref() == Some(LSP_WORKER_THREAD_NAME)),
        "`lsp_request_on_worker` must run its work on the ordered LSP lane \
         thread `{LSP_WORKER_THREAD_NAME}`, not inline on the awaiting runtime \
         worker. Diagnostics were published from {threads:?} — which is what a \
         wrapper gutted to `lsp_request_impl(..).await` would report, while \
         still returning exactly the right value."
    );
}

/// (r) Each PRODUCTION lane declares the abandoned-job policy its work requires.
///
/// The structural twin of (o)/(o2), and the guard neither of them can be. Both
/// of those measure a TEST-LOCAL lane, because forcing an abandonment means
/// parking every consumer and doing that to a process-wide `static` would
/// starve whichever other test in this binary is using it. So before this test,
/// rebuilding `LSP_LANE` as `Lane::cancelling_pool(..)` — which would start
/// silently discarding queued `didOpen`s in the shipped GUI — left every test in
/// the binary green, exactly the way declaring `LSP_POOL` with `Lane::new` did
/// before (p) existed.
///
/// It is a three-line assertion over `Lane::on_abandon()` for that reason: the
/// property is a declaration, so a declaration is the honest thing to check.
/// What LICENSES each value is not: `LSP_POOL` may discard only because every
/// method `lane_for_method` routes there is read-only against server-side state,
/// which (i)/(j) pin from the other side.
#[test]
fn each_production_lane_declares_the_abandon_policy_its_work_requires() {
    use crate::large_stack::{ENGINE_LANE, LSP_LANE, LSP_POOL, OnAbandon};

    assert_eq!(
        LSP_LANE.on_abandon(),
        OnAbandon::Run,
        "the ORDERED LSP lane must run an abandoned job anyway. It carries \
         `didOpen`/`didChange`/`didClose`; discarding one unrun leaves \
         `InProcessLsp` without the document, after which `didChange` applies \
         nothing and every query answers None for that URI — permanently, and \
         with no error anywhere."
    );
    assert_eq!(
        ENGINE_LANE.on_abandon(),
        OnAbandon::Run,
        "the ENGINE lane must run an abandoned job anyway: it carries the \
         projection / incremental-re-eval commands, several of which mutate \
         session state. It is also fed only by the BLOCKING seam today, whose \
         submitter is parked in `recv()` and cannot be dropped — so this is the \
         declaration that keeps it correct if an async submitter is ever added."
    );
    assert_eq!(
        LSP_POOL.on_abandon(),
        OnAbandon::Discard,
        "the QUERY pool is the one destination that may discard — and the \
         anti-vacuity half of this test. Without it, a change that made \
         `OnAbandon::Run` universal would satisfy the two assertions above \
         while deleting cancel-at-the-lane outright."
    );
}
