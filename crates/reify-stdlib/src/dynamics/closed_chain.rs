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

/// Reduce a raw loop-constraint Jacobian to full row rank.
///
/// # Policy (resolves GAP 3)
///
/// A raw 6-row Jacobian from [`loop_residual_twist`](crate::loop_closure) is
/// structurally rank-deficient for sub-6-DOF loops:
///
/// 1. **Closing-joint projection** — rows corresponding to directions the
///    closing joint absorbs (its motion-subspace `S_close`) are zeroed by
///    projecting each raw row onto the orthogonal complement of S_close.
///    For a revolute pin about +y, `S_close = [[0,1,0, 0,0,0]]` and the
///    ωy row is zeroed; the kinematic Newton solve zeros the residual in that
///    direction by adjusting the closing joint's free coordinate, so keeping
///    the ωy row would over-constrain the mechanism.
///
/// 2. **Numerical row reduction** — Gaussian elimination with partial row
///    pivoting collects rows whose pivot exceeds `eps`.  Zero rows from step 1
///    (and any other structurally zero or linearly dependent rows) are discarded.
///
/// # Parameters
/// - `a_full`: raw constraint Jacobian, row-major, shape `rows × cols`.
/// - `rows`, `cols`: dimensions of `a_full`.
/// - `closing_subspace`: motion-subspace columns of the closing joint,
///   each as a `[f64; 6]` twist vector.  Pass `&[]` to skip projection.
/// - `eps`: row-pivot threshold for the row-reduction step.
///
/// # Returns
/// `(a_reduced, m_eff)` — the `m_eff × cols` reduced matrix (row-major) and
/// the effective row count.  `m_eff = 0` means A is zero (no constraints active).
pub fn reduce_constraint_rank(
    a_full: &[f64],
    rows: usize,
    cols: usize,
    closing_subspace: &[[f64; 6]],
    eps: f64,
) -> (Vec<f64>, usize) {
    // ── Step 1: orthonormalize S_close and project each row ──────────────────
    // Build an orthonormal basis for the closing-subspace columns using
    // Gram-Schmidt (in-place, storing the orthonormal vectors in `basis`).
    let mut basis: Vec<[f64; 6]> = Vec::with_capacity(closing_subspace.len());
    for &col in closing_subspace {
        let mut v = col;
        // Subtract projections onto already-orthonormalised basis vectors.
        for b in &basis {
            let dot: f64 = v.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
            for k in 0..6 {
                v[k] -= dot * b[k];
            }
        }
        let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > eps {
            let scale = 1.0 / norm;
            for x in &mut v {
                *x *= scale;
            }
            basis.push(v);
        }
    }

    // Compute the complement scale for each row.
    //
    // Each of the `rows` rows corresponds to one twist component (row index ri).
    // The closing joint's basis spans certain twist directions.  Row ri is
    // "absorbed" by the closing joint when the canonical unit vector e_ri lies
    // in span(basis), i.e. when Σ_b b[ri]² ≈ 1.  We scale each row by the
    // complement factor (1 − Σ_b b[ri]²), clamped to [0, 1], so absorbed rows
    // become zero and independent rows are unchanged (or partially attenuated
    // for oblique subspaces).
    let mut row_scales: Vec<f64> = Vec::with_capacity(rows);
    for ri in 0..rows {
        let mut scale_complement = 1.0_f64;
        for b in &basis {
            scale_complement -= b[ri] * b[ri];
        }
        row_scales.push(scale_complement.clamp(0.0, 1.0));
    }

    // Build the projected rows as an actual matrix for Gaussian elimination.
    // Each entry is (original_row_index, projected_row_vec) so we can recover
    // the original unscaled row once an independent pivot is found.
    let mut remaining: Vec<(usize, Vec<f64>)> = (0..rows)
        .map(|ri| {
            let scale = row_scales[ri];
            let row: Vec<f64> = (0..cols).map(|j| a_full[ri * cols + j] * scale).collect();
            (ri, row)
        })
        .collect();

    // ── Step 2: Gaussian elimination with partial row pivoting ───────────────
    //
    // We use the PROJECTED rows for the elimination (so zero-scaled rows never
    // attract a pivot) but record the ORIGINAL row index of each pivot found.
    // The returned reduced matrix is built from the original unscaled rows,
    // not the elimination-modified ones, so each output row is a scalar multiple
    // of a row from `a_full` (required by the virtual-work identity test).
    let mut pivot_original_indices: Vec<usize> = Vec::new();
    let mut pivot_col = 0usize;

    while pivot_col < cols && !remaining.is_empty() {
        // Find the row with the largest absolute value in column `pivot_col`.
        let (local_idx, pivot_val) = remaining
            .iter()
            .enumerate()
            .map(|(i, (_, row))| (i, row[pivot_col].abs()))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap_or((0, 0.0));

        if pivot_val <= eps {
            // No row has a large-enough pivot in this column — advance column.
            pivot_col += 1;
            continue;
        }

        // Extract the pivot row (for elimination) and record its original index.
        let (orig_idx, pivot_row) = remaining.remove(local_idx);
        let pv = pivot_row[pivot_col];

        // Eliminate this column from all remaining rows (projected versions).
        for (_, row) in &mut remaining {
            let factor = row[pivot_col] / pv;
            for j in 0..cols {
                row[j] -= factor * pivot_row[j];
            }
        }

        pivot_original_indices.push(orig_idx);
        pivot_col += 1;
    }

    // Return the ORIGINAL (unscaled) rows for the identified pivot indices.
    // Each returned row is exactly a_full[orig_idx * cols .. (orig_idx+1) * cols].
    let m_eff = pivot_original_indices.len();
    let mut a_red: Vec<f64> = Vec::with_capacity(m_eff * cols);
    for orig_idx in pivot_original_indices {
        a_red.extend_from_slice(&a_full[orig_idx * cols..(orig_idx + 1) * cols]);
    }
    (a_red, m_eff)
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
///
/// **Limitation:** Without pivoting, a near-zero intermediate pivot can trigger
/// a false `Singular` return even for a full-rank indefinite K if the system is
/// ill-conditioned between the M-block and the Schur complement.  Callers
/// needing maximum robustness should switch to a pivoted or Bunch-Kaufman
/// factorisation (out of scope here given the no-linalg-dependency constraint).
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
    use super::{reduce_constraint_rank, solve_closed_chain, ClosedChainError, DEFAULT_PIVOT_EPS};

    // ── reduce_constraint_rank: planar revolute 4-bar row filter ─────────────
    //
    // Builds a synthetic 6×3 raw A that mimics the raw twist Jacobian of a
    // planar revolute 4-bar (mechanism in the x-z plane, revolute pins about +y):
    //
    //   ωx row = [0, 0, 0]   — structurally zero (planar motion)
    //   ωy row = [a, b, c]   — nonzero BUT absorbed by closing revolute (+y)
    //   ωz row = [0, 0, 0]   — structurally zero
    //   vx row = [d, e, f]   — active constraint
    //   vy row = [0, 0, 0]   — structurally zero (planar motion)
    //   vz row = [g, h, k]   — active constraint
    //
    // Closing-joint motion subspace S_close = [[0,1,0, 0,0,0]] (+y revolute).
    // After projecting out S_close and row-reducing, expected m_eff = 2
    // (vx-row, vz-row).
    //
    // A second case checks that a duplicate row is dropped (full-row-rank
    // reduction leaves m_eff = 1 for a 2×3 A with identical rows).
    //
    // Achievability: deterministic linear algebra — exact expected result.
    #[test]
    fn reduce_constraint_rank_drops_closing_freedom_and_zero_rows() {
        let eps = 1e-10_f64;

        // ── case 1: planar revolute 4-bar synthetic A (6×3) ─────────────────
        let d = 1.3_f64;
        let e = -0.7_f64;
        let f = 0.4_f64;
        let g = -0.5_f64;
        let h = 0.9_f64;
        let k = -1.1_f64;
        let a_wy = 2.0_f64;
        let b_wy = -1.5_f64;
        let c_wy = 0.8_f64;
        // Row order: [ωx, ωy, ωz, vx, vy, vz] (6 rows) × 3 cols (row-major).
        let a_raw: Vec<f64> = vec![
            0.0, 0.0, 0.0,    // ωx
            a_wy, b_wy, c_wy, // ωy — absorbed by closing revolute +y
            0.0, 0.0, 0.0,    // ωz
            d,   e,   f,      // vx — active
            0.0, 0.0, 0.0,    // vy
            g,   h,   k,      // vz — active
        ];
        // Closing revolute about +y: S_close = [0,1,0, 0,0,0].
        let s_close: [f64; 6] = [0.0, 1.0, 0.0, 0.0, 0.0, 0.0];

        let (a_red, m_eff) = reduce_constraint_rank(&a_raw, 6, 3, &[s_close], eps);
        assert_eq!(m_eff, 2, "case1: expected m_eff=2 (vx-row + vz-row)");
        assert_eq!(a_red.len(), 2 * 3, "case1: reduced A is 2×3");

        // The two independent rows must span vx-row and vz-row (up to row order
        // and possible row scaling — we check that each retained row is a scalar
        // multiple of either vx-row or vz-row by verifying the cross-product
        // of the reduced row with the expected is zero).
        let vx_row = [d, e, f];
        let vz_row = [g, h, k];

        let is_parallel_to = |row: &[f64], ref_row: &[f64; 3]| -> bool {
            // row × ref_row == 0 ⟺ parallel (in ℝ³)
            // Use |row × ref_row|² / (|row|²·|ref_row|²) < eps²
            let cx = row[1] * ref_row[2] - row[2] * ref_row[1];
            let cy = row[2] * ref_row[0] - row[0] * ref_row[2];
            let cz = row[0] * ref_row[1] - row[1] * ref_row[0];
            let cross_sq = cx * cx + cy * cy + cz * cz;
            let norm_sq = row.iter().map(|x| x * x).sum::<f64>()
                * ref_row.iter().map(|x| x * x).sum::<f64>();
            cross_sq < 1e-20 * norm_sq.max(1e-30)
        };

        let row0 = &a_red[0..3];
        let row1 = &a_red[3..6];
        let row0_ok = is_parallel_to(row0, &vx_row) || is_parallel_to(row0, &vz_row);
        let row1_ok = is_parallel_to(row1, &vx_row) || is_parallel_to(row1, &vz_row);
        assert!(row0_ok, "case1: reduced row0 must be parallel to vx-row or vz-row");
        assert!(row1_ok, "case1: reduced row1 must be parallel to vx-row or vz-row");
        // The two rows must not be parallel to each other.
        assert!(
            !is_parallel_to(row0, &[row1[0], row1[1], row1[2]]),
            "case1: two reduced rows must be independent"
        );

        // ── case 2: 2×3 A with duplicate rows → m_eff = 1 ──────────────────
        let a_dup: Vec<f64> = vec![
            1.0, -2.0, 3.0,
            1.0, -2.0, 3.0,
        ];
        // No closing subspace to project (pass empty slice).
        let (a_red2, m_eff2) = reduce_constraint_rank(&a_dup, 2, 3, &[], eps);
        assert_eq!(m_eff2, 1, "case2: duplicate rows → m_eff=1");
        assert_eq!(a_red2.len(), 3, "case2: 1×3 reduced A");
        assert!(
            is_parallel_to(&a_red2, &[1.0, -2.0, 3.0]),
            "case2: retained row must be parallel to original"
        );
    }

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

    // S9: error guards — rank-deficient A ⇒ Singular; bad slice lengths ⇒ DimensionMismatch.
    #[test]
    fn singular_and_dimension_guards() {
        // (a) Rank-deficient A: duplicate rows ⇒ Singular.
        // M = diag(2,3,4), n=3, A = [[1,0,0],[1,0,0]] (m=2, duplicate rows)
        let m_diag3 = [2.0_f64, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 4.0];
        let a_dup = [1.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0]; // two identical rows
        let tau3 = [1.0_f64, 2.0, 3.0];
        let rhs2 = [0.0_f64, 0.0];
        let result = solve_closed_chain(&m_diag3, &tau3, &a_dup, &rhs2, 3, 2, DEFAULT_PIVOT_EPS);
        assert_eq!(
            result,
            Err(ClosedChainError::Singular),
            "duplicate-row A should yield Singular"
        );

        // (b) Dimension mismatch: tau_open wrong length.
        let result2 = solve_closed_chain(
            &m_diag3,
            &tau3[..2], // length 2 instead of 3
            &a_dup,
            &rhs2,
            3,
            2,
            DEFAULT_PIVOT_EPS,
        );
        assert_eq!(
            result2,
            Err(ClosedChainError::DimensionMismatch),
            "short tau_open should yield DimensionMismatch"
        );

        // (c) Dimension mismatch: a_matrix wrong length.
        let result3 = solve_closed_chain(
            &m_diag3,
            &tau3,
            &a_dup[..4], // length 4 instead of m*n=6
            &rhs2,
            3,
            2,
            DEFAULT_PIVOT_EPS,
        );
        assert_eq!(
            result3,
            Err(ClosedChainError::DimensionMismatch),
            "short a_matrix should yield DimensionMismatch"
        );

        // (d) Dimension mismatch: m_matrix wrong length.
        let result4 = solve_closed_chain(
            &m_diag3[..8], // length 8 instead of n*n=9
            &tau3,
            &a_dup,
            &rhs2,
            3,
            2,
            DEFAULT_PIVOT_EPS,
        );
        assert_eq!(
            result4,
            Err(ClosedChainError::DimensionMismatch),
            "short m_matrix should yield DimensionMismatch"
        );

        // (e) Dimension mismatch: accel_rhs wrong length.
        let result5 = solve_closed_chain(
            &m_diag3,
            &tau3,
            &a_dup,
            &rhs2[..1], // length 1 instead of m=2
            3,
            2,
            DEFAULT_PIVOT_EPS,
        );
        assert_eq!(
            result5,
            Err(ClosedChainError::DimensionMismatch),
            "short accel_rhs should yield DimensionMismatch"
        );
    }

    // S7: workless-constraint / virtual-work identity.
    // A=[1,-1] (m=1,n=2), θ̇=[1,1] satisfies A·θ̇=0 exactly.
    // Constraint torque Aᵀλ does zero net power on θ̇ ⇒ τ·θ̇ = τ_open·θ̇.
    #[test]
    fn constraint_forces_are_workless_virtual_work() {
        let m_matrix = [2.0_f64, 0.0, 0.0, 3.0]; // diag(2,3)
        let tau_open = [5.0_f64, -2.0];
        let a_matrix = [1.0_f64, -1.0]; // m=1, n=2
        let accel_rhs = [0.3_f64]; // arbitrary nonzero
        let theta_dot = [1.0_f64, 1.0]; // lies in ker(A): A·θ̇ = 1-1 = 0

        let sol = solve_closed_chain(
            &m_matrix,
            &tau_open,
            &a_matrix,
            &accel_rhs,
            2,
            1,
            DEFAULT_PIVOT_EPS,
        )
        .expect("workless constraint fixture should succeed");

        // (a) All outputs finite/non-NaN.
        for (i, &l) in sol.lambda.iter().enumerate() {
            assert!(l.is_finite(), "lambda[{i}] is not finite");
        }
        for (i, &t) in sol.tau.iter().enumerate() {
            assert!(t.is_finite(), "tau[{i}] is not finite");
        }

        // (b) Virtual-work identity: τ·θ̇ = τ_open·θ̇ (to 1e-10).
        let power_tau: f64 = sol.tau.iter().zip(&theta_dot).map(|(t, v)| t * v).sum();
        let power_open: f64 = tau_open.iter().zip(&theta_dot).map(|(t, v)| t * v).sum();
        let diff = (power_tau - power_open).abs();
        assert!(
            diff < 1e-10,
            "virtual-work identity violated: |τ·θ̇ - τ_open·θ̇| = {diff:.2e} > 1e-10"
        );

        // (c) Pin the solved λ — hand-derived closed form (to 1e-10).
        // KKT (3×3): 2q̈₀+λ=5, 3q̈₁−λ=−2, q̈₀−q̈₁=0.3 ⇒ 19−5λ=1.8 ⇒ 5λ=17.2 ⇒ λ=3.44
        assert_near("lambda", &sol.lambda, &[17.2 / 5.0], 1e-10);
    }

    // S5: 4×4 SPD non-diagonal M + full-row-rank 2×4 A — verify KKT residuals.
    // Build M = L·Lᵀ + I for L lower-triangular, ensuring SPD.
    // A has two independent rows (full row rank).
    #[test]
    fn augmented_kkt_residual_four_bar_like() {
        // Lower-triangular L (4×4).
        // M = L·Lᵀ + I to guarantee SPD.
        let l = [
            [1.0_f64, 0.0, 0.0, 0.0],
            [0.5, 1.0, 0.0, 0.0],
            [0.2, 0.3, 1.0, 0.0],
            [0.1, 0.4, 0.6, 1.0],
        ];
        let n = 4;
        let m = 2;
        // Compute L·Lᵀ + I row-major.
        let mut m_matrix = [0.0_f64; 16];
        for i in 0..n {
            m_matrix[i * n + i] = 1.0; // +I
            for (k, &l_ik) in l[i].iter().enumerate() {
                for j in 0..n {
                    m_matrix[i * n + j] += l_ik * l[j][k];
                }
            }
        }

        // A: 2×4, two independent rows mimicking a planar 4-bar loop residual.
        let a_matrix = [1.0_f64, -1.0, 0.0, 0.0, 0.0, 1.0, -1.0, 0.0];

        let tau_open = [3.0_f64, -1.0, 2.5, 0.7];
        let accel_rhs = [0.1_f64, -0.3];

        let sol = solve_closed_chain(
            &m_matrix,
            &tau_open,
            &a_matrix,
            &accel_rhs,
            n,
            m,
            DEFAULT_PIVOT_EPS,
        )
        .expect("4×4 SPD M + 2×4 A should succeed");

        // Verify block-row 1: M·q̈ + Aᵀλ = τ_open
        let tol = 1e-9;
        for i in 0..n {
            let mut lhs = 0.0;
            for j in 0..n {
                lhs += m_matrix[i * n + j] * sol.q_ddot[j];
            }
            for j in 0..m {
                lhs += a_matrix[j * n + i] * sol.lambda[j]; // Aᵀλ
            }
            assert!(
                (lhs - tau_open[i]).abs() < tol,
                "M·q̈ + Aᵀλ residual[{i}] = {:.2e} (tol {tol:.1e})",
                (lhs - tau_open[i]).abs()
            );
        }
        // Verify block-row 2: A·q̈ = accel_rhs
        for i in 0..m {
            let mut lhs = 0.0;
            for j in 0..n {
                lhs += a_matrix[i * n + j] * sol.q_ddot[j];
            }
            assert!(
                (lhs - accel_rhs[i]).abs() < tol,
                "A·q̈ residual[{i}] = {:.2e} (tol {tol:.1e})",
                (lhs - accel_rhs[i]).abs()
            );
        }
        // All λ must be finite.
        for (i, &l) in sol.lambda.iter().enumerate() {
            assert!(l.is_finite(), "lambda[{i}] is not finite");
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
