//! Unit tests for [`crate::large_stack`].
//!
//! ## Why the deep-recursion tests are safe (no "violent RED")
//!
//! The large-stack property is proven by [`deep_recurse`], which recurses
//! ~16 MiB deep — 8x a default 2 MiB stack. On a default-stack thread that would
//! SIGSEGV and abort the whole test binary, so the recursion is reached ONLY
//! through the large-stack helpers, and on the lanes only through
//! [`deep_recurse_if_on_thread`] / `deep_recurse_if_on_lane`, which check the
//! thread first: a degraded lane runs its work on a default-size stack, and must
//! yield a clean assertion failure rather than an overflow.

use crate::tests::test_helpers::{
    ANTI_WEDGE, DEEP_RECURSION_DEPTH, deep_recurse, deep_recurse_if_on_thread, post_and_wait,
};

/// Render a caught panic payload as a string, so a test can assert on the
/// ORIGINAL message rather than merely on "something panicked" — the latter is
/// also true when a helper substitutes a failure of its own.
///
/// `panic!("literal")` yields a `&'static str` payload while a formatted
/// `panic!("{x}")` yields a `String`, so both are tried.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic payload>".to_owned())
}

// ── spawn_on_large_stack: fire-and-forget variant (step-3/step-4) ────────────

/// (d) `spawn_on_large_stack` runs the fire-and-forget closure (which delivers
/// its side effect out-of-band via a channel, since the closure returns `()`),
/// and the returned `JoinHandle` joins cleanly.
#[test]
fn spawn_on_large_stack_runs_closure_and_handle_joins() {
    use crate::large_stack::spawn_on_large_stack;
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel::<u64>();
    let handle = spawn_on_large_stack(move || {
        // Fire-and-forget: communicate the result out-of-band via the channel.
        tx.send(42).expect("receiver must still be alive");
    })
    .expect("spawn_on_large_stack should create the thread");

    // The closure ran and delivered its side effect.
    let received = rx
        .recv()
        .expect("closure must send its value before exiting");
    assert_eq!(
        received, 42,
        "fire-and-forget closure must execute its side effect"
    );

    // The returned handle joins without panicking.
    handle.join().expect("large-stack thread must join cleanly");
}

/// (e) Deep recursion (~16 MiB) driven through the fire-and-forget
/// `spawn_on_large_stack` runs to completion (result observed via a channel).
/// The recursion runs ONLY on the helper's large-stack thread, so at RED
/// (helper absent) this is a compile error, never a SIGSEGV.
#[test]
fn spawn_on_large_stack_survives_deep_recursion_over_default_stack() {
    use crate::large_stack::spawn_on_large_stack;
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel::<u64>();
    let handle = spawn_on_large_stack(move || {
        let result = deep_recurse(DEEP_RECURSION_DEPTH);
        tx.send(result).expect("receiver must still be alive");
    })
    .expect("spawn_on_large_stack should create the thread");

    let result = rx
        .recv()
        .expect("deep-recursion closure must send its result");
    handle.join().expect("large-stack thread must join cleanly");

    assert_eq!(
        result,
        u64::from(DEEP_RECURSION_DEPTH) + 1,
        "deep recursion must run to completion on the fire-and-forget large stack"
    );
}

// ── Observability: named threads (review amendment) ──────────────────────────

/// (f) The per-call helper NAMES its thread, so a panic backtrace,
/// `RUST_BACKTRACE` dump, `top -H` row or debugger thread list identifies engine
/// work instead of reading `<unnamed>`.
///
/// This matters precisely because this module RELOCATES the work most likely to
/// crash (stack overflow, OCCT kernel failure) off the caller's thread, which
/// would otherwise have carried a meaningful name. The lanes' names are pinned
/// by their own tests.
#[test]
fn the_per_call_thread_is_named_for_observability() {
    use crate::large_stack::{ENGINE_THREAD_NAME, spawn_on_large_stack};
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel::<Option<String>>();
    let handle = spawn_on_large_stack(move || {
        let _ = tx.send(std::thread::current().name().map(str::to_owned));
    })
    .expect("spawn_on_large_stack should create the thread");
    let spawn_name = rx.recv().expect("closure must report its thread name");
    handle.join().expect("large-stack thread must join cleanly");
    assert_eq!(
        spawn_name.as_deref(),
        Some(ENGINE_THREAD_NAME),
        "spawn_on_large_stack's thread must be named for panic backtraces / profilers"
    );
}

// ── Fire-and-forget ENGINE-lane submission (task 7442) ───────────────────────
//
// `post_to_worker` queues a job without waiting for it, so an async command
// never parks a thread on engine work. Nobody waits on a posted job, so these
// tests synchronise through channels the jobs report on.

/// The `ThreadId` and name of the calling thread.
fn this_thread() -> (std::thread::ThreadId, Option<String>) {
    let thread = std::thread::current();
    (thread.id(), thread.name().map(str::to_owned))
}

/// Post a probe to the ENGINE lane and return the thread it ran on.
fn engine_lane_thread() -> std::thread::ThreadId {
    let (tx, rx) = std::sync::mpsc::channel();
    crate::large_stack::post_to_worker(move || {
        let _ = tx.send(std::thread::current().id());
    })
    .expect("posting to the ENGINE lane must succeed");
    rx.recv_timeout(ANTI_WEDGE)
        .expect("a probe posted to the ENGINE lane must run")
}

/// The job runs on the persistent ENGINE lane, and `post_to_worker` returns
/// while the job is still blocked — the job only proceeds once the test has
/// seen `post_to_worker` return.
#[test]
fn post_to_worker_runs_on_the_engine_lane_and_returns_before_the_job_finishes() {
    use crate::large_stack::{WORKER_THREAD_NAME, post_to_worker};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let caller = std::thread::current().id();
    let post_returned = Arc::new(AtomicBool::new(false));
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let (report_tx, report_rx) = std::sync::mpsc::channel();

    let seen_by_job = Arc::clone(&post_returned);
    post_to_worker(move || {
        let released = release_rx.recv_timeout(ANTI_WEDGE).is_ok();
        let returned_first = released && seen_by_job.load(Ordering::SeqCst);
        let _ = report_tx.send((this_thread(), returned_first));
    })
    .expect("posting to the ENGINE lane must succeed");
    post_returned.store(true, Ordering::SeqCst);
    let _ = release_tx.send(());

    let ((ran_on, name), returned_first) = report_rx
        .recv_timeout(ANTI_WEDGE)
        .expect("the posted job must run and report");
    assert_ne!(ran_on, caller, "the job must not run inline on the caller");
    assert_eq!(
        name.as_deref(),
        Some(WORKER_THREAD_NAME),
        "the job must run on the persistent ENGINE lane"
    );
    assert!(
        returned_first,
        "post_to_worker must return before its job finishes, not wait for it"
    );
}

/// A panicking posted job is contained on the lane: the SAME lane thread keeps
/// serving (`ThreadId`s are never reused, so equality proves survival).
#[test]
fn the_engine_lane_survives_a_panicking_posted_job() {
    use crate::large_stack::post_to_worker;

    let before = engine_lane_thread();
    post_to_worker(|| panic!("posted boom")).expect("posting a panicking job must succeed");
    let after = engine_lane_thread();

    assert_ne!(
        before,
        std::thread::current().id(),
        "the probe must run on the lane, not inline on the caller"
    );
    assert_eq!(
        after, before,
        "the SAME lane thread must survive a panicking posted job"
    );
}

/// A posted job gets the lane's large stack.
#[test]
fn post_to_worker_survives_deep_recursion_over_default_stack() {
    use crate::large_stack::{WORKER_THREAD_NAME, post_to_worker};

    let (tx, rx) = std::sync::mpsc::channel();
    post_to_worker(move || {
        let _ = tx.send(deep_recurse_if_on_thread(
            WORKER_THREAD_NAME,
            DEEP_RECURSION_DEPTH,
        ));
    })
    .expect("posting to the ENGINE lane must succeed");

    let depth_reached = rx
        .recv_timeout(ANTI_WEDGE)
        .expect("the posted job must run and report")
        .unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(
        depth_reached,
        u64::from(DEEP_RECURSION_DEPTH) + 1,
        "deep recursion must run to completion on the ENGINE lane's large stack"
    );
}

/// With no lane (the OS refused its mapping) the job still runs on a spawned
/// thread — never inline, because a poster may be a tokio worker, where OCCT's
/// synchronous kernel handle panics.
#[test]
fn post_without_a_lane_runs_the_job_on_a_spawned_engine_thread() {
    use crate::large_stack::{ENGINE_THREAD_NAME, post};

    let (tx, rx) = std::sync::mpsc::channel();
    post(
        None,
        Box::new(move || {
            let _ = tx.send(this_thread());
        }),
    )
    .expect("the no-lane arm must still accept the job");

    let (ran_on, name) = rx.recv_timeout(ANTI_WEDGE).expect("the job must run");
    assert_ne!(
        ran_on,
        std::thread::current().id(),
        "the job must never run inline on the poster"
    );
    assert_eq!(name.as_deref(), Some(ENGINE_THREAD_NAME));
}

/// A job handed back by a dead lane still runs, on a spawned engine thread
/// rather than inline on the poster.
#[test]
fn post_hands_a_job_refused_by_a_dead_lane_to_a_spawned_engine_thread() {
    use crate::large_stack::{ENGINE_THREAD_NAME, JobSender, post};

    let (lane_tx, lane_rx) = std::sync::mpsc::channel();
    drop(lane_rx);
    let dead = JobSender::new("dead-lane", lane_tx);

    let (tx, rx) = std::sync::mpsc::channel();
    post(
        Some(&dead),
        Box::new(move || {
            let _ = tx.send(this_thread());
        }),
    )
    .expect("a job refused by a dead lane must be recovered, not dropped");

    let (ran_on, name) = rx.recv_timeout(ANTI_WEDGE).expect("the job must run");
    assert_ne!(
        ran_on,
        std::thread::current().id(),
        "the recovered job must never run inline on the poster"
    );
    assert_eq!(name.as_deref(), Some(ENGINE_THREAD_NAME));
}

/// A job running ON the ENGINE lane may post to that same lane: posting never
/// waits, so — unlike the waiting seam pinned by
/// `submitting_to_your_own_lane_from_a_future_panics_loudly_instead_of_wedging_it`
/// — it cannot wedge the single consumer. The inner job runs after the outer one
/// returns, on the same thread.
#[test]
fn a_job_on_the_engine_lane_may_post_to_its_own_lane() {
    use crate::large_stack::post_to_worker;

    let (tx, rx) = std::sync::mpsc::channel();
    let inner_tx = tx.clone();
    post_to_worker(move || {
        let posted = post_to_worker(move || {
            let _ = inner_tx.send(("inner", std::thread::current().id(), true));
        });
        let _ = tx.send(("outer", std::thread::current().id(), posted.is_ok()));
    })
    .expect("posting to the ENGINE lane must succeed");

    let (first, outer_thread, posted_ok) =
        rx.recv_timeout(ANTI_WEDGE).expect("the outer job must run");
    let (second, inner_thread, _) = rx
        .recv_timeout(ANTI_WEDGE)
        .expect("the inner job must run — a wedged lane never reaches it");

    assert!(posted_ok, "posting from the lane to itself must succeed");
    assert_eq!(
        (first, second),
        ("outer", "inner"),
        "the inner job must be queued behind the outer one, not run inline"
    );
    assert_ne!(outer_thread, std::thread::current().id());
    assert_eq!(
        inner_thread, outer_thread,
        "the inner job must run on the same ENGINE lane thread"
    );
}

// ── Named LANES: one mechanism, several instances (task 5772) ───────────────
//
// A second lane must be a second INSTANCE of the lane mechanism, not a second
// design, and must inherit every property the engine lane proves: large stack,
// panic isolation, per-lane amortisation. One thread for all large-stack work
// would instead make a hover queue behind an in-flight geometry evaluation.
// Concurrency WITHIN a lane belongs to the "Bounded intra-lane concurrency"
// section below.

/// (q) Every large-stack thread name is DISTINCT, so a backtrace, `top -H` row or
/// profiler capture says which TIER and which LANE the work is on.
///
/// The 15-byte `pthread_setname_np` budget is proven at compile time by the
/// `const` assertion beside each constant in `large_stack.rs`.
///
/// The pool prefix is checked as the names it REALISES, `{prefix}{i}`: no realised
/// pool name may equal or prefix another tier's name, or a `top -H` filter keyed on
/// one would match the other's rows. Prefix-freedom is not asserted among the
/// whole-name tiers: `reify-engine` already prefixes `reify-engine-w`, and both are
/// selected by exact match.
#[test]
fn large_stack_thread_names_are_pairwise_distinct() {
    use crate::large_stack::{
        ENGINE_THREAD_NAME, LSP_POOL_SIZE, LSP_POOL_THREAD_PREFIX, LSP_WORKER_THREAD_NAME,
        WORKER_THREAD_NAME,
    };

    let declared = [
        (ENGINE_THREAD_NAME, "the per-call engine thread"),
        (WORKER_THREAD_NAME, "the persistent ENGINE lane"),
        (LSP_WORKER_THREAD_NAME, "the persistent LSP lane"),
        (LSP_POOL_THREAD_PREFIX, "the LSP query pool's consumer prefix"),
    ];

    for (i, (name, what)) in declared.iter().enumerate() {
        for (other, other_what) in &declared[i + 1..] {
            assert_ne!(
                name, other,
                "{what} and {other_what} must be distinguishable in a backtrace \
                 or profiler row"
            );
        }
    }

    // The names `Lane::sender` will actually give the pool's consumers — the
    // strings a capture shows, which is what the prefix half is about.
    let realised: Vec<String> = (0..LSP_POOL_SIZE)
        .map(|index| format!("{LSP_POOL_THREAD_PREFIX}{index}"))
        .collect();
    for consumer in &realised {
        for (other, other_what) in &declared[..declared.len() - 1] {
            assert!(
                !consumer.starts_with(other) && !other.starts_with(consumer.as_str()),
                "the realised query-pool consumer {consumer:?} and {other_what} \
                 ({other:?}) must be distinguishable: neither may equal or PREFIX \
                 the other, or a `top -H` filter or profiler alert keyed on one \
                 would match the other's rows"
            );
        }
    }
}

/// (r) The two lanes are genuinely SEPARATE threads, while each lane amortises
/// its own single thread across submissions.
///
/// Both halves matter and neither implies the other. "Different threads" is the
/// no-head-of-line-blocking property that justified splitting the lanes at all;
/// "same thread within a lane" is the amortisation property that makes it a lane
/// rather than a per-call spawn. `ThreadId`s are never reused within a process,
/// so equality proves reuse and inequality proves a distinct thread.
#[tokio::test]
async fn the_two_lanes_are_separate_threads_each_amortised() {
    use crate::large_stack::{LSP_LANE, dispatch_async};

    let caller_id = std::thread::current().id();

    let engine_a = engine_lane_thread();
    let engine_b = engine_lane_thread();
    let lsp_a = dispatch_async(LSP_LANE.sender(), async { std::thread::current().id() }).await;
    let lsp_b = dispatch_async(LSP_LANE.sender(), async { std::thread::current().id() }).await;

    // Non-vacuity: a degraded lane reports the CALLER's id, which would make the
    // "same thread within a lane" assertions trivially true and the "different
    // lanes" assertion trivially false. Rule that out before reading either.
    assert_ne!(
        engine_a, caller_id,
        "the engine lane must not have degraded to an inline call"
    );
    assert_ne!(
        lsp_a, caller_id,
        "the LSP lane must not have degraded to an inline call"
    );

    assert_eq!(
        engine_a, engine_b,
        "consecutive engine-lane jobs must share ONE persistent thread"
    );
    assert_eq!(
        lsp_a, lsp_b,
        "consecutive LSP-lane jobs must share ONE persistent thread"
    );
    assert_ne!(
        engine_a, lsp_a,
        "the lanes must be separate threads — sharing one would make a hover \
         queue behind an in-flight geometry evaluation, which is the regression \
         the lane split exists to prevent"
    );
}

/// (r3) The reentrancy guard is PER-LANE, not blanket: a job running on the
/// ENGINE lane may submit to the LSP lane and wait for it, because that lands on
/// a different thread with its own consumer.
///
/// This is the other half of (r4), and it is what makes the guard a correctness
/// check rather than a blunt "no submitting from a lane thread" rule that would
/// reject a legal composition. Asserting the inner job's thread — rather than
/// just that it returned — is what pins that it genuinely crossed lanes instead
/// of quietly degrading to an inline await on the outer lane's thread.
///
/// The ENGINE job drives the submission with a runtime of its own: a lane
/// thread has no ambient runtime, and without one `dispatch_async` awaits inline
/// and never reaches the guard. If the guard wrongly rejected the submission,
/// its panic would be contained by `post`, the report channel would disconnect,
/// and the `recv_timeout` below would fail at once rather than hang.
#[test]
fn a_job_on_one_lane_may_submit_to_the_other_lane() {
    use crate::large_stack::{
        LSP_LANE, LSP_WORKER_THREAD_NAME, WORKER_THREAD_NAME, dispatch_async, post_to_worker,
    };

    let (tx, rx) = std::sync::mpsc::channel();
    post_to_worker(move || {
        let outer = this_thread();
        let inner = tokio::runtime::Builder::new_current_thread()
            .build()
            .map(|runtime| {
                runtime.block_on(dispatch_async(LSP_LANE.sender(), async { this_thread() }))
            });
        let _ = tx.send((outer, inner));
    })
    .expect("posting to the ENGINE lane must succeed");

    let ((outer, outer_name), inner) = rx
        .recv_timeout(ANTI_WEDGE)
        .expect("the ENGINE job must run and report");
    let (inner, inner_name) = inner.expect("the ENGINE job must build its runtime");
    assert_eq!(outer_name.as_deref(), Some(WORKER_THREAD_NAME));
    assert_eq!(
        inner_name.as_deref(),
        Some(LSP_WORKER_THREAD_NAME),
        "a cross-lane submission must run on the OTHER lane's thread — the guard \
         must not reject it, and it must not degrade to an inline await"
    );
    assert_ne!(outer, inner);
}

/// (t) The LSP lane is panic-isolated: a panicking job re-raises its ORIGINAL
/// payload on its awaiter, and the SAME lane thread keeps serving.
///
/// A poisoned LSP job that killed the lane would silently downgrade every
/// FUTURE keystroke to inline execution on a ~2 MiB tokio stack — the exact
/// hazard the module exists to remove. `ThreadId`s are never reused, so an
/// equal pair across the panic proves the same thread survived.
#[tokio::test]
async fn lsp_lane_is_panic_isolated_and_survives() {
    use crate::large_stack::{LSP_LANE, dispatch_async};

    let caller_id = std::thread::current().id();
    let before = dispatch_async(LSP_LANE.sender(), async { std::thread::current().id() }).await;
    assert_ne!(
        before, caller_id,
        "the pre-panic job must run on the lane, not inline on the caller"
    );

    let poisoned = tokio::spawn(dispatch_async::<_, ()>(LSP_LANE.sender(), async {
        panic!("lsp boom")
    }))
    .await;
    let payload = poisoned
        .expect_err("a panicking LSP-lane job must reach its awaiter")
        .into_panic();
    assert_eq!(
        panic_message(&*payload),
        "lsp boom",
        "the awaiter must receive the JOB's original payload, not a substitute"
    );

    let (value, after) = dispatch_async(LSP_LANE.sender(), async {
        (5u32, std::thread::current().id())
    })
    .await;
    assert_eq!(
        value, 5,
        "the LSP lane must keep answering submissions after a poisoned job"
    );
    assert_eq!(
        after, before,
        "the SAME LSP lane thread must survive the panic"
    );
}

// ── ASYNC lane submission (task 5772) ────────────────────────────────────────
//
// `lsp_request` is an `async fn` on the tauri tokio runtime, so waiting on its
// lane job would pin a runtime worker for a whole LSP round trip on EVERY
// keystroke — precisely what an async command must not do.
//
// So the LSP lane's submission is async: box the job, reply over a
// `tokio::sync::oneshot`, and `.await` it, releasing the tokio worker while the
// lane thread computes. Not a new pattern — `debug_server::run_on_engine`
// already bridges async-caller-to-large-stack-thread with exactly
// `spawn_on_large_stack` + `oneshot`; this amortises it onto a persistent lane.
//
// These tests pin the properties that must hold: the value comes back, the
// runtime is NOT blocked, panics stay faithful, and the degraded arms still
// return rather than hanging an `.await`.

/// (u) The async submission returns the closure's value, and the closure body
/// runs on the LSP lane's thread — not on a tokio worker, and not inline.
#[tokio::test]
async fn lsp_lane_dispatch_returns_value_and_runs_on_the_lane() {
    use crate::large_stack::{LSP_LANE, LSP_WORKER_THREAD_NAME, dispatch_async};

    let caller_id = std::thread::current().id();
    // Owned and MOVED into the job, because the lane outlives this frame — a
    // heap-owned `Vec` rather than a `Copy` array, which the job would merely
    // copy.
    let data = Vec::from([1u64, 2, 3, 4, 5]);

    let (sum, inner_id, inner_name) = dispatch_async(LSP_LANE.sender(), async move {
        let s: u64 = data.iter().sum();
        (
            s,
            std::thread::current().id(),
            std::thread::current().name().map(str::to_owned),
        )
    })
    .await;

    assert_eq!(
        sum, 15,
        "the async submission must propagate the closure's value back to the awaiter"
    );
    assert_ne!(
        inner_id, caller_id,
        "the job must run on the lane, not inline on the awaiting runtime worker"
    );
    assert_eq!(
        inner_name.as_deref(),
        Some(LSP_WORKER_THREAD_NAME),
        "the async submission must run on the LSP lane's own named thread"
    );
}

/// (v) Awaiting the submission does NOT block the calling runtime — the whole
/// reason this variant exists.
///
/// `#[tokio::test]` builds a CURRENT-THREAD runtime, which makes this sharp: a
/// `tokio::spawn`ed task only runs when the single thread is free to poll it. So
/// the assertion is an ORDERING one, not a timing one, and cannot pass by luck.
///
/// * A BLOCKING impl (parking in `mpsc::recv()`) makes
///   the first poll return `Ready` only after the whole 300 ms job, with the one
///   thread parked throughout. The spawned task cannot have been polled yet, so
///   the flag reads `false` the instant the await resolves. RED.
/// * A NON-BLOCKING impl returns `Pending` immediately, the runtime polls the
///   spawned task (which completes at once), and 300 ms later the oneshot fires.
///   The flag therefore reads `true`. GREEN.
#[tokio::test]
async fn lane_dispatch_does_not_block_the_calling_runtime() {
    use crate::large_stack::{LSP_LANE, dispatch_async};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let concurrent_ran = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&concurrent_ran);

    // Spawned BEFORE the lane submission, and immediately ready — so the only
    // thing that can keep it from running is the runtime thread being blocked.
    let concurrent = tokio::spawn(async move {
        flag.store(true, Ordering::SeqCst);
        "concurrent task done"
    });

    // Long enough that "did the runtime get to poll anything else?" is not a
    // close call either way.
    let lane_result = dispatch_async(LSP_LANE.sender(), async {
        std::thread::sleep(std::time::Duration::from_millis(300));
        "lane job done"
    })
    .await;

    // Read the flag at THIS instant — before awaiting the handle, which would
    // give a blocked runtime a second chance to run the task and hide the bug.
    let ran_during_the_job = concurrent_ran.load(Ordering::SeqCst);

    assert_eq!(
        lane_result, "lane job done",
        "the lane job must still deliver its value"
    );
    assert_eq!(
        concurrent.await.expect("the concurrent task must not panic"),
        "concurrent task done",
        "the concurrent task must complete"
    );
    assert!(
        ran_during_the_job,
        "a concurrently-spawned task had still not been polled when the lane job \
         finished — awaiting the lane BLOCKED the runtime thread, which is \
         exactly the failure this async variant exists to prevent"
    );
}

/// (w) Panic fidelity survives the shape change: a panicking job surfaces on the
/// AWAITING submitter carrying its ORIGINAL payload.
///
/// The payload assertion is load-bearing — a lane that merely died, or a oneshot
/// that merely disconnected, would also produce "something panicked", just with
/// a substituted message.
///
/// `tokio::spawn` is the unwind boundary: it catches a task panic and hands back
/// the payload through [`tokio::task::JoinError::into_panic`], so no
/// `futures::FutureExt::catch_unwind` (and no new dependency) is needed.
#[tokio::test]
async fn lane_dispatch_propagates_the_original_job_panic() {
    use crate::large_stack::{LSP_LANE, dispatch_async};

    let joined = tokio::spawn(async {
        // Concrete `T = ()` so inference is unambiguous; the closure never
        // returns normally, but the panic must still cross the lane AND the
        // oneshot to reach the awaiting task.
        dispatch_async::<_, ()>(LSP_LANE.sender(), async { panic!("async boom") }).await;
    })
    .await;

    let err = joined.expect_err("a panicking job must surface as a panicked task");
    assert!(
        err.is_panic(),
        "the task must have PANICKED, not been cancelled: {err:?}"
    );
    let payload = err.into_panic();
    assert_eq!(
        panic_message(&*payload),
        "async boom",
        "the awaiter must receive the JOB's original payload, not a substitute"
    );
}

/// (x) The DEGRADED arm of the async path: with no lane, the future is awaited
/// natively on the caller and the `.await` still RESOLVES.
///
/// The body is trivial, so this shows only that the ARM resolves, not that real
/// work does. The production claim is
/// `lsp_bridge_tests::lsp_request_on_lane_without_a_lane_still_resolves_to_the_right_value`,
/// which drives the same arm through the real composition.
#[tokio::test]
async fn async_dispatch_without_a_lane_runs_inline_and_still_resolves() {
    use crate::large_stack::dispatch_async;

    let caller_id = std::thread::current().id();

    let (value, ran_on) =
        dispatch_async(None, async { (77u32, std::thread::current().id()) }).await;

    assert_eq!(
        value, 77,
        "the degraded async arm must still return the future's output — never a \
         lost result, and never a hung await"
    );
    assert_eq!(
        ran_on, caller_id,
        "with no lane the future must be polled INLINE in the caller's own frame"
    );
}

/// (y) The ASYNC path onto the LSP lane carries the LARGE STACK too — the whole
/// point of routing `lsp_request` there.
///
/// (u) proved the async submission lands on the lane; this proves the lane it
/// lands on is the 256 MiB one, through the async entry point specifically. An
/// impl that built its lane without
/// [`crate::large_stack::COMPILE_STACK_SIZE`] fails here: ~8 KiB/frame x 2048 is
/// ~16 MiB, 8x the ~2 MiB default a tokio worker would have given this work.
///
/// Per the module's "no violent RED" doctrine the recursion is reached ONLY
/// through [`deep_recurse_if_on_thread`], so a degraded lane yields a clean
/// assertion failure instead of overflowing and SIGABRTing the whole binary.
#[tokio::test]
async fn lsp_lane_dispatch_survives_deep_recursion_over_default_stack() {
    use crate::large_stack::{LSP_LANE, LSP_WORKER_THREAD_NAME, dispatch_async};

    let result = dispatch_async(LSP_LANE.sender(), async {
        deep_recurse_if_on_thread(LSP_WORKER_THREAD_NAME, DEEP_RECURSION_DEPTH)
    })
    .await;

    let depth_reached = result.unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(
        depth_reached,
        u64::from(DEEP_RECURSION_DEPTH) + 1,
        "deep recursion must run to completion on the LSP lane's large stack, \
         reached through the ASYNC entry point `lsp_request` uses"
    );
}

/// (z) The `SendError` recovery arm: a job handed back by a dead queue still runs
/// and delivers its value, and NOT in the submitting async frame.
///
/// The handed-back job carries a `Handle::block_on`, which panics "Cannot start a
/// runtime from within a runtime" inside the tauri runtime, so the arm must hand
/// it to a thread outside any runtime context. A SYNTHETIC sender whose consumer is
/// already gone provokes the arm deterministically.
#[tokio::test]
async fn async_dispatch_recovers_a_handed_back_job_off_the_submitting_frame() {
    use crate::large_stack::{JobSender, dispatch_async};

    // Consumer dropped before any send: every `send` fails at once with
    // `SendError(job)`, which is the arm under test.
    let (tx, rx) = std::sync::mpsc::channel();
    drop(rx);
    let dead = JobSender::new("test-dead-async", tx);

    let caller_id = std::thread::current().id();

    let (value, ran_on) = dispatch_async(Some(&dead), async move {
        (4242u32, std::thread::current().id())
    })
    .await;

    assert_eq!(
        value, 4242,
        "a job handed back by a dead queue must still be run and its value \
         delivered — degraded, never lost"
    );
    assert_ne!(
        ran_on, caller_id,
        "the handed-back job carries a `Handle::block_on`, so it must NOT be \
         run in the submitting async frame (which is inside the runtime) — that \
         panics with the nested-runtime message instead of resolving"
    );
}

/// (r4) A FUTURE that submits to the lane it is being DRIVEN on — and would wait
/// for it — gets a loud panic naming that lane, and the lane survives.
///
/// Without the guard the lane's single consumer could run the inner job only after
/// the outer one returned, while the outer one waits for it: the lane and every
/// later submitter in the process would hang. The panic must escape the inner
/// future mid-poll, unwind out of [`tokio::runtime::Handle::block_on`], be caught by
/// the outer job's `catch_unwind`, ride back over the `oneshot`, and be
/// `resume_unwind`-ed on the awaiting submitter; `tokio::spawn` is the unwind
/// boundary, as in (w).
///
/// The guard is reached because the outer job is driven by `handle.block_on`, so
/// `try_current()` inside the inner submission succeeds. (A lane thread with no
/// ambient runtime awaits inline and enqueues nothing, which cannot wedge.)
///
/// This test cannot honour the "no violent RED" doctrine: without the guard it
/// hangs rather than failing, because the wedge is of a process-wide `static`
/// lane.
#[tokio::test]
async fn submitting_to_your_own_lane_from_a_future_panics_loudly_instead_of_wedging_it() {
    use crate::large_stack::{LSP_LANE, LSP_WORKER_THREAD_NAME, dispatch_async};

    let joined = tokio::spawn(async {
        // The OUTER future is driven ON the LSP lane; the inner submission
        // targets that same lane, which is the wedge.
        dispatch_async(LSP_LANE.sender(), async {
            dispatch_async(LSP_LANE.sender(), async { 1u32 }).await
        })
        .await
    })
    .await;

    let err = joined.expect_err(
        "re-entrant async submission must panic on the awaiting submitter rather \
         than wedge the lane",
    );
    assert!(
        err.is_panic(),
        "the awaiting task must have PANICKED, not been cancelled: {err:?}"
    );
    let message = panic_message(&*err.into_panic());
    assert!(
        message.contains("re-entrant submission"),
        "the panic must name the reentrancy rather than surface as a generic \
         channel or oneshot error, got: {message}"
    );
    assert!(
        message.contains(LSP_WORKER_THREAD_NAME),
        "the panic must name the LANE that was re-entered, got: {message}"
    );

    // Survival, and on the SAME lane thread: a lane killed by the rejected
    // submission would hang here, and one that had silently degraded to an
    // inline await would answer from the caller's thread instead. Asserting the
    // thread NAME rather than just the value is what separates those two.
    let (value, ran_on) = dispatch_async(LSP_LANE.sender(), async {
        (7u32, std::thread::current().name().map(str::to_owned))
    })
    .await;
    assert_eq!(
        value, 7,
        "the lane must survive a rejected re-entrant submission and keep serving \
         every other caller in the process"
    );
    assert_eq!(
        ran_on.as_deref(),
        Some(LSP_WORKER_THREAD_NAME),
        "the surviving lane must still be the LANE — a degraded inline await \
         would answer from the awaiting runtime worker instead"
    );
}
// ── Bounded intra-lane concurrency (task 6517) ───────────────────────────────
//
// A lane of N consumers bounds head-of-line blocking at N. Every test here
// declares its OWN `static POOL: Lane = Lane::pool(..)` inside its fn body: the
// production lanes are process-wide statics shared by concurrently-running
// tests, and parking one of their consumers would starve another test and hang
// the suite. Concurrency is measured with an arrival counter under a `Condvar`
// and a wall-clock deadline, so a lane that still serializes fails a clean
// assertion rather than hanging.

/// Recurse ~16 MiB ONLY if this thread is a consumer of the pool named by `prefix`
/// (a `{prefix}<digits>` thread); otherwise report where it is, without
/// recursing. The pool sibling of [`deep_recurse_if_on_thread`], which matches one
/// exact name.
fn deep_recurse_if_on_lane(prefix: &'static str, depth: u32) -> Result<u64, String> {
    let actual = std::thread::current().name().map(str::to_owned);
    let on_pool_consumer = actual.as_deref().is_some_and(|name| {
        name.strip_prefix(prefix)
            .is_some_and(|index| !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()))
    });
    if !on_pool_consumer {
        return Err(format!(
            "refusing to recurse ~16 MiB on thread {actual:?}: expected a \
             consumer of the {prefix:?} pool lane (a `{prefix}<index>` thread). \
             The lane degraded to an inline call, so recursing here would \
             overflow a default-size stack and abort the entire test binary."
        ));
    }
    Ok(deep_recurse(depth))
}

/// Post `n` jobs to `lane`, each of which records its arrival and then parks on a
/// `Condvar` until all `n` have arrived or a deadline elapses. Returns, per job,
/// whether it saw all `n` in flight AT ONCE.
///
/// On a single-consumer lane job 1 holds the only consumer, the rest never start,
/// and job 1 reports `false` at the deadline. The wait parks rather than spins, so
/// it cannot starve the siblings it waits for.
///
/// It first asserts that the lane started: with no lane, `post` runs every job on
/// its own spawned thread, so all `n` would see each other with zero consumers.
fn observe_concurrent_arrivals(lane: &'static crate::large_stack::Lane, n: usize) -> Vec<bool> {
    use crate::large_stack::post;
    use std::sync::{Arc, Condvar, Mutex};

    /// A liveness backstop, not the property under test. Well under [`ANTI_WEDGE`],
    /// because a serialized lane releases its verdicts one deadline apart and each
    /// `recv_timeout(ANTI_WEDGE)` below must outlast that gap.
    const ARRIVAL_DEADLINE: std::time::Duration =
        std::time::Duration::from_secs(ANTI_WEDGE.as_secs() / 4);

    assert!(
        lane.sender().is_some(),
        "precondition: the pool must have started its consumers. With no lane \
         every job runs on its own spawned thread, all {n} observe each other \
         regardless, and the measurement below is vacuous."
    );

    let arrivals = Arc::new((Mutex::new(0usize), Condvar::new()));
    let (verdict_tx, verdict_rx) = std::sync::mpsc::channel();
    for _ in 0..n {
        let arrivals = Arc::clone(&arrivals);
        let verdict_tx = verdict_tx.clone();
        post(
            lane.sender(),
            Box::new(move || {
                let (count, all_arrived) = &*arrivals;
                let mut arrived = count.lock().expect("arrival counter poisoned");
                *arrived += 1;
                all_arrived.notify_all();
                let (arrived, _) = all_arrived
                    .wait_timeout_while(arrived, ARRIVAL_DEADLINE, |arrived| *arrived < n)
                    .expect("arrival counter poisoned");
                let saw_all = *arrived >= n;
                drop(arrived);
                let _ = verdict_tx.send(saw_all);
            }),
        )
        .expect("posting to a started pool must succeed");
    }

    (0..n)
        .map(|_| {
            verdict_rx
                .recv_timeout(ANTI_WEDGE)
                .expect("every posted job must report its verdict")
        })
        .collect()
}

/// (aa) A size-N lane runs N jobs CONCURRENTLY: each of three jobs must see all
/// three in flight at once. A single-consumer lane fails at the deadline.
#[test]
fn a_pool_lane_runs_its_jobs_concurrently_up_to_its_size() {
    use crate::large_stack::Lane;

    const SIZE: usize = 3;
    static POOL: Lane = Lane::pool("t6517-conc-", SIZE);

    let observed = observe_concurrent_arrivals(&POOL, SIZE);

    assert_eq!(
        observed.len(),
        SIZE,
        "every submitter must have produced a verdict"
    );
    assert!(
        observed.iter().all(|saw_all| *saw_all),
        "a size-{SIZE} lane must run {SIZE} jobs at once: every job must observe \
         all {SIZE} arrivals before the deadline, got {observed:?}. A `false` \
         means that job timed out waiting for siblings that never started — \
         i.e. the lane still serializes."
    );
}

/// (ab) A pool NAMES each consumer `{prefix}{index}` and AMORTISES them: every job
/// lands on one of at most `size` threads, and never on the caller.
#[test]
fn a_pool_lane_names_and_amortises_each_consumer_thread() {
    use crate::large_stack::Lane;
    use std::collections::HashSet;

    const PREFIX: &str = "t6517-name-";
    const SIZE: usize = 2;
    static POOL: Lane = Lane::pool(PREFIX, SIZE);

    let caller_id = std::thread::current().id();
    let expected_names: HashSet<String> = (0..SIZE).map(|i| format!("{PREFIX}{i}")).collect();

    let mut seen_ids = HashSet::new();
    let mut seen_names = HashSet::new();
    for _ in 0..24 {
        let (id, name) = post_and_wait(POOL.sender(), || {
            (
                std::thread::current().id(),
                std::thread::current().name().map(str::to_owned),
            )
        });
        assert_ne!(
            id, caller_id,
            "a pool job must run on a lane consumer, not degrade to an inline \
             call on the submitter"
        );
        let name = name.expect("a pool consumer thread must be NAMED, not `<unnamed>`");
        assert!(
            expected_names.contains(&name),
            "a pool consumer must be named `{{prefix}}{{index}}` for some index \
             < {SIZE}; expected one of {expected_names:?}, got {name:?}"
        );
        assert!(
            name.len() <= 15,
            "the consumer name {name:?} must fit Linux's 15-byte \
             pthread_setname_np budget, or `std` silently drops it"
        );
        seen_ids.insert(id);
        seen_names.insert(name);
    }

    assert!(
        seen_ids.len() <= SIZE,
        "a size-{SIZE} lane must amortise at most {SIZE} threads across every \
         submission, saw {} distinct ThreadIds",
        seen_ids.len()
    );
    assert!(
        !seen_ids.is_empty(),
        "the submission loop must have run at least one job"
    );
}

/// (ac) A size-1 lane's consumer keeps its exact constant name, with no index
/// suffix, so profiler filters and `top -H` alerts keyed on the constants still
/// match.
#[test]
fn a_single_consumer_lane_keeps_its_exact_thread_name() {
    use crate::large_stack::{ENGINE_LANE, LSP_LANE, LSP_WORKER_THREAD_NAME, WORKER_THREAD_NAME};

    let engine = post_and_wait(ENGINE_LANE.sender(), || {
        std::thread::current().name().map(str::to_owned)
    });
    assert_eq!(
        engine.as_deref(),
        Some(WORKER_THREAD_NAME),
        "the size-1 ENGINE lane must keep its exact name — no `0` suffix"
    );

    let lsp = post_and_wait(LSP_LANE.sender(), || {
        std::thread::current().name().map(str::to_owned)
    });
    assert_eq!(
        lsp.as_deref(),
        Some(LSP_WORKER_THREAD_NAME),
        "the size-1 LSP lane must keep its exact name — no `0` suffix"
    );
}

/// (ad) A pool consumer carries the LARGE STACK: it survives ~16 MiB of recursion.
/// `Builder::new().name(..)` without `.stack_size(..)` compiles fine and yields a
/// 2 MiB consumer.
#[test]
fn a_pool_lane_carries_the_large_stack() {
    use crate::large_stack::Lane;

    const PREFIX: &str = "t6517-deep-";
    static POOL: Lane = Lane::pool(PREFIX, 2);

    let result = post_and_wait(POOL.sender(), || {
        deep_recurse_if_on_lane(PREFIX, DEEP_RECURSION_DEPTH)
    });

    let depth_reached = result.unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(
        depth_reached,
        u64::from(DEEP_RECURSION_DEPTH) + 1,
        "deep recursion must run to completion on a pool consumer's large stack"
    );
}

/// (ae) A panicking pool job re-raises its ORIGINAL payload on its awaiter, and the
/// pool afterwards still runs `size` jobs CONCURRENTLY — "it still answers" would
/// also hold for a pool that had lost N-1 consumers.
#[tokio::test]
async fn a_pool_lane_is_panic_isolated_and_keeps_all_its_consumers() {
    use crate::large_stack::{Lane, dispatch_async};

    const SIZE: usize = 3;
    static POOL: Lane = Lane::pool("t6517-panic", SIZE);

    let before = observe_concurrent_arrivals(&POOL, SIZE);
    assert!(
        before.iter().all(|saw_all| *saw_all),
        "precondition: the pool must run {SIZE} jobs at once BEFORE the panic, \
         got {before:?}"
    );

    let poisoned = tokio::spawn(dispatch_async::<_, ()>(POOL.sender(), async {
        panic!("pool boom")
    }))
    .await;
    let payload = poisoned
        .expect_err("a panicking pool job must reach its awaiter")
        .into_panic();
    assert_eq!(
        panic_message(&*payload),
        "pool boom",
        "the awaiter must receive the JOB's original payload, not a substitute"
    );

    let after = observe_concurrent_arrivals(&POOL, SIZE);
    assert!(
        after.iter().all(|saw_all| *saw_all),
        "every consumer must survive a poisoned job: the pool must still run \
         {SIZE} jobs at once, got {after:?}. A `false` here means the panic \
         killed a consumer and silently narrowed the pool."
    );
}

/// (af) A job running ON a pool that submits to THAT pool is rejected loudly,
/// naming the reentrancy and the lane, and the pool survives.
///
/// The guard is reached as in (r4): the outer future is driven by
/// `handle.block_on`, so the inner submission sees a runtime. Without the guard
/// this size-2 pool's second consumer would serve the inner job, so the test fails
/// on the missing panic rather than hanging.
#[tokio::test]
async fn submitting_to_your_own_pool_panics_loudly_instead_of_wedging_it() {
    use crate::large_stack::{Lane, dispatch_async};

    const PREFIX: &str = "t6517-reent-";
    static POOL: Lane = Lane::pool(PREFIX, 2);

    let joined = tokio::spawn(dispatch_async(POOL.sender(), async {
        dispatch_async(POOL.sender(), async { 1u32 }).await
    }))
    .await;
    let err = joined.expect_err(
        "a re-entrant pool submission must panic on its awaiter rather than \
         wedge the pool",
    );
    assert!(
        err.is_panic(),
        "the awaiting task must have PANICKED, not been cancelled: {err:?}"
    );
    let message = panic_message(&*err.into_panic());
    assert!(
        message.contains("re-entrant submission"),
        "the panic must name the reentrancy rather than surface as a generic \
         channel error, got: {message}"
    );
    assert!(
        message.contains(PREFIX),
        "the panic must name the LANE that was re-entered, got: {message}"
    );

    let (value, name) = post_and_wait(POOL.sender(), || {
        (7u32, std::thread::current().name().map(str::to_owned))
    });
    assert_eq!(
        value, 7,
        "the pool must survive a rejected re-entrant submission and keep serving"
    );
    assert!(
        name.as_deref().is_some_and(|n| n.starts_with(PREFIX)),
        "the surviving pool must still be the POOL — a degraded lane would \
         answer from a spawned default-stack thread instead, got {name:?}"
    );
}

/// (ag) A pool job may submit to ANOTHER lane, and lands on that lane's thread.
///
/// Every consumer of a pool publishes the SAME lane name; a per-consumer name would
/// make (af) hold for one consumer and silently fail for the rest. Asserting the
/// inner thread's NAME pins that the submission crossed lanes. As in (r3), the pool
/// job drives the submission with its own runtime, and a wrongly rejected
/// submission makes `post_and_wait` fail at once.
#[test]
fn a_pool_job_may_submit_to_another_lane() {
    use crate::large_stack::{LSP_LANE, LSP_WORKER_THREAD_NAME, Lane, dispatch_async};

    const PREFIX: &str = "t6517-cross-";
    static POOL: Lane = Lane::pool(PREFIX, 2);

    let ((outer, outer_name), inner) = post_and_wait(POOL.sender(), || {
        let outer = this_thread();
        let inner = tokio::runtime::Builder::new_current_thread()
            .build()
            .map(|runtime| {
                runtime.block_on(dispatch_async(LSP_LANE.sender(), async { this_thread() }))
            });
        (outer, inner)
    });

    let (inner, inner_name) = inner.expect("the pool job must build its runtime");
    assert!(
        outer_name.as_deref().is_some_and(|n| n.starts_with(PREFIX)),
        "the outer job must run on a pool consumer, got {outer_name:?}"
    );
    assert_eq!(
        inner_name.as_deref(),
        Some(LSP_WORKER_THREAD_NAME),
        "a cross-lane submission from a pool must run on the OTHER lane's \
         thread — the guard must not reject it, and it must not degrade to an \
         inline await on the pool consumer"
    );
    assert_ne!(outer, inner);
}

/// (al) `Lane::started()` reports 0 before creation and the full `size` after, on a
/// TEST-LOCAL pool so both halves are observable. `lsp_lane_routing_tests`' (p)
/// asserts the after half against the production `LSP_POOL`, which another test
/// may already have created.
#[test]
fn a_lane_reports_the_consumers_it_actually_started() {
    use crate::large_stack::Lane;

    const SIZE: usize = 3;
    static POOL: Lane = Lane::pool("t6517-started", SIZE);

    assert_eq!(
        POOL.started(),
        0,
        "a lane nobody has submitted to must report ZERO started consumers — \
         lanes are created lazily on the first `sender()` call, and a session \
         that never submits must pay nothing for them. A non-zero count here \
         means either eager creation or a `started()` that echoes `size()`, and \
         the second would make (p)'s partial-spawn guard vacuous."
    );

    POOL.sender()
        .expect("the pool must start at least one consumer under test conditions");

    // Strict equality: the silent narrowing this catches IS a count between 1
    // and `SIZE`, which `>= 1` cannot see.
    let started = POOL.started();
    assert_eq!(
        started, SIZE,
        "the lane started {started} of {SIZE} consumers. TRIAGE THE \
         ENVIRONMENT FIRST, and the discriminator is on stderr: `Lane::sender` \
         warns and continues on a partial spawn failure, printing `failed to \
         spawn t6517-started lane consumer <i> of {SIZE}` with the OS error \
         whenever a 256 MiB mapping was refused. This binary declares many at \
         once — every lane and pool in it, across concurrently-running tests — \
         so a restrictive `RLIMIT_AS`, `vm.overcommit_memory=2`, a low \
         `vm.max_map_count` or a container memory cap can red this line with \
         nothing in this crate having changed, which is precisely the case \
         `Lane::sender` was written to survive. With NO such warning present \
         the shortfall IS a code defect: a `started()` that under-reports \
         makes (p)'s production guard meaningless."
    );
    assert_eq!(
        started,
        POOL.size(),
        "on a healthy machine the realised count is the declared one; when it \
         is not, THAT is the fact worth reporting, and until `started()` \
         existed nothing in the process could state it."
    );
}
