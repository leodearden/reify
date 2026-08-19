//! Regression guard: `CapturingSubscriberBuilder::build()` must capture an
//! event even when a subscriber-less sibling thread hit the callsite first
//! (task 6273).
//!
//! Calls the leaf `build()` directly rather than the `warn_capturing_subscriber()`
//! wrapper, so a future refactor that moves the priming call up into a wrapper
//! is caught here.
//!
//! # HARD INVARIANT: exactly one `#[test]` in this file
//!
//! Do NOT add a second `#[test]` here, and do NOT merge this file with the
//! sibling `*_callsite_race.rs` guards. The bug only expresses itself in a
//! pristine process — at most one live dispatcher, and no global default
//! installed yet — and any sibling test in the same binary destroys both
//! preconditions, leaving a guard that still passes but for the wrong reason.
//! The mechanism is documented in full on
//! `reify_test_support::prime_tracing_callsite_cache`; the test's first
//! statement asserts the preconditions so a violation fails loudly instead of
//! degrading silently.

/// The probe text, unique across the workspace, so no other test can have
/// registered this callsite before the sibling thread does. Single source of
/// truth: `probe()` formats it and the assertion below matches on it.
const PROBE_MESSAGE: &str = "capturing_builder_callsite_race probe";

/// The probe callsite.
///
/// A single non-generic fn, so both threads execute the one macro expansion.
/// `#[inline(never)]` is cosmetic — `tracing::warn!` expands to a function-local
/// `static CALLSITE`, and inlining a non-generic fn does not duplicate its
/// statics — it only keeps the frame legible in a backtrace.
#[inline(never)]
fn probe() {
    tracing::warn!("{}", PROBE_MESSAGE);
}

#[test]
fn capturing_builder_captures_event_after_sibling_thread_registers_callsite() {
    // Precondition, not decoration — see the HARD INVARIANT above.
    // `has_been_set()` goes true on any `set_global_default` *or*
    // `set_default`/`with_default` (tracing-core 0.1.36 `dispatcher.rs:327`
    // and `:849`), so this catches every way a sibling test could have
    // disarmed the guard.
    assert!(
        !tracing::dispatcher::has_been_set(),
        "precondition violated: a tracing dispatcher was already installed before \
         this test ran — a sibling #[test] in this binary has destroyed the guard"
    );

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
