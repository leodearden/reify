//! Public morph API types: [`MorphOptions`] and [`MorphFailure`].
//!
//! Implements task #4 of the mesh-morphing PRD
//! (`docs/prds/v0_3/mesh-morphing.md`): the consumer-neutral public API
//! surface for the morph engine.

// ── MorphOptions ──────────────────────────────────────────────────────────────

/// Tunable parameters for the mesh-morphing pipeline.
///
/// Defaults are calibrated from PRD `docs/prds/v0_3/mesh-morphing.md`
/// §"Quality threshold for fallback" and §"Spatially-varying fictitious
/// stiffness". All six values are intentionally tunable — PRD task #13
/// (calibration pass) may adjust them based on benchmarking.
#[derive(Debug, Clone, PartialEq)]
pub struct MorphOptions {
    /// Minimum scaled Jacobian below which an element is considered inverted.
    ///
    /// PRD §"Quality threshold for fallback": default 0.15.
    pub quality_floor_min_scaled_jacobian: f64,

    /// Maximum acceptable fraction of elements with scaled Jacobian < 0.25.
    ///
    /// PRD §"Quality threshold for fallback": default 0.01 (1 %).
    pub quality_floor_pct_below_025: f64,

    /// Maximum acceptable multiplicative increase in element aspect ratio
    /// relative to the pre-morph mesh.
    ///
    /// PRD §"Quality threshold for fallback": default 2.0×.
    pub quality_aspect_ratio_increase_max: f64,

    /// Scaled-Jacobian delta below which the Laplacian quick-pass is
    /// considered converged and no elastic solve is triggered.
    ///
    /// PRD §"Spatially-varying fictitious stiffness": default 0.01.
    pub laplacian_quickpass_threshold: f64,

    /// Base unitless Young's modulus for the fictitious-stiffness elastic
    /// solve. The spatial-stiffness rule (PRD task #8) scales this value
    /// elementwise; the absolute base cancels from the BVP since loads = [].
    ///
    /// PRD §"Spatially-varying fictitious stiffness": default 1.0.
    pub fictitious_youngs_modulus_base: f64,

    /// Fictitious Poisson's ratio for the elastic solve.
    ///
    /// PRD §"Spatially-varying fictitious stiffness": default 0.3.
    pub fictitious_poisson_ratio: f64,
}

impl Default for MorphOptions {
    fn default() -> Self {
        Self {
            quality_floor_min_scaled_jacobian: 0.15,
            quality_floor_pct_below_025: 0.01,
            quality_aspect_ratio_increase_max: 2.0,
            laplacian_quickpass_threshold: 0.01,
            fictitious_youngs_modulus_base: 1.0,
            fictitious_poisson_ratio: 0.3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn morph_options_default_returns_prd_calibrated_quality_and_stiffness_values() {
        let opts = MorphOptions::default();
        assert!((opts.quality_floor_min_scaled_jacobian - 0.15).abs() < 1e-12);
        assert!((opts.quality_floor_pct_below_025 - 0.01).abs() < 1e-12);
        assert!((opts.quality_aspect_ratio_increase_max - 2.0).abs() < 1e-12);
        assert!((opts.laplacian_quickpass_threshold - 0.01).abs() < 1e-12);
        assert!((opts.fictitious_youngs_modulus_base - 1.0).abs() < 1e-12);
        assert!((opts.fictitious_poisson_ratio - 0.3).abs() < 1e-12);
    }
}
