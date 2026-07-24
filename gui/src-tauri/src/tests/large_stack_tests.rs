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

    assert_eq!(sum, 10, "closure return value must be propagated to the caller");
    assert_ne!(
        inner_id, caller_id,
        "closure must execute on a distinct (large-stack) thread, not the caller"
    );
    // `data` is still usable here — the borrow ended when the helper returned.
    assert_eq!(data.len(), 4, "borrowed local must remain owned by the caller");
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
    let received = rx.recv().expect("closure must send its value before exiting");
    assert_eq!(received, 42, "fire-and-forget closure must execute its side effect");

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

    let result = rx.recv().expect("deep-recursion closure must send its result");
    handle.join().expect("large-stack thread must join cleanly");

    assert_eq!(
        result,
        u64::from(DEEP_RECURSION_DEPTH) + 1,
        "deep recursion must run to completion on the fire-and-forget large stack"
    );
}
