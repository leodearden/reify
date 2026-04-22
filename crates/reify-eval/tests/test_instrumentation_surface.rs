//! Compile-time lock for the `test-instrumentation` feature surface.
//!
//! This file exists solely to verify that `Engine::last_guard_phase_group_evals()`
//! is reachable from integration tests when the `test-instrumentation` feature is
//! active.  If the self-dev-dep entry in `Cargo.toml`
//! (`reify-eval = { path = ".", features = ["test-instrumentation"] }`) is
//! removed, this file will fail to compile because `last_guard_phase_group_evals`
//! is gated behind `#[cfg(any(test, feature = "test-instrumentation"))]`.
//!
//! Runtime behaviour of the counter (counter == 2 on the per-group skip
//! scenarios) is locked by `tests/guard_eval.rs` and `tests/edit_source.rs`.
//!
//! See task 2137 for context on why the accessor is restricted to
//! test-instrumentation rather than exposed as a stable public metric.

use reify_constraints::SimpleConstraintChecker;
use reify_eval::Engine;

#[test]
fn last_guard_phase_group_evals_is_accessible() {
    // Use an empty prelude so this test does not depend on stdlib loading.
    // The call is the entire point: it must compile, proving the feature wire-up
    // in Cargo.toml is intact.
    let engine = Engine::with_prelude(Box::new(SimpleConstraintChecker), None, &[]);
    let _ = engine.last_guard_phase_group_evals();
}
