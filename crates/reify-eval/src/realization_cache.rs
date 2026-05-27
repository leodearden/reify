//! Multi-dimensional cache keyed by `(entity_id, repr_kind, options_hash, tol)`.
//!
//! [`RealizationCache<V>`] stores one [`crate::tolerance_bucket::ToleranceBucket<V>`] per
//! `(entity_id, repr_kind, options_hash)` key triple and delegates partial-order
//! insert/lookup/eviction to the inner bucket.
//!
//! # Cache semantics
//!
//! The outer key is `(repr_kind: ReprKind, entity_id: &str, options_hash: ContentHash)`
//! addressed through a three-level nested map.  Each bucket implements the "tighter
//! satisfies looser" rule: a cached entry at tolerance `t_cached` satisfies a request at
//! `t_req` when `t_cached ≤ t_req`.  Partial-order matching is scoped per `options_hash` —
//! a hit in one bucket never satisfies a lookup under a different `options_hash`.
//!
//! # Keying design (per PRDs `docs/prds/v0_2/multi-kernel.md` and
//! `docs/prds/v0_3/multi-kernel-phase-3.md` §4 "Per-op option folding into the cache key")
//!
//! The logical key is `(entity_id, repr_kind, options_hash)` — a three-dimensional
//! classifier.  `ReprKind` (BRep | Mesh | Sdf | Voxel) identifies the kernel-family;
//! `entity_id` identifies the source entity; `options_hash` folds per-op option fields
//! (e.g. `force_tet` from `VolumeMeshOptions`) into the key so that two solves on
//! identical geometry with different options do not share a slot.
//!
//! `options_hash = ContentHash(0)` is the explicit "no options" sentinel for ops that
//! have no parameterisation — it is a first-class partition value, not magic.  See PRD §4
//! for the convention; it matches the `compute_cache_key.rs` `ContentHash(0)` baseline.
//!
//! The cache methods take separate arguments rather than an aggregate key struct —
//! see "Storage layout" for the allocation-free read path rationale.
//!
//! ## Storage layout
//!
//! Internally uses
//! `HashMap<ReprKind, HashMap<String, HashMap<ContentHash, ToleranceBucket<V>>>>` (three
//! nested maps).  This keeps the hot read paths (`lookup`, `bucket_len`) allocation-free:
//!
//! - The outer lookup keys on `ReprKind`, which is `Copy` (no heap allocation).
//! - The middle `HashMap<String, …>` supports `&str` lookup via the standard
//!   `Borrow<str>` implementation — no `entity.to_owned()` needed on reads.
//! - The innermost lookup keys on `ContentHash`, which is `Copy` (no heap allocation).
//!
//! The flat-tuple alternative `HashMap<(String, ReprKind), …>` cannot be queried
//! with `(&str, ReprKind)` because the `Borrow` trait is not implemented for
//! heterogeneous tuples, forcing an allocation per read.
//!
//! An alternative outer key `HashMap<(ReprKind, ContentHash), HashMap<String, …>>` was
//! rejected: it would allocate a fresh entity `String` per `(ReprKind, ContentHash)` pair,
//! breaking the allocation contract when multiple `options_hash` values appear for the
//! same entity.
//!
//! `insert` only allocates a new `String` key when an entity first appears under a
//! given `repr_kind`; that allocation is unavoidable and bounded to at most one per
//! `(entity, repr_kind)` pair.  Subsequent inserts at the same `(entity, repr_kind)` —
//! regardless of `options_hash` — take the `get_mut` fast path and produce zero `String`
//! allocations.  This invariant is enforced by the `get_mut` fast path in `insert` — do
//! not collapse it back to a single `entry().or_default()` chain, as that would call
//! `entity.to_owned()` unconditionally on every call.
//!
//! This module introduces the data structure with the final
//! `(entity_id, repr_kind, options_hash, tol)` keying.  It is *not* wired into
//! `CacheStore` or `NodeId::Realization` — that is task 2641's responsibility.
//!
//! The public API takes `entity: &str`, `repr_kind: ReprKind`, `tol: f64`, and
//! `options_hash: ContentHash` as separate arguments rather than a combined key struct,
//! keeping the API decoupled from the internal storage shape and preserving the
//! allocation-free read path.  Task 2641 may upgrade to `&RealizationNodeId` if richer
//! identity is needed.

use std::collections::HashMap;

use reify_core::ContentHash;
use reify_ir::ReprKind;

use crate::tolerance_bucket::ToleranceBucket;

/// Sentinel value for the "no options" case — pass at call sites that carry no
/// per-op parameterisation.
///
/// Defined in PRD §4 (`docs/prds/v0_3/multi-kernel-phase-3.md`); matches the
/// `ContentHash(0)` baseline convention in `compute_cache_key.rs`.
///
/// Task δ (3435) and task ξ (3442) will replace `NO_OPTIONS` at their
/// respective BRep / volume-mesh call sites with real per-op option hashes
/// once the option structs expose `ContentHash` output.  Grep for
/// `NO_OPTIONS` to locate every replacement target.
pub const NO_OPTIONS: ContentHash = ContentHash(0);

/// Cache keyed by `(entity_id, repr_kind, options_hash, tol: f64)`.
///
/// Internally uses a nested
/// `HashMap<ReprKind, HashMap<String, HashMap<ContentHash, ToleranceBucket<V>>>>` so
/// that read paths (`lookup`, `bucket_len`) are allocation-free — the outer key is
/// `ReprKind` (a `Copy` type), the middle map supports `&str` lookup via `Borrow<str>`,
/// and the innermost key is `ContentHash` (also `Copy`).
/// Partial-order insert/lookup and bounded-cardinality eviction are handled by each
/// inner [`ToleranceBucket`], scoped per `options_hash`.
#[derive(Debug, Default)]
pub struct RealizationCache<V> {
    buckets: HashMap<ReprKind, HashMap<String, HashMap<ContentHash, ToleranceBucket<V>>>>,
}

impl<V> RealizationCache<V> {
    /// Creates an empty `RealizationCache`.
    pub fn new() -> Self {
        Self {
            buckets: HashMap::new(),
        }
    }

    /// Inserts `val` at `(entity, repr_kind, options_hash, tol)`.
    ///
    /// Returns `true` if the entry was inserted, or `false` if an existing entry with
    /// a tighter (or equal) tolerance already satisfies this tolerance within the same
    /// `options_hash` bucket — mirroring [`ToleranceBucket::insert`]'s semantics.
    ///
    /// `options_hash = ContentHash(0)` is the "no options" sentinel (PRD §4).
    ///
    /// **Allocation discipline:** the entity `String` key is allocated at most once per
    /// `(entity, repr_kind)` pair, regardless of `options_hash`.  Subsequent inserts at
    /// the same `(entity, repr_kind)` take the `get_mut` fast path — zero String
    /// allocations.  Do not collapse the fast path to a single `entry().or_default()`
    /// chain; see module docs.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `tol` is NaN, infinite, or negative.
    /// This forwards [`ToleranceBucket`]'s precondition: `tol` must be finite and
    /// non-negative (`tol.is_finite() && tol >= 0.0`; see
    /// [`crate::tolerance_gate::is_valid_tolerance_si`]).
    pub fn insert(
        &mut self,
        entity: &str,
        repr_kind: ReprKind,
        tol: f64,
        options_hash: ContentHash,
        val: V,
    ) -> bool {
        let inner = self.buckets.entry(repr_kind).or_default();
        if let Some(by_options) = inner.get_mut(entity) {
            // Fast path: entity already present under this repr_kind — no String allocation.
            by_options
                .entry(options_hash)
                .or_insert_with(ToleranceBucket::new)
                .insert(tol, val)
        } else {
            // Slow path: first appearance of this entity under repr_kind — pay one to_owned().
            inner
                .entry(entity.to_owned())
                .or_default()
                .entry(options_hash)
                .or_insert_with(ToleranceBucket::new)
                .insert(tol, val)
        }
    }

    /// Looks up the loosest cached entry that satisfies `tol` under
    /// `(entity, repr_kind, options_hash)`.
    ///
    /// Returns `Some(&val)` for the loosest satisfying entry (`cached_tol ≤ tol`) within
    /// the `options_hash` bucket, or `None` if no entry satisfies.  A hit in one
    /// `options_hash` bucket never satisfies a lookup under a different `options_hash`.
    ///
    /// `options_hash = ContentHash(0)` is the "no options" sentinel (PRD §4).
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `tol` is NaN, infinite, or negative.
    /// This forwards [`ToleranceBucket`]'s precondition: `tol` must be finite and
    /// non-negative (`tol.is_finite() && tol >= 0.0`; see
    /// [`crate::tolerance_gate::is_valid_tolerance_si`]).
    pub fn lookup(
        &self,
        entity: &str,
        repr_kind: ReprKind,
        tol: f64,
        options_hash: ContentHash,
    ) -> Option<&V> {
        self.buckets
            .get(&repr_kind)
            .and_then(|inner| inner.get(entity))
            .and_then(|by_options| by_options.get(&options_hash))
            .and_then(|b| b.lookup(tol))
    }

    /// Returns the number of entries in the bucket for `(entity, repr_kind, options_hash)`.
    pub fn bucket_len(
        &self,
        entity: &str,
        repr_kind: ReprKind,
        options_hash: ContentHash,
    ) -> usize {
        self.buckets
            .get(&repr_kind)
            .and_then(|inner| inner.get(entity))
            .and_then(|by_options| by_options.get(&options_hash))
            .map_or(0, |b| b.len())
    }

    /// Returns `true` if no entries are cached.
    pub fn is_empty(&self) -> bool {
        self.buckets.values().all(|inner| {
            inner
                .values()
                .all(|by_options| by_options.values().all(|b| b.is_empty()))
        })
    }

    /// Returns the total number of cached entries across all buckets.
    pub fn len(&self) -> usize {
        self.buckets
            .values()
            .flat_map(|inner| inner.values())
            .flat_map(|by_options| by_options.values())
            .map(|b| b.len())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use reify_core::ContentHash;
    use reify_ir::ReprKind;

    use super::RealizationCache;

    /// Happy-path: single (entity, repr_kind) round-trip with exact and looser lookup.
    ///
    /// - Insert at tol=0.01 → exact hit at 0.01.
    /// - Looser request (tol=0.1) is satisfied by the tighter cached entry.
    /// - Tighter request (tol=0.001) misses because no cached entry satisfies it.
    #[test]
    fn insert_and_lookup_partial_order_single_repr_kind() {
        let mut cache = RealizationCache::<u32>::new();
        cache.insert("Bracket", ReprKind::BRep, 0.01, ContentHash(0), 42);

        // Exact tolerance hit.
        assert_eq!(
            cache.lookup("Bracket", ReprKind::BRep, 0.01, ContentHash(0)),
            Some(&42),
            "exact tolerance should hit"
        );

        // Looser request: tighter cached entry (0.01) satisfies request (0.1).
        assert_eq!(
            cache.lookup("Bracket", ReprKind::BRep, 0.1, ContentHash(0)),
            Some(&42),
            "looser request must hit tighter cached entry"
        );

        // Tighter request: cached 0.01 does NOT satisfy a request for 0.001.
        assert!(
            cache
                .lookup("Bracket", ReprKind::BRep, 0.001, ContentHash(0))
                .is_none(),
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
        cache.insert("A", ReprKind::BRep, 0.01, ContentHash(0), 1);
        cache.insert("A", ReprKind::Mesh, 0.01, ContentHash(0), 2);

        assert_eq!(
            cache.lookup("A", ReprKind::BRep, 0.01, ContentHash(0)),
            Some(&1),
            "BRep bucket should hold value 1"
        );
        assert_eq!(
            cache.lookup("A", ReprKind::Mesh, 0.01, ContentHash(0)),
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
            cache.lookup("MissingEntity", ReprKind::BRep, 0.01, ContentHash(0)),
            None,
            "empty cache must return None"
        );

        // Populated cache with a different entity.
        let mut cache = RealizationCache::<u32>::new();
        cache.insert("KnownEntity", ReprKind::BRep, 0.01, ContentHash(0), 99);
        assert_eq!(
            cache.lookup("MissingEntity", ReprKind::BRep, 0.01, ContentHash(0)),
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
        cache.insert("X", ReprKind::BRep, 0.01, ContentHash(0), 10);
        cache.insert("Y", ReprKind::BRep, 0.01, ContentHash(0), 20);

        assert_eq!(
            cache.lookup("X", ReprKind::BRep, 0.01, ContentHash(0)),
            Some(&10),
            "entity X bucket must hold value 10"
        );
        assert_eq!(
            cache.lookup("Y", ReprKind::BRep, 0.01, ContentHash(0)),
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
        cache.insert("E1", ReprKind::BRep, 0.01, ContentHash(0), 1);
        assert!(
            !cache.is_empty(),
            "cache must not be empty after first insert"
        );
        assert_eq!(cache.len(), 1, "len must be 1 after first insert");

        // Second insert under a different (entity, repr_kind) pair: len becomes 2.
        cache.insert("E1", ReprKind::Mesh, 0.01, ContentHash(0), 2);
        assert_eq!(
            cache.len(),
            2,
            "len must be 2 after two inserts at different keys"
        );
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
        let first = cache.insert("A", ReprKind::BRep, 0.01, ContentHash(0), 1);
        assert!(first, "first insert must succeed");
        assert_eq!(cache.len(), 1);

        // Attempt to insert at looser tolerance (0.1); existing 0.01 ≤ 0.1 → reject.
        let second = cache.insert("A", ReprKind::BRep, 0.1, ContentHash(0), 2);
        assert!(
            !second,
            "looser insert must be rejected when tighter entry is cached"
        );

        // The original tighter value must still be present.
        assert_eq!(
            cache.lookup("A", ReprKind::BRep, 0.01, ContentHash(0)),
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
            let accepted = cache.insert("E", ReprKind::BRep, t, ContentHash(0), i as u32);
            assert!(accepted, "insert at tol={t} must be accepted");
        }

        assert_eq!(
            cache.bucket_len("E", ReprKind::BRep, ContentHash(0)),
            SOFT_CAPACITY,
            "bucket must be capped at SOFT_CAPACITY after eviction"
        );
        assert_eq!(
            cache.len(),
            SOFT_CAPACITY,
            "total cache len must equal SOFT_CAPACITY after single-bucket eviction"
        );
    }

    /// Two distinct `options_hash` values under the same `(entity, repr_kind, tol)` must
    /// produce two distinct slots — neither value shadows the other.
    ///
    /// This is the mirror of `repr_kind_distinguishes_buckets_under_same_entity`, guarding
    /// the new `options_hash` dimension added in PRD §4.  If `options_hash` were ignored
    /// in the cache key, the second insert would land in the same `ToleranceBucket` as the
    /// first and be REJECTED (existing entry satisfies it), causing the lookup to return the
    /// wrong value for one of the two slots.
    #[test]
    fn options_hash_distinguishes_buckets_under_same_entity_and_repr_kind() {
        let hash_a = ContentHash::of_str("force_tet=true");
        let hash_b = ContentHash::of_str("force_tet=false");

        let mut cache = RealizationCache::<u32>::new();
        let inserted_a = cache.insert("A", ReprKind::BRep, 0.01, hash_a, 1);
        let inserted_b = cache.insert("A", ReprKind::BRep, 0.01, hash_b, 2);
        assert!(inserted_a, "insert under hash_a must succeed");
        assert!(
            inserted_b,
            "insert under hash_b must succeed (distinct slot)"
        );

        assert_eq!(
            cache.lookup("A", ReprKind::BRep, 0.01, hash_a),
            Some(&1),
            "lookup under hash_a must return value 1"
        );
        assert_eq!(
            cache.lookup("A", ReprKind::BRep, 0.01, hash_b),
            Some(&2),
            "lookup under hash_b must return value 2 (not shadowed by hash_a)"
        );
    }

    /// Partial-order tolerance lookup is scoped per `options_hash` — it does not cross
    /// into a different `options_hash` bucket.
    ///
    /// Scenario:
    /// - Insert at tol=0.01 under `options_hash_a`.
    /// - Lookup at tol=0.1 under `options_hash_a` → hits (tighter 0.01 satisfies looser 0.1).
    /// - Lookup at tol=0.1 under `options_hash_b` → misses (no entry under that hash).
    ///
    /// Guards against a bug where `lookup` ignores `options_hash` and falls through to
    /// a bucket belonging to a different options_hash value.
    #[test]
    fn partial_order_tolerance_lookup_works_within_fixed_options_hash() {
        let hash_a = ContentHash::of_str("opts_a");
        let hash_b = ContentHash::of_str("opts_b");

        let mut cache = RealizationCache::<u32>::new();
        cache.insert("E", ReprKind::BRep, 0.01, hash_a, 42);

        // Looser request within the same options_hash: tighter cached entry satisfies it.
        assert_eq!(
            cache.lookup("E", ReprKind::BRep, 0.1, hash_a),
            Some(&42),
            "looser request within same options_hash must hit tighter cached entry"
        );

        // Same (entity, repr_kind, tol) but different options_hash: must miss.
        assert_eq!(
            cache.lookup("E", ReprKind::BRep, 0.1, hash_b),
            None,
            "lookup under different options_hash must miss even when tol would satisfy"
        );
    }

    /// `ContentHash(0)` (the PRD §4 "no options" sentinel) behaves as a first-class
    /// partition key, not as magic that bypasses the dimension.
    ///
    /// Inserts at `ContentHash(0)` and `ContentHash::of_str("anything")` under the same
    /// `(entity, repr_kind, tol)` must both succeed and both be retrievable.
    #[test]
    fn content_hash_zero_sentinel_partitions_like_any_other() {
        let sentinel = ContentHash(0);
        let other = ContentHash::of_str("anything");

        let mut cache = RealizationCache::<u32>::new();
        let inserted_sentinel = cache.insert("B", ReprKind::BRep, 0.01, sentinel, 10);
        let inserted_other = cache.insert("B", ReprKind::BRep, 0.01, other, 20);
        assert!(
            inserted_sentinel,
            "insert under ContentHash(0) must succeed"
        );
        assert!(
            inserted_other,
            "insert under non-zero hash must succeed (distinct slot)"
        );

        assert_eq!(
            cache.lookup("B", ReprKind::BRep, 0.01, sentinel),
            Some(&10),
            "lookup under ContentHash(0) must return sentinel-slot value"
        );
        assert_eq!(
            cache.lookup("B", ReprKind::BRep, 0.01, other),
            Some(&20),
            "lookup under non-zero hash must return its own value"
        );
    }

    /// Structural regression pin: two distinct `options_hash` values must produce two distinct
    /// `ToleranceBucket` slots at SOFT_CAPACITY scale — neither set of inserts displaces or
    /// shadows the other.
    ///
    /// Inserts `SOFT_CAPACITY` entries under `options_hash_a`, then `SOFT_CAPACITY` more at
    /// the SAME `(entity, repr_kind, tol)` coordinates under `options_hash_b`.  Asserts:
    /// 1. Every B-insert returns `true` (no A-entry satisfies it — they're in distinct buckets).
    /// 2. `cache.len() == 2 * SOFT_CAPACITY` (B-inserts add a new bucket; do not displace A).
    /// 3. Round-trip lookups retrieve the correct value for each `(entity, tol, options_hash)`.
    /// 4. `bucket_len(entity, repr_kind, options_hash_a/b)` equals `SOFT_CAPACITY` each —
    ///    eviction is per-`options_hash`, not cross-`options_hash`.
    ///
    /// Fails if a future refactor collapses the `options_hash` dimension (e.g. by folding both
    /// hashes into the same `ToleranceBucket`): in that case B-inserts return `false`,
    /// `len() == SOFT_CAPACITY` (not `2 * SOFT_CAPACITY`), and lookups return wrong values.
    /// This matches the hex-wedge `force_tet` regression shape described in PRD §4 (M-024).
    #[test]
    fn options_hash_dimension_does_not_collapse_under_cardinality_check() {
        use crate::tolerance_bucket::SOFT_CAPACITY;

        let hash_a = ContentHash::of_str("force_tet=true");
        let hash_b = ContentHash::of_str("force_tet=false");

        // Build SOFT_CAPACITY strictly-descending tolerances (tighter → accepted, since
        // no prior entry satisfies each successive tighter request).
        // SOFT_CAPACITY is 5 → tols = [0.05, 0.04, 0.03, 0.02, 0.01]
        let tols: Vec<f64> = (0..SOFT_CAPACITY)
            .map(|i| 0.05 - (i as f64) * 0.01)
            .collect();

        let entity = "TargetEntity";
        let mut cache = RealizationCache::<u32>::new();

        // Insert SOFT_CAPACITY entries under hash_a.
        for (i, &t) in tols.iter().enumerate() {
            let ok = cache.insert(entity, ReprKind::BRep, t, hash_a, i as u32);
            assert!(ok, "hash_a insert at tol={t} must be accepted");
        }

        // Insert the same tols under hash_b — must all succeed (different bucket).
        for (i, &t) in tols.iter().enumerate() {
            let ok = cache.insert(entity, ReprKind::BRep, t, hash_b, (i + 100) as u32);
            assert!(
                ok,
                "hash_b insert at tol={t} must be accepted (independent bucket, not shadowed by hash_a)"
            );
        }

        // 1. Total entry count is 2 * SOFT_CAPACITY — no collapse.
        assert_eq!(
            cache.len(),
            2 * SOFT_CAPACITY,
            "len must be 2*SOFT_CAPACITY; a collapsed dimension would give SOFT_CAPACITY"
        );

        // 2. Each options_hash bucket has exactly SOFT_CAPACITY entries.
        assert_eq!(
            cache.bucket_len(entity, ReprKind::BRep, hash_a),
            SOFT_CAPACITY,
            "hash_a bucket must hold SOFT_CAPACITY entries"
        );
        assert_eq!(
            cache.bucket_len(entity, ReprKind::BRep, hash_b),
            SOFT_CAPACITY,
            "hash_b bucket must hold SOFT_CAPACITY entries"
        );

        // 3. Round-trip lookups retrieve values from the correct bucket.
        //    The tightest tol in our set is tols[SOFT_CAPACITY-1] = 0.01; looking up at
        //    that tolerance retrieves the entry from the matching options_hash bucket.
        let tightest = tols[SOFT_CAPACITY - 1];
        let val_a = cache.lookup(entity, ReprKind::BRep, tightest, hash_a);
        let val_b = cache.lookup(entity, ReprKind::BRep, tightest, hash_b);
        assert!(val_a.is_some(), "hash_a lookup at tightest tol must hit");
        assert!(val_b.is_some(), "hash_b lookup at tightest tol must hit");
        // Values must differ — each bucket stored different u32 values.
        assert_ne!(
            val_a, val_b,
            "hash_a and hash_b buckets must hold distinct values (not the same slot)"
        );
    }
}
