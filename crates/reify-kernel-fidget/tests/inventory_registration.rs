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

use reify_types::{KernelRegistration, Operation, ReprKind};

/// Fidget's capability descriptor must enumerate exactly the three
/// SDF-Boolean operations Fidget supports.
///
/// Positive pins: `(BooleanUnion/Difference/Intersection, Sdf)` — the
/// complete set of SDF-native Booleans Fidget can execute.
///
/// Negative pins: `(BooleanUnion, BRep)` and `(BooleanUnion, Mesh)` must both
/// return `false`. This enforces the Fidget/OCCT/Manifold roles split: Fidget
/// is the SDF specialist, B-rep Booleans belong to OCCT, Mesh Booleans belong
/// to Manifold. A future regression adding B-rep or Mesh claims to Fidget's
/// descriptor would be caught at test time, preventing the dispatcher from
/// routing those Booleans into a stub that cannot perform them.
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

    // Negative pin — Fidget does NOT handle B-rep Booleans.
    assert!(
        !descriptor.supports(Operation::BooleanUnion, ReprKind::BRep),
        "Fidget must NOT declare (BooleanUnion, BRep) — B-rep Booleans are OCCT's domain",
    );

    // Negative pin — Fidget does NOT handle Mesh Booleans.
    assert!(
        !descriptor.supports(Operation::BooleanUnion, ReprKind::Mesh),
        "Fidget must NOT declare (BooleanUnion, Mesh) — Mesh Booleans are Manifold's domain",
    );
}
