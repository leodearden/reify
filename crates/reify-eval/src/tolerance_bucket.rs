//! Partial-order cache lookup and bounded-cardinality eviction for tolerance buckets.
//!
//! See docs/prds/v0_2/per-purpose-tolerance.md "Resolved design decisions" for the
//! specification that drives this module.
//!
//! # Overview
//!
//! [`ToleranceBucket<V>`] is a small, Vec-backed ordered container that implements the
//! "tighter satisfies looser" partial-order cache lookup described in the PRD.  Each
//! bucket is keyed externally by `(entity_id, repr_kind)` (task 2640's responsibility).
//! Within a bucket, entries are sorted **ascending** by tolerance value so that lookup
//! can find the *loosest satisfying* entry (largest `cached_tol ≤ requested_tol`) with
//! a simple reverse scan.
//!
//! Bucket cardinality is bounded by [`SOFT_CAPACITY`].  After every successful insert
//! that pushes length beyond the cap, the loosest (largest) entries are dropped —
//! they are redundant whenever a tighter satisfying entry remains in the bucket.

/// Maximum number of entries kept in a [`ToleranceBucket`] after eviction.
///
/// Matches the PRD's "~5 typical population 1-3" guidance.  A constant (not a
/// constructor argument) keeps the API surface minimal and lets tests reference the
/// same symbol without magic-number desync.
pub const SOFT_CAPACITY: usize = 5;

/// A small, sorted-ascending cache bucket keyed by tolerance value.
///
/// Each entry is a `(cached_tol: f64, value: V)` pair.  Entries are stored in
/// ascending order of `cached_tol` so that lookup can efficiently find the loosest
/// (largest) satisfying entry via a reverse scan.
///
/// See module-level documentation for the partial-order semantics and eviction policy.
#[derive(Debug, Default)]
pub struct ToleranceBucket<V> {
    /// Entries sorted ascending by `cached_tol`.
    entries: Vec<(f64, V)>,
}

impl<V> ToleranceBucket<V> {
    /// Creates an empty `ToleranceBucket`.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Returns the number of entries currently in the bucket.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the bucket contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Inserts `(tol, val)` into the bucket in sorted-ascending order and returns
    /// `true`, or returns `false` without modifying the bucket if any existing entry
    /// already satisfies `tol`.
    ///
    /// # "Tighter satisfies looser" rule (PRD: per-purpose-tolerance.md)
    ///
    /// An existing entry with `cached_tol <= tol` already satisfies any request at
    /// tolerance `tol`.  Inserting a new, looser entry would be redundant — any
    /// downstream consumer that could use the new entry can also use the existing
    /// tighter one.  This function is therefore *idempotent under partial-order
    /// satisfaction*: inserting a tolerance that is already dominated by an existing
    /// entry is a no-op.
    ///
    /// After a successful insert the `entries` slice remains sorted ascending by
    /// `cached_tol`, which is the invariant assumed by [`lookup`](Self::lookup).
    ///
    /// # Panics
    ///
    /// In debug builds, panics when `tol` is `NaN`, infinite, or negative.
    /// The `debug_assert!` keeps NaN out of all sort/compare operations inside
    /// the bucket, which would otherwise produce `None` from `partial_cmp` and
    /// violate the sorted-ascending invariant.
    pub fn insert(&mut self, tol: f64, val: V) -> bool {
        debug_assert!(
            tol.is_finite() && tol >= 0.0,
            "ToleranceBucket: tolerance must be finite and non-negative, got {tol}"
        );
        // Reject if any existing entry already satisfies this tolerance.
        if self.entries.iter().any(|(cached_tol, _)| *cached_tol <= tol) {
            return false;
        }
        // The rejection rule above guarantees every remaining entry has `cached_tol > tol`,
        // so the new entry is strictly tighter than all existing ones and belongs at
        // index 0 — the ascending-sorted front of the Vec.  No `partition_point` needed.
        self.entries.insert(0, (tol, val));
        debug_assert!(
            self.entries.windows(2).all(|w| w[0].0 <= w[1].0),
            "ToleranceBucket: sorted-ascending invariant violated after insert"
        );
        // Evict the loosest (largest cached_tol) entry when cardinality exceeds the cap.
        //
        // Because entries are sorted ascending, the loosest entry is always at the end.
        // Each successful insert adds exactly one entry, so at most one entry needs to be
        // evicted per call — `pop` reads directly as "drop the loosest if over cap".
        //
        // Eviction never invalidates future cache hits: any request the evicted entry
        // would have satisfied is also satisfied by every tighter entry that remains
        // (by the partial-order rule `cached_tol <= requested_tol`).  Evicted entries
        // are therefore always redundant at the moment of eviction.
        if self.entries.len() > SOFT_CAPACITY {
            self.entries.pop();
        }
        true
    }

    /// Returns a reference to the value of the *loosest satisfying* entry for
    /// `requested_tol`, or `None` if no cached entry satisfies the request.
    ///
    /// An entry satisfies the request when `cached_tol <= requested_tol` ("tighter
    /// satisfies looser").  Among all satisfying entries the one with the *largest*
    /// `cached_tol` (best-fit / loosest satisfying) is returned; this minimises
    /// downstream consumer cost while keeping each bucket entry actively useful.
    ///
    /// Because entries are sorted ascending, iterating in reverse (largest first)
    /// yields the loosest satisfying entry on the first match — O(n) worst-case
    /// but typically O(1) at n ≤ [`SOFT_CAPACITY`].
    ///
    /// # Panics
    ///
    /// In debug builds, panics when `requested_tol` is `NaN`, infinite, or negative.
    pub fn lookup(&self, requested_tol: f64) -> Option<&V> {
        debug_assert!(
            requested_tol.is_finite() && requested_tol >= 0.0,
            "ToleranceBucket: tolerance must be finite and non-negative, got {requested_tol}"
        );
        debug_assert!(
            self.entries.windows(2).all(|w| w[0].0 <= w[1].0),
            "ToleranceBucket: entries must be sorted ascending by tolerance"
        );
        // Reverse scan: largest cached_tol first — return the first (loosest) that satisfies.
        self.entries
            .iter()
            .rev()
            .find_map(|(t, v)| if *t <= requested_tol { Some(v) } else { None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_rejects_equal_tolerance() {
        // Boundary of the `<=` rule: an existing entry at exactly `tol` already
        // satisfies any request at that tolerance, so a second insert at the same
        // value must be rejected and leave the bucket unchanged.
        let mut bucket = ToleranceBucket::<u32>::new();
        assert!(bucket.insert(0.01, 1u32));
        assert_eq!(bucket.len(), 1);
        assert!(!bucket.insert(0.01, 2u32), "equal-tolerance insert must be rejected");
        assert_eq!(bucket.len(), 1, "bucket must be unchanged after rejection");
        assert_eq!(bucket.lookup(0.01), Some(&1u32));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn insert_panics_on_infinite_tolerance() {
        let mut bucket = ToleranceBucket::<u32>::new();
        bucket.insert(f64::INFINITY, 0u32);
    }

    #[test]
    fn successful_insert_always_lands_at_front() {
        // The rejection rule guarantees every successful insert is strictly tighter
        // than all existing entries, so it always lands at index 0 (the tightest slot).
        // Verify through exact-tolerance lookups: the most recently inserted entry must
        // be reachable at its precise tolerance, and loosest-satisfying semantics must
        // still return the correct (loosest) entry for wider requests.
        let mut bucket = ToleranceBucket::<u8>::new();
        assert!(bucket.insert(0.1, 1u8));
        assert_eq!(bucket.lookup(0.1), Some(&1u8));

        // 0.01 is tighter than 0.1 → succeeds; reachable at its exact tol.
        assert!(bucket.insert(0.01, 2u8));
        assert_eq!(bucket.lookup(0.01), Some(&2u8));
        assert_eq!(bucket.lookup(0.1), Some(&1u8));   // loosest satisfying 0.1 is still 0.1

        // 0.001 is tighter than both → succeeds; reachable at its exact tol.
        assert!(bucket.insert(0.001, 3u8));
        assert_eq!(bucket.lookup(0.001), Some(&3u8)); // tightest entry, exact tol
        assert_eq!(bucket.lookup(0.01), Some(&2u8));  // loosest satisfying 0.01 is 0.01
        assert_eq!(bucket.lookup(0.1), Some(&1u8));   // loosest satisfying 0.1 is 0.1
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn insert_panics_on_nan_tolerance() {
        let mut bucket = ToleranceBucket::<u32>::new();
        bucket.insert(f64::NAN, 0u32);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn insert_panics_on_negative_tolerance() {
        let mut bucket = ToleranceBucket::<u32>::new();
        bucket.insert(-1.0e-3, 0u32);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn lookup_panics_on_nan_tolerance() {
        let mut bucket = ToleranceBucket::<u32>::new();
        bucket.insert(0.01, 42u32);
        bucket.lookup(f64::NAN);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn lookup_panics_on_infinite_tolerance() {
        let mut bucket = ToleranceBucket::<u32>::new();
        bucket.insert(0.01, 42u32);
        bucket.lookup(f64::INFINITY);
    }

    #[test]
    fn bucket_evicts_loosest_when_capacity_exceeded() {
        let mut bucket = ToleranceBucket::<&str>::new();
        // Insert 6 successively-tightening tolerances. Each is strictly tighter than
        // the previous, so all 6 inserts succeed (no existing entry satisfies the new one).
        let tols = [1.0, 0.5, 0.25, 0.125, 0.0625, 0.03125];
        let tags = ["v1", "v2", "v3", "v4", "v5", "v6"];
        for (i, (&tol, &tag)) in tols.iter().zip(tags.iter()).enumerate() {
            let result = bucket.insert(tol, tag);
            assert!(result, "insert #{i} must succeed for tol={tol}");
        }
        // After 6 inserts, eviction must have kept len at SOFT_CAPACITY (5).
        assert_eq!(bucket.len(), SOFT_CAPACITY, "bucket len must equal SOFT_CAPACITY after overflow");
        // The loosest entry "v1" (1.0) must have been evicted.
        // The next-loosest "v2" (0.5) is now the loosest in the bucket.
        // lookup(2.0): loosest satisfying should be 0.5 = "v2" (not "v1").
        assert_eq!(bucket.lookup(2.0), Some(&"v2"), "v1 (1.0) should be evicted; loosest is now v2 (0.5)");
        assert_eq!(bucket.lookup(0.6), Some(&"v2"), "0.5 <= 0.6, so v2 is loosest satisfying");
        assert_eq!(bucket.lookup(1.0), Some(&"v2"), "no entry at 1.0 after eviction; loosest <= 1.0 is 0.5 = v2");
    }

    #[test]
    fn empty_bucket_has_no_hits() {
        let bucket = ToleranceBucket::<u32>::new();
        assert_eq!(bucket.len(), 0);
        assert!(bucket.is_empty());
        assert!(bucket.lookup(0.01).is_none());
    }

    #[test]
    fn lookup_returns_loosest_satisfying_entry_among_many() {
        let mut bucket = ToleranceBucket::<&str>::new();
        // Insert 0.001 first — succeeds (no existing entry satisfies 0.001).
        assert!(bucket.insert(0.001, "A"));
        // Insert 0.0001 — succeeds because 0.001 does NOT satisfy 0.0001 (0.001 > 0.0001).
        assert!(bucket.insert(0.0001, "B"));
        assert_eq!(bucket.len(), 2);

        // lookup(0.001): largest cached_tol <= 0.001 is 0.001 = "A".
        assert_eq!(bucket.lookup(0.001), Some(&"A"));
        // lookup(0.01): both satisfy; loosest = 0.001 = "A".
        assert_eq!(bucket.lookup(0.01), Some(&"A"));
        // lookup(0.0005): only 0.0001 satisfies (0.001 > 0.0005); loosest = 0.0001 = "B".
        assert_eq!(bucket.lookup(0.0005), Some(&"B"));
        // lookup(0.0001): largest cached_tol <= 0.0001 is 0.0001 = "B".
        assert_eq!(bucket.lookup(0.0001), Some(&"B"));
        // lookup(0.00001): nothing satisfies.
        assert!(bucket.lookup(0.00001).is_none());
    }

    #[test]
    fn insert_idempotent_when_existing_entry_satisfies() {
        let mut bucket = ToleranceBucket::<&str>::new();
        // First insert: no existing entry satisfies 0.001, so it succeeds.
        assert!(bucket.insert(0.001, "tight"));
        assert_eq!(bucket.len(), 1);
        // Second insert: the existing 0.001 entry satisfies 0.01 (0.001 <= 0.01),
        // so the insert must be rejected (per PRD "Insert when no entry satisfies").
        assert!(!bucket.insert(0.01, "loose"));
        assert_eq!(bucket.len(), 1, "bucket must be unchanged after rejected insert");
        assert_eq!(bucket.lookup(0.01), Some(&"tight"));
        assert_eq!(bucket.lookup(0.001), Some(&"tight"));
    }

    #[test]
    fn single_insert_partial_order_satisfies_at_or_above_cached_tol() {
        let mut bucket = ToleranceBucket::<&str>::new();
        bucket.insert(0.01, "A");
        // equal cached_tol satisfies (rule is <=)
        assert_eq!(bucket.lookup(0.01), Some(&"A"));
        // looser request hits tighter cached entry
        assert_eq!(bucket.lookup(0.1), Some(&"A"));
        // tighter request than cached entry — no satisfaction
        assert!(bucket.lookup(0.001).is_none());
        assert_eq!(bucket.len(), 1);
    }
}
