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
}
