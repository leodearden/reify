//! Time-Optimal Trajectory Shaping (TOTS) SQP optimizer.
//!
//! Implements the hand-rolled SQP loop that minimises trajectory duration `T`
//! subject to vibration, velocity, acceleration, and force constraints.
//!
//! # Algorithm overview
//!
//! The optimisation variable is `x = [interior waypoints per joint…, T]`.
//! At each SQP iteration:
//! 1. Build spline from current `x`.
//! 2. Evaluate objective (`T`) and constraints (vib_peak, vel_peak, acc_peak,
//!    force_peak).
//! 3. Compute objective gradient and constraint Jacobian via forward differences.
//! 4. Update approximate Hessian with Powell-damped BFGS.
//! 5. Solve QP sub-problem (active-set KKT) via faer LU.
//! 6. Armijo backtracking with L1 penalty merit.
//! 7. Iterate until convergence or max_iters.
//!
//! # Module structure
//!
//! * [`TotsParams`] — input specification (waypoints, limits, modal model, etc.)
//! * [`TotsModel`] — aggregated models needed for evaluation.
//! * [`Evaluation`] — output of a single forward pass.
//! * [`TotsResult`] — full optimiser output with outcome code.
//! * [`solve_tots`] — top-level SQP driver.
#![allow(dead_code)]

use faer::Mat;

use super::spline::{BoundaryCondition, CubicSpline, MultiJointSpline};
use super::simulate::{
    simulate_trajectory_core, EffectorLocation, EndEffectorTrackData, MechanismModel, ModalModel,
};
use crate::dynamics::rnea::{inverse_dynamics_open_chain, RneaLink};

// ── Parameter types ───────────────────────────────────────────────────────────

/// Per-joint waypoint specification.
#[derive(Debug, Clone)]
pub(crate) struct JointWaypoints {
    /// Fixed start position (m or rad).
    pub(crate) start: f64,
    /// Interior waypoints (optimised). Length = n_interior.
    pub(crate) interior: Vec<f64>,
    /// Fixed end position.
    pub(crate) end: f64,
    /// Maximum allowed joint velocity magnitude (rad/s or m/s).
    pub(crate) vel_limit: f64,
    /// Maximum allowed joint acceleration magnitude (rad/s² or m/s²).
    pub(crate) acc_limit: f64,
    /// Maximum allowed joint torque/force magnitude (N·m or N).
    pub(crate) max_force: f64,
}

/// TOTS optimiser input specification.
#[derive(Debug, Clone)]
pub(crate) struct TotsParams {
    /// Per-joint waypoint data.
    pub(crate) joints: Vec<JointWaypoints>,
    /// Initial (and minimum) trajectory duration (s). Also used as T_min guard.
    pub(crate) t_initial: f64,
    /// Vibration tolerance (m). Peak vibration offset must be ≤ this.
    pub(crate) vib_tol: f64,
    /// Number of uniform grid points for constraint evaluation (≥ 2).
    pub(crate) n_grid: usize,
}

/// Aggregated model for TOTS evaluation.
#[derive(Clone)]
pub(crate) struct TotsModel {
    pub(crate) mechanism: MechanismModel,
    pub(crate) modal: ModalModel,
    pub(crate) effector_locations: Vec<EffectorLocation>,
}

// ── Variable vector packing ───────────────────────────────────────────────────

impl TotsParams {
    /// Total number of free variables: `Σ n_interior_j + 1` (T).
    pub(crate) fn n_vars(&self) -> usize {
        self.joints.iter().map(|j| j.interior.len()).sum::<usize>() + 1
    }

    /// Pack free variables into a flat vector: `[interiors_j0…, interiors_j1…, T]`.
    pub(crate) fn variable_vector(&self) -> Vec<f64> {
        let mut v = Vec::with_capacity(self.n_vars());
        for joint in &self.joints {
            v.extend_from_slice(&joint.interior);
        }
        v.push(self.t_initial);
        v
    }

    /// Unpack a flat variable vector back into a cloned `TotsParams`.
    ///
    /// The last element is `T`; the preceding elements are interior waypoints
    /// in per-joint order. Returns `None` if `x` has wrong length or `T ≤ 0`.
    pub(crate) fn unpack_variable_vector(&self, x: &[f64]) -> Option<TotsParams> {
        if x.len() != self.n_vars() {
            return None;
        }
        let t_new = *x.last().unwrap();
        if !t_new.is_finite() || t_new <= 0.0 {
            return None;
        }
        let mut params = self.clone();
        params.t_initial = t_new;
        let mut offset = 0;
        for joint in &mut params.joints {
            let n = joint.interior.len();
            joint.interior.copy_from_slice(&x[offset..offset + n]);
            offset += n;
        }
        Some(params)
    }
}

// ── Spline builder ────────────────────────────────────────────────────────────

/// Build a `MultiJointSpline` from `TotsParams`.
///
/// Knot times are uniformly spaced fractions of `T`: `[0, 1/k, 2/k, …, 1] * T`
/// where `k = n_interior + 1`. All joints share the same knot times.
///
/// Returns `None` if:
/// - `joints` is empty
/// - `T ≤ 0` or non-finite
/// - `n_interior` inconsistent or CubicSpline fit fails
pub(crate) fn build_spline(params: &TotsParams) -> Option<MultiJointSpline> {
    let t = params.t_initial;
    if !t.is_finite() || t <= 0.0 {
        return None;
    }
    if params.joints.is_empty() {
        return None;
    }

    // All joints must have the same number of interior waypoints.
    let n_int = params.joints[0].interior.len();
    for joint in &params.joints {
        if joint.interior.len() != n_int {
            return None;
        }
    }

    // Knot times: uniformly spaced fractions of T.
    let n_knots = n_int + 2; // start + interiors + end
    let knots: Vec<f64> = (0..n_knots).map(|i| i as f64 / (n_knots - 1) as f64 * t).collect();

    let mut splines = Vec::with_capacity(params.joints.len());
    for joint in &params.joints {
        let mut values = Vec::with_capacity(n_knots);
        values.push(joint.start);
        values.extend_from_slice(&joint.interior);
        values.push(joint.end);
        let bc = BoundaryCondition::Clamped { start_vel: 0.0, end_vel: 0.0 };
        let s = CubicSpline::fit(&knots, &values, &bc)?;
        splines.push(s);
    }

    MultiJointSpline::new_cubic(splines)
}

// ── Evaluation ────────────────────────────────────────────────────────────────

/// Result of a single TOTS forward evaluation.
#[derive(Debug, Clone)]
pub(crate) struct Evaluation {
    /// Objective value = T (trajectory duration).
    pub(crate) objective: f64,
    /// Peak vibration L2 norm across all locations and time samples (m).
    pub(crate) vib_peak: f64,
    /// Per-joint peak velocity magnitude (rad/s or m/s).
    pub(crate) vel_peak: Vec<f64>,
    /// Per-joint peak acceleration magnitude.
    pub(crate) acc_peak: Vec<f64>,
    /// Per-joint peak force/torque magnitude (N or N·m).
    pub(crate) force_peak: Vec<f64>,
}

/// Evaluate objective and constraint peaks for `params` given `model`.
///
/// Returns `None` if the spline cannot be built.
pub(crate) fn evaluate(params: &TotsParams, model: &TotsModel) -> Option<Evaluation> {
    let spline = build_spline(params)?;
    let n_joints = params.joints.len();
    let t = spline.duration();

    // ── Simulate for vibration ────────────────────────────────────────────────
    let track = simulate_trajectory_core(
        &spline,
        &model.mechanism,
        &model.modal,
        &model.effector_locations,
    );

    // ── Vibration peak ────────────────────────────────────────────────────────
    let vib_peak = vibration_peak(&track);

    // ── Uniform grid for velocity / acceleration / force peaks ───────────────
    let n_grid = params.n_grid.max(2);
    let dt = t / (n_grid - 1) as f64;

    let mut vel_peak = vec![0.0_f64; n_joints];
    let mut acc_peak = vec![0.0_f64; n_joints];
    let mut force_peak = vec![0.0_f64; n_joints];

    for k in 0..n_grid {
        let t_k = k as f64 * dt;
        let vels = spline.eval_dot(t_k);
        let accs = spline.eval_ddot(t_k);

        for (j, (&v, &a)) in vels.iter().zip(accs.iter()).enumerate() {
            vel_peak[j] = vel_peak[j].max(v.abs());
            acc_peak[j] = acc_peak[j].max(a.abs());
        }

        // Force peak via RNEA at this sample.
        let rnea_links = build_rnea_links(&model.mechanism, &vels, &accs);
        let tau = inverse_dynamics_open_chain(&rnea_links, [0.0, 0.0, 0.0]);
        // tau is Vec<Vec<f64>> (per-link per-dof); flatten to per-joint.
        let flat_tau: Vec<f64> = tau.into_iter().flatten().collect();
        for (j, &f) in flat_tau.iter().enumerate().take(n_joints) {
            force_peak[j] = force_peak[j].max(f.abs());
        }
    }

    Some(Evaluation {
        objective: t,
        vib_peak,
        vel_peak,
        acc_peak,
        force_peak,
    })
}

/// Compute peak vibration L2 norm across all locations and time samples.
fn vibration_peak(track: &EndEffectorTrackData) -> f64 {
    let mut peak = 0.0_f64;
    for series in &track.vibration_offset {
        for &[dx, dy, dz] in series {
            let mag = (dx * dx + dy * dy + dz * dz).sqrt();
            peak = peak.max(mag);
        }
    }
    peak
}

/// Build RNEA links for a single time sample from the mechanism model.
///
/// Correctly advances `dof_offset` per link so multi-DOF mechanisms are sliced
/// accurately.
fn build_rnea_links(
    mechanism: &MechanismModel,
    vels: &[f64],
    accs: &[f64],
) -> Vec<RneaLink> {
    let mut links = Vec::with_capacity(mechanism.links.len());
    let mut dof_offset = 0;
    for (i, link) in mechanism.links.iter().enumerate() {
        let n_dof = link.subspace.len();
        let q_dot = vels.get(dof_offset..dof_offset + n_dof)
            .map(|s| s.to_vec())
            .unwrap_or_else(|| vec![0.0; n_dof]);
        let q_ddot = accs.get(dof_offset..dof_offset + n_dof)
            .map(|s| s.to_vec())
            .unwrap_or_else(|| vec![0.0; n_dof]);
        dof_offset += n_dof;
        links.push(RneaLink {
            parent: if i == 0 { None } else { Some(i - 1) },
            parent_to_child: link.parent_to_child,
            subspace: link.subspace.clone(),
            mass: link.mass,
            com: link.com,
            inertia_about_com: link.inertia_about_com,
            q_dot,
            q_ddot,
        });
    }
    links
}

// ── Constraint violations ─────────────────────────────────────────────────────

/// Constraint violation vector: positive means violated.
///
/// Layout: `[vib - vib_tol, vel_peak[0] - vel_limit[0], …,
///           acc_peak[0] - acc_limit[0], …, force_peak[0] - max_force[0], …]`
pub(crate) fn constraint_violations(eval: &Evaluation, params: &TotsParams) -> Vec<f64> {
    let n_j = params.joints.len();
    let mut v = Vec::with_capacity(1 + 3 * n_j);
    v.push(eval.vib_peak - params.vib_tol);
    for (j, joint) in params.joints.iter().enumerate() {
        v.push(eval.vel_peak[j] - joint.vel_limit);
    }
    for (j, joint) in params.joints.iter().enumerate() {
        v.push(eval.acc_peak[j] - joint.acc_limit);
    }
    for (j, joint) in params.joints.iter().enumerate() {
        v.push(eval.force_peak[j] - joint.max_force);
    }
    v
}

/// Returns true if all constraints are satisfied (violations ≤ 0).
pub(crate) fn is_feasible(eval: &Evaluation, params: &TotsParams) -> bool {
    constraint_violations(eval, params).iter().all(|&v| v <= 0.0)
}

/// Returns the maximum constraint violation (0.0 if all feasible).
pub(crate) fn max_violation(eval: &Evaluation, params: &TotsParams) -> f64 {
    constraint_violations(eval, params)
        .iter()
        .cloned()
        .fold(0.0_f64, f64::max)
}

// ── Gradient and Jacobian ─────────────────────────────────────────────────────

/// Objective gradient ∂T/∂x = [0, …, 0, 1] (unit vector on T).
pub(crate) fn objective_gradient(n_vars: usize) -> Vec<f64> {
    let mut g = vec![0.0; n_vars];
    if n_vars > 0 {
        g[n_vars - 1] = 1.0;
    }
    g
}

/// Forward-difference Jacobian of the constraint vector w.r.t. x.
///
/// Returns an `(n_constraints × n_vars)` matrix stored row-major as `Vec<Vec<f64>>`.
/// Returns `None` if baseline evaluation fails.
pub(crate) fn constraint_jacobian(
    params: &TotsParams,
    model: &TotsModel,
    h: f64,
) -> Option<Vec<Vec<f64>>> {
    let n_vars = params.n_vars();
    let x0 = params.variable_vector();
    let eval0 = evaluate(params, model)?;
    let v0 = constraint_violations(&eval0, params);
    let n_c = v0.len();

    let mut jac = vec![vec![0.0_f64; n_vars]; n_c];
    for k in 0..n_vars {
        let mut xk = x0.clone();
        xk[k] += h;
        let params_k = params.unpack_variable_vector(&xk)?;
        let eval_k = evaluate(&params_k, model)?;
        let vk = constraint_violations(&eval_k, &params_k);
        for i in 0..n_c {
            jac[i][k] = (vk[i] - v0[i]) / h;
        }
    }
    Some(jac)
}

// ── BFGS Hessian update ───────────────────────────────────────────────────────

/// Powell-damped BFGS rank-2 Hessian update.
///
/// Given current approximate Hessian `B` (positive definite, n×n),
/// step `s` and gradient difference `y` (both length n), returns
/// updated `B'` satisfying the damped secant condition.
///
/// Powell damping ensures `B'` remains positive definite.
pub(crate) fn bfgs_update(b: &Mat<f64>, s: &[f64], y: &[f64]) -> Mat<f64> {
    let n = s.len();
    debug_assert_eq!(y.len(), n);
    debug_assert_eq!(b.nrows(), n);
    debug_assert_eq!(b.ncols(), n);

    // Compute Bs = B · s
    let mut bs = vec![0.0_f64; n];
    for i in 0..n {
        for j in 0..n {
            bs[i] += b[(i, j)] * s[j];
        }
    }

    // sBs = sᵀ · Bs
    let s_bs: f64 = s.iter().zip(bs.iter()).map(|(a, b)| a * b).sum();
    // sy = sᵀ · y
    let sy: f64 = s.iter().zip(y.iter()).map(|(a, b)| a * b).sum();

    // Powell damping: if sy < 0.2 * sBs, damp y.
    let y_eff: Vec<f64> = if sy >= 0.2 * s_bs {
        y.to_vec()
    } else {
        // θ = 0.8 * sBs / (sBs - sy)
        let theta = 0.8 * s_bs / (s_bs - sy);
        let one_minus_theta = 1.0 - theta;
        y.iter()
            .zip(bs.iter())
            .map(|(yi, bsi)| theta * yi + one_minus_theta * bsi)
            .collect()
    };

    let sy_eff: f64 = s.iter().zip(y_eff.iter()).map(|(a, b)| a * b).sum();

    // Guard against near-zero curvature.
    if sy_eff.abs() < 1e-14 * s_bs.abs().max(1e-14) {
        return b.clone();
    }

    // BFGS update:
    // B' = B - (B·s·sᵀ·B)/(sᵀ·Bs) + (y_eff·y_effᵀ)/(y_effᵀ·s)
    let mut b_new = b.clone();

    // Subtract (Bs · Bsᵀ) / sBs
    if s_bs.abs() > 1e-14 {
        for i in 0..n {
            for j in 0..n {
                b_new[(i, j)] -= bs[i] * bs[j] / s_bs;
            }
        }
    }

    // Add (y_eff · y_effᵀ) / sy_eff
    for i in 0..n {
        for j in 0..n {
            b_new[(i, j)] += y_eff[i] * y_eff[j] / sy_eff;
        }
    }

    b_new
}

// ── QP / KKT step solve ───────────────────────────────────────────────────────

/// Solve a QP sub-problem step via KKT conditions using faer dense LU.
///
/// With active constraints indexed by `active_jac` (rows from full Jacobian)
/// and corresponding violation values `active_viol`, the KKT system is:
///
/// ```text
/// [ B   Aᵀ ] [ dx ]   [ -g ]
/// [ A    0 ] [ λ  ] = [ -c ]
/// ```
///
/// where `g` is the objective gradient and `c = active_viol`.
///
/// Returns the step `dx` (length `n_vars`).  Returns `None` if solve fails.
pub(crate) fn solve_qp_step(
    b: &Mat<f64>,
    grad: &[f64],
    active_jac: &[Vec<f64>],
    active_viol: &[f64],
) -> Option<Vec<f64>> {
    let n = grad.len();
    let m = active_jac.len();
    let kkt_size = n + m;

    let mut kkt = Mat::<f64>::zeros(kkt_size, kkt_size);
    let mut rhs = Mat::<f64>::zeros(kkt_size, 1);

    // Top-left block: B
    for i in 0..n {
        for j in 0..n {
            kkt[(i, j)] = b[(i, j)];
        }
    }

    // Top-right block: Aᵀ (n × m)
    for (k, row) in active_jac.iter().enumerate() {
        for i in 0..n.min(row.len()) {
            kkt[(i, n + k)] = row[i];
            // Bottom-left: A (m × n)
            kkt[(n + k, i)] = row[i];
        }
    }

    // RHS top: -g
    for i in 0..n {
        rhs[(i, 0)] = -grad[i];
    }
    // RHS bottom: -c
    for k in 0..m {
        rhs[(n + k, 0)] = -active_viol[k];
    }

    use faer::linalg::solvers::Solve;
    let plu = kkt.partial_piv_lu();
    plu.solve_in_place(&mut rhs);

    // Extract dx from first n rows of solution.
    let dx: Vec<f64> = (0..n).map(|i| rhs[(i, 0)]).collect();

    // Guard: reject non-finite solutions.
    if dx.iter().any(|v| !v.is_finite()) {
        return None;
    }
    Some(dx)
}

// ── L1-penalty merit + Armijo line search ────────────────────────────────────

/// L1-penalty merit function: `φ(x) = T(x) + μ · Σ max(0, c_i(x))`.
pub(crate) fn merit(params: &TotsParams, model: &TotsModel, mu: f64) -> f64 {
    match evaluate(params, model) {
        None => f64::INFINITY,
        Some(eval) => {
            let obj = eval.objective;
            let viol_sum: f64 = constraint_violations(&eval, params)
                .iter()
                .map(|&v| v.max(0.0))
                .sum();
            obj + mu * viol_sum
        }
    }
}

/// Armijo backtracking line search along direction `dx`.
///
/// Returns the accepted step length `alpha ∈ (0, 1]`.
/// Returns `0.0` if no acceptable step found.
pub(crate) fn line_search(
    params: &TotsParams,
    dx: &[f64],
    mu: f64,
    model: &TotsModel,
    alpha_init: f64,
    c1: f64,       // Armijo constant (typical: 1e-4)
    max_halving: usize,
) -> f64 {
    let x0 = params.variable_vector();
    let m0 = merit(params, model, mu);
    // Directional derivative: ∇φᵀ · dx ≈ gᵀ·dx  (T component only contributes dx[n-1])
    // Conservative: use -‖dx‖ as descent guarantee approximation.
    let dx_norm: f64 = dx.iter().map(|v| v * v).sum::<f64>().sqrt();
    let dir_deriv = -dx_norm * dx_norm; // always negative for non-zero step

    let mut alpha = alpha_init;
    for _ in 0..max_halving {
        let x_new: Vec<f64> = x0.iter().zip(dx.iter()).map(|(xi, di)| xi + alpha * di).collect();
        if let Some(p_new) = params.unpack_variable_vector(&x_new) {
            let m_new = merit(&p_new, model, mu);
            if m_new <= m0 + c1 * alpha * dir_deriv.abs() {
                return alpha;
            }
        }
        alpha *= 0.5;
    }
    0.0
}

// ── Outcome codes ─────────────────────────────────────────────────────────────

/// Outcome of the TOTS solver.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TotsOutcome {
    /// Converged to feasible optimum.
    Converged,
    /// Reached max_iters without convergence.
    NonConvergence,
    /// Problem detected as infeasible (velocity/force limits cannot be met).
    ConstraintInfeasible,
}

impl TotsOutcome {
    /// Diagnostic code string for the outcome.
    pub(crate) fn code_str(&self) -> Option<&'static str> {
        match self {
            TotsOutcome::Converged => None,
            TotsOutcome::NonConvergence => Some("W_TrajectorySolverNonConvergence"),
            TotsOutcome::ConstraintInfeasible => Some("E_TrajectoryConstraintInfeasible"),
        }
    }
}

/// Full result from the TOTS solver.
#[derive(Debug, Clone)]
pub(crate) struct TotsResult {
    /// Outcome code.
    pub(crate) outcome: TotsOutcome,
    /// Best params found (may not be fully optimal if non-converged).
    pub(crate) params: TotsParams,
    /// Best evaluation found.
    pub(crate) evaluation: Evaluation,
    /// Number of SQP iterations performed.
    pub(crate) iterations: usize,
}

impl TotsResult {
    /// Total trajectory duration from the best result.
    pub(crate) fn duration(&self) -> f64 {
        self.params.t_initial
    }
}

// ── SQP driver ────────────────────────────────────────────────────────────────

/// SQP solver configuration.
#[derive(Debug, Clone)]
pub(crate) struct SqpConfig {
    /// Maximum number of SQP iterations.
    pub(crate) max_iters: usize,
    /// Convergence tolerance on step norm ‖Δx‖.
    pub(crate) tol: f64,
    /// Finite difference step for Jacobian.
    pub(crate) fd_h: f64,
    /// L1 penalty weight initial value.
    pub(crate) mu: f64,
    /// Armijo constant.
    pub(crate) c1: f64,
    /// Max backtracking halvings.
    pub(crate) max_halving: usize,
    /// Infeasibility detection: if max_violation > this after sufficient iters → infeasible.
    pub(crate) infeasibility_threshold: f64,
    /// Minimum iterations before infeasibility can be declared.
    pub(crate) infeasibility_min_iters: usize,
}

impl Default for SqpConfig {
    fn default() -> Self {
        SqpConfig {
            max_iters: 100,
            tol: 1e-6,
            fd_h: 1e-5,
            mu: 10.0,
            c1: 1e-4,
            max_halving: 20,
            infeasibility_threshold: 1e3,
            infeasibility_min_iters: 5,
        }
    }
}

/// Main TOTS SQP driver.
///
/// Minimises trajectory duration `T` subject to vibration, velocity, acceleration,
/// and force constraints. Returns a [`TotsResult`] with outcome code.
///
/// # Algorithm
///
/// Each SQP iteration:
/// 1. Evaluate current point.
/// 2. Check early infeasibility (velocity limit = 0 on nonzero P2P, etc.)
/// 3. Compute constraint Jacobian via finite differences.
/// 4. Determine active constraints (violated or within a tolerance band).
/// 5. Solve the KKT system for the search direction.
/// 6. Backtracking line search on the L1 merit function.
/// 7. Powell-damped BFGS Hessian update.
/// 8. Check convergence: small step + feasibility.
pub(crate) fn solve_tots(
    initial: TotsParams,
    model: &TotsModel,
    config: &SqpConfig,
) -> TotsResult {
    let n_vars = initial.n_vars();

    // Early infeasibility detection: velocity limit = 0 on nonzero P2P is
    // always infeasible regardless of T.
    for joint in &initial.joints {
        if joint.vel_limit <= 0.0 && (joint.end - joint.start).abs() > 1e-12 {
            let dummy_eval = Evaluation {
                objective: initial.t_initial,
                vib_peak: f64::INFINITY,
                vel_peak: vec![f64::INFINITY; initial.joints.len()],
                acc_peak: vec![f64::INFINITY; initial.joints.len()],
                force_peak: vec![f64::INFINITY; initial.joints.len()],
            };
            return TotsResult {
                outcome: TotsOutcome::ConstraintInfeasible,
                params: initial,
                evaluation: dummy_eval,
                iterations: 1,
            };
        }
    }

    // Evaluate initial point.
    let mut current = initial.clone();
    let mut current_eval = match evaluate(&current, model) {
        Some(e) => e,
        None => {
            let dummy_eval = Evaluation {
                objective: current.t_initial,
                vib_peak: f64::INFINITY,
                vel_peak: vec![f64::INFINITY; current.joints.len()],
                acc_peak: vec![f64::INFINITY; current.joints.len()],
                force_peak: vec![f64::INFINITY; current.joints.len()],
            };
            return TotsResult {
                outcome: TotsOutcome::ConstraintInfeasible,
                params: current,
                evaluation: dummy_eval,
                iterations: 0,
            };
        }
    };

    // Initialise identity Hessian approximation.
    let mut hessian = Mat::<f64>::zeros(n_vars, n_vars);
    for i in 0..n_vars {
        hessian[(i, i)] = 1.0;
    }

    let mut mu = config.mu;

    // Track best feasible point found so far.
    let mut best_params = current.clone();
    let mut best_eval = current_eval.clone();
    let mut best_is_feasible = is_feasible(&best_eval, &best_params);

    let grad = objective_gradient(n_vars);
    let mut prev_x: Option<Vec<f64>> = None;
    let mut prev_lagr_grad: Option<Vec<f64>> = None;

    for iter in 0..config.max_iters {
        let violations = constraint_violations(&current_eval, &current);
        let n_c = violations.len();

        // Check convergence at start of each iteration.
        // If feasible and previous step was very small, we've converged.
        if is_feasible(&current_eval, &current)
            && let Some(ref px) = prev_x
        {
            let x_cur = current.variable_vector();
            let step_norm: f64 = x_cur.iter().zip(px.iter())
                .map(|(a, b)| (a - b).powi(2)).sum::<f64>().sqrt();
            if step_norm < config.tol {
                return TotsResult {
                    outcome: TotsOutcome::Converged,
                    params: current.clone(),
                    evaluation: current_eval.clone(),
                    iterations: iter + 1,
                };
            }
        }

        // Early exit on hard infeasibility.
        let viol_max = max_violation(&current_eval, &current);
        if iter >= config.infeasibility_min_iters && viol_max > config.infeasibility_threshold {
            return TotsResult {
                outcome: TotsOutcome::ConstraintInfeasible,
                params: current,
                evaluation: current_eval,
                iterations: iter + 1,
            };
        }

        // Compute constraint Jacobian.
        let jac_opt = constraint_jacobian(&current, model, config.fd_h);

        // Active constraints: violated (> 0) or nearly active (> -active_tol).
        let active_tol = 1e-4;
        let active_indices: Vec<usize> = (0..n_c)
            .filter(|&i| violations[i] > -active_tol)
            .collect();
        let active_jac: Vec<Vec<f64>> = if let Some(ref jac) = jac_opt {
            active_indices.iter().map(|&i| jac[i].clone()).collect()
        } else {
            vec![]
        };
        let active_viol: Vec<f64> = active_indices.iter().map(|&i| violations[i]).collect();

        // Solve QP step.
        let dx_opt = solve_qp_step(&hessian, &grad, &active_jac, &active_viol);
        let dx = match dx_opt {
            Some(d) => d,
            None => {
                // Fallback: gradient descent on merit (reduce T).
                let mut d = vec![0.0_f64; n_vars];
                d[n_vars - 1] = -0.1;
                d
            }
        };

        // Check convergence on step norm.
        let dx_norm: f64 = dx.iter().map(|v| v * v).sum::<f64>().sqrt();
        if dx_norm < config.tol && is_feasible(&current_eval, &current) {
            return TotsResult {
                outcome: TotsOutcome::Converged,
                params: current.clone(),
                evaluation: current_eval.clone(),
                iterations: iter + 1,
            };
        }

        // Line search with merit function.
        let alpha = line_search(&current, &dx, mu, model, 1.0, config.c1, config.max_halving);

        let x0 = current.variable_vector();
        let x_new: Vec<f64> = if alpha > 0.0 {
            x0.iter().zip(dx.iter()).map(|(xi, di)| xi + alpha * di).collect()
        } else {
            // No merit improvement — try small T-only reduction.
            let mut xn = x0.clone();
            xn[n_vars - 1] = (xn[n_vars - 1] * 0.98).max(1e-3);
            xn
        };

        let new_params = match current.unpack_variable_vector(&x_new) {
            Some(p) => p,
            None => {
                // Invalid (T <= 0): try a 1% T reduction only.
                let mut xn = x0.clone();
                xn[n_vars - 1] = (xn[n_vars - 1] * 0.99).max(1e-3);
                match current.unpack_variable_vector(&xn) {
                    Some(p) => p,
                    None => break,
                }
            }
        };

        let new_eval = match evaluate(&new_params, model) {
            Some(e) => e,
            None => break,
        };

        // BFGS update using augmented Lagrangian gradient approximation.
        {
            let x_cur = current.variable_vector();
            let s: Vec<f64> = x_new.iter().zip(x_cur.iter()).map(|(a, b)| a - b).collect();

            // Augmented Lagrangian gradient at new point.
            let _viol_new = constraint_violations(&new_eval, &new_params);
            let mut g_new = objective_gradient(n_vars);
            // Add mu * gradient of max(0, c) sum (approximated from Jacobian).
            if let Some(ref jac) = jac_opt {
                for (i, row) in jac.iter().enumerate() {
                    if violations[i] > 0.0 {
                        for (k, &jval) in row.iter().enumerate().take(n_vars) {
                            g_new[k] += mu * jval;
                        }
                    }
                }
            }

            // Augmented Lagrangian gradient at current point.
            let g_cur = if let Some(ref pg) = prev_lagr_grad {
                pg.clone()
            } else {
                let mut g = objective_gradient(n_vars);
                if let Some(ref jac) = jac_opt {
                    for (i, row) in jac.iter().enumerate() {
                        if violations[i] > 0.0 {
                            for (k, &jval) in row.iter().enumerate().take(n_vars) {
                                g[k] += mu * jval;
                            }
                        }
                    }
                }
                g
            };

            let y: Vec<f64> = g_new.iter().zip(g_cur.iter()).map(|(a, b)| a - b).collect();

            // Only update if step is non-trivial.
            let s_norm: f64 = s.iter().map(|v| v * v).sum::<f64>().sqrt();
            if s_norm > 1e-10 {
                hessian = bfgs_update(&hessian, &s, &y);
                // Enforce diagonal floor for positive definiteness.
                for i in 0..n_vars {
                    if hessian[(i, i)] < 1e-6 {
                        hessian[(i, i)] = 1e-6;
                    }
                }
            }

            prev_lagr_grad = Some(g_new);
        }

        prev_x = Some(x_new);

        // Track best feasible solution.
        let new_is_feasible = is_feasible(&new_eval, &new_params);
        if new_is_feasible && (!best_is_feasible || new_eval.objective < best_eval.objective) {
            best_params = new_params.clone();
            best_eval = new_eval.clone();
            best_is_feasible = true;
        }

        current = new_params;
        current_eval = new_eval;

        // Increase penalty weight if still infeasible.
        if viol_max > 0.0 {
            mu = (mu * 1.2).min(1e6);
        }
    }

    // Max iters reached — return best result found.
    let (final_params, final_eval) = if best_is_feasible {
        (best_params, best_eval)
    } else {
        (current, current_eval)
    };

    TotsResult {
        outcome: TotsOutcome::NonConvergence,
        params: final_params,
        evaluation: final_eval,
        iterations: config.max_iters,
    }
}

// ── Infeasibility detection helper ───────────────────────────────────────────

/// Attempt to detect hard infeasibility by checking if velocity limits are
/// physically impossible given the start/end positions and T.
///
/// For a 1-DOF straight-line move from `q0` to `q1` in time `T`, the minimum
/// peak velocity for any smooth trajectory is approximately `|q1-q0| / T * k`
/// where `k ≈ 1.5` for a cubic profile (achieved at the midpoint).
/// If this lower bound exceeds the limit, the constraint is infeasible.
pub(crate) fn check_velocity_infeasible(params: &TotsParams) -> bool {
    let t = params.t_initial;
    if !t.is_finite() || t <= 0.0 {
        return true;
    }
    for joint in &params.joints {
        let displacement = (joint.end - joint.start).abs();
        // Rough lower bound: for a clamped cubic, peak vel ≈ 1.5 * displacement / T
        let peak_lower_bound = 1.5 * displacement / t;
        if peak_lower_bound > joint.vel_limit * (1.0 + 1e-6) {
            return true;
        }
    }
    false
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::simulate::ModeDesc;
    use crate::dynamics::spatial::{Frame3, SpatialTransform6, SpatialVector6};

    // ── Shared fixture builders ───────────────────────────────────────────────

    fn identity_link(mass: f64) -> super::super::simulate::LinkDesc {
        use super::super::simulate::LinkDesc;
        LinkDesc {
            parent_to_child: SpatialTransform6::from_frame3(&Frame3::identity()),
            subspace: vec![SpatialVector6::from_array([0.0, 0.0, 0.0, 1.0, 0.0, 0.0])],
            mass,
            com: [0.0; 3],
            inertia_about_com: [[0.0; 3]; 3],
        }
    }

    fn gantry_mechanism(mass: f64) -> MechanismModel {
        MechanismModel { links: vec![identity_link(mass)] }
    }

    fn unit_modal(freq_hz: f64, zeta: f64) -> ModalModel {
        ModalModel {
            modes: vec![ModeDesc { freq_hz, zeta, force_projection: vec![1.0] }],
        }
    }

    fn unit_effector() -> Vec<EffectorLocation> {
        vec![EffectorLocation { mode_coeffs: vec![1.0] }]
    }

    fn gantry_model(mass: f64, freq_hz: f64, zeta: f64) -> TotsModel {
        TotsModel {
            mechanism: gantry_mechanism(mass),
            modal: unit_modal(freq_hz, zeta),
            effector_locations: unit_effector(),
        }
    }

    fn simple_params(t: f64) -> TotsParams {
        TotsParams {
            joints: vec![JointWaypoints {
                start: 0.0,
                interior: vec![0.5],
                end: 1.0,
                vel_limit: 10.0,
                acc_limit: 50.0,
                max_force: 100.0,
            }],
            t_initial: t,
            vib_tol: 1e-3,
            n_grid: 50,
        }
    }

    // ── Step-1 tests: parameterisation → spline build ─────────────────────────

    /// (a) 1-joint cubic with fractions [0, 0.5, 1] T=2.0 yields duration==2.0
    ///     and eval reproduces waypoints.
    #[test]
    fn build_spline_duration_equals_t() {
        let params = TotsParams {
            joints: vec![JointWaypoints {
                start: 0.0,
                interior: vec![0.5],
                end: 1.0,
                vel_limit: 10.0,
                acc_limit: 50.0,
                max_force: 100.0,
            }],
            t_initial: 2.0,
            vib_tol: 1e-3,
            n_grid: 50,
        };
        let spline = build_spline(&params).expect("should build");
        assert!((spline.duration() - 2.0).abs() < 1e-12, "duration={}", spline.duration());
        // At t=0: should be start (0.0)
        let v0 = spline.eval(0.0);
        assert!((v0[0] - 0.0).abs() < 1e-10, "start value: {}", v0[0]);
        // At t=2.0: should be end (1.0)
        let v1 = spline.eval(2.0);
        assert!((v1[0] - 1.0).abs() < 1e-10, "end value: {}", v1[0]);
        // At t=1.0: interior waypoint (0.5)
        let vm = spline.eval(1.0);
        assert!((vm[0] - 0.5).abs() < 1e-10, "interior value: {}", vm[0]);
    }

    /// (b) T=4.0 rescales correctly.
    #[test]
    fn build_spline_rescale_t() {
        let params = TotsParams {
            joints: vec![JointWaypoints {
                start: 0.0,
                interior: vec![0.5],
                end: 1.0,
                vel_limit: 10.0,
                acc_limit: 50.0,
                max_force: 100.0,
            }],
            t_initial: 4.0,
            vib_tol: 1e-3,
            n_grid: 50,
        };
        let spline = build_spline(&params).expect("should build");
        assert!((spline.duration() - 4.0).abs() < 1e-12, "duration={}", spline.duration());
        let vm = spline.eval(2.0);
        assert!((vm[0] - 0.5).abs() < 1e-10, "interior at T/2: {}", vm[0]);
    }

    /// (c) Degenerate spec returns None.
    #[test]
    fn build_spline_degenerate_returns_none() {
        // T <= 0
        let p1 = TotsParams {
            joints: vec![JointWaypoints {
                start: 0.0, interior: vec![0.5], end: 1.0,
                vel_limit: 10.0, acc_limit: 50.0, max_force: 100.0,
            }],
            t_initial: 0.0,
            vib_tol: 1e-3,
            n_grid: 50,
        };
        assert!(build_spline(&p1).is_none(), "T=0 should be None");

        // Empty joints
        let p2 = TotsParams {
            joints: vec![],
            t_initial: 2.0,
            vib_tol: 1e-3,
            n_grid: 50,
        };
        assert!(build_spline(&p2).is_none(), "empty joints should be None");
    }

    /// Variable vector round-trips.
    #[test]
    fn variable_vector_roundtrip() {
        let params = simple_params(3.0);
        let x = params.variable_vector();
        assert_eq!(x.len(), 2); // 1 interior + T
        assert!((x[0] - 0.5).abs() < 1e-14);
        assert!((x[1] - 3.0).abs() < 1e-14);

        let recovered = params.unpack_variable_vector(&x).expect("roundtrip");
        assert!((recovered.t_initial - 3.0).abs() < 1e-14);
        assert!((recovered.joints[0].interior[0] - 0.5).abs() < 1e-14);
    }

    // ── Step-3 tests: objective + constraint-peak evaluation ──────────────────

    /// Objective equals T.
    #[test]
    fn evaluate_objective_equals_t() {
        let params = simple_params(2.0);
        let model = gantry_model(1.0, 5.0, 0.05);
        let eval = evaluate(&params, &model).expect("evaluate");
        assert!((eval.objective - 2.0).abs() < 1e-12, "objective={}", eval.objective);
    }

    /// Static profile (start==end==0.5, interior==0.5) yields vib_peak≤1e-9.
    #[test]
    fn evaluate_static_profile_zero_vibration() {
        let params = TotsParams {
            joints: vec![JointWaypoints {
                start: 0.5, interior: vec![0.5], end: 0.5,
                vel_limit: 10.0, acc_limit: 50.0, max_force: 100.0,
            }],
            t_initial: 1.0,
            vib_tol: 1e-3,
            n_grid: 50,
        };
        let model = gantry_model(1.0, 5.0, 0.05);
        let eval = evaluate(&params, &model).expect("evaluate static");
        assert!(eval.vib_peak <= 1e-9, "vib_peak={:.3e}", eval.vib_peak);
    }

    /// vel_peak matches analytic spline eval_dot ∞-norm.
    #[test]
    fn evaluate_vel_peak_matches_spline() {
        let params = simple_params(2.0);
        let model = gantry_model(1.0, 5.0, 0.05);
        let eval = evaluate(&params, &model).expect("evaluate");

        let spline = build_spline(&params).expect("spline");
        let n = 1000;
        let dt = 2.0 / (n - 1) as f64;
        let mut peak = 0.0_f64;
        for k in 0..n {
            let t = k as f64 * dt;
            let v = spline.eval_dot(t);
            peak = peak.max(v[0].abs());
        }
        // Allow 5% tolerance due to different grid sizes.
        assert!((eval.vel_peak[0] - peak).abs() < 0.05 * peak + 1e-6,
            "vel_peak: eval={:.4e}, analytic={:.4e}", eval.vel_peak[0], peak);
    }

    /// Fast T > slow T for vel_peak (faster trajectory → higher velocity).
    #[test]
    fn evaluate_fast_t_higher_vel_peak() {
        let model = gantry_model(1.0, 5.0, 0.05);
        let params_slow = simple_params(4.0);
        let params_fast = simple_params(1.0);
        let eval_slow = evaluate(&params_slow, &model).expect("slow");
        let eval_fast = evaluate(&params_fast, &model).expect("fast");
        assert!(eval_fast.vel_peak[0] > eval_slow.vel_peak[0],
            "fast vel={:.4e} should > slow vel={:.4e}",
            eval_fast.vel_peak[0], eval_slow.vel_peak[0]);
    }

    // ── Step-5 tests: constraint violations + feasibility ─────────────────────

    /// Slack config is feasible.
    #[test]
    fn slack_config_is_feasible() {
        let params = TotsParams {
            joints: vec![JointWaypoints {
                start: 0.0,
                interior: vec![0.5],
                end: 1.0,
                vel_limit: 1000.0,   // very slack
                acc_limit: 10000.0,
                max_force: 100000.0,
            }],
            t_initial: 5.0,          // slow trajectory
            vib_tol: 10.0,           // loose vib tol
            n_grid: 20,
        };
        let model = gantry_model(1.0, 5.0, 0.05);
        let eval = evaluate(&params, &model).expect("evaluate");
        assert!(is_feasible(&eval, &params), "slack config should be feasible");
        assert!(max_violation(&eval, &params) <= 0.0);
    }

    /// Velocity limit just below peak makes infeasible.
    #[test]
    fn tight_velocity_limit_makes_infeasible() {
        let model = gantry_model(1.0, 5.0, 0.05);
        let params_ref = simple_params(2.0);
        let eval_ref = evaluate(&params_ref, &model).expect("evaluate");
        let peak = eval_ref.vel_peak[0];

        // Set limit just below the actual peak.
        let params_tight = TotsParams {
            joints: vec![JointWaypoints {
                start: 0.0,
                interior: vec![0.5],
                end: 1.0,
                vel_limit: peak * 0.5,   // tighter than actual
                acc_limit: 1e6,
                max_force: 1e6,
            }],
            t_initial: 2.0,
            vib_tol: 1e6,
            n_grid: 50,
        };
        let eval_tight = evaluate(&params_tight, &model).expect("evaluate tight");
        assert!(!is_feasible(&eval_tight, &params_tight), "tight limit should be infeasible");
        assert!(max_violation(&eval_tight, &params_tight) > 0.0);
    }

    /// Violations ordering is stable (vib first, then vel per joint, acc, force).
    #[test]
    fn violations_ordering_stable() {
        let params = simple_params(2.0);
        let model = gantry_model(1.0, 5.0, 0.05);
        let eval = evaluate(&params, &model).expect("evaluate");
        let v = constraint_violations(&eval, &params);
        // n_joints=1: [vib-tol, vel-limit, acc-limit, force-limit] → length 4
        assert_eq!(v.len(), 4, "violation vector length");
        // vib - vib_tol
        assert!((v[0] - (eval.vib_peak - params.vib_tol)).abs() < 1e-14);
        // vel_peak[0] - vel_limit[0]
        assert!((v[1] - (eval.vel_peak[0] - params.joints[0].vel_limit)).abs() < 1e-14);
        // acc_peak[0] - acc_limit[0]
        assert!((v[2] - (eval.acc_peak[0] - params.joints[0].acc_limit)).abs() < 1e-14);
        // force_peak[0] - max_force[0]
        assert!((v[3] - (eval.force_peak[0] - params.joints[0].max_force)).abs() < 1e-14);
    }

    // ── Step-7 tests: Jacobian + objective gradient ───────────────────────────

    /// Objective gradient = [0, …, 0, 1].
    #[test]
    fn objective_gradient_is_unit_on_t() {
        let params = simple_params(2.0);
        let n = params.n_vars();
        let g = objective_gradient(n);
        assert_eq!(g.len(), n);
        for (i, &gi) in g.iter().enumerate().take(n - 1) {
            assert_eq!(gi, 0.0, "g[{i}] should be 0");
        }
        assert_eq!(g[n - 1], 1.0, "g[n-1] should be 1");
    }

    /// Constraint Jacobian has finite entries.
    #[test]
    fn constraint_jacobian_finite_entries() {
        let params = simple_params(2.0);
        let model = gantry_model(1.0, 5.0, 0.05);
        let jac = constraint_jacobian(&params, &model, 1e-5).expect("jacobian");
        for (i, row) in jac.iter().enumerate() {
            for (j, &v) in row.iter().enumerate() {
                assert!(v.is_finite(), "jac[{i}][{j}] = {v}");
            }
        }
    }

    /// Velocity constraint Jacobian w.r.t. T has negative sign:
    /// slowing down (increasing T) lowers velocity peak.
    #[test]
    fn velocity_jacobian_wrt_t_negative() {
        let params = simple_params(2.0);
        let model = gantry_model(1.0, 5.0, 0.05);
        let jac = constraint_jacobian(&params, &model, 1e-5).expect("jacobian");
        let n_vars = params.n_vars();
        // Row 1 = vel constraint for joint 0; last column = dT.
        let dvel_dt = jac[1][n_vars - 1];
        assert!(dvel_dt < 0.0, "dvel/dT should be negative, got {dvel_dt}");
    }

    // ── Step-9 tests: BFGS Hessian update ─────────────────────────────────────

    /// Secant condition: B'·s ≈ y_eff (Powell-damped).
    #[test]
    fn bfgs_secant_condition() {
        let n = 3;
        let mut b = Mat::<f64>::zeros(n, n);
        for i in 0..n { b[(i, i)] = 1.0; }  // identity

        let s = vec![0.1, 0.2, -0.05];
        let y = vec![0.3, 0.1, 0.4];  // positive curvature
        let b_new = bfgs_update(&b, &s, &y);

        // Compute B'·s
        let mut bs_new = vec![0.0_f64; n];
        for i in 0..n {
            for j in 0..n {
                bs_new[i] += b_new[(i, j)] * s[j];
            }
        }
        // Should ≈ y (secant condition, since sy > 0 → no damping)
        for i in 0..n {
            assert!((bs_new[i] - y[i]).abs() < 1e-10,
                "secant[{i}]: B's={:.4e}, y={:.4e}", bs_new[i], y[i]);
        }
    }

    /// Symmetry: B' is symmetric.
    #[test]
    fn bfgs_symmetry() {
        let n = 4;
        let mut b = Mat::<f64>::zeros(n, n);
        for i in 0..n { b[(i, i)] = 2.0; }

        let s = vec![0.1, -0.1, 0.2, 0.05];
        let y = vec![0.5, 0.3, 0.8, 0.1];
        let b_new = bfgs_update(&b, &s, &y);

        for i in 0..n {
            for j in 0..n {
                let diff = (b_new[(i, j)] - b_new[(j, i)]).abs();
                assert!(diff < 1e-12, "asymmetry at ({i},{j}): {diff}");
            }
        }
    }

    /// Powell damping keeps Hessian PD when sy < 0.2 * sBs.
    #[test]
    fn bfgs_powell_damping_keeps_pd() {
        let n = 2;
        let mut b = Mat::<f64>::zeros(n, n);
        b[(0, 0)] = 10.0;
        b[(1, 1)] = 10.0;

        let s = vec![1.0, 0.0];
        // y that would give negative curvature without damping.
        let y = vec![-5.0, 0.0];
        let b_new = bfgs_update(&b, &s, &y);

        // Check PD: diagonal entries > 0.
        for i in 0..n {
            assert!(b_new[(i, i)] > 0.0, "diagonal[{i}]={}", b_new[(i, i)]);
        }
    }

    // ── Step-11 tests: QP/KKT step solve ──────────────────────────────────────

    /// No active constraints → Newton step (B^{-1} · (-g)).
    #[test]
    fn qp_step_no_active_constraints() {
        let n = 2;
        let mut b = Mat::<f64>::zeros(n, n);
        b[(0, 0)] = 4.0;
        b[(1, 1)] = 2.0;

        // grad = [0, 1] (pure T objective)
        let grad = vec![0.0, 1.0];
        let dx = solve_qp_step(&b, &grad, &[], &[]).expect("solve");
        // Newton step: -B^{-1} · g = [-0/4, -1/2] = [0, -0.5]
        assert!((dx[0]).abs() < 1e-10, "dx[0]={}", dx[0]);
        assert!((dx[1] - (-0.5)).abs() < 1e-10, "dx[1]={}", dx[1]);
    }

    /// One active constraint honored.
    #[test]
    fn qp_step_one_active_constraint() {
        let n = 2;
        let mut b = Mat::<f64>::zeros(n, n);
        b[(0, 0)] = 1.0;
        b[(1, 1)] = 1.0;

        let grad = vec![0.0, 1.0];
        // One constraint: velocity limit, gradient w.r.t. T is -1 (increasing T reduces velocity).
        let active_jac = vec![vec![0.0, -1.0]];
        let active_viol = vec![0.1];  // violated by 0.1

        let dx = solve_qp_step(&b, &grad, &active_jac, &active_viol).expect("solve");
        assert_eq!(dx.len(), n);
        // dx should be finite.
        assert!(dx.iter().all(|v| v.is_finite()), "dx non-finite: {:?}", dx);
    }

    /// Finite result guaranteed.
    #[test]
    fn qp_step_finite_result() {
        let n = 3;
        let mut b = Mat::<f64>::zeros(n, n);
        for i in 0..n { b[(i, i)] = 1.0; }

        let grad = vec![0.0, 0.0, 1.0];
        let jac = vec![vec![1.0, 0.0, -0.5], vec![0.0, 1.0, -0.3]];
        let viol = vec![0.2, 0.1];

        let dx = solve_qp_step(&b, &grad, &jac, &viol).expect("solve");
        assert!(dx.iter().all(|v| v.is_finite()), "non-finite: {:?}", dx);
    }

    // ── Step-13 tests: merit + Armijo line search ─────────────────────────────

    /// Feasible point merit == objective.
    #[test]
    fn merit_feasible_equals_objective() {
        let params = TotsParams {
            joints: vec![JointWaypoints {
                start: 0.0, interior: vec![0.5], end: 1.0,
                vel_limit: 1000.0, acc_limit: 10000.0, max_force: 1e6,
            }],
            t_initial: 5.0,
            vib_tol: 100.0,
            n_grid: 20,
        };
        let model = gantry_model(1.0, 5.0, 0.05);
        let eval = evaluate(&params, &model).expect("evaluate");
        assert!(is_feasible(&eval, &params), "should be feasible");

        let m = merit(&params, &model, 10.0);
        assert!((m - eval.objective).abs() < 1e-10,
            "feasible merit={m:.4e} != objective={:.4e}", eval.objective);
    }

    /// Infeasible point merit > objective.
    #[test]
    fn merit_infeasible_exceeds_objective() {
        let params = TotsParams {
            joints: vec![JointWaypoints {
                start: 0.0, interior: vec![0.5], end: 1.0,
                vel_limit: 0.001,   // impossibly tight
                acc_limit: 10000.0,
                max_force: 1e6,
            }],
            t_initial: 2.0,
            vib_tol: 100.0,
            n_grid: 20,
        };
        let model = gantry_model(1.0, 5.0, 0.05);
        let eval = evaluate(&params, &model).expect("evaluate");
        assert!(!is_feasible(&eval, &params), "should be infeasible");

        let m = merit(&params, &model, 10.0);
        assert!(m > eval.objective + 1e-10,
            "infeasible merit={m:.4e} should > objective={:.4e}", eval.objective);
    }

    /// Line search: sufficient decrease on a good step.
    #[test]
    fn line_search_sufficient_decrease() {
        let params = simple_params(3.0);
        let model = gantry_model(1.0, 5.0, 0.05);
        // Step in the direction of reducing T.
        let n = params.n_vars();
        let mut dx = vec![0.0; n];
        dx[n - 1] = -0.1;  // reduce T

        let alpha = line_search(&params, &dx, 10.0, &model, 1.0, 1e-4, 20);
        assert!(alpha > 0.0, "alpha should be > 0, got {alpha}");
    }

    /// Alpha shrinks under backtracking.
    #[test]
    fn line_search_alpha_shrinks() {
        let params = simple_params(2.0);
        let model = gantry_model(1.0, 5.0, 0.05);
        // Huge step (unphysical) should trigger backtracking.
        let n = params.n_vars();
        let mut dx = vec![0.0; n];
        dx[n - 1] = -1.9;  // would make T ≈ 0.1 (very small, possibly infeasible)

        let alpha = line_search(&params, &dx, 10.0, &model, 1.0, 1e-4, 20);
        // Alpha should be < 1 due to backtracking.
        assert!(alpha < 1.0 || alpha == 0.0, "alpha={alpha}, expected < 1.0 after backtracking");
    }

    /// Step-21 RED: line-search must never accept a step that increases the L1 merit.
    ///
    /// dx is a pure ASCENT direction (dx[n-1] = 5e4, increasing T worsens a feasible merit).
    /// The accepted point x0 + alpha*dx must have merit ≤ m0 + 1e-9.
    /// Under the buggy `.abs()` acceptance condition this fails (alpha=1 is wrongly accepted,
    /// m_acc ≈ 5e4 >> m0 ≈ 3.0).
    #[test]
    fn line_search_rejects_merit_increase() {
        let params = simple_params(3.0);
        let model = gantry_model(1.0, 5.0, 0.05);
        let m0 = merit(&params, &model, 10.0);

        let n = params.n_vars();
        let mut dx = vec![0.0_f64; n];
        dx[n - 1] = 5.0e4; // pure ascent: increasing T worsens the merit for a feasible slack point

        let alpha = line_search(&params, &dx, 10.0, &model, 1.0, 1e-4, 20);

        // Reconstruct the accepted point x0 + alpha*dx; treat unpack failure as infinity.
        let x0 = params.variable_vector();
        let x_acc: Vec<f64> = x0.iter().zip(dx.iter()).map(|(xi, di)| xi + alpha * di).collect();
        let m_acc = match params.unpack_variable_vector(&x_acc) {
            Some(p_acc) => merit(&p_acc, &model, 10.0),
            None => f64::INFINITY,
        };

        assert!(
            m_acc <= m0 + 1e-9,
            "line search accepted a step that increased merit: m0={m0:.6}, m_acc={m_acc:.6}, alpha={alpha:.6}"
        );
    }

    // ── Step-15 tests: SQP driver convergence ────────────────────────────────

    fn gantry_fixture(t_init: f64) -> (TotsParams, TotsModel) {
        let params = TotsParams {
            joints: vec![JointWaypoints {
                start: 0.0,
                interior: vec![0.5],
                end: 1.0,
                vel_limit: 5.0,
                acc_limit: 50.0,
                max_force: 100.0,
            }],
            t_initial: t_init,
            vib_tol: 1e-3,
            n_grid: 30,
        };
        let model = gantry_model(1.0, 5.0, 0.05);
        (params, model)
    }

    /// 1-DOF gantry: Converged, iterations ≤ 100, result shorter than baseline.
    #[test]
    fn sqp_gantry_converges() {
        let (params, model) = gantry_fixture(3.0);
        let baseline_t = params.t_initial;
        let config = SqpConfig::default();
        let result = solve_tots(params, &model, &config);

        assert_eq!(result.outcome, TotsOutcome::Converged,
            "expected Converged, got {:?}", result.outcome);
        assert!(result.iterations <= 100, "iterations={}", result.iterations);
        assert!(result.duration() <= baseline_t + 1e-6,
            "result T={:.4} should be ≤ baseline T={:.4}", result.duration(), baseline_t);
        assert!(result.evaluation.vib_peak <= 1e-3 + 1e-9,
            "vib_peak={:.4e}", result.evaluation.vib_peak);
    }

    // ── Step-17 tests: non-convergence ───────────────────────────────────────

    /// max_iters=2 → NonConvergence, code_str, finite params.
    #[test]
    fn sqp_non_convergence_with_max_iters_2() {
        let (params, model) = gantry_fixture(3.0);
        let config = SqpConfig { max_iters: 2, ..Default::default() };
        let result = solve_tots(params, &model, &config);

        assert_eq!(result.outcome, TotsOutcome::NonConvergence,
            "expected NonConvergence, got {:?}", result.outcome);
        assert_eq!(result.outcome.code_str(), Some("W_TrajectorySolverNonConvergence"));
        assert!(result.duration().is_finite(), "duration should be finite");
        assert!(result.duration() > 0.0, "duration should be > 0");
    }

    // ── Step-19 tests: infeasibility detection ────────────────────────────────

    /// velocity_limit=0 on nonzero P2P → ConstraintInfeasible.
    #[test]
    fn sqp_infeasible_zero_velocity_limit() {
        let params = TotsParams {
            joints: vec![JointWaypoints {
                start: 0.0,
                interior: vec![0.5],
                end: 1.0,
                vel_limit: 0.0,   // impossible: any motion requires nonzero velocity
                acc_limit: 50.0,
                max_force: 100.0,
            }],
            t_initial: 2.0,
            vib_tol: 1e-3,
            n_grid: 30,
        };
        let model = gantry_model(1.0, 5.0, 0.05);
        let config = SqpConfig {
            max_iters: 20,
            infeasibility_threshold: 1e-9,  // very tight: detect immediately
            infeasibility_min_iters: 1,
            ..Default::default()
        };
        let result = solve_tots(params, &model, &config);

        assert_eq!(result.outcome, TotsOutcome::ConstraintInfeasible,
            "expected ConstraintInfeasible, got {:?}", result.outcome);
        assert_eq!(result.outcome.code_str(), Some("E_TrajectoryConstraintInfeasible"));
        assert!(result.iterations <= 20, "should exit quickly");
        assert!(result.duration().is_finite(), "params should be finite");
    }
}
