// SPDX-License-Identifier: AGPL-3.0-or-later

//! FDM effective-property correlation library (task β) — the compute
//! source-of-truth for the v0.5 FDM-as-printed-FEA PRD §"Built-in property
//! correlations".
//!
//! Pure-`f64` correlations that turn a base filament material plus FDM process
//! parameters (infill density + pattern) into the foundation's
//! transverse-isotropic / orthotropic constitutive constants. The δ-task
//! (`as_printed_material` R-fast ComputeNode) calls these directly.
//!
//! The default numeric constants here mirror the stdlib
//! `FDMCorrelationDefaults` structure in
//! `crates/reify-compiler/stdlib/fdm_correlations.ri`. Both surfaces are
//! pinned by their own tests and MUST move together (see Plan
//! §"Design Decisions"): the stdlib surface is the human-facing
//! citation/override surface; this Rust surface is what δ computes against.

#[cfg(test)]
mod tests {
    use super::*;

    /// Tight tolerance — these correlations are a handful of f64 ops with no
    /// accumulation; `powf(2.0)` ULP error is far below 1e-12.
    const EPS: f64 = 1e-12;

    #[test]
    fn gibson_ashby_infill_factor_matches_c_rho_pow_n() {
        // C·ρ^n with C=1, n=2: 0.2^2 = 0.04.
        assert!(
            (gibson_ashby_infill_factor(0.2, 1.0, 2.0) - 0.04).abs() < EPS,
            "C·ρ^n at ρ=0.2, C=1, n=2 should be 0.04"
        );
        // Full density ⇒ no knockdown.
        assert!(
            (gibson_ashby_infill_factor(1.0, 1.0, 2.0) - 1.0).abs() < EPS,
            "full density should give factor 1.0"
        );
    }

    #[test]
    fn default_constants_match_prd_and_stdlib() {
        assert_eq!(BUILD_Z_MODULUS_RATIO, 0.67);
        assert_eq!(BUILD_Z_STRENGTH_RATIO, 0.52);
        assert_eq!(GIBSON_ASHBY_C, 1.0);
        assert_eq!(GIBSON_ASHBY_N, 2.0);
    }
}
