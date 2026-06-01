//! Lagrange-multiplier closed-chain inverse-dynamics kernel.
//!
//! Implements the augmented KKT / saddle-point system that extends
//! the open-chain RNEA result to mechanisms with loop closures
//! (`docs/prds/v0_3/rigid-body-dynamics.md` §5.3 / task RBD-ζ, Phase 3).
//!
//! **Pure-Rust `f64` numerics — no Reify-level `Value` dispatch.**
//! Assembly of the joint-space inertia matrix M (via unit-acceleration RNEA /
//! CRBA), the loop-constraint Jacobian A(θ) from
//! `loop_closure::chain_jacobian_fd`, and the acceleration-level constraint
//! RHS −Ȧ·θ̇ are all deferred to the consumer task RBD-η (Phase 4
//! eval-side dispatch), mirroring the `rnea.rs` module boundary exactly.
//! The end-to-end `examples/dynamics/closed_4bar_idyn.ri` execution
//! (ΔKE+ΔPE 1 µJ virtual-work check over a trajectory) is also η's
//! responsibility.
//!
//! # Solved system
//!
//! Given:
//! - M ∈ ℝ^{n×n} SPD joint-space inertia,
//! - τ_open ∈ ℝ^n open-chain RNEA generalized force,
//! - A ∈ ℝ^{m×n} loop-constraint Jacobian,
//! - b ∈ ℝ^m acceleration-level constraint RHS (−Ȧ·θ̇),
//!
//! solve the symmetric KKT system
//!
//! ```text
//! K · z = r,   K = [[M, Aᵀ],[A, 0]],   z = [q̈; λ],   r = [τ_open; b]
//! ```
//!
//! and return q̈ = z[0..n], λ = z[n..n+m], and
//! τ = τ_open + Aᵀλ (= τ_open + τ_closed).
//!
//! # Solver choice
//!
//! K is symmetric **indefinite** (the zero (2,2) block causes both positive
//! and negative eigenvalues).  `reify-stdlib` carries no heavyweight linalg
//! dependency (Cargo.toml: reify-core/gcode/ir/tracing only), so we reuse
//! the un-pivoted LDLᵀ pattern from `loop_closure_solver::solve_normal_equations`.
//! **M-block-first ordering is correctness-critical**: leading principal
//! minors through row n are positive (M≻0), and the trailing Schur block
//! −A·M⁻¹·Aᵀ is negative-definite when A has full row rank, so the
//! un-pivoted factorization is valid.  Placing the zero block first would
//! yield a zero first pivot and break the factorization immediately.

/// Default pivot threshold below which the LDLᵀ factor is treated as
/// singular.  Mirrors `loop_closure_solver::DEFAULT_SINGULARITY_PIVOT_EPS`.
pub const DEFAULT_PIVOT_EPS: f64 = 1e-12;

/// Errors returned by [`solve_closed_chain`].
#[derive(Debug, Clone, PartialEq)]
pub enum ClosedChainError {
    /// One or more input slice lengths are inconsistent with the declared `n`
    /// and `m` dimensions (e.g. `m_matrix.len() != n * n`).
    DimensionMismatch,
    /// The augmented KKT matrix is (numerically) singular: either the
    /// joint-space inertia M is singular or A does not have full row rank,
    /// causing an absolute LDLᵀ pivot to fall below `pivot_eps`.
    Singular,
}

/// Outputs of [`solve_closed_chain`].
#[derive(Debug, Clone, PartialEq)]
pub struct ClosedChainSolution {
    /// Joint accelerations q̈ ∈ ℝ^n.
    pub q_ddot: Vec<f64>,
    /// Lagrange multipliers λ ∈ ℝ^m (constraint forces in generalised
    /// coordinates; empty when m = 0).
    pub lambda: Vec<f64>,
    /// Corrected generalised forces τ = τ_open + Aᵀλ ∈ ℝ^n.
    /// Constraint forces Aᵀλ do no virtual work on velocities satisfying
    /// the velocity-level loop closure A·θ̇ = 0 (D'Alembert / virtual-work
    /// principle), so τ·θ̇ = τ_open·θ̇ for any θ̇ ∈ ker(A).
    pub tau: Vec<f64>,
}

/// Solve the closed-chain inverse-dynamics KKT system.
///
/// # Parameters
/// - `m_matrix`: n×n row-major SPD joint-space inertia M(θ).
/// - `tau_open`: n-vector open-chain RNEA generalised force τ_open = Mq̈ + C + G.
/// - `a_matrix`: m×n row-major loop-constraint Jacobian A(θ);
///   produced by `loop_closure::chain_jacobian_fd` at the consumer (RBD-η).
/// - `accel_rhs`: m-vector acceleration-level constraint RHS −Ȧ·θ̇.
/// - `n`: number of independent DOFs (columns of A, size of M).
/// - `m`: number of loop-closure scalar constraints (rows of A).
/// - `pivot_eps`: absolute pivot threshold for singularity detection;
///   use [`DEFAULT_PIVOT_EPS`] unless you have a specific reason to change it.
///
/// # Workless-constraint property
///
/// The constraint forces Aᵀλ returned in `tau` satisfy the virtual-work
/// identity: for any velocity θ̇ with A·θ̇ = 0,
/// `(τ − τ_open) · θ̇ = λᵀ(A·θ̇) = 0`.
/// The end-to-end ΔKE + ΔPE 1 µJ check over a trajectory is delivered by
/// consumer RBD-η via `examples/dynamics/closed_4bar_idyn.ri`.
///
/// # Errors
/// - [`ClosedChainError::DimensionMismatch`] if any slice length is inconsistent.
/// - [`ClosedChainError::Singular`] if the KKT matrix is numerically singular.
pub fn solve_closed_chain(
    m_matrix: &[f64],
    tau_open: &[f64],
    a_matrix: &[f64],
    accel_rhs: &[f64],
    n: usize,
    m: usize,
    pivot_eps: f64,
) -> Result<ClosedChainSolution, ClosedChainError> {
    // ── Input validation ──────────────────────────────────────────────────────
    if m_matrix.len() != n * n
        || tau_open.len() != n
        || a_matrix.len() != m * n
        || accel_rhs.len() != m
    {
        return Err(ClosedChainError::DimensionMismatch);
    }

    let k = n + m; // size of the augmented KKT system

    if m == 0 {
        // ── m=0 fast path: K = M (SPD), no constraint rows/columns ──────────
        let mut a_work: Vec<f64> = m_matrix.to_vec();
        let mut b_work: Vec<f64> = tau_open.to_vec();
        if !ldlt_solve_symmetric(&mut a_work, &mut b_work, n, pivot_eps) {
            return Err(ClosedChainError::Singular);
        }
        return Ok(ClosedChainSolution {
            q_ddot: b_work,
            lambda: vec![],
            tau: tau_open.to_vec(),
        });
    }

    // ── General m>0 path: assemble the (n+m)×(n+m) symmetric KKT matrix ────
    //
    // Layout (M-block FIRST — correctness-critical; see module doc):
    //   K = [ M   Aᵀ ]   rows 0..n,   cols 0..n  → M
    //       [ A    0 ]   rows n..n+m, cols 0..n  → A
    //                    rows 0..n,   cols n..k  → Aᵀ
    //                    rows n..k,   cols n..k  → 0
    let mut kkt: Vec<f64> = vec![0.0; k * k];

    // Top-left n×n block: M
    for i in 0..n {
        for j in 0..n {
            kkt[i * k + j] = m_matrix[i * n + j];
        }
    }
    // Top-right n×m block: Aᵀ  (kkt[i, n+j] = a_matrix[j*n+i])
    // Bottom-left m×n block: A  (kkt[n+i, j] = a_matrix[i*n+j])
    for i in 0..m {
        for j in 0..n {
            let a_val = a_matrix[i * n + j];
            kkt[(n + i) * k + j] = a_val; // A block
            kkt[j * k + (n + i)] = a_val; // Aᵀ block
        }
    }
    // Bottom-right m×m block remains 0 (already zeroed).

    // Debug-mode symmetry self-check.
    #[cfg(debug_assertions)]
    for i in 0..k {
        for j in 0..k {
            debug_assert_eq!(
                kkt[i * k + j],
                kkt[j * k + i],
                "KKT symmetry violated at [{i},{j}]"
            );
        }
    }

    // RHS: [τ_open; accel_rhs]
    let mut rhs: Vec<f64> = Vec::with_capacity(k);
    rhs.extend_from_slice(tau_open);
    rhs.extend_from_slice(accel_rhs);

    // Solve K · z = rhs in-place.
    if !ldlt_solve_symmetric(&mut kkt, &mut rhs, k, pivot_eps) {
        return Err(ClosedChainError::Singular);
    }

    let q_ddot = rhs[..n].to_vec();
    let lambda = rhs[n..k].to_vec();

    // τ = τ_open + Aᵀλ
    let mut tau: Vec<f64> = tau_open.to_vec();
    for i in 0..n {
        for j in 0..m {
            tau[i] += a_matrix[j * n + i] * lambda[j];
        }
    }

    Ok(ClosedChainSolution { q_ddot, lambda, tau })
}

// ── Private LDLᵀ solver ───────────────────────────────────────────────────────

/// Solve `A · x = b` in-place for a k×k symmetric matrix.
///
/// Uses un-pivoted LDLᵀ factorisation (strict-lower triangle → L with unit
/// diagonal; diagonal → D).  The pivot guard tests `|d_jj| < pivot_eps` —
/// **not** `d_jj > 0` — so negative pivots in the trailing Schur block of the
/// indefinite KKT are accepted, provided the SPD M-block is ordered first.
///
/// Returns `true` on success (`b` holds the solution), `false` if any pivot is
/// too small (singular or near-singular matrix).
fn ldlt_solve_symmetric(a: &mut [f64], b: &mut [f64], k: usize, pivot_eps: f64) -> bool {
    if k == 0 {
        return true;
    }
    debug_assert_eq!(a.len(), k * k);
    debug_assert_eq!(b.len(), k);

    // LDLᵀ factorisation: overwrite a so that strict-lower triangle = L
    // (unit diagonal implied), diagonal = D.
    for j in 0..k {
        // D[j,j] = a[j,j] − Σ_{p<j} L[j,p]² · D[p,p]
        let mut d_jj = a[j * k + j];
        for p in 0..j {
            d_jj -= a[j * k + p] * a[j * k + p] * a[p * k + p];
        }
        // Abs-pivot guard: allows negative pivots (indefinite KKT trailing block).
        if d_jj.abs() < pivot_eps {
            return false;
        }
        a[j * k + j] = d_jj;
        // L[i,j] = (a[i,j] − Σ_{p<j} L[i,p]·L[j,p]·D[p,p]) / D[j,j]  for i > j
        for i in (j + 1)..k {
            let mut s = a[i * k + j];
            for p in 0..j {
                s -= a[i * k + p] * a[j * k + p] * a[p * k + p];
            }
            a[i * k + j] = s / d_jj;
        }
    }
    // Forward solve L · y = b.
    for i in 0..k {
        let mut s = b[i];
        for p in 0..i {
            s -= a[i * k + p] * b[p];
        }
        b[i] = s;
    }
    // Diagonal solve D · z = y.
    for i in 0..k {
        b[i] /= a[i * k + i];
    }
    // Back solve Lᵀ · x = z.
    for i in (0..k).rev() {
        let mut s = b[i];
        for p in (i + 1)..k {
            s -= a[p * k + i] * b[p];
        }
        b[i] = s;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: assert two f64 slices are element-wise within `tol`.
    fn assert_near(label: &str, got: &[f64], want: &[f64], tol: f64) {
        assert_eq!(got.len(), want.len(), "{label}: length mismatch");
        for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert!(
                (g - w).abs() <= tol,
                "{label}[{i}]: got {g:.6e}, want {w:.6e}, diff {:.2e} > tol {tol:.2e}",
                (g - w).abs()
            );
        }
    }

    // S3: analytic 2-DOF + 1-constraint, hand-solved closed-form.
    // M = diag(2,3), A = [1,1] (m=1, n=2), τ_open=[10,6], accel_rhs=[1].
    // KKT 3×3: {2q̈₀ + λ = 10 ; 3q̈₁ + λ = 6 ; q̈₀ + q̈₁ = 1}
    // Solving: from rows 1&2: q̈₀=(10-λ)/2, q̈₁=(6-λ)/3.
    // Row 3: (10-λ)/2 + (6-λ)/3 = 1 → 15 + 12 - 5λ = 6 → λ = (27-6)/5 = 21/5 = 4.2
    // Wait, let me redo: 3(10-λ)/6 + 2(6-λ)/6 = 1 → 30-3λ+12-2λ=6 → 42-5λ=6 → λ=36/5=7.2
    // q̈₀ = (10-7.2)/2 = 1.4, q̈₁ = (6-7.2)/3 = -0.4. Check: 1.4-0.4=1 ✓
    // τ = [10+7.2, 6+7.2] = [17.2, 13.2]
    #[test]
    fn analytic_two_dof_single_constraint() {
        let m_matrix = [2.0_f64, 0.0, 0.0, 3.0]; // diag(2,3), n=2
        let tau_open = [10.0_f64, 6.0];
        let a_matrix = [1.0_f64, 1.0]; // m=1, n=2
        let accel_rhs = [1.0_f64];

        let sol = solve_closed_chain(
            &m_matrix,
            &tau_open,
            &a_matrix,
            &accel_rhs,
            2,
            1,
            DEFAULT_PIVOT_EPS,
        )
        .expect("analytic case should succeed");

        assert_near("q_ddot", &sol.q_ddot, &[1.4, -0.4], 1e-12);
        assert_near("lambda", &sol.lambda, &[7.2], 1e-12);
        assert_near("tau", &sol.tau, &[17.2, 13.2], 1e-12);
    }

    // S1: m=0 (no constraints) must reduce to the open-chain system M·q̈ = τ_open.
    // Inputs: M = diag(2,4), τ_open = [6,8], m=0 ⇒ q̈ = [6/2, 8/4] = [3, 2].
    #[test]
    fn no_constraints_reduces_to_open_chain() {
        let m_matrix = [2.0_f64, 0.0, 0.0, 4.0]; // 2×2 diagonal, row-major
        let tau_open = [6.0_f64, 8.0];
        let a_matrix: &[f64] = &[];
        let accel_rhs: &[f64] = &[];
        let n = 2;
        let m = 0;

        let sol = solve_closed_chain(
            &m_matrix,
            &tau_open,
            a_matrix,
            accel_rhs,
            n,
            m,
            DEFAULT_PIVOT_EPS,
        )
        .expect("m=0 solve should succeed");

        assert_near("q_ddot", &sol.q_ddot, &[3.0, 2.0], 1e-12);
        assert!(sol.lambda.is_empty(), "lambda must be empty for m=0");
        assert_near("tau", &sol.tau, &[6.0, 8.0], 1e-12);
    }
}
