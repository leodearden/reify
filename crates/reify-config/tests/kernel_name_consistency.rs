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

/// Exhaustiveness guard: adding a `KernelId` variant without updating this
/// test is a compile error (missing match arm) AND a runtime failure
/// (length mismatch). Both signal that a new per-kernel consistency test
/// must be added above.
#[test]
fn kernel_id_variants_have_consistency_pins() {
    // Compile-time guard — a non-wildcard match forces every variant to be listed.
    // Adding a sixth KernelId variant without adding it here is a compile error.
    fn _exhaustiveness_guard(id: KernelId) {
        match id {
            KernelId::Occt
            | KernelId::Manifold
            | KernelId::Fidget
            | KernelId::OpenVdb
            | KernelId::Gmsh => (),
        }
    }

    // Runtime guard — ALL.len() must match the count of per-kernel tests above.
    // If you added a KernelId variant and fixed the compile error above, also
    // add the corresponding per-kernel consistency test function in this file
    // and bump this count.
    assert_eq!(
        KernelId::ALL.len(),
        5,
        "KernelId::ALL has grown beyond 5 variants; add a per-kernel \
         consistency test in crates/reify-config/tests/kernel_name_consistency.rs \
         for the new variant and update this count"
    );
}
