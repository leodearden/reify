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

/// Citation + confidence record for a single correlation constant.
///
/// The Rust mirror of the stdlib `MaterialPropertyProvenance` slot plus the
/// parallel `..._low_confidence : Bool` flag, collapsed into one struct (δ
/// reads the stdlib surface for the user-facing citation; this carries the
/// same information for Rust consumers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorrelationProvenance {
    /// Short tag identifying the data origin.
    pub source: &'static str,
    /// The specific record / reference within `source`.
    pub reference: &'static str,
    /// Free-text caveats.
    pub notes: &'static str,
    /// `true` ⇒ the default is approximate; a coupon override is recommended
    /// where accuracy matters (PRD §"Built-in property correlations":
    /// FDM-specific Gibson-Ashby exponents; directional pattern factors).
    pub low_confidence: bool,
}

// ── Build-Z knockdown ratios ────────────────────────────────────────────────

/// E_z / E_xy: build-direction Young's-modulus knockdown (PLA-calibrated).
/// `< 1` guarantees the PRD C3/C4 invariant "build-Z is the weakest axis".
///
/// Mirrors stdlib `FDMCorrelationDefaults.build_z_modulus_ratio`.
pub const BUILD_Z_MODULUS_RATIO: f64 = 0.67;

/// Provenance for [`BUILD_Z_MODULUS_RATIO`].
pub const BUILD_Z_MODULUS_RATIO_PROVENANCE: CorrelationProvenance = CorrelationProvenance {
    source: "Reify FDM correlations v1",
    reference: "PMC9828590 — PLA-calibrated build-Z modulus ratio E_z/E_xy ≈ 0.67",
    notes: "Inter-layer bonds carry tensile load worse than continuous beads; 0.67 is the PLA-calibrated default.",
    low_confidence: false,
};

/// σ_z / σ_xy: build-direction strength knockdown (PLA-calibrated). Carried
/// for downstream limit-state checks; β's elastic assemblers use the modulus
/// ratio.
///
/// Mirrors stdlib `FDMCorrelationDefaults.build_z_strength_ratio`.
pub const BUILD_Z_STRENGTH_RATIO: f64 = 0.52;

/// Provenance for [`BUILD_Z_STRENGTH_RATIO`].
pub const BUILD_Z_STRENGTH_RATIO_PROVENANCE: CorrelationProvenance = CorrelationProvenance {
    source: "Reify FDM correlations v1",
    reference: "PMC9828590 — PLA-calibrated build-Z strength ratio σ_z/σ_xy ≈ 0.52",
    notes: "Build-direction strength knockdown for downstream limit-state / safety-factor checks.",
    low_confidence: false,
};

// ── Gibson-Ashby infill law (E_eff / E_solid = C · ρ^n) ──────────────────────

/// Coefficient C of the Gibson-Ashby open-cell foam law.
///
/// Mirrors stdlib `FDMCorrelationDefaults.gibson_ashby_c`.
pub const GIBSON_ASHBY_C: f64 = 1.0;

/// Provenance for [`GIBSON_ASHBY_C`].
pub const GIBSON_ASHBY_C_PROVENANCE: CorrelationProvenance = CorrelationProvenance {
    source: "Reify FDM correlations v1",
    reference: "Gibson & Ashby 1997, Cellular Solids — open-cell foam coefficient C=1",
    notes: "E_eff/E_solid = C·ρ^n; C=1 is the standard open-cell prefactor.",
    low_confidence: false,
};

/// Exponent n of the Gibson-Ashby open-cell foam law (bending-dominated).
/// The FDM-specific value is approximate and pattern-dependent — flagged
/// low-confidence.
///
/// Mirrors stdlib `FDMCorrelationDefaults.gibson_ashby_n`.
pub const GIBSON_ASHBY_N: f64 = 2.0;

/// Provenance for [`GIBSON_ASHBY_N`].
pub const GIBSON_ASHBY_N_PROVENANCE: CorrelationProvenance = CorrelationProvenance {
    source: "Reify FDM correlations v1",
    reference: "Gibson & Ashby 1997, Cellular Solids — bending-dominated exponent n=2",
    notes: "n=2 corresponds to bending-dominated open-cell deformation; the FDM-specific exponent is approximate.",
    low_confidence: true,
};

/// Gibson-Ashby open-cell foam knockdown: `E_eff / E_solid = C · ρ^n`.
///
/// `density` is the relative density (infill volume fraction) ρ ∈ (0, 1];
/// `c` and `n` are the law coefficients ([`GIBSON_ASHBY_C`] / [`GIBSON_ASHBY_N`]
/// by default, or coupon-override values). Full density (ρ = 1) yields a
/// factor of 1.0 (no knockdown).
pub fn gibson_ashby_infill_factor(density: f64, c: f64, n: f64) -> f64 {
    c * density.powf(n)
}

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

    #[test]
    fn pattern_factors_near_isotropic_have_equal_in_plane_factors() {
        // Gyroid/cubic ≈ in-plane isotropic: strong == weak == NEAR_ISOTROPIC_FACTOR.
        for p in [InfillPattern::Gyroid, InfillPattern::Cubic] {
            let f = pattern_factors(p);
            assert_eq!(
                f.in_plane_strong, f.in_plane_weak,
                "{:?} should be near-isotropic (strong == weak)",
                p
            );
            assert_eq!(f.in_plane_strong, NEAR_ISOTROPIC_FACTOR);
        }
    }

    #[test]
    fn pattern_factors_directional_have_strong_greater_than_weak() {
        // Grid/triangular/honeycomb are directional: strong > weak (drives the
        // orthotropic E1 > E2 split).
        for p in [
            InfillPattern::Grid,
            InfillPattern::Triangular,
            InfillPattern::Honeycomb,
        ] {
            let f = pattern_factors(p);
            assert!(
                f.in_plane_strong > f.in_plane_weak,
                "{:?} should be directional (strong > weak), got {:?}",
                p,
                f
            );
            assert!(f.in_plane_weak > 0.0, "{:?} weak factor must be positive", p);
        }
    }
}
