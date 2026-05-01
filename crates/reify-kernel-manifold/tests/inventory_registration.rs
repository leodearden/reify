//! Integration tests for the Manifold v0.2 multi-kernel adapter registration.
//!
//! Pins the Manifold [`CapabilityDescriptor`] (step-1/step-2) and the
//! `inventory::submit!` plumbing (step-3/step-4).
//!
//! Unlike the OCCT counterpart (`crates/reify-kernel-occt/tests/inventory_registration.rs`),
//! these tests are NOT gated on an `OCCT_AVAILABLE`-style flag — the manifold
//! adapter submits unconditionally in this v0.2 scaffold task (no
//! `cfg(has_manifold)` gate; see design decisions in `src/register.rs`).
//!
//! # Design template
//!
//! `crates/reify-kernel-occt/tests/inventory_registration.rs:1-152`.

use reify_types::{KernelRegistration, Operation, ReprKind};

/// Manifold's capability descriptor must enumerate exactly the three
/// mesh-Boolean operations Manifold supports.
///
/// Positive pins: `(BooleanUnion/Difference/Intersection, Mesh)` — the
/// complete set of mesh-native Booleans Manifold can execute.
///
/// Negative pin: `(BooleanUnion, BRep)` must return `false`. This enforces
/// the OCCT/Manifold roles split: a future regression adding B-rep claims
/// to Manifold's descriptor would route B-rep Booleans through the Manifold
/// stub, which cannot handle them, producing a runtime error. The pin catches
/// that regression at test time.
#[test]
fn manifold_capability_descriptor_lists_mesh_booleans() {
    let descriptor = reify_kernel_manifold::register::manifold_capability_descriptor();

    // Positive pins — mesh Booleans ×3.
    assert!(
        descriptor.supports(Operation::BooleanUnion, ReprKind::Mesh),
        "Manifold must declare (BooleanUnion, Mesh)",
    );
    assert!(
        descriptor.supports(Operation::BooleanDifference, ReprKind::Mesh),
        "Manifold must declare (BooleanDifference, Mesh)",
    );
    assert!(
        descriptor.supports(Operation::BooleanIntersection, ReprKind::Mesh),
        "Manifold must declare (BooleanIntersection, Mesh)",
    );

    // Negative pin — Manifold does NOT handle B-rep Booleans.
    // The dispatcher would otherwise route B-rep inputs through Manifold
    // when no OCCT kernel is registered, failing at runtime. The OCCT/Manifold
    // split must stay explicit: B-rep ops belong to OCCT.
    assert!(
        !descriptor.supports(Operation::BooleanUnion, ReprKind::BRep),
        "Manifold must NOT declare (BooleanUnion, BRep) — B-rep Booleans are OCCT's domain",
    );
}

/// Manifold submits exactly one `KernelRegistration` named `"manifold"` into
/// the `inventory::iter::<KernelRegistration>()` set. This is the inventory-
/// plumbing pin: a missing or incorrectly-gated `inventory::submit!` would be
/// caught here.
///
/// The submitted registration's `descriptor()` must be function-pointer-
/// identical to `register::manifold_capability_descriptor` — a divergence
/// would indicate two parallel descriptor sources. Set-equality of the
/// materialised `supports` is also asserted as defence-in-depth.
///
/// # Design template
///
/// `crates/reify-kernel-occt/tests/inventory_registration.rs:96-151`.
#[test]
fn manifold_kernel_registration_appears_in_inventory_iter() {
    let manifold_entries: Vec<&KernelRegistration> = inventory::iter::<KernelRegistration>()
        .into_iter()
        .filter(|reg| reg.name == "manifold")
        .collect();

    assert_eq!(
        manifold_entries.len(),
        1,
        "expected exactly one inventory::submit! for kernel name \"manifold\", found {}",
        manifold_entries.len(),
    );

    // Pin via function-pointer identity: the intent is "the inventory
    // submission's `descriptor` field points at the same
    // `manifold_capability_descriptor` function the rest of the crate uses".
    // `std::ptr::fn_addr_eq` is the explicit, intent-revealing comparison.
    let inventory_fn = manifold_entries[0].descriptor;
    let direct_fn: fn() -> reify_types::CapabilityDescriptor =
        reify_kernel_manifold::register::manifold_capability_descriptor;
    assert!(
        std::ptr::fn_addr_eq(inventory_fn, direct_fn),
        "the inventory-submitted descriptor must be the same function pointer as \
         `register::manifold_capability_descriptor` — a divergence indicates two \
         parallel descriptor sources",
    );

    // Also pin the materialised result as a HashSet (set equality —
    // order-insensitive) as defence-in-depth for the case where fn pointers
    // diverge but happen to produce equivalent content.
    let inventory_supports: std::collections::HashSet<(Operation, ReprKind)> =
        (manifold_entries[0].descriptor)().supports.into_iter().collect();
    let direct_supports: std::collections::HashSet<(Operation, ReprKind)> =
        reify_kernel_manifold::register::manifold_capability_descriptor()
            .supports
            .into_iter()
            .collect();
    assert_eq!(
        inventory_supports, direct_supports,
        "the inventory descriptor's supports SET must equal the direct call's — \
         order-insensitive so future literal reordering doesn't trip a false-positive",
    );
}
