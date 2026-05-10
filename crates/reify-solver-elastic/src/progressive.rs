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
/// Field names mirror `reify_eval::ElasticResult` (minus `solve_time_ms`,
/// which is a cache-eviction metric rather than a solver output) so the
/// cache layer (PRD task #16) can convert losslessly without an adapter.
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
    fn near_constraint_boundary_returns_false_when_yield_stress_is_none() {
        let opts = ProgressiveOptions { yield_stress: None, ..Default::default() };
        let result = make_result(1e30);
        assert!(
            !near_constraint_boundary(&result, &opts),
            "near_constraint_boundary must return false when yield_stress is None"
        );
    }

    #[test]
    fn refinement_pass_tuning_halves_mesh_and_tenths_cg_per_level() {
        let opts = ProgressiveOptions { target_tolerance: 0.05, ..Default::default() };
        // level=1: mesh_tol = 0.05 × 4 × 0.5 = 0.10, cg_tol = 1e-3 × 0.1 = 1e-4
        let pt1 = refinement_pass_tuning(&opts, 1);
        assert!((pt1.mesh_tol - 0.10).abs() < 1e-15, "level=1 mesh_tol={}", pt1.mesh_tol);
        assert!((pt1.cg_tol - 1e-4).abs() < 1e-15, "level=1 cg_tol={}", pt1.cg_tol);
        // level=2: mesh_tol = 0.05 × 4 × 0.25 = 0.05, cg_tol = 1e-5
        let pt2 = refinement_pass_tuning(&opts, 2);
        assert!((pt2.mesh_tol - 0.05).abs() < 1e-15, "level=2 mesh_tol={}", pt2.mesh_tol);
        assert!((pt2.cg_tol - 1e-5).abs() < 1e-15, "level=2 cg_tol={}", pt2.cg_tol);
        // level=3: mesh_tol = 0.05 × 4 × 0.125 = 0.025, cg_tol = 1e-6
        let pt3 = refinement_pass_tuning(&opts, 3);
        assert!((pt3.mesh_tol - 0.025).abs() < 1e-15, "level=3 mesh_tol={}", pt3.mesh_tol);
        assert!((pt3.cg_tol - 1e-6).abs() < 1e-15, "level=3 cg_tol={}", pt3.cg_tol);
    }

    #[test]
    fn coarse_pass_tuning_returns_4x_mesh_and_loose_cg() {
        let opts = ProgressiveOptions { target_tolerance: 0.05, ..Default::default() };
        let pt = coarse_pass_tuning(&opts);
        assert_eq!(pt.mesh_tol, 0.20, "mesh_tol must be target_tolerance × 4");
        assert_eq!(pt.cg_tol, 1e-3, "cg_tol must be 1e-3 for coarse pass");

        // Different tolerance — defeats hardcoded-constant returns.
        let opts2 = ProgressiveOptions { target_tolerance: 0.01, ..Default::default() };
        let pt2 = coarse_pass_tuning(&opts2);
        assert!((pt2.mesh_tol - 0.04).abs() < 1e-15, "mesh_tol for 0.01 must be 0.04, got {}", pt2.mesh_tol);
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
