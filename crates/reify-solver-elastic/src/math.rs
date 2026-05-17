//! Shared low-level math primitives for the elastic-solver kernel.
//!
//! This module is the single authoritative source for math helpers that are
//! used by three or more modules within `reify-solver-elastic`. Items live
//! here instead of in their first consumer to avoid duplicate definitions
//! that diverge silently under future edits.
//!
//! # Current exports
//!
//! - [`MIN_JACOBIAN_DET`] — conservative lower bound on `|det J|` used by the
//!   degenerate-element guard in assembly, stress-recovery, and interpolation.
//! - [`inverse_transpose_3x3`] — `M⁻ᵀ` via the standard 3×3 cofactor /
//!   adjugate formula; used wherever physical-frame gradients must be pushed
//!   from reference coordinates (`∇_x N_i = J⁻ᵀ ∇_ξ N_i`).
//!
//! ## History
//!
//! Both items were originally defined locally in `assembly/tet.rs` and then
//! duplicated into `result.rs` and `interpolation.rs` with comments noting
//! "extract when a third consumer appears". Task 3719 executed that extraction
//! once the third consumer (interpolation) was confirmed; PRD task #21
//! (diagnostics) will supersede `MIN_JACOBIAN_DET` with a mesh-scale-aware
//! degeneracy detector.

/// Conservative lower bound on `|det J|` for the debug-mode
/// degenerate-element guard used across assembly, stress recovery, and
/// interpolation.
///
/// Anything at or below this threshold is treated as a malformed element
/// and trips a `debug_assert!` rather than silently dividing by it (which
/// would propagate `±∞` / `NaN` through the inverse Jacobian). `1e-30` is
/// far below any plausible real-world element volume even in micrometre
/// meshes, so the check should never false-positive on valid inputs. PRD
/// task #21 (diagnostics) will replace this placeholder with a proper
/// mesh-scale-aware degeneracy detector.
pub(crate) const MIN_JACOBIAN_DET: f64 = 1.0e-30;

/// Return `(M⁻¹)ᵀ = M⁻ᵀ` for a 3×3 matrix via the standard cofactor /
/// adjugate formula.
///
/// `det` is the determinant of `m`, taken from the caller (already computed
/// alongside the forward Jacobian rather than recomputed here). The
/// relationship used is:
///
/// ```text
/// M⁻¹ = adj(M) / det M,   adj(M)[i][j] = cofactor(M)[j][i]
/// ```
///
/// so `(M⁻ᵀ)[i][j] = (M⁻¹)[j][i] = cofactor(M)[i][j] / det M`. Each
/// cofactor is `(-1)^(i+j)` times the 2×2 minor obtained by deleting row
/// `i` and column `j`.
///
/// # Preconditions
///
/// `det != 0`. For a degenerate / inverted element with `det == 0` the
/// result is non-finite (division by zero); diagnosing that condition is
/// PRD task #21's job. Callers should check with `MIN_JACOBIAN_DET` before
/// calling this function.
#[allow(clippy::needless_range_loop)]
pub(crate) fn inverse_transpose_3x3(m: &[[f64; 3]; 3], det: f64) -> [[f64; 3]; 3] {
    let mut inv_t = [[0.0_f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            // Indices of the two rows / columns that survive after
            // deleting row i / column j.
            let r0 = if i == 0 { 1 } else { 0 };
            let r1 = if i == 2 { 1 } else { 2 };
            let c0 = if j == 0 { 1 } else { 0 };
            let c1 = if j == 2 { 1 } else { 2 };
            let minor = m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
            let sign = if (i + j) % 2 == 0 { 1.0 } else { -1.0 };
            inv_t[i][j] = sign * minor / det;
        }
    }
    inv_t
}

#[cfg(test)]
mod tests {
    use super::inverse_transpose_3x3;

    /// Identity matrix: `I⁻ᵀ = I`, exact bit-for-bit (no rounding because all
    /// minors are 0 or 1 and `det = 1`).
    #[test]
    fn inverse_transpose_3x3_identity() {
        let id: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let result = inverse_transpose_3x3(&id, 1.0);
        assert_eq!(result, id);
    }

    /// Non-symmetric shear-coupled matrix: `M = [[1,2,3],[0,1,4],[5,6,0]]`,
    /// `det(M) = 1`.
    ///
    /// Because M is non-symmetric, `M⁻ᵀ ≠ M⁻¹`, so this test exercises the
    /// *transpose* direction of the function — a regression that returned
    /// `M⁻¹` instead of `M⁻ᵀ` (or any sign-flipped cofactor pattern that
    /// accidentally cancels on symmetric input) would produce wrong off-diagonal
    /// entries here and fail.
    ///
    /// Hand derivation (det = 1, so `M⁻ᵀ = cofactor(M)`):
    ///
    /// ```text
    /// cofactor(M)[0][0] = +(1·0 − 4·6) = −24
    /// cofactor(M)[0][1] = −(0·0 − 4·5) =  20
    /// cofactor(M)[0][2] = +(0·6 − 1·5) =  −5
    /// cofactor(M)[1][0] = −(2·0 − 3·6) =  18
    /// cofactor(M)[1][1] = +(1·0 − 3·5) = −15
    /// cofactor(M)[1][2] = −(1·6 − 2·5) =   4
    /// cofactor(M)[2][0] = +(2·4 − 3·1) =   5
    /// cofactor(M)[2][1] = −(1·4 − 3·0) =  −4
    /// cofactor(M)[2][2] = +(1·1 − 2·0) =   1
    /// ```
    ///
    /// Verified: `M · M⁻¹ = I` where `M⁻¹ = (M⁻ᵀ)ᵀ`.
    #[test]
    fn inverse_transpose_3x3_nonsymmetric() {
        let m: [[f64; 3]; 3] = [[1.0, 2.0, 3.0], [0.0, 1.0, 4.0], [5.0, 6.0, 0.0]];
        let det = 1.0_f64;
        let result = inverse_transpose_3x3(&m, det);
        #[rustfmt::skip]
        let expected: [[f64; 3]; 3] = [
            [-24.0,  20.0, -5.0],
            [ 18.0, -15.0,  4.0],
            [  5.0,  -4.0,  1.0],
        ];
        for i in 0..3 {
            for j in 0..3 {
                let diff = (result[i][j] - expected[i][j]).abs();
                assert!(
                    diff < 1e-12,
                    "result[{i}][{j}] = {} but expected {}; diff = {diff}",
                    result[i][j],
                    expected[i][j],
                );
            }
        }
    }
}
