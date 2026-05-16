//! Presence-only registry of `NodeKind`s whose producers implement
//! [`WarmStartable`](crate::warm::WarmStartable).
//!
//! Powers the PRD §5 B5 invariant gate (M-013 fix): the runtime asserts that
//! every kind whose `default_traits()` advertises `WARM_STARTABLE` has at
//! least one producer registered, and conversely that no producer registers
//! a kind whose declared traits do not include `WARM_STARTABLE`. The check is
//! exposed via `reify_runtime::assert_warm_startable_coextensive` (B5 ζ).
//!
//! ## Why presence-only (not a factory map)
//!
//! PRD §5 B5's strawman shape — `HashMap<NodeKind, fn() -> Box<dyn WarmStartable>>` —
//! is explicitly relaxed to "or equivalent". The bidirectional coextension
//! check only needs `contains_kind`, never factory invocation. Adopting a
//! factory map would force inventing stateful `WarmStartable` wrappers for
//! kinds whose current producers (`CgWarmState` in solver-elastic,
//! `OcctKernel` in kernel-occt) don't fit a uniform trait shape — pure scope
//! creep. The registry API can extend backwards-compatibly later if a real
//! consultation/restoration use site materialises.
//!
//! ## Static-init plumbing
//!
//! Each producer crate (`reify-kernel-occt`, `reify-solver-elastic`)
//! submits a [`WarmStartableRegistration`] via `inventory::submit!`. The
//! runtime calls [`WarmStartableRegistry::from_inventory`] at scheduler init
//! to materialise the registry; this preserves the dependency-direction
//! inversion (reify-runtime does not depend on the adapter/solver crates).
//! Mirrors the existing [`crate::geometry::KernelRegistration`] pattern.

use std::collections::HashSet;

use crate::NodeKind;

/// Presence-only registry of [`NodeKind`]s whose producers implement
/// [`WarmStartable`](crate::warm::WarmStartable).
///
/// See the [module-level docs](self) for the rationale behind the
/// presence-only shape (instead of a factory map).
#[derive(Clone, Debug, Default)]
pub struct WarmStartableRegistry {
    kinds: HashSet<NodeKind>,
}

impl WarmStartableRegistry {
    /// Returns an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `kind` has at least one [`WarmStartable`](crate::warm::WarmStartable)
    /// producer registered. Idempotent: repeated calls with the same kind are a no-op.
    pub fn register(&mut self, kind: NodeKind) {
        self.kinds.insert(kind);
    }

    /// Returns `true` if `kind` has been registered.
    pub fn contains_kind(&self, kind: NodeKind) -> bool {
        self.kinds.contains(&kind)
    }

    /// Iterate the registered kinds. Order is unspecified (HashSet-backed).
    pub fn kinds(&self) -> impl Iterator<Item = NodeKind> + '_ {
        self.kinds.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeKind;

    #[test]
    fn new_is_empty() {
        let r = WarmStartableRegistry::new();
        assert!(!r.contains_kind(NodeKind::Value));
        assert!(!r.contains_kind(NodeKind::Constraint));
        assert!(!r.contains_kind(NodeKind::Realization));
        assert!(!r.contains_kind(NodeKind::Resolution));
        assert!(!r.contains_kind(NodeKind::Compute));
    }

    #[test]
    fn default_matches_new() {
        let a = WarmStartableRegistry::new();
        let b = WarmStartableRegistry::default();
        for k in NodeKind::ALL {
            assert_eq!(a.contains_kind(k), b.contains_kind(k));
        }
    }

    #[test]
    fn register_then_contains_kind_true() {
        let mut r = WarmStartableRegistry::new();
        r.register(NodeKind::Realization);
        assert!(r.contains_kind(NodeKind::Realization));
    }

    #[test]
    fn unregistered_kind_is_absent() {
        let mut r = WarmStartableRegistry::new();
        r.register(NodeKind::Realization);
        assert!(!r.contains_kind(NodeKind::Value));
        assert!(!r.contains_kind(NodeKind::Constraint));
        assert!(!r.contains_kind(NodeKind::Resolution));
        assert!(!r.contains_kind(NodeKind::Compute));
    }

    #[test]
    fn register_is_idempotent() {
        let mut r = WarmStartableRegistry::new();
        r.register(NodeKind::Compute);
        r.register(NodeKind::Compute);
        r.register(NodeKind::Compute);
        assert!(r.contains_kind(NodeKind::Compute));
        // No other kinds appear as a side-effect of the repeated registration.
        let kinds: Vec<NodeKind> = r.kinds().collect();
        assert_eq!(kinds.len(), 1);
        assert_eq!(kinds[0], NodeKind::Compute);
    }

    #[test]
    fn kinds_yields_registered_set() {
        let mut r = WarmStartableRegistry::new();
        r.register(NodeKind::Realization);
        r.register(NodeKind::Resolution);
        r.register(NodeKind::Compute);
        let mut got: Vec<NodeKind> = r.kinds().collect();
        got.sort_by_key(|k| format!("{k:?}"));
        let mut want = vec![NodeKind::Compute, NodeKind::Realization, NodeKind::Resolution];
        want.sort_by_key(|k| format!("{k:?}"));
        assert_eq!(got, want);
    }
}
