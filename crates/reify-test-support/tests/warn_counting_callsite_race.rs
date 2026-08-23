//! Regression guard: `warn_counting_subscriber()` must count a WARN even when a
//! subscriber-less sibling thread hit the callsite first (task 6273).
//!
//! Calls the leaf `warn_counting_subscriber()` directly rather than the
//! `warn_counting_guard()` wrapper, so a future refactor that moves the priming
//! call up into a wrapper is caught here. The wrapper's own
//! `set_default`-shaped usage — the exact shape of the reported flake — is
//! pinned separately by `warn_counting_guard_callsite_race.rs`.
//!
//! Shape, preconditions and the HARD INVARIANT that this file hold exactly one
//! `#[test]`: see `tests/common/mod.rs`.

mod common;

/// The probe callsite (see `common`'s "Shape of a guard").
#[inline(never)]
fn probe() {
    tracing::warn!("warn_counting_callsite_race probe");
}

#[test]
fn warn_counting_subscriber_counts_warn_after_sibling_thread_registers_callsite() {
    common::assert_pristine_process();

    let (subscriber, counter) = reify_test_support::warn_counting_subscriber();

    tracing::subscriber::with_default(subscriber, || {
        // A subscriber-less sibling thread performs the callsite's FIRST hit
        // in this process. Pre-fix, that caches `Interest::never()` on the
        // callsite and the macro gate elides every later hit — on every
        // thread, including this one, inside `with_default`.
        std::thread::spawn(probe).join().unwrap();

        probe();
    });

    reify_test_support::assert_warn_count(
        &counter,
        1,
        "warn must survive a sibling thread registering the callsite first",
    );
}
