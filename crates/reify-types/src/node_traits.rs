//! Composable execution-trait flags for node-kind classification.
//!
//! Implements the [`NodeTraits`] bitflag newtype (four trait flags from §7.6 "Node Traits")
//! and the [`NodeKind`] enum (five-variant canonical taxonomy mirroring `NodeId`'s 5 variants
//! in `reify-eval`); see `docs/reify-implementation-architecture.md` and
//! `docs/prds/v0_3/node-traits-unification.md`.
//!
//! Also provides [`HasNodeKind`] and [`NodeTraitsMap`] (PRD §5 B1): a per-instance /
//! per-kind override map with kind-derived fallback, generic over key type so that
//! `NodeId` (which lives in `reify-eval`) can be used as the key in `reify-runtime`
//! without violating the crate dependency order.
//!
//! ### Trait semantics (§7.6 table)
//!
//! | Flag              | Meaning |
//! |-------------------|---------|
//! | `IMMEDIATE`       | Result is available as soon as inputs are known; no warm-start needed |
//! | `WARM_STARTABLE`  | Can be resumed from a saved warm state to avoid full recomputation |
//! | `PROGRESSIVE`     | Emits partial results before reaching a final value |
//! | `COMMITTABLE`     | Produces a value that can be committed to the snapshot store |
//!
//! Nothing in this crate or its dependents currently dispatches on these traits.
//! They are purely declarative scaffolding for downstream scheduler/cache tasks to adopt.

use std::collections::HashMap;
use std::hash::Hash;
use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};

/// Composable execution-trait flags for a node kind.
///
/// See `docs/reify-implementation-architecture.md §7.6 Node Traits`.
///
/// Implemented as a `u8` bitflag newtype to avoid introducing a third-party
/// dependency (`bitflags`, `enumflags2`) for ~30 lines of trivial logic.
/// Use [`NodeTraits::union`] / [`NodeTraits::intersection`] in `const` contexts;
/// the `|` / `&` operators are available for non-const use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NodeTraits(u8);

impl NodeTraits {
    /// The empty (no flags) value. Equivalent to [`Default::default`].
    #[inline]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Returns `true` if no flags are set.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns `true` if `self` contains all flags in `other` (subset test).
    #[inline]
    pub const fn contains(self, other: NodeTraits) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Result is available as soon as inputs are known; no warm-start needed.
    ///
    /// See `docs/reify-implementation-architecture.md §7.6`.
    pub const IMMEDIATE: NodeTraits = NodeTraits(0b0000_0001);

    /// Can be resumed from a saved warm state to avoid full recomputation.
    ///
    /// See `docs/reify-implementation-architecture.md §7.6`.
    pub const WARM_STARTABLE: NodeTraits = NodeTraits(0b0000_0010);

    /// Emits partial results before reaching a final value.
    ///
    /// See `docs/reify-implementation-architecture.md §7.6`.
    pub const PROGRESSIVE: NodeTraits = NodeTraits(0b0000_0100);

    /// Produces a value that can be committed to the snapshot store.
    ///
    /// See `docs/reify-implementation-architecture.md §7.6`.
    pub const COMMITTABLE: NodeTraits = NodeTraits(0b0000_1000);

    /// Bitwise OR of every declared flag constant (used by the [`Not`] impl).
    ///
    /// Derived from the flag constants rather than a hand-written literal so
    /// that any future flag added to the list above is automatically included
    /// in `Not`'s domain, removing a quiet-failure foot-gun.
    const ALL_MASK: u8 =
        Self::IMMEDIATE.0 | Self::WARM_STARTABLE.0 | Self::PROGRESSIVE.0 | Self::COMMITTABLE.0;

    /// Returns the union of `self` and `other` (bitwise OR).
    ///
    /// Prefer this over the `|` operator in `const` contexts, where
    /// `std::ops::BitOr` cannot be called on stable Rust.
    #[inline]
    pub const fn union(self, other: NodeTraits) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns the intersection of `self` and `other` (bitwise AND).
    ///
    /// Prefer this over the `&` operator in `const` contexts.
    #[inline]
    pub const fn intersection(self, other: NodeTraits) -> Self {
        Self(self.0 & other.0)
    }
}

impl BitOr for NodeTraits {
    type Output = Self;
    #[inline]
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitAnd for NodeTraits {
    type Output = Self;
    #[inline]
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl BitOrAssign for NodeTraits {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAndAssign for NodeTraits {
    #[inline]
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl Not for NodeTraits {
    type Output = Self;
    /// Bitwise NOT within the declared four-flag domain.
    ///
    /// `!flag` excludes `flag` and includes all other declared flags.
    #[inline]
    fn not(self) -> Self {
        Self(!self.0 & Self::ALL_MASK)
    }
}

/// Canonical node-kind discriminant: mirrors the 5 variants of `NodeId` in `reify-eval`.
///
/// Carries the architecture-specified default [`NodeTraits`] sets for each kind
/// (trait flags from §7.6 "Node Traits"); see `docs/reify-implementation-architecture.md`
/// §2.1, §7.6, and `docs/prds/v0_3/node-traits-unification.md §4`.
///
/// `NodeKind` lives in `reify-types` so that any crate depending on `reify-types`
/// can use it without pulling in `reify-eval`. The conversion bridge
/// `impl From<&NodeId> for NodeKind` is hosted in `reify-eval` (the only crate where
/// both types are visible without an orphan-rule violation — see PRD §4).
///
/// `reify-runtime` re-exports this type via `pub use reify_types::NodeKind`, keeping
/// all existing `reify_runtime::commitment::NodeKind` call sites working without change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// A value cell node. Default traits: [`NodeTraits::IMMEDIATE`].
    ///
    /// See §2.1 "Node Taxonomy" and §7.6 "Node Traits" in
    /// `docs/reify-implementation-architecture.md`.
    Value,
    /// A constraint node. Default traits: empty (no flags set).
    ///
    /// See §2.1 "Node Taxonomy" and §7.6 "Node Traits" in
    /// `docs/reify-implementation-architecture.md`.
    Constraint,
    /// A realization (geometry output) node. Default traits: `WARM_STARTABLE | COMMITTABLE`.
    ///
    /// See §2.1 "Node Taxonomy" and §7.6 "Node Traits" in
    /// `docs/reify-implementation-architecture.md`.
    Realization,
    /// A resolution (constraint solver) node. Default traits: `WARM_STARTABLE | COMMITTABLE`.
    ///
    /// See §2.1 "Node Taxonomy" and §7.6 "Node Traits" in
    /// `docs/reify-implementation-architecture.md`.
    Resolution,
    /// A compute node (e.g. an @optimized FEA/solver computation). Default traits:
    /// `WARM_STARTABLE | COMMITTABLE`.
    ///
    /// See §2.1 "Node Taxonomy" and §7.6 "Node Traits" in
    /// `docs/reify-implementation-architecture.md`.
    Compute,
}

impl NodeKind {
    /// Returns the architecture-specified default [`NodeTraits`] for this node kind.
    ///
    /// ## Derivation
    ///
    /// Each per-kind default is drawn from several sections of
    /// `docs/reify-implementation-architecture.md`:
    ///
    /// | Section | Role in this function |
    /// |---------|----------------------|
    /// | §2.1 "Node Taxonomy (7 Types)" | Canonical description of each kind's purpose |
    /// | §3.3 "Two-Cone Scheduling Model" | P0/P1-fast scheduling → `IMMEDIATE` |
    /// | §4.1 "The WarmStartable Protocol" | Iterative/incremental computation → `WARM_STARTABLE` |
    /// | §7.3 "Task Commitment Policy" | May run past commitment thresholds → `COMMITTABLE` |
    /// | §7.6 "Node Traits" | Authoritative definition of the four trait flags |
    ///
    /// **`IMMEDIATE` kinds** (`Value`): §3.3 classifies value cell reads as P0/P1-fast —
    /// cheap, sub-frame reads evaluable inline. No warm-start or commitment machinery is
    /// required.
    ///
    /// **`WARM_STARTABLE | COMMITTABLE` kinds** (`Resolution`, `Realization`, `Compute`):
    /// §4.1 targets these as the long-running iterative/incremental computations that
    /// benefit from resuming a saved warm state. §7.3 notes that they may run past a
    /// commitment threshold and must therefore finish against their original snapshot,
    /// justifying `COMMITTABLE`.
    ///
    /// **`Constraint` (empty)**: Predicate evaluation is cheap but §7.6 does not yet
    /// classify it under any of the four traits. Assigning `IMMEDIATE` would conflate it
    /// with sub-frame `ValueCell` reads; leaving the set empty is the conservative choice
    /// until a downstream scheduler task formalises the policy (PRD §12 Q-1).
    pub const fn default_traits(self) -> NodeTraits {
        match self {
            NodeKind::Value => NodeTraits::IMMEDIATE,
            NodeKind::Constraint => NodeTraits::empty(),
            NodeKind::Realization => NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE),
            NodeKind::Resolution => NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE),
            NodeKind::Compute => NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE),
        }
    }
}

/// Marker trait for keys that can be projected to a [`NodeKind`] discriminant.
///
/// Implemented by `reify_eval::cache::NodeId` (orphan-rule-clean host alongside
/// the existing `From<&NodeId> for NodeKind` bridge in `reify-eval/src/cache.rs`).
/// Test code may impl this for a local key type to exercise `NodeTraitsMap` in
/// `reify-types` unit tests without pulling in `reify-eval`. See PRD §5 B1.
pub trait HasNodeKind {
    fn node_kind(&self) -> NodeKind;
}

/// Per-instance / per-kind override map for [`NodeTraits`], with kind-derived
/// fallback. Bridges audit findings M-002 and M-005 (per-NodeId trait map).
///
/// Resolution precedence (see PRD §6 trait-resolution-chain): per-instance >
/// per-kind > kind-derived `default_traits()`. Default-empty preserves
/// the prior scheduler behaviour because every `resolve` call still returns
/// the §7.6 architecture default for un-overridden nodes.
///
/// Generic over key type K so the production wiring uses
/// `NodeTraitsMap<reify_eval::cache::NodeId>` while reify-types unit tests can
/// substitute a local key (NodeId lives in reify-eval per PRD §11 / task α).
#[derive(Clone, Debug)]
pub struct NodeTraitsMap<K: Eq + Hash + HasNodeKind> {
    instance: HashMap<K, NodeTraits>,
    by_kind: HashMap<NodeKind, NodeTraits>,
}

impl<K: Eq + Hash + HasNodeKind> Default for NodeTraitsMap<K> {
    /// Returns an empty map. Does not require `K: Default` (unlike `#[derive(Default)]`).
    fn default() -> Self {
        Self {
            instance: HashMap::new(),
            by_kind: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash + HasNodeKind> NodeTraitsMap<K> {
    /// Creates a new empty `NodeTraitsMap`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a per-instance trait override for `node_id`. Overwrites any prior value.
    ///
    /// Allows full per-instance freedom — including Constraint+IMMEDIATE per PRD §12 Q-6.
    pub fn set_instance(&mut self, node_id: K, traits: NodeTraits) {
        self.instance.insert(node_id, traits);
    }

    /// Set a per-kind trait override applied to all keys of that kind absent an instance entry.
    pub fn set_type(&mut self, kind: NodeKind, traits: NodeTraits) {
        self.by_kind.insert(kind, traits);
    }

    /// Resolve effective traits, consulting instance → by_kind → kind-derived default in turn.
    pub fn resolve(&self, node_id: &K) -> NodeTraits {
        if let Some(t) = self.instance.get(node_id) {
            return *t;
        }
        let kind = node_id.node_kind();
        if let Some(t) = self.by_kind.get(&kind) {
            return *t;
        }
        kind.default_traits()
    }
}

#[cfg(test)]
mod node_traits_map_tests {
    use super::*;

    #[derive(Clone, Debug, Eq, Hash, PartialEq)]
    struct TestKey {
        id: u32,
        kind: NodeKind,
    }

    impl HasNodeKind for TestKey {
        fn node_kind(&self) -> NodeKind {
            self.kind
        }
    }

    fn value_key(id: u32) -> TestKey {
        TestKey { id, kind: NodeKind::Value }
    }
    fn compute_key(id: u32) -> TestKey {
        TestKey { id, kind: NodeKind::Compute }
    }
    fn constraint_key(id: u32) -> TestKey {
        TestKey { id, kind: NodeKind::Constraint }
    }

    #[test]
    fn empty_map_resolves_to_kind_derived_default() {
        let m = NodeTraitsMap::<TestKey>::default();
        assert_eq!(m.resolve(&value_key(0)), NodeTraits::IMMEDIATE);
        assert_eq!(
            m.resolve(&compute_key(0)),
            NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE)
        );
        assert_eq!(m.resolve(&constraint_key(0)), NodeTraits::empty());
    }

    #[test]
    fn set_type_resolves_to_type_value_for_matching_kind() {
        let mut m = NodeTraitsMap::<TestKey>::default();
        m.set_type(NodeKind::Compute, NodeTraits::PROGRESSIVE);
        assert_eq!(m.resolve(&compute_key(1)), NodeTraits::PROGRESSIVE);
    }

    #[test]
    fn set_type_isolates_other_kinds() {
        let mut m = NodeTraitsMap::<TestKey>::default();
        m.set_type(NodeKind::Compute, NodeTraits::PROGRESSIVE);
        // Value default unaffected
        assert_eq!(m.resolve(&value_key(1)), NodeTraits::IMMEDIATE);
    }

    #[test]
    fn set_instance_resolves_to_instance_value() {
        let mut m = NodeTraitsMap::<TestKey>::default();
        let key = compute_key(7);
        m.set_instance(key.clone(), NodeTraits::PROGRESSIVE.union(NodeTraits::COMMITTABLE));
        assert_eq!(
            m.resolve(&key),
            NodeTraits::PROGRESSIVE.union(NodeTraits::COMMITTABLE)
        );
    }

    #[test]
    fn instance_wins_over_type_wins_over_default() {
        let mut m = NodeTraitsMap::<TestKey>::default();
        let x = NodeTraits::WARM_STARTABLE;
        let y = NodeTraits::PROGRESSIVE;
        m.set_type(NodeKind::Compute, x);
        let key_42 = compute_key(42);
        let key_99 = compute_key(99);
        m.set_instance(key_42.clone(), y);
        assert_eq!(m.resolve(&key_42), y);
        assert_eq!(m.resolve(&key_99), x);
        // Value kind falls back to kind default (unaffected by Compute type override)
        assert_eq!(m.resolve(&value_key(0)), NodeTraits::IMMEDIATE);
    }

    #[test]
    fn set_instance_isolates_other_node_ids() {
        let mut m = NodeTraitsMap::<TestKey>::default();
        let key_a = compute_key(1);
        let key_b = compute_key(2);
        m.set_instance(key_a, NodeTraits::PROGRESSIVE);
        // key_b resolves to Compute kind default
        assert_eq!(
            m.resolve(&key_b),
            NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE)
        );
    }

    #[test]
    fn set_type_overwrites_previous_value() {
        let mut m = NodeTraitsMap::<TestKey>::default();
        m.set_type(NodeKind::Compute, NodeTraits::IMMEDIATE);
        m.set_type(NodeKind::Compute, NodeTraits::PROGRESSIVE);
        assert_eq!(m.resolve(&compute_key(0)), NodeTraits::PROGRESSIVE);
    }

    #[test]
    fn set_instance_overwrites_previous_value() {
        let mut m = NodeTraitsMap::<TestKey>::default();
        let key = compute_key(5);
        m.set_instance(key.clone(), NodeTraits::IMMEDIATE);
        m.set_instance(key.clone(), NodeTraits::PROGRESSIVE);
        assert_eq!(m.resolve(&key), NodeTraits::PROGRESSIVE);
    }

    #[test]
    fn set_instance_constraint_immediate_override_is_returned_verbatim() {
        // Q-6 resolution: per-instance Constraint+IMMEDIATE is allowed without
        // any code-level ceiling — resolve returns the instance value verbatim.
        let mut m = NodeTraitsMap::<TestKey>::default();
        let key = constraint_key(1);
        m.set_instance(key.clone(), NodeTraits::IMMEDIATE);
        assert_eq!(m.resolve(&key), NodeTraits::IMMEDIATE);
    }

    #[test]
    fn default_is_empty_for_both_maps() {
        let m = NodeTraitsMap::<TestKey>::default();
        // Sanity: no stale entries; each kind falls back to default_traits()
        assert_eq!(m.resolve(&value_key(0)), NodeKind::Value.default_traits());
        assert_eq!(m.resolve(&compute_key(0)), NodeKind::Compute.default_traits());
        assert_eq!(
            m.resolve(&constraint_key(0)),
            NodeKind::Constraint.default_traits()
        );
        assert_eq!(
            m.resolve(&TestKey { id: 0, kind: NodeKind::Realization }),
            NodeKind::Realization.default_traits()
        );
        assert_eq!(
            m.resolve(&TestKey { id: 0, kind: NodeKind::Resolution }),
            NodeKind::Resolution.default_traits()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Step 1: empty(), default(), is_empty() ---

    #[test]
    fn empty_has_no_flags() {
        let empty = NodeTraits::empty();
        assert_eq!(empty, NodeTraits::default());
        assert!(empty.is_empty());
    }

    #[test]
    fn default_equals_empty() {
        assert_eq!(NodeTraits::default(), NodeTraits::empty());
    }

    #[test]
    fn is_empty_true_for_empty() {
        assert!(NodeTraits::empty().is_empty());
    }

    // --- Step 3: flag constants, pairwise distinct, non-empty, contains ---

    #[test]
    fn flag_constants_are_non_empty() {
        assert!(!NodeTraits::IMMEDIATE.is_empty());
        assert!(!NodeTraits::WARM_STARTABLE.is_empty());
        assert!(!NodeTraits::PROGRESSIVE.is_empty());
        assert!(!NodeTraits::COMMITTABLE.is_empty());
    }

    #[test]
    fn flag_constants_are_pairwise_distinct() {
        assert_ne!(NodeTraits::IMMEDIATE, NodeTraits::WARM_STARTABLE);
        assert_ne!(NodeTraits::IMMEDIATE, NodeTraits::PROGRESSIVE);
        assert_ne!(NodeTraits::IMMEDIATE, NodeTraits::COMMITTABLE);
        assert_ne!(NodeTraits::WARM_STARTABLE, NodeTraits::PROGRESSIVE);
        assert_ne!(NodeTraits::WARM_STARTABLE, NodeTraits::COMMITTABLE);
        assert_ne!(NodeTraits::PROGRESSIVE, NodeTraits::COMMITTABLE);
    }

    #[test]
    fn contains_self() {
        assert!(NodeTraits::IMMEDIATE.contains(NodeTraits::IMMEDIATE));
        assert!(NodeTraits::WARM_STARTABLE.contains(NodeTraits::WARM_STARTABLE));
        assert!(NodeTraits::PROGRESSIVE.contains(NodeTraits::PROGRESSIVE));
        assert!(NodeTraits::COMMITTABLE.contains(NodeTraits::COMMITTABLE));
    }

    #[test]
    fn does_not_contain_other_flag() {
        assert!(!NodeTraits::IMMEDIATE.contains(NodeTraits::WARM_STARTABLE));
        assert!(!NodeTraits::IMMEDIATE.contains(NodeTraits::PROGRESSIVE));
        assert!(!NodeTraits::IMMEDIATE.contains(NodeTraits::COMMITTABLE));
        assert!(!NodeTraits::WARM_STARTABLE.contains(NodeTraits::IMMEDIATE));
        assert!(!NodeTraits::WARM_STARTABLE.contains(NodeTraits::PROGRESSIVE));
        assert!(!NodeTraits::WARM_STARTABLE.contains(NodeTraits::COMMITTABLE));
    }

    // --- Step 5: bitwise operators and const helpers ---

    #[test]
    fn bitor_contains_both_flags() {
        let combined = NodeTraits::IMMEDIATE | NodeTraits::COMMITTABLE;
        assert!(combined.contains(NodeTraits::IMMEDIATE));
        assert!(combined.contains(NodeTraits::COMMITTABLE));
        assert!(!combined.contains(NodeTraits::WARM_STARTABLE));
        assert!(!combined.contains(NodeTraits::PROGRESSIVE));
    }

    #[test]
    fn bitand_gives_intersection() {
        let warm_committable = NodeTraits::WARM_STARTABLE | NodeTraits::COMMITTABLE;
        let result = warm_committable & NodeTraits::WARM_STARTABLE;
        assert_eq!(result, NodeTraits::WARM_STARTABLE);
    }

    #[test]
    fn bitor_assign_mutates_in_place() {
        let mut t = NodeTraits::IMMEDIATE;
        t |= NodeTraits::COMMITTABLE;
        assert_eq!(t, NodeTraits::IMMEDIATE | NodeTraits::COMMITTABLE);
    }

    #[test]
    fn bitand_assign_mutates_in_place() {
        let mut t = NodeTraits::WARM_STARTABLE | NodeTraits::COMMITTABLE;
        t &= NodeTraits::WARM_STARTABLE;
        assert_eq!(t, NodeTraits::WARM_STARTABLE);
    }

    #[test]
    fn not_immediate_excludes_immediate_includes_others() {
        let not_immediate = !NodeTraits::IMMEDIATE;
        assert!(!not_immediate.contains(NodeTraits::IMMEDIATE));
        assert!(not_immediate.contains(NodeTraits::WARM_STARTABLE));
        assert!(not_immediate.contains(NodeTraits::PROGRESSIVE));
        assert!(not_immediate.contains(NodeTraits::COMMITTABLE));
    }

    #[test]
    fn not_empty_contains_exactly_all_four_flags() {
        // !empty() must be exactly the OR of all four declared flags — this locks
        // in the ALL_MASK contract: no stray bits leak outside the four-flag domain.
        let all_flags = NodeTraits::IMMEDIATE
            | NodeTraits::WARM_STARTABLE
            | NodeTraits::PROGRESSIVE
            | NodeTraits::COMMITTABLE;
        assert_eq!(!NodeTraits::empty(), all_flags);
    }

    #[test]
    fn not_all_flags_is_empty() {
        let all_flags = NodeTraits::IMMEDIATE
            | NodeTraits::WARM_STARTABLE
            | NodeTraits::PROGRESSIVE
            | NodeTraits::COMMITTABLE;
        assert!((!all_flags).is_empty());
    }

    #[test]
    fn union_equals_bitor() {
        assert_eq!(
            NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE),
            NodeTraits::WARM_STARTABLE | NodeTraits::COMMITTABLE
        );
    }

    #[test]
    fn intersection_equals_bitand() {
        let combined = NodeTraits::WARM_STARTABLE | NodeTraits::COMMITTABLE;
        assert_eq!(
            combined.intersection(NodeTraits::WARM_STARTABLE),
            NodeTraits::WARM_STARTABLE
        );
    }

    // Compile-time check: const expressible via union
    const FOO: NodeTraits = NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE);

    #[test]
    fn const_union_is_usable() {
        assert_eq!(FOO, NodeTraits::WARM_STARTABLE | NodeTraits::COMMITTABLE);
    }

    // --- NodeKind (canonical 5-variant enum, mirrors NodeId) ---

    #[test]
    fn node_kind_variants_are_distinct() {
        use NodeKind::*;
        assert_ne!(Value, Constraint);
        assert_ne!(Value, Realization);
        assert_ne!(Value, Resolution);
        assert_ne!(Value, Compute);
        assert_ne!(Constraint, Realization);
        assert_ne!(Constraint, Resolution);
        assert_ne!(Constraint, Compute);
        assert_ne!(Realization, Resolution);
        assert_ne!(Realization, Compute);
        assert_ne!(Resolution, Compute);
    }

    #[test]
    fn node_kind_value_default_traits() {
        assert_eq!(NodeKind::Value.default_traits(), NodeTraits::IMMEDIATE);
    }

    #[test]
    fn node_kind_constraint_default_traits() {
        // Q-1 resolution: empty, preserving the prior ConstraintNode default-traits behavior (now NodeKind::Constraint).
        assert_eq!(NodeKind::Constraint.default_traits(), NodeTraits::empty());
    }

    #[test]
    fn node_kind_resolution_default_traits() {
        assert_eq!(
            NodeKind::Resolution.default_traits(),
            NodeTraits::WARM_STARTABLE | NodeTraits::COMMITTABLE
        );
    }

    #[test]
    fn node_kind_realization_default_traits() {
        assert_eq!(
            NodeKind::Realization.default_traits(),
            NodeTraits::WARM_STARTABLE | NodeTraits::COMMITTABLE
        );
    }

    #[test]
    fn node_kind_compute_default_traits() {
        assert_eq!(
            NodeKind::Compute.default_traits(),
            NodeTraits::WARM_STARTABLE | NodeTraits::COMMITTABLE
        );
    }
}
