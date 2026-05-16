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
pub const MIN_JACOBIAN_DET: f64 = 1.0e-30;

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
pub fn inverse_transpose_3x3(m: &[[f64; 3]; 3], det: f64) -> [[f64; 3]; 3] {
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
    use super::{inverse_transpose_3x3, MIN_JACOBIAN_DET};

    /// Pin the exact threshold value — any future accidental drift trips this
    /// test immediately.
    #[test]
    fn min_jacobian_det_constant_value() {
        assert_eq!(MIN_JACOBIAN_DET, 1.0e-30);
    }

    /// Identity matrix: `I⁻ᵀ = I`, exact bit-for-bit (no rounding because all
    /// minors are 0 or 1 and `det = 1`).
    #[test]
    fn inverse_transpose_3x3_identity() {
        let id: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let result = inverse_transpose_3x3(&id, 1.0);
        assert_eq!(result, id);
    }

    /// Diagonal matrix `diag(2, 3, 4)` with `det = 24`.
    ///
    /// `M⁻¹ = diag(1/2, 1/3, 1/4)`.  Because the matrix is symmetric,
    /// `M⁻ᵀ = M⁻¹ = diag(0.5, 1/3, 0.25)`.  Check each entry to within
    /// an absolute tolerance of `1e-12`.
    #[test]
    fn inverse_transpose_3x3_known_3x3() {
        let m: [[f64; 3]; 3] = [[2.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 4.0]];
        let det = 24.0_f64;
        let result = inverse_transpose_3x3(&m, det);
        let expected: [[f64; 3]; 3] =
            [[0.5, 0.0, 0.0], [0.0, 1.0 / 3.0, 0.0], [0.0, 0.0, 0.25]];
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
