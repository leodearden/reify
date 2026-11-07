//! Regression guard: when `prime_tracing_callsite_cache()` loses the
//! `set_global_default` race, the resulting "priming is INACTIVE" diagnostic
//! must reach the *assertion panic*, not only stderr (task 6273).
//!
//! # Why this needs its own binary
//!
//! It deliberately installs a competing global default as its first act, which
//! is exactly the state the `*_callsite_race.rs` guards assert they are NOT in.
//! Run in a shared binary it would disarm every one of them. Cargo gives each
//! `tests/*.rs` file its own process, so isolating it here is the fix.
//!
//! The same HARD INVARIANT therefore applies: exactly one `#[test]` in this
//! file. See `tests/common/mod.rs`.
//!
//! # Why the panic, and not stderr, is the channel under test
//!
//! `INIT.call_once` runs on whichever test first touches a subscriber
//! constructor. libtest captures stderr per test thread and replays it only for
//! a *failing* test, so the install-time `eprintln!` lands in the buffer of a
//! test that typically passes — and is discarded. The operator then sees a bare
//! count-of-zero panic with no hint. This guard pins the flag-plus-note path
//! that closes that gap, and keeps it from decaying into dead code.

mod common;

/// A competing global default, standing in for e.g.
/// `tracing_subscriber::fmt().with_env_filter(..).init()`.
struct Competing;

impl tracing::Subscriber for Competing {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        false
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[test]
fn lost_priming_race_is_reported_in_the_assertion_panic() {
    common::assert_pristine_process();

    // Win the global-default race, so the constructor's priming loses it.
    tracing::subscriber::set_global_default(Competing)
        .expect("nothing else may have installed a global default in this process");

    // Primes internally; the install fails, which must set the process flag.
    let (_subscriber, counter) = reify_test_support::warn_counting_subscriber();

    // Provoke the count-of-zero failure an operator would actually hit. The
    // default hook is muted so the expected panic does not read as a real one
    // in the test log; it is restored immediately afterwards.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        reify_test_support::assert_warn_count(&counter, 1, "provoked failure");
    }))
    .expect_err("assert_warn_count must panic when the counter is 0 but 1 is expected");
    std::panic::set_hook(previous_hook);

    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&'static str>()
                .map(|s| (*s).to_owned())
        })
        .expect("panic payload must be a string");

    assert!(
        message.contains("provoked failure"),
        "panic message lost its caller-supplied context: {message:?}"
    );
    assert!(
        message.contains("priming is INACTIVE"),
        "a lost priming race must be reported in the assertion panic the operator \
         reads, not only on the stderr libtest discards for a passing test; got: \
         {message:?}"
    );
}
