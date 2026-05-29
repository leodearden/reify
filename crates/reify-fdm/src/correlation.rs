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
//!
//! Divergence is caught *indirectly*: each surface is independently pinned to
//! the PRD-specified literals by its own test — this crate's
//! `default_constants_match_prd` and the compiler's
//! `fdm_correlations_stdlib_compile.rs`. There is deliberately no single test
//! that loads both surfaces and compares them directly, because `reify-fdm` is
//! a zero-dependency crate (Plan §"Design Decisions") and cannot load the
//! compiler stdlib without a cross-crate dependency. The residual risk: a
//! value changed on one side stays caught only if that side's pinning test is
//! also updated to the new PRD literal — so keep both literal lists in
//! lock-step with the PRD.

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

// ── Infill-pattern factors ──────────────────────────────────────────────────

/// Rust mirror of the stdlib `InfillPattern` enum
/// (`crates/reify-compiler/stdlib/fdm.ri`). Variants are in the canonical order
/// pinned by α's
/// `fdm_stdlib_compile.rs::infill_pattern_enum_has_five_variants_in_canonical_order`
/// (near-isotropic first, then directional); any future addition must be
/// appended, never inserted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InfillPattern {
    /// Near-isotropic.
    Gyroid,
    /// Near-isotropic.
    Cubic,
    /// Directional.
    Grid,
    /// Directional.
    Triangular,
    /// Directional.
    Honeycomb,
}

/// In-plane directional knockdown factors for an infill pattern: a `strong`
/// (along-raster) and `weak` (transverse) multiplier on the
/// infill-density-derived in-plane modulus. Near-isotropic patterns have
/// `in_plane_strong == in_plane_weak`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PatternFactors {
    /// Strong (along-raster) in-plane factor.
    pub in_plane_strong: f64,
    /// Weak (transverse) in-plane factor.
    pub in_plane_weak: f64,
}

/// Near-isotropic (gyroid/cubic) in-plane factor. Mirrors stdlib
/// `FDMCorrelationDefaults.pattern_near_isotropic_factor`.
pub const NEAR_ISOTROPIC_FACTOR: f64 = 1.0;

/// Directional strong (along-raster) in-plane factor. Mirrors stdlib
/// `FDMCorrelationDefaults.pattern_directional_strong_factor`.
pub const DIRECTIONAL_STRONG_FACTOR: f64 = 1.0;

/// Directional weak (transverse) in-plane factor. Mirrors stdlib
/// `FDMCorrelationDefaults.pattern_directional_weak_factor`.
pub const DIRECTIONAL_WEAK_FACTOR: f64 = 0.6;

/// Provenance for the directional pattern factors — approximate, no
/// PRD-pinned calibration, flagged low-confidence.
pub const DIRECTIONAL_FACTOR_PROVENANCE: CorrelationProvenance = CorrelationProvenance {
    source: "Reify FDM correlations v1",
    reference: "PRD §Built-in property correlations — grid/triangular/honeycomb directional factors",
    notes: "strong > weak yields the orthotropic E1 > E2 in-plane split; the magnitudes are approximate.",
    low_confidence: true,
};

/// In-plane [`PatternFactors`] for an infill pattern. Near-isotropic patterns
/// (gyroid/cubic) return equal factors (both = [`NEAR_ISOTROPIC_FACTOR`));
/// directional patterns (grid/triangular/honeycomb) return
/// [`DIRECTIONAL_STRONG_FACTOR`] > [`DIRECTIONAL_WEAK_FACTOR`].
pub fn pattern_factors(p: InfillPattern) -> PatternFactors {
    match p {
        InfillPattern::Gyroid | InfillPattern::Cubic => PatternFactors {
            in_plane_strong: NEAR_ISOTROPIC_FACTOR,
            in_plane_weak: NEAR_ISOTROPIC_FACTOR,
        },
        InfillPattern::Grid | InfillPattern::Triangular | InfillPattern::Honeycomb => {
            PatternFactors {
                in_plane_strong: DIRECTIONAL_STRONG_FACTOR,
                in_plane_weak: DIRECTIONAL_WEAK_FACTOR,
            }
        }
    }
}

// ── Effective-property assemblers ─────────────────────────────────────────────

/// Isotropic elastic properties of the base filament material (SI units).
/// The single input to the correlation assemblers; δ builds this from the
/// resolved base `Material`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BaseElastic {
    /// Solid (fully dense) Young's modulus, Pa.
    pub youngs_modulus: f64,
    /// Isotropic Poisson ratio (dimensionless).
    pub poisson_ratio: f64,
    /// Solid mass density, kg/m³.
    pub density: f64,
}

/// The 5-constant transverse-isotropic conformer result (plus density).
///
/// Field names mirror the stdlib `TransverseIsotropicMaterial`
/// (`crates/reify-compiler/stdlib/constitutive.ri`) one-to-one so δ maps this
/// onto the stdlib constructor without a translation layer. The in-plane (XY,
/// print-plane) is the isotropy plane; the axial direction is the build (Z)
/// axis. The in-plane shear is derived on the producer side as
/// `G12 = e_in_plane / (2 (1 + nu_in_plane))`; `g_axial` is the independent
/// out-of-plane shear (G13 = G23).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransverseIsoConstants {
    /// In-plane Young's modulus E1 = E2 (isotropy plane), Pa.
    pub e_in_plane: f64,
    /// Axial (build-Z) Young's modulus E3, Pa — the weakest axis.
    pub e_axial: f64,
    /// In-plane Poisson ratio ν12 (dimensionless).
    pub nu_in_plane: f64,
    /// Axial Poisson ratio ν13 = ν23 (dimensionless).
    pub nu_axial: f64,
    /// Axial shear modulus G13 = G23, Pa (the independent shear constant).
    pub g_axial: f64,
    /// Effective mass density, kg/m³ (solid density · solid fraction).
    pub density: f64,
}

/// User-supplied coupon overrides for measured effective properties.
///
/// Any set (`Some`) field beats the corresponding computed default; unset
/// (`None`) fields fall back to the correlation result. This is the Rust
/// mirror of the stdlib `FDMCouponOverride` structure; δ reads that stdlib
/// `Value` and builds this. All values are SI (`f64`): moduli in Pa.
///
/// `Default` yields the all-`None` (no-override) coupon.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CouponOverride {
    /// Measured in-plane (print-plane) Young's modulus Ex, Pa. Overrides
    /// `e_in_plane` / `e1`. Takes priority over [`ey`](Self::ey).
    pub ex: Option<f64>,
    /// Measured in-plane Young's modulus Ey, Pa. Overrides the in-plane
    /// modulus when [`ex`](Self::ex) is unset; sets `e2` on the orthotropic
    /// path.
    pub ey: Option<f64>,
    /// Measured build-direction (Z) Young's modulus Ez, Pa. Overrides
    /// `e_axial` / `e3`.
    pub ez: Option<f64>,
    /// Measured shear modulus, Pa. Overrides the conformer's independent
    /// stored shear constant — `g_axial` on the transverse-isotropic path,
    /// `g12` on the orthotropic path.
    pub gxy: Option<f64>,
    /// Override for the Gibson-Ashby coefficient C of the infill knockdown
    /// law (`E_eff/E_solid = C·ρ^n`); the "infill curve override". Replaces
    /// [`GIBSON_ASHBY_C`] before the factor is computed.
    pub infill_c: Option<f64>,
    /// Override for the Gibson-Ashby exponent n of the infill knockdown law.
    /// Replaces [`GIBSON_ASHBY_N`] before the factor is computed.
    pub infill_n: Option<f64>,
}

/// Shared preamble for the effective-property assemblers: resolve the
/// Gibson-Ashby `(C, n)` from the coupon (or the [`GIBSON_ASHBY_C`] /
/// [`GIBSON_ASHBY_N`] defaults), compute the infill knockdown factor, and
/// fetch the pattern's in-plane factors. Extracting this keeps the two
/// assemblers from drifting if the override-resolution logic ever grows
/// (e.g. clamping/validation).
///
/// `solid_fraction` is the infill relative density ρ ∈ (0, 1]. The domain is
/// guarded by a `debug_assert!` so a bad value (ρ ≤ 0 ⇒ zero/singular
/// stiffness; ρ > 1 ⇒ moduli above the base material) surfaces here, at the
/// source, rather than as a degenerate constitutive matrix downstream.
/// Callers (δ) are expected to pre-validate; the assert is a development-time
/// backstop, compiled out of release builds.
fn infill_and_pattern(
    solid_fraction: f64,
    pattern: InfillPattern,
    coupon: &CouponOverride,
) -> (f64, PatternFactors) {
    debug_assert!(
        solid_fraction > 0.0 && solid_fraction <= 1.0,
        "solid_fraction (infill relative density) must be in (0, 1]; got {solid_fraction}"
    );
    let c = coupon.infill_c.unwrap_or(GIBSON_ASHBY_C);
    let n = coupon.infill_n.unwrap_or(GIBSON_ASHBY_N);
    let infill = gibson_ashby_infill_factor(solid_fraction, c, n);
    let pf = pattern_factors(pattern);
    (infill, pf)
}

/// Build the default-path transverse-isotropic constants for an FDM-printed
/// region: base filament + infill solid fraction + infill pattern.
///
/// In-plane modulus is the base modulus knocked down by the Gibson-Ashby
/// infill law and scaled by the pattern's strong-direction factor; the axial
/// (build-Z) modulus applies the [`BUILD_Z_MODULUS_RATIO`] knockdown so the
/// build direction is the weakest axis (PRD C4 invariant).
///
/// Coupon overrides (set fields of `coupon`) beat the computed defaults:
/// `infill_c`/`infill_n` replace the Gibson-Ashby `(C, n)` before the factor
/// is computed; `ex` (else `ey`) sets `e_in_plane`; `ez` sets `e_axial`; `gxy`
/// sets the independent stored shear `g_axial`. Each unset field falls back to
/// the correlation result.
pub fn effective_transverse_isotropic(
    base: BaseElastic,
    solid_fraction: f64,
    pattern: InfillPattern,
    coupon: &CouponOverride,
) -> TransverseIsoConstants {
    let (infill, pf) = infill_and_pattern(solid_fraction, pattern, coupon);

    // In-plane modulus: ex (else ey) override beats the computed default.
    let e_in_plane = coupon
        .ex
        .or(coupon.ey)
        .unwrap_or(base.youngs_modulus * infill * pf.in_plane_strong);
    // Axial (build-Z) modulus: ez override beats the knockdown of the
    // (possibly overridden) in-plane modulus.
    let e_axial = coupon.ez.unwrap_or(e_in_plane * BUILD_Z_MODULUS_RATIO);
    let nu_in_plane = base.poisson_ratio;
    let nu_axial = base.poisson_ratio;
    // Axial shear: gxy override beats the default derived from the axial
    // modulus (the independent transverse-isotropic shear constant).
    let g_axial = coupon
        .gxy
        .unwrap_or(e_axial / (2.0 * (1.0 + nu_axial)));
    let density = base.density * solid_fraction;

    TransverseIsoConstants {
        e_in_plane,
        e_axial,
        nu_in_plane,
        nu_axial,
        g_axial,
        density,
    }
}

/// The 9-constant orthotropic conformer result (plus density).
///
/// Field names mirror the stdlib `OrthotropicMaterial`
/// (`crates/reify-compiler/stdlib/constitutive.ri`) one-to-one. Axis
/// convention: `1` = along-raster (strong in-plane), `2` = transverse
/// in-plane, `3` = build (Z) axis — the weakest. This is the opt-in path for
/// known-unidirectional raster; the transverse-isotropic 5-constant model
/// ([`effective_transverse_isotropic`]) remains the default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrthotropicConstants {
    /// Along-raster Young's modulus E1, Pa (strongest).
    pub e1: f64,
    /// Transverse in-plane Young's modulus E2, Pa.
    pub e2: f64,
    /// Build-Z Young's modulus E3, Pa (weakest).
    pub e3: f64,
    /// In-plane shear modulus G12, Pa.
    pub g12: f64,
    /// G13 shear modulus, Pa.
    pub g13: f64,
    /// G23 shear modulus, Pa.
    pub g23: f64,
    /// Poisson ratio ν12 (dimensionless).
    pub nu12: f64,
    /// Poisson ratio ν13 (dimensionless).
    pub nu13: f64,
    /// Poisson ratio ν23 (dimensionless).
    pub nu23: f64,
    /// Effective mass density, kg/m³ (solid density · solid fraction).
    pub density: f64,
}

/// Build the orthotropic constants for a known-unidirectional-raster FDM
/// region. Opt-in path; the transverse-isotropic model is the default.
///
/// The in-plane directional split comes from the pattern's strong/weak
/// factors (E1 = strong, E2 = weak); the build-Z modulus applies the
/// [`BUILD_Z_MODULUS_RATIO`] knockdown to the weak in-plane modulus, so the
/// ordering is E1 > E2 > E3 with the build direction weakest. Shear moduli use
/// a geometric-mean estimate of the relevant axial pair (no independent shear
/// data); Poisson ratios default to the base value.
///
/// Coupon overrides beat the defaults: `infill_c`/`infill_n` replace the
/// Gibson-Ashby `(C, n)`; `ex`/`ey`/`ez` set E1/E2/E3; `gxy` sets the in-plane
/// shear G12. Unset fields fall back to the correlation result.
pub fn effective_orthotropic(
    base: BaseElastic,
    solid_fraction: f64,
    pattern: InfillPattern,
    coupon: &CouponOverride,
) -> OrthotropicConstants {
    let (infill, pf) = infill_and_pattern(solid_fraction, pattern, coupon);

    let e1 = coupon
        .ex
        .unwrap_or(base.youngs_modulus * infill * pf.in_plane_strong);
    let e2 = coupon
        .ey
        .unwrap_or(base.youngs_modulus * infill * pf.in_plane_weak);
    // Build-Z is the weakest axis: knock the (weak) in-plane modulus down.
    let e3 = coupon.ez.unwrap_or(e2 * BUILD_Z_MODULUS_RATIO);

    let nu12 = base.poisson_ratio;
    let nu13 = base.poisson_ratio;
    let nu23 = base.poisson_ratio;

    // Geometric-mean shear estimate per plane (no independent shear data);
    // gxy overrides the in-plane shear G12.
    let g12 = coupon
        .gxy
        .unwrap_or((e1 * e2).sqrt() / (2.0 * (1.0 + nu12)));
    let g13 = (e1 * e3).sqrt() / (2.0 * (1.0 + nu13));
    let g23 = (e2 * e3).sqrt() / (2.0 * (1.0 + nu23));

    let density = base.density * solid_fraction;

    OrthotropicConstants {
        e1,
        e2,
        e3,
        g12,
        g13,
        g23,
        nu12,
        nu13,
        nu23,
        density,
    }
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

    /// Pins the Rust-side default constants to the PRD-specified literals.
    /// Scope is the Rust surface only — the name no longer claims `_and_stdlib`
    /// because nothing here loads the stdlib. The stdlib `FDMCorrelationDefaults`
    /// surface is pinned to the same literals independently by
    /// `reify-compiler/tests/fdm_correlations_stdlib_compile.rs`; see the module
    /// header for why a direct cross-surface comparison lives in neither place.
    #[test]
    fn default_constants_match_prd() {
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

    /// `|a - b| <= rel · |b|` — relative-tolerance comparison.
    fn approx_rel(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs()
    }

    /// A PLA-like base filament: E ≈ 2.3 GPa, ν = 0.35, ρ ≈ 1.24 g/cc.
    fn pla_base() -> BaseElastic {
        BaseElastic {
            youngs_modulus: 2.3e9,
            poisson_ratio: 0.35,
            density: 1240.0,
        }
    }

    #[test]
    fn transverse_iso_dense_gyroid_recovers_base_in_plane_and_weak_axial() {
        let base = pla_base();
        let c = effective_transverse_isotropic(base, 1.0, InfillPattern::Gyroid, &CouponOverride::default());
        // Dense (ρ=1) + near-isotropic (factor 1) ⇒ in-plane modulus ≈ base E.
        assert!(
            approx_rel(c.e_in_plane, base.youngs_modulus, 1e-3),
            "dense gyroid e_in_plane {} should ≈ base E {}",
            c.e_in_plane,
            base.youngs_modulus
        );
        // Build-Z axial modulus = in-plane · 0.67 exactly (PRD C4 invariant).
        assert!(
            (c.e_axial - c.e_in_plane * BUILD_Z_MODULUS_RATIO).abs() < EPS,
            "e_axial {} should equal e_in_plane · BUILD_Z_MODULUS_RATIO {}",
            c.e_axial,
            c.e_in_plane * BUILD_Z_MODULUS_RATIO
        );
        // Build-Z (axial) is the weakest axis — the load-bearing invariant.
        assert!(
            c.e_axial < c.e_in_plane,
            "build-Z (axial {}) must be weaker than in-plane ({})",
            c.e_axial,
            c.e_in_plane
        );
    }

    #[test]
    fn transverse_iso_sparse_infill_applies_gibson_ashby_knockdown() {
        let base = pla_base();
        let c =
            effective_transverse_isotropic(base, 0.2, InfillPattern::Gyroid, &CouponOverride::default());
        // ρ=0.2 ⇒ Gibson-Ashby factor 0.2^2 = 0.04 ⇒ e_in_plane = base E · 0.04.
        let expected = base.youngs_modulus * 0.04;
        assert!(
            approx_rel(c.e_in_plane, expected, 1e-3),
            "sparse-infill e_in_plane {} should ≈ base E · 0.04 = {}",
            c.e_in_plane,
            expected
        );
    }

    #[test]
    fn coupon_override_replaces_computed_constants() {
        let base = pla_base();
        // No-override baseline at the same inputs, for the fall-back comparison.
        let baseline = effective_transverse_isotropic(
            base,
            0.5,
            InfillPattern::Gyroid,
            &CouponOverride::default(),
        );
        let coupon = CouponOverride {
            ex: Some(4.0e9),
            ez: Some(1.5e9),
            gxy: Some(1.2e9),
            infill_n: Some(1.8),
            ..Default::default()
        };
        let c = effective_transverse_isotropic(base, 0.5, InfillPattern::Gyroid, &coupon);

        // The PRD signal: a coupon-measured value beats the computed default.
        assert_eq!(c.e_in_plane, 4.0e9, "ex overrides e_in_plane");
        assert_eq!(c.e_axial, 1.5e9, "ez overrides e_axial");
        assert_eq!(
            c.g_axial, 1.2e9,
            "gxy overrides the conformer's independent stored shear (g_axial)"
        );

        // Fields the coupon left None fall back to the computed defaults.
        assert_eq!(
            c.nu_in_plane, baseline.nu_in_plane,
            "nu_in_plane (no override) falls back to the default"
        );
        assert_eq!(
            c.nu_axial, baseline.nu_axial,
            "nu_axial (no override) falls back to the default"
        );
        assert_eq!(
            c.density, baseline.density,
            "density (no override) falls back to the default"
        );
    }

    #[test]
    fn coupon_infill_exponent_override_changes_knockdown() {
        let base = pla_base();
        let rho = 0.2;
        let baseline =
            effective_transverse_isotropic(base, rho, InfillPattern::Gyroid, &CouponOverride::default());
        // Override only the Gibson-Ashby exponent (ex unset ⇒ effect observable).
        let coupon = CouponOverride {
            infill_n: Some(1.8),
            ..Default::default()
        };
        let c = effective_transverse_isotropic(base, rho, InfillPattern::Gyroid, &coupon);

        // e_in_plane must now follow base · C · ρ^1.8 (the overridden curve).
        let expected = base.youngs_modulus * gibson_ashby_infill_factor(rho, GIBSON_ASHBY_C, 1.8);
        assert!(
            (c.e_in_plane - expected).abs() <= 1e-9 * expected,
            "infill_n override should drive e_in_plane to base·C·ρ^1.8 = {}, got {}",
            expected,
            c.e_in_plane
        );
        // n: 2.0 → 1.8 raises the factor (ρ<1) ⇒ higher modulus than the default.
        assert!(
            c.e_in_plane > baseline.e_in_plane,
            "smaller exponent ⇒ less knockdown ⇒ higher e_in_plane than the default"
        );
    }

    #[test]
    fn orthotropic_directional_pattern_orders_e1_gt_e2_gt_e3() {
        let base = pla_base();
        // Grid is directional ⇒ a genuine in-plane split.
        let c = effective_orthotropic(base, 0.5, InfillPattern::Grid, &CouponOverride::default());
        // Along-raster (E1) stiffer than transverse (E2).
        assert!(
            c.e1 > c.e2,
            "directional pattern: E1 ({}) should exceed E2 ({})",
            c.e1,
            c.e2
        );
        // Build-Z (E3) is the weakest axis: E3 < E2 (PRD ordering E1 > E2 > E3).
        assert!(
            c.e3 < c.e2,
            "build-Z E3 ({}) must be weaker than in-plane E2 ({})",
            c.e3,
            c.e2
        );
        assert!(c.e1 > c.e3, "E1 ({}) > E3 ({}) — build-Z weakest", c.e1, c.e3);
    }

    #[test]
    fn orthotropic_coupon_overrides_e1_and_e3() {
        let base = pla_base();
        let coupon = CouponOverride {
            ex: Some(5.0e9),
            ez: Some(1.0e9),
            ..Default::default()
        };
        let c = effective_orthotropic(base, 0.5, InfillPattern::Grid, &coupon);
        assert_eq!(c.e1, 5.0e9, "ex overrides E1");
        assert_eq!(c.e3, 1.0e9, "ez overrides E3");
    }
}
