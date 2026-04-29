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
//!
//! **Composition model:** this heuristic assumes stages compose
//! *multiplicatively* (each stage's output quality-factor multiplies): the
//! composed tolerance is the product of per-stage tolerances, not their sum.
//! Per-stage values may therefore exceed `requested_tol` when
//! `requested_tol < 1` (e.g. tol=1e-3, N=2 → per_stage ≈ 0.0253) — that
//! is by design.

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
    debug_assert!(
        n_stages > 0,
        "tolerance_budget: n_stages must be ≥ 1, got {n_stages}"
    );
    debug_assert!(
        requested_tol.is_finite() && requested_tol >= 0.0,
        "tolerance_budget: requested_tol must be finite and non-negative, got {requested_tol}"
    );
    if n_stages == 1 {
        return requested_tol * SAFETY_FACTOR;
    }
    requested_tol.powf(1.0 / n_stages as f64) * SAFETY_FACTOR
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_stage_applies_safety_factor() {
        // N=1: per_stage = tol * SAFETY_FACTOR exactly.
        // The n_stages==1 path is short-circuited (no powf call), so the result
        // is bit-exact `requested_tol * SAFETY_FACTOR` on all libm implementations.
        let observed = per_stage_tolerance(0.001, 1);
        let expected = 0.001 * SAFETY_FACTOR;
        assert_eq!(
            observed, expected,
            "per_stage_tolerance(0.001, 1): expected {expected}, got {observed}"
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "requested_tol must be finite and non-negative")]
    fn tol_nan_panics_in_debug() {
        // NaN is not finite — debug_assert in step-9 will catch it.
        per_stage_tolerance(f64::NAN, 2);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "requested_tol must be finite and non-negative")]
    fn tol_infinite_panics_in_debug() {
        // +inf is not finite — debug_assert in step-9 will catch it.
        per_stage_tolerance(f64::INFINITY, 2);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "requested_tol must be finite and non-negative")]
    fn tol_negative_panics_in_debug() {
        // Negative tolerance is not physically meaningful.
        per_stage_tolerance(-1e-3, 2);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "n_stages must be")]
    fn n_stages_zero_panics_in_debug() {
        // Precondition: n_stages > 0. Zero is a programmer error
        // (1/0 = inf, tol^inf is nonsense). Enforced by a debug_assert.
        per_stage_tolerance(0.001, 0);
    }

    #[test]
    fn zero_tolerance_returns_zero() {
        // Zero is a valid finite, non-negative tolerance ("perfect" representation).
        // 0.0_f64.powf(x) == 0.0 for any x > 0 per IEEE 754, so per_stage(0.0, N)
        // returns 0.0 for all valid N. Pins the edge-case against future asserts
        // that might accidentally tighten the precondition to > 0.0.
        for &n in &[1_usize, 2, 3, 5] {
            assert_eq!(
                per_stage_tolerance(0.0, n),
                0.0,
                "per_stage_tolerance(0.0, {n}) must return 0.0"
            );
        }
    }

    #[test]
    fn composition_within_budget() {
        // Safety invariant: composing N stages each at per_stage(tol, N) yields
        // tol * 0.8^N, which is always ≤ tol (because 0.8 < 1).
        //
        // Closed-form check: per_stage(tol, N)^N ≈ tol * 0.8^N within float-eps,
        // and that composed value is strictly < tol for tol > 0 and N ≥ 1.
        let float_eps = 1e-10;
        for &tol in &[1e-3_f64, 1e-2_f64] {
            for &n in &[1_usize, 2, 3, 5] {
                let per_stage = per_stage_tolerance(tol, n);
                let composed = per_stage.powi(n as i32);
                let expected_composed = tol * 0.8_f64.powi(n as i32);
                assert!(
                    (composed - expected_composed).abs() < float_eps,
                    "per_stage({tol},{n})^{n} = {composed} should ≈ tol*0.8^N = {expected_composed}"
                );
                assert!(
                    composed < tol,
                    "safety violation: per_stage({tol},{n})^{n} = {composed} ≥ tol = {tol}"
                );
            }
        }
    }

    #[test]
    fn geometric_split_multi_stages() {
        // Pin per_stage = tol^(1/N) * SAFETY_FACTOR for representative N and tol.
        let eps = 1e-12;
        for &tol in &[1e-3_f64, 1e-4_f64] {
            for &n in &[2_usize, 3, 5] {
                let expected = tol.powf(1.0 / n as f64) * 0.8;
                let observed = per_stage_tolerance(tol, n);
                assert!(
                    (observed - expected).abs() < eps,
                    "per_stage_tolerance({tol}, {n}): expected {expected}, got {observed}"
                );
            }
        }
    }
}
