//! Regression guard: `CountingSubscriberBuilder::build()` must count an event
//! even when a subscriber-less sibling thread hit the callsite first
//! (task 6273).
//!
//! Calls the leaf `build()` directly rather than a wrapper, so a future
//! refactor that moves the priming call up into a wrapper is caught here.
//!
//! Shape, preconditions and the HARD INVARIANT that this file hold exactly one
//! `#[test]`: see `tests/common/mod.rs`.

mod common;

/// The probe callsite (see `common`'s "Shape of a guard").
#[inline(never)]
fn probe() {
    tracing::warn!("counting_builder_callsite_race probe");
}

#[test]
fn counting_builder_counts_event_after_sibling_thread_registers_callsite() {
    common::assert_pristine_process();

    let (subscriber, counters) = reify_test_support::CountingSubscriberBuilder::new()
        .count_level(tracing::Level::WARN)
        .build();

    tracing::subscriber::with_default(subscriber, || {
        // A subscriber-less sibling thread performs the callsite's FIRST hit
        // in this process. Pre-fix, that caches `Interest::never()` on the
        // callsite and the macro gate elides every later hit — on every
        // thread, including this one, inside `with_default`.
        std::thread::spawn(probe).join().unwrap();

        probe();
    });

    reify_test_support::assert_warn_count(
        &counters[&tracing::Level::WARN],
        1,
        "counted event must survive a sibling thread registering the callsite first",
    );
}
