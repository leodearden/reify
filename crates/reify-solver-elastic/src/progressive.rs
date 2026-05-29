//! Progressive-solve framework for the linear-elastostatic FEA kernel.
//!
//! PRD reference: `docs/prds/v0_3/structural-analysis-fea.md` task #15.
//!
//! This module supplies the **scheduling/policy primitives** that the engine
//! integration (PRD task #16) and the auto-resolve loop will compose:
//!
//! - [`PassTuning`] — `(mesh_tol, cg_tol)` pair for a single solve pass.
//! - [`coarse_pass_tuning`] — derive the fast first-pass tuning (`tol×4`, CG `1e-3`).
//! - [`refinement_pass_tuning`] — derive per-refinement-level tuning (halve mesh, ÷10 CG per level).
//! - [`near_constraint_boundary`] — auto-refine trigger: `max_von_mises` within
//!   `near_boundary_pct` of `yield_stress`.
//! - [`should_refine`] — decision oracle: returns [`AdvanceDecision::Continue`] or
//!   [`AdvanceDecision::Terminate`] given budget, demand, and auto-detect signals.

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

/// Mesh and CG tolerance pair for a single solve pass.
///
/// PRD task #15: "Coarse pass: mesh at `tol × 4`, CG tolerance `1e-3`.
/// Each refinement halves mesh element size and tightens CG tolerance by 10×."
///
/// `mesh_tol` feeds directly into the Gmsh mesh-from-B-rep pipeline (PRD
/// task #17) and `cg_tol` maps to [`crate::CgSolverOptions::tolerance`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PassTuning {
    /// Target mesh element size tolerance (same units as geometry, typically metres).
    pub mesh_tol: f64,
    /// CG solver convergence tolerance (relative residual).
    pub cg_tol: f64,
}

/// Derive the tuning for refinement level `level`.
///
/// Per PRD task #15: "Each refinement halves mesh element size and tightens
/// CG tolerance by 10×."
///
/// Formulas:
/// - `mesh_tol = target_tolerance × 4.0 × 0.5^level`
/// - `cg_tol = 1e-3 × 0.1^level`
///
/// # Level correspondence
///
/// `level = 0` gives the same result as [`coarse_pass_tuning`] — both are
/// closed-form at level 0, avoiding a special case. Use [`coarse_pass_tuning`]
/// at level 0 for readability; use this function for levels ≥ 1.
pub fn refinement_pass_tuning(opts: &ProgressiveOptions, level: usize) -> PassTuning {
    PassTuning {
        mesh_tol: opts.target_tolerance * 4.0 * 0.5_f64.powi(level as i32),
        cg_tol: 1e-3 * 0.1_f64.powi(level as i32),
    }
}

/// Derive the coarse-pass tuning from `opts`.
///
/// Per PRD task #15: "Coarse pass: mesh at `tol × 4` (4× coarser than
/// requested), CG tolerance `1e-3` (loose)."
///
/// Equivalent to [`refinement_pass_tuning`]`(opts, 0)`.
pub fn coarse_pass_tuning(opts: &ProgressiveOptions) -> PassTuning {
    PassTuning {
        mesh_tol: opts.target_tolerance * 4.0,
        cg_tol: 1e-3,
    }
}

/// A snapshot of a single FEA solve at a given refinement level.
///
/// Field names are intended to mirror `reify_eval::ElasticResult` (minus
/// `solve_time_ms`, which is a cache-eviction metric rather than a solver
/// output) to simplify conversion in the cache layer when PRD task #16
/// wires the engine integration.  No compile-time adapter or `From` impl
/// exists yet — field correspondence is documented by convention until
/// task #16 lands and can enforce the invariant with an explicit impl.
///
/// Defined locally in this crate to avoid a `reify-solver-elastic →
/// reify-eval` dependency edge (the reverse edge already exists per
/// `persistent_cache.rs`).
#[derive(Debug, Clone, PartialEq)]
pub struct PartialElasticResult {
    /// Nodal displacement vector (3 DOFs per node, flat).
    pub displacement: Vec<f64>,
    /// Nodal or element stress components (flat, ordering determined by assembler).
    pub stress: Vec<f64>,
    /// Maximum von Mises stress across the mesh (Pa). Used by the
    /// [`near_constraint_boundary`] auto-refine trigger.
    pub max_von_mises: f64,
    /// Whether the CG inner solve converged within its tolerance.
    pub converged: bool,
    /// Number of CG iterations taken to reach convergence (or hit the limit).
    pub iterations: u32,
}

/// Returns `true` if `result.max_von_mises` is within `opts.near_boundary_pct`
/// of `opts.yield_stress`, triggering auto-refinement.
///
/// Specifically: `result.max_von_mises >= (1.0 − opts.near_boundary_pct) × yield_stress`.
///
/// Returns `false` unconditionally when `opts.yield_stress` is `None`
/// (yield-proximity auto-refinement is disabled).
///
/// # Non-finite `max_von_mises`
///
/// If `result.max_von_mises` is NaN or ±Inf (e.g. from a diverged solve),
/// this function returns `false` — it treats a non-finite stress as "no
/// refinement triggered" rather than silently propagating a NaN comparison.
/// Callers should check `result.converged` before acting on the result; a
/// `false` return here does **not** distinguish a healthy "below threshold"
/// from a "solver produced garbage" case.
///
/// # Examples
///
/// ```
/// use reify_solver_elastic::progressive::{near_constraint_boundary, PartialElasticResult, ProgressiveOptions};
///
/// fn make_r(max_von_mises: f64) -> PartialElasticResult {
///     PartialElasticResult { displacement: vec![], stress: vec![], max_von_mises, converged: false, iterations: 0 }
/// }
///
/// // No yield stress → always false regardless of von Mises value.
/// let opts_none = ProgressiveOptions { yield_stress: None, ..Default::default() };
/// assert!(!near_constraint_boundary(&make_r(1e30), &opts_none));
///
/// // yield_stress = 200 MPa, near_boundary_pct = 0.10 → threshold = 180 MPa.
/// let opts = ProgressiveOptions { yield_stress: Some(200e6), near_boundary_pct: 0.10, ..Default::default() };
///
/// // Well below threshold (100 MPa < 180 MPa) → false.
/// assert!(!near_constraint_boundary(&make_r(100e6), &opts));
///
/// // Exactly at threshold (180 MPa >= 180 MPa) → true (`>=` semantics).
/// assert!(near_constraint_boundary(&make_r(180e6), &opts));
///
/// // Above threshold (195 MPa >= 180 MPa) → true.
/// assert!(near_constraint_boundary(&make_r(195e6), &opts));
/// ```
pub fn near_constraint_boundary(result: &PartialElasticResult, opts: &ProgressiveOptions) -> bool {
    debug_assert!(
        opts.target_tolerance > 0.0,
        "target_tolerance must be positive, got {}",
        opts.target_tolerance
    );
    debug_assert!(
        opts.near_boundary_pct > 0.0 && opts.near_boundary_pct < 1.0,
        "near_boundary_pct must be in (0, 1), got {}",
        opts.near_boundary_pct
    );
    // Non-finite stress (NaN / ±Inf) is treated as "no refinement needed"
    // rather than relying on the undefined behaviour of a NaN comparison.
    if !result.max_von_mises.is_finite() {
        return false;
    }
    match opts.yield_stress {
        Some(yield_stress) => result.max_von_mises >= (1.0 - opts.near_boundary_pct) * yield_stress,
        None => false,
    }
}

/// Demand signal passed by a downstream consumer to [`should_refine`].
///
/// Distinguishes "caller wants more accuracy" from "no explicit request".
/// The auto-refine trigger ([`near_constraint_boundary`]) is checked
/// independently — both triggers are evaluated in [`should_refine`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RefinementDemand {
    /// No explicit accuracy request from the caller; only the auto-trigger
    /// may initiate a further refinement pass.
    None,
    /// Caller explicitly requests a more-accurate pass (e.g., user drags
    /// an accuracy slider or the downstream engine asks for a re-solve).
    More,
}

/// Reason why [`should_refine`] terminated the refinement schedule.
///
/// Surfaced in `AdvanceDecision::Terminate` so engine integration
/// (PRD task #16) can emit informative diagnostics without re-deriving
/// the cause from state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TerminationReason {
    /// `current_level >= opts.max_refinements`: refinement budget exhausted.
    BudgetExhausted,
    /// Neither [`RefinementDemand::More`] nor the [`near_constraint_boundary`]
    /// auto-trigger fired; no reason to refine further.
    NoRefinementRequested,
}

/// Decision returned by [`should_refine`].
///
/// `Continue` carries the [`PassTuning`] for the next refinement level;
/// `Terminate` carries the [`TerminationReason`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AdvanceDecision {
    /// Proceed with the next refinement pass at the given tuning.
    Continue(PassTuning),
    /// Stop refinement for the given reason.
    Terminate(TerminationReason),
}

/// Decide whether to issue another refinement pass.
///
/// Decision rule (checked in order):
/// 1. If `current_level >= opts.max_refinements` → `Terminate(BudgetExhausted)`.
/// 2. If `demand == More` OR [`near_constraint_boundary`] fires →
///    `Continue(refinement_pass_tuning(opts, current_level + 1))`.
/// 3. Otherwise → `Terminate(NoRefinementRequested)`.
///
/// The budget check takes priority so callers with `demand == More` cannot
/// exceed the configured refinement budget.
///
/// PRD reference: `docs/prds/v0_3/structural-analysis-fea.md` task #15.
pub fn should_refine(
    opts: &ProgressiveOptions,
    current_level: usize,
    last_result: &PartialElasticResult,
    demand: RefinementDemand,
) -> AdvanceDecision {
    debug_assert!(
        opts.max_refinements > 0,
        "max_refinements must be > 0, got {}",
        opts.max_refinements
    );
    debug_assert!(
        opts.target_tolerance > 0.0,
        "target_tolerance must be positive, got {}",
        opts.target_tolerance
    );
    use AdvanceDecision::*;
    use TerminationReason::*;
    if current_level >= opts.max_refinements {
        return Terminate(BudgetExhausted);
    }
    if matches!(demand, RefinementDemand::More) || near_constraint_boundary(last_result, opts) {
        Continue(refinement_pass_tuning(opts, current_level + 1))
    } else {
        Terminate(NoRefinementRequested)
    }
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

    fn make_result(max_von_mises: f64) -> PartialElasticResult {
        PartialElasticResult {
            displacement: vec![],
            stress: vec![],
            max_von_mises,
            converged: false,
            iterations: 0,
        }
    }

    #[test]
    fn near_constraint_boundary_triggers_at_and_above_threshold_false_well_below() {
        // yield_stress = 200e6, near_boundary_pct = 0.10 → threshold = 180e6
        let opts = ProgressiveOptions {
            yield_stress: Some(200e6),
            near_boundary_pct: 0.10,
            ..Default::default()
        };
        // (a) well below threshold: max_von_mises = 100e6 → false
        assert!(
            !near_constraint_boundary(&make_result(100e6), &opts),
            "100 MPa is well below threshold (180 MPa), must return false"
        );
        // (b) exactly at threshold: max_von_mises = 180e6 → true (>= semantics)
        assert!(
            near_constraint_boundary(&make_result(180e6), &opts),
            "180 MPa is exactly at threshold, must return true (>= semantics)"
        );
        // (c) above threshold: max_von_mises = 195e6 → true
        assert!(
            near_constraint_boundary(&make_result(195e6), &opts),
            "195 MPa is above threshold (180 MPa), must return true"
        );
    }

    #[test]
    fn near_constraint_boundary_non_finite_max_von_mises_returns_false() {
        // NaN and ±Inf max_von_mises must return false (not trigger spurious refinement).
        let opts = ProgressiveOptions {
            yield_stress: Some(200e6),
            near_boundary_pct: 0.10,
            ..Default::default()
        };
        assert!(
            !near_constraint_boundary(&make_result(f64::NAN), &opts),
            "NaN max_von_mises must return false"
        );
        assert!(
            !near_constraint_boundary(&make_result(f64::INFINITY), &opts),
            "+Inf max_von_mises must return false"
        );
        assert!(
            !near_constraint_boundary(&make_result(f64::NEG_INFINITY), &opts),
            "-Inf max_von_mises must return false"
        );
    }

    #[test]
    fn near_constraint_boundary_returns_false_when_yield_stress_is_none() {
        let opts = ProgressiveOptions {
            yield_stress: None,
            ..Default::default()
        };
        let result = make_result(1e30);
        assert!(
            !near_constraint_boundary(&result, &opts),
            "near_constraint_boundary must return false when yield_stress is None"
        );
    }

    #[test]
    fn refinement_pass_tuning_halves_mesh_and_tenths_cg_per_level() {
        let opts = ProgressiveOptions {
            target_tolerance: 0.05,
            ..Default::default()
        };
        // level=1: mesh_tol = 0.05 × 4 × 0.5 = 0.10, cg_tol = 1e-3 × 0.1 = 1e-4
        let pt1 = refinement_pass_tuning(&opts, 1);
        assert!(
            (pt1.mesh_tol - 0.10).abs() < 1e-15,
            "level=1 mesh_tol={}",
            pt1.mesh_tol
        );
        assert!(
            (pt1.cg_tol - 1e-4).abs() < 1e-15,
            "level=1 cg_tol={}",
            pt1.cg_tol
        );
        // level=2: mesh_tol = 0.05 × 4 × 0.25 = 0.05, cg_tol = 1e-5
        let pt2 = refinement_pass_tuning(&opts, 2);
        assert!(
            (pt2.mesh_tol - 0.05).abs() < 1e-15,
            "level=2 mesh_tol={}",
            pt2.mesh_tol
        );
        assert!(
            (pt2.cg_tol - 1e-5).abs() < 1e-15,
            "level=2 cg_tol={}",
            pt2.cg_tol
        );
        // level=3: mesh_tol = 0.05 × 4 × 0.125 = 0.025, cg_tol = 1e-6
        let pt3 = refinement_pass_tuning(&opts, 3);
        assert!(
            (pt3.mesh_tol - 0.025).abs() < 1e-15,
            "level=3 mesh_tol={}",
            pt3.mesh_tol
        );
        assert!(
            (pt3.cg_tol - 1e-6).abs() < 1e-15,
            "level=3 cg_tol={}",
            pt3.cg_tol
        );
    }

    #[test]
    fn coarse_pass_tuning_returns_4x_mesh_and_loose_cg() {
        let opts = ProgressiveOptions {
            target_tolerance: 0.05,
            ..Default::default()
        };
        let pt = coarse_pass_tuning(&opts);
        assert_eq!(pt.mesh_tol, 0.20, "mesh_tol must be target_tolerance × 4");
        assert_eq!(pt.cg_tol, 1e-3, "cg_tol must be 1e-3 for coarse pass");

        // Different tolerance — defeats hardcoded-constant returns.
        let opts2 = ProgressiveOptions {
            target_tolerance: 0.01,
            ..Default::default()
        };
        let pt2 = coarse_pass_tuning(&opts2);
        assert!(
            (pt2.mesh_tol - 0.04).abs() < 1e-15,
            "mesh_tol for 0.01 must be 0.04, got {}",
            pt2.mesh_tol
        );
        assert_eq!(pt2.cg_tol, 1e-3);
    }

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
        assert_eq!(
            original, cloned,
            "PartialElasticResult must round-trip through Clone+PartialEq"
        );
        assert_eq!(cloned.displacement, vec![1.0, -2.0]);
        assert_eq!(cloned.stress, vec![100e6, -50e6]);
        assert_eq!(cloned.max_von_mises, 100e6);
        assert!(cloned.converged);
        assert_eq!(cloned.iterations, 7);
    }

    #[test]
    fn should_refine_continues_on_explicit_demand_when_budget_remains() {
        // current_level=1, max_refinements=5, demand=More, yield_stress=None
        // → auto-trigger cannot fire; demand alone must drive Continue.
        let opts = ProgressiveOptions {
            max_refinements: 5,
            yield_stress: None,
            target_tolerance: 0.05,
            ..Default::default()
        };
        let result = make_result(0.0);
        let expected = AdvanceDecision::Continue(refinement_pass_tuning(&opts, 2));
        assert_eq!(
            should_refine(&opts, 1, &result, RefinementDemand::More),
            expected,
            "demand=More within budget must yield Continue at next level"
        );
    }

    #[test]
    fn should_refine_continues_on_near_boundary_auto_trigger() {
        // demand=None, but max_von_mises is above the near-boundary threshold.
        let opts = ProgressiveOptions {
            max_refinements: 5,
            yield_stress: Some(200e6),
            near_boundary_pct: 0.10,
            target_tolerance: 0.05,
        };
        // 195 MPa >= (1 - 0.10) * 200 MPa = 180 MPa → near_constraint_boundary = true.
        let result = make_result(195e6);
        let current_level = 2;
        let expected = AdvanceDecision::Continue(refinement_pass_tuning(&opts, current_level + 1));
        assert_eq!(
            should_refine(&opts, current_level, &result, RefinementDemand::None),
            expected,
            "near-boundary auto-trigger must cause Continue when demand=None"
        );
    }

    #[test]
    fn should_refine_terminates_with_no_request_when_neither_trigger_fires() {
        // demand=None, yield_stress=None → neither trigger fires → NoRefinementRequested.
        let opts = ProgressiveOptions {
            max_refinements: 5,
            yield_stress: None,
            ..Default::default()
        };
        let result = make_result(0.0);
        assert_eq!(
            should_refine(&opts, 1, &result, RefinementDemand::None),
            AdvanceDecision::Terminate(TerminationReason::NoRefinementRequested),
            "no demand + no auto-trigger must yield NoRefinementRequested"
        );
    }

    #[test]
    fn should_refine_with_zero_max_refinements_terminates_budget_exhausted() {
        // max_refinements == 0 is a legitimate "coarse-pass-only" configuration:
        // the caller wants exactly one coarse pass with no refinements.
        // should_refine must return Terminate(BudgetExhausted) immediately rather
        // than panicking — the debug_assert!(opts.max_refinements > 0) footgun
        // made this crash in debug/test builds while release returned the correct
        // value.
        let opts = ProgressiveOptions {
            max_refinements: 0,
            ..Default::default()
        };
        assert_eq!(
            should_refine(&opts, 0, &make_result(0.0), RefinementDemand::More),
            AdvanceDecision::Terminate(TerminationReason::BudgetExhausted),
            "max_refinements==0 (coarse-only config) must yield BudgetExhausted, not panic"
        );
    }

    #[test]
    fn should_refine_terminates_when_budget_exhausted() {
        // max_refinements = 3, current_level = 3 → budget exhausted, even with More demand.
        let opts = ProgressiveOptions {
            max_refinements: 3,
            ..Default::default()
        };
        let result = make_result(0.0);
        assert_eq!(
            should_refine(&opts, 3, &result, RefinementDemand::More),
            AdvanceDecision::Terminate(TerminationReason::BudgetExhausted),
            "current_level >= max_refinements must always yield BudgetExhausted"
        );
    }

    #[test]
    fn should_refine_budget_exhausted_overrides_auto_trigger() {
        // The auto-trigger fires (max_von_mises above threshold), but the
        // budget is already exhausted.  Budget check must take priority —
        // neither demand nor auto-detect can exceed max_refinements.
        let opts = ProgressiveOptions {
            max_refinements: 3,
            yield_stress: Some(200e6),
            near_boundary_pct: 0.10, // threshold = 180 MPa
            target_tolerance: 0.05,
        };
        // 195 MPa >= 180 MPa → near_constraint_boundary would return true if
        // budget were not exhausted.
        let result = make_result(195e6);
        assert_eq!(
            should_refine(&opts, 3, &result, RefinementDemand::None),
            AdvanceDecision::Terminate(TerminationReason::BudgetExhausted),
            "budget exhausted must override auto-trigger (near_constraint_boundary)"
        );
    }

    #[test]
    fn progressive_options_default_has_sane_values() {
        let opts = ProgressiveOptions::default();
        assert!(
            opts.target_tolerance > 0.0,
            "target_tolerance must be positive"
        );
        assert!(opts.max_refinements > 0, "max_refinements must be > 0");
        assert!(
            opts.near_boundary_pct > 0.0 && opts.near_boundary_pct < 1.0,
            "near_boundary_pct must be in (0, 1), got {}",
            opts.near_boundary_pct
        );
        assert!(
            opts.yield_stress.is_none(),
            "yield_stress default must be None"
        );
    }
}
