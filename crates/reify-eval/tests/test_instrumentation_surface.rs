//! Compile-time and runtime lock for the `test-instrumentation` feature surface.
//!
//! This file serves two purposes:
//!
//! 1. **Runtime lock** — asserts that `Engine::last_guard_phase_group_evals()`
//!    returns `0` on a freshly constructed Engine (before any `edit_source` /
//!    `edit_param` call).
//!
//! 2. **Compile-time lock** — this file only compiles successfully when the
//!    `test-instrumentation` feature is enabled on `reify-eval`.  If the
//!    self-dev-dep entry in `Cargo.toml` (`reify-eval = { path = ".",
//!    features = ["test-instrumentation"] }`) is removed, this file will fail
//!    to compile because `last_guard_phase_group_evals` is gated behind
//!    `#[cfg(any(test, feature = "test-instrumentation"))]`.
//!
//! See task 2137 for context on why the accessor is restricted to
//! test-instrumentation rather than exposed as a stable public metric.

use reify_constraints::SimpleConstraintChecker;
use reify_eval::Engine;

#[test]
fn fresh_engine_guard_phase_counter_is_zero() {
    let engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    assert_eq!(
        engine.last_guard_phase_group_evals(),
        0,
        "fresh Engine must expose counter = 0 before any edit"
    );
}
