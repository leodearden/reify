//! Unit tests for [`crate::large_stack`] — the defense-in-depth helper that
//! runs the GUI's synchronous compile calls on a dedicated OS thread with an
//! explicit LARGE stack (task 5357, belt-and-suspenders atop task 5337's
//! compiler-layer `stacker::maybe_grow` + recursion-depth cap).
//!
//! ## Why the deep-recursion tests are safe (no "violent RED")
//!
//! The large-stack property is proven by [`deep_recurse`], which pins ~8 KiB of
//! stack per frame and recurses ~2048 deep (~16 MiB — 8x the compiler's 2 MiB
//! default worker stack). Running that on a default-stack thread would SIGSEGV
//! and abort the *entire* test binary. To avoid that, the recursion is invoked
//! ONLY through the large-stack helpers, never on a default-stack thread. At RED
//! the helper symbol does not exist, so the test binary fails to COMPILE (a clean
//! compile-error RED — the recursion never executes). At GREEN the helper supplies
//! the large stack, so the recursion survives. An impl lacking `stack_size` would
//! abort the deep-recursion test, so the test genuinely drives the feature.
//!
//! That argument covers the persistent-worker section below (task 5772): its
//! deep-recursion test invokes the recursion ONLY through
//! `large_stack::run_on_worker`, whose symbol is absent at RED — so that RED is
//! likewise a clean compile error.
//!
//! One CORRECTION to the argument above, found while driving 5772's step-3 RED:
//! "invoked through a large-stack helper" does NOT by itself imply "runs on a
//! large stack". Every helper documents an INLINE-degradation arm that hands the
//! closure back to the CALLER's default-size stack — `run_on_large_stack` when
//! the OS refuses the 256 MiB mapping, `run_on_worker` additionally when the
//! worker is dead. Driving the recursion through a degraded helper overflowed
//! and SIGABRTed the whole test binary, taking every other test's result with
//! it. [`deep_recurse_if_on_thread`] closes that hole for the worker tier by
//! CHECKING the thread before recursing, so a degraded helper yields a clean
//! assertion failure instead. The two task-5357 tests still call [`deep_recurse`]
//! directly; their degradation arm needs the OS to refuse a mapping, which no
//! test can provoke, so they are left as 5357 wrote them.

/// A recursive frame that pins ~8 KiB of live stack per call and USES the
/// recursive result (non-tail), defeating tail-call optimization and dead-frame
/// elision. `#[inline(never)]` keeps each level a real call frame; the
/// `black_box`ed 8 KiB buffer forces the optimizer to materialize the frame.
///
/// `deep_recurse(n) == n + 1` (base case returns 1, each of the `n` recursive
/// frames adds `buf[8191] == 1`), so callers get a deterministic sentinel proving
/// the recursion ran to completion rather than being elided.
#[inline(never)]
fn deep_recurse(depth: u32) -> u64 {
    // 8 KiB per frame. Touch both ends so the whole buffer is committed and the
    // frame cannot be elided.
    let mut buf = [0u8; 8192];
    buf[0] = 1;
    buf[8191] = 1;
    let buf = std::hint::black_box(buf);
    if depth == 0 {
        return u64::from(buf[0]); // sentinel base == 1
    }
    // Use the recursive result (non-tail) so the frame stays live across the call.
    let below = deep_recurse(depth - 1);
    std::hint::black_box(below + u64::from(buf[8191]))
}

/// Depth for the deep-recursion survival tests: ~8 KiB/frame x 2048 ≈ 16 MiB,
/// i.e. 8x the 2 MiB default stack (a no-`stack_size` impl overflows) and 16x
/// under the 256 MiB `COMPILE_STACK_SIZE` constant (GREEN is reliable).
const DEEP_RECURSION_DEPTH: u32 = 2048;

/// Recurse ~16 MiB ONLY if we genuinely landed on the expected large-stack
/// thread; otherwise report where we actually are, without recursing.
///
/// "Invoked through a large-stack helper" does NOT by itself imply "runs on a
/// large stack": every helper documents an INLINE-degradation arm that hands the
/// closure back to the CALLER's default-size stack — `run_on_large_stack` when
/// the OS refuses the 256 MiB mapping, `run_on_worker` additionally when the
/// queue is dead. Recursing there overflows and SIGABRTs the whole test binary,
/// taking every other test's result with it (observed while driving task 5772's
/// step-3 RED, where a panicking job had killed the worker).
///
/// Checking first is what makes the module docs' "no violent RED" claim true by
/// CONSTRUCTION rather than by assumption: a degraded helper now yields a clean
/// assertion failure naming the thread it ran on.
fn deep_recurse_if_on_thread(expected_name: &'static str, depth: u32) -> Result<u64, String> {
    let actual = std::thread::current().name().map(str::to_owned);
    if actual.as_deref() != Some(expected_name) {
        return Err(format!(
            "refusing to recurse ~16 MiB on thread {actual:?}: expected the \
             large-stack thread {expected_name:?}. The helper degraded to an \
             inline call, so recursing here would overflow a default-size stack \
             and abort the entire test binary."
        ));
    }
    Ok(deep_recurse(depth))
}

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

/// (a) `run_on_large_stack` returns the closure's computed value, runs the
/// closure on a DISTINCT thread (not the caller), and permits the closure to
/// borrow a caller-stack local by reference (proving the non-`'static` scoped
/// design — no move, no `Arc` clone required).
#[test]
fn run_on_large_stack_returns_value_and_runs_on_distinct_thread() {
    use crate::large_stack::run_on_large_stack;

    let caller_id = std::thread::current().id();
    // A local owned by the caller's stack; the closure borrows it by reference.
    let data = [1u64, 2, 3, 4];

    let (sum, inner_id) = run_on_large_stack(|| {
        // Borrow `data` — no move, no `'static` bound. Only compiles if the
        // helper uses a scoped thread.
        let s: u64 = data.iter().sum();
        (s, std::thread::current().id())
    });

    assert_eq!(
        sum, 10,
        "closure return value must be propagated to the caller"
    );
    assert_ne!(
        inner_id, caller_id,
        "closure must execute on a distinct (large-stack) thread, not the caller"
    );
    // `data` is still usable here — the borrow ended when the helper returned.
    assert_eq!(
        data.len(),
        4,
        "borrowed local must remain owned by the caller"
    );
}

/// (b) A panic inside the closure propagates OUT of `run_on_large_stack`
/// (faithful panic semantics via `resume_unwind`), rather than being swallowed
/// or aborting the process.
#[test]
fn run_on_large_stack_propagates_closure_panic() {
    use crate::large_stack::run_on_large_stack;
    use std::panic::AssertUnwindSafe;

    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // Concrete `T = ()` so inference is unambiguous; the closure never
        // returns normally, but the panic must still cross the thread boundary.
        run_on_large_stack::<_, ()>(|| panic!("boom"));
    }));

    assert!(
        result.is_err(),
        "a panic inside the closure must propagate out of run_on_large_stack"
    );
}

/// (c) Deep recursion (~16 MiB) that would overflow the 2 MiB default stack runs
/// to completion when driven through `run_on_large_stack`. The recursion runs
/// ONLY on the helper's large-stack thread, so at RED (helper absent) this is a
/// compile error, never a SIGSEGV.
#[test]
fn run_on_large_stack_survives_deep_recursion_over_default_stack() {
    use crate::large_stack::run_on_large_stack;

    let result = run_on_large_stack(|| deep_recurse(DEEP_RECURSION_DEPTH));

    assert_eq!(
        result,
        u64::from(DEEP_RECURSION_DEPTH) + 1,
        "deep recursion must run to completion on the large stack (deep_recurse(n) == n + 1)"
    );
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

/// (f) Both helpers NAME their thread, so a panic backtrace, `RUST_BACKTRACE`
/// dump, `top -H` row or debugger thread list identifies compile-bearing work
/// instead of reading `<unnamed>`.
///
/// This matters precisely because this module RELOCATES the work most likely to
/// crash (stack overflow, OCCT kernel failure) off the caller's thread, which
/// would otherwise have carried a meaningful Tauri-command / tokio-worker name.
#[test]
fn large_stack_threads_are_named_for_observability() {
    use crate::large_stack::{
        COMPILE_THREAD_NAME, ENGINE_THREAD_NAME, run_on_large_stack, spawn_on_large_stack,
    };
    use std::sync::mpsc;

    // Blocking helper: read the name from inside the worker.
    let blocking_name = run_on_large_stack(|| std::thread::current().name().map(str::to_owned));
    assert_eq!(
        blocking_name.as_deref(),
        Some(COMPILE_THREAD_NAME),
        "run_on_large_stack's thread must be named for panic backtraces / profilers"
    );

    // Fire-and-forget helper: same, reported out-of-band via a channel.
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

// ── Persistent large-stack worker (task 5772) ────────────────────────────────
//
// The third tier. `run_on_large_stack` / `spawn_on_large_stack` each spawn a
// FRESH 256 MiB thread per call; 256 MiB is far above glibc's ~40 MiB
// thread-stack cache ceiling, so that mapping is never recycled and every call
// pays a full `mmap` + guard-page `mprotect` + `munmap`. Negligible against a
// compile, pure overhead on the per-frame projection commands (`set_parameter`
// fires per slider-drag frame). `run_on_worker` amortises it: ONE process-wide
// large-stack thread, fed by a job queue, for the process lifetime.
//
// These tests pin exactly that difference — same large stack, different mapping
// LIFETIME — plus the observability name the long-lived thread earns.

/// (g) `run_on_worker` returns the closure's computed value and runs it on a
/// thread DISTINCT from the caller, named [`crate::large_stack::WORKER_THREAD_NAME`].
///
/// Note the closure MOVES its captured data (`'static` bound) rather than
/// borrowing a caller-stack local as `run_on_large_stack`'s scoped design
/// permits — that is the deliberate API price of a persistent worker, and this
/// test pins it by construction.
#[test]
fn run_on_worker_returns_value_and_runs_on_named_distinct_thread() {
    use crate::large_stack::{WORKER_THREAD_NAME, run_on_worker};

    let caller_id = std::thread::current().id();
    // Owned by the caller and MOVED into the job — the worker outlives this
    // frame, so it cannot borrow from it.
    let data = vec![1u64, 2, 3, 4];

    let (sum, inner_id, inner_name) = run_on_worker(move || {
        let s: u64 = data.iter().sum();
        (
            s,
            std::thread::current().id(),
            std::thread::current().name().map(str::to_owned),
        )
    });

    assert_eq!(
        sum, 10,
        "closure return value must be propagated back to the submitter"
    );
    assert_ne!(
        inner_id, caller_id,
        "closure must execute on the worker thread, not the submitter"
    );
    assert_eq!(
        inner_name.as_deref(),
        Some(WORKER_THREAD_NAME),
        "the persistent worker must be named — it is long-lived, so it appears \
         in every profiler capture and thread-list dump for the whole process"
    );
}

/// (h) PERSISTENCE — the property that names this tier. Three successive
/// `run_on_worker` calls all land on the SAME thread (one saved 256 MiB
/// mapping), whereas two `run_on_large_stack` calls land on DIFFERENT threads
/// (a fresh mapping each).
///
/// Both halves are deterministic: `ThreadId`s are guaranteed never to be reused
/// within a process, even after a thread terminates, so an equal pair proves
/// reuse and an unequal pair proves a fresh spawn.
#[test]
fn run_on_worker_reuses_one_persistent_thread_unlike_run_on_large_stack() {
    use crate::large_stack::{run_on_large_stack, run_on_worker};

    let caller_id = std::thread::current().id();
    let first = run_on_worker(|| std::thread::current().id());
    let second = run_on_worker(|| std::thread::current().id());
    let third = run_on_worker(|| std::thread::current().id());

    // Non-vacuity: if the helper had degraded to inline execution, all three
    // would trivially be equal — to the CALLER's own id. Rule that out first,
    // so "all equal" can only mean "one real worker served all three".
    assert_ne!(
        first, caller_id,
        "jobs must run on a worker thread, not degrade to an inline call on the caller"
    );

    assert_eq!(
        first, second,
        "consecutive run_on_worker jobs must run on the SAME persistent thread"
    );
    assert_eq!(
        second, third,
        "the worker must stay the same thread across every submission"
    );

    // The explicit contrast: the per-call tier re-spawns every time. This is
    // the cost `run_on_worker` exists to amortise away on high-frequency paths.
    let per_call_a = run_on_large_stack(|| std::thread::current().id());
    let per_call_b = run_on_large_stack(|| std::thread::current().id());
    assert_ne!(
        per_call_a, per_call_b,
        "run_on_large_stack must keep its per-call spawn (a fresh thread each call)"
    );
    assert_ne!(
        per_call_a, first,
        "the per-call tier must not be silently delegating to the shared worker"
    );
}

/// (i) LARGE STACK — deep recursion (~16 MiB) that would overflow the 2 MiB
/// default stack runs to completion on the persistent worker, proving
/// [`crate::large_stack::COMPILE_STACK_SIZE`] is applied to it and not just to
/// the per-call helpers.
///
/// The recursion runs ONLY through the helper, and only once
/// [`deep_recurse_if_on_thread`] has confirmed the helper did not degrade to an
/// inline call — so neither an absent symbol nor a dead worker can turn this
/// into a SIGSEGV. See the module docs.
#[test]
fn run_on_worker_survives_deep_recursion_over_default_stack() {
    use crate::large_stack::{WORKER_THREAD_NAME, run_on_worker};

    let result =
        run_on_worker(|| deep_recurse_if_on_thread(WORKER_THREAD_NAME, DEEP_RECURSION_DEPTH));

    let depth_reached = result.unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(
        depth_reached,
        u64::from(DEEP_RECURSION_DEPTH) + 1,
        "deep recursion must run to completion on the persistent worker's large stack"
    );
}

/// (j) [`crate::large_stack::WORKER_THREAD_NAME`] fits Linux's 15-byte
/// `pthread_setname_np` budget and is DISTINCT from both per-call tier names, so
/// a profiler row or backtrace says which tier the work arrived on.
#[test]
fn worker_thread_name_is_distinct_and_fits_the_linux_budget() {
    use crate::large_stack::{COMPILE_THREAD_NAME, ENGINE_THREAD_NAME, WORKER_THREAD_NAME};

    assert!(
        WORKER_THREAD_NAME.len() <= 15,
        "thread name must fit Linux's 15-byte pthread_setname_np limit \
         (std SILENTLY ignores an over-long name), got {} bytes: {WORKER_THREAD_NAME:?}",
        WORKER_THREAD_NAME.len()
    );
    assert_ne!(
        WORKER_THREAD_NAME, COMPILE_THREAD_NAME,
        "the persistent worker must be distinguishable from the per-call compile thread"
    );
    assert_ne!(
        WORKER_THREAD_NAME, ENGINE_THREAD_NAME,
        "the persistent worker must be distinguishable from the fire-and-forget engine thread"
    );
}

// ── Shared-worker robustness (task 5772) ─────────────────────────────────────
//
// The properties a PER-CALL spawn never needed. A per-call thread's death costs
// exactly one call; the shared worker's would cost every future one in the
// process, so a single poisoned job must not disable the mechanism for
// everybody else.
//
// RED shape (no hang, by construction). Before the worker runs jobs under
// `catch_unwind`, a panicking job unwinds the worker thread, dropping the
// `Receiver`. Every later `send` then returns `SendError`, and `run_on_worker`'s
// inline-recovery arm runs the recovered job on the submitter — so (l) fails
// deterministically on a ThreadId mismatch rather than blocking. (k) likewise
// fails on the payload assertion: the submitter sees its reply channel
// disconnect and raises "large-stack worker died", not the original "boom".
//
// (m) is a GREEN-side guarantee. Because the worker is PROCESS-WIDE, whether it
// is already poisoned when (m) runs depends on test-thread interleaving with (k)
// and (l), so at RED it may pass or fail — it cannot hang either way. (l) is the
// deterministic RED for panic isolation.

/// (k) A panicking job propagates the panic to ITS submitter, carrying the
/// ORIGINAL payload — the same faithful semantics
/// `run_on_large_stack_propagates_closure_panic` pins for the scoped tier.
///
/// The payload assertion is the load-bearing half: a worker that merely died
/// would also make `catch_unwind` return `Err`, just with the helper's
/// "worker died" message instead of the closure's own.
#[test]
fn run_on_worker_propagates_the_original_job_panic_to_its_submitter() {
    use crate::large_stack::run_on_worker;
    use std::panic::AssertUnwindSafe;

    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // Concrete `T = ()` so inference is unambiguous; the closure never
        // returns normally, but the panic must still cross back over the queue.
        run_on_worker::<_, ()>(|| panic!("boom"));
    }));

    let payload = result.expect_err("a panicking job must propagate out of run_on_worker");
    assert_eq!(
        panic_message(&*payload),
        "boom",
        "the submitter must receive the JOB's original panic payload, not a \
         substitute raised by the helper"
    );
}

/// (l) The worker SURVIVES a panicking job. This is the whole difference between
/// a shared worker and a per-call spawn: one poisoned job must not disable the
/// mechanism for every future caller in the process.
///
/// `ThreadId`s are never reused, so an equal pair across the panic proves the
/// SAME thread kept serving — not that a replacement was silently spun up.
#[test]
fn run_on_worker_survives_a_panicking_job() {
    use crate::large_stack::run_on_worker;
    use std::panic::AssertUnwindSafe;

    let caller_id = std::thread::current().id();
    let before = run_on_worker(|| std::thread::current().id());
    // Non-vacuity: a helper that had ALREADY degraded to inline execution would
    // report the caller's own id both before and after, passing this test while
    // proving nothing. Pin that the recorded id really is a worker's.
    assert_ne!(
        before, caller_id,
        "the pre-panic job must run on a worker thread, not inline on the caller"
    );

    let poisoned = std::panic::catch_unwind(AssertUnwindSafe(|| {
        run_on_worker::<_, ()>(|| panic!("poisoned job"));
    }));
    assert!(
        poisoned.is_err(),
        "the panicking job must still surface as a panic on its submitter"
    );

    let (value, after) = run_on_worker(|| (7u32, std::thread::current().id()));
    assert_eq!(
        value, 7,
        "the worker must keep answering submissions after a poisoned job"
    );
    assert_eq!(
        after, before,
        "the SAME persistent worker thread must survive the panic — a per-call \
         spawn can afford to die, a shared one cannot"
    );
}

/// (m) Concurrent submitters each get their OWN result, and all of them run on
/// the one shared worker: no cross-talk between the per-call reply channels, and
/// no accidental second worker under contention.
#[test]
fn concurrent_submitters_share_one_worker_without_cross_talk() {
    use crate::large_stack::run_on_worker;

    const SUBMITTERS: u64 = 8;

    let handles: Vec<_> = (0..SUBMITTERS)
        .map(|i| {
            std::thread::spawn(move || {
                // A distinct closure per submitter, so a mis-routed reply shows
                // up as a wrong value rather than a coincidentally-equal one.
                let (doubled, worker_id) =
                    run_on_worker(move || (i * 2, std::thread::current().id()));
                (i, doubled, worker_id)
            })
        })
        .collect();

    let results: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().expect("submitter thread must not panic"))
        .collect();
    assert_eq!(
        results.len() as u64,
        SUBMITTERS,
        "every submitter must be accounted for"
    );

    for (i, doubled, _) in &results {
        assert_eq!(
            *doubled,
            i * 2,
            "submitter {i} received another submitter's result — the per-call \
             reply channels must not cross-talk"
        );
    }

    let (_, _, first_worker) = results[0];
    for (i, _, worker_id) in &results {
        assert_eq!(
            *worker_id, first_worker,
            "submitter {i} ran on a different thread — concurrent submissions \
             must all land on the ONE persistent worker"
        );
    }
}

// ── Degraded (no-worker) arm (task 5772) ─────────────────────────────────────
//
// Task 5357 DOCUMENTED `run_on_large_stack`'s inline fallback for a refused
// 256 MiB mapping but could not test it: `pthread_create` failure is not
// provokable from a unit test, so the policy rested on prose alone. The worker
// tier closes that gap by testing the SEAM instead of the OS — `dispatch(None,
// f)` is precisely the "no worker available" arm — so the behaviour every tier
// promises under stress is exercised rather than merely asserted.

/// (n) The `None` arm returns the closure's value AND runs it inline, i.e. the
/// closure observes the CALLER's own `ThreadId`.
///
/// Inline-ness is the load-bearing half: it is the precise claim the fallback
/// policy makes — "the worst case is exactly the pre-task-5357 behaviour, never
/// a lost result". A degraded arm that returned the right value from some other
/// thread would satisfy the value check and still break the promise.
#[test]
fn dispatch_without_a_worker_runs_the_closure_inline_on_the_caller() {
    use crate::large_stack::dispatch;

    let caller_id = std::thread::current().id();

    let (value, ran_on) = dispatch(None, || (99u32, std::thread::current().id()));

    assert_eq!(
        value, 99,
        "the degraded arm must still return the closure's value — never a lost result"
    );
    assert_eq!(
        ran_on, caller_id,
        "with no worker the closure must run INLINE on the caller's own stack"
    );
}

/// (o) A panic through the `None` arm still reaches the caller carrying its
/// ORIGINAL payload, so panic semantics do not silently change at the moment the
/// mechanism degrades — which is exactly when a caller can least afford a
/// surprise.
#[test]
fn dispatch_without_a_worker_still_propagates_panics() {
    use crate::large_stack::dispatch;
    use std::panic::AssertUnwindSafe;

    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        dispatch::<_, ()>(None, || panic!("degraded boom"));
    }));

    let payload = result.expect_err("a panic through the degraded arm must reach the caller");
    assert_eq!(
        panic_message(&*payload),
        "degraded boom",
        "the degraded arm must deliver the closure's ORIGINAL payload, like every other tier"
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

/// (p) The LSP lane runs its jobs on its OWN named thread
/// ([`crate::large_stack::LSP_WORKER_THREAD_NAME`]), distinct from the caller.
///
/// The name is the observability half: a long-lived thread appears in every
/// profiler capture and `top -H` listing for the process's whole life, so a lane
/// that reported `reify-engine-w` — or `<unnamed>` — would make a keystroke-path
/// stall indistinguishable from a geometry-evaluation stall.
#[test]
fn lsp_lane_runs_jobs_on_its_own_named_thread() {
    use crate::large_stack::{LSP_LANE, LSP_WORKER_THREAD_NAME, dispatch};

    let caller_id = std::thread::current().id();
    // Owned and MOVED — a lane outlives the frame that submitted to it, exactly
    // as the engine lane's `'static` bound requires.
    let data = vec![10u64, 20, 30];

    let (sum, inner_id, inner_name) = dispatch(LSP_LANE.sender(), move || {
        let s: u64 = data.iter().sum();
        (
            s,
            std::thread::current().id(),
            std::thread::current().name().map(str::to_owned),
        )
    });

    assert_eq!(
        sum, 60,
        "the LSP lane must propagate its closure's value back to the submitter"
    );
    assert_ne!(
        inner_id, caller_id,
        "the LSP lane must run its job on a lane thread, not degrade to an inline call"
    );
    assert_eq!(
        inner_name.as_deref(),
        Some(LSP_WORKER_THREAD_NAME),
        "the LSP lane's thread must carry its OWN name, so a profiler row says \
         which lane stalled"
    );
}

/// (q) [`crate::large_stack::LSP_WORKER_THREAD_NAME`] is distinct from every
/// other large-stack thread name and fits Linux's 15-byte `pthread_setname_np`
/// budget (`std` SILENTLY ignores an over-long name, so this must be asserted
/// rather than assumed).
#[test]
fn lsp_worker_thread_name_is_distinct_and_fits_the_linux_budget() {
    use crate::large_stack::{
        COMPILE_THREAD_NAME, ENGINE_THREAD_NAME, LSP_WORKER_THREAD_NAME, WORKER_THREAD_NAME,
    };

    assert!(
        LSP_WORKER_THREAD_NAME.len() <= 15,
        "thread name must fit Linux's 15-byte pthread_setname_np limit, got {} bytes: \
         {LSP_WORKER_THREAD_NAME:?}",
        LSP_WORKER_THREAD_NAME.len()
    );
    for (other, what) in [
        (WORKER_THREAD_NAME, "the persistent ENGINE lane"),
        (COMPILE_THREAD_NAME, "the per-call compile thread"),
        (ENGINE_THREAD_NAME, "the fire-and-forget engine thread"),
    ] {
        assert_ne!(
            LSP_WORKER_THREAD_NAME, other,
            "the LSP lane must be distinguishable from {what}"
        );
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
#[test]
fn the_two_lanes_are_separate_threads_each_amortised() {
    use crate::large_stack::{LSP_LANE, dispatch, run_on_worker};

    let caller_id = std::thread::current().id();

    let engine_a = run_on_worker(|| std::thread::current().id());
    let engine_b = run_on_worker(|| std::thread::current().id());
    let lsp_a = dispatch(LSP_LANE.sender(), || std::thread::current().id());
    let lsp_b = dispatch(LSP_LANE.sender(), || std::thread::current().id());

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

/// (s) LARGE STACK — the LSP lane survives ~16 MiB of recursion, 8x the 2 MiB
/// default a tokio worker gives it today. An impl that built the lane without
/// [`crate::large_stack::COMPILE_STACK_SIZE`] fails here.
///
/// Per the module docs' "no violent RED" doctrine the recursion is reached ONLY
/// through [`deep_recurse_if_on_thread`], so a degraded lane yields a clean
/// assertion failure instead of overflowing and SIGABRTing the whole binary.
#[test]
fn lsp_lane_survives_deep_recursion_over_default_stack() {
    use crate::large_stack::{LSP_LANE, LSP_WORKER_THREAD_NAME, dispatch};

    let result = dispatch(LSP_LANE.sender(), || {
        deep_recurse_if_on_thread(LSP_WORKER_THREAD_NAME, DEEP_RECURSION_DEPTH)
    });

    let depth_reached = result.unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(
        depth_reached,
        u64::from(DEEP_RECURSION_DEPTH) + 1,
        "deep recursion must run to completion on the LSP lane's large stack"
    );
}

/// (t) The LSP lane inherits the engine lane's panic isolation: a panicking job
/// re-raises its ORIGINAL payload on ITS submitter, and the lane thread SURVIVES
/// to serve later submissions.
///
/// This is the property that must not be lost when a mechanism is instantiated
/// twice. A poisoned LSP job that killed the lane would silently downgrade every
/// FUTURE keystroke to inline execution on a ~2 MiB tokio stack — the exact
/// hazard the module exists to remove, re-introduced through the back door.
#[test]
fn lsp_lane_is_panic_isolated_and_survives() {
    use crate::large_stack::{LSP_LANE, dispatch};
    use std::panic::AssertUnwindSafe;

    let caller_id = std::thread::current().id();
    let before = dispatch(LSP_LANE.sender(), || std::thread::current().id());
    assert_ne!(
        before, caller_id,
        "the pre-panic job must run on the lane, not inline on the caller"
    );

    let poisoned = std::panic::catch_unwind(AssertUnwindSafe(|| {
        dispatch::<_, ()>(LSP_LANE.sender(), || panic!("lsp boom"));
    }));
    let payload = poisoned.expect_err("a panicking LSP-lane job must reach its submitter");
    assert_eq!(
        panic_message(&*payload),
        "lsp boom",
        "the submitter must receive the JOB's original payload, not a substitute"
    );

    let (value, after) = dispatch(LSP_LANE.sender(), || (5u32, std::thread::current().id()));
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
// `run_on_worker` parks its caller in `mpsc::recv()`. For the fourteen migrated
// commands that is free: they are sync `#[tauri::command] fn`s, which Tauri runs
// as `ExecutionContext::Blocking` on their own thread. `lsp_request` is an
// `async fn` on the tauri tokio runtime, so the same call would pin a runtime
// worker for a whole LSP round trip on EVERY keystroke — precisely what an async
// command must not do.
//
// So the LSP lane needs an async submission: box the job the same way, reply
// over a `tokio::sync::oneshot`, and `.await` it, releasing the tokio worker
// while the lane thread computes. Not a new pattern — `debug_server::run_on_engine`
// already bridges async-caller-to-large-stack-thread with exactly
// `spawn_on_large_stack` + `oneshot`; this amortises it onto a persistent lane.
//
// These tests pin the four properties that must survive the shape change: the
// value comes back, the runtime is NOT blocked, panics stay faithful, and the
// degraded arm still returns rather than hanging an `.await`.

/// (u) The async submission returns the closure's value, and the closure body
/// runs on the LSP lane's thread — not on a tokio worker, and not inline.
#[tokio::test]
async fn run_on_lsp_worker_returns_value_and_runs_on_the_lane() {
    use crate::large_stack::{LSP_WORKER_THREAD_NAME, run_on_lsp_worker};

    let caller_id = std::thread::current().id();
    let data = vec![1u64, 2, 3, 4, 5];

    let (sum, inner_id, inner_name) = run_on_lsp_worker(move || {
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
        "the async submission must use the SAME LSP lane as the blocking seam — \
         one mechanism, not a third"
    );
}

/// (v) Awaiting the submission does NOT block the calling runtime — the whole
/// reason this variant exists.
///
/// `#[tokio::test]` builds a CURRENT-THREAD runtime, which makes this sharp: a
/// `tokio::spawn`ed task only runs when the single thread is free to poll it. So
/// the assertion is an ORDERING one, not a timing one, and cannot pass by luck.
///
/// * A BLOCKING impl (what `run_on_worker` does — park in `mpsc::recv()`) makes
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
    let lane_result = run_on_lsp_worker(|| {
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
        run_on_lsp_worker::<_, ()>(|| panic!("async boom")).await;
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

/// (x) The DEGRADED arm of the async path: with no lane, the closure runs inline
/// on the caller and the `.await` still RESOLVES.
///
/// A refused 256 MiB mapping must never hang an `await`. The blocking seam's
/// `None` arm is already tested; this pins that the async variant inherits it
/// rather than reimplementing it, so both degradation arms are exercised by a
/// test instead of resting on prose.
#[tokio::test]
async fn async_dispatch_without_a_lane_runs_inline_and_still_resolves() {
    use crate::large_stack::dispatch_async;

    let caller_id = std::thread::current().id();

    let (value, ran_on) = dispatch_async(None, || (77u32, std::thread::current().id())).await;

    assert_eq!(
        value, 77,
        "the degraded async arm must still return the closure's value — never a \
         lost result, and never a hung await"
    );
    assert_eq!(
        ran_on, caller_id,
        "with no lane the closure must run INLINE on the caller's own stack"
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

    let result = run_on_lsp_worker(|| {
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
