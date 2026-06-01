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

/// Data-driven exhaustiveness check: iterates `KernelId::ALL` so a newly added
/// variant that lacks a corresponding `*_KERNEL_NAME` const (or a per-variant
/// test above) fails loudly rather than being silently swallowed by a wildcard.
///
/// To add a new kernel:
/// 1. Add the adapter const in the new `reify-kernel-<name>/src/register.rs`.
/// 2. Add a per-kernel consistency test function above
///    (named `<kernel>_kernel_name_const_matches_kernel_id_display`).
/// 3. Update the match arm here to cover the new variant.
/// 4. Update the `assert_eq!(KernelId::ALL.len(), ...)` count.
#[test]
fn all_kernel_ids_have_adapter_name_const() {
    // Fails when a new variant is added to KernelId::ALL without updating this
    // file.  The correct count is the new KernelId::ALL.len().
    assert_eq!(
        KernelId::ALL.len(),
        5,
        "KernelId::ALL.len() changed; add a per-kernel test above, a match \
         arm below, and update this expected count"
    );

    // Map each variant to its adapter const.  The wildcard arm panics so a
    // new, unhandled variant is caught at test-run time even if the len check
    // above is not yet updated.
    for id in KernelId::ALL {
        let expected = id.to_string();
        let actual = match id {
            KernelId::Occt => reify_kernel_occt::register::OCCT_KERNEL_NAME,
            KernelId::Manifold => reify_kernel_manifold::register::MANIFOLD_KERNEL_NAME,
            KernelId::Fidget => reify_kernel_fidget::register::FIDGET_KERNEL_NAME,
            KernelId::OpenVdb => reify_kernel_openvdb::register::OPENVDB_KERNEL_NAME,
            KernelId::Gmsh => reify_kernel_gmsh::register::GMSH_KERNEL_NAME,
            _ => panic!(
                "Unknown KernelId variant {id:?}; add its adapter const to this match"
            ),
        };
        assert_eq!(
            expected, actual,
            "KernelId::{id:?} adapter name drifted from KernelId::{id:?}.to_string()"
        );
    }
}
