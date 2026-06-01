//! Cross-crate consistency pins: `*_KERNEL_NAME` consts vs `KernelId::*.to_string()`.
//!
//! ## Why this test lives in `reify-config`
//!
//! Each kernel adapter crate (`reify-kernel-{occt,manifold,fidget,openvdb,gmsh}`)
//! declares a `pub const *_KERNEL_NAME: &str = "..."` used as the key in the
//! dispatcher registry and the value in `inventory::submit!` registrations.
//! These strings MUST equal `KernelId::*.to_string()` from `reify-config` so
//! that the project-pin lookup (`Manifest::kernel_pins()`) can find the
//! registered adapter by name at runtime.
//!
//! Adapter crates intentionally do NOT depend on `reify-config` (that would
//! invert the layering), and `reify-config` carries no runtime dep on any
//! adapter. A single integration-test fan-in here — with adapters added only
//! to `[dev-dependencies]` — is the minimum-coupling way to enforce the
//! cross-crate contract without touching production dependency edges.
//!
//! See task 3139 / esc-3123-85 for the full design rationale.

use reify_config::KernelId;

#[test]
fn occt_kernel_name_const_matches_kernel_id_display() {
    assert_eq!(
        KernelId::Occt.to_string(),
        reify_kernel_occt::register::OCCT_KERNEL_NAME,
        "OCCT_KERNEL_NAME drifted from KernelId::Occt.to_string(); update \
         crates/reify-kernel-occt/src/register.rs to restore consistency"
    );
}

#[test]
fn manifold_kernel_name_const_matches_kernel_id_display() {
    assert_eq!(
        KernelId::Manifold.to_string(),
        reify_kernel_manifold::register::MANIFOLD_KERNEL_NAME,
        "MANIFOLD_KERNEL_NAME drifted from KernelId::Manifold.to_string(); update \
         crates/reify-kernel-manifold/src/register.rs to restore consistency"
    );
}

#[test]
fn fidget_kernel_name_const_matches_kernel_id_display() {
    assert_eq!(
        KernelId::Fidget.to_string(),
        reify_kernel_fidget::register::FIDGET_KERNEL_NAME,
        "FIDGET_KERNEL_NAME drifted from KernelId::Fidget.to_string(); update \
         crates/reify-kernel-fidget/src/register.rs to restore consistency"
    );
}

#[test]
fn openvdb_kernel_name_const_matches_kernel_id_display() {
    assert_eq!(
        KernelId::OpenVdb.to_string(),
        reify_kernel_openvdb::register::OPENVDB_KERNEL_NAME,
        "OPENVDB_KERNEL_NAME drifted from KernelId::OpenVdb.to_string(); update \
         crates/reify-kernel-openvdb/src/register.rs to restore consistency"
    );
}

#[test]
fn gmsh_kernel_name_const_matches_kernel_id_display() {
    assert_eq!(
        KernelId::Gmsh.to_string(),
        reify_kernel_gmsh::register::GMSH_KERNEL_NAME,
        "GMSH_KERNEL_NAME drifted from KernelId::Gmsh.to_string(); update \
         crates/reify-kernel-gmsh/src/register.rs to restore consistency"
    );
}

/// Exhaustiveness reminder: all five known variants are listed explicitly so a
/// reviewer can see at a glance that every adapter is covered.  The wildcard
/// arm is required because `KernelId` is `#[non_exhaustive]` in `reify-core`
/// (external crates cannot write an exhaustive match on it); exhaustiveness at
/// the type level is guaranteed by `reify-core`'s own `ALL`-based tests.
///
/// To add a new kernel: add a new per-kernel consistency test function above
/// (named `<kernel>_kernel_name_const_matches_kernel_id_display`).
const _EXHAUSTIVENESS_PIN: fn(KernelId) = |id| match id {
    KernelId::Occt | KernelId::Manifold | KernelId::Fidget | KernelId::OpenVdb | KernelId::Gmsh => {}
    _ => {}
};
