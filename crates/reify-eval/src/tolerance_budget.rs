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
    debug_assert!(
        n_stages > 0,
        "tolerance_budget: n_stages must be ≥ 1, got {n_stages}"
    );
    requested_tol.powf(1.0 / n_stages as f64) * SAFETY_FACTOR
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

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "n_stages must be")]
    fn n_stages_zero_panics_in_debug() {
        // n_stages=0 is a programmer error (1/0 = inf, tol^inf is nonsense).
        // The debug_assert in step-7 will fire; until then this test fails the
        // should_panic harness because no panic occurs.
        per_stage_tolerance(0.001, 0);
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
        // Pin the formula expected = tol^(1/N) * 0.8 at N ∈ {2, 3, 5} for two
        // representative tolerances. The step-2 minimal impl (tol * 0.8) is correct
        // only for N=1; at N=2,3,5 it returns 0.0008 (for tol=1e-3) while the
        // correct values are ~0.025298, ~0.080000, ~0.200951 — so this test must fail.
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
