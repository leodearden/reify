/// PSD (positive semi-definite) validation helpers for `MassProperties.inertia`.
///
/// The inertia tensor of a rigid body must be positive semi-definite (all
/// eigenvalues ≥ 0) by physics. This module provides a dependency-free
/// analytic check on the symmetric part of a 3×3 matrix.
///
/// # Design
///
/// `is_symmetric_psd` computes the three eigenvalues of the symmetric part
/// `S = (M + Mᵀ)/2` via the closed-form Smith/trigonometric method for
/// symmetric 3×3 matrices (NaN-free for the diagonal/degenerate cases the
/// tests exercise — diagonal eigenvalues equal the diagonal entries exactly).
/// PSD is declared when all eigenvalues ≥ −tol.
///
/// `inertia_3x3_from_value` extracts a `[[f64;3];3]` from either a
/// `Value::Matrix` (3 rows × 3 cols) or a nested `Value::List` (3 elements,
/// each a 3-element `Value::List`). Scalar entries are extracted via their
/// `si_value` field.
///
/// # References
///
/// Smith, O.K. (1961). "Eigenvalues of a symmetric 3×3 matrix."
/// *Communications of the ACM*, 4(4), 168.

use reify_ir::Value;

/// Extract a `[[f64; 3]; 3]` from `v`, which may be either
/// `Value::Matrix(rows)` (3 rows × 3 values each) or a nested
/// `Value::List` (3 elements, each a 3-element list).
///
/// Returns `None` when:
/// - the outer container has a length other than 3,
/// - any row has a length other than 3, or
/// - any cell is not numeric (`Value::Real` or `Value::Scalar { si_value }`).
pub fn inertia_3x3_from_value(v: &Value) -> Option<[[f64; 3]; 3]> {
    // Helper: extract f64 from a single cell.
    // Handles Value::Int (whole-number literals like `0`, `1`, `-1`),
    // Value::Real (floating-point literals like `0.0`, `1.5`), and
    // Value::Scalar (dimensioned quantities, using their si_value).
    fn cell_f64(cell: &Value) -> Option<f64> {
        match cell {
            Value::Int(n) => Some(*n as f64),
            Value::Real(r) => Some(*r),
            Value::Scalar { si_value, .. } => Some(*si_value),
            _ => None,
        }
    }

    // Helper: extract a row of 3 f64 from a slice of Values.
    fn row_3(vals: &[Value]) -> Option<[f64; 3]> {
        if vals.len() != 3 {
            return None;
        }
        let a = cell_f64(&vals[0])?;
        let b = cell_f64(&vals[1])?;
        let c = cell_f64(&vals[2])?;
        Some([a, b, c])
    }

    match v {
        Value::Matrix(rows) => {
            if rows.len() != 3 {
                return None;
            }
            let r0 = row_3(&rows[0])?;
            let r1 = row_3(&rows[1])?;
            let r2 = row_3(&rows[2])?;
            Some([r0, r1, r2])
        }
        Value::List(outer) => {
            if outer.len() != 3 {
                return None;
            }
            let r0 = match &outer[0] {
                Value::List(row) => row_3(row)?,
                _ => return None,
            };
            let r1 = match &outer[1] {
                Value::List(row) => row_3(row)?,
                _ => return None,
            };
            let r2 = match &outer[2] {
                Value::List(row) => row_3(row)?,
                _ => return None,
            };
            Some([r0, r1, r2])
        }
        _ => None,
    }
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
        let min = a.min(b).min(c);
        return min >= -tol;
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

    let min_eig = eig0.min(eig1).min(eig2);
    min_eig >= -tol
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
    use reify_core::DimensionVector;

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

    // ── inertia_3x3_from_value ────────────────────────────────────────────────

    #[test]
    fn extracts_from_value_matrix() {
        let rows = vec![
            vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(2.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(0.0), Value::Real(3.0)],
        ];
        let v = Value::Matrix(rows);
        let m = inertia_3x3_from_value(&v).expect("should extract from Value::Matrix");
        assert_eq!(m, [[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 3.0]]);
    }

    #[test]
    fn extracts_from_nested_list() {
        let row = |a: f64, b: f64, c: f64| {
            Value::List(vec![Value::Real(a), Value::Real(b), Value::Real(c)])
        };
        let v = Value::List(vec![row(1.0, 0.0, 0.0), row(0.0, 2.0, 0.0), row(0.0, 0.0, 3.0)]);
        let m = inertia_3x3_from_value(&v).expect("should extract from nested Value::List");
        assert_eq!(m, [[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 3.0]]);
    }

    #[test]
    fn extracts_scalar_si_value() {
        // A cell that is Value::Scalar{si_value} (e.g. kg·m²) should also be extracted.
        let scalar_cell = Value::Scalar {
            si_value: 5.0,
            dimension: DimensionVector::MASS, // arbitrary dimensioned value
        };
        let rows = vec![
            vec![scalar_cell, Value::Real(0.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)],
        ];
        let v = Value::Matrix(rows);
        let m = inertia_3x3_from_value(&v).expect("should extract Scalar si_value");
        assert!((m[0][0] - 5.0).abs() < 1e-12, "expected si_value 5.0, got {}", m[0][0]);
    }

    #[test]
    fn returns_none_for_wrong_shape() {
        // A 2×3 matrix
        let rows = vec![
            vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)],
        ];
        let v = Value::Matrix(rows);
        assert!(
            inertia_3x3_from_value(&v).is_none(),
            "2×3 matrix should return None"
        );
    }

    #[test]
    fn returns_none_for_non_numeric_cell() {
        let rows = vec![
            vec![Value::Bool(true), Value::Real(0.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)],
        ];
        let v = Value::Matrix(rows);
        assert!(
            inertia_3x3_from_value(&v).is_none(),
            "matrix with non-numeric cell (Value::Bool) should return None"
        );
    }
}
