//! Partial-order cache lookup and bounded-cardinality eviction for tolerance buckets.
//!
//! See docs/prds/v0_2/per-purpose-tolerance.md "Resolved design decisions" for the
//! specification that drives this module.

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
}
