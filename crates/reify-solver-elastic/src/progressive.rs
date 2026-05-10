/// Progressive-solve framework for the linear-elastostatic FEA kernel.
///
/// PRD reference: `docs/prds/v0_3/structural-analysis-fea.md` task #15.
///
/// This module supplies the **scheduling/policy primitives** that the engine
/// integration (PRD task #16) and the auto-resolve loop will compose:
///
/// - [`PassTuning`] — `(mesh_tol, cg_tol)` pair for a single solve pass.
/// - [`coarse_pass_tuning`] — derive the fast first-pass tuning (`tol×4`, CG `1e-3`).
/// - [`refinement_pass_tuning`] — derive per-refinement-level tuning (halve mesh, ÷10 CG per level).
/// - [`near_constraint_boundary`] — auto-refine trigger: `max_von_mises` within
///   `near_boundary_pct` of `yield_stress`.
/// - [`should_refine`] — decision oracle: returns [`AdvanceDecision::Continue`] or
///   [`AdvanceDecision::Terminate`] given budget, demand, and auto-detect signals.

/// Configuration for the progressive-solve schedule.
///
/// Created with [`ProgressiveOptions::default()`] for typical engineering use
/// or constructed field-by-field for custom tolerances and budgets.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressiveOptions {
    /// Requested engineering accuracy (mesh tolerance). The coarse pass uses
    /// `target_tolerance × 4`; each refinement halves the mesh element size.
    pub target_tolerance: f64,

    /// Material yield stress in Pa. When `Some`, the auto-refine trigger activates
    /// if `max_von_mises` comes within `near_boundary_pct` of this value.
    /// `None` disables yield-proximity auto-refinement.
    pub yield_stress: Option<f64>,

    /// Fraction of `yield_stress` defining the "near-boundary" zone.
    /// Default 0.10 means "within 10% of yield stress triggers auto-refinement".
    /// Must be in `(0.0, 1.0)`.
    pub near_boundary_pct: f64,

    /// Maximum number of refinement passes beyond the initial coarse pass.
    /// When `current_level >= max_refinements`, [`should_refine`] returns
    /// [`AdvanceDecision::Terminate(TerminationReason::BudgetExhausted)`].
    pub max_refinements: usize,
}

impl Default for ProgressiveOptions {
    /// Returns a sensible engineering default:
    /// - `target_tolerance`: `1e-3` (representative engineering tolerance in metres)
    /// - `yield_stress`: `None` (no yield-proximity auto-refinement)
    /// - `near_boundary_pct`: `0.10` (10% of yield stress)
    /// - `max_refinements`: `5` (up to 5 refinement passes)
    fn default() -> Self {
        Self {
            target_tolerance: 1e-3,
            yield_stress: None,
            near_boundary_pct: 0.10,
            max_refinements: 5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_elastic_result_round_trips_through_clone_and_eq() {
        let original = PartialElasticResult {
            displacement: vec![1.0, -2.0],
            stress: vec![100e6, -50e6],
            max_von_mises: 100e6,
            converged: true,
            iterations: 7,
        };
        let cloned = original.clone();
        assert_eq!(original, cloned, "PartialElasticResult must round-trip through Clone+PartialEq");
        assert_eq!(cloned.displacement, vec![1.0, -2.0]);
        assert_eq!(cloned.stress, vec![100e6, -50e6]);
        assert_eq!(cloned.max_von_mises, 100e6);
        assert!(cloned.converged);
        assert_eq!(cloned.iterations, 7);
    }

    #[test]
    fn progressive_options_default_has_sane_values() {
        let opts = ProgressiveOptions::default();
        assert!(opts.target_tolerance > 0.0, "target_tolerance must be positive");
        assert!(opts.max_refinements > 0, "max_refinements must be > 0");
        assert!(
            opts.near_boundary_pct > 0.0 && opts.near_boundary_pct < 1.0,
            "near_boundary_pct must be in (0, 1), got {}",
            opts.near_boundary_pct
        );
        assert!(opts.yield_stress.is_none(), "yield_stress default must be None");
    }
}
