//! Integration tests for the Gmsh v0.3 multi-kernel adapter registration.
//!
//! Pins the Gmsh [`CapabilityDescriptor`] (positive + negative role-split
//! pins) and the `inventory::submit!` plumbing (function-pointer identity
//! and set-equality of materialised supports).
//!
//! Like the OpenVDB counterpart (the closest design template), these tests
//! are NOT gated on a `has_gmsh` flag — the gmsh adapter submits
//! unconditionally in this v0.3 scaffold task (no `cfg(has_gmsh)` gate; see
//! design decisions in `src/register.rs`).
//!
//! # Design template
//!
//! `crates/reify-kernel-openvdb/tests/inventory_registration.rs:1-130`.

use reify_kernel_gmsh::register::{GMSH_KERNEL_NAME, gmsh_capability_descriptor};
use reify_ir::{CapabilityDescriptor, KernelRegistration, Operation, ReprKind};

/// Gmsh's capability descriptor must enumerate exactly the
/// `(Convert{from: Mesh}, VolumeMesh)` surface→volume meshing operation.
///
/// Positive pin: `(Convert{from: Mesh}, VolumeMesh)` — the singular Gmsh
/// op surface in v0.3.
///
/// Negative pins: `(BooleanUnion, Mesh)` (Manifold's territory) and
/// `(Convert{from: BRep}, Mesh)` (OCCT's territory) must both return
/// `false`. This enforces the gmsh / occt / manifold / fidget / openvdb
/// roles split: gmsh is the surface→volume tet mesher, BRep tessellation
/// belongs to OCCT, Mesh Booleans belong to Manifold. A future regression
/// adding Mesh-Boolean or BRep-tessellation claims to gmsh's descriptor
/// would be caught at test time, preventing the dispatcher from routing
/// those into a stub that cannot perform them.
#[test]
fn gmsh_capability_descriptor_lists_mesh_to_volume_mesh_conversion() {
    let descriptor = gmsh_capability_descriptor();

    // Positive pin — surface→volume tet meshing is the *only* gmsh op in v0.3.
    assert!(
        descriptor.supports(
            Operation::Convert {
                from: ReprKind::Mesh
            },
            ReprKind::VolumeMesh,
        ),
        "Gmsh must declare (Convert{{from: Mesh}}, VolumeMesh)",
    );

    // Negative pin — Gmsh does NOT handle Mesh Booleans.
    assert!(
        !descriptor.supports(Operation::BooleanUnion, ReprKind::Mesh),
        "Gmsh must NOT declare (BooleanUnion, Mesh) — Mesh Booleans are Manifold's domain",
    );

    // Negative pin — Gmsh does NOT handle BRep tessellation.
    assert!(
        !descriptor.supports(
            Operation::Convert {
                from: ReprKind::BRep
            },
            ReprKind::Mesh,
        ),
        "Gmsh must NOT declare (Convert{{from: BRep}}, Mesh) — BRep tessellation is OCCT's domain",
    );
}

/// Gmsh submits exactly one `KernelRegistration` named `"gmsh"` into the
/// `inventory::iter::<KernelRegistration>()` set. This is the inventory-
/// plumbing pin: a missing or incorrectly-gated `inventory::submit!` would
/// be caught here.
///
/// The submitted registration's `descriptor()` must be function-pointer-
/// identical to `register::gmsh_capability_descriptor` — a divergence
/// would indicate two parallel descriptor sources. Set-equality of the
/// materialised `supports` is also asserted as defence-in-depth.
///
/// # Linker-anchor pattern
///
/// Referencing `gmsh_capability_descriptor` here (before the
/// `inventory::iter` call below) mirrors the pattern in
/// `dispatcher_integration.rs` — an observable reference to a symbol in
/// `register.rs` forces the linker to keep the entire compilation unit,
/// including the `inventory::submit!` constructor.
#[test]
fn gmsh_kernel_registration_appears_in_inventory_iter() {
    let direct_fn: fn() -> CapabilityDescriptor = gmsh_capability_descriptor;

    let gmsh_entries: Vec<&KernelRegistration> = inventory::iter::<KernelRegistration>()
        .filter(|reg| reg.name == GMSH_KERNEL_NAME)
        .collect();

    assert_eq!(
        gmsh_entries.len(),
        1,
        "expected exactly one inventory::submit! for kernel name {GMSH_KERNEL_NAME:?}, found {}",
        gmsh_entries.len(),
    );

    // Pin via function-pointer identity: the inventory submission's
    // `descriptor` field must point at the same `gmsh_capability_descriptor`
    // function the rest of the crate uses.
    let inventory_fn = gmsh_entries[0].descriptor;
    assert!(
        std::ptr::fn_addr_eq(inventory_fn, direct_fn),
        "the inventory-submitted descriptor must be the same function pointer as \
         `register::gmsh_capability_descriptor` — a divergence indicates two \
         parallel descriptor sources",
    );

    // Defence-in-depth: pin the materialised result as a HashSet (set
    // equality — order-insensitive) for the case where fn pointers diverge
    // but happen to produce equivalent content.
    let inventory_supports: std::collections::HashSet<(Operation, ReprKind)> =
        (gmsh_entries[0].descriptor)()
            .supports
            .into_iter()
            .collect();
    let direct_supports: std::collections::HashSet<(Operation, ReprKind)> =
        gmsh_capability_descriptor().supports.into_iter().collect();
    assert_eq!(
        inventory_supports, direct_supports,
        "the inventory descriptor's supports SET must equal the direct call's — \
         order-insensitive so future literal reordering doesn't trip a false-positive",
    );
}

/// Defence-in-depth: the gmsh descriptor must contain *exactly* one
/// `(op, repr)` entry. A future kernel author who accidentally adds Mesh
/// Booleans, BRep tessellation, or any other op-repr pair to gmsh's
/// descriptor would be caught at test time — preventing scope-creep that
/// would make gmsh a competing claimant for ops outside its surface→volume
/// remit.
#[test]
fn gmsh_descriptor_contains_only_one_supports_entry() {
    let descriptor = gmsh_capability_descriptor();
    assert_eq!(
        descriptor.supports.len(),
        1,
        "Gmsh's v0.3 descriptor must contain exactly one entry — \
         (Convert{{from: Mesh}}, VolumeMesh). Adding ops outside the \
         surface→volume remit would let gmsh compete for unrelated \
         lex-min tie-breaks. Got: {:?}",
        descriptor.supports,
    );
}
