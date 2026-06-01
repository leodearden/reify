//! Full consolidation identity test: asserts that `reify_config::KernelId`,
//! `reify_core::KernelId`, and `reify_ir::KernelId` are all the SAME type.
//!
//! This test fails to compile until step-6 replaces reify-config's local enum
//! with `pub use reify_core::KernelId;`.

/// Compile-time coercion: config → core (fails until they are the same type).
fn _c2core(x: reify_config::KernelId) -> reify_core::KernelId {
    x
}

/// Compile-time coercion: config → ir (fails until they are the same type).
fn _c2ir(x: reify_config::KernelId) -> reify_ir::KernelId {
    x
}

/// Runtime value check across all three paths.
#[test]
fn kernel_id_is_consolidated_single_type() {
    assert_eq!(reify_config::KernelId::Gmsh, reify_core::KernelId::Gmsh);
    assert_eq!(reify_config::KernelId::Occt, reify_ir::KernelId::Occt);
    assert_eq!(reify_config::KernelId::Fidget, reify_core::KernelId::Fidget);
}
