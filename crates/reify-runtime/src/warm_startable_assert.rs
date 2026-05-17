//! Invariant gate that checks `NodeKind::default_traits()`'s declared
//! `WARM_STARTABLE` set agrees with the producer-side
//! [`WarmStartableRegistry`](reify_types::WarmStartableRegistry) — PRD §5 B5
//! / §6 I-3 (M-013 fix).
//!
//! Wired into the scheduler init path via
//! [`crate::concurrent::SchedulerConfig`]'s `warm_startable_registry: Option<…>`
//! field. The check uses `debug_assert_eq!` so release builds compile to a
//! no-op.

use reify_types::{NodeKind, NodeTraits, WarmStartableRegistry};

/// Assert that the producer-side [`WarmStartableRegistry`] and the
/// declaration-side `NodeKind::default_traits().contains(WARM_STARTABLE)`
/// agree for every variant of [`NodeKind`].
///
/// The check is bidirectional:
///
/// 1. *declared-without-registered* — every kind whose
///    `default_traits()` includes `WARM_STARTABLE` must appear in the
///    registry (otherwise downstream consumers would dispatch on a flag with
///    no producer behind it).
/// 2. *registered-without-declared* — every kind in the registry must have
///    `WARM_STARTABLE` set in its `default_traits()` (otherwise a producer
///    crate is registering a kind that the architecture does not classify as
///    warm-startable).
///
/// Implemented with `debug_assert_eq!`, so release builds compile to a no-op
/// regardless of the registry's contents. No extra `#[cfg(debug_assertions)]`
/// gate is required on the body.
///
/// See `docs/prds/v0_3/node-traits-unification.md` §5 B5 / §6 I-3 (M-013 fix).
pub fn assert_warm_startable_coextensive(registry: &WarmStartableRegistry) {
    for kind in NodeKind::ALL {
        let declared = kind.default_traits().contains(NodeTraits::WARM_STARTABLE);
        let registered = registry.contains_kind(kind);
        debug_assert_eq!(
            declared, registered,
            "NodeKind {kind:?}: WARM_STARTABLE declaration ({declared}) must match WarmStartableRegistry presence ({registered}) — see PRD §5 B5 (docs/prds/v0_3/node-traits-unification.md)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{NodeKind, WarmStartableRegistry};

    /// Helper: registry containing exactly the three declared-WARM_STARTABLE
    /// kinds (Realization, Resolution, Compute) — the positive case.
    fn declared_warm_kinds_only() -> WarmStartableRegistry {
        let mut r = WarmStartableRegistry::new();
        r.register(NodeKind::Realization);
        r.register(NodeKind::Resolution);
        r.register(NodeKind::Compute);
        r
    }

    // ── debug-mode panics ────────────────────────────────────────────────

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "WARM_STARTABLE")]
    fn empty_registry_panics_declared_without_registered() {
        // Realization, Resolution, Compute all declare WARM_STARTABLE per
        // NodeKind::default_traits() but the empty registry has none — the
        // declared-without-registered direction must panic.
        let r = WarmStartableRegistry::new();
        assert_warm_startable_coextensive(&r);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "WARM_STARTABLE")]
    fn extra_value_panics_registered_without_declared() {
        // Registry covers all declared-WARM_STARTABLE kinds (positive side
        // OK) AND additionally registers Value — but Value.default_traits()
        // = IMMEDIATE, no WARM_STARTABLE flag. The registered-without-declared
        // direction must panic.
        let mut r = declared_warm_kinds_only();
        r.register(NodeKind::Value);
        assert_warm_startable_coextensive(&r);
    }

    #[test]
    #[cfg(debug_assertions)]
    fn positive_case_does_not_panic() {
        // Registry equals exactly the declared-WARM_STARTABLE set — both
        // directions agree, no panic.
        let r = declared_warm_kinds_only();
        assert_warm_startable_coextensive(&r);
    }

    // ── release-mode no-op ───────────────────────────────────────────────

    #[test]
    #[cfg(not(debug_assertions))]
    fn release_mode_no_op_on_empty_registry() {
        // In release mode the debug_assert_eq! body is elided — even an empty
        // registry (which would panic in debug) must complete without panic.
        let r = WarmStartableRegistry::new();
        assert_warm_startable_coextensive(&r);
    }
}
