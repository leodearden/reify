//! How LSP requests are routed across the ordered lane (`LSP_LANE`) and the
//! query pool (`LSP_POOL`).
//!
//! The six state-mutating and lifecycle methods, plus any unrecognised one,
//! travel the size-1 ordered lane; the eight read-only queries travel the
//! `LSP_POOL_SIZE`-consumer pool. The membership is authoritative in
//! `lane_for_method`'s `matches!` arm; the constants below are its test copies,
//! which (j) executes against the real dispatcher.

use std::sync::Arc;

use serde_json::json;

use crate::lsp_bridge::{LspBridge, lsp_request_impl};
use crate::tests::test_helpers::init_and_open;
use reify_lsp::server::NotificationSink;

/// The order-sensitive / lifecycle methods, which keep a single FIFO consumer.
/// Reordering two of them is corruption, not staleness: applying edit N+1 before
/// edit N yields text neither side ever had.
const ORDERED_METHODS: [&str; 6] = [
    "initialize",
    "initialized",
    "textDocument/didOpen",
    "textDocument/didChange",
    "textDocument/didClose",
    "shutdown",
];

/// The read-only query methods, which may run concurrently: each takes
/// `state.read().await`, clones what it needs and drops the guard. Reordering one
/// against a notification can only make it read older text.
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

/// Strings `InProcessLsp::handle_request` does NOT accept. (h) uses them to pin the
/// conservative default: an unclassified method must not acquire concurrency by not
/// being listed.
const UNRECOGNISED_METHODS: [&str; 2] = ["textDocument/notAThing", ""];

/// Every arm string `InProcessLsp::handle_request` accepts, transcribed by hand from
/// `crates/reify-lsp/src/bridge.rs`'s `match method`. (j) drives every entry through
/// the real dispatcher, so an arm renamed or removed in `reify-lsp` reds there. An
/// arm ADDED there is not caught until it is transcribed here.
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
/// Compared by POINTER identity: two distinct lanes both answer a probe off the
/// caller's thread, so only pointer equality says "the same queue". The
/// unrecognised rows are what tell the structural default (list the
/// concurrency-safe set, default to ordered) from the opposite spelling.
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

/// (i) The eight read-only query methods route to the QUERY POOL — a different
/// queue from the ordered lane, or (h) and (i) would both hold with nothing
/// split.
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

/// (j) The classification is TOTAL and DISJOINT over `HANDLE_REQUEST_ARMS`, and the
/// arm list is EXECUTED against the real dispatcher.
///
/// The set half relates three hand-maintained arrays, which proves them consistent
/// with each other and nothing more. So every `HANDLE_REQUEST_ARMS` entry is also
/// driven through [`lsp_request_impl`] and must NOT come back with `reify-lsp`'s
/// own `bridge::error_prefix::UNSUPPORTED_METHOD` (an `Ok`, or an `Err` from its
/// params parse, both mean the arm exists), while every `UNRECOGNISED_METHODS`
/// entry MUST. A removed or renamed arm therefore reds here; a NEW arm nobody
/// transcribed does not.
///
/// `"null"` is the params payload for every row, because the question is which ARM
/// was reached. Each row gets its own bridge, so `shutdown` cannot change a later
/// row's answer.
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

/// (l) END-TO-END: a real `textDocument/hover` completes while another consumer of
/// the same pool is OCCUPIED — the head-of-line-blocking property, through the real
/// `lsp_request_on_lane` composition.
///
/// The pool is TEST-LOCAL and size 2: park one consumer, leave exactly one free.
/// Parking a consumer of the process-wide `LSP_POOL` would starve
/// concurrently-running tests. Against a single-consumer lane this fails as a clean
/// `tokio::time::timeout` elapse, and the probe is released on every exit path.
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
/// Each request is awaited before the next is issued, as `lspClient.ts` drives a
/// single request. `didOpen` / `didChange` travel the ordered lane, and the `hover`
/// that reads the result travels the pool, on a different thread. The `didChange`
/// job has returned before the `hover` is submitted, so the pool cannot observe
/// pre-change text. This pins the AWAITING client only: the shipped editor's
/// debounced `didChange` CAN be overtaken, as disclosed on
/// [`crate::large_stack::Lane`].
///
/// The hover payload carries the parameter's DEFAULT VALUE, so "before" and "after"
/// are distinct observations: the new value is present AND the old one is gone.
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

/// (p) The PRODUCTION query pool runs the consumers it DECLARES, and the
/// ordered lane keeps exactly one.
///
/// STRUCTURAL: `LSP_POOL.size() == LSP_POOL_SIZE` catches a static rebuilt with
/// `Lane::new` or a stray literal; `LSP_LANE.size() == 1` is the invariant
/// `didChange` ordering rests on. (`LSP_POOL_SIZE >= 2` is a `const` assertion
/// beside the constant.)
///
/// BEHAVIOURAL: every probe dispatched through the real pool must land on an
/// indexed `{LSP_POOL_THREAD_PREFIX}{i}` thread. A size-1 lane names its
/// consumer exactly `name`, so a collapsed pool reports the bare prefix and reds
/// here. The observed set is NOT asserted to be the full set: which consumer
/// wins the freed receiver lock is the OS's choice, and parking all four would
/// starve concurrently-running tests.
///
/// REALISED: `LSP_POOL.started()` must equal `LSP_POOL_SIZE`, because
/// `Lane::sender` survives a partial spawn failure with a narrower pool.
#[test]
fn the_query_pool_runs_the_consumers_it_declares() {
    use crate::large_stack::{LSP_LANE, LSP_POOL, LSP_POOL_SIZE, LSP_POOL_THREAD_PREFIX};
    use crate::tests::test_helpers::post_and_wait;
    use std::collections::HashSet;

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
        "the ordered lane must stay single-consumer: notifications are \
         order-sensitive against each other, and a second consumer would let a \
         `didChange` overtake an earlier one"
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

    // After the loop: the count is meaningful only once `sender()` has run.
    // The 0-before-use half is `large_stack_tests`' (al), on a test-local pool,
    // because another test may already have created `LSP_POOL`.
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
}

/// A [`NotificationSink`] that records the NAME of the thread each
/// `publish_diagnostics` call arrives on — the one hook `reify-lsp` exposes that
/// can report where server-side work ran.
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

/// (q) The PRODUCTION entry point runs its work on a lane thread, not inline on
/// the awaiting runtime worker.
///
/// Asserted against `lsp_request_on_worker` itself: a wrapper gutted to
/// `lsp_request_impl(..).await` would still return the right values, so (h),
/// (i), (l) and (n) would all stay green. The observation is the sink:
/// `did_open` publishes diagnostics synchronously on whatever thread runs the
/// handler, and broken source guarantees a publish.
///
/// The query pool is covered by composition rather than observation, because no
/// query publishes anything: `lsp_request_on_worker` is
/// `lsp_request_on_lane(lane_for_method(&method), ..)` for every method, (i)
/// shows `lane_for_method` returns `LSP_POOL`'s sender for the eight queries,
/// and (p) shows those consumers are real pool threads.
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
