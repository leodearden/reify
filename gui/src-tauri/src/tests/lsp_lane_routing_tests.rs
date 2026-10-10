//! How LSP requests are routed across the ordered lane (`LSP_LANE`) and the
//! query pool (`LSP_POOL`).

use std::sync::Arc;

use serde_json::json;

use crate::lsp_bridge::{LspBridge, lsp_request_impl};
use crate::tests::test_helpers::init_and_open;
use reify_lsp::server::NotificationSink;

// ── Task 6517: the ordered lane / query pool split ───────────────────────────
//
// Task 5772 put every `lsp_request` on one large-stack lane with one consumer,
// so every request serialized against every other. Task 6517 bounds that by
// routing LSP work over TWO lanes instead of one.
//
// The classification key is LSP PROTOCOL semantics — does this method mutate
// server-side document/session state? — for the reasons `lane_for_method`'s
// "The classification key is LSP PROTOCOL semantics" doc gives.
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
// consumer, which is what `didChange` correctness rests on, pinned by (p). The
// only ordering given up is query-vs-notification, which is precisely the
// pre-5772 behaviour on the multi-threaded tauri runtime and which `reify-lsp`'s
// own `RwLock`/`Mutex` already serialise for safety: a query can read older
// text — staleness, never corruption.

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
/// it needs and drops the guard (four of them then run blocking work on the
/// consumer that drives them). Reordering these against a notification can only
/// make one read older text.
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
/// does: `Editor.tsx` fires `didChange` from a debounced `setTimeout` that only
/// its rename and find-uses commands wait for, and CodeMirror issues
/// completion/hover/highlight from independent sources, so a real query CAN
/// overtake a real `didChange`. That
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
