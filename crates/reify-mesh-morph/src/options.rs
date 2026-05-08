//! Public morph API types: [`MorphOptions`] and [`MorphFailure`].
//!
//! Implements task #4 of the mesh-morphing PRD
//! (`docs/prds/v0_3/mesh-morphing.md`): the consumer-neutral public API
//! surface for the morph engine.

use crate::eligibility::Reason;
use crate::types::{InversionDetails, MetricsBreached, SolverErrorPayload};

// ── MorphFailure ──────────────────────────────────────────────────────────────

/// Structured failure result from [`crate::morph`].
///
/// Four variants covering the full failure-mode taxonomy in PRD
/// `docs/prds/v0_3/mesh-morphing.md` §"Failure-mode visibility":
///
/// | Variant | Produced by |
/// |---------|-------------|
/// | `Ineligible` | PRD task #3 eligibility predicate (already shipped) |
/// | `QualityHardFail` | PRD task #9 quality-check pass |
/// | `QualitySoftFail` | PRD task #9 quality-check pass |
/// | `SolverError` | PRD task #7 elastic-solve kernel |
///
/// The exhaustive contract test in `options::tests` acts as a compile-fence:
/// adding, removing, or renaming a variant breaks the test immediately.
#[derive(Debug, Clone, PartialEq)]
pub enum MorphFailure {
    /// The edit was rejected by the eligibility predicate (task #3).
    ///
    /// Mirrors `eligibility::Eligibility::Ineligible`. The full structured
    /// [`Reason`] is preserved so callers can route failure-mode counters
    /// (PRD task #11) without re-running eligibility.
    Ineligible(Reason),

    /// One or more output elements have a negative Jacobian (hard inversion).
    ///
    /// Produced by the quality-check pass in PRD task #9. The payload
    /// identifies the first offending element.
    QualityHardFail(InversionDetails),

    /// Soft quality thresholds were breached but no hard inversion occurred.
    ///
    /// Produced by the quality-check pass in PRD task #9. The payload
    /// records which of the [`crate::MorphOptions`] thresholds tripped.
    QualitySoftFail(MetricsBreached),

    /// The elastic-solve kernel failed (e.g. singular stiffness matrix).
    ///
    /// Produced by PRD task #7's `solve_elastic_static` integration.
    /// The payload is wrapped in [`SolverErrorPayload`] so future tasks can
    /// add structured fields (e.g. a kernel-error code from
    /// `reify-solver-elastic`) without a breaking change to match arms.
    SolverError(SolverErrorPayload),
}

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
    use crate::eligibility::Reason;
    use crate::types::{InversionDetails, MetricsBreached};

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

    #[test]
    fn morph_failure_four_variants_construct_and_pattern_match_exhaustively() {
        let ineligible = MorphFailure::Ineligible(Reason::StructuralChange);
        let hard_fail = MorphFailure::QualityHardFail(InversionDetails {
            element_index: 7,
            jacobian: -1.0,
        });
        let soft_fail = MorphFailure::QualitySoftFail(MetricsBreached {
            min_scaled_jacobian: Some(0.10),
            pct_below_025: Some(0.02),
            max_aspect_ratio_increase: Some(2.5),
        });
        let solver_err =
            MorphFailure::SolverError(SolverErrorPayload::new("singular stiffness matrix"));

        // Exhaustive compile-fence: a no-wildcard match over each of the four
        // locally-bound variants ensures that adding, removing, or renaming a
        // variant in MorphFailure breaks compilation immediately — the contract
        // the doc-comment on MorphFailure advertises. Each arm also probes the
        // carried payload via a field accessor so a constructor that drops or
        // swaps a field is caught (not merely PartialEq reflexivity).
        for failure in [&ineligible, &hard_fail, &soft_fail, &solver_err] {
            match failure {
                MorphFailure::Ineligible(reason) => {
                    assert_eq!(*reason, Reason::StructuralChange);
                }
                MorphFailure::QualityHardFail(d) => {
                    assert_eq!(d.element_index, 7);
                }
                MorphFailure::QualitySoftFail(m) => {
                    assert_eq!(m.min_scaled_jacobian, Some(0.10));
                }
                MorphFailure::SolverError(p) => {
                    assert_eq!(p.message(), "singular stiffness matrix");
                }
            }
        }
    }
}
