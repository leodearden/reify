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

/// Convergence and iteration knobs for [`newton_solve`] / [`solve_loop_closure`].
///
/// PRD defaults — `tol_pos_m = 1e-6` (1 µm position), `tol_rot_rad = 1e-6`
/// (1 µrad rotation), `max_iters = 50`.  See
/// `docs/prds/v0_2/kinematic-constraints.md` §"Loop-closure solver".
#[derive(Debug, Clone)]
pub struct NewtonConfig {
    /// Linear-residual tolerance for convergence (metres).
    pub tol_pos_m: f64,
    /// Angular-residual tolerance for convergence (radians).
    pub tol_rot_rad: f64,
    /// Maximum Newton iterations before giving up.
    pub max_iters: usize,
}

impl Default for NewtonConfig {
    fn default() -> Self {
        Self {
            tol_pos_m: 1e-6,
            tol_rot_rad: 1e-6,
            max_iters: 50,
        }
    }
}

/// Strategy for picking the initial free-variable values for a loop-closure
/// snapshot solve.
///
/// `WarmStart(v)` uses the supplied vector directly (typical: previous
/// snapshot's converged values).  `Midpoint` queries each free joint's range
/// midpoint via [`reify_stdlib::loop_closure::joint_range_midpoint`].
#[derive(Debug, Clone)]
pub enum StartStrategy {
    /// Re-use a prior solution.  Vector length must match the free-variable count.
    WarmStart(Vec<f64>),
    /// Initialise from each free joint's range midpoint.
    Midpoint,
}

/// Result of a Newton solve.
///
/// `Converged` — both linear and angular residual sub-norms below their
/// configured tolerances.  `NotConverged` — `max_iters` exhausted without
/// hitting tolerance.  `Singular` — the Gauss-Newton normal-equations matrix
/// hit the min-pivot threshold (rank-deficient Jacobian); reported separately
/// so callers can emit the PRD's `W_KINEMATIC_SINGULARITY` warning class.
#[derive(Debug, Clone)]
pub enum NewtonOutcome {
    /// Solver reached tolerance.
    Converged {
        /// Free-variable values at convergence.
        x: Vec<f64>,
        /// Number of Newton iterations taken.
        iters: usize,
        /// Combined residual norm (sqrt(linear² + angular²)) at convergence.
        residual_norm: f64,
    },
    /// Solver hit `max_iters` without reaching tolerance.
    NotConverged {
        /// Free-variable values at the last iteration.
        x: Vec<f64>,
        /// Combined residual norm at the last iteration.
        residual_norm: f64,
    },
    /// Solver detected a rank-deficient Jacobian (min-pivot < 1e-12).
    Singular {
        /// Free-variable values at the iteration where singularity was detected.
        x: Vec<f64>,
        /// Number of completed iterations before singularity.
        iters: usize,
    },
}

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
