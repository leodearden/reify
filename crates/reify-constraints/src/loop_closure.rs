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
