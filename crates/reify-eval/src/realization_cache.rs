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
//!
//! ## Families (γ #4730, PRD `docs/prds/v0_6/selective-realization-eviction.md` D4)
//!
//! An entity's *family* is everything a re-execution of that entity's realization
//! must not reuse: its terminal entry under every repr kind, options hash and
//! tolerance, plus every cross-kernel conversion intermediate it owns. Intermediates
//! are cached under a key derived from `(owner, ConversionSlot)` by
//! [`RealizationCache::intermediate_key`] — the single home of that grammar — and
//! inserted through [`RealizationCache::insert_intermediate`], which records the
//! owner in a structured index. [`RealizationCache::evict_family`] removes the whole
//! family through that index, never by parsing key strings.

use std::collections::{HashMap, HashSet};

use reify_core::ContentHash;
use reify_ir::{GeometryHandleId, ReprKind};

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

/// Which input of its owning realization a cross-kernel conversion intermediate
/// converts — the second half of an intermediate's cache identity (the first is
/// the owning realization's entity; see [`RealizationCache::intermediate_key`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConversionSlot {
    /// An input produced by the owning realization itself, identified by its
    /// local step index — stable across identical rebuilds.
    Step(usize),
    /// An input produced outside the owning realization (a cross-realization
    /// `Sub` operand), identified by its kernel handle id.
    External(GeometryHandleId),
}

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
    /// Monotonic lifetime count of NEW *terminal* realization entries.
    /// See [`RealizationCache::realization_entries`] for the full contract.
    terminal_entries: usize,
    /// Owner entity → the intermediate keys it inserted through
    /// [`RealizationCache::insert_intermediate`]. Family membership is read from
    /// here, so [`RealizationCache::evict_family`] never parses a key.
    intermediates_by_owner: HashMap<String, HashSet<String>>,
}

impl<V> RealizationCache<V> {
    /// Creates an empty `RealizationCache`.
    pub fn new() -> Self {
        Self {
            buckets: HashMap::new(),
            terminal_entries: 0,
            intermediates_by_owner: HashMap::new(),
        }
    }

    /// Number of NEW **terminal** realization entries created over this cache's
    /// lifetime — the signal behind [`crate::CacheStats::realization_entries`].
    ///
    /// # Contract
    ///
    /// - **Monotonic.** Incremented only by an *accepted* [`insert_terminal`](Self::insert_terminal)
    ///   (one that returns `true`, i.e. genuinely realized and cached new geometry).
    ///   Never decremented — not by [`remove`](Self::remove), not by
    ///   [`evict_family`](Self::evict_family), and not by the whole-cache
    ///   [`clear`](Self::clear). No method on this type can lower the count. See
    ///   [`clear`](Self::clear) for why that survives the whole-cache flush by
    ///   construction.
    /// - **Terminal only.** [`insert_intermediate`](Self::insert_intermediate) —
    ///   the cross-kernel conversion path — and plain [`insert`](Self::insert)
    ///   are deliberately NOT counted. Conversion
    ///   intermediates are steps *within* one realization, not realizations.
    /// - **NOT a live size.** This is deliberately not [`len`](Self::len): each
    ///   [`ToleranceBucket`] caps at `SOFT_CAPACITY` and evicts its loosest entry
    ///   inside the same `insert` call, so a genuinely-new insert can be net-zero
    ///   growth. `len()` would silently under-report realizations by the eviction
    ///   count; this counter answers "how many times did we actually realize and
    ///   cache new geometry", which is the re-mesh-avoidance question.
    pub fn realization_entries(&self) -> usize {
        self.terminal_entries
    }

    /// Drops every cached entry, leaving the cache empty.
    ///
    /// This is the whole-cache flush behind
    /// [`Engine::clear_realization_cache`](crate::Engine::clear_realization_cache),
    /// the public escape hatch and the GUI whole-file reload. Edits do not
    /// call it: `edit_param` / `edit_source` evict only stale families
    /// through [`evict_family`](Self::evict_family).
    ///
    /// **Monotonicity holds by construction.** The flush clears `buckets`
    /// in place and structurally cannot reach `terminal_entries`, so the
    /// lifetime counter documented by
    /// [`realization_entries`](Self::realization_entries) survives it without
    /// any save/restore dance at the call site. Reseating the whole struct to
    /// [`RealizationCache::new`] instead would zero the counter — that is
    /// exactly what this method exists to make impossible: a reset-on-flush
    /// counter would be zeroed by every reload and useless for cross-reload
    /// measurement.
    ///
    /// Pinned by `clear_empties_the_cache_but_preserves_realization_entries`
    /// below in this file and by
    /// `realization_entries_survives_clear_realization_cache` in
    /// `tests/harness_tolerance/tolerance_wiring_e2e.rs`.
    pub fn clear(&mut self) {
        self.buckets.clear();
        self.intermediates_by_owner.clear();
    }

    /// Inserts a **terminal realization** at `(entity, repr_kind, options_hash, tol)`,
    /// counting it in [`realization_entries`](Self::realization_entries).
    ///
    /// Identical to [`insert`](Self::insert) in every respect except that an
    /// accepted insert (returning `true`) also increments the monotonic lifetime
    /// counter. A rejected insert (`false` — an existing entry with a tighter or
    /// equal tolerance already dominates this one) is a cache *hit*: nothing was
    /// realized, so nothing is counted.
    ///
    /// This is the counted half of a deliberate split. The realization pipeline has
    /// exactly two cache-insert sites: the cross-kernel *conversion intermediate*
    /// site uses [`insert_intermediate`](Self::insert_intermediate) and is
    /// uncounted, while the terminal
    /// realization site (gated on `is_terminal_realization`, in `engine_build.rs`)
    /// uses this method. Keeping the split in two named methods — rather than a
    /// boolean parameter — makes it explicit at every call site which side it is on.
    ///
    /// # Panics
    ///
    /// Forwards [`insert`](Self::insert)'s debug-build precondition on `tol`.
    pub fn insert_terminal(
        &mut self,
        entity: &str,
        repr_kind: ReprKind,
        tol: f64,
        options_hash: ContentHash,
        val: V,
    ) -> bool {
        let inserted = self.insert(entity, repr_kind, tol, options_hash, val);
        if inserted {
            self.terminal_entries += 1;
        }
        inserted
    }

    /// Inserts `val` at `(entity, repr_kind, options_hash, tol)`.
    ///
    /// Returns `true` if the entry was inserted, or `false` if an existing entry with
    /// a tighter (or equal) tolerance already satisfies this tolerance within the same
    /// `options_hash` bucket — mirroring [`ToleranceBucket::insert`]'s semantics.
    ///
    /// `options_hash = ContentHash(0)` is the "no options" sentinel (PRD §4).
    ///
    /// **Uncounted path.** This does NOT move
    /// [`realization_entries`](Self::realization_entries). Use
    /// [`insert_terminal`](Self::insert_terminal) at the terminal-realization site
    /// so the realization is counted, and
    /// [`insert_intermediate`](Self::insert_intermediate) for a conversion
    /// intermediate so [`evict_family`](Self::evict_family) can reach it.
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

    /// Removes and returns the value cached at *exactly* `(entity, repr_kind,
    /// tol, options_hash)`, or `None` if no such entry exists.
    ///
    /// Mirrors [`lookup`](Self::lookup)'s key navigation but with `get_mut`, and
    /// delegates the bucket-local removal to [`ToleranceBucket::remove`] — an
    /// **exact** tolerance match, NOT partial-order satisfaction. Used by the
    /// intermediate-cache rollback (task 4050 step-14): a failed realization
    /// drops exactly the keys it inserted, leaving every sibling slot intact.
    ///
    /// Now-empty inner maps are left in place (cheap; `is_empty`/`len` already
    /// tolerate empty buckets), mirroring the no-prune policy elsewhere in the
    /// cache.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `tol` is NaN, infinite, or negative (forwards
    /// [`ToleranceBucket::remove`]'s precondition) — but only once an actual
    /// bucket is reached; an absent `(entity, repr_kind, options_hash)` returns
    /// `None` before any tolerance check.
    pub fn remove(
        &mut self,
        entity: &str,
        repr_kind: ReprKind,
        tol: f64,
        options_hash: ContentHash,
    ) -> Option<V> {
        self.buckets
            .get_mut(&repr_kind)
            .and_then(|inner| inner.get_mut(entity))
            .and_then(|by_options| by_options.get_mut(&options_hash))
            .and_then(|b| b.remove(tol))
    }

    /// The cache key of the conversion intermediate that `owner`'s realization
    /// produced for input `slot` — the single home of the intermediate key
    /// grammar.
    ///
    /// The key is stable across identical rebuilds of the owning realization (so
    /// a rebuild reuses the intermediate) and distinct per slot. It embeds a `#`,
    /// which cannot occur in a DSL entity identifier, so it never collides with a
    /// real entity's terminal key.
    pub fn intermediate_key(owner: &str, slot: ConversionSlot) -> String {
        match slot {
            ConversionSlot::Step(idx) => format!("{owner}#conv-step{idx}"),
            ConversionSlot::External(handle) => format!("{owner}#conv-ext{}", handle.0),
        }
    }

    /// Inserts the conversion intermediate `owner`'s realization produced for
    /// input `slot`, at `(intermediate_key(owner, slot), repr_kind, options_hash,
    /// tol)`, and records it as part of `owner`'s family.
    ///
    /// Uncounted: an intermediate is a step *within* one realization, so this
    /// never moves [`realization_entries`](Self::realization_entries). Returns
    /// [`insert`](Self::insert)'s accepted/dominated verdict.
    pub fn insert_intermediate(
        &mut self,
        owner: &str,
        slot: ConversionSlot,
        repr_kind: ReprKind,
        tol: f64,
        options_hash: ContentHash,
        val: V,
    ) -> bool {
        let key = Self::intermediate_key(owner, slot);
        let inserted = self.insert(&key, repr_kind, tol, options_hash, val);
        match self.intermediates_by_owner.get_mut(owner) {
            Some(keys) => {
                keys.insert(key);
            }
            None => {
                self.intermediates_by_owner
                    .insert(owner.to_owned(), HashSet::from([key]));
            }
        }
        inserted
    }

    /// Evicts `entity`'s whole family: its terminal entries under every
    /// `ReprKind`, options hash and tolerance, and every conversion intermediate
    /// it owns (see the module docs, "Families").
    ///
    /// Called by selective realization eviction at the `edit_param` /
    /// `edit_source` compare sites for every entity an edit made stale, and by
    /// the re-demand hash gate (task 4740) for a realization whose input-cone
    /// hash moved. A no-op for an absent entity. Never touches
    /// [`realization_entries`](Self::realization_entries).
    pub fn evict_family(&mut self, entity: &str) {
        let owned = self
            .intermediates_by_owner
            .remove(entity)
            .unwrap_or_default();
        for by_entity in self.buckets.values_mut() {
            by_entity.remove(entity);
            for key in &owned {
                by_entity.remove(key);
            }
        }
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

    /// Task 4050: `RealizationCache::remove` exact-key round-trip + wrong-key
    /// no-ops that leave every sibling slot intact.
    ///
    /// Underpins atomic intermediate-cache rollback (step-14): a failed
    /// realization must drop exactly the entries it inserted at their precise
    /// `(entity, repr_kind, tol, options_hash)` keys while leaving sibling slots
    /// (differing only in `options_hash` or `repr_kind`) untouched. Uses the
    /// real `KernelHandle` value type the production cache stores.
    #[test]
    fn remove_round_trip_and_wrong_key_no_ops_leave_siblings_intact() {
        use super::NO_OPTIONS;
        use reify_ir::{GeometryHandleId, KernelHandle, KernelId};

        let primary = KernelHandle {
            kernel: KernelId::Manifold,
            id: GeometryHandleId(5),
        };
        let sibling_opts = KernelHandle {
            kernel: KernelId::Manifold,
            id: GeometryHandleId(6),
        };
        let sibling_repr = KernelHandle {
            kernel: KernelId::Occt,
            id: GeometryHandleId(7),
        };

        let mut cache = RealizationCache::<KernelHandle>::new();
        // Primary slot + two siblings differing only in options_hash / repr_kind.
        cache.insert("Bracket", ReprKind::Mesh, 0.01, NO_OPTIONS, primary);
        cache.insert(
            "Bracket",
            ReprKind::Mesh,
            0.01,
            ContentHash(7),
            sibling_opts,
        );
        cache.insert("Bracket", ReprKind::BRep, 0.01, NO_OPTIONS, sibling_repr);

        // All three are independently retrievable up front.
        assert_eq!(
            cache.lookup("Bracket", ReprKind::Mesh, 0.01, NO_OPTIONS),
            Some(&primary)
        );
        assert_eq!(
            cache.lookup("Bracket", ReprKind::Mesh, 0.01, ContentHash(7)),
            Some(&sibling_opts)
        );
        assert_eq!(
            cache.lookup("Bracket", ReprKind::BRep, 0.01, NO_OPTIONS),
            Some(&sibling_repr)
        );

        // Wrong-key removes are None no-ops (no panic), one per key dimension.
        assert_eq!(
            cache.remove("Other", ReprKind::Mesh, 0.01, NO_OPTIONS),
            None,
            "wrong entity must miss"
        );
        assert_eq!(
            cache.remove("Bracket", ReprKind::Voxel, 0.01, NO_OPTIONS),
            None,
            "wrong repr_kind must miss"
        );
        assert_eq!(
            cache.remove("Bracket", ReprKind::Mesh, 0.005, NO_OPTIONS),
            None,
            "wrong (exact) tol must miss — remove is exact, not partial-order"
        );
        assert_eq!(
            cache.remove("Bracket", ReprKind::Mesh, 0.01, ContentHash(999)),
            None,
            "wrong options_hash must miss"
        );

        // None of the no-op removes disturbed any slot.
        assert_eq!(
            cache.lookup("Bracket", ReprKind::Mesh, 0.01, NO_OPTIONS),
            Some(&primary)
        );
        assert_eq!(
            cache.lookup("Bracket", ReprKind::Mesh, 0.01, ContentHash(7)),
            Some(&sibling_opts)
        );
        assert_eq!(
            cache.lookup("Bracket", ReprKind::BRep, 0.01, NO_OPTIONS),
            Some(&sibling_repr)
        );

        // Remove the primary at its exact key: returns the stored handle.
        assert_eq!(
            cache.remove("Bracket", ReprKind::Mesh, 0.01, NO_OPTIONS),
            Some(primary),
            "exact-key remove must return the stored handle"
        );
        // The primary slot now misses…
        assert_eq!(
            cache.lookup("Bracket", ReprKind::Mesh, 0.01, NO_OPTIONS),
            None,
            "removed slot must miss on subsequent lookup"
        );
        // …but the options_hash and repr_kind siblings are untouched.
        assert_eq!(
            cache.lookup("Bracket", ReprKind::Mesh, 0.01, ContentHash(7)),
            Some(&sibling_opts),
            "options_hash sibling must survive removal of the NO_OPTIONS slot"
        );
        assert_eq!(
            cache.lookup("Bracket", ReprKind::BRep, 0.01, NO_OPTIONS),
            Some(&sibling_repr),
            "repr_kind sibling must survive removal of the Mesh slot"
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

    /// step-07: a `KernelHandle` value round-trips through the value-agnostic
    /// `RealizationCache` unchanged — insert then lookup at the same
    /// `(entity, repr, tol, options)` key returns the same handle. Pins the
    /// PRD §8 signal that the cache carries the typed kernel-tagged handle
    /// cleanly (and that `reify_ir::{KernelId, KernelHandle}` resolve at the
    /// crate root).
    #[test]
    fn kernel_handle_round_trips_through_realization_cache() {
        use reify_ir::{GeometryHandleId, KernelHandle, KernelId};

        let mut cache = RealizationCache::<KernelHandle>::new();
        let handle = KernelHandle {
            kernel: KernelId::Manifold,
            id: GeometryHandleId(5),
        };
        cache.insert("Bracket", ReprKind::Mesh, 0.01, super::NO_OPTIONS, handle);

        assert_eq!(
            cache.lookup("Bracket", ReprKind::Mesh, 0.01, super::NO_OPTIONS),
            Some(&handle),
            "KernelHandle must round-trip through the value-agnostic cache"
        );
    }

    // ---------------------------------------------------------------------
    // task 4152 step-01: `realization_entries()` — the monotonic lifetime
    // count of NEW *terminal* realization cache entries, surfaced through
    // `CacheStats.realization_entries` (PRD `docs/prds/v0_4/fea-result-model.md`
    // B9: "the shared volume mesh is realized and cached exactly once across
    // both load cases").  These tests pin the counter contract at the cache
    // level, independently of the engine wiring.
    // ---------------------------------------------------------------------

    /// (a) A fresh cache — via either `new()` or `Default` — starts at zero.
    #[test]
    fn realization_entries_starts_at_zero() {
        let cache = RealizationCache::<u32>::new();
        assert_eq!(
            cache.realization_entries(),
            0,
            "a freshly constructed cache must have realized nothing yet"
        );

        let defaulted = RealizationCache::<u32>::default();
        assert_eq!(
            defaulted.realization_entries(),
            0,
            "`Default` must agree with `new()` on the initial counter value"
        );
    }

    /// (b) An accepted `insert_terminal` increments the counter by exactly 1.
    #[test]
    fn insert_terminal_increments_realization_entries_once_per_new_entry() {
        let mut cache = RealizationCache::<u32>::new();

        let accepted = cache.insert_terminal("Body", ReprKind::Mesh, 0.01, ContentHash(0), 1);
        assert!(accepted, "first terminal insert must be accepted");
        assert_eq!(
            cache.realization_entries(),
            1,
            "an accepted terminal insert must increment the counter by exactly 1"
        );

        // A distinct entity is a distinct realization → 2.
        assert!(cache.insert_terminal("Other", ReprKind::Mesh, 0.01, ContentHash(0), 2));
        assert_eq!(
            cache.realization_entries(),
            2,
            "a second, distinct terminal realization must increment again"
        );
    }

    /// (c) A dominated re-insert (equal or looser tolerance) is a cache HIT:
    /// `insert_terminal` returns `false` and the counter does not move.
    ///
    /// Mirrors `tolerance_bucket::tests::insert_rejects_equal_tolerance` — an
    /// equal-tolerance re-insert keeps the existing value and mutates nothing,
    /// so it did not "realize" anything new.
    #[test]
    fn dominated_insert_terminal_is_a_hit_and_does_not_increment() {
        let mut cache = RealizationCache::<u32>::new();
        assert!(cache.insert_terminal("Body", ReprKind::Mesh, 0.01, ContentHash(0), 1));
        assert_eq!(cache.realization_entries(), 1);

        // Equal tolerance: dominated by the existing entry.
        let equal = cache.insert_terminal("Body", ReprKind::Mesh, 0.01, ContentHash(0), 99);
        assert!(
            !equal,
            "equal-tolerance re-insert must be rejected as a hit"
        );
        assert_eq!(
            cache.realization_entries(),
            1,
            "a rejected (dominated) insert must NOT increment the counter"
        );

        // Looser tolerance: also dominated by the tighter cached entry.
        let looser = cache.insert_terminal("Body", ReprKind::Mesh, 0.1, ContentHash(0), 98);
        assert!(!looser, "looser re-insert must be rejected as a hit");
        assert_eq!(
            cache.realization_entries(),
            1,
            "a looser dominated insert must NOT increment the counter"
        );
    }

    /// (d) Plain `insert` is the INTERMEDIATE (uncounted) path — it never
    /// touches the counter, even when it genuinely inserts.
    ///
    /// This is the split that makes B9's "exactly 1" achievable: the
    /// OCCT→gmsh VolumeMesh route caches per-step conversion intermediates
    /// (`engine_build.rs`) through `insert_intermediate`, which delegates to
    /// `insert`, and those are steps *within* one realization, not
    /// realizations.
    #[test]
    fn plain_insert_never_increments_realization_entries() {
        let mut cache = RealizationCache::<u32>::new();

        let accepted = cache.insert("Body#conv-step0", ReprKind::BRep, 0.01, ContentHash(0), 1);
        assert!(accepted, "plain insert of a new entry must be accepted");
        assert_eq!(
            cache.realization_entries(),
            0,
            "the intermediate-conversion path must not be counted as a realization"
        );

        // Interleaving a terminal insert leaves the intermediate uncounted.
        assert!(cache.insert_terminal("Body", ReprKind::Mesh, 0.01, ContentHash(0), 2));
        assert!(cache.insert("Body#conv-step1", ReprKind::Mesh, 0.01, ContentHash(0), 3));
        assert_eq!(
            cache.realization_entries(),
            1,
            "only the terminal insert counts, regardless of interleaving"
        );
    }

    /// (e) The counter is a LIFETIME metric, not a live size: neither `remove`
    /// nor `evict_family` decrements it, and an owned conversion intermediate
    /// is never counted on the way in.
    #[test]
    fn remove_and_evict_family_do_not_decrement_realization_entries() {
        use super::ConversionSlot;

        let mut cache = RealizationCache::<u32>::new();
        assert!(cache.insert_terminal("Body", ReprKind::Mesh, 0.01, ContentHash(0), 1));
        assert!(cache.insert_terminal("Other", ReprKind::Mesh, 0.01, ContentHash(0), 2));
        assert!(cache.insert_intermediate(
            "Other",
            ConversionSlot::Step(0),
            ReprKind::Mesh,
            0.01,
            ContentHash(0),
            3,
        ));
        assert_eq!(
            cache.realization_entries(),
            2,
            "an owned conversion intermediate must not be counted as a realization"
        );

        assert_eq!(
            cache.remove("Body", ReprKind::Mesh, 0.01, ContentHash(0)),
            Some(1),
            "exact-key remove must return the cached value"
        );
        assert_eq!(
            cache.realization_entries(),
            2,
            "`remove` must NOT decrement the monotonic lifetime counter"
        );

        cache.evict_family("Other");
        assert_eq!(
            cache.realization_entries(),
            2,
            "`evict_family` must NOT decrement the monotonic lifetime counter"
        );
        assert!(cache.is_empty(), "every entry should now be gone");
    }

    // ---------------------------------------------------------------------
    // γ (#4730): `evict_family` — the keyed eviction primitive behind
    // selective realization eviction (PRD
    // `docs/prds/v0_6/selective-realization-eviction.md` D4). An entity's
    // FAMILY is its terminal key under every repr kind, options hash and
    // tolerance, PLUS every cross-kernel conversion intermediate it owns:
    // those intermediates are keyed stably across rebuilds, so leaving one
    // behind would hand a re-executed realization its OLD converted input.
    // ---------------------------------------------------------------------

    /// Every repr-kind, tolerance and options-hash slot of the evicted entity
    /// goes; a sibling entity is untouched.
    #[test]
    fn evict_family_removes_every_repr_tol_and_options_variant_of_the_entity() {
        use super::NO_OPTIONS;

        let mut cache = RealizationCache::<u32>::new();
        // Looser first: a looser insert behind a tighter one is a dominated hit.
        assert!(cache.insert_terminal("A", ReprKind::BRep, 1e-4, NO_OPTIONS, 1));
        assert!(cache.insert_terminal("A", ReprKind::BRep, 1e-6, NO_OPTIONS, 2));
        assert!(cache.insert_terminal("A", ReprKind::Mesh, 1e-6, NO_OPTIONS, 3));
        assert!(cache.insert_terminal("A", ReprKind::BRep, 1e-6, ContentHash(7), 4));
        assert!(cache.insert_terminal("B", ReprKind::BRep, 1e-6, NO_OPTIONS, 5));

        cache.evict_family("A");

        assert_eq!(cache.lookup("A", ReprKind::BRep, 1e-4, NO_OPTIONS), None);
        assert_eq!(cache.lookup("A", ReprKind::BRep, 1e-6, NO_OPTIONS), None);
        assert_eq!(cache.lookup("A", ReprKind::Mesh, 1e-6, NO_OPTIONS), None);
        assert_eq!(
            cache.lookup("A", ReprKind::BRep, 1e-6, ContentHash(7)),
            None
        );
        assert_eq!(
            cache.lookup("B", ReprKind::BRep, 1e-6, NO_OPTIONS),
            Some(&5),
            "a sibling entity's family must survive"
        );
        assert_eq!(cache.len(), 1, "only B's entry may remain");
    }

    /// The family includes the conversion intermediates the entity OWNS, and
    /// membership is by owner, never by name prefix: evicting "A" leaves "AB"
    /// whole.
    #[test]
    fn evict_family_removes_the_entitys_conversion_intermediates_but_not_a_name_prefix_sibling() {
        use super::{ConversionSlot, NO_OPTIONS};
        use reify_ir::GeometryHandleId;

        let tol = 1e-6;
        let step0 = ConversionSlot::Step(0);
        let ext42 = ConversionSlot::External(GeometryHandleId(42));
        let key = |owner: &str, slot| RealizationCache::<u32>::intermediate_key(owner, slot);

        let mut cache = RealizationCache::<u32>::new();
        assert!(cache.insert_intermediate("A", step0, ReprKind::Mesh, tol, NO_OPTIONS, 1));
        assert!(cache.insert_intermediate("A", ext42, ReprKind::Mesh, tol, NO_OPTIONS, 2));
        assert!(cache.insert_intermediate("AB", step0, ReprKind::Mesh, tol, NO_OPTIONS, 3));
        assert!(cache.insert_terminal("A", ReprKind::BRep, tol, NO_OPTIONS, 4));
        assert!(cache.insert_terminal("AB", ReprKind::BRep, tol, NO_OPTIONS, 5));
        assert_eq!(
            cache.lookup(&key("A", step0), ReprKind::Mesh, tol, NO_OPTIONS),
            Some(&1),
            "premise: an owned intermediate is servable through its key"
        );

        cache.evict_family("A");

        assert_eq!(
            cache.lookup(&key("A", step0), ReprKind::Mesh, tol, NO_OPTIONS),
            None
        );
        assert_eq!(
            cache.lookup(&key("A", ext42), ReprKind::Mesh, tol, NO_OPTIONS),
            None
        );
        assert_eq!(cache.lookup("A", ReprKind::BRep, tol, NO_OPTIONS), None);
        assert_eq!(
            cache.lookup(&key("AB", step0), ReprKind::Mesh, tol, NO_OPTIONS),
            Some(&3),
            "a name-prefix sibling's intermediate must survive"
        );
        assert_eq!(
            cache.lookup("AB", ReprKind::BRep, tol, NO_OPTIONS),
            Some(&5),
            "a name-prefix sibling's terminal must survive"
        );
    }

    /// PRD D4/§6 "tolerance interplay": evicting one family leaves a
    /// survivor's tolerance partial order exactly as it was.
    #[test]
    fn evict_family_composes_with_the_tolerance_partial_order_of_survivors() {
        use super::NO_OPTIONS;
        use crate::tolerance_bucket::SOFT_CAPACITY;

        let mut cache = RealizationCache::<u32>::new();
        assert!(cache.insert_terminal("B", ReprKind::BRep, 1e-4, NO_OPTIONS, 40));
        assert!(cache.insert_terminal("B", ReprKind::BRep, 1e-6, NO_OPTIONS, 60));
        assert!(cache.insert_terminal("A", ReprKind::BRep, 1e-6, NO_OPTIONS, 1));
        let survivor_len = cache.bucket_len("B", ReprKind::BRep, NO_OPTIONS);
        assert_eq!(survivor_len, 2);

        cache.evict_family("A");

        assert_eq!(
            cache.lookup("B", ReprKind::BRep, 1e-5, NO_OPTIONS),
            Some(&60),
            "a tighter survivor entry satisfies a looser request"
        );
        assert_eq!(
            cache.lookup("B", ReprKind::BRep, 1e-3, NO_OPTIONS),
            Some(&40),
            "the loosest satisfying survivor entry is served"
        );
        assert_eq!(
            cache.lookup("B", ReprKind::BRep, 1e-7, NO_OPTIONS),
            None,
            "no survivor entry is tight enough"
        );
        assert_eq!(
            cache.bucket_len("B", ReprKind::BRep, NO_OPTIONS),
            survivor_len
        );

        for (i, tol) in [1e-7, 1e-8, 1e-9, 1e-10].into_iter().enumerate() {
            assert!(cache.insert_terminal("B", ReprKind::BRep, tol, NO_OPTIONS, i as u32));
        }
        assert_eq!(
            cache.bucket_len("B", ReprKind::BRep, NO_OPTIONS),
            SOFT_CAPACITY,
            "the survivor bucket still caps at SOFT_CAPACITY"
        );
    }

    #[test]
    fn evict_family_on_an_absent_entity_is_a_no_op() {
        use super::NO_OPTIONS;

        let mut cache = RealizationCache::<u32>::new();
        assert!(cache.insert_terminal("B", ReprKind::BRep, 1e-6, NO_OPTIONS, 1));
        let before = cache.len();

        cache.evict_family("Absent");

        assert_eq!(cache.len(), before);
        assert_eq!(
            cache.lookup("B", ReprKind::BRep, 1e-6, NO_OPTIONS),
            Some(&1)
        );
    }

    /// After the whole-cache `clear`, intermediate ownership recorded before
    /// it must not linger: an intermediate inserted after the flush is still
    /// reached by `evict_family`.
    #[test]
    fn clear_also_forgets_intermediate_ownership() {
        use super::{ConversionSlot, NO_OPTIONS};

        let slot = ConversionSlot::Step(0);
        let key = RealizationCache::<u32>::intermediate_key("A", slot);

        let mut cache = RealizationCache::<u32>::new();
        assert!(cache.insert_intermediate("A", slot, ReprKind::Mesh, 1e-6, NO_OPTIONS, 1));
        cache.clear();
        assert!(cache.is_empty());

        assert!(cache.insert_intermediate("A", slot, ReprKind::Mesh, 1e-6, NO_OPTIONS, 2));
        cache.evict_family("A");

        assert_eq!(cache.lookup(&key, ReprKind::Mesh, 1e-6, NO_OPTIONS), None);
        assert!(cache.is_empty());
    }

    /// (e2) The whole-cache flush behind `Engine::clear_realization_cache`
    /// empties the cache without touching the lifetime counter.
    ///
    /// This is the structural half of the monotonicity invariant; see
    /// [`RealizationCache::clear`] for why it holds by construction.
    #[test]
    fn clear_empties_the_cache_but_preserves_realization_entries() {
        let mut cache = RealizationCache::<u32>::new();
        assert!(cache.insert_terminal("Body", ReprKind::Mesh, 0.01, ContentHash(0), 1));
        assert!(cache.insert_terminal("Other", ReprKind::BRep, 0.02, ContentHash(0), 2));
        assert_eq!(cache.realization_entries(), 2);
        assert_eq!(cache.len(), 2);

        cache.clear();

        assert!(cache.is_empty(), "`clear` must drop every cached entry");
        assert_eq!(cache.len(), 0, "`clear` must leave a zero live size");
        assert_eq!(
            cache.lookup("Body", ReprKind::Mesh, 0.01, ContentHash(0)),
            None,
            "a flushed entry must no longer be servable"
        );
        assert_eq!(
            cache.realization_entries(),
            2,
            "`clear` must NOT reset the monotonic lifetime counter — \
             edit_param/edit_source flush on every edit"
        );

        // A post-flush realization is genuinely new, so it still counts.
        assert!(cache.insert_terminal("Body", ReprKind::Mesh, 0.01, ContentHash(0), 3));
        assert_eq!(
            cache.realization_entries(),
            3,
            "re-realizing after a flush must increment, not restore"
        );
    }

    /// (f) Monotonicity survives `SOFT_CAPACITY` eviction — this is precisely
    /// why `len()` is NOT the signal.
    ///
    /// Seven strictly-tightening terminal inserts into ONE bucket are all
    /// accepted, so seven new entries were genuinely realized; but the bucket
    /// evicts its loosest entry on every insert past `SOFT_CAPACITY`, so
    /// `len()` reports 5.  A `len()`-based metric would silently under-report
    /// realizations by the eviction count.
    #[test]
    fn realization_entries_is_monotonic_across_soft_capacity_eviction() {
        use crate::tolerance_bucket::SOFT_CAPACITY;

        let mut cache = RealizationCache::<u32>::new();

        // Strictly descending tolerances: each new entry is tighter than every
        // existing one, so no insert is ever dominated (cf.
        // `cache_len_caps_at_soft_capacity_per_bucket`).
        let tols = [0.1_f64, 0.05, 0.04, 0.03, 0.02, 0.01, 0.005];
        assert_eq!(tols.len(), SOFT_CAPACITY + 2);

        for (i, &t) in tols.iter().enumerate() {
            let accepted = cache.insert_terminal("E", ReprKind::BRep, t, ContentHash(0), i as u32);
            assert!(accepted, "terminal insert at tol={t} must be accepted");
        }

        assert_eq!(
            cache.realization_entries(),
            tols.len(),
            "every accepted terminal insert must be counted, eviction notwithstanding"
        );
        assert_eq!(
            cache.len(),
            SOFT_CAPACITY,
            "live `len()` caps at SOFT_CAPACITY — this is why it is NOT the signal"
        );
        assert!(
            cache.realization_entries() > cache.len(),
            "the lifetime counter must be able to exceed the live cache size"
        );
    }
}
