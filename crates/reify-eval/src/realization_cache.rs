//! Multi-dimensional cache keyed by `(entity_id, repr_kind, tol)`.
//!
//! [`RealizationCache<V>`] stores one [`crate::tolerance_bucket::ToleranceBucket<V>`] per
//! `(entity_id, repr_kind)` outer key and delegates partial-order insert/lookup/eviction
//! to the inner bucket.
//!
//! # Cache semantics
//!
//! The outer key is `(repr_kind: ReprKind, entity_id: &str)` addressed through a two-level
//! nested map.  Each bucket implements the "tighter satisfies looser" rule: a cached entry
//! at tolerance `t_cached` satisfies a request at `t_req` when `t_cached ≤ t_req`.
//!
//! # Keying design (per PRD `docs/prds/v0_2/multi-kernel.md` "Resolved design decisions")
//!
//! The logical key is `(entity_id, repr_kind)` — the two-dimensional classifier introduced
//! by the multi-kernel PRD.  `ReprKind` (BRep | Mesh | Sdf | Voxel) identifies the
//! kernel-family that produced the representation; `entity_id` identifies the source entity.
//! Together they uniquely partition the tolerance space.
//!
//! ## Storage layout
//!
//! Internally uses `HashMap<ReprKind, HashMap<String, ToleranceBucket<V>>>` (nested maps).
//! This keeps the hot read paths (`lookup`, `bucket_len`) allocation-free:
//!
//! - The outer lookup keys on `ReprKind`, which is `Copy` (no heap allocation).
//! - The inner `HashMap<String, …>` supports `&str` lookup via the standard
//!   `Borrow<str>` implementation — no `entity.to_owned()` needed on reads.
//!
//! The flat-tuple alternative `HashMap<(String, ReprKind), …>` cannot be queried
//! with `(&str, ReprKind)` because the `Borrow` trait is not implemented for
//! heterogeneous tuples, forcing an allocation per read.
//!
//! `insert` only allocates a new `String` key when an entity first appears under a
//! given `repr_kind`; that allocation is unavoidable and bounded to at most one per
//! `(entity, repr_kind)` pair.
//!
//! This module introduces the data structure with the final `(entity_id, repr_kind, tol)`
//! keying.  It is *not* wired into `CacheStore` or `NodeId::Realization` —
//! that is task 2641's responsibility.
//!
//! The public API takes `entity: &str` and `repr_kind: ReprKind` as separate arguments
//! rather than a combined key struct, keeping the API decoupled from the internal
//! storage shape.  Task 2641 may upgrade to `&RealizationNodeId` if richer identity
//! is needed.

use std::collections::HashMap;

use reify_types::ReprKind;

use crate::tolerance_bucket::ToleranceBucket;

/// Cache keyed by `(entity_id, repr_kind, tol: f64)`.
///
/// Internally uses a nested `HashMap<ReprKind, HashMap<String, ToleranceBucket<V>>>` so
/// that read paths (`lookup`, `bucket_len`) are allocation-free — the outer key is
/// `ReprKind` (a `Copy` type) and the inner map supports `&str` lookup via `Borrow<str>`.
/// Partial-order insert/lookup and bounded-cardinality eviction are handled by each
/// inner [`ToleranceBucket`].
#[derive(Debug, Default)]
pub struct RealizationCache<V> {
    buckets: HashMap<ReprKind, HashMap<String, ToleranceBucket<V>>>,
}

impl<V> RealizationCache<V> {
    /// Creates an empty `RealizationCache`.
    pub fn new() -> Self {
        Self {
            buckets: HashMap::new(),
        }
    }

    /// Inserts `val` at `(entity, repr_kind, tol)`.
    ///
    /// Returns `true` if the entry was inserted, or `false` if an existing entry with
    /// a tighter (or equal) tolerance already satisfies this tolerance — mirroring
    /// [`ToleranceBucket::insert`]'s semantics.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `tol` is NaN, infinite, or negative.
    /// This forwards [`ToleranceBucket`]'s precondition: `tol` must be finite and
    /// non-negative (`tol.is_finite() && tol >= 0.0`).
    pub fn insert(&mut self, entity: &str, repr_kind: ReprKind, tol: f64, val: V) -> bool {
        self.buckets
            .entry(repr_kind)
            .or_default()
            .entry(entity.to_owned())
            .or_insert_with(ToleranceBucket::new)
            .insert(tol, val)
    }

    /// Looks up the loosest cached entry that satisfies `tol` under `(entity, repr_kind)`.
    ///
    /// Returns `Some(&val)` for the loosest satisfying entry (`cached_tol ≤ tol`), or
    /// `None` if no entry satisfies.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `tol` is NaN, infinite, or negative.
    /// This forwards [`ToleranceBucket`]'s precondition: `tol` must be finite and
    /// non-negative (`tol.is_finite() && tol >= 0.0`).
    pub fn lookup(&self, entity: &str, repr_kind: ReprKind, tol: f64) -> Option<&V> {
        self.buckets
            .get(&repr_kind)
            .and_then(|inner| inner.get(entity))
            .and_then(|b| b.lookup(tol))
    }

    /// Returns the number of entries in the bucket for `(entity, repr_kind)`.
    pub fn bucket_len(&self, entity: &str, repr_kind: ReprKind) -> usize {
        self.buckets
            .get(&repr_kind)
            .and_then(|inner| inner.get(entity))
            .map_or(0, |b| b.len())
    }

    /// Returns `true` if no entries are cached.
    pub fn is_empty(&self) -> bool {
        self.buckets
            .values()
            .all(|inner| inner.values().all(|b| b.is_empty()))
    }

    /// Returns the total number of cached entries across all buckets.
    pub fn len(&self) -> usize {
        self.buckets
            .values()
            .flat_map(|inner| inner.values())
            .map(|b| b.len())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use reify_types::ReprKind;

    use super::RealizationCache;

    /// Happy-path: single (entity, repr_kind) round-trip with exact and looser lookup.
    ///
    /// - Insert at tol=0.01 → exact hit at 0.01.
    /// - Looser request (tol=0.1) is satisfied by the tighter cached entry.
    /// - Tighter request (tol=0.001) misses because no cached entry satisfies it.
    #[test]
    fn insert_and_lookup_partial_order_single_repr_kind() {
        let mut cache = RealizationCache::<u32>::new();
        cache.insert("Bracket", ReprKind::BRep, 0.01, 42);

        // Exact tolerance hit.
        assert_eq!(
            cache.lookup("Bracket", ReprKind::BRep, 0.01),
            Some(&42),
            "exact tolerance should hit"
        );

        // Looser request: tighter cached entry (0.01) satisfies request (0.1).
        assert_eq!(
            cache.lookup("Bracket", ReprKind::BRep, 0.1),
            Some(&42),
            "looser request must hit tighter cached entry"
        );

        // Tighter request: cached 0.01 does NOT satisfy a request for 0.001.
        assert!(
            cache.lookup("Bracket", ReprKind::BRep, 0.001).is_none(),
            "tighter request than cached must miss"
        );
    }

    /// Two different `repr_kind`s under the same entity must have independent buckets.
    ///
    /// If `repr_kind` were ignored in the cache key, both inserts would land in the same
    /// `ToleranceBucket`.  The second insert at the same tolerance would then be REJECTED
    /// (existing entry satisfies it), and `lookup("A", ReprKind::Mesh, 0.01)` would return
    /// `Some(&1)` instead of `Some(&2)`.  This test guards against that regression.
    #[test]
    fn repr_kind_distinguishes_buckets_under_same_entity() {
        let mut cache = RealizationCache::<u32>::new();
        cache.insert("A", ReprKind::BRep, 0.01, 1);
        cache.insert("A", ReprKind::Mesh, 0.01, 2);

        assert_eq!(
            cache.lookup("A", ReprKind::BRep, 0.01),
            Some(&1),
            "BRep bucket should hold value 1"
        );
        assert_eq!(
            cache.lookup("A", ReprKind::Mesh, 0.01),
            Some(&2),
            "Mesh bucket must be independent of BRep bucket and hold value 2"
        );
    }

    /// Lookups for entities that were never inserted must return `None`.
    ///
    /// Tested on both an empty cache and a cache that already has entries for a
    /// different entity — the miss must not bleed across entity boundaries.
    #[test]
    fn lookup_misses_for_unknown_entity() {
        // Empty cache.
        let cache = RealizationCache::<u32>::new();
        assert_eq!(
            cache.lookup("MissingEntity", ReprKind::BRep, 0.01),
            None,
            "empty cache must return None"
        );

        // Populated cache with a different entity.
        let mut cache = RealizationCache::<u32>::new();
        cache.insert("KnownEntity", ReprKind::BRep, 0.01, 99);
        assert_eq!(
            cache.lookup("MissingEntity", ReprKind::BRep, 0.01),
            None,
            "lookup for unknown entity must not bleed from known entity"
        );
    }

    /// Two distinct entity IDs under the same `repr_kind` must have independent buckets.
    ///
    /// This is the dual of `repr_kind_distinguishes_buckets_under_same_entity`: here the
    /// repr_kind is fixed and the entity varies.  Guards against an implementation bug
    /// that ignores the entity portion of the key.
    #[test]
    fn entity_id_distinguishes_buckets_under_same_repr_kind() {
        let mut cache = RealizationCache::<u32>::new();
        cache.insert("X", ReprKind::BRep, 0.01, 10);
        cache.insert("Y", ReprKind::BRep, 0.01, 20);

        assert_eq!(
            cache.lookup("X", ReprKind::BRep, 0.01),
            Some(&10),
            "entity X bucket must hold value 10"
        );
        assert_eq!(
            cache.lookup("Y", ReprKind::BRep, 0.01),
            Some(&20),
            "entity Y bucket must be independent of X and hold value 20"
        );
    }

    /// `len()` and `is_empty()` must track insertions accurately.
    ///
    /// A fresh cache starts empty; each successful insert increments `len()`.
    /// Two inserts under different `(entity, repr_kind)` keys produce `len() == 2`.
    #[test]
    fn len_and_is_empty_track_inserts() {
        let mut cache = RealizationCache::<u32>::new();

        // Fresh cache is empty.
        assert!(cache.is_empty(), "new cache must be empty");
        assert_eq!(cache.len(), 0, "new cache len must be 0");

        // One insert: len becomes 1.
        cache.insert("E1", ReprKind::BRep, 0.01, 1);
        assert!(!cache.is_empty(), "cache must not be empty after first insert");
        assert_eq!(cache.len(), 1, "len must be 1 after first insert");

        // Second insert under a different (entity, repr_kind) pair: len becomes 2.
        cache.insert("E1", ReprKind::Mesh, 0.01, 2);
        assert_eq!(cache.len(), 2, "len must be 2 after two inserts at different keys");
    }

    /// Inserting at a looser tolerance when a tighter entry is already cached must be
    /// rejected (`insert` returns `false`) and must not displace the tighter entry.
    ///
    /// Partial-order rule: a cached entry at `t_cached` satisfies any request at
    /// `t_req ≥ t_cached`.  Inserting a new entry at `t_new > t_cached` would be
    /// redundant — every consumer that could use the new entry can also use the
    /// existing tighter one.  The cache must reject the redundant insert.
    #[test]
    fn looser_insert_rejected_when_tighter_cached() {
        let mut cache = RealizationCache::<u32>::new();

        // Insert at tighter tolerance (0.01).
        let first = cache.insert("A", ReprKind::BRep, 0.01, 1);
        assert!(first, "first insert must succeed");
        assert_eq!(cache.len(), 1);

        // Attempt to insert at looser tolerance (0.1); existing 0.01 ≤ 0.1 → reject.
        let second = cache.insert("A", ReprKind::BRep, 0.1, 2);
        assert!(!second, "looser insert must be rejected when tighter entry is cached");

        // The original tighter value must still be present.
        assert_eq!(
            cache.lookup("A", ReprKind::BRep, 0.01),
            Some(&1),
            "tighter cached entry must not be displaced by rejected looser insert"
        );
        assert_eq!(cache.len(), 1, "len must remain 1 after rejected insert");
    }

    /// After more than `SOFT_CAPACITY` inserts under one `(entity, repr_kind)`, the
    /// bucket is capped at `SOFT_CAPACITY` via eviction of the loosest entry.
    ///
    /// Confirms that `RealizationCache` correctly forwards to `ToleranceBucket`'s
    /// eviction logic and that `bucket_len` / `len` reflect the post-eviction count.
    #[test]
    fn cache_len_caps_at_soft_capacity_per_bucket() {
        use crate::tolerance_bucket::SOFT_CAPACITY;

        let mut cache = RealizationCache::<u32>::new();

        // Insert SOFT_CAPACITY + 1 entries, each strictly tighter than the previous.
        // With descending tolerances (0.1, 0.05, 0.04, 0.03, 0.02, 0.01), each new
        // entry is tighter than all existing ones (no existing cached_tol ≤ new_tol),
        // so every insert succeeds.  After the (SOFT_CAPACITY+1)-th insert the bucket
        // evicts its loosest (largest) entry, capping at SOFT_CAPACITY.
        let tols = [0.1_f64, 0.05, 0.04, 0.03, 0.02, 0.01];
        assert_eq!(tols.len(), SOFT_CAPACITY + 1);

        for (i, &t) in tols.iter().enumerate() {
            let accepted = cache.insert("E", ReprKind::BRep, t, i as u32);
            assert!(accepted, "insert at tol={t} must be accepted");
        }

        assert_eq!(
            cache.bucket_len("E", ReprKind::BRep),
            SOFT_CAPACITY,
            "bucket must be capped at SOFT_CAPACITY after eviction"
        );
        assert_eq!(
            cache.len(),
            SOFT_CAPACITY,
            "total cache len must equal SOFT_CAPACITY after single-bucket eviction"
        );
    }
}
