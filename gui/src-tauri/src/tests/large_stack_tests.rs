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
    //
    // Deliberately a heap-owned `Vec`, NOT the `[1u64, 2, 3, 4]` array clippy's
    // `useless_vec` would suggest: `[u64; N]` is `Copy`, so the array spelling
    // would let the closure COPY the payload and the test would no longer
    // exercise the move that the `'static` bound actually forces.
    let data = Vec::from([1u64, 2, 3, 4]);

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

/// (o2) The `SendError` recovery arm of the BLOCKING lane: a job handed back by
/// a dead queue still runs, INLINE on the submitting frame, and its value
/// reaches the caller.
///
/// The mirror of (z), and a DIFFERENT code path from (n)'s `None` arm: `None`
/// means "there was never a lane", while this means "the lane existed and its
/// consumer is gone", which is reached only after the job has been boxed and
/// pushed. Both must honour the same "never lose a result" promise, and until
/// this test that half rested on prose.
///
/// Note the OPPOSITE expectation from (z): the blocking lane's jobs are plain
/// sync closures with a stated runtime-agnostic precondition (see `dispatch`),
/// so running the recovered job right here is legal and is the cheapest place to
/// run it. The async lane's job pre-bakes a `Handle::block_on` and therefore
/// must go off-frame — the two arms differ because their JOB TYPES differ, not
/// by oversight.
///
/// Provoked deterministically with a SYNTHETIC sender whose consumer is already
/// gone: no real lane, no `pthread_create` failure, no timing.
#[test]
fn dispatch_recovers_a_handed_back_job_inline_on_the_caller() {
    use crate::large_stack::{JobSender, dispatch};

    // Consumer dropped before any send: every `send` fails at once with
    // `SendError(job)`, which is the arm under test.
    let (tx, rx) = std::sync::mpsc::channel();
    drop(rx);
    let dead = JobSender::new("test-dead-blocking", tx);

    let caller_id = std::thread::current().id();

    let (value, ran_on) = dispatch(Some(&dead), || (99u32, std::thread::current().id()));

    assert_eq!(
        value, 99,
        "a job handed back by a dead queue must still be run and its value \
         delivered — degraded, never lost"
    );
    assert_eq!(
        ran_on, caller_id,
        "the blocking lane's recovered job runs INLINE in the submitting frame, \
         which its runtime-agnostic precondition makes legal"
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
    // as the engine lane's `'static` bound requires. Heap-owned `Vec` rather
    // than a `Copy` array, for the reason spelled out in
    // `run_on_worker_returns_value_and_runs_on_named_distinct_thread`.
    let data = Vec::from([10u64, 20, 30]);

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
/// EXTENDED for task 6517 to cover [`crate::large_stack::LSP_POOL_THREAD_PREFIX`].
/// The query pool is the first name here that is a PREFIX rather than a whole
/// thread name, which makes distinctness sharper, not looser: a prefix that
/// merely differed in a suffix — say `reify-lsp-w` against a pool prefixed
/// `reify-lsp-w` — would produce consumers named `reify-lsp-w0`, and a
/// `top -H` filter or profiler alert keyed on the ordered lane would then match
/// pool rows too. Pairwise inequality over the raw strings is the check that
/// rules that out at its root.
#[test]
fn large_stack_thread_names_are_pairwise_distinct() {
    use crate::large_stack::{
        COMPILE_THREAD_NAME, ENGINE_THREAD_NAME, LSP_POOL_THREAD_PREFIX, LSP_WORKER_THREAD_NAME,
        WORKER_THREAD_NAME,
    };

    let names = [
        (COMPILE_THREAD_NAME, "the per-call compile thread"),
        (ENGINE_THREAD_NAME, "the fire-and-forget engine thread"),
        (WORKER_THREAD_NAME, "the persistent ENGINE lane"),
        (LSP_WORKER_THREAD_NAME, "the persistent LSP lane"),
        (LSP_POOL_THREAD_PREFIX, "the LSP query pool's consumer prefix"),
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

/// (r2) A job that submits to the lane it is RUNNING ON gets a loud panic
/// carrying that lane's name — not the process-wide wedge the same code would
/// otherwise produce — and the lane keeps working afterwards.
///
/// The wedge is the module's worst possible outcome and the only failure mode it
/// could not resolve: a lane has a SINGLE consumer, so the inner job can only run
/// once the outer one returns, while the outer one blocks in `recv()` waiting for
/// it. The lane thread never returns to its `for job in rx` loop, so the lane is
/// dead AND every later submitter in the process blocks forever too — silently,
/// unrecoverably.
///
/// Both halves of the assertion are load-bearing. The PANIC is what replaces the
/// hang; the SURVIVAL is what makes panicking the right answer, and it is not
/// free — the check runs on the lane thread, inside the running job, so its
/// unwind is caught by that job's own `catch_unwind` and re-raised on the
/// submitter like any other job panic. A guard placed on the submitting side, or
/// one raised outside the job body, would kill the shared lane for everybody.
///
/// Not reachable from the fourteen migrated call sites; the guard exists because
/// the lane is SHARED and grows new callers (`main.rs::mcp_tool_call`, task 5466,
/// is already named as a future one).
///
/// UNLIKE the deep-recursion tests, this one cannot honour the module's "no
/// violent RED" doctrine: the failure it guards against is a wedge of a
/// process-wide `static` lane, so if the guard is ever removed this test hangs —
/// and so does every other engine-lane test in the binary, whatever this one
/// does. A timeout here would only make this test's report legible while the
/// rest of the suite hung anyway, so the honest note is this paragraph rather
/// than a wrapper that implies protection it cannot give.
#[test]
fn submitting_to_your_own_lane_panics_loudly_instead_of_wedging_it() {
    use crate::large_stack::{WORKER_THREAD_NAME, run_on_worker};
    use std::panic::AssertUnwindSafe;

    let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // The OUTER job runs on the engine lane; the inner submission targets
        // that same lane, which is the wedge.
        run_on_worker(|| run_on_worker(|| 1u32))
    }));

    let payload = outcome
        .expect_err("re-entrant submission must panic on the submitter rather than wedge the lane");
    let message = panic_message(&*payload);
    assert!(
        message.contains("re-entrant submission"),
        "the panic must name the reentrancy rather than surface as a generic \
         channel error, got: {message}"
    );
    assert!(
        message.contains(WORKER_THREAD_NAME),
        "the panic must name the LANE that was re-entered, got: {message}"
    );

    // Survival: the lane still answers. A wedged lane would hang here instead —
    // which is exactly why this assertion is placed after the panic one.
    assert_eq!(
        run_on_worker(|| 7u32),
        7,
        "the lane must survive a rejected re-entrant submission and keep serving \
         every other caller in the process"
    );
}

/// (r3) The guard is PER-LANE, not blanket: a job running on one lane may submit
/// to the OTHER one, because that lands on a different thread with its own
/// consumer.
///
/// This is the other half of (r2), and it is what makes the guard a correctness
/// check rather than a blunt "no submitting from a lane thread" rule that would
/// reject a legal composition. Asserting the inner job's `ThreadId` — rather than
/// just that it returned — is what pins that it genuinely crossed lanes instead
/// of quietly degrading to an inline call on the outer lane's thread.
#[test]
fn a_job_on_one_lane_may_submit_to_the_other_lane() {
    use crate::large_stack::{LSP_LANE, dispatch, run_on_worker};

    let (outer, inner) = run_on_worker(|| {
        let outer = std::thread::current().id();
        let inner = dispatch(LSP_LANE.sender(), || std::thread::current().id());
        (outer, inner)
    });

    assert_ne!(
        outer, inner,
        "a cross-lane submission must run on the OTHER lane's thread — the guard \
         must not reject it, and it must not degrade to an inline call"
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
    // Owned and MOVED into the job, heap-owned `Vec` rather than a `Copy`
    // array, for the reason spelled out in
    // `run_on_worker_returns_value_and_runs_on_named_distinct_thread`.
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
/// A refused 256 MiB mapping must never hang an `await`. The blocking seam's
/// `None` arm is already tested; this pins that the async variant inherits it
/// rather than reimplementing it, so both degradation arms are exercised by a
/// test instead of resting on prose.
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

/// (r4) The ASYNC half of (r2): a FUTURE that submits to the lane it is being
/// DRIVEN on gets the same loud panic naming that lane, and the lane survives.
///
/// (r2) covers the BLOCKING seam. This is not a duplicate of it, because the
/// unwind takes a materially different route and it is the route the module's
/// docs lean on hardest. The panic must escape the INNER future mid-poll,
/// unwind out of [`tokio::runtime::Handle::block_on`] (whose `enter_runtime`
/// guard has to restore the runtime context on the way out), be caught by the
/// OUTER job's own `catch_unwind`, ride back over the `tokio::sync::oneshot`,
/// and be `resume_unwind`-ed on the awaiting submitter. Any one of those links
/// failing turns a loud rejection back into the process-wide wedge the guard
/// exists to replace — which is the module's stated worst outcome, so the async
/// half deserves the same guard the blocking half got.
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
/// Carries (r2)'s caveat unchanged: if the guard is ever removed this test hangs
/// rather than failing, because the wedge is of a process-wide `static` lane.
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
// `AtomicUsize` arrival counter plus a generous wall-clock DEADLINE: a lane that
// failed to run N jobs at once makes the counter stall, the deadline elapses,
// and the job returns `false` — a clean assertion failure naming what it saw.
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

/// Submit `n` jobs to `lane` from `n` SEPARATE submitter threads, each of which
/// increments a shared arrival counter and then parks until every one of the `n`
/// has arrived — or until a wall-clock deadline elapses.
///
/// Returns, per job, whether it observed all `n` in flight AT ONCE. On a
/// single-consumer lane job 1 parks holding the only consumer, jobs 2..n never
/// start, the deadline elapses and job 1 reports `false` — a bounded assertion
/// failure, never a hang, which is the property this whole file is written to.
///
/// Factored out because it is the measurement BOTH (aa) and (ae) need: (aa)
/// establishes the concurrency, (ae) re-establishes it after a panic to prove no
/// consumer was lost. Writing it twice would let the two drift.
fn observe_concurrent_arrivals(lane: &'static crate::large_stack::Lane, n: usize) -> Vec<bool> {
    use crate::large_stack::dispatch;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Generous relative to the work (an atomic increment), so only a genuine
    /// serialization can exhaust it. It is a liveness BACKSTOP, not the
    /// property under test — see the section header.
    const ARRIVAL_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

    let arrived = Arc::new(AtomicUsize::new(0));
    let submitters: Vec<_> = (0..n)
        .map(|_| {
            let arrived = Arc::clone(&arrived);
            std::thread::spawn(move || {
                dispatch(lane.sender(), move || {
                    arrived.fetch_add(1, Ordering::SeqCst);
                    let deadline = std::time::Instant::now() + ARRIVAL_DEADLINE;
                    while arrived.load(Ordering::SeqCst) < n {
                        if std::time::Instant::now() >= deadline {
                            return false;
                        }
                        std::thread::yield_now();
                    }
                    true
                })
            })
        })
        .collect();

    submitters
        .into_iter()
        .map(|h| h.join().expect("a submitter thread must not panic"))
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
    use crate::large_stack::{Lane, dispatch};
    use std::collections::HashSet;

    const PREFIX: &str = "t6517-name-";
    const SIZE: usize = 2;
    static POOL: Lane = Lane::pool(PREFIX, SIZE);

    let caller_id = std::thread::current().id();
    let expected_names: HashSet<String> = (0..SIZE).map(|i| format!("{PREFIX}{i}")).collect();

    let mut seen_ids = HashSet::new();
    let mut seen_names = HashSet::new();
    for _ in 0..24 {
        let (id, name) = dispatch(POOL.sender(), || {
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
    use crate::large_stack::{
        LSP_LANE, LSP_WORKER_THREAD_NAME, WORKER_THREAD_NAME, dispatch, run_on_worker,
    };

    let engine = run_on_worker(|| std::thread::current().name().map(str::to_owned));
    assert_eq!(
        engine.as_deref(),
        Some(WORKER_THREAD_NAME),
        "the size-1 ENGINE lane must keep its exact name — no `0` suffix"
    );

    let lsp = dispatch(LSP_LANE.sender(), || {
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
    use crate::large_stack::{Lane, dispatch};

    const PREFIX: &str = "t6517-deep-";
    static POOL: Lane = Lane::pool(PREFIX, 2);

    let result = dispatch(POOL.sender(), || {
        deep_recurse_if_on_lane(PREFIX, DEEP_RECURSION_DEPTH)
    });

    let depth_reached = result.unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(
        depth_reached,
        u64::from(DEEP_RECURSION_DEPTH) + 1,
        "deep recursion must run to completion on a pool consumer's large stack"
    );
}

/// (ae) A panicking pool job re-raises its ORIGINAL payload on ITS submitter,
/// and the pool afterwards still runs `size` jobs CONCURRENTLY.
///
/// The second half is what makes this more than a re-run of (t) against a new
/// instance. A pool has N consumers, so "it still answers" is satisfied by a
/// pool that lost N-1 of them — the panic would have silently converted the
/// bounded-blocking guarantee back into the total serialization task 6517 exists
/// to remove, while every simple survival assertion stayed green. Re-measuring
/// full concurrency is the only assertion that can see that.
#[test]
fn a_pool_lane_is_panic_isolated_and_keeps_all_its_consumers() {
    use crate::large_stack::{Lane, dispatch};
    use std::panic::AssertUnwindSafe;

    const SIZE: usize = 3;
    static POOL: Lane = Lane::pool("t6517-panic", SIZE);

    let before = observe_concurrent_arrivals(&POOL, SIZE);
    assert!(
        before.iter().all(|saw_all| *saw_all),
        "precondition: the pool must run {SIZE} jobs at once BEFORE the panic, \
         got {before:?}"
    );

    let poisoned = std::panic::catch_unwind(AssertUnwindSafe(|| {
        dispatch::<_, ()>(POOL.sender(), || panic!("pool boom"));
    }));
    let payload = poisoned.expect_err("a panicking pool job must reach its submitter");
    assert_eq!(
        panic_message(&*payload),
        "pool boom",
        "the submitter must receive the JOB's original payload, not a substitute"
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
/// Carries (r2)'s caveat unchanged: if the guard is ever removed this test HANGS
/// rather than failing. It is bounded here in a way (r2) is not — the wedged
/// lane is test-local, so the damage cannot escape into another test — but the
/// test itself would still not terminate.
#[test]
fn submitting_to_your_own_pool_panics_loudly_instead_of_wedging_it() {
    use crate::large_stack::{Lane, dispatch};
    use std::panic::AssertUnwindSafe;

    const PREFIX: &str = "t6517-reent-";
    static POOL: Lane = Lane::pool(PREFIX, 2);

    let poisoned = std::panic::catch_unwind(AssertUnwindSafe(|| {
        dispatch(POOL.sender(), || dispatch(POOL.sender(), || 1u32))
    }));
    let payload = poisoned.expect_err(
        "a re-entrant pool submission must panic on its submitter rather than \
         wedge the pool",
    );
    let message = panic_message(&*payload);
    assert!(
        message.contains("re-entrant submission"),
        "the panic must name the reentrancy rather than surface as a generic \
         channel error, got: {message}"
    );
    assert!(
        message.contains(PREFIX),
        "the panic must name the LANE that was re-entered, got: {message}"
    );

    let (value, name) = dispatch(POOL.sender(), || {
        (7u32, std::thread::current().name().map(str::to_owned))
    });
    assert_eq!(
        value, 7,
        "the pool must survive a rejected re-entrant submission and keep serving"
    );
    assert!(
        name.as_deref().is_some_and(|n| n.starts_with(PREFIX)),
        "the surviving pool must still be the POOL — a degraded inline call \
         would answer from the submitter's thread instead, got {name:?}"
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
#[test]
fn a_pool_job_may_submit_to_another_lane() {
    use crate::large_stack::{Lane, WORKER_THREAD_NAME, dispatch, run_on_worker};

    const PREFIX: &str = "t6517-cross-";
    static POOL: Lane = Lane::pool(PREFIX, 2);

    let (outer_id, inner_id, inner_name) = dispatch(POOL.sender(), || {
        let outer_id = std::thread::current().id();
        let (inner_id, inner_name) = run_on_worker(|| {
            (
                std::thread::current().id(),
                std::thread::current().name().map(str::to_owned),
            )
        });
        (outer_id, inner_id, inner_name)
    });

    assert_ne!(
        outer_id, inner_id,
        "a cross-lane submission from a pool must run on the OTHER lane's \
         thread — the guard must not reject it, and it must not degrade to an \
         inline call on the pool consumer"
    );
    assert_eq!(
        inner_name.as_deref(),
        Some(WORKER_THREAD_NAME),
        "the inner job must land on the ENGINE lane specifically"
    );
}

// ── Cancel at the lane (task 6517) ───────────────────────────────────────────
//
// Task 5772 disclosed that routing `lsp_request` onto a lane removed
// DROP-CANCELLATION: before it, an abandoned `invoke` dropped the Tauri
// command's future and the LSP work stopped at its next `.await`; after it, the
// future is moved into a job and driven by `Handle::block_on` on a thread with
// no cancellation point, so dropping the awaiting side only drops the `oneshot`
// receiver while the work runs to completion regardless.
//
// The restoration is deliberately PARTIAL and needs no new dependency, no new
// token type and no change to the job contract: `dispatch_async` already moves a
// `tokio::sync::oneshot::Sender` into the job, and `Sender::is_closed()` is true
// exactly when the awaiting side's future was dropped. Checking it before
// driving anything skips an abandoned job instead of executing it.
//
// # Why these two tests use a SYNTHETIC sender
//
// Both need to observe the queue between the submission and the job running,
// which no real lane permits — a real consumer would pick the job up
// immediately. Building a `JobSender` over a channel whose `Receiver` the TEST
// holds makes the ordering provable rather than timed: the job cannot possibly
// have run before the test runs it by hand. No global lane is touched, and no
// assertion depends on a race.

/// (ah) An ABANDONED submission is dropped at the lane instead of driven.
///
/// The abandonment is modelled exactly as production produces it: the awaiting
/// side's future is dropped (here by `tokio::time::timeout` elapsing; in the GUI
/// by a closed window, a navigated-away pane, or a keystroke's request
/// superseded by the next one), which drops the `oneshot` receiver and closes
/// the `reply_tx` the job holds.
///
/// The job is then invoked ON A PLAIN THREAD rather than in this frame, for a
/// reason that is the difference between an attributable RED and a confusing
/// one: the job carries a `Handle::block_on`, and calling that inside this
/// runtime panics "Cannot start a runtime from within a runtime". On a plain
/// `std` thread it is legal — so at RED the future genuinely runs and this fails
/// on the flag it is about, rather than on a nested-runtime panic that names
/// nothing.
///
/// The join assertion is the second half of the claim: a skipped job must be a
/// clean NO-OP, not a new failure mode. Returning early drops the captured
/// future without polling it, which runs its destructors exactly as an abandoned
/// Tauri command future did before task 5772.
#[tokio::test]
async fn an_abandoned_submission_is_dropped_at_the_lane_instead_of_driven() {
    use crate::large_stack::{JobSender, dispatch_async};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    // The test HOLDS `rx`, so nothing drains the queue and the job provably
    // cannot run before the hand-invocation below.
    let (tx, rx) = std::sync::mpsc::channel();
    let sender = JobSender::new("test-cancel", tx);

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
        "precondition: the job cannot have run yet — this test holds the only \
         `Receiver`"
    );

    let job = rx
        .try_recv()
        .expect("the abandoned request must still have been ENQUEUED — this test \
                 is about what the lane does with it, not about whether it arrived");

    // A plain `std` thread is never a runtime context, so the job's
    // `Handle::block_on` is legal there and a non-skipping impl genuinely drives
    // the future — which is what makes the assertion below attributable.
    std::thread::spawn(move || job())
        .join()
        .expect(
            "invoking a skipped job must be a clean no-op: dropping the captured \
             future must not panic, or cancellation would trade wasted work for a \
             new failure mode",
        );

    assert!(
        !polled.load(Ordering::SeqCst),
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
    use crate::large_stack::{JobSender, dispatch_async};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    let (tx, rx) = std::sync::mpsc::channel();
    let sender = JobSender::new("test-live", tx);

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
