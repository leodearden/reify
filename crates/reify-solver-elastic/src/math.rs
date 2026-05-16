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
