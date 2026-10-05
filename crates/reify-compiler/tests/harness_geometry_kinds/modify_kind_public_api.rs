//! Regression-lock for Task 2238: `ModifyKind::VARIANT_COUNT` must remain accessible
//! to crate-external consumers after `ModifyKind::ALL` is narrowed to module-private.
//!
//! This test compiles as a separate crate (integration-test context), exercising the
//! public API surface exactly as `reify-eval`, `reify-test-support`, and other downstream
//! crates do.  A unit test inside `types.rs` would pass even if `VARIANT_COUNT` were
//! narrowed to `pub(crate)` by accident; placing it here pins the *external* contract.
//!
//! The actual numeric value of `VARIANT_COUNT` is already locked at compile time by
//! `const _: () = assert!(CASES.len() == ModifyKind::VARIANT_COUNT, …)` in
//! `geometry_modify.rs`.  Hard-coding it here would cause a spurious test failure
//! whenever a new variant is added, without adding contract coverage not already present.

use reify_compiler::ModifyKind;

/// Compile-time visibility check: `VARIANT_COUNT` must be reachable from an external crate.
const _: usize = ModifyKind::VARIANT_COUNT;

#[test]
#[allow(clippy::assertions_on_constants)]
fn variant_count_is_publicly_accessible() {
    // Read the constant to exercise the public API at runtime.  We assert only
    // that it is non-zero (sanity), not the exact value — the compile-time lock
    // in geometry_modify.rs already pins VARIANT_COUNT == CASES.len().
    assert!(ModifyKind::VARIANT_COUNT > 0);
}
