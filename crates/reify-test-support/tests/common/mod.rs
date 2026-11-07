//! Shared scaffolding for the `*_callsite_race.rs` regression guards (task 6273).
//!
//! Not a test binary: cargo auto-discovers only top-level `tests/*.rs`, so this
//! `tests/common/mod.rs` is compiled *into* each guard rather than run as a
//! fifth one. It exists so the rationale below has a single home — three or
//! four verbatim copies of it drift independently, and a correction to the
//! mechanism then has to be applied everywhere or become inconsistent.
//!
//! # HARD INVARIANT: exactly one `#[test]` per guard binary
//!
//! Do NOT add a second `#[test]` to any `*_callsite_race.rs` file, and do NOT
//! merge those files with each other. The bug they guard only expresses itself
//! in a pristine process — at most one live dispatcher, and no global default
//! subscriber installed yet — and any sibling test in the same binary destroys
//! both preconditions, leaving a guard that still passes but for the wrong
//! reason. Since cargo gives each `tests/*.rs` file its own process, one file
//! per guard is the only shape that keeps them honest.
//!
//! The full mechanism is documented on
//! `reify_test_support::prime_tracing_callsite_cache`, which carries the
//! authoritative version.
//!
//! # Shape of a guard
//!
//! Each guard defines its own private `#[inline(never)] fn probe()` holding a
//! `tracing::warn!` whose message is unique across the workspace, so no other
//! test can have registered that callsite before the guard's sibling thread
//! does. The probe stays per-binary precisely because the callsite must: it is
//! the thing under test. A single non-generic fn means both threads execute
//! the one macro expansion; `#[inline(never)]` is cosmetic — `tracing::warn!`
//! expands to a function-local `static CALLSITE`, and inlining a non-generic
//! fn does not duplicate its statics — it only keeps the frame legible in a
//! backtrace.

/// Assert the process-global preconditions every callsite-race guard depends
/// on, as the test's first statement.
///
/// This is a precondition check, not decoration: the "exactly one `#[test]`
/// per binary" rule above is otherwise enforced only by prose, so a future
/// contributor adding a second test (or a helper that transitively primes)
/// would leave a guard that still passes while no longer exercising the
/// poisoning path at all.
///
/// `has_been_set()` goes true on any `set_global_default` *or*
/// `set_default`/`with_default` (tracing-core 0.1.36 `dispatcher.rs:327` and
/// `:849`), so this one call catches every way a sibling test could have
/// disarmed the guard.
///
/// # Panics
///
/// Panics if a tracing dispatcher has already been installed in this process.
pub fn assert_pristine_process() {
    assert!(
        !tracing::dispatcher::has_been_set(),
        "precondition violated: a tracing dispatcher was already installed before \
         this test ran — a sibling #[test] in this binary has destroyed the guard \
         (see tests/common/mod.rs)"
    );
}
