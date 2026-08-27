//! Regression guard: `CapturingSubscriberBuilder::build()` must capture an
//! event even when a subscriber-less sibling thread hit the callsite first
//! (task 6273).
//!
//! Calls the leaf `build()` directly rather than the `warn_capturing_subscriber()`
//! wrapper, so a future refactor that moves the priming call up into a wrapper
//! is caught here.
//!
//! Shape, preconditions and the HARD INVARIANT that this file hold exactly one
//! `#[test]`: see `tests/common/mod.rs`.

mod common;

/// The probe text, unique across the workspace, so no other test can have
/// registered this callsite before the sibling thread does. Single source of
/// truth: `probe()` formats it and the assertion below matches on it.
const PROBE_MESSAGE: &str = "capturing_builder_callsite_race probe";

/// The probe callsite (see `common`'s "Shape of a guard").
#[inline(never)]
fn probe() {
    tracing::warn!("{}", PROBE_MESSAGE);
}

#[test]
fn capturing_builder_captures_event_after_sibling_thread_registers_callsite() {
    common::assert_pristine_process();

    let (subscriber, capture) =
        reify_test_support::CapturingSubscriberBuilder::new(tracing::Level::WARN).build();

    tracing::subscriber::with_default(subscriber, || {
        // A subscriber-less sibling thread performs the callsite's FIRST hit
        // in this process. Pre-fix, that caches `Interest::never()` on the
        // callsite and the macro gate elides every later hit — on every
        // thread, including this one, inside `with_default`.
        std::thread::spawn(probe).join().unwrap();

        probe();
    });

    let messages = capture.messages();
    assert_eq!(
        capture.count(),
        1,
        "captured event must survive a sibling thread registering the callsite \
         first; captured messages: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains(PROBE_MESSAGE)),
        "no captured message contained {PROBE_MESSAGE:?}; captured messages: {messages:?}"
    );
}
