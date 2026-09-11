//! Regression guard: `warn_counting_guard()` must count a WARN even when a
//! subscriber-less sibling thread hit the callsite first (task 6273).
//!
//! The sibling guards pin the three *leaf* constructors. This one pins the
//! `warn_counting_guard()` / `set_default` shape instead — deliberately, for
//! two reasons: it is the exact shape the reported flake took
//! (`gui/src-tauri/src/tests/claude_bridge_tests.rs` obtains its counter this
//! way), and a leaf-only guard would still pass if the wrapper ever stopped
//! delegating to `warn_counting_subscriber()` and hand-rolled its own
//! subscriber.
//!
//! Shape, preconditions and the HARD INVARIANT that this file hold exactly one
//! `#[test]`: see `tests/common/mod.rs`.

mod common;

/// The probe callsite (see `common`'s "Shape of a guard").
#[inline(never)]
fn probe() {
    tracing::warn!("warn_counting_guard_callsite_race probe");
}

#[test]
fn warn_counting_guard_counts_warn_after_sibling_thread_registers_callsite() {
    common::assert_pristine_process();

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
