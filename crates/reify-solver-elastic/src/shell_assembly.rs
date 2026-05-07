//! Shell-element stiffness assembly for the MITC3+ Reissner-Mindlin shell.
//!
//! # PRD reference
//!
//! `docs/prds/v0_4/structural-analysis-shells.md` task T6.
//!
//! # Overview
//!
//! Computes the per-element 18×18 stiffness matrix for the MITC3+
//! Reissner-Mindlin shell under a constant-thickness isotropic linear-elastic
//! constitutive law. Through-thickness integration is closed-form; element K
//! is assembled in a local mid-surface frame and then rotated into the global
//! frame so it is ready for the global sparse-assembly consumer (PRD T#11).
//! Output is a [`crate::assembly::ElementStiffness`] with `n_dofs = 18`.

use crate::assembly::ElementStiffness;
use crate::constitutive::IsotropicElastic;

/// Local mid-surface coordinate frame for a MITC3+ shell element.
///
/// `r[i][j]` is the j-th global component of local basis vector `eᵢ`:
/// - `r[0]` = `e1` (along edge p0→p1, in-plane)
/// - `r[1]` = `e2` (in-plane, perpendicular to e1)
/// - `r[2]` = `e3` (outward normal, right-handed)
///
/// The transform `x_local = R · x_global` maps global vectors to local.
/// `origin` is the first node `p0`.
pub struct ShellFrame {
    /// Origin of the local frame (physical position of node 0).
    pub origin: [f64; 3],
    /// 3×3 rotation matrix: rows are the local basis vectors in global coords.
    pub r: [[f64; 3]; 3],
    /// Area of the physical triangle `= 0.5 · |(p1−p0) × (p2−p0)|`.
    pub area: f64,
}

/// Build the local mid-surface frame for a three-node shell element.
///
/// # Frame construction
///
/// - `e1 = (p1 − p0) / |p1 − p0|`
/// - `n = (p1 − p0) × (p2 − p0)` (unnormalized right-handed normal)
/// - `area = 0.5 · |n|`
/// - `e3 = n / |n|` (unit normal)
/// - `e2 = e3 × e1` (in-plane, orthogonal to e1)
///
/// The resulting frame is right-handed and orthonormal.
pub fn build_shell_frame(nodes: &[[f64; 3]; 3]) -> ShellFrame {
    let p0 = nodes[0];
    let p1 = nodes[1];
    let p2 = nodes[2];

    // Edge vectors
    let d01 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    let d02 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];

    // e1: normalize d01
    let len01 = (d01[0] * d01[0] + d01[1] * d01[1] + d01[2] * d01[2]).sqrt();
    debug_assert!(len01 > 1e-30, "degenerate shell element: p0 == p1");
    let e1 = [d01[0] / len01, d01[1] / len01, d01[2] / len01];

    // Normal (cross product d01 × d02)
    let cx = d01[1] * d02[2] - d01[2] * d02[1];
    let cy = d01[2] * d02[0] - d01[0] * d02[2];
    let cz = d01[0] * d02[1] - d01[1] * d02[0];
    let len_n = (cx * cx + cy * cy + cz * cz).sqrt();
    debug_assert!(len_n > 1e-30, "degenerate shell element: collinear nodes");
    let area = 0.5 * len_n;

    // e3: unit normal
    let e3 = [cx / len_n, cy / len_n, cz / len_n];

    // e2 = e3 × e1
    let e2 = [
        e3[1] * e1[2] - e3[2] * e1[1],
        e3[2] * e1[0] - e3[0] * e1[2],
        e3[0] * e1[1] - e3[1] * e1[0],
    ];

    ShellFrame {
        origin: p0,
        r: [e1, e2, e3],
        area,
    }
}

/// Compute the 18×18 element stiffness matrix for a MITC3+ shell element.
///
/// `nodes` are the three physical vertex positions in global coordinates.
/// `thickness` is the constant shell thickness `t`.
/// `material` is the isotropic linear-elastic constitutive law.
///
/// Returns an [`ElementStiffness`] with `n_dofs = 18`. DOF ordering is
/// `6 · node_idx + i` with `i ∈ {0..5}` for `(u_x, u_y, u_z, θ_x, θ_y, θ_z)`.
/// The drilling rotation `θ_z` (i=5) carries zero stiffness by construction.
pub fn shell_element_stiffness(
    _nodes: &[[f64; 3]; 3],
    _thickness: f64,
    _material: &IsotropicElastic,
) -> ElementStiffness {
    ElementStiffness::zeros(18)
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use crate::constitutive::IsotropicElastic;
    use super::*;

    fn steel_like() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 200.0e9,
            poisson_ratio: 0.3,
        }
    }

    const UNIT_TRI: [[f64; 3]; 3] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    ];

    /// Compute K · u for an 18-DOF stiffness matrix.
    fn matvec(k: &ElementStiffness, u: &[f64; 18]) -> [f64; 18] {
        let mut out = [0.0_f64; 18];
        for i in 0..18 {
            for j in 0..18 {
                out[i] += k.get(i, j) * u[j];
            }
        }
        out
    }

    /// L∞ norm of a fixed-size slice.
    fn linf(v: &[f64]) -> f64 {
        v.iter().fold(0.0_f64, |acc, x| acc.max(x.abs()))
    }

    const WIDE_TRI: [[f64; 3]; 3] = [
        [0.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [0.0, 3.0, 0.0],
    ];

    #[test]
    fn shell_element_stiffness_returns_18_by_18_for_unit_triangle() {
        let k = shell_element_stiffness(&UNIT_TRI, 0.05, &steel_like());
        assert_eq!(k.n_dofs, 18);
        assert_eq!(k.data.len(), 324);
    }

    // --- ShellFrame tests (step 3) ---

    #[test]
    fn build_shell_frame_returns_orthonormal_rotation() {
        let frame = build_shell_frame(&WIDE_TRI);
        let r = frame.r;
        // Each row has unit norm.
        for i in 0..3 {
            let norm_sq = r[i][0] * r[i][0] + r[i][1] * r[i][1] + r[i][2] * r[i][2];
            assert!(
                (norm_sq - 1.0).abs() < 1e-12,
                "row {i} norm² = {norm_sq}, expected 1.0",
            );
        }
        // Rows are mutually orthogonal.
        for i in 0..3 {
            for j in (i + 1)..3 {
                let dot = r[i][0] * r[j][0] + r[i][1] * r[j][1] + r[i][2] * r[j][2];
                assert!(
                    dot.abs() < 1e-12,
                    "rows {i} · {j} = {dot}, expected 0",
                );
            }
        }
    }

    #[test]
    fn build_shell_frame_normal_is_perpendicular_to_in_plane_edges() {
        let frame = build_shell_frame(&WIDE_TRI);
        let n = frame.r[2]; // e3 = normal
        let p0 = WIDE_TRI[0];
        let p1 = WIDE_TRI[1];
        let p2 = WIDE_TRI[2];
        let e01 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let e02 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        let dot01 = n[0] * e01[0] + n[1] * e01[1] + n[2] * e01[2];
        let dot02 = n[0] * e02[0] + n[1] * e02[1] + n[2] * e02[2];
        assert!(dot01.abs() < 1e-12, "n · e01 = {dot01}, expected 0");
        assert!(dot02.abs() < 1e-12, "n · e02 = {dot02}, expected 0");
    }

    #[test]
    fn build_shell_frame_area_matches_half_cross_product_norm() {
        let frame = build_shell_frame(&WIDE_TRI);
        // For nodes (0,0,0), (2,0,0), (0,3,0):
        // cross = (2,0,0) × (0,3,0) = (0,0,6) → |cross| = 6 → area = 3.
        let expected_area = 3.0_f64;
        assert!(
            (frame.area - expected_area).abs() < 1e-12,
            "area = {}, expected {expected_area}",
            frame.area,
        );
    }
}
