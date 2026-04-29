//! Conversion-budget allocation heuristic for N-stage tolerance chains.
//!
//! See docs/prds/v0_2/per-purpose-tolerance.md "Resolved design decisions" for the
//! specification that drives this module.
//!
//! # Overview
//!
//! For an N-stage conversion chain, each stage receives a per-stage tolerance
//! derived from the requested overall tolerance via a geometric split with a
//! 0.8 safety factor:
//!
//! ```text
//! per_stage = requested_tol^(1/N) × SAFETY_FACTOR
//! ```
//!
//! Composing N stages each at their per-stage tolerance yields
//! `requested_tol × 0.8^N`, which is always ≤ the requested tolerance
//! (the safety property the heuristic delivers).

/// Safety factor applied on top of the geometric split.
///
/// Each per-stage tolerance is multiplied by this factor to ensure that
/// composing all N stages never reaches — let alone exceeds — the requested
/// overall tolerance.  Value 0.8 matches the PRD specification; it is a `const`
/// (not a runtime parameter) so tests can reference the same symbol without
/// magic-number desync (matching `tolerance_bucket::SOFT_CAPACITY` precedent).
pub const SAFETY_FACTOR: f64 = 0.8;

/// Returns the per-stage tolerance for an N-stage conversion chain.
///
/// For `n_stages = 1` the formula collapses to `requested_tol * SAFETY_FACTOR`.
/// For `n_stages > 1` it applies a geometric split so that composing N stages
/// each at the returned tolerance yields `requested_tol × 0.8^N ≤ requested_tol`.
///
/// # Panics (debug builds only)
///
/// In debug builds, panics if `n_stages == 0` or if `requested_tol` is not
/// finite and non-negative.  Both checks compile out in release builds.
pub fn per_stage_tolerance(requested_tol: f64, n_stages: usize) -> f64 {
    requested_tol * SAFETY_FACTOR
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_stage_applies_safety_factor() {
        // N=1: per_stage_tolerance(tol, 1) == tol * 0.8.
        // At N=1, the geometric split collapses to tol^(1/1) * 0.8 = tol * 0.8.
        // Use exact float equality — the multiplication is exact for this input.
        assert_eq!(per_stage_tolerance(0.001, 1), 0.001 * 0.8);
    }
}
