//! Regression-lock for Task 2238: `ModifyKind::VARIANT_COUNT` must remain accessible
//! to crate-external consumers after `ModifyKind::ALL` is narrowed to module-private.
//!
//! This test compiles as a separate crate (integration-test context), exercising the
//! public API surface exactly as `reify-eval`, `reify-test-support`, and other downstream
//! crates do.  A unit test inside `types.rs` would pass even if `VARIANT_COUNT` were
//! narrowed to `pub(crate)` by accident; placing it here pins the *external* contract.

use reify_compiler::ModifyKind;

#[test]
fn variant_count_is_publicly_accessible_and_equals_five() {
    assert_eq!(ModifyKind::VARIANT_COUNT, 5);
}
