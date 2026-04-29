//! Loop-closure Newton solver, configuration types, and convenience wrappers.
//!
//! Generic Gauss-Newton solver and configuration for closing kinematic
//! loops: callers supply a residual+jacobian closure, the solver returns a
//! [`NewtonOutcome`] describing convergence, divergence, or a singular Jacobian.
//!
//! Public API surface:
//!   * [`NewtonConfig`] `{ tol_pos_m, tol_rot_rad, max_iters }` — defaults
//!     1 µm position / 1 µrad rotation / 50 iters per the PRD.
//!   * [`StartStrategy`]`::{WarmStart(Vec<f64>), Midpoint}` — initial guess
//!     for the free-variable vector.  Warm-start re-uses a prior snapshot's
//!     converged values; midpoint queries each free joint's
//!     `joint_range_midpoint` from `reify_stdlib::loop_closure`.
//!   * [`NewtonOutcome`]`::{Converged, NotConverged, Singular}`.
//!   * [`newton_solve`]`<F>(x0, residual_jac, &config) -> NewtonOutcome`
//!     where `F: FnMut(&[f64]) -> Option<(Vec<f64>, Vec<Vec<f64>>)>` returns
//!     `(residual, jacobian_columns)`.  Generic over loop topology.
//!   * [`solve_loop_closure`]`(chain_a, vals_a, chain_b, vals_b_initial,
//!     free_b, strategy, config) -> NewtonOutcome` — single-loop convenience
//!     wrapper that builds the residual+jacobian closure from stdlib helpers.
//!
//! Twist convention: `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` per loop residual /
//! per Jacobian column (angular first, linear last) — single canonical
//! ordering across this module and `reify_stdlib::loop_closure`.
//!
//! Convergence rule: a Newton iteration is "converged" iff the per-iteration
//! residual's **linear sub-norm** is below `config.tol_pos_m` AND its
//! **angular sub-norm** is below `config.tol_rot_rad`.  The two tolerances are
//! honoured independently, matching the PRD's `1 µm position / 1 µrad rotation`
//! defaults; users may tighten one without affecting the other.  Stacked
//! multi-loop residuals aggregate sub-norms via L2 across loops.
//!
//! Jacobian strategy: chain Jacobians come from
//! `reify_stdlib::loop_closure::chain_jacobian_fd` (central difference,
//! correct for all joint kinds).  Per-joint analytic columns from
//! `per_joint_jacobian_local` are wired but not yet composed into chain
//! Jacobians via SE(3) adjoint transport — that optimisation is a v0.2
//! follow-up.  Singularity detection: Gauss-Newton normal-equations matrix
//! is factorised inline with LDLᵀ; min-pivot below
//! [`NewtonConfig::singularity_pivot_eps`] dispatches to
//! [`NewtonOutcome::Singular`] (the signal task 9 will translate into the
//! PRD's `W_KINEMATIC_SINGULARITY` warning).
//!
//! Robustness scope (v0.2 task 2 MVP): the solver is pure Gauss-Newton with
//! no damping or line search.  A monotonic-divergence guard (see
//! [`DIVERGENCE_LIMIT`]) bails out early as `NotConverged` if the residual
//! norm increases for several iterations in a row, preventing run-away
//! step-uphill behaviour from exhausting `max_iters` on the wrong iterate.
//! Levenberg–Marquardt damping and an Armijo line search are tracked as
//! follow-ups for v0.2 task 9 once real-world non-linear loops surface
//! cases the bare Gauss-Newton step cannot handle.
//!
//! See `docs/prds/v0_2/kinematic-constraints.md` §"Loop-closure solver" for the
//! full design rationale.

/// Convergence and iteration knobs for [`newton_solve`] / [`solve_loop_closure`].
///
/// PRD defaults — `tol_pos_m = 1e-6` (1 µm position), `tol_rot_rad = 1e-6`
/// (1 µrad rotation), `max_iters = 50`,
/// `singularity_pivot_eps = 1e-12`.  See
/// `docs/prds/v0_2/kinematic-constraints.md` §"Loop-closure solver".
#[derive(Debug, Clone)]
pub struct NewtonConfig {
    /// Linear-residual tolerance for convergence (metres).
    pub tol_pos_m: f64,
    /// Angular-residual tolerance for convergence (radians).
    pub tol_rot_rad: f64,
    /// Maximum Newton iterations before giving up.
    pub max_iters: usize,
    /// Min absolute LDLᵀ pivot below which the normal-equations matrix
    /// `JᵀJ` is treated as singular (rank-deficient Jacobian).  Tightening
    /// this admits more conditioned-but-near-singular problems; loosening
    /// it triggers the [`NewtonOutcome::Singular`] path earlier.  Default
    /// `1e-12` is a conservative double-precision threshold.
    pub singularity_pivot_eps: f64,
}

impl Default for NewtonConfig {
    fn default() -> Self {
        Self {
            tol_pos_m: 1e-6,
            tol_rot_rad: 1e-6,
            max_iters: 50,
            singularity_pivot_eps: DEFAULT_SINGULARITY_PIVOT_EPS,
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
/// `InvalidInput` — caller-supplied inputs failed validation (length
/// mismatch, out-of-range index, missing joint range for `Midpoint`); kept
/// distinct from `NotConverged` so callers can tell "solver couldn't reach
/// tol" from "you gave me bad inputs".
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
    /// Solver hit `max_iters` without reaching tolerance.  `x` and
    /// `residual_norm` correspond to the same iterate: `residual_norm` is
    /// the combined norm of `r(x)` at the returned `x`.
    NotConverged {
        /// Free-variable values at the last iteration.
        x: Vec<f64>,
        /// Combined residual norm at the last iteration (same iterate as `x`).
        residual_norm: f64,
    },
    /// Solver detected a rank-deficient Jacobian (min-pivot below
    /// [`NewtonConfig::singularity_pivot_eps`]).
    Singular {
        /// Free-variable values at the iteration where singularity was detected.
        x: Vec<f64>,
        /// Number of completed iterations before singularity.
        iters: usize,
    },
    /// Caller-supplied inputs failed validation (e.g. `WarmStart` length
    /// mismatch, `free_b` index out of range, `Midpoint` for a joint with
    /// no range).  Distinct from `NotConverged` so the contract is explicit.
    InvalidInput {
        /// Human-readable diagnostic; suitable for `tracing::warn!` or test
        /// assertions but not a stable API string.
        reason: String,
    },
}

/// Default pivot threshold below which the LDLᵀ factor is treated as
/// singular.  Used by [`NewtonConfig::default()`] —
/// [`NewtonConfig::singularity_pivot_eps`] is the user-configurable knob.
const DEFAULT_SINGULARITY_PIVOT_EPS: f64 = 1e-12;

/// Number of consecutive residual-norm increases that trigger the
/// divergence guard in [`newton_solve`].  See the module-level rustdoc
/// "Robustness scope" note for rationale.
const DIVERGENCE_LIMIT: usize = 3;

/// Compute split position / rotation residual sub-norms over a stacked twist
/// residual.
///
/// The residual is laid out as `[ω_x, ω_y, ω_z, v_x, v_y, v_z]` per loop
/// (mirroring the `transform_log` / `joint_jacobian` Map shape).  We aggregate
/// across loops with L2-norm so a multi-loop residual collapses to two
/// scalars: `(angular_norm, linear_norm)`.
///
/// Malformed-input contract: if `r.len()` is not a multiple of 6, the
/// trailing partial chunk is split by index — first 3 entries contribute to
/// the angular norm, remaining ≤2 to the linear norm.  This is a degraded
/// best-effort guard so a caller bug doesn't panic in release; in dev a
/// `debug_assert!` will catch the misuse loudly.  Pinned by
/// `position_rotation_norms_partial_chunk_partitions_by_index` below.
fn position_rotation_norms(r: &[f64]) -> (f64, f64) {
    debug_assert!(
        r.len().is_multiple_of(6),
        "residual length {} is not a multiple of 6 — caller is misusing the stacked-twist contract",
        r.len()
    );
    let mut ang2 = 0.0;
    let mut lin2 = 0.0;
    for chunk in r.chunks(6) {
        // chunk may be shorter than 6 only on malformed input — guard so we
        // don't panic in release on caller errors.  See doc above.
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
/// Returns `None` if the min absolute pivot drops below `pivot_eps` — that
/// is the signal that the Gauss-Newton normal-equations matrix `JᵀJ` is
/// rank-deficient.  Callers should pass [`NewtonConfig::singularity_pivot_eps`].
fn solve_normal_equations(
    mut a: Vec<Vec<f64>>,
    mut b: Vec<f64>,
    pivot_eps: f64,
) -> Option<Vec<f64>> {
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
        for (k, row) in a.iter().enumerate().take(j) {
            d_jj -= a[j][k] * a[j][k] * row[k];
        }
        if d_jj.abs() < pivot_eps {
            return None;
        }
        a[j][j] = d_jj;
        // Compute L[i,j] for i > j: a[i,j] = (a[i,j] - Σ_{k<j} L[i,k]*L[j,k]*D[k,k]) / D[j,j]
        for i in (j + 1)..n {
            let mut s = a[i][j];
            for (k, row) in a.iter().enumerate().take(j) {
                s -= a[i][k] * a[j][k] * row[k];
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
/// Singularity rule: per the inlined LDLᵀ pivot check
/// ([`NewtonConfig::singularity_pivot_eps`] threshold), any rank-deficient
/// `JᵀJ` returns `NewtonOutcome::Singular`.
///
/// Divergence guard: if the combined residual norm strictly increases for
/// [`DIVERGENCE_LIMIT`] consecutive iterations, the solver bails out early
/// as `NotConverged` with the iterate at which divergence was detected.
/// This prevents an undamped Gauss-Newton step from running uphill until
/// `max_iters` is reached.  See the module-level "Robustness scope" note.
///
/// Result invariant: `NotConverged.x` and `NotConverged.residual_norm` always
/// correspond to the same iterate — `residual_norm` is the combined norm
/// (`sqrt(linear² + angular²)`) of `r(x)` at the returned `x`.
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
    let mut prev_combined_norm: Option<f64> = None;
    let mut diverging_streak: usize = 0;

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
        let combined_norm = (ang_norm * ang_norm + lin_norm * lin_norm).sqrt();
        last_residual_norm = combined_norm;

        if lin_norm < config.tol_pos_m && ang_norm < config.tol_rot_rad {
            return NewtonOutcome::Converged {
                x,
                iters: iter,
                residual_norm: combined_norm,
            };
        }

        // Divergence guard: residual strictly grew vs. previous iter.  After
        // DIVERGENCE_LIMIT consecutive growths, bail out — undamped
        // Gauss-Newton has no recovery, so iterating further only wastes
        // work and risks numerical blow-up.  See module rustdoc.
        if let Some(prev) = prev_combined_norm {
            if combined_norm > prev {
                diverging_streak += 1;
                if diverging_streak >= DIVERGENCE_LIMIT {
                    return NewtonOutcome::NotConverged {
                        x,
                        residual_norm: combined_norm,
                    };
                }
            } else {
                diverging_streak = 0;
            }
        }
        prev_combined_norm = Some(combined_norm);

        // Build JᵀJ (n×n) and Jᵀr (n).
        // TODO(perf, suggestion #5): JᵀJ is allocated as a fresh vec-of-vecs
        // per iteration, and the full n×n matrix is populated even though it
        // is symmetric.  For multi-loop / many-DOF problems this defeats
        // cache locality.  A future pass should reuse a single contiguous
        // `Vec<f64>` scratch buffer (length n*n) across iterations and only
        // populate j>=i, mirroring the rest.
        if j_cols.len() != n {
            return NewtonOutcome::NotConverged {
                x,
                residual_norm: combined_norm,
            };
        }
        if j_cols.iter().any(|c| c.len() != r.len()) {
            return NewtonOutcome::NotConverged {
                x,
                residual_norm: combined_norm,
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
        let dx = match solve_normal_equations(jtj, neg_jtr, config.singularity_pivot_eps) {
            Some(d) => d,
            None => {
                return NewtonOutcome::Singular { x, iters: iter };
            }
        };
        for i in 0..n {
            x[i] += dx[i];
        }
    }

    // After max_iters (without convergence): re-evaluate r(x) at the
    // final iterate so `residual_norm` matches the returned `x`.  Without
    // this, `last_residual_norm` would reflect r(x_{N-1}) — the iterate
    // BEFORE the final Newton step — which is misleading to callers that
    // use `residual_norm` to gauge how close they got to a solution.
    // Fall back to `last_residual_norm` if the closure refuses the final
    // iterate (e.g. a chain that goes Value::Undef under the new values).
    if let Some((r, _)) = residual_jac(&x) {
        let (ang_norm, lin_norm) = position_rotation_norms(&r);
        last_residual_norm = (ang_norm * ang_norm + lin_norm * lin_norm).sqrt();
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
                let reason = format!(
                    "WarmStart length {} != free_b length {}",
                    v.len(),
                    free_b.len()
                );
                tracing::warn!("solve_loop_closure: {reason}");
                return NewtonOutcome::InvalidInput { reason };
            }
            v.clone()
        }
        StartStrategy::Midpoint => {
            let mut out = Vec::with_capacity(free_b.len());
            for &i in free_b {
                if i >= chain_b.len() {
                    let reason = format!(
                        "free_b index {} out of range (chain_b len {})",
                        i,
                        chain_b.len()
                    );
                    tracing::warn!("solve_loop_closure: {reason}");
                    return NewtonOutcome::InvalidInput { reason };
                }
                match reify_stdlib::loop_closure::joint_range_midpoint(&chain_b[i]) {
                    Some(m) => out.push(m),
                    None => {
                        let reason = format!(
                            "joint_range_midpoint returned None for free_b[{i}] — joint missing range or malformed"
                        );
                        tracing::warn!("solve_loop_closure: {reason}");
                        return NewtonOutcome::InvalidInput { reason };
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
        assert_eq!(cfg.singularity_pivot_eps, 1e-12);
    }

    #[test]
    fn newton_config_constructible_with_custom_values() {
        let cfg = NewtonConfig {
            tol_pos_m: 1e-3,
            tol_rot_rad: 1e-4,
            max_iters: 100,
            singularity_pivot_eps: 1e-10,
        };
        assert_eq!(cfg.tol_pos_m, 1e-3);
        assert_eq!(cfg.tol_rot_rad, 1e-4);
        assert_eq!(cfg.max_iters, 100);
        assert_eq!(cfg.singularity_pivot_eps, 1e-10);
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
        let _bad = NewtonOutcome::InvalidInput {
            reason: "bad".to_string(),
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
            ..NewtonConfig::default()
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

    // ── position_rotation_norms partial-chunk contract (suggestion 3) ──

    /// Documented best-effort behavior on malformed input: the trailing
    /// partial chunk is split by index — first 3 entries contribute to
    /// `ang2`, remaining (up to 2) to `lin2`.  Test runs only in release
    /// since debug_assert! would panic on the misuse.
    #[cfg(not(debug_assertions))]
    #[test]
    fn position_rotation_norms_partial_chunk_partitions_by_index() {
        // 8-element residual: full 6-chunk + 2-element partial.  The
        // partial's indices 0..2 are angular, so both go to ang2.
        let r = [3.0_f64, 4.0, 0.0, 0.0, 0.0, 0.0, 5.0, 12.0];
        let (ang, lin) = super::position_rotation_norms(&r);
        // ang2 = 3² + 4² + 5² + 12² = 9 + 16 + 25 + 144 = 194 → sqrt ≈ 13.9284
        assert!((ang - 194.0_f64.sqrt()).abs() < 1e-12);
        // lin2 = 0 → 0
        assert!(lin.abs() < 1e-12);

        // 10-element residual: full 6-chunk + 4-element partial.  Indices
        // 0..2 angular, index 3 linear.
        let r2 = [0.0_f64; 6]
            .iter()
            .copied()
            .chain([1.0, 2.0, 3.0, 4.0])
            .collect::<Vec<f64>>();
        let (ang2, lin2) = super::position_rotation_norms(&r2);
        // ang² = 1 + 4 + 9 = 14 → sqrt ≈ 3.7417
        assert!((ang2 - 14.0_f64.sqrt()).abs() < 1e-12);
        // lin² = 16 → 4
        assert!((lin2 - 4.0).abs() < 1e-12);
    }

    /// Empty residual must collapse to zero norms.
    #[test]
    fn position_rotation_norms_empty_residual_returns_zero() {
        let (ang, lin) = super::position_rotation_norms(&[]);
        assert_eq!(ang, 0.0);
        assert_eq!(lin, 0.0);
    }

    /// In dev (debug_assertions on), partial-chunk input must panic
    /// loudly — this catches caller bugs at the source.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "not a multiple of 6")]
    fn position_rotation_norms_partial_chunk_panics_in_dev() {
        let r = vec![1.0, 2.0, 3.0]; // length 3, not multiple of 6
        let _ = super::position_rotation_norms(&r);
    }

    // ── Non-linear convergence (suggestion 4) ───────────────────────────

    #[test]
    fn newton_solve_quadratic_converges_via_multiple_iters() {
        // r(x) = x[0]^2 - 4, J = 2*x[0].  True roots ±2.  Starting from
        // x = 5 the linear case from before would solve in 1 iter; the
        // quadratic requires several Newton steps.  Catches sign-errors
        // and ordering bugs in the Jacobian assembly that the linear
        // tests can't surface.
        let cfg = NewtonConfig {
            tol_pos_m: 1e-9,
            tol_rot_rad: 1e-9,
            max_iters: 50,
            ..NewtonConfig::default()
        };
        let closure = |x: &[f64]| -> Option<(Vec<f64>, Vec<Vec<f64>>)> {
            assert_eq!(x.len(), 1);
            let r = vec![0.0, 0.0, 0.0, x[0] * x[0] - 4.0, 0.0, 0.0];
            let j = vec![vec![0.0, 0.0, 0.0, 2.0 * x[0], 0.0, 0.0]];
            Some((r, j))
        };
        let outcome = newton_solve(vec![5.0], closure, &cfg);
        match outcome {
            NewtonOutcome::Converged { x, iters, residual_norm } => {
                assert!(
                    (x[0] - 2.0).abs() < 1e-6,
                    "expected x≈2.0, got {}",
                    x[0]
                );
                assert!(
                    iters >= 2,
                    "non-linear case must require multiple Newton iters; got iters={iters}"
                );
                assert!(
                    residual_norm < 1e-9,
                    "expected residual_norm < 1e-9 at convergence, got {residual_norm}"
                );
            }
            other => panic!("expected Converged on quadratic, got {other:?}"),
        }
    }

    // ── Residual-consistency invariant (suggestion 1) ───────────────────

    #[test]
    fn newton_solve_not_converged_residual_matches_returned_x() {
        // Force NotConverged with max_iters > 0 and a residual that the
        // Newton step won't fully resolve at the final iterate (we
        // construct a linear residual whose root is reachable, but cap
        // max_iters before the convergence-check iteration runs).
        //
        // Linear residual r(x) = x[0] - 0.3, J = 1.  From x0 = 0.5:
        //   iter 0:  r(0.5) = 0.2 → step → x = 0.3
        //   iter 1:  r(0.3) = 0.0 → would converge.
        // With max_iters = 1 we exit AFTER stepping at iter 0 without
        // the iter-1 convergence check.  The returned x must be 0.3,
        // and the returned residual_norm must be the norm of r(x=0.3) = 0,
        // NOT the pre-step r(x=0.5) = 0.2.
        let cfg = NewtonConfig {
            tol_pos_m: 1e-12,
            tol_rot_rad: 1e-12,
            max_iters: 1,
            ..NewtonConfig::default()
        };
        let outcome = newton_solve(vec![0.5], linear_1d_closure(0.3), &cfg);
        match outcome {
            NewtonOutcome::NotConverged { x, residual_norm } => {
                assert!((x[0] - 0.3).abs() < 1e-9, "expected x≈0.3, got {}", x[0]);
                // r(0.3) = 0 → residual_norm should be ≈ 0, not 0.2.
                assert!(
                    residual_norm < 1e-9,
                    "residual_norm should match r(x_final)=0, got {residual_norm}"
                );
            }
            other => panic!("expected NotConverged, got {other:?}"),
        }
    }

    // ── Divergence guard (suggestion 2) ─────────────────────────────────

    #[test]
    fn newton_solve_divergence_guard_bails_out_on_monotonic_growth() {
        // Construct a closure whose residual grows monotonically with
        // each iteration, simulating an undamped Gauss-Newton run-away.
        //
        // We track call count via a Cell so the closure is FnMut.  Each
        // call returns a residual whose linear x-component is `2.0 *
        // call_count` and whose Jacobian column is identity (so the
        // Newton step = -residual, but we ignore the geometry — what
        // matters is that the *next* call sees a larger residual).
        let mut iter_counter: usize = 0;
        let closure = move |_x: &[f64]| -> Option<(Vec<f64>, Vec<Vec<f64>>)> {
            iter_counter += 1;
            // Residual grows linearly with iteration.
            let r = vec![0.0, 0.0, 0.0, iter_counter as f64, 0.0, 0.0];
            // Identity-ish J — the algebraic step doesn't matter, the
            // closure's residual ramp drives divergence detection.
            let j = vec![vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0]];
            Some((r, j))
        };
        let cfg = NewtonConfig {
            tol_pos_m: 1e-6,
            tol_rot_rad: 1e-6,
            max_iters: 50,
            ..NewtonConfig::default()
        };
        let outcome = newton_solve(vec![0.0], closure, &cfg);
        match outcome {
            NewtonOutcome::NotConverged { residual_norm, .. } => {
                // Guard fires after DIVERGENCE_LIMIT (=3) consecutive
                // increases.  We start at iter 0 with norm 1, then 2, 3, 4.
                // First "increase" tracked at iter 1 (1→2), streak=1.
                // Iter 2 (2→3): streak=2.  Iter 3 (3→4): streak=3 → bail.
                // Final norm should be at iter 3's residual = 4.
                assert!(
                    residual_norm.is_finite(),
                    "expected finite residual_norm at divergence bail-out"
                );
                assert!(
                    residual_norm < 50.0,
                    "divergence guard must bail before max_iters; got residual_norm {residual_norm}"
                );
            }
            other => panic!("expected NotConverged from divergence guard, got {other:?}"),
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

    fn axis_z() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)])
    }

    fn length_range(lo: f64, up: f64) -> Value {
        Value::Range {
            lower: Some(Box::new(Value::length(lo))),
            upper: Some(Box::new(Value::length(up))),
            lower_inclusive: true,
            upper_inclusive: true,
        }
    }

    fn angle_range(lo: f64, up: f64) -> Value {
        Value::Range {
            lower: Some(Box::new(Value::angle(lo))),
            upper: Some(Box::new(Value::angle(up))),
            lower_inclusive: true,
            upper_inclusive: true,
        }
    }

    fn prismatic_x_0_to_1() -> Value {
        eval_builtin("prismatic", &[axis_x(), length_range(0.0, 1.0)])
    }

    fn revolute_z_0_to_pi() -> Value {
        eval_builtin(
            "revolute",
            &[axis_z(), angle_range(0.0, std::f64::consts::PI)],
        )
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
            ..NewtonConfig::default()
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

    // ── Tolerance plumbing tests (step-19) ──────────────────────────────

    #[test]
    fn solve_loop_closure_loose_position_tol_converges_quickly() {
        // chain_a fixed at 0.5m; chain_b's free var starts at 0.499m.
        // Initial residual ~1mm in linear x. With a loose 1e-3 m tolerance,
        // we should converge ~immediately (Newton solves the linear case
        // in 1 iter to machine precision anyway).
        let chain_a = vec![prismatic_x_0_to_1()];
        let vals_a = vec![0.5];
        let chain_b = vec![prismatic_x_0_to_1()];
        let vals_b_initial = vec![0.499];
        let free_b = vec![0];
        let strategy = StartStrategy::WarmStart(vec![0.499]);
        let cfg_loose = NewtonConfig {
            tol_pos_m: 1e-3,
            tol_rot_rad: 1e-3,
            max_iters: 100,
            ..NewtonConfig::default()
        };

        let outcome = solve_loop_closure(
            &chain_a,
            &vals_a,
            &chain_b,
            &vals_b_initial,
            &free_b,
            &strategy,
            &cfg_loose,
        );
        match outcome {
            NewtonOutcome::Converged { residual_norm, .. } => {
                assert!(
                    residual_norm < 1e-3,
                    "residual_norm should be below loose tol 1e-3, got {residual_norm}"
                );
            }
            other => panic!("expected Converged with loose tol, got {other:?}"),
        }
    }

    #[test]
    fn solve_loop_closure_tight_tol_still_converges() {
        // Same starting point, but tight 1e-9 tolerance. Linear case →
        // Newton finds the root in 1 step regardless of tol.
        let chain_a = vec![prismatic_x_0_to_1()];
        let vals_a = vec![0.5];
        let chain_b = vec![prismatic_x_0_to_1()];
        let vals_b_initial = vec![0.499];
        let free_b = vec![0];
        let strategy = StartStrategy::WarmStart(vec![0.499]);
        let cfg_tight = NewtonConfig {
            tol_pos_m: 1e-9,
            tol_rot_rad: 1e-9,
            max_iters: 100,
            ..NewtonConfig::default()
        };

        let outcome = solve_loop_closure(
            &chain_a,
            &vals_a,
            &chain_b,
            &vals_b_initial,
            &free_b,
            &strategy,
            &cfg_tight,
        );
        match outcome {
            NewtonOutcome::Converged { residual_norm, .. } => {
                assert!(
                    residual_norm < 1e-9,
                    "residual_norm should be below tight tol 1e-9, got {residual_norm}"
                );
            }
            other => panic!("expected Converged with tight tol, got {other:?}"),
        }
    }

    #[test]
    fn newton_solve_split_tolerance_rotational_below_linear_above_not_converged() {
        // Build a contrived residual closure where:
        //   linear residual = 1e-2 (above tol_pos_m = 1e-3)
        //   angular residual = 1e-5 (below tol_rot_rad = 1e-3)
        // Convergence rule MUST require BOTH to be below their respective
        // tolerances — so this should NOT report Converged, even though one
        // sub-norm is below tol.
        // We use max_iters=0 so we cleanly exit with NotConverged at the
        // initial residual (without taking a Newton step that would change
        // the analysis).
        let cfg = NewtonConfig {
            tol_pos_m: 1e-3,
            tol_rot_rad: 1e-3,
            max_iters: 0,
            ..NewtonConfig::default()
        };
        let closure = |_x: &[f64]| -> Option<(Vec<f64>, Vec<Vec<f64>>)> {
            // [ω_x, ω_y, ω_z, v_x, v_y, v_z]
            let r = vec![1e-5, 0.0, 0.0, 1e-2, 0.0, 0.0];
            let j = vec![vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0]];
            Some((r, j))
        };
        let outcome = newton_solve(vec![0.0], closure, &cfg);
        match outcome {
            NewtonOutcome::NotConverged { residual_norm, .. } => {
                // residual_norm = sqrt((1e-5)^2 + (1e-2)^2) ≈ 1e-2
                assert!(
                    (residual_norm - 1e-2).abs() < 1e-7,
                    "expected residual_norm ≈ 1e-2, got {residual_norm}"
                );
            }
            other => panic!(
                "linear above tol must NOT converge even when angular below tol; got {other:?}"
            ),
        }
    }

    // ── Non-linear loop closure (suggestion 4) ──────────────────────────

    #[test]
    fn solve_loop_closure_revolute_then_prismatic_converges_with_rotation() {
        // chain = [revolute_z, prismatic_x] — non-commuting composition:
        // the prismatic translation happens in the rotated frame, so the
        // SE(3) residual is genuinely non-linear in the free vars.
        // chain_a fixed at (θ=π/3, t=0.5) → end-effector at
        //   T = R_z(π/3) · Trans_x(0.5) → translation (0.25, 0.433, 0).
        // chain_b free at (θ, t) starting from (0.0, 0.0).  Solver must
        // recover (π/3, 0.5).  Newton with FD Jacobian should still
        // converge but on a residual whose linear/angular parts are
        // *both* non-trivial — this exercises sign-/ordering-bugs in the
        // Jacobian assembly that single-prismatic tests cannot.
        let chain_a = vec![revolute_z_0_to_pi(), prismatic_x_0_to_1()];
        let theta_a = std::f64::consts::PI / 3.0;
        let vals_a = vec![theta_a, 0.5];
        let chain_b = vec![revolute_z_0_to_pi(), prismatic_x_0_to_1()];
        let vals_b_initial = vec![0.0, 0.0];
        let free_b = vec![0, 1];
        let strategy = StartStrategy::WarmStart(vec![0.1, 0.1]);
        // Generous max_iters; tight tol so we exercise convergence rate.
        let cfg = NewtonConfig {
            tol_pos_m: 1e-8,
            tol_rot_rad: 1e-8,
            max_iters: 50,
            ..NewtonConfig::default()
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
            NewtonOutcome::Converged { x, iters: _, residual_norm } => {
                assert_eq!(x.len(), 2);
                // Rotation about z must match.
                assert!(
                    (x[0] - theta_a).abs() < 1e-5,
                    "expected θ ≈ {theta_a}, got {}",
                    x[0]
                );
                // Prismatic length must match.
                assert!(
                    (x[1] - 0.5).abs() < 1e-5,
                    "expected t ≈ 0.5, got {}",
                    x[1]
                );
                assert!(
                    residual_norm < 1e-6,
                    "expected tight residual after convergence, got {residual_norm}"
                );
            }
            other => panic!(
                "expected Converged on revolute+prismatic loop closure, got {other:?}"
            ),
        }
    }

    // ── InvalidInput contract tests (suggestion 7) ──────────────────────

    #[test]
    fn solve_loop_closure_warm_start_length_mismatch_returns_invalid_input() {
        let chain_a = vec![prismatic_x_0_to_1()];
        let vals_a = vec![0.5];
        let chain_b = vec![prismatic_x_0_to_1()];
        let vals_b_initial = vec![0.0];
        let free_b = vec![0];
        // WarmStart length 2 ≠ free_b length 1.
        let strategy = StartStrategy::WarmStart(vec![0.0, 0.1]);
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
            NewtonOutcome::InvalidInput { reason } => {
                assert!(
                    reason.contains("WarmStart length"),
                    "expected reason to mention WarmStart length, got {reason:?}"
                );
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn solve_loop_closure_midpoint_free_b_out_of_range_returns_invalid_input() {
        let chain_a = vec![prismatic_x_0_to_1()];
        let vals_a = vec![0.5];
        let chain_b = vec![prismatic_x_0_to_1()];
        let vals_b_initial = vec![0.0];
        // free_b index 5 is out of range for chain_b of length 1.
        let free_b = vec![5];
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
            NewtonOutcome::InvalidInput { reason } => {
                assert!(
                    reason.contains("out of range"),
                    "expected reason to mention out-of-range index, got {reason:?}"
                );
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn solve_loop_closure_midpoint_missing_range_returns_invalid_input() {
        // A revolute Map without a range field — joint_range_midpoint → None.
        // Build a malformed Map directly.
        let mut bad_joint_map = std::collections::BTreeMap::new();
        bad_joint_map.insert(
            Value::String("kind".to_string()),
            Value::String("revolute".to_string()),
        );
        bad_joint_map.insert(
            Value::String("axis".to_string()),
            Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]),
        );
        // No "range" key.
        let bad_joint = Value::Map(bad_joint_map);
        let chain_a = vec![prismatic_x_0_to_1()];
        let vals_a = vec![0.5];
        let chain_b = vec![bad_joint];
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
            NewtonOutcome::InvalidInput { reason } => {
                assert!(
                    reason.contains("joint_range_midpoint"),
                    "expected reason to mention joint_range_midpoint, got {reason:?}"
                );
            }
            other => panic!("expected InvalidInput, got {other:?}"),
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
