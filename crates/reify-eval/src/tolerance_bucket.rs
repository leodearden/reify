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
            crate::tolerance_gate::is_valid_tolerance_si(tol),
            "ToleranceBucket: tolerance must be finite and non-negative, got {tol}"
        );
        // Reject if any existing entry already satisfies this tolerance.
        if self
            .entries
            .iter()
            .any(|(cached_tol, _)| *cached_tol <= tol)
        {
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
            crate::tolerance_gate::is_valid_tolerance_si(requested_tol),
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

    /// Removes and returns the value stored at *exactly* `tol`, or `None` if no
    /// entry has that precise tolerance key.
    ///
    /// Unlike [`lookup`](Self::lookup) this is an **exact** match — NOT
    /// partial-order satisfaction — because the caller (intermediate-cache
    /// rollback, task 4050 step-14) must drop the specific entry it inserted at
    /// a known tolerance, not the loosest satisfying one. The match mirrors how
    /// [`insert`](Self::insert) positions by tolerance value: exact f64
    /// equality on the key the rollback log recorded, which is recomputed by the
    /// identical `per_stage_tolerance_for_plan` call (no derivation drift).
    ///
    /// Removing from a sorted-ascending `Vec` preserves the ordering invariant
    /// assumed by [`lookup`](Self::lookup), so no re-sort is needed.
    ///
    /// # Panics
    ///
    /// In debug builds, panics when `tol` is `NaN`, infinite, or negative —
    /// mirroring [`insert`](Self::insert) / [`lookup`](Self::lookup) so an
    /// invalid key never reaches the equality scan.
    pub fn remove(&mut self, tol: f64) -> Option<V> {
        debug_assert!(
            crate::tolerance_gate::is_valid_tolerance_si(tol),
            "ToleranceBucket: tolerance must be finite and non-negative, got {tol}"
        );
        // Exact f64-key match is intentional here (see the doc above): the
        // rollback removes the precise tolerance it inserted, recomputed
        // identically, so value equality — not an epsilon/partial-order check —
        // is correct. `clippy::float_cmp` would otherwise flag the `==`.
        #[allow(clippy::float_cmp)]
        let idx = self
            .entries
            .iter()
            .position(|(cached_tol, _)| *cached_tol == tol)?;
        Some(self.entries.remove(idx).1)
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
        assert!(
            !bucket.insert(0.01, 2u32),
            "equal-tolerance insert must be rejected"
        );
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
        assert_eq!(bucket.lookup(0.1), Some(&1u8)); // loosest satisfying 0.1 is still 0.1

        // 0.001 is tighter than both → succeeds; reachable at its exact tol.
        assert!(bucket.insert(0.001, 3u8));
        assert_eq!(bucket.lookup(0.001), Some(&3u8)); // tightest entry, exact tol
        assert_eq!(bucket.lookup(0.01), Some(&2u8)); // loosest satisfying 0.01 is 0.01
        assert_eq!(bucket.lookup(0.1), Some(&1u8)); // loosest satisfying 0.1 is 0.1
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

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn lookup_panics_on_negative_tolerance() {
        let mut bucket = ToleranceBucket::<u32>::new();
        bucket.insert(0.01, 42u32);
        bucket.lookup(-1.0e-3);
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
        assert_eq!(
            bucket.len(),
            SOFT_CAPACITY,
            "bucket len must equal SOFT_CAPACITY after overflow"
        );
        // The loosest entry "v1" (1.0) must have been evicted.
        // The next-loosest "v2" (0.5) is now the loosest in the bucket.
        // lookup(2.0): loosest satisfying should be 0.5 = "v2" (not "v1").
        assert_eq!(
            bucket.lookup(2.0),
            Some(&"v2"),
            "v1 (1.0) should be evicted; loosest is now v2 (0.5)"
        );
        assert_eq!(
            bucket.lookup(0.6),
            Some(&"v2"),
            "0.5 <= 0.6, so v2 is loosest satisfying"
        );
        assert_eq!(
            bucket.lookup(1.0),
            Some(&"v2"),
            "no entry at 1.0 after eviction; loosest <= 1.0 is 0.5 = v2"
        );
    }

    #[test]
    fn bucket_evicts_loosest_across_many_overflows() {
        // 10 successively-tightening tolerances (each exactly half the previous).
        // Every insert is strictly tighter than all prior entries, so all 10 succeed.
        // Starting from the 6th insert, each one pushes len past SOFT_CAPACITY and
        // evicts the loosest (largest) entry — testing pop() fires on every overflow,
        // not just the first.
        let tols = [
            1.0,
            0.5,
            0.25,
            0.125,
            0.0625,
            0.03125,
            0.015625,
            0.0078125,
            0.00390625,
            0.001953125,
        ];
        let mut bucket = ToleranceBucket::<u32>::new();
        for (i, &tol) in tols.iter().enumerate() {
            let result = bucket.insert(tol, i as u32);
            assert!(result, "insert #{i} must succeed for tol={tol}");
            // len must clamp at SOFT_CAPACITY — catches pop() becoming a no-op on
            // subsequent overflows and >= vs > drift in the cap comparison.
            assert_eq!(
                bucket.len(),
                (i + 1).min(SOFT_CAPACITY),
                "bucket len must equal (i+1).min(SOFT_CAPACITY) after insert #{i}",
            );
        }
        // After all 10 inserts the 5 tightest entries remain:
        // (0.001953125, 9), (0.00390625, 8), (0.0078125, 7), (0.015625, 6), (0.03125, 5).
        // Loosest in-bucket: 0.03125 = value 5.
        assert_eq!(
            bucket.lookup(2.0),
            Some(&5u32),
            "loosest in-bucket entry after 10 inserts should be 0.03125 = value 5",
        );
        // 0.0625 (value 4) was evicted on the 10th insert.
        // lookup(0.06) — largest cached_tol <= 0.06 is 0.03125 = value 5.
        assert_eq!(
            bucket.lookup(0.06),
            Some(&5u32),
            "0.0625 was evicted; loosest satisfying 0.06 is now 0.03125 = value 5",
        );
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
        assert_eq!(
            bucket.len(),
            1,
            "bucket must be unchanged after rejected insert"
        );
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

    // ---- task 4050: ToleranceBucket::remove (exact-tolerance removal) ----
    //
    // `remove` underpins atomic intermediate-cache rollback (step-14): a failed
    // realization must drop exactly the entries it inserted, keyed on the
    // precise f64 tolerance used at insert. Unlike `lookup`/`insert` it is an
    // EXACT match (no partial-order satisfaction), and the bucket's `entries`
    // field is private, so the cache layer needs the bucket to expose `remove`.

    #[test]
    fn remove_returns_value_and_empties_bucket() {
        let mut bucket = ToleranceBucket::<u32>::new();
        assert!(bucket.insert(0.01, 7u32));
        assert_eq!(bucket.len(), 1);

        assert_eq!(
            bucket.remove(0.01),
            Some(7u32),
            "remove must return the value stored at the exact tolerance"
        );
        assert_eq!(
            bucket.len(),
            0,
            "bucket must be empty after removing its only entry"
        );
        assert!(
            bucket.lookup(0.01).is_none(),
            "the removed entry must no longer be found"
        );
    }

    #[test]
    fn remove_of_absent_tolerance_returns_none() {
        // Removing a tolerance that was never inserted is a no-op returning
        // None — both on an empty bucket and on a populated one (whose entry is
        // left intact). This is the rollback-of-an-uninserted-key path.
        let mut empty = ToleranceBucket::<u32>::new();
        assert_eq!(
            empty.remove(0.5),
            None,
            "remove on an empty bucket returns None"
        );

        let mut bucket = ToleranceBucket::<u32>::new();
        assert!(bucket.insert(0.01, 7u32));
        assert_eq!(
            bucket.remove(0.5),
            None,
            "remove of a never-inserted tolerance returns None"
        );
        assert_eq!(
            bucket.len(),
            1,
            "a no-op remove must leave the bucket unchanged"
        );
        assert_eq!(
            bucket.lookup(0.01),
            Some(&7u32),
            "the existing entry must remain after a no-op remove"
        );
    }

    #[test]
    fn remove_middle_entry_keeps_others_and_preserves_sorted_order() {
        // Insert three successively-tightening tolerances (each strictly tighter
        // than the prior, so all three land): entries == [0.001, 0.01, 0.1].
        let mut bucket = ToleranceBucket::<&str>::new();
        assert!(bucket.insert(0.1, "loose"));
        assert!(bucket.insert(0.01, "mid"));
        assert!(bucket.insert(0.001, "tight"));
        assert_eq!(bucket.len(), 3);

        // Remove the middle entry by its exact tolerance.
        assert_eq!(bucket.remove(0.01), Some("mid"));
        assert_eq!(bucket.len(), 2, "exactly one entry must be removed");

        // The other two entries are intact and still reachable.
        assert_eq!(bucket.lookup(0.001), Some(&"tight"));
        assert_eq!(bucket.lookup(0.1), Some(&"loose"));
        // The middle tolerance is gone: a request at exactly 0.01 now falls
        // through to the tighter 0.001 entry (loosest-satisfying partial order).
        assert_eq!(bucket.lookup(0.01), Some(&"tight"));

        // The entries Vec remains sorted ascending. The child test module can
        // read the private `entries` field directly; this mirrors the
        // debug-invariant insert/lookup assert.
        assert!(
            bucket.entries.windows(2).all(|w| w[0].0 <= w[1].0),
            "entries must remain sorted ascending after remove"
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn remove_panics_on_nan_tolerance() {
        // remove mirrors insert/lookup's precondition guard so a NaN key never
        // reaches the f64 equality scan.
        let mut bucket = ToleranceBucket::<u32>::new();
        bucket.insert(0.01, 42u32);
        bucket.remove(f64::NAN);
    }
}
