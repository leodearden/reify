//! Type-identity test: asserts that `reify_ir::KernelId` IS `reify_core::KernelId`
//! (not a separate enum).
//!
//! This test fails to compile until step-4 replaces reify-ir's local enum
//! with `pub use reify_core::KernelId;`.

/// Compile-time type-identity coercions.  These functions are never called at
/// runtime; they exist to produce a type-mismatch compiler error if the two
/// paths name distinct types.
fn _same_via_root(x: reify_core::KernelId) -> reify_ir::KernelId {
    x
}

fn _same_via_geometry(x: reify_core::KernelId) -> reify_ir::geometry::KernelId {
    x
}

/// Smoke test: the compile-time coercion functions above carry the full weight
/// of the type-identity guarantee.  This single runtime assertion is kept as a
/// sanity check that the binary actually runs; further `assert_eq!` calls would
/// be tautological (both sides name the same type, so they cannot fail
/// independently of the coercions).
#[test]
fn kernel_id_is_the_reify_core_type() {
    assert_eq!(reify_ir::KernelId::Occt, reify_core::KernelId::Occt);
}
