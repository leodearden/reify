//! Loop-closure Newton solver, configuration types, and convenience wrappers.
//!
//! Generic Gauss-Newton solver and configuration for closing kinematic
//! loops: callers supply a residual+jacobian closure, the solver returns a
//! [`NewtonOutcome`] describing convergence, divergence, or a singular Jacobian.
//!
//! ## γ widening (PRD KCC-γ, 2026-05-27)
//!
//! [`solve_loop_closure`] and [`solve_loop_closure_with_diagnostics`] now take
//! `vals_a: &[JointValue]` / `vals_b_initial: &[JointValue]` instead of the
//! prior `&[f64]`.  Multi-DOF joints (planar / spherical / cylindrical) can now
//! participate in closed-chain Newton solves; the Newton state remains a flat
//! `Vec<f64>` internally, with [`JointKind::flat_len`](crate::loop_closure_value::JointKind::flat_len)
//! driving the storage width and a per-iteration `renormalize_quaternion`
//! projecting Sphere slots back onto S³.  `StartStrategy::WarmStart(Vec<f64>)`
//! still carries an already-flattened vector (Newton-state coordinates, not
//! per-JointValue) for backward compatibility.
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
//! `crate::loop_closure::chain_jacobian_fd` (central difference,
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
    /// Per-component Tikhonov damping added to `JᵀJ`'s diagonal before
    /// the LDLᵀ factorisation.  Empty (default) means no damping.  The
    /// per-component vector lets [`solve_loop_closure`] target only the
    /// redundant Sphere-quaternion storage components (3 manifold DOF in 4
    /// stored components; the 4th column of the storage-perturbed FD Jacobian
    /// is the unit-norm-constraint normal direction → rank-deficient by
    /// construction) without polluting non-Sphere joints' Newton steps.
    /// PRD §5.3 "redundant Lagrangian coordinates" trick: damping the
    /// off-manifold direction restores a non-singular `JᵀJ`; the closure-
    /// internal `renormalize_quaternion` then projects the iterate back to
    /// S³ each step.  Length must either be 0 or match the Newton state
    /// width; out-of-range indices are silently ignored.
    pub regularization_per_diag: Vec<f64>,
}

impl Default for NewtonConfig {
    fn default() -> Self {
        Self {
            tol_pos_m: 1e-6,
            tol_rot_rad: 1e-6,
            max_iters: 50,
            singularity_pivot_eps: DEFAULT_SINGULARITY_PIVOT_EPS,
            regularization_per_diag: Vec::new(),
        }
    }
}

/// Strategy for picking the initial free-variable values for a loop-closure
/// snapshot solve.
///
/// `WarmStart(v)` uses the supplied vector directly (typical: previous
/// snapshot's converged values).  `Midpoint` queries each free joint's range
/// midpoint via [`crate::loop_closure::joint_range_midpoint`].
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
    ///
    /// The diagnostic-emitting wrapper [`solve_loop_closure_with_diagnostics`]
    /// translates this variant into a [`DiagnosticCode::KinematicSingularity`]
    /// Warning; [`LoopClosureReport::is_singular()`] returns `true` when this
    /// variant is present.  The `x` payload is preserved verbatim as the
    /// last-converged config the PRD requires.
    ///
    /// [`DiagnosticCode::KinematicSingularity`]: reify_types::DiagnosticCode::KinematicSingularity
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

/// Outcome of a [`solve_loop_closure_with_diagnostics`] call: the underlying
/// Newton outcome and any [`Diagnostic`]s the wrapper emitted.
///
/// The `outcome` field carries the canonical "what happened" enum from
/// [`solve_loop_closure`].  Use [`is_singular()`] to test whether the solver
/// detected a rank-deficient Jacobian — the accessor derives directly from
/// `outcome`, so the two cannot drift out of agreement by construction.
/// `diagnostics` collects the typed
/// [`DiagnosticCode::KinematicSingularity`] / `KinematicOverconstrained` /
/// `KinematicUnderconstrained` entries the PRD task 9 prose requires
/// (`docs/prds/v0_2/kinematic-constraints.md` §"Singularity, over/under-constraint
/// diagnostics").
///
/// See [`solve_loop_closure_with_diagnostics`] for the per-variant emission
/// rules.  Future task 10 (sweep API integration) will be the first consumer
/// that surfaces these diagnostics through the snapshot-call path.
///
/// [`is_singular()`]: LoopClosureReport::is_singular
/// [`Diagnostic`]: reify_types::Diagnostic
/// [`DiagnosticCode::KinematicSingularity`]: reify_types::DiagnosticCode::KinematicSingularity
#[derive(Debug, Clone)]
pub struct LoopClosureReport {
    /// The Newton solver's canonical outcome (Converged / NotConverged /
    /// Singular / InvalidInput).  For the over-constrained short-circuit
    /// path, this is `NotConverged { x, residual_norm: f64::INFINITY }`
    /// (the solver was not run; see
    /// [`solve_loop_closure_with_diagnostics`] for the contract).
    pub outcome: NewtonOutcome,
    /// Typed diagnostic entries the wrapper emitted (over-/under-constrained
    /// pre-checks and singular post-process).  Empty for a balanced,
    /// non-singular solve.
    pub diagnostics: Vec<reify_types::Diagnostic>,
}

impl LoopClosureReport {
    /// Returns `true` iff the Newton solver detected a rank-deficient Jacobian
    /// (i.e. `outcome` is [`NewtonOutcome::Singular`]).
    ///
    /// This is the single source of truth for singularity: the result is
    /// derived from `outcome` on demand, so `is_singular()` and `outcome`
    /// cannot drift out of agreement by construction.
    pub fn is_singular(&self) -> bool {
        matches!(self.outcome, NewtonOutcome::Singular { .. })
    }
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

/// Solve `A · x = b` for `x` in-place where `A` is a small dense symmetric
/// (semi-)PD matrix supplied as a flat row-major slice of length `n*n`, and
/// `b` is the RHS vector of length `n` that is overwritten with the solution.
///
/// Uses inlined LDLᵀ factorisation.  `a` is overwritten during factorisation
/// (strict-lower triangle → L, diagonal → D).
///
/// Returns `true` on success (`b` now holds `x`), or `false` if the minimum
/// absolute pivot drops below `pivot_eps` — the signal that `JᵀJ` is
/// rank-deficient.  Callers should pass [`NewtonConfig::singularity_pivot_eps`].
///
/// Precondition: `a.len() == n*n` and `b.len() == n` (asserted in debug builds).
///
/// Optional per-component Tikhonov regularization is added to `a`'s diagonal
/// before factorisation: `a[i*n+i] += reg[i]` for `i < reg.len()`.  Out-of-range
/// indices are ignored, and an empty `reg` slice is a no-op.  Used by
/// [`solve_loop_closure`] to damp the redundant-coordinate direction of
/// Sphere-slot Newton states (see [`NewtonConfig::regularization_per_diag`]).
fn solve_normal_equations(
    a: &mut [f64],
    b: &mut [f64],
    n: usize,
    pivot_eps: f64,
    reg: &[f64],
) -> bool {
    if n == 0 {
        return true;
    }
    debug_assert_eq!(a.len(), n * n);
    debug_assert_eq!(b.len(), n);
    // Apply per-component Tikhonov damping to the diagonal before LDLᵀ.
    // Damping the off-manifold direction of a Sphere slot's redundant
    // 4-storage-component Jacobian restores a non-singular JᵀJ; the
    // closure-internal renormalize_quaternion projects the iterate back to S³.
    for i in 0..n.min(reg.len()) {
        a[i * n + i] += reg[i];
    }
    // LDLᵀ: a is overwritten so that the strict-lower triangle holds L
    // (with implicit unit diagonal) and the diagonal holds D.
    for j in 0..n {
        // Compute D[j,j] = a[j,j] - Σ_{k<j} L[j,k]^2 * D[k,k]
        let mut d_jj = a[j * n + j];
        for k in 0..j {
            d_jj -= a[j * n + k] * a[j * n + k] * a[k * n + k];
        }
        if d_jj.abs() < pivot_eps {
            return false;
        }
        a[j * n + j] = d_jj;
        // Compute L[i,j] for i > j: a[i,j] = (a[i,j] - Σ_{k<j} L[i,k]*L[j,k]*D[k,k]) / D[j,j]
        for i in (j + 1)..n {
            let mut s = a[i * n + j];
            for k in 0..j {
                s -= a[i * n + k] * a[j * n + k] * a[k * n + k];
            }
            a[i * n + j] = s / d_jj;
        }
    }
    // Forward solve L · y = b (L unit-lower).
    for i in 0..n {
        let mut s = b[i];
        for k in 0..i {
            s -= a[i * n + k] * b[k];
        }
        b[i] = s;
    }
    // Diagonal solve D · z = y.
    for i in 0..n {
        b[i] /= a[i * n + i];
    }
    // Back solve Lᵀ · x = z.
    for i in (0..n).rev() {
        let mut s = b[i];
        for k in (i + 1)..n {
            s -= a[k * n + i] * b[k];
        }
        b[i] = s;
    }
    true
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
pub fn newton_solve<F>(x0: Vec<f64>, residual_jac: F, config: &NewtonConfig) -> NewtonOutcome
where
    F: FnMut(&[f64]) -> Option<(Vec<f64>, Vec<Vec<f64>>)>,
{
    newton_solve_with_projection(x0, residual_jac, |_x: &mut [f64]| {}, config)
}

/// Generic Gauss-Newton solver with a per-step projection hook.
///
/// Behaves identically to [`newton_solve`] except that after each
/// Newton step `x[i] += dx[i]`, the `post_step` closure is invoked with a
/// mutable reference to `x`.  This is the seam where on-manifold projection
/// happens — e.g. [`solve_loop_closure`] passes a closure that walks free
/// `JointValue::Sphere` slots and applies [`renormalize_quaternion`]
/// per-slot in storage space so the next iteration's `residual_jac` call
/// receives a unit-norm quaternion.
///
/// The projection is also applied to the final iterate before the function
/// returns, so [`NewtonOutcome::Converged::x`] and
/// [`NewtonOutcome::NotConverged::x`] always satisfy the manifold
/// invariants `post_step` enforces.
///
/// PRD §5.3 "redundant Lagrangian coordinates" trick: a Sphere slot has 4
/// stored components (`flat_len = 4`) but only 3 manifold DOF; the Newton
/// step is taken in storage space (`x += δx`); `post_step` projects each
/// Sphere slot back to S³ so the iterate stays on-manifold.
///
/// [`renormalize_quaternion`]: crate::loop_closure_value::JointValue::renormalize_quaternion
pub fn newton_solve_with_projection<F, P>(
    x0: Vec<f64>,
    mut residual_jac: F,
    mut post_step: P,
    config: &NewtonConfig,
) -> NewtonOutcome
where
    F: FnMut(&[f64]) -> Option<(Vec<f64>, Vec<Vec<f64>>)>,
    P: FnMut(&mut [f64]),
{
    let mut x = x0;
    let n = x.len();
    // Apply the projection to x0 itself — the caller's seed may be off-
    // manifold (e.g. WarmStart from a non-unit quaternion); the closure
    // expects on-manifold input from the very first iteration.
    post_step(&mut x);
    let mut last_residual_norm = f64::INFINITY;
    let mut prev_combined_norm: Option<f64> = None;
    let mut diverging_streak: usize = 0;

    // Scratch buffers reused across iterations to avoid per-iter allocation.
    // n is fixed for the lifetime of this call; residual_jac shape is
    // validated each iteration before we read into the buffers.
    let mut jtj_flat: Vec<f64> = vec![0.0; n * n];
    let mut jtr: Vec<f64> = vec![0.0; n];
    let mut dx: Vec<f64> = vec![0.0; n];

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

        // Build JᵀJ (n×n) and Jᵀr (n) into the hoisted scratch buffers.
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
        // Exploit symmetry: populate only the lower triangle (j <= i) —
        // n*(n+1)/2 dot products instead of n²; LDLᵀ reads only the lower
        // triangle and diagonal, so no mirroring is needed.
        for i in 0..n {
            for j in 0..=i {
                jtj_flat[i * n + j] = j_cols[i]
                    .iter()
                    .zip(j_cols[j].iter())
                    .map(|(a, b)| a * b)
                    .sum();
            }
            jtr[i] = j_cols[i].iter().zip(r.iter()).map(|(a, b)| a * b).sum();
        }
        // Solve JᵀJ · δx = -Jᵀr (dx is loaded with -jtr as RHS; solution
        // overwrites dx in place; jtj_flat is overwritten by LDLᵀ — both are
        // repopulated at the top of the next iteration).
        for i in 0..n {
            dx[i] = -jtr[i];
        }
        if !solve_normal_equations(
            &mut jtj_flat,
            &mut dx,
            n,
            config.singularity_pivot_eps,
            &config.regularization_per_diag,
        ) {
            return NewtonOutcome::Singular { x, iters: iter };
        }
        for i in 0..n {
            x[i] += dx[i];
        }
        // On-manifold projection of the post-step iterate.  No-op when
        // the caller supplied the default identity closure (via
        // [`newton_solve`]); for [`solve_loop_closure`], walks Sphere
        // slots and renormalizes each quaternion sub-vector.
        post_step(&mut x);
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
/// **`InvalidInput` contract** — the following are detected before the Newton
/// solve begins and return [`NewtonOutcome::InvalidInput`] regardless of
/// strategy:
/// - any `free_b` index ≥ `chain_b.len()` (index addresses a non-existent joint);
/// - any `free_b` index ≥ `vals_b_initial.len()` (index addresses a
///   non-existent initial value);
/// - [`StartStrategy::WarmStart`] vector length ≠ `free_b.len()`.
///
/// Internally builds a residual+jacobian closure that calls
/// [`crate::loop_closure::loop_residual_twist`] and
/// [`crate::loop_closure::chain_jacobian_fd`], then dispatches to
/// [`newton_solve`].
///
/// Multi-loop is future work (the [`newton_solve`] core is generic — callers
/// can stack residuals/columns from multiple loops).
///
/// ## See also
///
/// [`solve_loop_closure_with_diagnostics`] — diagnostic-emitting wrapper that
/// adds over/under-constrained pre-checks and a singularity post-process,
/// returning a [`LoopClosureReport`] (the canonical "what happened" outcome
/// plus an [`is_singular()`] accessor and any
/// [`DiagnosticCode::KinematicSingularity`] / `KinematicOverconstrained` /
/// `KinematicUnderconstrained` entries the PRD task 9 prose requires).
///
/// [`is_singular()`]: LoopClosureReport::is_singular
///
/// [`DiagnosticCode::KinematicSingularity`]: reify_types::DiagnosticCode::KinematicSingularity
pub fn solve_loop_closure(
    chain_a: &[reify_types::Value],
    vals_a: &[crate::loop_closure_value::JointValue],
    chain_b: &[reify_types::Value],
    vals_b_initial: &[crate::loop_closure_value::JointValue],
    free_b: &[usize],
    strategy: &StartStrategy,
    config: &NewtonConfig,
) -> NewtonOutcome {
    // Delegate all four input-validity checks to the shared helper.
    if let Some(reason) = validate_loop_closure_inputs(chain_b, vals_b_initial, free_b, strategy) {
        tracing::warn!("solve_loop_closure: {reason}");
        return NewtonOutcome::InvalidInput { reason };
    }

    // Derive the per-free-joint shape descriptor used for both the Newton
    // state width and the unflatten arithmetic inside the closure.
    let free_shapes: Vec<crate::loop_closure_value::JointKind> = match free_joint_shapes(chain_b, free_b) {
        Some(v) => v,
        None => {
            let reason = "free_b joint kind is unknown to JointKind::from_str".to_string();
            tracing::warn!("solve_loop_closure: {reason}");
            return NewtonOutcome::InvalidInput { reason };
        }
    };

    // KCC-γ §11.1 producer-side signal: probe each free joint's analytic-J
    // path once at solve start so the tracing event fires for callers / test
    // captures.  For multi-DOF kinds (planar / spherical / cylindrical) the
    // `joint_jacobian` builtin emits a `tracing::debug!` at target
    // `reify_stdlib::joints` carrying `kind = <kind>` — see
    // `joint_jacobian_value` in `joints.rs`.  The probe's return value is
    // intentionally discarded: the chain Jacobian itself is still composed via
    // FD inside the closure (see `chain_jacobian_fd`).  KCC-θ/ι will replace
    // the FD path with SE(3) adjoint transport over these analytic columns.
    for &i in free_b {
        let _ = crate::eval_builtin("joint_jacobian", std::slice::from_ref(&chain_b[i]));
    }

    // Resolve initial x0 from the strategy.  Inputs are validated above, so
    // each branch is infallible: WarmStart carries an already-flattened
    // Newton state of length `Σ free_shapes[k].flat_len()`; Midpoint
    // builds the flat state by flattening the per-kind midpoint surfaces.
    let x0: Vec<f64> = match strategy {
        StartStrategy::WarmStart(v) => v.clone(),
        StartStrategy::Midpoint => {
            let mut x = Vec::with_capacity(free_shapes.iter().map(|k| k.flat_len()).sum());
            for &i in free_b {
                // Validated above — joint_range_midpoint returns Some.
                let mid = crate::loop_closure::joint_range_midpoint(&chain_b[i])
                    .expect("joint_range_midpoint validated above");
                x.extend_from_slice(mid.as_f64_slice());
            }
            x
        }
    };

    // KCC-γ step-12: build per-component Tikhonov damping for Sphere slots
    // ONLY.  Sphere stores 4 quaternion components but the manifold has 3
    // DOFs; the storage-perturbed FD Jacobian's columns are linearly
    // dependent in the unit-norm-constraint normal direction (`transform_at`
    // normalises the input quaternion back to S³, so any pure-storage
    // perturbation in the q-direction has zero effect on the chain
    // transform).  Without damping, JᵀJ is rank-deficient on every Sphere
    // free slot → LDLᵀ pivot guard fires → spurious NewtonOutcome::Singular.
    // Damping the off-manifold direction with `SPHERE_TIKHONOV_DAMPING`
    // restores a non-singular JᵀJ while the closure-internal
    // renormalize_quaternion projects the Newton iterate back to the manifold
    // after each step.  Non-Sphere components carry zero damping → existing
    // 1-DOF / 2-DOF / 3-DOF (planar) chain Newton paths are byte-for-byte
    // unchanged (verified by the unchanged-output expectation of the existing
    // `solve_loop_closure_with_diagnostics_emits_singularity_for_rank_one_chain`
    // test in `crates/reify-constraints/tests/loop_closure_diagnostics_tests.rs`).
    let mut effective_config = config.clone();
    if effective_config.regularization_per_diag.is_empty() {
        let total_width: usize = free_shapes.iter().map(|k| k.flat_len()).sum();
        let mut reg = vec![0.0; total_width];
        let mut cursor = 0;
        for &k in &free_shapes {
            let width = k.flat_len();
            if matches!(k, crate::loop_closure_value::JointKind::Spherical) {
                for r in &mut reg[cursor..cursor + width] {
                    *r = SPHERE_TIKHONOV_DAMPING;
                }
            }
            cursor += width;
        }
        effective_config.regularization_per_diag = reg;
    }

    // Capture inputs for the closure.  The closure is FnMut over an internal
    // scratch buffer for vals_b to avoid reallocating each call.
    let chain_a_vec = chain_a.to_vec();
    let vals_a_vec = vals_a.to_vec();
    let chain_b_vec = chain_b.to_vec();
    let mut vals_b_scratch = vals_b_initial.to_vec();
    let free_b_vec = free_b.to_vec();
    let free_shapes_vec = free_shapes;
    // Clone for the projection closure (captured separately from the
    // residual_jac closure, both via move).
    let free_shapes_proj = free_shapes_vec.clone();

    let closure = move |x: &[f64]| -> Option<(Vec<f64>, Vec<Vec<f64>>)> {
        // Expected width = Σ free_shapes[k].flat_len().  The Newton state
        // is the flat concatenation of per-free-joint storage payloads.
        let expected_width: usize = free_shapes_vec.iter().map(|k| k.flat_len()).sum();
        if x.len() != expected_width {
            return None;
        }
        // Unflatten the per-free-joint slices into typed JointValues and
        // splice them back into vals_b_scratch at the free indices.  Per-step
        // projection (sphere renormalization) is already applied to `x` by
        // `newton_solve_with_projection`'s post-step hook, so the storage
        // here is on-manifold by construction; the unflatten is a no-op
        // projection (preserves bit-for-bit equality with `x`).
        let mut cursor = 0;
        for (k, &i) in free_b_vec.iter().enumerate() {
            if i >= vals_b_scratch.len() {
                return None;
            }
            let width = free_shapes_vec[k].flat_len();
            let chunk = &x[cursor..cursor + width];
            let jv = crate::loop_closure_value::JointValue::from_slice(
                free_shapes_vec[k],
                chunk,
            )
            .ok()?;
            cursor += width;
            vals_b_scratch[i] = jv;
        }
        let twist = crate::loop_closure::loop_residual_twist(
            &chain_a_vec,
            &vals_a_vec,
            &chain_b_vec,
            &vals_b_scratch,
        )?;
        let cols = crate::loop_closure::chain_jacobian_fd(
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

    // Per-step on-manifold projection of x.  Walks each free joint's flat
    // storage chunk; for Sphere slots, the chunk is a 4-component quaternion
    // [w, x, y, z] that we renormalize in-place to lie on S³.  Other
    // kinds (Scalar, Cyl, Planar, Fixed) are unaffected.  PRD §5.3 redundant-
    // coordinate Newton: storage-space step + manifold projection.
    let post_step = move |x: &mut [f64]| {
        let mut cursor = 0;
        for &k in &free_shapes_proj {
            let width = k.flat_len();
            if matches!(k, crate::loop_closure_value::JointKind::Spherical) {
                let chunk = &mut x[cursor..cursor + width];
                let norm_sq = chunk.iter().map(|v| v * v).sum::<f64>();
                if norm_sq > 0.0 {
                    let norm = norm_sq.sqrt();
                    for v in chunk.iter_mut() {
                        *v /= norm;
                    }
                } else {
                    // Degenerate: fall back to identity quaternion
                    // [w=1, x=0, y=0, z=0] (canonical Sphere identity).
                    chunk[0] = 1.0;
                    for v in &mut chunk[1..] {
                        *v = 0.0;
                    }
                }
            }
            cursor += width;
        }
    };

    newton_solve_with_projection(x0, closure, post_step, &effective_config)
}

/// Tikhonov-damping magnitude applied to each storage component of every
/// Sphere free slot before LDLᵀ.  Chosen to be:
///   - Strictly greater than `NewtonConfig`'s default
///     `singularity_pivot_eps` (1e-12) so the LDLᵀ pivot guard does not
///     fire on the redundant-coordinate direction; and
///   - Small enough relative to typical chain Jacobian magnitudes (O(1) for
///     unit-vector tangents) to leave the converged solution and convergence
///     rate of the on-manifold (3-DOF) Newton step essentially unchanged.
///
/// Tied to `singularity_pivot_eps` indirectly: any caller that loosens
/// `singularity_pivot_eps` past 1e-8 should also supply its own
/// `regularization_per_diag` if they want a guaranteed-non-singular path.
const SPHERE_TIKHONOV_DAMPING: f64 = 1e-8;

/// Compute per-free-joint shape descriptors from `chain_b[free_b[k]]`.
/// Returns None if any kind is unrecognised (defensive — chain_b joints
/// are upstream-validated by `extract_loop_closure_chains` /
/// `is_joint_value`, but a hand-built fixture might slip a malformed
/// joint Map through).
fn free_joint_shapes(
    chain_b: &[reify_types::Value],
    free_b: &[usize],
) -> Option<Vec<crate::loop_closure_value::JointKind>> {
    let mut out = Vec::with_capacity(free_b.len());
    for &i in free_b {
        if i >= chain_b.len() {
            return None;
        }
        let map = match &chain_b[i] {
            reify_types::Value::Map(m) => m,
            _ => return None,
        };
        let kind = match map.get(&reify_types::Value::String("kind".to_string())) {
            Some(reify_types::Value::String(s)) => s.as_str(),
            _ => return None,
        };
        out.push(crate::loop_closure_value::JointKind::from_str(kind)?);
    }
    Some(out)
}

/// Manifold DOF count per joint kind, used by the over/under-constrained
/// pre-check in `solve_loop_closure_with_diagnostics`.  Differs from
/// `JointKind::flat_len` (storage width) for `Spherical` (3 manifold DOF
/// vs 4 quaternion storage components) and `Fixed` (0 DOF vs 1-element
/// sentinel slot).
///
/// Returns 1 for Prismatic / Revolute / Coupling, 0 for Fixed, 2 for
/// Cylindrical, 3 for Planar, 3 for Spherical.  Sum across `free_b`
/// gives the kinematic free-DOF count that balances against the
/// 6-component loop residual: `< 6` = over-constrained, `== 6` =
/// well-posed, `> 6` = under-constrained.
fn dof_count_for_balance(kind: crate::loop_closure_value::JointKind) -> usize {
    use crate::loop_closure_value::JointKind;
    match kind {
        JointKind::Prismatic | JointKind::Revolute | JointKind::Coupling => 1,
        JointKind::Fixed => 0,
        JointKind::Cylindrical => 2,
        JointKind::Planar => 3,
        JointKind::Spherical => 3,
    }
}

/// A typed loop-closure chain pair extracted from a v0.2 Mechanism Map.
///
/// The classification is based on the closing joint (the last element of
/// `chain_b`):
/// - `WellFormed` — the closing joint appears exactly once in `chain_b`
///   (its last element only). Produced by the parent-conflict branch of
///   `append_body`. Valid linear kinematic chain, solver-feedable via
///   `chain_transform` / `solve_loop_closure` without further filtering.
/// - `Cycle` — the closing joint appears more than once in `chain_b`
///   (both at the end and at least once mid-walk). Produced by the
///   cycle/self-loop branch of `append_body`. **Not** a valid linear
///   kinematic chain — composing the same joint's transform twice in
///   different positions is not physically meaningful. Consumers must
///   not feed `Cycle` chains directly to `chain_transform` /
///   `solve_loop_closure`.
///
/// Both variants carry named fields `chain_a` and `chain_b` (world
/// sentinel stripped) so consumers can destructure unambiguously:
///
/// ```ignore
/// match chain {
///     LoopClosureChain::WellFormed { chain_a, chain_b } => { /* feed to solver */ }
///     LoopClosureChain::Cycle { .. } => { /* skip or handle separately */ }
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum LoopClosureChain {
    /// `chain_b` contains the closing joint exactly once (its last element).
    /// Produced by the parent-conflict branch of `append_body`.
    /// Solver-feedable via `chain_transform` / `solve_loop_closure`.
    WellFormed {
        chain_a: Vec<reify_types::Value>,
        chain_b: Vec<reify_types::Value>,
        /// The closing joint: propagated from the loop-closure record's
        /// `closing_joint` field so callers do not need `chain_b.last().unwrap()`
        /// (a partial function). Equals the last element of both `chain_a` and
        /// `chain_b` by the path invariant.
        closing_joint: reify_types::Value,
    },
    /// `chain_b` contains the closing joint more than once — at the end
    /// (the appended closing edge) and once mid-walk (as an ancestor of
    /// `parent` in the cycle case, or because `at == parent` in the
    /// self-loop case). NOT a valid linear kinematic chain. Produced by
    /// the cycle/self-loop branch of `append_body`.
    Cycle {
        chain_a: Vec<reify_types::Value>,
        chain_b: Vec<reify_types::Value>,
        /// The closing joint: propagated from the loop-closure record's
        /// `closing_joint` field so callers do not need `chain_b.last().unwrap()`
        /// (a partial function). Equals the last element of both `chain_a` and
        /// `chain_b` by the path invariant.
        closing_joint: reify_types::Value,
    },
}

/// Extract loop-closure chain pairs from a v0.2 Mechanism Map.
///
/// Returns `None` if `mech_map` is not a `Value::Map` with
/// `kind = "mechanism"` — i.e. the caller passed something that is not a
/// Mechanism.
///
/// Returns `Some(vec![])` for a valid open-chain Mechanism (no loop closures).
/// A missing `loop_closures` field is treated as an empty list as
/// defense-in-depth against hand-built Mechanism Maps (e.g. test fixtures)
/// that omit the field. `make_empty_mechanism` always emits the field, so
/// no Mechanism Map produced by the v0.2 builder will lack it; this branch
/// is only reachable from external/test callers.
///
/// For each loop-closure record in the `loop_closures` list, extracts
/// `path_a` and `path_b`, strips the world sentinel from the front of each
/// path, and classifies the resulting chain pair as a [`LoopClosureChain`].
/// The world sentinel is identified by `kind = "world"`.
///
/// **Classification rule:** a chain pair is [`LoopClosureChain::Cycle`] iff
/// `chain_b` contains its last element (the closing joint) more than once.
/// This subsumes both the 2-body cycle case (`chain_b = [j_b, j_a, j_b]`,
/// `j_b` twice) and the self-loop case (`chain_b = [j, j]`, `j` twice).
/// Parent-conflict pairs (`chain_b`'s last element occurs exactly once)
/// classify as [`LoopClosureChain::WellFormed`].
///
/// Returns `None` on any shape error:
/// - a `loop_closures` entry is not a `Value::Map`
/// - `path_a` or `path_b` fields are missing or not a `Value::List`
/// - either path has fewer than 2 elements (the stripped tail would not terminate at a closing joint)
/// - the first element of a path does not have `kind = "world"`
///
/// Downstream contract: chains terminate at the closing joint (the last
/// element equals `loop_closure.closing_joint`), world sentinel stripped.
pub fn mechanism_loop_closure_chains(
    mech_map: &reify_types::Value,
) -> Option<Vec<LoopClosureChain>> {
    use reify_types::Value;

    // Validate kind="mechanism".
    let map = match mech_map {
        Value::Map(m) => m,
        _ => return None,
    };
    if map.get(&Value::String("kind".to_string())) != Some(&Value::String("mechanism".to_string()))
    {
        return None;
    }

    // Read loop_closures list; treat missing field as empty as
    // defense-in-depth for hand-built test Maps (the v0.2 builder always
    // emits the field).
    let loop_closures: &[Value] = match map.get(&Value::String("loop_closures".to_string())) {
        Some(Value::List(lc)) => lc,
        None => &[],
        _ => return None, // present but wrong type
    };

    let mut pairs = Vec::new();
    for entry in loop_closures {
        let lc_map = match entry {
            Value::Map(m) => m,
            _ => return None,
        };

        let path_a = match lc_map.get(&Value::String("path_a".to_string())) {
            Some(Value::List(p)) => p,
            _ => return None,
        };
        let path_b = match lc_map.get(&Value::String("path_b".to_string())) {
            Some(Value::List(p)) => p,
            _ => return None,
        };

        let chain_a = strip_world_sentinel(path_a)?;
        let chain_b = strip_world_sentinel(path_b)?;

        // Extract closing_joint from the record's explicit field rather than
        // re-deriving it from chain_b.last() (a partial function). Production
        // records always carry this field; absence signals a malformed entry.
        let closing_joint = match lc_map.get(&Value::String("closing_joint".to_string())) {
            Some(v) => v.clone(),
            None => return None,
        };

        // Classify: Cycle iff the closing joint appears more than once in
        // chain_b. This subsumes the 2-body cycle ([j_b, j_a, j_b], j_b twice)
        // and the self-loop ([j, j], j twice) without a chain_b.last() call.
        let is_cycle = chain_b.iter().filter(|j| *j == &closing_joint).count() > 1;
        let entry = if is_cycle {
            LoopClosureChain::Cycle {
                chain_a,
                chain_b,
                closing_joint,
            }
        } else {
            LoopClosureChain::WellFormed {
                chain_a,
                chain_b,
                closing_joint,
            }
        };
        pairs.push(entry);
    }

    Some(pairs)
}

/// Strip the world sentinel from the front of a path, returning the tail.
///
/// Returns `None` if:
/// - the path is empty,
/// - the first element is not the world sentinel (`kind = "world"`),
/// - or the path has fewer than 2 elements (a chain stripped to empty
///   would not terminate at a closing joint, which violates the caller's
///   downstream contract — an empty chain cannot be fed to
///   `chain_transform` / `solve_loop_closure`).
fn strip_world_sentinel(path: &[reify_types::Value]) -> Option<Vec<reify_types::Value>> {
    use reify_types::Value;

    // Reject `[world]` and shorter — the stripped tail would be empty,
    // violating the contract that returned chains terminate at the
    // closing joint.
    if path.len() < 2 {
        return None;
    }

    let first = path.first()?;
    let is_world = match first {
        Value::Map(m) => {
            m.get(&Value::String("kind".to_string())) == Some(&Value::String("world".to_string()))
        }
        _ => false,
    };
    if !is_world {
        return None;
    }
    Some(path[1..].to_vec())
}

/// Hard-coded number of components in a single-loop closure residual.
///
/// The closure residual is a stacked twist `[ω_x, ω_y, ω_z, v_x, v_y, v_z]`
/// (6 components per loop).  `solve_loop_closure` is single-loop today
/// (task 2584's MVP scope), so the residual is exactly 6 entries.  Multi-loop
/// stacking is deferred (PRD task 10 / future work).
///
/// `solve_loop_closure_with_diagnostics` compares `free_b.len()` against this
/// constant for its over/under-constrained pre-check; the assumption that
/// `free_b.len()` is a faithful free-DOF count holds because
/// `chain_jacobian_fd` already rejects chains containing multi-DOF joints
/// (planar/spherical/cylindrical) by returning `None`, so the only chains
/// the solver accepts are 1-DOF (prismatic, revolute, coupling) plus 0-DOF
/// (fixed).
const SINGLE_LOOP_RESIDUAL_COUNT: usize = 6;

/// Diagnostic-emitting wrapper around [`solve_loop_closure`].
///
/// Adds three pre-/post-process steps on top of the chain-based Newton
/// solve, each translating a runtime condition into a typed [`Diagnostic`]:
///
/// 1. **Over-constrained pre-check** — if `free_b.len() < 6` (`= SINGLE_LOOP_RESIDUAL_COUNT`),
///    the wrapper short-circuits the Newton solve and returns
///    [`NewtonOutcome::NotConverged`] with `residual_norm: f64::INFINITY`,
///    accompanied by a [`DiagnosticCode::KinematicOverconstrained`] Error.
///    The diagnostic, not a plausible-looking config, is the user-facing
///    signal of structural infeasibility per the PRD prose.
/// 2. **Under-constrained pre-check** — if `free_b.len() > 6`, the wrapper
///    emits a [`DiagnosticCode::KinematicUnderconstrained`] Warning and
///    delegates to [`solve_loop_closure`].  The "closest-to-previous config"
///    semantics the PRD describes are realised by the caller's choice of
///    [`StartStrategy::WarmStart`].
///    *(Wired in step-8 of task 2677.)*
/// 3. **Singular post-process** — if the delegated Newton outcome is
///    [`NewtonOutcome::Singular`], the wrapper appends a
///    [`DiagnosticCode::KinematicSingularity`] Warning.  The `Singular`
///    variant's `x` payload carries the last-converged config the PRD
///    requires; [`LoopClosureReport::is_singular()`] returns `true`
///    automatically because the accessor derives from the `outcome` tag.
///    *(Wired in step-10 of task 2677.)*
///
/// **Single-loop assumption** — `solve_loop_closure` builds a 6-component
/// twist residual against one closure constraint.  The free-DOF balance
/// check therefore hard-codes `SINGLE_LOOP_RESIDUAL_COUNT = 6`.  Multi-loop
/// generalisation is deferred to the [`newton_solve`] core (which is
/// already generic over residual shape) plus a future caller that stacks
/// residuals from multiple loops.
///
/// **1-DOF chain assumption** — `chain_jacobian_fd` returns `None` for
/// chains containing planar/spherical/cylindrical (multi-DOF) joints, so
/// `free_b.len()` is a faithful free-DOF count for the chains the existing
/// solver supports.
///
/// See `docs/prds/v0_2/kinematic-constraints.md` §"Singularity,
/// over/under-constraint diagnostics" and the [`LoopClosureReport`] type
/// for the canonical return shape.
///
/// [`Diagnostic`]: reify_types::Diagnostic
/// [`DiagnosticCode::KinematicOverconstrained`]: reify_types::DiagnosticCode::KinematicOverconstrained
/// [`DiagnosticCode::KinematicUnderconstrained`]: reify_types::DiagnosticCode::KinematicUnderconstrained
/// [`DiagnosticCode::KinematicSingularity`]: reify_types::DiagnosticCode::KinematicSingularity
pub fn solve_loop_closure_with_diagnostics(
    chain_a: &[reify_types::Value],
    vals_a: &[crate::loop_closure_value::JointValue],
    chain_b: &[reify_types::Value],
    vals_b_initial: &[crate::loop_closure_value::JointValue],
    free_b: &[usize],
    strategy: &StartStrategy,
    config: &NewtonConfig,
) -> LoopClosureReport {
    // Both entry points share `validate_loop_closure_inputs` as their single
    // validation source of truth.  Running validation BEFORE the DOF-balance
    // check means a caller that passes structurally over-constrained AND
    // malformed inputs receives the more accurate `InvalidInput` outcome
    // rather than `KinematicOverconstrained` — and short-circuiting before
    // validation would let out-of-range `free_b` indices silently shrink the
    // returned `x` (a length-mismatch contract violation).
    if let Some(reason) = validate_loop_closure_inputs(chain_b, vals_b_initial, free_b, strategy) {
        tracing::warn!("solve_loop_closure_with_diagnostics: {reason}");
        return LoopClosureReport {
            outcome: NewtonOutcome::InvalidInput { reason },
            diagnostics: Vec::new(),
        };
    }

    let mut diagnostics: Vec<reify_types::Diagnostic> = Vec::new();

    // KCC-γ step-12: count manifold DOF (`dof_count`) rather than free_b
    // slots so a 3-DOF planar joint balances correctly against the 6-component
    // loop residual.  `dof_count_for_balance` returns 1/1/1/0/2/3/3 for
    // prismatic/revolute/coupling/fixed/cylindrical/planar/spherical (manifold
    // DOF; fixed is 0-DOF and does not contribute to free DOFs).
    //
    // The "any multi-DOF joint present" discriminator distinguishes:
    //   - 1 prismatic vs 6 (1 < 6, single-DOF only) → over-constrained
    //     short-circuit: 1 scalar free var cannot possibly span the 6-D
    //     residual subspace, so the Newton solve would be pointless.
    //   - 1 planar vs 6 (3 < 6, includes multi-DOF) → delegate: a planar
    //     joint can satisfy any planar (3-D) closure residual; the Newton
    //     solve discovers whether the loop's actual residual subspace falls
    //     within the planar reach.  The PRD §11.1 producer-side scenario
    //     "Multi-DOF closed chain converges" exercises exactly this case.
    let (free_dof_count, any_multi_dof): (usize, bool) = match free_joint_shapes(chain_b, free_b) {
        Some(shapes) => {
            let dof: usize = shapes.iter().map(|k| dof_count_for_balance(*k)).sum();
            let any_multi = shapes.iter().any(|k| dof_count_for_balance(*k) > 1);
            (dof, any_multi)
        }
        None => {
            // free_b joint has an unknown kind — defer to the inner solver,
            // which returns InvalidInput with a more specific reason.  The
            // diagnostic path treats this as 0 free DOFs (over-constrained
            // pre-check would fire but the inner solver's InvalidInput trumps).
            (0, false)
        }
    };

    if free_dof_count < SINGLE_LOOP_RESIDUAL_COUNT && !any_multi_dof {
        // Over-constrained AND every free joint is single-DOF: short-circuit
        // Newton; the diagnostic IS the signal.  Multi-DOF joints (planar/
        // spherical/cylindrical) skip this branch and delegate to the solver
        // even when free_dof_count < 6 — see the comment above the
        // `(free_dof_count, any_multi_dof)` derivation.
        let diag = reify_types::Diagnostic::error(format!(
            "kinematic system over-constrained: {} free DOFs vs {} loop residuals",
            free_dof_count,
            SINGLE_LOOP_RESIDUAL_COUNT
        ))
        .with_code(reify_types::DiagnosticCode::KinematicOverconstrained);
        diagnostics.push(diag);

        // Resolve the returned `x` from the strategy.  Inputs are validated
        // above, so each branch produces a vector of length matching the
        // Newton state width — preserving the implicit contract that
        // `outcome.x` aligns positionally with the requested free vars.
        // Precise contents matter less than the diagnostic itself
        // (residual_norm is f64::INFINITY and downstream tooling treats the
        // diagnostic as the user-facing signal); the length invariant is
        // what callers index against.
        let x: Vec<f64> = match strategy {
            StartStrategy::WarmStart(v) => v.clone(),
            StartStrategy::Midpoint => {
                let mut x = Vec::new();
                for &i in free_b {
                    let mid = crate::loop_closure::joint_range_midpoint(&chain_b[i])
                        .expect("joint_range_midpoint validated above");
                    x.extend_from_slice(mid.as_f64_slice());
                }
                x
            }
        };

        return LoopClosureReport {
            outcome: NewtonOutcome::NotConverged {
                x,
                residual_norm: f64::INFINITY,
            },
            diagnostics,
        };
    }

    if free_dof_count > SINGLE_LOOP_RESIDUAL_COUNT {
        // Under-constrained: Newton still runs (Gauss-Newton with WarmStart
        // converges to the local minimum closest to the warm-started point —
        // that IS the PRD's "closest-to-previous config" semantics).  The
        // warning gives the user a signal that the mechanism is structurally
        // under-determined and might want an explicit binding.
        let diag = reify_types::Diagnostic::warning(format!(
            "kinematic system under-constrained: {} free DOFs vs {} loop residuals; consider adding an explicit binding",
            free_dof_count,
            SINGLE_LOOP_RESIDUAL_COUNT
        ))
        .with_code(reify_types::DiagnosticCode::KinematicUnderconstrained);
        diagnostics.push(diag);
    } else if free_dof_count < SINGLE_LOOP_RESIDUAL_COUNT && any_multi_dof {
        // KCC-γ step-12: multi-DOF free joint(s) with total DOF count below
        // the 6-component loop residual.  We DELEGATE rather than short-
        // circuit (a planar/spherical/cylindrical joint can satisfy any
        // residual that falls within its motion subspace; only the inner
        // Newton solve discovers whether the loop's actual residual is
        // reachable).  Emit an under-constrained warning so the user sees
        // the structural imbalance signal regardless of solver outcome.
        let diag = reify_types::Diagnostic::warning(format!(
            "kinematic system under-constrained: {} free DOFs vs {} loop residuals; multi-DOF joint(s) may still satisfy a reduced-subspace residual",
            free_dof_count,
            SINGLE_LOOP_RESIDUAL_COUNT
        ))
        .with_code(reify_types::DiagnosticCode::KinematicUnderconstrained);
        diagnostics.push(diag);
    }

    // Balanced (== 6), under-constrained (> 6), or multi-DOF imbalanced
    // (< 6 with planar/spherical/cylindrical free joints).  Delegate to the
    // existing solver; post-process the singular outcome.
    let outcome = solve_loop_closure(
        chain_a,
        vals_a,
        chain_b,
        vals_b_initial,
        free_b,
        strategy,
        config,
    );

    // Singular post-process: translate NewtonOutcome::Singular into the
    // PRD's W_KINEMATIC_SINGULARITY warning class.  The Singular variant's
    // `x` payload already carries the last-converged config the PRD
    // requires; the wrapper's only job is to surface the typed diagnostic
    // alongside the outcome.  Other outcomes (Converged / NotConverged /
    // InvalidInput) add no singularity entry; `is_singular()` derives from
    // the `outcome` tag at the type level — one source of truth.
    if matches!(outcome, NewtonOutcome::Singular { .. }) {
        let diag = reify_types::Diagnostic::warning(
            "kinematic singularity detected: rank-deficient Jacobian; last-converged config returned",
        )
        .with_code(reify_types::DiagnosticCode::KinematicSingularity);
        diagnostics.push(diag);
    }

    LoopClosureReport {
        outcome,
        diagnostics,
    }
}

/// Single validation entry point used by both [`solve_loop_closure`] and
/// [`solve_loop_closure_with_diagnostics`].  Returns `Some(reason)` describing
/// the first failed check, or `None` if every input is well-formed.
///
/// Checks: every `free_b` index addresses a valid joint in `chain_b` AND a
/// valid initial value in `vals_b_initial`, the `WarmStart` vector length
/// matches `free_b.len()`, and `Midpoint`'s joint-range lookup succeeds for
/// each free joint.  The diagnostic wrapper short-circuits on DOF balance only
/// AFTER this validation passes, so a structurally over-constrained AND
/// malformed input surfaces `InvalidInput` (the more accurate signal) rather
/// than `KinematicOverconstrained`.
fn validate_loop_closure_inputs(
    chain_b: &[reify_types::Value],
    vals_b_initial: &[crate::loop_closure_value::JointValue],
    free_b: &[usize],
    strategy: &StartStrategy,
) -> Option<String> {
    for &i in free_b {
        if i >= chain_b.len() {
            return Some(format!(
                "free_b index {} out of range (chain_b len {})",
                i,
                chain_b.len()
            ));
        }
        if i >= vals_b_initial.len() {
            return Some(format!(
                "free_b index {} out of range (vals_b_initial len {})",
                i,
                vals_b_initial.len()
            ));
        }
    }
    match strategy {
        StartStrategy::WarmStart(v) => {
            // KCC-γ step-12: WarmStart carries the flat Newton state — its
            // length is `Σ_{k ∈ free_b} JointKind::flat_len(chain_b[k].kind)`,
            // NOT `free_b.len()`.  Compute the expected width here so a
            // single-DOF chain still validates as len == free_b.len() and a
            // multi-DOF chain validates against the wider Newton state.
            let expected: usize = match free_joint_shapes(chain_b, free_b) {
                Some(shapes) => shapes.iter().map(|k| k.flat_len()).sum(),
                None => {
                    return Some(
                        "free_b joint kind is unknown to JointKind::from_str".to_string(),
                    );
                }
            };
            if v.len() != expected {
                return Some(format!(
                    "WarmStart length {} != free_b length {}",
                    v.len(),
                    expected,
                ));
            }
        }
        StartStrategy::Midpoint => {
            for &i in free_b {
                // free_b indices already validated above; `chain_b[i]` is safe.
                if crate::loop_closure::joint_range_midpoint(&chain_b[i]).is_none() {
                    return Some(format!(
                        "joint_range_midpoint returned None for free_b[{i}] — joint missing range or malformed"
                    ));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loop_closure_value::JointValue;

    // ── Public type API surface (step-11) ──────────────────────────────

    #[test]
    fn newton_config_default_values() {
        let cfg = NewtonConfig::default();
        assert_eq!(cfg.tol_pos_m, 1e-6);
        assert_eq!(cfg.tol_rot_rad, 1e-6);
        assert_eq!(cfg.max_iters, 50);
        assert_eq!(cfg.singularity_pivot_eps, 1e-12);
    }

    // ── newton_solve tests (step-13) ────────────────────────────────────

    /// Build a residual+jacobian closure for a 1-D linear residual r(x) = x - target.
    /// J column shape: [0,0,0, 1,0,0] (linear in x).
    #[allow(clippy::type_complexity)] // test helper; unwrapping into a type alias would reduce clarity
    fn linear_1d_closure(target: f64) -> impl FnMut(&[f64]) -> Option<(Vec<f64>, Vec<Vec<f64>>)> {
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

    // ── Off-diagonal JᵀJ regression guard (step-3) ─────────────────────

    #[test]
    fn newton_solve_2d_off_diagonal_jtj_converges() {
        // Construct a 2-free-variable problem where the two Jacobian columns
        // share components, so JᵀJ has a non-zero off-diagonal.
        //
        // Two stacked 12-element residuals (two "loops"):
        //   r[3]  = 2*x[0] + x[1] - 4   (linear-x component of loop 0)
        //   r[9]  = x[0]  + 3*x[1] - 5  (linear-x component of loop 1)
        //
        // Columns of the Jacobian:
        //   c0 = [..., 2, ..., 1, ...]  (dr/dx0)
        //   c1 = [..., 1, ..., 3, ...]  (dr/dx1)
        //
        // JᵀJ = [[c0·c0, c0·c1], [c1·c0, c1·c1]] = [[5, 5], [5, 10]]
        //   → off-diagonal entry of 5, so LDLᵀ correctness is exercised on a
        //     JᵀJ whose lower-triangle-only storage carries a real off-diagonal
        //     entry (rather than the trivial diagonal case).
        //
        // Closed-form root: 2x+y=4, x+3y=5 ⟹ x=1.4, y=1.2.
        let cfg = NewtonConfig::default();
        let closure = |x: &[f64]| -> Option<(Vec<f64>, Vec<Vec<f64>>)> {
            assert_eq!(x.len(), 2);
            let mut r = vec![0.0; 12];
            r[3] = 2.0 * x[0] + x[1] - 4.0;
            r[9] = x[0] + 3.0 * x[1] - 5.0;
            let mut c0 = vec![0.0; 12];
            c0[3] = 2.0;
            c0[9] = 1.0;
            let mut c1 = vec![0.0; 12];
            c1[3] = 1.0;
            c1[9] = 3.0;
            Some((r, vec![c0, c1]))
        };
        let outcome = newton_solve(vec![0.0, 0.0], closure, &cfg);
        match outcome {
            NewtonOutcome::Converged {
                x,
                iters,
                residual_norm,
            } => {
                assert!(
                    (x[0] - 1.4).abs() < 1e-9,
                    "expected x[0] ≈ 1.4, got {}",
                    x[0]
                );
                assert!(
                    (x[1] - 1.2).abs() < 1e-9,
                    "expected x[1] ≈ 1.2, got {}",
                    x[1]
                );
                assert!(iters >= 1, "expected at least 1 Newton iteration");
                assert!(
                    residual_norm < 1e-8,
                    "expected tight residual at convergence, got {residual_norm}"
                );
            }
            other => panic!("expected Converged, got {other:?}"),
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
            NewtonOutcome::Converged {
                x,
                iters,
                residual_norm,
            } => {
                assert!((x[0] - 2.0).abs() < 1e-6, "expected x≈2.0, got {}", x[0]);
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
                    (residual_norm - 4.0).abs() < 1e-12,
                    "expected residual_norm == 4.0 at divergence bail-out (iter 3 ramp = 1→2→3→4), got {residual_norm}"
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

    use crate::eval_builtin;
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
        // α-pre bridge: per-joint motion values typed as Vec<JointValue>
        // (all `Scalar` for these scalar-joint chains).  solve_loop_closure
        // still consumes `&[f64]` in α-pre, so the values pass through
        // `flatten_dofs` at the call boundary.  flatten_dofs of a single
        // `Scalar(0.5)` is exactly `vec![0.5]`, so behaviour is preserved.
        let vals_a = vec![JointValue::Scalar(0.5)];
        let chain_b = vec![prismatic_x_0_to_1()];
        let vals_b_initial = vec![JointValue::Scalar(0.0)];
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
        let vals_a = vec![JointValue::Scalar(0.5)];
        let chain_b = vec![prismatic_x_0_to_1()];
        let vals_b_initial = vec![JointValue::Scalar(0.0)];
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
        let vals_a = vec![JointValue::Scalar(0.5)];
        let chain_b = vec![prismatic_x_0_to_1()];
        let vals_b_initial = vec![JointValue::Scalar(0.499)];
        let free_b = vec![0];
        // WarmStart payload is an abstract Newton-state seed (NOT a chain
        // motion-value vector — its length matches `free_b`, not chain_b),
        // so it stays `Vec<f64>` per the α-pre scope.
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
        let vals_a = vec![JointValue::Scalar(0.5)];
        let chain_b = vec![prismatic_x_0_to_1()];
        let vals_b_initial = vec![JointValue::Scalar(0.499)];
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
        let vals_a = vec![JointValue::Scalar(theta_a), JointValue::Scalar(0.5)];
        let chain_b = vec![revolute_z_0_to_pi(), prismatic_x_0_to_1()];
        let vals_b_initial = vec![JointValue::Scalar(0.0), JointValue::Scalar(0.0)];
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
            NewtonOutcome::Converged {
                x,
                iters: _,
                residual_norm,
            } => {
                assert_eq!(x.len(), 2);
                // Rotation about z must match.
                assert!(
                    (x[0] - theta_a).abs() < 1e-5,
                    "expected θ ≈ {theta_a}, got {}",
                    x[0]
                );
                // Prismatic length must match.
                assert!((x[1] - 0.5).abs() < 1e-5, "expected t ≈ 0.5, got {}", x[1]);
                assert!(
                    residual_norm < 1e-6,
                    "expected tight residual after convergence, got {residual_norm}"
                );
            }
            other => panic!("expected Converged on revolute+prismatic loop closure, got {other:?}"),
        }
    }

    // ── InvalidInput contract tests (suggestion 7) ──────────────────────

    #[test]
    fn solve_loop_closure_warm_start_length_mismatch_returns_invalid_input() {
        let chain_a = vec![prismatic_x_0_to_1()];
        let vals_a = vec![JointValue::Scalar(0.5)];
        let chain_b = vec![prismatic_x_0_to_1()];
        let vals_b_initial = vec![JointValue::Scalar(0.0)];
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
        let vals_a = vec![JointValue::Scalar(0.5)];
        let chain_b = vec![prismatic_x_0_to_1()];
        let vals_b_initial = vec![JointValue::Scalar(0.0)];
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
    fn solve_loop_closure_warm_start_free_b_out_of_range_returns_invalid_input() {
        let chain_a = vec![prismatic_x_0_to_1()];
        let vals_a = vec![JointValue::Scalar(0.5)];
        let chain_b = vec![prismatic_x_0_to_1()];
        let vals_b_initial = vec![JointValue::Scalar(0.0)];
        // free_b index 5 is out of range for chain_b of length 1.
        let free_b = vec![5];
        // WarmStart vec has length 1 to match free_b len — so the length-mismatch
        // guard does NOT pre-empt the new chain_b bound check.
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
            NewtonOutcome::InvalidInput { reason } => {
                // Must pin the *chain_b* guard specifically — both new validation
                // messages contain "out of range", so asserting "chain_b" ensures
                // this test exercises the right bound.
                assert!(
                    reason.contains("chain_b"),
                    "expected reason to mention chain_b, got {reason:?}"
                );
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn solve_loop_closure_warm_start_free_b_index_exceeds_vals_b_initial_returns_invalid_input() {
        let chain_a = vec![prismatic_x_0_to_1()];
        let vals_a = vec![JointValue::Scalar(0.5)];
        // chain_b has 2 joints so free_b[0]=1 passes the chain_b bound check.
        let chain_b = vec![prismatic_x_0_to_1(), prismatic_x_0_to_1()];
        // vals_b_initial has only 1 entry — free_b[0]=1 is OOB here.
        let vals_b_initial = vec![JointValue::Scalar(0.0)];
        let free_b = vec![1];
        // WarmStart vec length 1 matches free_b len — length-mismatch guard does not fire.
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
            NewtonOutcome::InvalidInput { reason } => {
                assert!(
                    reason.contains("vals_b_initial"),
                    "expected reason to mention vals_b_initial, got {reason:?}"
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
        let vals_a = vec![JointValue::Scalar(0.5)];
        let chain_b = vec![bad_joint];
        let vals_b_initial = vec![JointValue::Scalar(0.0)];
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
        let vals_a = vec![JointValue::Scalar(0.5)];
        let chain_b = vec![prismatic_x_0_to_1()];
        let vals_b_initial = vec![JointValue::Scalar(0.0)]; // FREE var
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

    // ── solve_normal_equations flat-buffer unit tests (step-1) ────────────

    #[test]
    fn solve_normal_equations_flat_solves_pd_2x2_in_place() {
        // A = [[2, 1], [1, 3]] (row-major: [2.0, 1.0, 1.0, 3.0])
        // b = [4, 5]  →  Ax = b  →  x = [1.4, 1.2]
        // (2x+y=4, x+3y=5 ⟹ x=1.4, y=1.2)
        let mut a = vec![2.0_f64, 1.0, 1.0, 3.0];
        let mut b = vec![4.0_f64, 5.0];
        let result = super::solve_normal_equations(&mut a, &mut b, 2, 1e-12, &[]);
        assert!(result, "expected solve to succeed on PD matrix");
        assert!(
            (b[0] - 1.4).abs() < 1e-9,
            "expected b[0] ≈ 1.4, got {}",
            b[0]
        );
        assert!(
            (b[1] - 1.2).abs() < 1e-9,
            "expected b[1] ≈ 1.2, got {}",
            b[1]
        );
    }

    #[test]
    fn solve_normal_equations_flat_singular_returns_false() {
        // A = [[1, 2], [2, 4]] — rank-1 (singular): D[1,1] = 4 - 2²·1 = 0.
        let mut a = vec![1.0_f64, 2.0, 2.0, 4.0];
        let mut b = vec![1.0_f64, 2.0];
        let result = super::solve_normal_equations(&mut a, &mut b, 2, 1e-12, &[]);
        assert!(!result, "expected solve to fail on singular matrix");
    }

    #[test]
    fn solve_normal_equations_flat_n_zero_returns_true() {
        // n = 0 edge case: empty slices must return true immediately.
        let mut a: Vec<f64> = vec![];
        let mut b: Vec<f64> = vec![];
        let result = super::solve_normal_equations(&mut a, &mut b, 0, 1e-12, &[]);
        assert!(result, "expected solve to succeed on n=0 (trivial case)");
    }

    // ── mechanism_loop_closure_chains tests (step-7) ─────────────────────

    /// `mechanism_loop_closure_chains` on a closed-chain mechanism returns
    /// `Some(vec![LoopClosureChain::WellFormed { chain_a, chain_b }])` with
    /// the world sentinel stripped from each path and the chains terminating
    /// at the closing joint.
    ///
    /// Scenario: parent-conflict via `body(m0, solid_a, j_x, j_a)` then
    /// `body(m1, solid_b, j_x, j_b)`. The expected paths are:
    ///   path_a = [world, j_a, j_x]  (recorded by body() for parent j_a)
    ///   path_b = [world, j_b, j_x]  (recorded by body() for parent j_b)
    /// After world-sentinel stripping:
    ///   chain_a = [j_a, j_x]
    ///   chain_b = [j_b, j_x]
    /// j_x appears exactly once in chain_b → WellFormed.
    #[test]
    fn mechanism_loop_closure_chains_extracts_pairs() {
        use crate::eval_builtin;

        // Build joints (from existing test helpers in this file).
        let j_a = eval_builtin("prismatic", &[axis_x(), length_range(0.0, 1.0)]);
        let j_b = eval_builtin(
            "prismatic",
            &[
                Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]),
                length_range(0.0, 1.0),
            ],
        );
        let j_x = revolute_z_0_to_pi();
        let solid_a = Value::String("solidA".to_string());
        let solid_b = Value::String("solidB".to_string());

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin("body", &[m0, solid_a, j_x.clone(), j_a.clone()]);
        let m2 = eval_builtin("body", &[m1, solid_b, j_x.clone(), j_b.clone()]);

        let chains = super::mechanism_loop_closure_chains(&m2);
        assert!(
            chains.is_some(),
            "mechanism_loop_closure_chains must return Some for a valid mechanism"
        );
        let pairs = chains.unwrap();
        assert_eq!(pairs.len(), 1, "one loop-closure pair expected");

        let (chain_a, chain_b, cj) = match &pairs[0] {
            super::LoopClosureChain::WellFormed {
                chain_a,
                chain_b,
                closing_joint,
            } => (chain_a, chain_b, closing_joint),
            other => panic!("expected WellFormed, got {:?}", other),
        };
        // chain_a = [j_a, j_x] (world sentinel stripped from [world, j_a, j_x])
        assert_eq!(chain_a.len(), 2, "chain_a should have 2 elements");
        assert_eq!(&chain_a[0], &j_a, "chain_a[0] should be j_a");
        assert_eq!(
            &chain_a[1], &j_x,
            "chain_a[1] should be j_x (closing joint)"
        );
        // chain_b = [j_b, j_x] (world sentinel stripped from [world, j_b, j_x])
        assert_eq!(chain_b.len(), 2, "chain_b should have 2 elements");
        assert_eq!(&chain_b[0], &j_b, "chain_b[0] should be j_b");
        assert_eq!(
            &chain_b[1], &j_x,
            "chain_b[1] should be j_x (closing joint)"
        );
        // closing_joint is propagated from the loop-closure record.
        assert_eq!(cj, &j_x, "closing_joint should be j_x");
    }

    /// `mechanism_loop_closure_chains` on an open-chain mechanism (no loop
    /// closures) returns `Some(vec![])` — an empty list of pairs.
    #[test]
    fn mechanism_loop_closure_chains_open_chain_returns_empty_vec() {
        use crate::eval_builtin;

        let j_a = prismatic_x_0_to_1();
        let solid_a = Value::String("solidA".to_string());

        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin("body", &[m0, solid_a, j_a]);

        let chains = super::mechanism_loop_closure_chains(&m1);
        assert_eq!(
            chains,
            Some(vec![]),
            "open-chain mechanism must return Some(empty vec)"
        );
    }

    /// `mechanism_loop_closure_chains` on a non-Mechanism value returns `None`.
    #[test]
    fn mechanism_loop_closure_chains_non_mechanism_returns_none() {
        // Non-Map.
        assert_eq!(
            super::mechanism_loop_closure_chains(&Value::Int(42)),
            None,
            "Int input must return None"
        );

        // Map with wrong kind.
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("kind".to_string()),
            Value::String("joint".to_string()),
        );
        assert_eq!(
            super::mechanism_loop_closure_chains(&Value::Map(m)),
            None,
            "Map with kind='joint' (not 'mechanism') must return None"
        );

        // World sentinel.
        let world = crate::eval_builtin("world", &[]);
        assert_eq!(
            super::mechanism_loop_closure_chains(&world),
            None,
            "world sentinel must return None"
        );
    }

    /// `mechanism_loop_closure_chains` accumulates ALL loop-closure entries
    /// in iteration order. A regression that returned only the first entry
    /// (or dropped a later entry) would be caught here.
    #[test]
    fn mechanism_loop_closure_chains_extracts_multiple_pairs() {
        use std::collections::BTreeMap;

        // Build joint Maps used as path elements.
        let world = crate::eval_builtin("world", &[]);
        let j_a = prismatic_x_0_to_1();
        let j_b = revolute_z_0_to_pi();
        let j_x = Value::Map({
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("joint".to_string()),
            );
            m.insert(
                Value::String("tag".to_string()),
                Value::String("x".to_string()),
            );
            m
        });
        let j_y = Value::Map({
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("joint".to_string()),
            );
            m.insert(
                Value::String("tag".to_string()),
                Value::String("y".to_string()),
            );
            m
        });

        // Hand-construct a Mechanism Map with two loop_closure records.
        let mut lc1 = BTreeMap::new();
        lc1.insert(
            Value::String("kind".to_string()),
            Value::String("loop_closure".to_string()),
        );
        lc1.insert(Value::String("body_id".to_string()), Value::Int(1));
        lc1.insert(Value::String("closing_joint".to_string()), j_x.clone());
        lc1.insert(
            Value::String("path_a".to_string()),
            Value::List(vec![world.clone(), j_a.clone(), j_x.clone()]),
        );
        lc1.insert(
            Value::String("path_b".to_string()),
            Value::List(vec![world.clone(), j_b.clone(), j_x.clone()]),
        );

        let mut lc2 = BTreeMap::new();
        lc2.insert(
            Value::String("kind".to_string()),
            Value::String("loop_closure".to_string()),
        );
        lc2.insert(Value::String("body_id".to_string()), Value::Int(2));
        lc2.insert(Value::String("closing_joint".to_string()), j_y.clone());
        lc2.insert(
            Value::String("path_a".to_string()),
            Value::List(vec![world.clone(), j_a.clone(), j_y.clone()]),
        );
        lc2.insert(
            Value::String("path_b".to_string()),
            Value::List(vec![world.clone(), j_b.clone(), j_y.clone()]),
        );

        let mut mech = BTreeMap::new();
        mech.insert(
            Value::String("kind".to_string()),
            Value::String("mechanism".to_string()),
        );
        mech.insert(
            Value::String("loop_closures".to_string()),
            Value::List(vec![Value::Map(lc1), Value::Map(lc2)]),
        );

        let chains = super::mechanism_loop_closure_chains(&Value::Map(mech));
        let pairs = chains.expect("two-entry mechanism must return Some");
        assert_eq!(pairs.len(), 2, "both loop-closure entries must surface");

        // First pair: chain_a = [j_a, j_x], chain_b = [j_b, j_x], closing_joint = j_x.
        // j_x appears exactly once in chain_b → WellFormed.
        let (chain_a0, chain_b0, cj0) = match &pairs[0] {
            super::LoopClosureChain::WellFormed {
                chain_a,
                chain_b,
                closing_joint,
            } => (chain_a, chain_b, closing_joint),
            other => panic!("expected WellFormed for first pair, got {:?}", other),
        };
        assert_eq!(chain_a0, &vec![j_a.clone(), j_x.clone()]);
        assert_eq!(chain_b0, &vec![j_b.clone(), j_x.clone()]);
        assert_eq!(cj0, &j_x, "first pair closing_joint should be j_x");

        // Second pair: chain_a = [j_a, j_y], chain_b = [j_b, j_y], closing_joint = j_y.
        // j_y appears exactly once in chain_b → WellFormed.
        let (chain_a1, chain_b1, cj1) = match &pairs[1] {
            super::LoopClosureChain::WellFormed {
                chain_a,
                chain_b,
                closing_joint,
            } => (chain_a, chain_b, closing_joint),
            other => panic!("expected WellFormed for second pair, got {:?}", other),
        };
        assert_eq!(chain_a1, &vec![j_a.clone(), j_y.clone()]);
        assert_eq!(chain_b1, &vec![j_b.clone(), j_y.clone()]);
        assert_eq!(cj1, &j_y, "second pair closing_joint should be j_y");
    }

    /// A malformed second loop-closure entry (e.g. missing `path_a`) makes
    /// the whole call fail with `None`. Pins the early-exit-via-`?`
    /// contract — we don't leak a partial-accumulation result that
    /// includes only the first (well-formed) pair.
    #[test]
    fn mechanism_loop_closure_chains_malformed_second_entry_returns_none() {
        use std::collections::BTreeMap;

        let world = crate::eval_builtin("world", &[]);
        let j_a = prismatic_x_0_to_1();
        let j_b = revolute_z_0_to_pi();
        let j_x = Value::Map({
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("joint".to_string()),
            );
            m.insert(
                Value::String("tag".to_string()),
                Value::String("x".to_string()),
            );
            m
        });

        // Well-formed first entry.
        let mut lc1 = BTreeMap::new();
        lc1.insert(
            Value::String("kind".to_string()),
            Value::String("loop_closure".to_string()),
        );
        lc1.insert(Value::String("body_id".to_string()), Value::Int(1));
        lc1.insert(Value::String("closing_joint".to_string()), j_x.clone());
        lc1.insert(
            Value::String("path_a".to_string()),
            Value::List(vec![world.clone(), j_a.clone(), j_x.clone()]),
        );
        lc1.insert(
            Value::String("path_b".to_string()),
            Value::List(vec![world.clone(), j_b.clone(), j_x.clone()]),
        );

        // Second entry: missing `path_a` field.
        let mut lc2 = BTreeMap::new();
        lc2.insert(
            Value::String("kind".to_string()),
            Value::String("loop_closure".to_string()),
        );
        lc2.insert(
            Value::String("path_b".to_string()),
            Value::List(vec![world.clone(), j_b.clone(), j_x.clone()]),
        );

        let mut mech = BTreeMap::new();
        mech.insert(
            Value::String("kind".to_string()),
            Value::String("mechanism".to_string()),
        );
        mech.insert(
            Value::String("loop_closures".to_string()),
            Value::List(vec![Value::Map(lc1), Value::Map(lc2)]),
        );

        assert_eq!(
            super::mechanism_loop_closure_chains(&Value::Map(mech)),
            None,
            "a malformed second entry must fail the whole call"
        );
    }

    /// Defense-in-depth: a hand-built Mechanism Map that omits the
    /// `loop_closures` field is treated as having no loop closures (the
    /// v0.2 builder always emits the field, so this only matters for
    /// hand-constructed callers). Pins `None => &[]` in the dispatch to
    /// guard against a regression that flipped that branch to
    /// `None => return None`.
    #[test]
    fn mechanism_loop_closure_chains_missing_field_returns_empty_vec() {
        use std::collections::BTreeMap;

        let mut mech = BTreeMap::new();
        mech.insert(
            Value::String("kind".to_string()),
            Value::String("mechanism".to_string()),
        );
        // Intentionally omit `loop_closures` (and other fields not consulted
        // by `mechanism_loop_closure_chains`).

        assert_eq!(
            super::mechanism_loop_closure_chains(&Value::Map(mech)),
            Some(vec![]),
            "a Mechanism Map without the loop_closures field must yield Some(empty)"
        );
    }

    /// Regression pin for the missing-`closing_joint` guard in
    /// `mechanism_loop_closure_chains`.
    ///
    /// A loop-closure entry that carries `kind`, `body_id`, `path_a`, `path_b`
    /// but intentionally omits `closing_joint` must cause the whole call to
    /// return `None`.
    ///
    /// # Warning
    /// Do NOT change the guard to `closing_joint = chain_b.last().cloned()`.
    /// `chain_b.last()` is a partial function — it would return `None` on an
    /// empty chain and silently mis-classify cycles.  The explicit `closing_joint`
    /// field is the single source of truth; absence signals a malformed record.
    #[test]
    fn mechanism_loop_closure_chains_missing_closing_joint_returns_none() {
        use std::collections::BTreeMap;

        let world = crate::eval_builtin("world", &[]);
        let j_a = prismatic_x_0_to_1();
        let j_b = revolute_z_0_to_pi();
        let j_x = Value::Map({
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("kind".to_string()),
                Value::String("joint".to_string()),
            );
            m.insert(
                Value::String("tag".to_string()),
                Value::String("x".to_string()),
            );
            m
        });

        // Single loop-closure entry: all required fields EXCEPT `closing_joint`.
        // path_a/path_b are well-formed only to reach the closing_joint check;
        // the behavior under test is the explicit-field guard, not path validation.
        let mut lc1 = BTreeMap::new();
        lc1.insert(
            Value::String("kind".to_string()),
            Value::String("loop_closure".to_string()),
        );
        lc1.insert(Value::String("body_id".to_string()), Value::Int(1));
        // NOTE: `closing_joint` is intentionally omitted.
        lc1.insert(
            Value::String("path_a".to_string()),
            Value::List(vec![world.clone(), j_a.clone(), j_x.clone()]),
        );
        lc1.insert(
            Value::String("path_b".to_string()),
            Value::List(vec![world.clone(), j_b.clone(), j_x.clone()]),
        );

        let mut mech = BTreeMap::new();
        mech.insert(
            Value::String("kind".to_string()),
            Value::String("mechanism".to_string()),
        );
        mech.insert(
            Value::String("loop_closures".to_string()),
            Value::List(vec![Value::Map(lc1)]),
        );

        assert_eq!(
            super::mechanism_loop_closure_chains(&Value::Map(mech)),
            None,
            "missing closing_joint field must make the whole call return None (missing-closing_joint guard)"
        );
    }

    /// Regression pin for the wrong-type guard in `mechanism_loop_closure_chains`
    /// (the `_ => return None` arm of the `loop_closures` match).
    ///
    /// This is the read-side counterpart of the write-side test
    /// `append_body_wrong_typed_loop_closures_returns_undef` in `mechanism.rs`.
    /// Both use `Value::Int(0)` as the wrong-type sentinel to keep the symmetric
    /// guard pair visually aligned.
    ///
    /// A Mechanism Map where `loop_closures` is present but has a non-`List`
    /// type must return `None`, not `Some([])` (which would silently swallow
    /// the corrupt record).
    #[test]
    fn mechanism_loop_closure_chains_wrong_typed_loop_closures_returns_none() {
        use std::collections::BTreeMap;

        let mut mech = BTreeMap::new();
        mech.insert(
            Value::String("kind".to_string()),
            Value::String("mechanism".to_string()),
        );
        // `loop_closures` present but wrong type — mirrors mechanism.rs:1547.
        mech.insert(Value::String("loop_closures".to_string()), Value::Int(0));

        assert_eq!(
            super::mechanism_loop_closure_chains(&Value::Map(mech)),
            None,
            "loop_closures present but non-List must return None (wrong-typed loop_closures guard)"
        );
    }

    /// Pins that `mechanism_loop_closure_chains` correctly classifies and
    /// extracts the cycle case as `LoopClosureChain::Cycle { chain_a, chain_b }`,
    /// where `chain_b` contains the closing joint twice.  Specifically:
    ///
    /// - `chain_a == [j_b]`            (path_a = [world, j_b] stripped)
    /// - `chain_b == [j_b, j_a, j_b]`  (path_b = [world, j_b, j_a, j_b] stripped)
    /// - j_b (the closing joint = chain_b.last()) appears twice in chain_b → Cycle
    ///
    /// Both paths have length ≥ 2, so `strip_world_sentinel` accepts them.
    /// Regression-proofs the cycle path's length≥2 invariant against any
    /// future change to `strip_world_sentinel` or the cycle branch of
    /// `append_body`, and pins the duplicated-closing-joint shape that
    /// triggers `LoopClosureChain::Cycle` classification.
    #[test]
    fn mechanism_loop_closure_chains_extracts_cycle_pair() {
        use crate::eval_builtin;

        // Build joints using the existing test helpers.
        let j_a = prismatic_x_0_to_1();
        let j_b = revolute_z_0_to_pi();
        let solid_a = Value::String("solidA".to_string());
        let solid_b = Value::String("solidB".to_string());

        // Two-body cycle: body(m0, solid_a, j_a, j_b) then body(m1, solid_b, j_b, j_a).
        // After body-1: joint_parents = {j_a: j_b}.
        // body-2 triggers cycle_introduced → records loop closure with:
        //   path_a = [world, j_b]            (at=j_b, walk_to_world(jp, j_b)=[j_b])
        //   path_b = [world, j_b, j_a, j_b]  (walk_to_world(jp, j_a)=[j_b,j_a], j_b appended)
        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin("body", &[m0, solid_a, j_a.clone(), j_b.clone()]);
        let m2 = eval_builtin("body", &[m1, solid_b, j_b.clone(), j_a.clone()]);

        let chains = super::mechanism_loop_closure_chains(&m2);
        assert!(
            chains.is_some(),
            "mechanism_loop_closure_chains must return Some for a cycle mechanism"
        );
        let pairs = chains.unwrap();
        assert_eq!(pairs.len(), 1, "one loop-closure pair expected");

        let (chain_a, chain_b, cj) = match &pairs[0] {
            super::LoopClosureChain::Cycle {
                chain_a,
                chain_b,
                closing_joint,
            } => (chain_a, chain_b, closing_joint),
            other => panic!("expected Cycle, got {:?}", other),
        };
        // chain_a = [j_b]  (world sentinel stripped from [world, j_b])
        assert_eq!(chain_a, &vec![j_b.clone()], "chain_a should be [j_b]");
        // chain_b = [j_b, j_a, j_b]  (world sentinel stripped from [world, j_b, j_a, j_b])
        // j_b appears at index 0 and index 2 → closing joint count > 1 → Cycle.
        assert_eq!(
            chain_b,
            &vec![j_b.clone(), j_a.clone(), j_b.clone()],
            "chain_b should be [j_b, j_a, j_b] (closing joint duplicated)"
        );
        // Both chains end with j_b (the closing joint).
        assert_eq!(
            chain_a.last(),
            Some(&j_b),
            "chain_a must terminate at j_b (the closing joint)"
        );
        assert_eq!(
            chain_b.last(),
            Some(&j_b),
            "chain_b must terminate at j_b (the closing joint)"
        );
        // closing_joint is propagated directly from the record.
        assert_eq!(cj, &j_b, "closing_joint should be j_b");
    }

    /// Pins that a self-loop mechanism (same joint passed as both `at` and
    /// `parent` to `body()`) produces a `LoopClosureChain::Cycle` entry with:
    ///
    /// - `chain_a == [j]`    (path_a = [world, j] stripped)
    /// - `chain_b == [j, j]` (path_b = [world, j, j] stripped)
    ///
    /// j (the closing joint = chain_b.last()) appears twice in chain_b → Cycle.
    ///
    /// Complements the raw-record test in `mechanism.rs::self_loop_records_loop_closure_constraint`
    /// by exercising the full round-trip through the public extractor.  That
    /// test verifies the stored `path_b = [world, j, j]` shape; this test
    /// verifies that `mechanism_loop_closure_chains` classifies the stripped
    /// chain correctly as `Cycle` rather than silently passing it as WellFormed
    /// (which would be wrong physics — the solver cannot use a chain that
    /// applies the same joint twice).
    #[test]
    fn mechanism_loop_closure_chains_extracts_self_loop_pair() {
        use crate::eval_builtin;

        let j = prismatic_x_0_to_1();
        let solid = Value::String("solid".to_string());

        // Self-loop: pass j as both `at` (args[2]) and `parent` (args[3]).
        let m0 = eval_builtin("mechanism", &[]);
        let m1 = eval_builtin("body", &[m0, solid, j.clone(), j.clone()]);

        let chains = super::mechanism_loop_closure_chains(&m1);
        assert!(
            chains.is_some(),
            "mechanism_loop_closure_chains must return Some for a self-loop mechanism"
        );
        let pairs = chains.unwrap();
        assert_eq!(
            pairs.len(),
            1,
            "one loop-closure pair expected for self-loop"
        );

        let (chain_a, chain_b, cj) = match &pairs[0] {
            super::LoopClosureChain::Cycle {
                chain_a,
                chain_b,
                closing_joint,
            } => (chain_a, chain_b, closing_joint),
            other => panic!("expected Cycle for self-loop, got {:?}", other),
        };
        // chain_a = [j]    (world sentinel stripped from [world, j])
        assert_eq!(chain_a, &vec![j.clone()], "chain_a should be [j]");
        // chain_b = [j, j] (world sentinel stripped from [world, j, j])
        // j appears at index 0 and index 1 → closing joint count > 1 → Cycle.
        assert_eq!(
            chain_b,
            &vec![j.clone(), j.clone()],
            "chain_b should be [j, j] (closing joint is at=parent, duplicated)"
        );
        assert_eq!(
            chain_a.last(),
            Some(&j),
            "chain_a must terminate at j (the closing joint)"
        );
        assert_eq!(
            chain_b.last(),
            Some(&j),
            "chain_b must terminate at j (the closing joint)"
        );
        // closing_joint is propagated directly from the record.
        assert_eq!(cj, &j, "closing_joint should be j");
    }

    /// `strip_world_sentinel` rejects a single-element `[world]` path.
    /// The function's contract requires returned chains to terminate at
    /// the closing joint, which an empty chain violates.
    #[test]
    fn strip_world_sentinel_rejects_world_only_path() {
        let world = crate::eval_builtin("world", &[]);
        let path = vec![world];
        assert_eq!(
            super::strip_world_sentinel(&path),
            None,
            "[world] alone must be rejected (would yield empty chain)"
        );
    }

    // ── KCC-γ step-11: widened solver — multi-DOF chain participation ──
    //
    // These tests pass `&vals_a` / `&vals_b_initial` as `&[JointValue]`
    // directly to the widened `solve_loop_closure` /
    // `solve_loop_closure_with_diagnostics`.  They will fail to compile
    // until step-12 widens the solver signatures.

    fn axis_y() -> Value {
        Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)])
    }

    fn planar_xy_joint_wide() -> Value {
        // Planar joint with ranges wide enough to admit the expected
        // converged config (0.25, 0.433, 0) → wrap the same range_x /
        // range_y / range_theta layout `make_planar` validates.
        eval_builtin(
            "planar",
            &[
                axis_x(),
                axis_y(),
                length_range(-1.0, 1.0),
                length_range(-1.0, 1.0),
                angle_range(-std::f64::consts::PI, std::f64::consts::PI),
            ],
        )
    }

    fn spherical_swing_joint() -> Value {
        // Axis-isotropic 3-DOF spherical joint with swing magnitude up to π.
        eval_builtin(
            "spherical",
            &[angle_range(0.0, std::f64::consts::PI)],
        )
    }

    #[test]
    fn solve_loop_closure_planar_chain_converges() {
        // chain_a = [revolute_z @ π/3, prismatic_x @ 0.5m] →
        //   end-effector at R_z(π/3) · Trans_x(0.5) →
        //   translation ≈ (0.25, 0.433, 0), rotation R_z(π/3).
        // chain_b = [planar_xy] with planar slot free; WarmStart from the
        // zero planar config.  Convergence requires finding the planar
        // (x, y, θ) that closes the chain at the chain_a target — namely
        // (0.25, 0.433, π/3).  This is the canonical multi-DOF closed-chain
        // participation case the KCC-γ widening enables.
        let chain_a = vec![revolute_z_0_to_pi(), prismatic_x_0_to_1()];
        let theta_a = std::f64::consts::PI / 3.0;
        let vals_a = vec![JointValue::Scalar(theta_a), JointValue::Scalar(0.5)];
        let chain_b = vec![planar_xy_joint_wide()];
        let vals_b_initial = vec![JointValue::Planar([0.0, 0.0, 0.0])];
        let free_b = vec![0];
        // WarmStart payload is the flat Newton state (length = sum of
        // free-joint `flat_len` widths).  Planar has flat_len=3, so the
        // warm-start vector has 3 components.
        let strategy = StartStrategy::WarmStart(vec![0.0, 0.0, 0.0]);
        let cfg = NewtonConfig {
            tol_pos_m: 1e-6,
            tol_rot_rad: 1e-6,
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
            NewtonOutcome::Converged {
                x,
                iters,
                residual_norm,
            } => {
                assert!(
                    iters < 50,
                    "expected convergence in <50 iters, got {iters} (residual_norm={residual_norm})"
                );
                assert!(
                    residual_norm < cfg.tol_pos_m + cfg.tol_rot_rad,
                    "expected residual_norm below combined tol, got {residual_norm}"
                );
                assert_eq!(x.len(), 3, "planar has flat_len=3 free components");
                // x = [tx, ty, theta] of the planar joint; check the
                // converged solution matches the chain_a target.
                assert!(
                    (x[0] - 0.25).abs() < 1e-4,
                    "expected x_tx ≈ 0.25, got {}",
                    x[0]
                );
                let target_ty = 0.5 * (std::f64::consts::PI / 3.0).sin();
                assert!(
                    (x[1] - target_ty).abs() < 1e-4,
                    "expected x_ty ≈ {target_ty}, got {}",
                    x[1]
                );
                assert!(
                    (x[2] - theta_a).abs() < 1e-4,
                    "expected x_theta ≈ π/3, got {}",
                    x[2]
                );
            }
            other => panic!("expected Converged on planar-only chain_b, got {other:?}"),
        }
    }

    #[test]
    fn solve_loop_closure_sphere_slot_renormalizes_after_step() {
        // chain_a = [spherical] held at identity quaternion → end-effector
        // is identity transform.  chain_b = [spherical] free, seeded from a
        // *non-unit-norm* quaternion (‖q‖ ≈ 1.58).  Without the per-step
        // renormalization wired in step-12's solver, `transform_at("spherical",
        // non_unit_q)` would emit Value::Undef and the closure would
        // short-circuit to NotConverged.  With renormalization, the solver
        // converges to a unit quaternion that aligns chain_b with chain_a's
        // identity end-effector.
        let chain_a = vec![spherical_swing_joint()];
        let vals_a = vec![JointValue::Sphere([1.0, 0.0, 0.0, 0.0])];
        let chain_b = vec![spherical_swing_joint()];
        // Non-unit-norm initial quaternion: ‖[1.5, 0.5, 0, 0]‖ = √2.5 ≈ 1.58.
        let vals_b_initial = vec![JointValue::Sphere([1.5, 0.5, 0.0, 0.0])];
        let free_b = vec![0];
        // WarmStart payload = the flat Newton state (4 components for the
        // sphere's storage width).
        let strategy = StartStrategy::WarmStart(vec![1.5, 0.5, 0.0, 0.0]);
        let cfg = NewtonConfig {
            tol_pos_m: 1e-6,
            tol_rot_rad: 1e-6,
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
            NewtonOutcome::Converged { x, .. } => {
                assert_eq!(x.len(), 4, "sphere has flat_len=4 stored components");
                // After convergence the Newton-state quaternion should be
                // (approximately) unit-norm; the per-step renormalization
                // projects the iterate back to S³ each closure evaluation.
                let norm_sq = x[0] * x[0] + x[1] * x[1] + x[2] * x[2] + x[3] * x[3];
                let norm = norm_sq.sqrt();
                assert!(
                    (norm - 1.0).abs() < 1e-3,
                    "converged sphere quaternion should be ≈ unit-norm (proves \
                     renormalization fired), got ‖q‖ = {norm}"
                );
            }
            other => panic!(
                "expected Converged from non-unit-norm sphere seed (renormalization \
                 should fire to keep the quaternion on S³), got {other:?}"
            ),
        }
    }

    #[test]
    fn solve_loop_closure_with_diagnostics_planar_chain_converges() {
        // Same physical scenario as
        // solve_loop_closure_planar_chain_converges, routed through the
        // diagnostic-emitting wrapper.  A 3-DOF planar free joint against
        // a 6-component loop residual is under-constrained (3 < 6), so
        // the wrapper emits a single KinematicUnderconstrained warning
        // and still delegates to the inner solver.  Convergence is
        // expected via the WarmStart seed that lands near the root.
        let chain_a = vec![revolute_z_0_to_pi(), prismatic_x_0_to_1()];
        let theta_a = std::f64::consts::PI / 3.0;
        let vals_a = vec![JointValue::Scalar(theta_a), JointValue::Scalar(0.5)];
        let chain_b = vec![planar_xy_joint_wide()];
        let vals_b_initial = vec![JointValue::Planar([0.0, 0.0, 0.0])];
        let free_b = vec![0];
        let strategy = StartStrategy::WarmStart(vec![0.0, 0.0, 0.0]);
        let cfg = NewtonConfig {
            tol_pos_m: 1e-6,
            tol_rot_rad: 1e-6,
            max_iters: 50,
            ..NewtonConfig::default()
        };

        let report = solve_loop_closure_with_diagnostics(
            &chain_a,
            &vals_a,
            &chain_b,
            &vals_b_initial,
            &free_b,
            &strategy,
            &cfg,
        );

        // The diagnostic wrapper does not report singularity for a
        // well-conditioned planar root; only the under-constrained warning
        // (if any) is expected.
        assert!(
            !report.is_singular(),
            "planar chain converges without singularity, got {:?}",
            report.outcome
        );
        match report.outcome {
            NewtonOutcome::Converged {
                x, residual_norm, ..
            } => {
                assert_eq!(x.len(), 3);
                assert!(residual_norm < cfg.tol_pos_m + cfg.tol_rot_rad);
            }
            other => panic!(
                "expected Converged from solve_loop_closure_with_diagnostics, got {other:?}"
            ),
        }
    }

    #[test]
    fn solve_normal_equations_flat_solves_pd_3x3_in_place() {
        // A = [[4,1,1],[1,3,0],[1,0,2]] (row-major: [4,1,1, 1,3,0, 1,0,2])
        // b = [6, 4, 3]  →  Ax = b  →  x = [1, 1, 1]
        // (4+1+1=6, 1+3+0=4, 1+0+2=3)
        //
        // Exercises the inner Σ_{k<j} loop at j=2 (k=0..2) and the back-solve
        // loop at i=0 (k=1..3) — the row-major indexing surface LDLᵀ is most
        // sensitive to.  LDLᵀ factors: D=[4, 11/4, 19/11],
        // L[1,0]=1/4, L[2,0]=1/4, L[2,1]=-1/11.
        let mut a = vec![4.0_f64, 1.0, 1.0, 1.0, 3.0, 0.0, 1.0, 0.0, 2.0];
        let mut b = vec![6.0_f64, 4.0, 3.0];
        let result = super::solve_normal_equations(&mut a, &mut b, 3, 1e-12, &[]);
        assert!(result, "expected solve to succeed on 3×3 PD matrix");
        assert!(
            (b[0] - 1.0).abs() < 1e-9,
            "expected b[0] ≈ 1.0, got {}",
            b[0]
        );
        assert!(
            (b[1] - 1.0).abs() < 1e-9,
            "expected b[1] ≈ 1.0, got {}",
            b[1]
        );
        assert!(
            (b[2] - 1.0).abs() < 1e-9,
            "expected b[2] ≈ 1.0, got {}",
            b[2]
        );
    }
}
