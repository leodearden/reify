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

use reify_types::{Operation, ReprKind};

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
