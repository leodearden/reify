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
//! That argument covers the persistent-worker section below verbatim (task 5772):
//! its deep-recursion test invokes [`deep_recurse`] ONLY through
//! `large_stack::run_on_worker`, whose symbol is absent at RED — so that RED is
//! likewise a clean compile error and the ~16 MiB recursion never touches a
//! default-size stack.

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

    let first = run_on_worker(|| std::thread::current().id());
    let second = run_on_worker(|| std::thread::current().id());
    let third = run_on_worker(|| std::thread::current().id());

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
/// The recursion runs ONLY through the helper, so at RED (symbol absent) this is
/// a compile error, never a SIGSEGV — see the module docs.
#[test]
fn run_on_worker_survives_deep_recursion_over_default_stack() {
    use crate::large_stack::run_on_worker;

    let result = run_on_worker(|| deep_recurse(DEEP_RECURSION_DEPTH));

    assert_eq!(
        result,
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
