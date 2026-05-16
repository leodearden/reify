//! PRD §5 B5 / I-3 (M-013 fix): pins that `reify-kernel-occt`'s static-init
//! submission of `WarmStartableRegistration { kind: NodeKind::Realization }`
//! is visible to downstream binaries through the `inventory` crate.
//!
//! Runs in both `cfg(has_occt)` and `cfg(not(has_occt))` (stub) builds — both
//! variants of `OcctKernel`/`OcctKernelHandle` impl `WarmStartable`, so the
//! registration is unconditional. Mirrors the unconditional submission
//! pattern used by `reify-kernel-manifold`.

// Force the `reify-kernel-occt` crate to be linked into the test binary so
// the static-init submission in `src/warm_register.rs` is picked up by
// `inventory::iter`. Without a reference to a symbol from the crate, the
// linker dead-strips it and the submission never fires (mirrors the
// `_link_force` fn in
// `reify-solver-elastic/tests/warm_startable_registration.rs`).
//
// Using a `pub fn` reference (rather than a `pub const`) for parity with the
// solver-elastic test and to avoid rustc's const-inlining behaviour silently
// skipping the linkage edge — `inventory::submit!` itself uses `#[used]` on
// its static so the const path likely works too, but a true symbol reference
// is the unambiguous shape.
use reify_kernel_occt::register::occt_capability_descriptor;
use reify_types::{CapabilityDescriptor, NodeKind, WarmStartableRegistration, WarmStartableRegistry};

// Silence dead-code lints on the linkage-forcing reference — its only purpose
// is to keep the OCCT lib's static-init records from being stripped.
#[allow(dead_code)]
fn _link_force() -> fn() -> CapabilityDescriptor {
    occt_capability_descriptor
}

#[test]
fn from_inventory_contains_realization() {
    let r = WarmStartableRegistry::from_inventory();
    assert!(
        r.contains_kind(NodeKind::Realization),
        "expected reify-kernel-occt's static-init submission to register NodeKind::Realization"
    );
}

#[test]
fn exactly_one_realization_registration() {
    // Cardinality pin: the OCCT crate submits exactly one
    // WarmStartableRegistration with kind == Realization. A future duplicate
    // submission (e.g. accidentally adding a second submit! during the
    // mesh-morph wiring) would silently change downstream registry observable
    // size; this assertion fails loudly.
    let count = inventory::iter::<WarmStartableRegistration>
        .into_iter()
        .filter(|reg| matches!(reg.kind, NodeKind::Realization))
        .count();
    assert_eq!(
        count, 1,
        "expected exactly one WarmStartableRegistration for NodeKind::Realization, got {count}"
    );
}
