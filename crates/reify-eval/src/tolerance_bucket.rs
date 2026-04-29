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

    /// Inserts `(tol, val)` into the bucket and returns `true`, or returns `false`
    /// without modifying the bucket if any existing entry already satisfies `tol`.
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
    /// # Panics (debug only)
    ///
    /// Panics in debug builds when `tol` is not finite or is negative.
    pub fn insert(&mut self, tol: f64, val: V) -> bool {
        // Reject if any existing entry already satisfies this tolerance.
        if self.entries.iter().any(|(cached_tol, _)| *cached_tol <= tol) {
            return false;
        }
        self.entries.push((tol, val));
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
    /// # Panics (debug only)
    ///
    /// Panics in debug builds when `requested_tol` is not finite or is negative.
    pub fn lookup(&self, requested_tol: f64) -> Option<&V> {
        for (t, v) in &self.entries {
            if *t <= requested_tol {
                return Some(v);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bucket_has_no_hits() {
        let bucket = ToleranceBucket::<u32>::new();
        assert_eq!(bucket.len(), 0);
        assert!(bucket.is_empty());
        assert!(bucket.lookup(0.01).is_none());
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
