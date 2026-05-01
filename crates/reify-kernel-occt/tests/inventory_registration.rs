//! Integration tests for the OCCT v0.2 multi-kernel adapter registration.
//!
//! Pins the OCCT [`CapabilityDescriptor`] (step 5) and the `inventory::submit!`
//! plumbing (step 7). Both tests are gated on [`reify_kernel_occt::OCCT_AVAILABLE`]
//! so stub-mode CI builds (where OCCT C++ libs are absent) compile and pass
//! the test binary as no-ops — matching the `cfg(has_occt)` gate on the OCCT
//! kernel itself in `crates/reify-kernel-occt/src/lib.rs:22-83`.

use reify_types::{Operation, ReprKind};

/// OCCT's capability descriptor must enumerate every operation routed
/// through `OcctKernelHandle::execute`, paired with `ReprKind::BRep`.
///
/// Negative pin: `(BooleanUnion, Mesh)` returns `false` — OCCT does not
/// produce mesh-family booleans (that's the v0.3 manifold story).
#[test]
fn occt_capability_descriptor_lists_brep_primitives_and_booleans() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
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
