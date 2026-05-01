//! Integration tests for the Fidget v0.2 multi-kernel adapter registration.
//!
//! Pins the Fidget [`CapabilityDescriptor`] (step-1/step-2) and the
//! `inventory::submit!` plumbing (step-5/step-6).
//!
//! Unlike the OCCT counterpart (`crates/reify-kernel-occt/tests/inventory_registration.rs`),
//! these tests are NOT gated on an `OCCT_AVAILABLE`-style flag — the fidget
//! adapter submits unconditionally in this v0.2 scaffold task (no
//! `cfg(has_fidget)` gate; see design decisions in `src/register.rs`).
//!
//! # Design template
//!
//! `crates/reify-kernel-manifold/tests/inventory_registration.rs:1-112`.

use reify_types::{CapabilityDescriptor, KernelRegistration, Operation, ReprKind};

/// Fidget's capability descriptor must enumerate exactly the three
/// SDF-Boolean operations Fidget supports.
///
/// Positive pins: `(BooleanUnion/Difference/Intersection, Sdf)` — the
/// complete set of SDF-native Booleans Fidget can execute.
///
/// Negative pins: every combination of `{BooleanUnion, BooleanDifference,
/// BooleanIntersection} × {BRep, Mesh, Voxel}` must return `false`. This
/// covers all non-`Sdf` [`ReprKind`] variants, mirroring the positive-pin
/// enumeration to prevent a regression that selectively adds B-rep, Mesh, or
/// Voxel support to Difference or Intersection (without Union) from slipping
/// through. This enforces the Fidget/OCCT/Manifold/OpenVDB roles split:
/// Fidget is the SDF specialist, B-rep Booleans belong to OCCT, Mesh Booleans
/// belong to Manifold, and Voxel Booleans belong to OpenVDB. A future
/// regression adding any of these claims to Fidget's descriptor would be
/// caught at test time, preventing the dispatcher from routing those Booleans
/// into a stub that cannot perform them.
#[test]
fn fidget_capability_descriptor_lists_sdf_booleans() {
    let descriptor = reify_kernel_fidget::register::fidget_capability_descriptor();

    // Positive pins — SDF Booleans ×3.
    assert!(
        descriptor.supports(Operation::BooleanUnion, ReprKind::Sdf),
        "Fidget must declare (BooleanUnion, Sdf)",
    );
    assert!(
        descriptor.supports(Operation::BooleanDifference, ReprKind::Sdf),
        "Fidget must declare (BooleanDifference, Sdf)",
    );
    assert!(
        descriptor.supports(Operation::BooleanIntersection, ReprKind::Sdf),
        "Fidget must declare (BooleanIntersection, Sdf)",
    );

    // Negative pins — Fidget does NOT handle B-rep Booleans (OCCT's domain).
    assert!(!descriptor.supports(Operation::BooleanUnion,        ReprKind::BRep), "Fidget must NOT declare (BooleanUnion, BRep) — B-rep Booleans are OCCT's domain");
    assert!(!descriptor.supports(Operation::BooleanDifference,   ReprKind::BRep), "Fidget must NOT declare (BooleanDifference, BRep) — B-rep Booleans are OCCT's domain");
    assert!(!descriptor.supports(Operation::BooleanIntersection, ReprKind::BRep), "Fidget must NOT declare (BooleanIntersection, BRep) — B-rep Booleans are OCCT's domain");

    // Negative pins — Fidget does NOT handle Mesh Booleans (Manifold's domain).
    assert!(!descriptor.supports(Operation::BooleanUnion,        ReprKind::Mesh), "Fidget must NOT declare (BooleanUnion, Mesh) — Mesh Booleans are Manifold's domain");
    assert!(!descriptor.supports(Operation::BooleanDifference,   ReprKind::Mesh), "Fidget must NOT declare (BooleanDifference, Mesh) — Mesh Booleans are Manifold's domain");
    assert!(!descriptor.supports(Operation::BooleanIntersection, ReprKind::Mesh), "Fidget must NOT declare (BooleanIntersection, Mesh) — Mesh Booleans are Manifold's domain");

    // Negative pins — Fidget does NOT handle Voxel Booleans (OpenVDB's domain).
    assert!(!descriptor.supports(Operation::BooleanUnion,        ReprKind::Voxel), "Fidget must NOT declare (BooleanUnion, Voxel) — Voxel Booleans are OpenVDB's domain");
    assert!(!descriptor.supports(Operation::BooleanDifference,   ReprKind::Voxel), "Fidget must NOT declare (BooleanDifference, Voxel) — Voxel Booleans are OpenVDB's domain");
    assert!(!descriptor.supports(Operation::BooleanIntersection, ReprKind::Voxel), "Fidget must NOT declare (BooleanIntersection, Voxel) — Voxel Booleans are OpenVDB's domain");
}

/// Fidget submits exactly one `KernelRegistration` named `"fidget"` into
/// the `inventory::iter::<KernelRegistration>()` set. This is the inventory-
/// plumbing pin: a missing or incorrectly-gated `inventory::submit!` would be
/// caught here.
///
/// The submitted registration's `descriptor()` must be function-pointer-
/// identical to `register::fidget_capability_descriptor` — a divergence
/// would indicate two parallel descriptor sources. Set-equality of the
/// materialised `supports` is also asserted as defence-in-depth.
///
/// # Design template
///
/// `crates/reify-kernel-manifold/tests/inventory_registration.rs:69-112`.
#[test]
fn fidget_kernel_registration_appears_in_inventory_iter() {
    // Linker anchor: referencing `fidget_capability_descriptor` here (before
    // the `inventory::iter` call below) mirrors the pattern in
    // `dispatcher_integration.rs` — an observable reference to a symbol in
    // `register.rs` forces the linker to keep the entire compilation unit,
    // including the `inventory::submit!` constructor.
    let direct_fn: fn() -> CapabilityDescriptor =
        reify_kernel_fidget::register::fidget_capability_descriptor;

    let fidget_entries: Vec<&KernelRegistration> = inventory::iter::<KernelRegistration>()
        .into_iter()
        .filter(|reg| reg.name == "fidget")
        .collect();

    assert_eq!(
        fidget_entries.len(),
        1,
        "expected exactly one inventory::submit! for kernel name \"fidget\", found {}",
        fidget_entries.len(),
    );

    // Pin via function-pointer identity: the intent is "the inventory
    // submission's `descriptor` field points at the same
    // `fidget_capability_descriptor` function the rest of the crate uses".
    // `std::ptr::fn_addr_eq` is the explicit, intent-revealing comparison.
    let inventory_fn = fidget_entries[0].descriptor;
    assert!(
        std::ptr::fn_addr_eq(inventory_fn, direct_fn),
        "the inventory-submitted descriptor must be the same function pointer as \
         `register::fidget_capability_descriptor` — a divergence indicates two \
         parallel descriptor sources",
    );

    // Also pin the materialised result as a HashSet (set equality —
    // order-insensitive) as defence-in-depth for the case where fn pointers
    // diverge but happen to produce equivalent content.
    let inventory_supports: std::collections::HashSet<(Operation, ReprKind)> =
        (fidget_entries[0].descriptor)().supports.into_iter().collect();
    let direct_supports: std::collections::HashSet<(Operation, ReprKind)> =
        reify_kernel_fidget::register::fidget_capability_descriptor()
            .supports
            .into_iter()
            .collect();
    assert_eq!(
        inventory_supports, direct_supports,
        "the inventory descriptor's supports SET must equal the direct call's — \
         order-insensitive so future literal reordering doesn't trip a false-positive",
    );
}
