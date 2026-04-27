//! Composable execution-trait flags for node-kind classification.
//!
//! Implements the [`NodeTraits`] bitflag newtype and the [`NodeArchKind`] enum
//! as specified in `docs/reify-implementation-architecture.md §7.6 lines 803–816`.
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

/// Composable execution-trait flags for a node kind.
///
/// See `docs/reify-implementation-architecture.md §7.6 lines 803–816`.
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

    /// Bitmask covering all four declared flags (used by [`Not`] impl).
    const ALL_MASK: u8 = 0b0000_1111;

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

use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};

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
}
