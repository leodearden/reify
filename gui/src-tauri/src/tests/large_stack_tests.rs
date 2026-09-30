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
    use crate::large_stack::{ENGINE_THREAD_NAME, JobSender, OnAbandon, post};

    let (lane_tx, lane_rx) = std::sync::mpsc::channel();
    drop(lane_rx);
    let dead = JobSender::new("dead-lane", lane_tx, OnAbandon::Run);

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
// What none of THESE claims — (q) through (t) — is concurrency WITHIN a lane:
// (r) pins that the two lanes are separate threads, not that either lane runs
// two jobs at once. That boundary is no longer open, though. Task 6517
// generalised `Lane` to N consumers and the "Bounded intra-lane concurrency"
// section below asserts it directly — (aa) is the head-of-line-blocking
// measurement, and it FAILS against a single-consumer lane. Read this paragraph
// as scoping the 5772 tests, not as a standing claim about the file.

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
///
/// EXTENDED for task 6517 to cover the query pool, in TWO halves, because the
/// pool's names are the only ones here that are realised rather than declared.
/// [`crate::large_stack::LSP_POOL_THREAD_PREFIX`] is a PREFIX, so what reaches
/// `top -H` is `{prefix}{i}`, and inequality over the four raw constants cannot
/// see the regressions this docstring claims to rule out —
/// [`crate::large_stack::LSP_WORKER_THREAD_NAME`] changed to `reify-lsp-p0`
/// would be distinct from the prefix and identical to a realised pool name, and
/// a pool prefix equal to another tier's whole name would yield `reify-lsp-w0`
/// and make a filter keyed on the ordered lane match pool rows. So the second
/// half asserts PREFIX-FREEDOM between each realised pool name and every other
/// tier: a pool row is naturally selected by prefix, and that selector must
/// catch nothing else.
///
/// Prefix-freedom is deliberately NOT asserted across the whole-name tiers, and
/// the reason is measured rather than assumed: `ENGINE_THREAD_NAME`
/// (`reify-engine`) already prefixes `WORKER_THREAD_NAME` (`reify-engine-w`).
/// Those two are distinct whole names selected by exact match, the containment
/// predates pools, and renaming a shipped thread is precisely what
/// [`crate::large_stack::Lane`]'s naming rule promises not to do — so it is
/// recorded here rather than asserted away.
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
    use crate::large_stack::{JobSender, OnAbandon, dispatch_async};

    // Consumer dropped before any send: every `send` fails at once with
    // `SendError(job)`, which is the arm under test.
    let (tx, rx) = std::sync::mpsc::channel();
    drop(rx);
    let dead = JobSender::new("test-dead-async", tx, OnAbandon::Run);

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
// ── Bounded intra-lane concurrency (task 6517) ───────────────────────────────
//
// The section above closes with the boundary task 5772 left open: "no test here
// claims concurrency WITHIN a lane". This section is that claim, and it is the
// whole content of task 6517's first half — a lane generalised from ONE consumer
// to N, so that head-of-line blocking among LSP queries becomes BOUNDED at the
// pool size rather than total.
//
// # Why every test here declares its OWN lane
//
// `ENGINE_LANE` and `LSP_LANE` are process-wide `static`s shared by every test
// in this binary, and cargo runs those tests CONCURRENTLY. A test that parked a
// consumer of a global pool to prove occupancy would therefore be parking a
// resource an unrelated test is simultaneously trying to use — starving it, and
// hanging the suite rather than failing it. So each test below declares a
// TEST-LOCAL `static POOL: Lane = Lane::pool(..)` inside its own fn body. A
// `static` in a fn body still has `'static` lifetime (which `Lane::sender`
// requires) but is nameable only from that fn, which makes the isolation
// structural rather than a convention someone must remember.
//
// # Why no RED here is a hang
//
// Same doctrine as the rest of this file. Concurrency is measured with an
// arrival counter under a `Condvar` plus a generous wall-clock DEADLINE: a lane
// that failed to run N jobs at once makes the counter stall, the deadline
// elapses, and the job returns `false` — a clean assertion failure naming what
// it saw.
// The deep-recursion test goes through [`deep_recurse_if_on_lane`], which
// refuses to recurse anywhere but a real pool consumer, for exactly the reason
// [`deep_recurse_if_on_thread`] exists.

/// Recurse ~16 MiB ONLY if we genuinely landed on a consumer of the pool lane
/// named by `prefix`; otherwise report where we actually are, without recursing.
///
/// The pool sibling of [`deep_recurse_if_on_thread`], and it must be a separate
/// helper rather than a call to that one: a pool consumer's thread name is
/// `{prefix}{index}`, so no single `&'static str` is the expected name. Matching
/// the prefix plus an all-digits tail is what keeps the check as tight as the
/// exact-name one — `reify-lsp-w` must not satisfy a `reify-lsp-p` probe, and
/// neither must a caller thread that merely happens to start with the prefix.
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

/// Post `n` jobs to `lane`, each of which increments a shared arrival counter
/// and then parks on a `Condvar` until every one of the `n` has arrived — or
/// until a wall-clock deadline elapses.
///
/// The wait PARKS rather than spins. A spinning arrival loop keeps every
/// consumer that has already arrived on a CPU for the whole wait, so under a
/// loaded verify it can starve the very sibling it is waiting for of the
/// scheduling it needs to arrive, and report a false "still serializes".
///
/// Returns, per job, whether it observed all `n` in flight AT ONCE. On a
/// single-consumer lane job 1 parks holding the only consumer, jobs 2..n never
/// start, the deadline elapses and job 1 reports `false` — a bounded assertion
/// failure, never a hang, which is the property this whole file is written to.
///
/// Factored out because it is the measurement BOTH (aa) and (ae) need: (aa)
/// establishes the concurrency, (ae) re-establishes it after a panic to prove no
/// consumer was lost. Writing it twice would let the two drift.
///
/// The non-vacuity guard is load-bearing HERE rather than in either caller,
/// because the degraded lane is invisible from the `Vec<bool>` this returns: a
/// lane with no consumers makes `post` run every job on its own spawned
/// default-stack thread, so all `n` jobs would run concurrently anyway and every
/// one would report `true` with ZERO consumers started. That is the exact
/// head-of-line-blocking property task 6517 exists to establish, asserted over a
/// lane that never ran — so it is ruled out before a single job is posted.
fn observe_concurrent_arrivals(lane: &'static crate::large_stack::Lane, n: usize) -> Vec<bool> {
    use crate::large_stack::post;
    use std::sync::{Arc, Condvar, Mutex};

    /// A liveness BACKSTOP, not the property under test — see the section
    /// header. A true serialization never recovers, so a long deadline costs
    /// nothing on green. It stays well under [`ANTI_WEDGE`] because a
    /// serialized lane releases its verdicts one deadline apart, and each
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

/// (aa) A size-N lane runs N jobs CONCURRENTLY — the head-of-line-blocking
/// measurement in regression-test form.
///
/// This is the property task 6517 exists to establish, and it is asserted
/// directly rather than inferred from thread names: every one of the three jobs
/// must observe all three in flight at once, which is only possible if three
/// consumers are draining the queue simultaneously. A single-consumer lane fails
/// it as a clean assertion bounded by [`observe_concurrent_arrivals`]'s
/// deadline, never as a hang.
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

/// (ab) A pool NAMES each consumer `{prefix}{index}` and AMORTISES them: every
/// job lands on a thread drawn from a set of at most `size`, and never on the
/// caller.
///
/// Three properties in one submission loop, because they are the same property
/// seen from three sides. The NAME is the observability half — a pool whose
/// consumers reported `<unnamed>`, or all reported the same string, would make a
/// stalled query indistinguishable from a stalled sibling in a `top -H` capture.
/// The bounded `ThreadId` SET is the amortisation half: a pool that spawned a
/// thread per job would pay the 256 MiB mapping this tier exists to eliminate.
/// And "never the caller" is the non-vacuity half — a lane that degraded to
/// inline execution would satisfy both of the others trivially.
///
/// The 15-byte assertion is the runtime companion to the `const _: () =
/// assert!(..)` beside each production prefix: `std` silently IGNORES a
/// `pthread_setname_np` name that overruns Linux's budget, so an over-long
/// pool name would not fail loudly — it would just not appear in `/proc`.
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

/// (ac) Generalising `Lane` to N consumers renames NOTHING: a size-1 lane still
/// reports its exact constant, with no index suffix.
///
/// The load-bearing half of the generalisation's compatibility story, and the
/// one a `format!("{name}{i}")`-for-every-lane implementation would silently
/// break: `reify-engine-w0` is a different string from `reify-engine-w`, so
/// every existing profiler alert, `top -H` filter and test assertion keyed on
/// the constants would stop matching. Pinning it here means the pool mechanism
/// cannot be landed by renaming the threads that predate it.
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

/// (ad) LARGE STACK — a pool consumer survives ~16 MiB of recursion, exactly as
/// a single-consumer lane does.
///
/// A pool is an INSTANCE of the lane mechanism, not a second design, so it must
/// inherit every property the single-consumer lanes already prove. The stack is
/// the one that would be easiest to lose while rewriting the spawn loop —
/// `Builder::new().name(..)` without `.stack_size(..)` compiles fine and yields
/// a 2 MiB consumer.
///
/// Per this file's "no violent RED" doctrine the recursion is reached ONLY
/// through [`deep_recurse_if_on_lane`], so a degraded lane yields a clean
/// assertion failure instead of SIGABRTing the whole binary.
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

/// (ae) A panicking pool job re-raises its ORIGINAL payload on ITS awaiter,
/// and the pool afterwards still runs `size` jobs CONCURRENTLY.
///
/// The second half is what makes this more than a re-run of (t) against a new
/// instance. A pool has N consumers, so "it still answers" is satisfied by a
/// pool that lost N-1 of them — the panic would have silently converted the
/// bounded-blocking guarantee back into the total serialization task 6517 exists
/// to remove, while every simple survival assertion stayed green. Re-measuring
/// full concurrency is the only assertion that can see that.
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

/// (af) A job running ON a pool that submits to THAT SAME pool is rejected
/// loudly, naming the reentrancy and the lane — and the pool survives.
///
/// The guard stays BLANKET per lane rather than becoming "reject only when no
/// consumer is free". That is deliberately conservative: a pool with a free
/// consumer could in principle serve a self-submission, but `size` simultaneous
/// self-submissions genuinely wedge a size-`size` pool, and the wedge is
/// process-wide and silent — the one outcome `large_stack`'s docs promise never
/// to produce. A rule whose safety depends on how many callers happen to be
/// in flight is not a rule.
///
/// The guard is reached for the reason (r4) gives: the outer future is driven
/// by `handle.block_on`, so the inner submission sees a runtime and falls
/// through to `assert_not_reentrant`. Unlike (r4), removing the guard would not
/// hang this test: this size-2 pool's second consumer would serve the inner
/// job, and the test would fail on the missing panic instead.
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
/// The pool counterpart of (r3), and it pins the same distinction: the guard is
/// keyed on the lane IDENTITY published by the consumer thread, not on "am I on
/// some lane thread". Every consumer of a pool publishes the SAME lane name —
/// which is what makes (af) work — so a sloppy generalisation could easily
/// publish a per-consumer name (`{prefix}{index}`) instead, which would leave
/// (af) passing for one consumer and silently failing for the rest. Asserting
/// the inner job's thread NAME rather than merely that it returned is what pins
/// that it genuinely crossed lanes.
///
/// Reached exactly as (r3) reaches it: the pool job drives the submission with
/// a runtime of its own, because a lane thread has no ambient runtime and
/// without one `dispatch_async` awaits inline and never reaches the guard. A
/// wrongly rejected submission panics inside the probe, `post` contains it, and
/// `post_and_wait` fails at once rather than hanging.
#[test]
fn a_pool_job_may_submit_to_another_lane() {
    use crate::large_stack::{LSP_WORKER_THREAD_NAME, Lane, run_on_lsp_worker};

    const PREFIX: &str = "t6517-cross-";
    static POOL: Lane = Lane::pool(PREFIX, 2);

    let ((outer, outer_name), inner) = post_and_wait(POOL.sender(), || {
        let outer = this_thread();
        let inner = tokio::runtime::Builder::new_current_thread()
            .build()
            .map(|runtime| runtime.block_on(run_on_lsp_worker(async { this_thread() })));
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

// ── Cancel at the lane (task 6517) ───────────────────────────────────────────
//
// `dispatch_async` moves a `tokio::sync::oneshot::Sender` into the job, and
// `Sender::is_closed()` is true exactly when the awaiting side's future was
// dropped. On a destination declared `OnAbandon::Discard`, checking it before
// driving anything skips an abandoned job instead of executing it. No new
// dependency, no new token type, no change to the job contract.
//
// # What this is, and is NOT, evidence of
//
// It is a STRUCTURAL guarantee, not a measured saving, and these tests are
// written knowing that. Task 5772 disclosed the lane as a loss of
// drop-cancellation on the premise that an abandoned frontend `invoke`
// previously dropped the Tauri command's future; against the pinned `tauri`
// 2.11.2 that premise is false. `InvokeResolver::respond_async` /
// `respond_async_serialized_inner` both `async_runtime::spawn(..)` and discard
// the returned handle, and dropping a tokio
// `JoinHandle` detaches rather than cancels — so the command future ran to
// completion before 5772 too. On the shipped app the only thing that closes the
// receiver is runtime/app teardown, which is why every test below MANUFACTURES
// the drop with `tokio::time::timeout`.
//
// # Why the policy is a property of the DESTINATION
//
// The check was first written blanket, and that was a defect: discarding a
// queued `textDocument/didOpen` means `InProcessLsp` never learns the document
// exists, and the file stays permanently dark to hover/completion/diagnostics.
// `OnAbandon::Run` is therefore the default and `OnAbandon::Discard` an opt-in
// carried by the queue — see `large_stack::OnAbandon`. (aj0) below pins the
// `Run` half; `lsp_bridge_tests`' (o2) pins it end-to-end on a real lane.
//
// # Why these tests use a SYNTHETIC sender
//
// They need to observe the queue between the submission and the job running,
// which no real lane permits — a real consumer would pick the job up
// immediately. Building a `JobSender` over a channel whose `Receiver` the TEST
// holds makes the ordering provable rather than timed: the job cannot possibly
// have run before the test runs it by hand. No global lane is touched, and no
// assertion depends on a race.

/// The shared body of (ah) and (aj0): manufacture an ABANDONED submission to a
/// destination declared with `on_abandon`, and report whether the lane drove
/// its future.
///
/// (ah) and (aj0) differ in exactly two things — the destination's declared
/// policy, and the polarity of the conclusion — so what they share is written
/// ONCE here. Writing it twice is what would let the two drift, and every part
/// of it is load-bearing: the synthetic sender whose `Receiver` this frame
/// holds, the elapsing timeout that PERFORMS the abandonment, the `try_recv`
/// proving the job was nonetheless enqueued, and the hand-invocation on a plain
/// `std` thread. A fix applied to one copy would silently not reach the other.
/// Same reason `observe_concurrent_arrivals` above exists.
///
/// The preconditions are asserted HERE rather than by the callers, because they
/// are preconditions of the MEASUREMENT and not either test's claim: the await
/// must elapse (that elapse *is* the abandonment), the future must not have
/// been polled beforehand (this frame holds the only `Receiver`), the job must
/// have been enqueued regardless, and invoking it must not panic. Only the
/// answer — was the future polled — is returned, and each caller asserts its
/// own polarity on it.
///
/// # Why the job is invoked on a plain `std` thread
///
/// The job carries a `Handle::block_on`, and calling that inside this test's
/// runtime panics "Cannot start a runtime from within a runtime". On a plain
/// `std` thread it is legal — so at RED the future genuinely runs and the
/// caller fails on the flag it is about, rather than on a nested-runtime panic
/// that names nothing.
async fn abandoned_submission_was_polled(
    on_abandon: crate::large_stack::OnAbandon,
    sender_name: &'static str,
) -> bool {
    use crate::large_stack::{JobSender, dispatch_async};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    // This frame HOLDS `rx`, so nothing drains the queue and the job provably
    // cannot run before the hand-invocation below.
    let (tx, rx) = std::sync::mpsc::channel();
    let sender = JobSender::new(sender_name, tx, on_abandon);

    let polled = Arc::new(AtomicBool::new(false));
    let polled_in_fut = Arc::clone(&polled);

    let elapsed = tokio::time::timeout(
        Duration::from_millis(50),
        dispatch_async(Some(&sender), async move {
            polled_in_fut.store(true, Ordering::SeqCst);
            7u32
        }),
    )
    .await;
    assert!(
        elapsed.is_err(),
        "precondition: with nobody draining the queue the await must time out — \
         that elapse is what DROPS the submitted future and abandons the request"
    );
    assert!(
        !polled.load(Ordering::SeqCst),
        "precondition: the job cannot have run yet — this frame holds the only \
         `Receiver`"
    );

    let job = rx.try_recv().expect(
        "the abandoned request must still have been ENQUEUED — these tests are \
         about what the lane does with it, not about whether it arrived",
    );

    std::thread::spawn(job).join().expect(
        "invoking the job must be a clean no-op or a clean run, never a panic. \
         On a `Discard` destination the captured future is dropped unpolled, \
         and that drop must not panic — cancellation must not trade wasted work \
         for a new failure mode.",
    );

    polled.load(Ordering::SeqCst)
}

/// (ah) An ABANDONED submission to an `OnAbandon::Discard` destination is
/// dropped at the lane instead of driven.
///
/// The abandonment is SYNTHESISED by `abandoned_submission_was_polled` above —
/// `tokio::time::timeout` elapsing drops the awaiting future, which drops the
/// `oneshot` receiver and closes the `reply_tx` the job holds. It is not
/// modelled on a production trigger, because in `tauri` 2.11.2 there isn't one
/// short of runtime teardown: an abandoned `invoke` leaves the command future
/// detached and running (see this section's header). This pins the mechanism,
/// not a saving.
///
/// `Discard` is the whole subject: on an `OnAbandon::Run` destination this same
/// submission MUST be driven, which is (aj0)'s claim and its visible twin.
#[tokio::test]
async fn an_abandoned_submission_is_dropped_at_the_lane_instead_of_driven() {
    use crate::large_stack::OnAbandon;

    let polled = abandoned_submission_was_polled(OnAbandon::Discard, "test-cancel").await;

    assert!(
        !polled,
        "the lane must DISCARD a job whose awaiting side is gone, not drive it. \
         The future was polled, so the abandoned request ran anyway — occupying a \
         consumer and delaying the live requests queued behind it, which is \
         exactly the cost task 5772 disclosed."
    );
}

/// (ai) The anti-vacuity twin of (ah): a LIVE submission is still driven.
///
/// Without this, (ah) would also be satisfied by an implementation that never
/// ran anything at all — a lane that dropped every job would pass a
/// "cancellation works" assertion perfectly while resolving nothing.
///
/// Same synthetic-sender shape, but a second thread drains the queue and runs
/// the job WHILE this task awaits, so the `oneshot` receiver is provably alive at
/// the moment the job body checks it. That thread is also what makes the job's
/// `Handle::block_on` legal, as in (ah).
#[tokio::test]
async fn a_live_submission_is_still_driven() {
    use crate::large_stack::{JobSender, OnAbandon, dispatch_async};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    // `Discard`, deliberately the same policy as (ah): that is what makes this
    // twin sharp. It proves the check gates on the RECEIVER being gone and on
    // nothing else — an implementation that read the policy and skipped
    // everything queued to a discarding destination would pass (ah) perfectly.
    let (tx, rx) = std::sync::mpsc::channel();
    let sender = JobSender::new("test-live", tx, OnAbandon::Discard);

    let polled = Arc::new(AtomicBool::new(false));
    let polled_in_fut = Arc::clone(&polled);

    let drainer = std::thread::spawn(move || {
        let job = rx.recv().expect("a live submission must be enqueued");
        job();
    });

    let value = tokio::time::timeout(
        Duration::from_secs(10),
        dispatch_async(Some(&sender), async move {
            polled_in_fut.store(true, Ordering::SeqCst);
            4242u32
        }),
    )
    .await
    .expect(
        "a submission whose awaiting side is still alive must RESOLVE — a lane \
         that skipped it would hang this await until the timeout",
    );

    drainer.join().expect("the drainer thread must not panic");

    assert_eq!(
        value, 4242,
        "a live submission must deliver its value unchanged: the cancellation \
         check must gate on the receiver being GONE, never on anything else"
    );
    assert!(
        polled.load(Ordering::SeqCst),
        "a live submission's future must actually be POLLED, not merely answered"
    );
}

/// (aj0) An abandoned submission to an `OnAbandon::Run` destination IS DRIVEN.
///
/// (ah)'s structural twin, and the one that makes cancel-at-the-lane a property
/// of the DESTINATION rather than a blanket rule. Same manufactured
/// abandonment, through the same `abandoned_submission_was_polled` body — the
/// declared policy is the ONLY thing that differs, which is what makes a
/// regression to the blanket `if reply_tx.is_closed()` red exactly here and
/// nowhere else. Sharing the body is what keeps that claim literally true
/// rather than true by resemblance.
///
/// What it stands in for is not hypothetical. The ordered LSP lane carries
/// `textDocument/didOpen`; discarding one unrun means `InProcessLsp` never
/// learns the document exists, `ReifyLanguageServer::did_change` then takes its
/// `didChange for unknown URI` branch and applies nothing, and every query
/// handler answers `Ok(None)` for that URI. The file is permanently dark until it is closed and
/// reopened. `lsp_bridge_tests`' (o2) pins that end-to-end through the real
/// composition; this pins the primitive underneath it.
#[tokio::test]
async fn an_abandoned_submission_to_a_run_destination_is_still_driven() {
    use crate::large_stack::OnAbandon;

    let polled = abandoned_submission_was_polled(OnAbandon::Run, "test-run-anyway").await;

    assert!(
        polled,
        "an `OnAbandon::Run` destination must DRIVE a job whose awaiting side is \
         gone. It was skipped — which for the ordered LSP lane means a queued \
         `didOpen` never reaches the server and the document stays permanently \
         unknown to hover, completion and diagnostics."
    );
}

/// (aj) The cancel path drops `fut` INSIDE the runtime context.
///
/// `dispatch_async` is generic over `Fut`, and the discard arm is the only place
/// a submitted future is disposed of without `handle.block_on` — so it is the
/// only place a captured tokio resource's destructor would run on a plain `std`
/// thread with no ambient runtime. The guard is one line (`let _enter =
/// handle.enter();`); without a test, deleting it leaves every other assertion
/// in this file green, because (ah)'s future captures only an `Arc<AtomicBool>`
/// and a `u32` and has no runtime-dependent destructor at all.
///
/// # Why the payload is a hand-written `Drop`, not a `tokio::time::Sleep`
///
/// Because the claim under test is "a destructor that needs the ambient runtime
/// context does not panic here", and `Handle::current()` IS that requirement,
/// stated directly. A never-polled `Sleep` short-circuits its own
/// `TimerEntry::cancel` when its inner state was never initialised, so it would
/// drop cleanly with or without the guard and the test would be vacuous. This
/// payload cannot be vacuous: `Handle::current()` panics "there is no reactor
/// running, must be called from the context of a Tokio 1.x runtime" whenever the
/// guard is absent.
///
/// # Why the assertion is a FLAG and not the join result
///
/// The discard arm also wraps the drop in `catch_unwind` — (ak)'s subject — so a
/// panicking destructor is swallowed and `join()` succeeds either way. Only a
/// flag stored AFTER the `Handle::current()` call can tell "dropped cleanly"
/// from "panicked and was caught".
#[tokio::test]
async fn the_cancel_path_drops_the_future_inside_the_runtime_context() {
    use crate::large_stack::{JobSender, OnAbandon, dispatch_async};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    /// Stands in for any tokio resource whose destructor needs the ambient
    /// runtime — `Sleep`, `Interval`, `TcpStream`, anything holding a driver
    /// handle.
    struct NeedsRuntimeOnDrop {
        dropped_cleanly: Arc<AtomicBool>,
    }
    impl Drop for NeedsRuntimeOnDrop {
        fn drop(&mut self) {
            // Panics without an ambient runtime context; the store below is
            // therefore reached only when `handle.enter()` installed one.
            let _handle = tokio::runtime::Handle::current();
            self.dropped_cleanly.store(true, Ordering::SeqCst);
        }
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let sender = JobSender::new("test-enter", tx, OnAbandon::Discard);

    let dropped_cleanly = Arc::new(AtomicBool::new(false));
    let payload = NeedsRuntimeOnDrop {
        dropped_cleanly: Arc::clone(&dropped_cleanly),
    };

    let elapsed = tokio::time::timeout(
        Duration::from_millis(50),
        dispatch_async(Some(&sender), async move {
            // Captured, never polled: the discard arm drops it unrun.
            let _payload = payload;
            7u32
        }),
    )
    .await;
    assert!(
        elapsed.is_err(),
        "precondition: nothing drains this queue, so the await must elapse and \
         abandon the submission"
    );

    let job = rx.try_recv().expect("the abandoned request must have been ENQUEUED");

    // A plain `std` thread — NO ambient runtime, exactly like a lane consumer.
    std::thread::spawn(job)
        .join()
        .expect("the job itself must not unwind into the consumer's receive loop");

    assert!(
        dropped_cleanly.load(Ordering::SeqCst),
        "the discarded future's destructor must run INSIDE the runtime context. \
         It did not: `Handle::current()` panicked \"there is no reactor running\" \
         on the lane consumer's plain `std` thread and was swallowed by the \
         discard arm's `catch_unwind`, so the resource was never released. That \
         is what `let _enter = handle.enter();` in the cancel path prevents."
    );
}

/// (ak) A PANICKING destructor on the cancel path cannot kill the consumer, and
/// the same sender keeps working.
///
/// The discard arm's second guard. That arm runs only on an `OnAbandon::Discard`
/// destination — today `LSP_POOL` — so a consumer lost here is one of that
/// pool's: the pool silently narrows, and with it the head-of-line bound it
/// exists to provide, while neither `Lane::size` nor `Lane::started` shows the
/// loss. Without this test, deleting the `catch_unwind` leaves the suite green,
/// because no other submitted future in this file has a destructor that can
/// panic.
///
/// Two assertions, because the first alone is not the claim. That the job
/// returns cleanly says the unwind did not escape; that a LATER live submission
/// through the SAME sender still resolves says the queue survived it — which is
/// what a real consumer's receive loop would have had to do next.
#[tokio::test]
async fn a_panicking_destructor_on_the_cancel_path_does_not_kill_the_sender() {
    use crate::large_stack::{JobSender, OnAbandon, dispatch_async};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    struct PanicsOnDrop;
    impl Drop for PanicsOnDrop {
        fn drop(&mut self) {
            panic!("destructor of a discarded future");
        }
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let sender = JobSender::new("test-panic-drop", tx, OnAbandon::Discard);

    // CONSTRUCTED OUTSIDE the async block and moved in, so it is part of the
    // future's captured state rather than a local. A value bound inside the body
    // does not exist until the future is polled — and the discard arm never
    // polls it, so an inside-the-body payload would make this test vacuous.
    let payload = PanicsOnDrop;

    let elapsed = tokio::time::timeout(
        Duration::from_millis(50),
        dispatch_async(Some(&sender), async move {
            let _payload = payload;
            7u32
        }),
    )
    .await;
    assert!(
        elapsed.is_err(),
        "precondition: nothing drains this queue, so the await must elapse and \
         abandon the submission"
    );

    let doomed = rx.try_recv().expect("the abandoned request must have been ENQUEUED");

    // The hook is left alone deliberately: it is process-global, and this binary
    // runs its tests in parallel, so swapping it would suppress an unrelated
    // test's panic message. The expected backtrace on stderr is the cheaper
    // cost.
    let joined = std::thread::spawn(doomed).join();

    assert!(
        joined.is_ok(),
        "a panicking destructor on the discard path must be CAUGHT inside the \
         job. It escaped, which on a real lane unwinds the consumer's receive \
         loop and takes that consumer out of the pool for the process lifetime."
    );

    // Second half: the queue is still usable. A live submission, drained and run
    // by a helper thread exactly as (ai) does.
    let polled = Arc::new(AtomicBool::new(false));
    let polled_in_fut = Arc::clone(&polled);
    let drainer = std::thread::spawn(move || {
        let job = rx.recv().expect("the follow-up submission must be enqueued");
        job();
    });

    let value = tokio::time::timeout(
        Duration::from_secs(10),
        dispatch_async(Some(&sender), async move {
            polled_in_fut.store(true, Ordering::SeqCst);
            4242u32
        }),
    )
    .await
    .expect("the sender must still serve a live submission after the panicking drop");

    drainer.join().expect("the drainer thread must not panic");

    assert_eq!(value, 4242, "the follow-up submission must deliver its value unchanged");
    assert!(
        polled.load(Ordering::SeqCst),
        "the follow-up submission's future must actually be POLLED"
    );
}

/// (al) `Lane::started()` reports 0 before creation and the full `size` after —
/// on a TEST-LOCAL pool, so both halves are observable.
///
/// `Lane::size()` is what a lane DECLARES; `Lane::started()` is what it got.
/// They diverge exactly when `Lane::sender` hit a partial spawn failure, which
/// it deliberately survives (a pool with three of four consumers still drains
/// its queue on a large stack, so degrading it would be strictly worse). The
/// cost of surviving it silently is that a pool which started 1 of 4 consumers
/// serializes every LSP query again — the exact regression task 6517 exists to
/// prevent — while `size()` still reports 4 and every routing test stays green.
///
/// `lsp_bridge_tests`' (p) asserts the AFTER half against the production
/// `LSP_POOL`. It cannot assert the BEFORE half: `LSP_POOL` is process-wide and
/// this binary runs its tests in parallel, so another test may already have
/// created it. A lane declared inside this fn body is touched by nothing else,
/// which is what makes "0 before, `size` after" a fact here rather than a race.
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

    // Strict equality, with the diagnostic carrying the triage rather than the
    // assertion being softened — `>= 1` cannot see the silent narrowing this
    // exists to catch, because that narrowing IS a count between 1 and `SIZE`.
    // See (p)'s twin in `lsp_bridge_tests` for the same reasoning at length.
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
