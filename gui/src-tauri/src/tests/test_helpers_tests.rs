// Tests for the shared `cwd_lock()` helper in `crate::tests::test_helpers`.
//
// The key invariant is that `cwd_lock()` returns the SAME `&'static Mutex<()>`
// instance on every call, so CWD-mutating tests across ALL test files in this
// crate share a single serialisation point.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::tests::test_helpers::cwd_lock;

/// (a) `cwd_lock()` returns the same pointer on every call — a true
/// process-global singleton, not a fresh `Mutex` per call.
#[test]
fn cwd_lock_returns_stable_reference() {
    let a = cwd_lock() as *const _;
    let b = cwd_lock() as *const _;
    assert!(
        std::ptr::eq(a, b),
        "cwd_lock() must return the same &'static Mutex on every call"
    );
}

/// (b) The lock provides mutual exclusion across threads: at most one thread
/// holds the guard simultaneously.
#[test]
fn cwd_lock_serializes_concurrent_acquirers() {
    use std::sync::Arc;
    use std::thread;

    let counter = Arc::new(AtomicUsize::new(0));

    let threads: Vec<_> = (0..4)
        .map(|_| {
            let counter = Arc::clone(&counter);
            thread::spawn(move || {
                let _guard = cwd_lock().lock().unwrap();
                // Increment while holding lock.
                let before = counter.fetch_add(1, Ordering::SeqCst);
                // No other thread should be inside the critical section.
                assert_eq!(
                    before, 0,
                    "concurrent_holding counter exceeded 1 — mutual exclusion violated"
                );
                // Simulate brief work.
                thread::yield_now();
                // Decrement before releasing lock.
                counter.fetch_sub(1, Ordering::SeqCst);
                // Guard dropped here.
            })
        })
        .collect();

    for t in threads {
        t.join().expect("thread panicked");
    }
}
