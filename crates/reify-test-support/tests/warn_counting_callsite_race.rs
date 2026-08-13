//! Regression guard: `warn_counting_guard()` must count a WARN even when a
//! subscriber-less sibling thread hit the callsite first (task 6273).
//!
//! # HARD INVARIANT: this file must contain EXACTLY ONE `#[test]`
//!
//! Do NOT merge this file with the other `*_callsite_race.rs` files, and do
//! NOT add a second test to it. The failure being guarded depends on two
//! *process-global* preconditions that any sibling test in the same binary
//! would destroy:
//!
//! 1. `tracing_core::callsite::Dispatchers::has_just_one == true` (at most one
//!    live dispatcher). A second concurrently-live dispatcher makes the
//!    rebuilder `and` every live dispatcher's `Interest`, and
//!    `Interest::never().and(Interest::always()) == Interest::sometimes()` —
//!    i.e. healthy, so the bug cannot express itself.
//! 2. No global default subscriber installed yet. Any sibling test that calls
//!    `prime_tracing_callsite_cache()` (directly, or transitively via one of
//!    the counting/capturing constructors) installs one for the whole process.
//!
//! With a sibling test present this file would still pass — but it would pass
//! for the wrong reason, silently stopping being a regression guard.

/// A unique callsite hit nowhere else in the workspace. `#[inline(never)]` so
/// the sibling thread and the test thread hit the *same* callsite.
#[inline(never)]
fn probe() {
    tracing::warn!("warn_counting_callsite_race probe");
}

#[test]
fn warn_counting_guard_counts_warn_after_sibling_thread_registers_callsite() {
    let (_guard, counter) = reify_test_support::warn_counting_guard();

    // A subscriber-less sibling thread performs the callsite's FIRST hit in
    // this process. Pre-fix, that caches `Interest::never()` on the callsite
    // and the macro gate elides every later hit — on every thread.
    std::thread::spawn(probe).join().unwrap();

    probe();

    reify_test_support::assert_warn_count(
        &counter,
        1,
        "warn must survive a sibling thread registering the callsite first",
    );
}
