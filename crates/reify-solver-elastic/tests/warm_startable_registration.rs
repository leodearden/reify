//! PRD §5 B5 / I-3 (M-013 fix): pins that `reify-solver-elastic`'s
//! static-init submission of
//! `WarmStartableRegistration { kind: NodeKind::Compute }` is visible to
//! downstream binaries through the `inventory` crate.
//!
//! The registration is presence-only (no factory) per the registry's
//! `HashSet<NodeKind>` shape; `CgWarmState`'s `into_opaque_state` /
//! `from_opaque_state` handshake remains the producer-side mechanism and
//! is untouched by this assertion. Mirrors the unconditional submission
//! pattern used by `reify-kernel-occt/src/warm_register.rs`.

// Force the `reify-solver-elastic` crate to be linked into the test binary
// so the static-init submission in `src/warm_register.rs` is picked up by
// `inventory::iter`. Without a reference to a symbol from the crate, the
// linker dead-strips it and the submission never fires (mirrors the
// `OCCT_AVAILABLE` import in
// `reify-kernel-occt/tests/warm_startable_registration.rs`).
use reify_solver_elastic::CgWarmState;
use reify_types::{NodeKind, WarmStartableRegistration, WarmStartableRegistry};

// Silence dead-code lints on the linkage-forcing reference — its only
// purpose is to keep the solver-elastic lib's static-init records from
// being stripped.
#[allow(dead_code)]
fn _link_force() -> Option<CgWarmState> {
    None
}

#[test]
fn from_inventory_contains_compute() {
    let r = WarmStartableRegistry::from_inventory();
    assert!(
        r.contains_kind(NodeKind::Compute),
        "expected reify-solver-elastic's static-init submission to register NodeKind::Compute"
    );
}

#[test]
fn at_least_one_compute_registration() {
    // Presence pin: at least one WarmStartableRegistration with
    // kind == Compute must appear in inventory. The registry's contract is
    // presence-only — duplicate registrations are idempotent at the `HashSet`
    // layer, so a future second Compute producer (e.g. a multigrid solver
    // variant gated behind a feature flag) would be perfectly legitimate. A
    // strict `== 1` pin would lock in a stricter contract than the production
    // code requires; `>= 1` matches the registry semantics.
    let count = inventory::iter::<WarmStartableRegistration>
        .into_iter()
        .filter(|reg| matches!(reg.kind, NodeKind::Compute))
        .count();
    assert!(
        count >= 1,
        "expected at least one WarmStartableRegistration for NodeKind::Compute, got {count}"
    );
}
