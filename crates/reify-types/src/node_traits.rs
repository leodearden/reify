//! Composable execution-trait flags for node-kind classification.
//!
//! Implements the [`NodeTraits`] bitflag newtype (four trait flags from §7.6 "Node Traits")
//! and the [`NodeArchKind`] enum (seven-kind taxonomy from §2.1 "Node Taxonomy (7 Types)");
//! see `docs/reify-implementation-architecture.md`.
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

/// Architectural node-kind taxonomy used to carry [`NodeTraits`] defaults.
///
/// Declares the seven node kinds from `docs/reify-implementation-architecture.md
/// §2.1 "Node Taxonomy (7 Types)"` and their documented default [`NodeTraits`] sets
/// (trait flags defined in §7.6 "Node Traits").
///
/// **Note on naming:** A 4-variant runtime/instance taxonomy also named `NodeKind`
/// exists in `reify_runtime::commitment` (introduced by task 2353 to key per-type
/// commitment policy overrides under §7.3). This enum is intentionally distinct:
/// it operates at the *type/architectural* level, includes three kinds
/// (`SchemaNode`, `SourceNode`, `ComputeNode`) whose Rust struct counterparts do not
/// yet exist in the codebase, and lives in `reify-types` (a lower layer that
/// `reify-runtime` depends on — the dependency arrow is `reify-runtime` → `reify-types`,
/// so this crate cannot reference `commitment::NodeKind`). Once future tasks introduce
/// the missing struct counterparts, a follow-up pass can decide whether to converge
/// the two enums.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeArchKind {
    /// A scalar value cell. Default traits: [`NodeTraits::IMMEDIATE`].
    ///
    /// See §2.1 "Node Taxonomy (7 Types)" and §7.6 "Node Traits" in
    /// `docs/reify-implementation-architecture.md`.
    ValueCellScalar,
    /// A schema node (structural type declaration). Default traits: [`NodeTraits::IMMEDIATE`].
    ///
    /// See §2.1 "Node Taxonomy (7 Types)" and §7.6 "Node Traits" in
    /// `docs/reify-implementation-architecture.md`.
    /// (No corresponding Rust struct in the codebase yet.)
    SchemaNode,
    /// A source/input node. Default traits: [`NodeTraits::IMMEDIATE`].
    ///
    /// See §2.1 "Node Taxonomy (7 Types)" and §7.6 "Node Traits" in
    /// `docs/reify-implementation-architecture.md`.
    /// (No corresponding Rust struct in the codebase yet.)
    SourceNode,
    /// A resolution node. Default traits: `WARM_STARTABLE | COMMITTABLE`.
    ///
    /// See §2.1 "Node Taxonomy (7 Types)" and §7.6 "Node Traits" in
    /// `docs/reify-implementation-architecture.md`.
    ResolutionNode,
    /// A realization node. Default traits: `WARM_STARTABLE | COMMITTABLE`.
    ///
    /// See §2.1 "Node Taxonomy (7 Types)" and §7.6 "Node Traits" in
    /// `docs/reify-implementation-architecture.md`.
    RealizationNode,
    /// A compute node. Default traits: `WARM_STARTABLE | COMMITTABLE`.
    ///
    /// See §2.1 "Node Taxonomy (7 Types)" and §7.6 "Node Traits" in
    /// `docs/reify-implementation-architecture.md`.
    /// (No corresponding Rust struct in the codebase yet.)
    ComputeNode,
    /// A constraint node. Default traits: empty (no flags set).
    ///
    /// See §2.1 "Node Taxonomy (7 Types)" and §7.6 "Node Traits" in
    /// `docs/reify-implementation-architecture.md`.
    ConstraintNode,
}

impl NodeArchKind {
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
    /// **`IMMEDIATE` kinds** (`ValueCellScalar`, `SchemaNode`, `SourceNode`): §3.3 classifies
    /// these as P0/P1-fast — cheap, sub-frame reads evaluable inline. No warm-start or
    /// commitment machinery is required.
    ///
    /// **`WARM_STARTABLE | COMMITTABLE` kinds** (`ResolutionNode`, `RealizationNode`,
    /// `ComputeNode`): §4.1 targets these as the long-running iterative/incremental
    /// computations that benefit from resuming a saved warm state. §7.3 notes that they
    /// may run past a commitment threshold and must therefore finish against their original
    /// snapshot, justifying `COMMITTABLE`.
    ///
    /// **`ConstraintNode` (empty)**: Predicate evaluation is cheap but §7.6 does not yet
    /// classify it under any of the four traits. Assigning `IMMEDIATE` would conflate it
    /// with sub-frame `ValueCell` reads; leaving the set empty is the conservative choice
    /// until a downstream scheduler task formalises the policy.
    pub const fn default_traits(self) -> NodeTraits {
        match self {
            NodeArchKind::ValueCellScalar => NodeTraits::IMMEDIATE,
            NodeArchKind::SchemaNode => NodeTraits::IMMEDIATE,
            NodeArchKind::SourceNode => NodeTraits::IMMEDIATE,
            NodeArchKind::ResolutionNode => {
                NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE)
            }
            NodeArchKind::RealizationNode => {
                NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE)
            }
            NodeArchKind::ComputeNode => {
                NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE)
            }
            NodeArchKind::ConstraintNode => NodeTraits::empty(),
        }
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

    // --- Step 7: NodeArchKind variants and default_traits() ---

    #[test]
    fn node_arch_kind_variants_are_distinct() {
        use NodeArchKind::*;
        assert_ne!(ValueCellScalar, SchemaNode);
        assert_ne!(ValueCellScalar, SourceNode);
        assert_ne!(ValueCellScalar, ResolutionNode);
        assert_ne!(ValueCellScalar, RealizationNode);
        assert_ne!(ValueCellScalar, ComputeNode);
        assert_ne!(ValueCellScalar, ConstraintNode);
        assert_ne!(SchemaNode, SourceNode);
        assert_ne!(ResolutionNode, RealizationNode);
        assert_ne!(RealizationNode, ComputeNode);
    }

    #[test]
    fn value_cell_scalar_default_traits() {
        assert_eq!(
            NodeArchKind::ValueCellScalar.default_traits(),
            NodeTraits::IMMEDIATE
        );
    }

    #[test]
    fn schema_node_default_traits() {
        assert_eq!(
            NodeArchKind::SchemaNode.default_traits(),
            NodeTraits::IMMEDIATE
        );
    }

    #[test]
    fn source_node_default_traits() {
        assert_eq!(
            NodeArchKind::SourceNode.default_traits(),
            NodeTraits::IMMEDIATE
        );
    }

    #[test]
    fn resolution_node_default_traits() {
        assert_eq!(
            NodeArchKind::ResolutionNode.default_traits(),
            NodeTraits::WARM_STARTABLE | NodeTraits::COMMITTABLE
        );
    }

    #[test]
    fn realization_node_default_traits() {
        assert_eq!(
            NodeArchKind::RealizationNode.default_traits(),
            NodeTraits::WARM_STARTABLE | NodeTraits::COMMITTABLE
        );
    }

    #[test]
    fn compute_node_default_traits() {
        assert_eq!(
            NodeArchKind::ComputeNode.default_traits(),
            NodeTraits::WARM_STARTABLE | NodeTraits::COMMITTABLE
        );
    }

    #[test]
    fn constraint_node_default_traits() {
        assert_eq!(
            NodeArchKind::ConstraintNode.default_traits(),
            NodeTraits::empty()
        );
    }
}
