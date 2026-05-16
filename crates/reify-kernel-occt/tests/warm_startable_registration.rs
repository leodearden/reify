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
// `MANIFOLD_KERNEL_NAME` import in
// `reify-kernel-manifold/tests/inventory_registration.rs`).
use reify_kernel_occt::OCCT_AVAILABLE;
use reify_types::{NodeKind, WarmStartableRegistration, WarmStartableRegistry};

// Silence dead-code lints on the linkage-forcing reference — its only purpose
// is to keep the OCCT lib's static-init records from being stripped.
#[allow(dead_code)]
const _LINK_FORCE: bool = OCCT_AVAILABLE;

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
