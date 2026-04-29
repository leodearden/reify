//! Loop-closure Newton solver, configuration types, and convenience wrappers.
//!
//! Generic Gauss-Newton solver and configuration for closing kinematic
//! loops: callers supply a residual+jacobian closure, the solver returns a
//! `NewtonOutcome` describing convergence, divergence, or a singular Jacobian.
//!
//! Public API surface (filled in by the TDD steps that follow):
//!   * `NewtonConfig { tol_pos_m, tol_rot_rad, max_iters }` — defaults 1µm / 1µrad / 50.
//!   * `StartStrategy::{WarmStart(Vec<f64>), Midpoint}`
//!   * `NewtonOutcome::{Converged, NotConverged, Singular}`
//!   * `newton_solve<F>(x0, residual_jac, &config) -> NewtonOutcome`
//!     where `F: FnMut(&[f64]) -> Option<(Vec<f64>, Vec<Vec<f64>>)>` returns
//!     `(residual, jacobian_columns)`.
//!   * `solve_loop_closure(chain_a, vals_a, chain_b, vals_b_initial, free_b,
//!                        strategy, config) -> NewtonOutcome` — convenience
//!     wrapper that builds the residual+jacobian closure from stdlib helpers.
//!
//! Convention: a Newton iteration is "converged" iff the per-iteration residual's
//! linear sub-norm is below `config.tol_pos_m` AND its angular sub-norm is below
//! `config.tol_rot_rad`.  The two tolerances are honoured independently, matching
//! the PRD's `1µm position / 1µrad rotation` defaults.
//!
//! See `docs/prds/v0_2/kinematic-constraints.md` §"Loop-closure solver" for the
//! design rationale.

#[cfg(test)]
mod tests {
    use super::*;

    // ── Public type API surface (step-11) ──────────────────────────────

    #[test]
    fn newton_config_default_values() {
        let cfg = NewtonConfig::default();
        assert_eq!(cfg.tol_pos_m, 1e-6);
        assert_eq!(cfg.tol_rot_rad, 1e-6);
        assert_eq!(cfg.max_iters, 50);
    }

    #[test]
    fn newton_config_constructible_with_custom_values() {
        let cfg = NewtonConfig {
            tol_pos_m: 1e-3,
            tol_rot_rad: 1e-4,
            max_iters: 100,
        };
        assert_eq!(cfg.tol_pos_m, 1e-3);
        assert_eq!(cfg.tol_rot_rad, 1e-4);
        assert_eq!(cfg.max_iters, 100);
    }

    #[test]
    fn start_strategy_variants_constructible() {
        let _ws = StartStrategy::WarmStart(vec![0.1, 0.2]);
        let _mp = StartStrategy::Midpoint;
    }

    #[test]
    fn newton_outcome_variants_constructible() {
        let _conv = NewtonOutcome::Converged {
            x: vec![1.0, 2.0],
            iters: 3,
            residual_norm: 1e-9,
        };
        let _notc = NewtonOutcome::NotConverged {
            x: vec![1.0],
            residual_norm: 0.5,
        };
        let _sing = NewtonOutcome::Singular {
            x: vec![1.0],
            iters: 2,
        };
    }

    #[test]
    fn types_implement_debug_and_clone() {
        let cfg = NewtonConfig::default();
        let _: NewtonConfig = cfg.clone();
        let _ = format!("{cfg:?}");

        let s = StartStrategy::Midpoint;
        let _: StartStrategy = s.clone();
        let _ = format!("{s:?}");

        let o = NewtonOutcome::NotConverged {
            x: vec![],
            residual_norm: 0.0,
        };
        let _: NewtonOutcome = o.clone();
        let _ = format!("{o:?}");
    }
}
