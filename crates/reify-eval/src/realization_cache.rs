//! Multi-dimensional cache keyed by `(entity_id, repr_kind, tol)`.
//!
//! [`RealizationCache<V>`] stores one [`crate::tolerance_bucket::ToleranceBucket<V>`] per
//! `(entity_id, repr_kind)` outer key and delegates partial-order insert/lookup/eviction
//! to the inner bucket.
//!
//! # Cache semantics
//!
//! The outer key is a `(entity_id: String, repr_kind: ReprKind)` tuple.  Each bucket
//! implements the "tighter satisfies looser" rule: a cached entry at tolerance `t_cached`
//! satisfies a request at `t_req` when `t_cached ≤ t_req`.
//!
//! # Keying design (per PRD `docs/prds/v0_2/multi-kernel.md` "Resolved design decisions")
//!
//! The outer key is `(entity_id: String, repr_kind: ReprKind)` — the two-dimensional
//! classifier introduced by the multi-kernel PRD.  `ReprKind` (BRep | Mesh | Sdf | Voxel)
//! identifies the kernel-family that produced the representation; `entity_id` identifies
//! the source entity.  Together they uniquely partition the tolerance space.
//!
//! This module introduces the data structure with the final `(entity_id, repr_kind, tol)`
//! keying.  It is *not* wired into `CacheStore` or `NodeId::Realization` —
//! that is task 2641's responsibility.
//!
//! The public API takes `entity: &str` and `repr_kind: ReprKind` as separate arguments
//! rather than a combined key struct, keeping the API decoupled from the internal
//! `HashMap<(String, ReprKind), ToleranceBucket<V>>` key shape.  Task 2641 may
//! upgrade to `&RealizationNodeId` if richer identity is needed.

use std::collections::HashMap;

use reify_types::ReprKind;

use crate::tolerance_bucket::ToleranceBucket;

/// Cache keyed by `(entity_id, repr_kind, tol: f64)`.
///
/// Internally stores one [`ToleranceBucket<V>`] per `(entity_id, repr_kind)` pair.
/// Partial-order insert/lookup and bounded-cardinality eviction are handled by the
/// inner bucket.
#[derive(Debug, Default)]
pub struct RealizationCache<V> {
    buckets: HashMap<(String, ReprKind), ToleranceBucket<V>>,
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
            .entry((entity.to_owned(), repr_kind))
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
            .get(&(entity.to_owned(), repr_kind))
            .and_then(|b| b.lookup(tol))
    }

    /// Returns the number of entries in the bucket for `(entity, repr_kind)`.
    pub fn bucket_len(&self, entity: &str, repr_kind: ReprKind) -> usize {
        self.buckets
            .get(&(entity.to_owned(), repr_kind))
            .map_or(0, |b| b.len())
    }

    /// Returns `true` if no entries are cached.
    pub fn is_empty(&self) -> bool {
        self.buckets.values().all(|b| b.is_empty())
    }

    /// Returns the total number of cached entries across all buckets.
    pub fn len(&self) -> usize {
        self.buckets.values().map(|b| b.len()).sum()
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
}
