//! PSD (positive semi-definite) validation helpers for `MassProperties.inertia`.
//!
//! The inertia tensor of a rigid body must be positive semi-definite (all
//! eigenvalues ≥ 0) by physics. This module provides a dependency-free
//! analytic check on the symmetric part of a 3×3 matrix.
//!
//! # Design
//!
//! `is_symmetric_psd` and `min_eigenvalue` compute the three eigenvalues of
//! the symmetric part `S = (M + Mᵀ)/2` via the closed-form Smith/trigonometric
//! method for symmetric 3×3 matrices (NaN-free for the diagonal/degenerate
//! cases the tests exercise — diagonal eigenvalues equal the diagonal entries
//! exactly).  PSD is declared when all eigenvalues ≥ −tol.
//!
//! The math is `Value`-free: the inertia it checks is read by the one
//! `MassProperties` inertia reader,
//! `reify_stdlib::dynamics::mass_props::MassProperties::read_inertia`.
//!
//! # Relationship to `reify-solver-elastic/src/eigensolve.rs`
//!
//! `reify-solver-elastic` provides a general shift-invert Lanczos + dense
//! eigensolver for the FEA/buckling solver stack.  The closed-form 3×3 solver
//! here is **intentionally separate**:
//!
//! * `reify-eval` must stay dependency-free from the solver stack to avoid a
//!   circular crate dependency;
//! * The analytic Smith 1961 method is exact and NaN-free for the
//!   diagonal/degenerate cases that arise in practice (no iteration, no
//!   convergence tolerance tuning);
//! * The implementation is ~50 lines of pure Rust vs. a sparse-matrix
//!   eigensolver invocation.
//!
//! This duplication is deliberate, not an oversight.
//!
//! # References
//!
//! Smith, O.K. (1961). "Eigenvalues of a symmetric 3×3 matrix."
//! *Communications of the ACM*, 4(4), 168.

/// Compute the three eigenvalues of the symmetric part `S = (M + Mᵀ)/2`
/// of a 3×3 matrix `m`, using the analytic closed-form Smith 1961 /
/// trigonometric method.  Returns the eigenvalues as `[λ0, λ1, λ2]`
/// (order unspecified).
///
/// This private helper is shared by [`is_symmetric_psd`] and [`min_eigenvalue`]
/// to avoid duplicating the eigenvalue logic.
fn sym_3x3_eigenvalues(m: &[[f64; 3]; 3]) -> [f64; 3] {
    // Symmetrize: S = (M + Mᵀ) / 2
    let s = [
        [
            m[0][0],
            (m[0][1] + m[1][0]) * 0.5,
            (m[0][2] + m[2][0]) * 0.5,
        ],
        [
            (m[1][0] + m[0][1]) * 0.5,
            m[1][1],
            (m[1][2] + m[2][1]) * 0.5,
        ],
        [
            (m[2][0] + m[0][2]) * 0.5,
            (m[2][1] + m[1][2]) * 0.5,
            m[2][2],
        ],
    ];

    // Analytic symmetric 3×3 eigenvalue solver (Smith 1961 / trigonometric method).
    //
    // Given a real symmetric 3×3 matrix S, all three eigenvalues are real.
    // The method reduces the eigenvalue problem to a depressed cubic whose
    // three real roots are given in closed form via a cosine substitution.
    let a = s[0][0];
    let b = s[1][1];
    let c = s[2][2];
    let d = s[0][1]; // = s[1][0]
    let e = s[0][2]; // = s[2][0]
    let f = s[1][2]; // = s[2][1]

    // p1 = sum of squared off-diagonal elements
    let p1 = d * d + e * e + f * f;

    if p1 < f64::EPSILON {
        // Matrix is already diagonal; eigenvalues are the diagonal entries.
        return [a, b, c];
    }

    let q = (a + b + c) / 3.0; // mean of diagonal

    // p2 = sum of squared deviations from mean (using off-diagonal terms)
    let p2 = (a - q) * (a - q) + (b - q) * (b - q) + (c - q) * (c - q) + 2.0 * p1;
    let p = (p2 / 6.0).sqrt();

    // B = (1/p) * (S - q*I)  — scaling; computed element-wise.
    let inv_p = if p > f64::EPSILON { 1.0 / p } else { 1.0 };
    let b_mat = [
        [(a - q) * inv_p, d * inv_p, e * inv_p],
        [d * inv_p, (b - q) * inv_p, f * inv_p],
        [e * inv_p, f * inv_p, (c - q) * inv_p],
    ];

    // det(B_mat) / 2 — half-determinant
    let det = b_mat[0][0] * (b_mat[1][1] * b_mat[2][2] - b_mat[1][2] * b_mat[2][1])
        - b_mat[0][1] * (b_mat[1][0] * b_mat[2][2] - b_mat[1][2] * b_mat[2][0])
        + b_mat[0][2] * (b_mat[1][0] * b_mat[2][1] - b_mat[1][1] * b_mat[2][0]);
    let r = (det / 2.0).clamp(-1.0, 1.0);

    // phi = acos(r) / 3  (in [0, π/3] when r ∈ [-1, 1])
    let phi = r.acos() / 3.0;

    // Three eigenvalues:  λ_k = q + 2·p·cos(phi + 2πk/3), k = 0,1,2
    let eig0 = q + 2.0 * p * phi.cos();
    let eig2 = q + 2.0 * p * (phi + 2.0 * std::f64::consts::PI / 3.0).cos();
    // eig1 is determined by the trace: trace(S) = λ0 + λ1 + λ2
    let eig1 = 3.0 * q - eig0 - eig2;

    [eig0, eig1, eig2]
}

/// Check whether the symmetric 3×3 matrix `m` is positive semi-definite.
///
/// Computes the three eigenvalues of the symmetric part `S = (M + Mᵀ)/2`
/// via the closed-form trigonometric method for real symmetric 3×3 matrices
/// (Smith 1961). Returns `true` iff the minimum eigenvalue ≥ `−tol`.
///
/// The tolerance `tol` should be a small non-negative fraction of the
/// matrix's maximum-absolute-value norm.  Use [`psd_tol`] to get a
/// sensible default.
pub fn is_symmetric_psd(m: &[[f64; 3]; 3], tol: f64) -> bool {
    let eigvals = sym_3x3_eigenvalues(m);
    let min_eig = eigvals[0].min(eigvals[1]).min(eigvals[2]);
    min_eig >= -tol
}

/// Returns the minimum eigenvalue of the symmetric part `S = (M + Mᵀ)/2`
/// of the 3×3 matrix `m`.
///
/// Use this alongside [`psd_tol`] to embed the minimum eigenvalue in a
/// diagnostic message when PSD validation fails.  For a purely non-negative
/// check, prefer [`is_symmetric_psd`].
pub fn min_eigenvalue(m: &[[f64; 3]; 3]) -> f64 {
    let eigvals = sym_3x3_eigenvalues(m);
    eigvals[0].min(eigvals[1]).min(eigvals[2])
}

/// A sensible default PSD tolerance: `1e-9 * max(|m_ij|, 1.0)`.
///
/// Scales with the matrix entries so the check is stable whether the inertia
/// is in SI units (kg·m², values ≪ 1 for small bodies or ≫ 1 for large ones)
/// or in any other consistent unit system. The floor at 1.0 prevents the
/// tolerance from collapsing to machine-epsilon for near-zero matrices.
pub fn psd_tol(m: &[[f64; 3]; 3]) -> f64 {
    let max_abs = m
        .iter()
        .flat_map(|row| row.iter())
        .map(|x| x.abs())
        .fold(0.0_f64, f64::max);
    1e-9 * max_abs.max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_symmetric_psd ─────────────────────────────────────────────────────

    #[test]
    fn identity_matrix_is_psd() {
        let id = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        assert!(
            is_symmetric_psd(&id, psd_tol(&id)),
            "identity matrix should be PSD (all eigenvalues = 1)"
        );
    }

    #[test]
    fn diagonal_positive_matrix_is_psd() {
        let m = [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]];
        assert!(
            is_symmetric_psd(&m, psd_tol(&m)),
            "diag(2,3,4) should be PSD (eigenvalues 2,3,4)"
        );
    }

    #[test]
    fn symmetric_non_diagonal_psd_matrix_is_psd() {
        // [[2,1,0],[1,2,0],[0,0,1]] — eigenvalues 1,1,3 (all >= 0) → PSD
        let m = [[2.0, 1.0, 0.0], [1.0, 2.0, 0.0], [0.0, 0.0, 1.0]];
        assert!(
            is_symmetric_psd(&m, psd_tol(&m)),
            "[[2,1,0],[1,2,0],[0,0,1]] has eigenvalues 1,1,3 — should be PSD"
        );
    }

    #[test]
    fn diagonal_with_negative_entry_is_not_psd() {
        // diag(1,1,-1) — min eigenvalue = -1 (far below tol ≈ 1e-9)
        let m = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]];
        assert!(
            !is_symmetric_psd(&m, psd_tol(&m)),
            "diag(1,1,-1) has min eigenvalue -1 — should NOT be PSD"
        );
    }

    #[test]
    fn symmetric_matrix_with_negative_eigenvalue_is_not_psd() {
        // [[0,2,0],[2,0,0],[0,0,1]] — eigenvalues -2,1,2 (min=-2) → NOT PSD
        // Verification: the characteristic polynomial is (1-λ)((−λ)²−4) = 0
        //   → λ=1, λ=±2. Min eigenvalue = -2.
        let m = [[0.0, 2.0, 0.0], [2.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        assert!(
            !is_symmetric_psd(&m, psd_tol(&m)),
            "[[0,2,0],[2,0,0],[0,0,1]] has min eigenvalue -2 — should NOT be PSD"
        );
    }
}
