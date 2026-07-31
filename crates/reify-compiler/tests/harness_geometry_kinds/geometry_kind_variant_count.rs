//! Regression-lock for Task 5754 (units-length κ): every `GeometryOp` kind family
//! must expose a `VARIANT_COUNT` associated const that is accessible to crate-external
//! consumers, mirroring the `ModifyKind::VARIANT_COUNT` precedent locked by
//! `tests/modify_kind_public_api.rs`.
//!
//! This test compiles as a separate crate — it is a module of the `harness_geometry_kinds`
//! integration-test binary (C1 layout contract; see that harness root's header for why it
//! is consolidated rather than standalone) — exercising the public API surface exactly as
//! `reify-eval`, `reify-test-support`, and other downstream
//! crates do.  A unit test inside `types.rs` would pass even if a `VARIANT_COUNT` were
//! narrowed to `pub(crate)` by accident; placing it here pins the *external* contract
//! that `reify-eval`'s registry-completeness locks depend on.
//!
//! **Deliberately NOT pinned here: the numeric census.**  Each family's actual variant
//! count is locked at the *consumer* sites, where a mismatch means something real (a
//! registry array has fallen out of sync with the enum):
//!   * `reify-eval/src/geometry_ops/tests.rs` — `compile_geometry_op_registry_completeness`
//!   * `reify-eval/tests/compile_geometry_op_characterization.rs` — the file-level
//!     `const ALL_*` tables and `coverage_all_variant_families_and_nested_kinds`
//!   * `reify-compiler/src/geometry_modify.rs` — `CASES.len() == ModifyKind::VARIANT_COUNT`
//!
//! Hard-coding the values here would cause a spurious test failure whenever a new variant
//! is legitimately added, without adding contract coverage not already present.  Do not
//! "helpfully" add numeric assertions to this file.

use reify_compiler::{
    BooleanOp, CurveKind, PatternKind, PrimitiveKind, ProfileKind, SurfaceKind, SweepKind,
    TransformKind,
};

// Compile-time visibility checks: `VARIANT_COUNT` must be reachable from an external
// crate for every kind family.  These fail to compile — not merely to assert — if a
// const is removed or narrowed below `pub`.
const _: usize = PrimitiveKind::VARIANT_COUNT;
const _: usize = BooleanOp::VARIANT_COUNT;
const _: usize = TransformKind::VARIANT_COUNT;
const _: usize = PatternKind::VARIANT_COUNT;
const _: usize = SweepKind::VARIANT_COUNT;
const _: usize = CurveKind::VARIANT_COUNT;
const _: usize = ProfileKind::VARIANT_COUNT;
const _: usize = SurfaceKind::VARIANT_COUNT;

#[test]
#[allow(clippy::assertions_on_constants)]
fn variant_counts_are_publicly_accessible() {
    // Read each constant to exercise the public API at runtime.  We assert only that
    // each is non-zero (sanity), not the exact value — see the module header for why
    // the numeric census is deliberately pinned at the consumer sites instead.
    assert!(PrimitiveKind::VARIANT_COUNT > 0);
    assert!(BooleanOp::VARIANT_COUNT > 0);
    assert!(TransformKind::VARIANT_COUNT > 0);
    assert!(PatternKind::VARIANT_COUNT > 0);
    assert!(SweepKind::VARIANT_COUNT > 0);
    assert!(CurveKind::VARIANT_COUNT > 0);
    assert!(ProfileKind::VARIANT_COUNT > 0);
    assert!(SurfaceKind::VARIANT_COUNT > 0);
}
