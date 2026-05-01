//! End-to-end pin for the cross-crate v0.2 multi-kernel inventory plumbing.
//!
//! This test binary's compile closure includes `reify-kernel-occt` (declared
//! in `crates/reify-eval/Cargo.toml:25-31` as a `[dev-dependencies]` entry),
//! so when OCCT is available the adapter's `inventory::submit!` (added in
//! task 2642 step 8) fires here and the registration appears in
//! `reify_eval::collect_registry()`.
//!
//! Pin scope: the chain `KernelRegistration in reify-types` →
//! `inventory::submit! in reify-kernel-occt` → `inventory::iter +
//! collect_registry in reify-eval`. A regression in any of those layers
//! (missing module declaration, wrong `cfg` gate, type mismatch on the
//! collect target) would break this test even though each crate's own
//! unit / integration tests stay green.

use reify_types::{Operation, ReprKind};

/// `collect_registry()` must surface the OCCT submission with a descriptor
/// that supports `(PrimitiveBox, BRep)` — a minimal proof that the
/// inventory plumbing is wired end-to-end across the three crates.
///
/// Skipped in stub mode: with `cfg(has_occt)` off, the OCCT submit
/// doesn't fire, so the registry is correctly empty and there's nothing
/// to assert.
#[test]
fn collect_registry_finds_occt_entry_with_brep_primitive_support() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        return;
    }

    let registry = reify_eval::collect_registry();

    let occt = registry.get("occt").expect(
        "collect_registry() must contain key \"occt\" once reify-kernel-occt's \
         inventory::submit! fires (gated on cfg(has_occt))",
    );

    assert!(
        occt.supports(Operation::PrimitiveBox, ReprKind::BRep),
        "the OCCT entry materialised by collect_registry() must declare \
         (PrimitiveBox, BRep) — caught a divergence between the inventory \
         submission's descriptor and the direct `register::occt_capability_descriptor()`",
    );
}
