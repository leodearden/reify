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

/// Pivot threshold below which the LDLᵀ factor is treated as singular.
const SINGULARITY_PIVOT_EPS: f64 = 1e-12;

/// Compute split position / rotation residual sub-norms over a stacked twist
/// residual.
///
/// The residual is laid out as `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` per loop
/// (mirroring the `transform_log` / `joint_jacobian` Map shape).  We aggregate
/// across loops with L2-norm so a multi-loop residual collapses to two
/// scalars: `(angular_norm, linear_norm)`.
fn position_rotation_norms(r: &[f64]) -> (f64, f64) {
    let mut ang2 = 0.0;
    let mut lin2 = 0.0;
    for chunk in r.chunks(6) {
        // chunk may be shorter than 6 only on malformed input — guard so we
        // don't panic in release on caller errors.
        for (i, v) in chunk.iter().enumerate() {
            if i < 3 {
                ang2 += v * v;
            } else {
                lin2 += v * v;
            }
        }
    }
    (ang2.sqrt(), lin2.sqrt())
}

/// Solve `A · x = b` for `x` where `A` is a small dense symmetric (semi-)PD
/// matrix supplied as `n×n` row-major nested `Vec`, using inlined LDLᵀ
/// factorisation.
///
/// Returns `None` if the min absolute pivot drops below
/// [`SINGULARITY_PIVOT_EPS`] — that is the signal that the Gauss-Newton
/// normal-equations matrix `JᵀJ` is rank-deficient.
fn solve_normal_equations(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = a.len();
    if n == 0 {
        return Some(vec![]);
    }
    if a.iter().any(|row| row.len() != n) || b.len() != n {
        return None;
    }
    // LDLᵀ: a is overwritten so that the strict-lower triangle holds L
    // (with implicit unit diagonal) and the diagonal holds D.
    for j in 0..n {
        // Compute D[j,j] = a[j,j] - Σ_{k<j} L[j,k]^2 * D[k,k]
        let mut d_jj = a[j][j];
        for k in 0..j {
            d_jj -= a[j][k] * a[j][k] * a[k][k];
        }
        if d_jj.abs() < SINGULARITY_PIVOT_EPS {
            return None;
        }
        a[j][j] = d_jj;
        // Compute L[i,j] for i > j: a[i,j] = (a[i,j] - Σ_{k<j} L[i,k]*L[j,k]*D[k,k]) / D[j,j]
        for i in (j + 1)..n {
            let mut s = a[i][j];
            for k in 0..j {
                s -= a[i][k] * a[j][k] * a[k][k];
            }
            a[i][j] = s / d_jj;
        }
    }
    // Forward solve L · y = b (L unit-lower).
    for i in 0..n {
        let mut s = b[i];
        for k in 0..i {
            s -= a[i][k] * b[k];
        }
        b[i] = s;
    }
    // Diagonal solve D · z = y.
    for i in 0..n {
        b[i] /= a[i][i];
    }
    // Back solve Lᵀ · x = z.
    for i in (0..n).rev() {
        let mut s = b[i];
        for k in (i + 1)..n {
            s -= a[k][i] * b[k];
        }
        b[i] = s;
    }
    Some(b)
}

/// Generic Gauss-Newton solver for closure-driven residual+jacobian problems.
///
/// `residual_jac(&x)` must return `(r, j_cols)` where `r` is the residual
/// vector (a stacked sequence of 6-element twists, `[ω; v]` per loop) and
/// `j_cols` is `Vec<Vec<f64>>` of length `x.len()` — one column per free
/// variable, each a `r.len()`-element twist column.  Returning `None` aborts
/// the solve as `NewtonOutcome::NotConverged` with `residual_norm` set to
/// `f64::INFINITY` (signal that the closure could not produce a residual,
/// e.g. a chain returned `Value::Undef`).
///
/// Convergence rule: per [`NewtonConfig::tol_pos_m`] / [`NewtonConfig::tol_rot_rad`],
/// we converge iff `linear_norm < tol_pos_m` AND `angular_norm < tol_rot_rad`.
/// Singularity rule: per the inlined LDLᵀ pivot check (1e-12 threshold), any
/// rank-deficient `JᵀJ` returns `NewtonOutcome::Singular`.
pub fn newton_solve<F>(
    x0: Vec<f64>,
    mut residual_jac: F,
    config: &NewtonConfig,
) -> NewtonOutcome
where
    F: FnMut(&[f64]) -> Option<(Vec<f64>, Vec<Vec<f64>>)>,
{
    let mut x = x0;
    let n = x.len();
    let mut last_residual_norm = f64::INFINITY;

    for iter in 0..config.max_iters {
        let (r, j_cols) = match residual_jac(&x) {
            Some(rj) => rj,
            None => {
                return NewtonOutcome::NotConverged {
                    x,
                    residual_norm: f64::INFINITY,
                };
            }
        };
        let (ang_norm, lin_norm) = position_rotation_norms(&r);
        last_residual_norm = (ang_norm * ang_norm + lin_norm * lin_norm).sqrt();
        if lin_norm < config.tol_pos_m && ang_norm < config.tol_rot_rad {
            return NewtonOutcome::Converged {
                x,
                iters: iter,
                residual_norm: last_residual_norm,
            };
        }
        // Build JᵀJ (n×n) and Jᵀr (n).
        if j_cols.len() != n {
            return NewtonOutcome::NotConverged {
                x,
                residual_norm: last_residual_norm,
            };
        }
        if j_cols.iter().any(|c| c.len() != r.len()) {
            return NewtonOutcome::NotConverged {
                x,
                residual_norm: last_residual_norm,
            };
        }
        let mut jtj: Vec<Vec<f64>> = vec![vec![0.0; n]; n];
        let mut jtr: Vec<f64> = vec![0.0; n];
        for i in 0..n {
            for j in 0..n {
                let mut s = 0.0;
                for (a, b) in j_cols[i].iter().zip(j_cols[j].iter()) {
                    s += a * b;
                }
                jtj[i][j] = s;
            }
            let mut s = 0.0;
            for (a, b) in j_cols[i].iter().zip(r.iter()) {
                s += a * b;
            }
            jtr[i] = s;
        }
        // Solve JᵀJ · δx = -Jᵀr.
        let neg_jtr: Vec<f64> = jtr.iter().map(|v| -v).collect();
        let dx = match solve_normal_equations(jtj, neg_jtr) {
            Some(d) => d,
            None => {
                return NewtonOutcome::Singular { x, iters: iter };
            }
        };
        for i in 0..n {
            x[i] += dx[i];
        }
    }

    // After max_iters: re-evaluate the residual at the final x so the
    // reported norm reflects the final iterate (not the last pre-step
    // residual).  If max_iters == 0, last_residual_norm is INFINITY; we
    // need to evaluate once at x0 to honour the user contract.
    if config.max_iters == 0 {
        if let Some((r, _)) = residual_jac(&x) {
            let (ang_norm, lin_norm) = position_rotation_norms(&r);
            last_residual_norm = (ang_norm * ang_norm + lin_norm * lin_norm).sqrt();
        }
    }
    NewtonOutcome::NotConverged {
        x,
        residual_norm: last_residual_norm,
    }
}

/// Single-loop convenience wrapper: drive `chain_b`'s free variables to
/// satisfy the loop-closure residual against the (fixed) `chain_a`.
///
/// `chain_a` / `vals_a` is the reference side (held fixed for the solve).
/// `chain_b` / `vals_b_initial` is the free side; the indices in `free_b`
/// select which entries of `vals_b_initial` the solver moves.  `strategy`
/// picks the initial guess: [`StartStrategy::WarmStart`] uses the supplied
/// vector directly (must match `free_b.len()`); [`StartStrategy::Midpoint`]
/// queries each free joint's `joint_range_midpoint` from `chain_b`.
///
/// Internally builds a residual+jacobian closure that calls
/// [`reify_stdlib::loop_closure::loop_residual_twist`] and
/// [`reify_stdlib::loop_closure::chain_jacobian_fd`], then dispatches to
/// [`newton_solve`].
///
/// Multi-loop is future work (the [`newton_solve`] core is generic — callers
/// can stack residuals/columns from multiple loops).
pub fn solve_loop_closure(
    chain_a: &[reify_types::Value],
    vals_a: &[f64],
    chain_b: &[reify_types::Value],
    vals_b_initial: &[f64],
    free_b: &[usize],
    strategy: &StartStrategy,
    config: &NewtonConfig,
) -> NewtonOutcome {
    // Resolve initial x0 from the strategy.
    let x0: Vec<f64> = match strategy {
        StartStrategy::WarmStart(v) => {
            if v.len() != free_b.len() {
                tracing::warn!(
                    "solve_loop_closure: WarmStart length {} != free_b length {}",
                    v.len(),
                    free_b.len()
                );
                return NewtonOutcome::NotConverged {
                    x: vec![],
                    residual_norm: f64::INFINITY,
                };
            }
            v.clone()
        }
        StartStrategy::Midpoint => {
            let mut out = Vec::with_capacity(free_b.len());
            for &i in free_b {
                if i >= chain_b.len() {
                    tracing::warn!(
                        "solve_loop_closure: free_b index {} out of range (chain_b len {})",
                        i,
                        chain_b.len()
                    );
                    return NewtonOutcome::NotConverged {
                        x: vec![],
                        residual_norm: f64::INFINITY,
                    };
                }
                match reify_stdlib::loop_closure::joint_range_midpoint(&chain_b[i]) {
                    Some(m) => out.push(m),
                    None => {
                        tracing::warn!(
                            "solve_loop_closure: joint_range_midpoint returned None for free_b[{i}] — joint missing range or malformed"
                        );
                        return NewtonOutcome::NotConverged {
                            x: vec![],
                            residual_norm: f64::INFINITY,
                        };
                    }
                }
            }
            out
        }
    };

    // Capture inputs for the closure.  The closure is FnMut over an internal
    // scratch buffer for vals_b to avoid reallocating each call.
    let chain_a_vec = chain_a.to_vec();
    let vals_a_vec = vals_a.to_vec();
    let chain_b_vec = chain_b.to_vec();
    let mut vals_b_scratch = vals_b_initial.to_vec();
    let free_b_vec = free_b.to_vec();

    let closure = move |x: &[f64]| -> Option<(Vec<f64>, Vec<Vec<f64>>)> {
        if x.len() != free_b_vec.len() {
            return None;
        }
        // Substitute x into the free entries of vals_b_scratch.
        for (k, &i) in free_b_vec.iter().enumerate() {
            if i >= vals_b_scratch.len() {
                return None;
            }
            vals_b_scratch[i] = x[k];
        }
        let twist = reify_stdlib::loop_closure::loop_residual_twist(
            &chain_a_vec,
            &vals_a_vec,
            &chain_b_vec,
            &vals_b_scratch,
        )?;
        let cols = reify_stdlib::loop_closure::chain_jacobian_fd(
            &chain_b_vec,
            &vals_b_scratch,
            &free_b_vec,
            1e-6,
        )?;
        // Twist is fixed-array [f64; 6]; convert to Vec<f64> for newton_solve.
        let r = twist.to_vec();
        let j_cols: Vec<Vec<f64>> = cols.into_iter().map(|c| c.to_vec()).collect();
        Some((r, j_cols))
    };

    newton_solve(x0, closure, config)
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

    // ── newton_solve tests (step-13) ────────────────────────────────────

    /// Build a residual+jacobian closure for a 1-D linear residual r(x) = x - target.
    /// J column shape: [0,0,0, 1,0,0] (linear in x).
    fn linear_1d_closure(
        target: f64,
    ) -> impl FnMut(&[f64]) -> Option<(Vec<f64>, Vec<Vec<f64>>)> {
        move |x: &[f64]| {
            assert_eq!(x.len(), 1);
            // Linear residual on first linear component.
            let r = vec![0.0, 0.0, 0.0, x[0] - target, 0.0, 0.0];
            // Single column: dr/dx0 = [0,0,0, 1,0,0].
            let j = vec![vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0]];
            Some((r, j))
        }
    }

    #[test]
    fn newton_solve_1d_linear_converges() {
        let cfg = NewtonConfig::default();
        let outcome = newton_solve(vec![0.5], linear_1d_closure(0.3), &cfg);
        match outcome {
            NewtonOutcome::Converged {
                x,
                iters,
                residual_norm,
            } => {
                assert!((x[0] - 0.3).abs() < 1e-9, "expected x≈0.3, got {}", x[0]);
                assert!(iters >= 1, "expected at least 1 iter");
                assert!(
                    residual_norm < cfg.tol_pos_m * 2.0,
                    "expected residual_norm < tol, got {residual_norm}"
                );
            }
            other => panic!("expected Converged, got {other:?}"),
        }
    }

    #[test]
    fn newton_solve_max_iters_zero_returns_not_converged() {
        let cfg = NewtonConfig {
            tol_pos_m: 1e-6,
            tol_rot_rad: 1e-6,
            max_iters: 0,
        };
        let outcome = newton_solve(vec![0.5], linear_1d_closure(0.3), &cfg);
        match outcome {
            NewtonOutcome::NotConverged { x, residual_norm } => {
                assert!((x[0] - 0.5).abs() < 1e-12);
                assert!(
                    (residual_norm - 0.2).abs() < 1e-9,
                    "expected residual_norm ≈ 0.2, got {residual_norm}"
                );
            }
            other => panic!("expected NotConverged, got {other:?}"),
        }
    }

    #[test]
    fn newton_solve_2d_diagonal_converges() {
        let cfg = NewtonConfig::default();
        let closure = |x: &[f64]| -> Option<(Vec<f64>, Vec<Vec<f64>>)> {
            assert_eq!(x.len(), 2);
            // Two stacked 6-vector residuals, one per "loop".
            let mut r = vec![0.0; 12];
            r[3] = x[0] - 1.0; // residual loop 0, linear x
            r[9] = x[1] - 2.0; // residual loop 1, linear x
            // Two columns, each 12-element.
            let mut c0 = vec![0.0; 12];
            c0[3] = 1.0;
            let mut c1 = vec![0.0; 12];
            c1[9] = 1.0;
            Some((r, vec![c0, c1]))
        };
        let outcome = newton_solve(vec![0.0, 0.0], closure, &cfg);
        match outcome {
            NewtonOutcome::Converged { x, iters, .. } => {
                assert!((x[0] - 1.0).abs() < 1e-9);
                assert!((x[1] - 2.0).abs() < 1e-9);
                assert!(iters >= 1);
            }
            other => panic!("expected Converged, got {other:?}"),
        }
    }

    #[test]
    fn newton_solve_rank_deficient_jacobian_returns_singular() {
        let cfg = NewtonConfig::default();
        // 2 free vars, but both columns are scaled copies of each other →
        // J^T J is singular (rank 1).
        let closure = |x: &[f64]| -> Option<(Vec<f64>, Vec<Vec<f64>>)> {
            assert_eq!(x.len(), 2);
            let r = vec![0.0, 0.0, 0.0, x[0] + 2.0 * x[1] - 1.0, 0.0, 0.0];
            // c0 = [0,0,0, 1,0,0]; c1 = [0,0,0, 2,0,0] = 2*c0
            let c0 = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0];
            let c1 = vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0];
            Some((r, vec![c0, c1]))
        };
        let outcome = newton_solve(vec![0.0, 0.0], closure, &cfg);
        match outcome {
            NewtonOutcome::Singular { x, .. } => {
                // x is whatever we had on the iteration that detected singularity
                assert_eq!(x.len(), 2);
            }
            other => panic!("expected Singular, got {other:?}"),
        }
    }

    #[test]
    fn newton_solve_closure_returning_none_returns_not_converged() {
        let cfg = NewtonConfig::default();
        let closure = |_x: &[f64]| -> Option<(Vec<f64>, Vec<Vec<f64>>)> { None };
        let outcome = newton_solve(vec![0.5], closure, &cfg);
        match outcome {
            NewtonOutcome::NotConverged { x, residual_norm } => {
                assert_eq!(x, vec![0.5]);
                assert!(residual_norm.is_infinite() || residual_norm.is_nan());
            }
            other => panic!("expected NotConverged, got {other:?}"),
        }
    }

    // ── solve_loop_closure tests (step-15, step-17, step-19) ────────────

    use reify_stdlib::eval_builtin;
    use reify_types::Value;

    fn axis_x() -> Value {
        Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)])
    }

    fn length_range(lo: f64, up: f64) -> Value {
        Value::Range {
            lower: Some(Box::new(Value::length(lo))),
            upper: Some(Box::new(Value::length(up))),
            lower_inclusive: true,
            upper_inclusive: true,
        }
    }

    fn prismatic_x_0_to_1() -> Value {
        eval_builtin("prismatic", &[axis_x(), length_range(0.0, 1.0)])
    }

    #[test]
    fn solve_loop_closure_midpoint_max_iters_zero_returns_midpoint_in_x() {
        // chain_b's joint is prismatic_x with range 0..1m → midpoint 0.5m.
        // Setting max_iters=0 should return NotConverged with x = [0.5]
        // (the midpoint, before any Newton step).
        let chain_a = vec![prismatic_x_0_to_1()];
        let vals_a = vec![0.5];
        let chain_b = vec![prismatic_x_0_to_1()];
        let vals_b_initial = vec![0.0];
        let free_b = vec![0];
        let strategy = StartStrategy::Midpoint;
        let cfg = NewtonConfig {
            tol_pos_m: 1e-6,
            tol_rot_rad: 1e-6,
            max_iters: 0,
        };

        let outcome = solve_loop_closure(
            &chain_a,
            &vals_a,
            &chain_b,
            &vals_b_initial,
            &free_b,
            &strategy,
            &cfg,
        );

        match outcome {
            NewtonOutcome::NotConverged { x, .. } => {
                assert_eq!(x.len(), 1);
                assert!(
                    (x[0] - 0.5).abs() < 1e-12,
                    "expected midpoint x=[0.5], got {x:?}"
                );
            }
            other => panic!("expected NotConverged with midpoint x, got {other:?}"),
        }
    }

    #[test]
    fn solve_loop_closure_midpoint_converges() {
        // Midpoint init at 0.5m, but chain_a's value is also 0.5m so we are
        // at the root immediately — trivially Converged in 0 iterations.
        let chain_a = vec![prismatic_x_0_to_1()];
        let vals_a = vec![0.5];
        let chain_b = vec![prismatic_x_0_to_1()];
        let vals_b_initial = vec![0.0];
        let free_b = vec![0];
        let strategy = StartStrategy::Midpoint;
        let cfg = NewtonConfig::default();

        let outcome = solve_loop_closure(
            &chain_a,
            &vals_a,
            &chain_b,
            &vals_b_initial,
            &free_b,
            &strategy,
            &cfg,
        );

        match outcome {
            NewtonOutcome::Converged { x, .. } => {
                assert!((x[0] - 0.5).abs() < 1e-6);
            }
            other => panic!("expected Converged, got {other:?}"),
        }
    }

    #[test]
    fn solve_loop_closure_warm_start_converges_single_prismatic() {
        // chain_a fixed at 0.5m; chain_b's free var should converge there.
        let chain_a = vec![prismatic_x_0_to_1()];
        let vals_a = vec![0.5];
        let chain_b = vec![prismatic_x_0_to_1()];
        let vals_b_initial = vec![0.0]; // FREE var
        let free_b = vec![0];
        let strategy = StartStrategy::WarmStart(vec![0.0]);
        let cfg = NewtonConfig::default();

        let outcome = solve_loop_closure(
            &chain_a,
            &vals_a,
            &chain_b,
            &vals_b_initial,
            &free_b,
            &strategy,
            &cfg,
        );

        match outcome {
            NewtonOutcome::Converged { x, .. } => {
                assert_eq!(x.len(), 1);
                assert!(
                    (x[0] - 0.5).abs() < 1e-6,
                    "expected x[0] ≈ 0.5, got {}",
                    x[0]
                );
            }
            other => panic!("expected Converged, got {other:?}"),
        }
    }
}
