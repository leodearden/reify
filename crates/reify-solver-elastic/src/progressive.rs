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

#[cfg(test)]
mod tests {
    use super::*;

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
