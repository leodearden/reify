//! Public morph API types: [`MorphOptions`] and [`MorphFailure`].
//!
//! Implements task #4 of the mesh-morphing PRD
//! (`docs/prds/v0_3/mesh-morphing.md`): the consumer-neutral public API
//! surface for the morph engine.

use crate::eligibility::Reason;
use crate::types::{InversionDetails, SoftFailDetails, SolverErrorPayload};

// ── StiffnessRule ─────────────────────────────────────────────────────────────

/// Per-element stiffness scaling rule for the fictitious-elastic morph.
///
/// Controls how the element-local Young's modulus `E_e` is derived from the
/// base value [`MorphOptions::fictitious_youngs_modulus_base`] during the FEA
/// solve in PRD `docs/prds/v0_3/mesh-morphing.md` §"Spatially-varying
/// fictitious stiffness". Small elements (near features) receive higher `E_e`
/// and thus absorb less displacement; large bulk-region elements receive lower
/// `E_e` and absorb most of the displacement — preserving mesh gradation.
///
/// The homogeneous BVP `K · u = 0` is invariant under uniform E rescaling
/// (only ratios E_i/E_j matter), so the absolute scale of `E_base` does not
/// affect the solution when `Uniform` is selected. For the spatially-varying
/// rules the ratios between element stiffnesses do affect the solution.
///
/// The exhaustive variant fence test `stiffness_rule_variants_construct_and_pattern_match_exhaustively`
/// in `options::tests` acts as a compile-fence: adding, removing, or renaming
/// a variant breaks the test immediately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StiffnessRule {
    /// `E_e = E_base` — uniform stiffness for every element.
    ///
    /// Bit-identical to the task #7 baseline. Use when comparing against the
    /// uniform-stiffness result or when mesh gradation is not a concern.
    ///
    /// PRD §"Spatially-varying fictitious stiffness": Uniform is the baseline;
    /// `InverseVolume` is the prescribed default for task #8.
    Uniform,

    /// `E_e = E_base / max(V_e, ε)` — stiffness inversely proportional to
    /// element volume.
    ///
    /// Small-volume elements (near features) become stiffer; large-volume
    /// bulk elements become softer and absorb most of the displacement.
    /// This is the **default** rule, prescribed by PRD task #8 for
    /// mesh-gradation preservation.
    ///
    /// `ε = 1e-30` guards against degenerate (zero-volume) tets producing
    /// infinite stiffness. Mirrors the `MIN_JACOBIAN_DET` precedent in
    /// `reify-solver-elastic`.
    InverseVolume,

    /// `E_e = E_base / max(mean_L²_e, ε)` — stiffness inversely proportional
    /// to the mean squared edge length.
    ///
    /// `mean_L²_e` averages `|v_i - v_j|²` over the 6 edges of the tet
    /// (pairs 0-1, 0-2, 0-3, 1-2, 1-3, 2-3) — robust to sliver tets with one
    /// extreme edge. This rule is behaviourally distinct from `InverseVolume`
    /// on irregular tets because it uses actual edge lengths rather than a
    /// V-derived proxy.
    ///
    /// `ε = 1e-30` guards against degenerate tets (all vertices coincident).
    InverseEdgeLengthSquared,
}

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
    /// Produced by the quality-check pass in PRD task #9. The payload records
    /// which of the [`crate::MorphOptions`] thresholds tripped. Also fires when
    /// a morphed tet is detected as exactly degenerate (scaled Jacobian == 0.0),
    /// independent of caller-configured thresholds — `degenerate_morphed_element`
    /// will be `Some(element_index)` in that case even when all threshold floors
    /// are set to zero.
    QualitySoftFail(SoftFailDetails),

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
    /// PRD §"Quality threshold for fallback": seed 0.15.
    /// Calibrated by task #2950 against tests/calibration.rs (plate
    /// hole-diameter and bracket fillet-radius sweeps under
    /// StiffnessRule::InverseVolume) to 0.02.
    pub quality_floor_min_scaled_jacobian: f64,

    /// Maximum acceptable fraction of elements with scaled Jacobian < 0.25.
    ///
    /// PRD §"Quality threshold for fallback": seed 0.01 (1 %). Production
    /// default retained at the PRD seed; the calibration sweep tests in
    /// `tests/calibration.rs` override this locally to 0.95 to admit the
    /// synthetic procedural fixtures' baseline pct distribution
    /// (structured hex-to-6-tet decomposition skews toward sj < 0.25).
    /// Task #2950 calibration confirmed the seed is conservative against
    /// the discriminating bracket fillet-radius sweep.
    pub quality_floor_pct_below_025: f64,

    /// Maximum acceptable multiplicative aspect-ratio factor (morphed_AR / source_AR)
    /// relative to the pre-morph mesh. PRD §"Quality threshold for fallback": seed
    /// 2.0×. A value > 1 indicates worsening; the threshold trips when the observed
    /// factor exceeds this maximum.
    /// Calibrated by task #2950 against tests/calibration.rs (plate
    /// hole-diameter and bracket fillet-radius sweeps under
    /// StiffnessRule::InverseVolume); seed retained.
    pub quality_aspect_ratio_factor_max: f64,

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

    /// Number of Jacobi iterations the Laplacian quick-pass runs before
    /// returning the smoothed mesh.
    ///
    /// PRD task #6 ("Laplacian quick-pass for trivially small changes"):
    /// 5–10 iterations is the typical range; default 8. Engine wiring
    /// (PRD task #10) reads this value and forwards it to
    /// [`crate::laplacian::laplacian_smooth`].
    pub laplacian_iterations: u32,

    /// Per-element stiffness scaling rule for the fictitious-elastic morph.
    ///
    /// PRD `docs/prds/v0_3/mesh-morphing.md` §"Spatially-varying fictitious
    /// stiffness" (task #8): [`StiffnessRule::InverseVolume`] is the
    /// prescribed default — small-volume elements (near features) become
    /// stiffer, preserving mesh gradation. Use [`StiffnessRule::Uniform`] to
    /// reproduce the task #7 baseline bit-for-bit.
    pub stiffness_rule: StiffnessRule,
}

impl Default for MorphOptions {
    fn default() -> Self {
        Self {
            // Calibrated by task #2950 against tests/calibration.rs (plate
            // hole-diameter and bracket fillet-radius sweeps under
            // StiffnessRule::InverseVolume).
            //
            // PRD seed was 0.15; calibration lowered to 0.02 because the
            // procedural plate-with-hole fixture (polar-radial grid
            // hex-to-6-tet decomposition) intrinsically produces tets with
            // min_sj ≈ 0.022–0.024 at small parameter steps — well below
            // the 0.15 PRD seed. The materially-better rule (>20%
            // improvement on the relevant metric) holds at the 0.02 floor
            // across the plate and bracket sweeps.
            quality_floor_min_scaled_jacobian: 0.02,
            // PRD seed 0.01 retained as the production default (task #3434
            // follow-up to #2950). Earlier calibration relaxed the default to
            // 0.95 to admit the synthetic procedural fixtures, but at 0.95 the
            // metric is near-vestigial in production (almost any mesh satisfies
            // pct < 0.95). The two materially-better-rule sweep tests in
            // tests/calibration.rs (plate hole-diameter, bracket fillet-radius)
            // override this field locally to 0.95 because the procedural
            // fixtures' structured hex-to-6-tet decomposition produces
            // populations skewed toward sj < 0.25 (e.g. plate base pct ≈ 0.91
            // at hole_diameter = 0.30). Re-evaluate the production default
            // against real CAD-derived meshes once PRD task #10 (engine wiring,
            // `lib.rs::morph()`) lands.
            quality_floor_pct_below_025: 0.01,
            // PRD seed 2.0 retained — calibration confirmed it discriminates
            // bracket fillet-radius distortion (AR ≈ 2.75 at target=0.15
            // rejects) while leaving plate hole-diameter and box wall-
            // thickness sweeps (max AR ≈ 1.84 and 1.0 respectively) in Pass.
            //
            // Known asymmetry vs the materiality bar: the 2.0 floor is
            // well above the 1.20 materiality factor used by the calibration
            // rule, so Pass cases with `morph_ar_factor ∈ (1.20, 2.0)` are
            // admitted even though the morph is technically materially
            // worse than a fresh remesh on AR (e.g. bracket target=0.12
            // Pass at AR=1.37). The calibration rule helper in
            // `tests/calibration.rs::assert_materially_better_rule_holds`
            // documents the asymmetry explicitly. If this floor is ever
            // tightened toward the materiality bar, that Pass-branch
            // helper must add the symmetric AR check.
            quality_aspect_ratio_factor_max: 2.0,
            laplacian_quickpass_threshold: 0.01,
            fictitious_youngs_modulus_base: 1.0,
            fictitious_poisson_ratio: 0.3,
            laplacian_iterations: 8,
            // PRD task #8: InverseVolume is the prescribed default for
            // mesh-gradation preservation (small elements stay stiff).
            // Backward-compatibility audit: all pre-task-#8 tests in this
            // crate use rigid-body BCs (invariant to E scaling — rigid modes
            // lie in the kernel of every K_e) or zero-displacement BCs
            // (post-Dirichlet K → diag(1.0), E-invariant). No pinned-output
            // tests were broken by switching the default from Uniform.
            stiffness_rule: StiffnessRule::InverseVolume,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eligibility::Reason;
    use crate::types::{InversionDetails, SoftFailDetails};

    #[test]
    fn morph_options_default_returns_prd_calibrated_quality_and_stiffness_values() {
        let opts = MorphOptions::default();
        // Threshold-related fields calibrated by task #2950 against
        // tests/calibration.rs (plate hole-diameter and bracket fillet-
        // radius sweeps under the StiffnessRule::InverseVolume production
        // default).
        assert!((opts.quality_floor_min_scaled_jacobian - 0.02).abs() < 1e-12);
        assert!((opts.quality_floor_pct_below_025 - 0.01).abs() < 1e-12);
        assert!((opts.quality_aspect_ratio_factor_max - 2.0).abs() < 1e-12);
        assert!((opts.laplacian_quickpass_threshold - 0.01).abs() < 1e-12);
        assert!((opts.fictitious_youngs_modulus_base - 1.0).abs() < 1e-12);
        assert!((opts.fictitious_poisson_ratio - 0.3).abs() < 1e-12);
        // PRD task #6 (Laplacian quick-pass): 5–10 typical, default 8.
        assert_eq!(opts.laplacian_iterations, 8);
        // PRD task #8 (spatially-varying fictitious stiffness): InverseVolume is the
        // prescribed default for mesh-gradation preservation.
        assert_eq!(opts.stiffness_rule, StiffnessRule::InverseVolume);
    }

    /// Compile-fence: exhaustive no-wildcard match over all three [`StiffnessRule`]
    /// variants. Adding, removing, or renaming a variant breaks compilation
    /// immediately — mirrors `elasticity_failure_variants_construct_and_pattern_match_exhaustively`
    /// (elasticity.rs:537-556) and `morph_failure_four_variants_*` below.
    #[test]
    fn stiffness_rule_variants_construct_and_pattern_match_exhaustively() {
        let uniform = StiffnessRule::Uniform;
        let inv_vol = StiffnessRule::InverseVolume;
        let inv_edge_l_sq = StiffnessRule::InverseEdgeLengthSquared;

        for rule in [&uniform, &inv_vol, &inv_edge_l_sq] {
            match rule {
                StiffnessRule::Uniform => {}
                StiffnessRule::InverseVolume => {}
                StiffnessRule::InverseEdgeLengthSquared => {}
            }
        }
    }

    #[test]
    fn morph_failure_four_variants_construct_and_pattern_match_exhaustively() {
        let ineligible = MorphFailure::Ineligible(Reason::StructuralChange);
        let hard_fail = MorphFailure::QualityHardFail(InversionDetails {
            element_index: 7,
            jacobian: -1.0,
        });
        let soft_fail = MorphFailure::QualitySoftFail(SoftFailDetails {
            min_scaled_jacobian: Some(0.10),
            pct_below_025: Some(0.02),
            max_aspect_ratio_factor: Some(2.5),
            degenerate_morphed_element: None,
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
                    assert_eq!(d.jacobian, -1.0);
                }
                MorphFailure::QualitySoftFail(m) => {
                    assert_eq!(m.min_scaled_jacobian, Some(0.10));
                    assert_eq!(m.pct_below_025, Some(0.02));
                    assert_eq!(m.max_aspect_ratio_factor, Some(2.5));
                }
                MorphFailure::SolverError(p) => {
                    assert_eq!(p.message(), "singular stiffness matrix");
                }
            }
        }
    }
}
