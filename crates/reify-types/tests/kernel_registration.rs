//! Shape pin for the v0.2 multi-kernel `KernelRegistration` struct.
//!
//! Per the PRD `docs/prds/v0_2/multi-kernel.md` "Resolved design decisions",
//! kernel adapters submit a registration record into a static linker-collected
//! set (the `inventory` crate). The registration record names the kernel,
//! supplies a function that constructs the kernel's [`CapabilityDescriptor`],
//! and supplies a factory that boxes a fresh `dyn GeometryKernel`.
//!
//! Why this lives in `reify-types`: the rationale at
//! `crates/reify-types/src/geometry.rs:226-230` documents the dependency
//! inversion — kernel adapter crates (`-occt`, future `-manifold`, `-fidget`,
//! `-openvdb`) depend on `reify-types` for the `GeometryKernel` trait but
//! deliberately NOT on `reify-eval`. Placing the registration record alongside
//! the trait keeps adapter crates from acquiring an upward dep on `reify-eval`.
//!
//! This test pins the constructible shape only — fields are public, the
//! `descriptor` returns an owned [`CapabilityDescriptor`], and `name` is a
//! `&'static str`. The cross-crate inventory plumbing is exercised by
//! integration tests in `reify-eval/tests/kernel_registry_inventory.rs`
//! (task 2642 step 9).

use reify_types::{
    CapabilityDescriptor, GeometryKernel, KernelRegistration, Operation, ReprKind,
};

#[test]
fn kernel_registration_struct_constructible_with_descriptor_and_factory() {
    let reg = KernelRegistration {
        name: "test-kernel",
        descriptor: || CapabilityDescriptor {
            supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
        },
        factory: || -> Box<dyn GeometryKernel> {
            unreachable!("factory not invoked in shape test")
        },
    };

    assert_eq!(reg.name, "test-kernel", "name field must round-trip");

    let desc = (reg.descriptor)();
    assert!(
        desc.supports(Operation::BooleanUnion, ReprKind::BRep),
        "descriptor closure must produce a CapabilityDescriptor whose supports table includes the seeded pair",
    );
}
