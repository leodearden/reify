//! Unit tests for [`crate::large_stack`] — the defense-in-depth module that runs
//! engine and compiler work on OS threads with an explicit LARGE stack (task
//! 5357, belt-and-suspenders atop task 5337's compiler-layer
//! `stacker::maybe_grow` + recursion-depth cap).
//!
//! ## Why the deep-recursion tests are safe (no "violent RED")
//!
//! The large-stack property is proven by [`deep_recurse`], which pins ~8 KiB of
//! stack per frame and recurses ~2048 deep (~16 MiB — 8x the compiler's 2 MiB
//! default worker stack). Running that on a default-stack thread would SIGSEGV
//! and abort the *entire* test binary. So the recursion is reached ONLY through
//! the large-stack helpers — and on the lanes only through
//! [`deep_recurse_if_on_thread`], which CHECKS the thread first: each lane has a
//! degraded arm that runs its work on a default-size stack, and a degraded lane
//! must yield a clean assertion failure rather than an overflow. The per-call
//! `spawn_on_large_stack` test calls [`deep_recurse`] directly: that helper's
//! only degradation is a refused spawn, which it reports as `Err` instead of
//! running the closure anywhere.

use crate::tests::test_helpers::{
    ANTI_WEDGE, DEEP_RECURSION_DEPTH, deep_recurse, deep_recurse_if_on_thread,
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

// ── Named LANES: one mechanism, two instances (task 5772) ────────────────────
//
// `lsp_request` also needs a large stack, and the task asks for ONE worker
// design rather than two divergent large-stack approaches. Taken as "one
// THREAD", though, that would be a latency regression: LSP dispatch never takes
// the engine mutex, so it shares nothing with engine work, yet a single-consumer
// queue would make a hover or completion queue behind an in-flight
// `set_parameter` geometry evaluation (hundreds of ms to seconds) — head-of-line
// blocking on the highest-frequency path in the GUI.
//
// So the mechanism is generalized into a named LANE: one code path, two `static`
// instances. These tests pin that "one mechanism" and "two threads" are BOTH
// true — a second lane must be a second INSTANCE, not a second design, and must
// inherit every property the engine lane already proves (large stack, panic
// isolation, per-lane amortisation).
//
// What none of them claims — and what no test here should be read as claiming —
// is concurrency WITHIN a lane. A lane has one consumer, so LSP requests now
// serialize against each other; (r) pins that the two lanes are separate
// threads, not that either lane runs two jobs at once. See `Lane`'s "What the
// split does NOT buy" for that boundary.

/// (q) Every large-stack thread name is DISTINCT from every other, so a
/// backtrace, `top -H` row or profiler capture says which TIER — and which LANE
/// — the work is on.
///
/// Distinctness is the whole content of the property: a shared label would make
/// a keystroke-path stall and a geometry-evaluation stall indistinguishable in
/// exactly the capture where telling them apart matters.
///
/// The other half the two per-constant tests this replaces used to assert —
/// `len() <= 15`, Linux's `pthread_setname_np` budget, which `std` silently
/// ignores when exceeded — is proven at COMPILE time by the
/// `const _: () = assert!(..)` block beside each constant in `large_stack.rs`.
/// A runtime assertion for it cannot fail in any build that exists, so carrying
/// one was dead weight; the const asserts are the real guard.
#[test]
fn large_stack_thread_names_are_pairwise_distinct() {
    use crate::large_stack::{ENGINE_THREAD_NAME, LSP_WORKER_THREAD_NAME, WORKER_THREAD_NAME};

    let names = [
        (ENGINE_THREAD_NAME, "the per-call engine thread"),
        (WORKER_THREAD_NAME, "the persistent ENGINE lane"),
        (LSP_WORKER_THREAD_NAME, "the persistent LSP lane"),
    ];

    for (i, (name, what)) in names.iter().enumerate() {
        for (other, other_what) in &names[i + 1..] {
            assert_ne!(
                name, other,
                "{what} and {other_what} must be distinguishable in a backtrace \
                 or profiler row"
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
    use crate::large_stack::run_on_lsp_worker;

    let caller_id = std::thread::current().id();

    let engine_a = engine_lane_thread();
    let engine_b = engine_lane_thread();
    let lsp_a = run_on_lsp_worker(async { std::thread::current().id() }).await;
    let lsp_b = run_on_lsp_worker(async { std::thread::current().id() }).await;

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
        LSP_WORKER_THREAD_NAME, WORKER_THREAD_NAME, post_to_worker, run_on_lsp_worker,
    };

    let (tx, rx) = std::sync::mpsc::channel();
    post_to_worker(move || {
        let outer = this_thread();
        let inner = tokio::runtime::Builder::new_current_thread()
            .build()
            .map(|runtime| runtime.block_on(run_on_lsp_worker(async { this_thread() })));
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
    use crate::large_stack::run_on_lsp_worker;

    let caller_id = std::thread::current().id();
    let before = run_on_lsp_worker(async { std::thread::current().id() }).await;
    assert_ne!(
        before, caller_id,
        "the pre-panic job must run on the lane, not inline on the caller"
    );

    let poisoned = tokio::spawn(run_on_lsp_worker::<_, ()>(async { panic!("lsp boom") })).await;
    let payload = poisoned
        .expect_err("a panicking LSP-lane job must reach its awaiter")
        .into_panic();
    assert_eq!(
        panic_message(&*payload),
        "lsp boom",
        "the awaiter must receive the JOB's original payload, not a substitute"
    );

    let (value, after) = run_on_lsp_worker(async { (5u32, std::thread::current().id()) }).await;
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
async fn run_on_lsp_worker_returns_value_and_runs_on_the_lane() {
    use crate::large_stack::{LSP_WORKER_THREAD_NAME, run_on_lsp_worker};

    let caller_id = std::thread::current().id();
    // Owned and MOVED into the job, because the lane outlives this frame — a
    // heap-owned `Vec` rather than a `Copy` array, which the job would merely
    // copy.
    let data = Vec::from([1u64, 2, 3, 4, 5]);

    let (sum, inner_id, inner_name) = run_on_lsp_worker(async move {
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
async fn run_on_lsp_worker_does_not_block_the_calling_runtime() {
    use crate::large_stack::run_on_lsp_worker;
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
    let lane_result = run_on_lsp_worker(async {
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
async fn run_on_lsp_worker_propagates_the_original_job_panic() {
    use crate::large_stack::run_on_lsp_worker;

    let joined = tokio::spawn(async {
        // Concrete `T = ()` so inference is unambiguous; the closure never
        // returns normally, but the panic must still cross the lane AND the
        // oneshot to reach the awaiting task.
        run_on_lsp_worker::<_, ()>(async { panic!("async boom") }).await;
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
/// A refused 256 MiB mapping must never hang an `await`; this exercises that
/// arm rather than leaving it to prose.
///
/// # Why this test alone is NOT sufficient, and must not be trusted as if it were
///
/// The body submitted here is deliberately trivial, which makes it blind to the
/// hazard that actually lived in this arm. In its earlier CLOSURE form
/// (`|| (77u32, thread::current().id())`) it needed no runtime, so it passed
/// while the ONLY production submission — a pre-baked
/// `Handle::block_on(lsp_request_impl(..))` — panicked "Cannot start a runtime
/// from within a runtime" every time this arm ran, unwinding the Tauri command
/// and leaving the frontend's `invoke` promise unresolved. A generic guard over
/// a stand-in body can only show that the ARM resolves, never that the real
/// WORK does. The claim about production belongs to
/// `lsp_bridge_tests::lsp_request_on_lane_without_a_lane_still_resolves_to_the_right_value`,
/// which drives the same arm through the real composition; do not weaken that
/// one on the grounds that this one covers it.
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
async fn run_on_lsp_worker_survives_deep_recursion_over_default_stack() {
    use crate::large_stack::{LSP_WORKER_THREAD_NAME, run_on_lsp_worker};

    let result = run_on_lsp_worker(async {
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

/// (z) The `SendError` recovery arm of the ASYNC lane, driven by a job that —
/// like production — must be driven by a [`tokio::runtime::Handle`] and
/// therefore cannot legally run in the submitting async frame.
///
/// The second half of the same finding (x) covers for the `None` arm. When
/// `send` fails, `mpsc` hands the JOB BACK, and the async lane's job is exactly
/// the one the lane thread would have run: a `Handle::block_on` of the caller's
/// future. Running that in this frame — a thread already inside the tauri tokio
/// runtime — hits `enter_runtime`'s `is_entered()` guard and panics "Cannot
/// start a runtime from within a runtime", which unwinds the Tauri command and
/// leaves the frontend's `invoke` promise unresolved. So the recovery arm must
/// hand the job to a thread that is NOT in a runtime context.
///
/// The failure is provoked deterministically with a SYNTHETIC sender whose
/// consumer is already gone — no real lane, no `pthread_create` failure, no
/// timing. Asserting on the RESOLVED VALUE (rather than merely "did not hang")
/// is what makes this a real assertion about recovery: a result must never be
/// lost, and it must be produced somewhere legal.
///
/// DEPENDS on [`crate::large_stack::JobSender`] being `pub(crate)`, so the
/// synthetic sender's type can be named here at all.
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
/// The alternative is the module's worst possible outcome: a lane has a SINGLE
/// consumer, so the inner job could only run once the outer one returned, while
/// the outer one waits for it. The lane thread would never return to its
/// `for job in rx` loop, so the lane is dead AND every later submitter in the
/// process hangs too — silently, unrecoverably.
///
/// The rejection takes a long route, and every link is load-bearing. The panic
/// must escape the INNER future mid-poll,
/// unwind out of [`tokio::runtime::Handle::block_on`] (whose `enter_runtime`
/// guard has to restore the runtime context on the way out), be caught by the
/// OUTER job's own `catch_unwind`, ride back over the `tokio::sync::oneshot`,
/// and be `resume_unwind`-ed on the awaiting submitter. Any one of those links
/// failing turns a loud rejection back into the process-wide wedge the guard
/// exists to replace.
///
/// `tokio::spawn` is the unwind boundary, exactly as in (w): the panic arrives
/// on the awaiting task, so [`tokio::task::JoinError::into_panic`] hands back
/// the payload without needing `futures::FutureExt::catch_unwind` (and so
/// without a new dependency).
///
/// # Why the guard is genuinely REACHED here
///
/// `dispatch_async` runs its reentrancy check AFTER an early return on
/// [`tokio::runtime::Handle::try_current`] being `Err`, so a submission from a
/// lane thread with NO ambient runtime would `.await` inline and never reach the
/// check. That is not a hole this test papers over — it is the safe arm: an
/// inline `.await` enqueues nothing, so it cannot wedge anything, and the guard
/// is only needed where a job would actually be queued. On the production path
/// the check IS reached, and that is what this test pins: the outer job is
/// driven by `handle.block_on`, which installs the runtime context, so
/// `try_current()` inside the inner submission succeeds and execution falls
/// through to `assert_not_reentrant`.
///
/// Unlike the deep-recursion tests, this one cannot honour the "no violent RED"
/// doctrine: if the guard is ever removed this test hangs rather than failing,
/// because the wedge is of a process-wide `static` lane. A timeout could only
/// make this test's report legible while every other LSP-lane test hung anyway.
#[tokio::test]
async fn submitting_to_your_own_lane_from_a_future_panics_loudly_instead_of_wedging_it() {
    use crate::large_stack::{LSP_WORKER_THREAD_NAME, run_on_lsp_worker};

    let joined = tokio::spawn(async {
        // The OUTER future is driven ON the LSP lane; the inner submission
        // targets that same lane, which is the wedge.
        run_on_lsp_worker(async { run_on_lsp_worker(async { 1u32 }).await }).await
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
    let (value, ran_on) =
        run_on_lsp_worker(async { (7u32, std::thread::current().name().map(str::to_owned)) }).await;
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
