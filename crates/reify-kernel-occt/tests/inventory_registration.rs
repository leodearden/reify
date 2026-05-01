//! Integration tests for the OCCT v0.2 multi-kernel adapter registration.
//!
//! Pins the OCCT [`CapabilityDescriptor`] (step 5) and the `inventory::submit!`
//! plumbing (step 7). Both tests are gated on [`reify_kernel_occt::OCCT_AVAILABLE`]
//! so stub-mode CI builds (where OCCT C++ libs are absent) compile and pass
//! the test binary as no-ops — matching the `cfg(has_occt)` gate on the OCCT
//! kernel itself in `crates/reify-kernel-occt/src/lib.rs:22-83`.

use reify_types::{KernelRegistration, Operation, ReprKind};

/// OCCT's capability descriptor must enumerate every operation routed
/// through `OcctKernelHandle::execute`, paired with `ReprKind::BRep`.
///
/// Negative pin: `(BooleanUnion, Mesh)` returns `false` — OCCT does not
/// produce mesh-family booleans (that's the v0.3 manifold story).
#[test]
fn occt_capability_descriptor_lists_brep_primitives_and_booleans() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping occt_capability_descriptor_lists_brep_primitives_and_booleans: \
             OCCT unavailable (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    let descriptor = reify_kernel_occt::register::occt_capability_descriptor();

    // Primitives ×4
    assert!(
        descriptor.supports(Operation::PrimitiveBox, ReprKind::BRep),
        "OCCT must declare (PrimitiveBox, BRep)",
    );
    assert!(
        descriptor.supports(Operation::PrimitiveCylinder, ReprKind::BRep),
        "OCCT must declare (PrimitiveCylinder, BRep)",
    );
    assert!(
        descriptor.supports(Operation::PrimitiveSphere, ReprKind::BRep),
        "OCCT must declare (PrimitiveSphere, BRep)",
    );
    assert!(
        descriptor.supports(Operation::PrimitiveTube, ReprKind::BRep),
        "OCCT must declare (PrimitiveTube, BRep)",
    );

    // Booleans ×3
    assert!(
        descriptor.supports(Operation::BooleanUnion, ReprKind::BRep),
        "OCCT must declare (BooleanUnion, BRep)",
    );
    assert!(
        descriptor.supports(Operation::BooleanDifference, ReprKind::BRep),
        "OCCT must declare (BooleanDifference, BRep)",
    );
    assert!(
        descriptor.supports(Operation::BooleanIntersection, ReprKind::BRep),
        "OCCT must declare (BooleanIntersection, BRep)",
    );

    // Representative samples from Modify, Sweep, Transform.
    assert!(
        descriptor.supports(Operation::ModifyFillet, ReprKind::BRep),
        "OCCT must declare (ModifyFillet, BRep)",
    );
    assert!(
        descriptor.supports(Operation::SweepExtrude, ReprKind::BRep),
        "OCCT must declare (SweepExtrude, BRep)",
    );
    assert!(
        descriptor.supports(Operation::TransformTranslate, ReprKind::BRep),
        "OCCT must declare (TransformTranslate, BRep)",
    );

    // Negative pin: OCCT does NOT produce Mesh-family results from booleans.
    // The dispatcher's BFS would otherwise route Mesh booleans through OCCT
    // when no manifold/-mesh kernel is registered, producing a runtime error
    // at execution time. Declaring the negative here keeps the v0.3 manifold
    // story explicit: `(BooleanUnion, Mesh)` belongs to a future Mesh kernel
    // (or to an `(Operation::Convert { from: BRep }, Mesh)` chain).
    assert!(
        !descriptor.supports(Operation::BooleanUnion, ReprKind::Mesh),
        "OCCT must NOT declare (BooleanUnion, Mesh) — see v0.3 manifold story",
    );
}

/// OCCT submits exactly one `KernelRegistration` named `"occt"` into the
/// `inventory::iter::<KernelRegistration>()` set. This is the inventory-
/// plumbing pin: an `inventory::submit!` that is missing or wrapped in the
/// wrong cfg gate is caught here.
///
/// The submitted registration's `descriptor()` must produce a
/// `CapabilityDescriptor` byte-equal (modulo `Vec` ordering) to
/// `register::occt_capability_descriptor()` — both must come from the same
/// underlying function pointer, so this assertion would only fire if a
/// future drift introduces two divergent descriptor functions.
#[test]
fn occt_kernel_registration_appears_in_inventory_iter() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping occt_kernel_registration_appears_in_inventory_iter: \
             OCCT unavailable (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    let occt_entries: Vec<&KernelRegistration> = inventory::iter::<KernelRegistration>()
        .into_iter()
        .filter(|reg| reg.name == "occt")
        .collect();

    assert_eq!(
        occt_entries.len(),
        1,
        "expected exactly one inventory::submit! for kernel name \"occt\", found {}",
        occt_entries.len(),
    );

    // Pin via function-pointer identity rather than Vec equality: the
    // intent is "the inventory submission's `descriptor` field points at
    // the same `occt_capability_descriptor` function the rest of the crate
    // uses" — Vec equality would also pass that check today, but is
    // order-sensitive and would spuriously break if the descriptor's
    // literal entries get reordered while still calling the same fn.
    // `std::ptr::fn_addr_eq` is the explicit, intent-revealing comparison.
    let inventory_fn = occt_entries[0].descriptor;
    let direct_fn: fn() -> reify_types::CapabilityDescriptor =
        reify_kernel_occt::register::occt_capability_descriptor;
    assert!(
        std::ptr::fn_addr_eq(inventory_fn, direct_fn),
        "the inventory-submitted descriptor must be the same function pointer as \
         `register::occt_capability_descriptor` — a divergence indicates two \
         parallel descriptor sources",
    );

    // Also pin the materialised result as a HashSet (set equality —
    // order-insensitive) as a defence-in-depth check for the case where
    // the fn pointers diverge but happen to produce equivalent content.
    let inventory_supports: std::collections::HashSet<(Operation, ReprKind)> =
        (occt_entries[0].descriptor)().supports.into_iter().collect();
    let direct_supports: std::collections::HashSet<(Operation, ReprKind)> =
        reify_kernel_occt::register::occt_capability_descriptor()
            .supports
            .into_iter()
            .collect();
    assert_eq!(
        inventory_supports, direct_supports,
        "the inventory descriptor's supports SET must equal the direct call's — \
         this is order-insensitive on purpose so future literal reordering of \
         occt_capability_descriptor's vec doesn't trip a false-positive",
    );
}
