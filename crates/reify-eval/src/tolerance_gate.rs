//! Canonical Gate 4 tolerance validity predicate.
//!
//! Provides the single source-of-truth helper [`is_valid_tolerance_si`] that
//! every tolerance Gate 4 site in the eval crate routes through. This module
//! exists to eliminate inlined duplication of the predicate
//! `v.is_finite() && v >= 0.0` (or its De-Morgan negation
//! `!v.is_finite() || v < 0.0`) that previously lived at ten code sites
//! across eight sibling modules.
//!
//! # Why a dedicated module?
//!
//! The predicate is shared by `tolerance_promise`, `tolerance_combine`,
//! `tolerance_bucket`, `tolerance_budget`, `tolerance_format`,
//! `tolerance_scope`, `dispatcher`, and `field_import_provenance` — none of
//! which "own" the predicate semantically. A dedicated module avoids
//! introducing an arbitrary one-way dependency from each site into one of
//! the others, and lets the contract live alongside its own unit tests.
//!
//! # Cross-extractor symmetry
//!
//! The predicate's body is now structurally impossible to drift between
//! sites: a single site forgetting `>= 0.0` (writing `> 0.0`) or
//! `is_finite()` is no longer a possibility because there is exactly one
//! authoritative implementation.
//!
//! Note: three load-bearing safety-net debug_asserts in
//! `combine_demanded_tolerance` (tolerance_combine.rs) and
//! `is_promise_insufficient` (tolerance_promise.rs) intentionally retain
//! their inline predicate bodies so the contract text remains visible at
//! the points where it is the cross-extractor invariant.

/// Gate 4: tolerance validity predicate.
///
/// Returns `true` iff `v` is finite (not NaN, not ±∞) AND non-negative
/// (`>= 0.0`).
///
/// Note: `-0.0` is accepted because IEEE 754 has `-0.0 >= 0.0` evaluate to
/// `true`. This matches the existing behavior of every Gate 4 call site
/// before the helper was extracted.
///
/// This is the canonical predicate referenced by every tolerance Gate 4
/// site in the eval crate.
pub fn is_valid_tolerance_si(v: f64) -> bool {
    v.is_finite() && v >= 0.0
}

#[cfg(test)]
mod tests {
    use super::is_valid_tolerance_si;

    #[test]
    fn is_valid_tolerance_si_rejects_nan() {
        assert!(!is_valid_tolerance_si(f64::NAN));
    }

    #[test]
    fn is_valid_tolerance_si_rejects_positive_infinity() {
        assert!(!is_valid_tolerance_si(f64::INFINITY));
    }

    #[test]
    fn is_valid_tolerance_si_rejects_negative_infinity() {
        assert!(!is_valid_tolerance_si(f64::NEG_INFINITY));
    }

    #[test]
    fn is_valid_tolerance_si_rejects_negative_finite() {
        assert!(!is_valid_tolerance_si(-1e-3));
    }

    #[test]
    fn is_valid_tolerance_si_accepts_positive_zero() {
        assert!(is_valid_tolerance_si(0.0));
    }

    #[test]
    fn is_valid_tolerance_si_accepts_negative_zero() {
        // IEEE 754: `-0.0 >= 0.0` evaluates to true, so -0.0 is accepted.
        assert!(is_valid_tolerance_si(-0.0));
    }

    #[test]
    fn is_valid_tolerance_si_accepts_typical_positive_finite() {
        // 50 µm — a typical valid tolerance value in SI metres.
        assert!(is_valid_tolerance_si(50e-6));
    }
}
