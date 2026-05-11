//! Shared MITC3 shell kinematics shared by `shell_assembly` and `shell_result`.
//!
//! Both `shell_element_stiffness` and `shell_element_stress` need the same
//! local 2D nodal coordinates, constant 2D shape gradients, covariant shear
//! B-matrix at the three MITC3 tying points, and the J2⁻ᵀ covariant→physical
//! map. This module is the single source of truth for those four quantities
//! (plus the J2 determinant `det2` used by the shear quadrature weight in the
//! stiffness path), so a sign-convention or tying-point-ordering change made
//! here propagates to both consumers atomically.

use crate::shell_assembly::ShellFrame;

/// Per-element shell kinematics derived from physical nodes + the local
/// mid-surface frame.
pub struct ShellKinematics {
    /// Local 2D coords: `xloc[i] = (R · (p_i − p0)).xy` (e3 component is 0
    /// for a flat triangle by construction of the frame).
    pub local_nodes_2d: [[f64; 2]; 3],
    /// Constant 2D shape gradients in the local frame: `dn[i] = [dN_i/dx, dN_i/dy]`.
    pub dn: [[f64; 2]; 3],
    /// Covariant shear B-matrix at each MITC3 tying point A, B, C:
    /// `b_cov_at_tying_points[tp_idx][cov_component][dof]` for
    /// `tp_idx in {A=0, B=1, C=2}` and `cov_component in {γ_ξζ=0, γ_ηζ=1}`.
    pub b_cov_at_tying_points: [[[f64; 18]; 2]; 3],
    /// J2⁻ᵀ — maps covariant (ξ,η) shear components to physical (x,y).
    pub jac2_inv_t: [[f64; 2]; 2],
    /// `det(J2)` — local 2D Jacobian determinant. Always `> 0` for a
    /// well-posed element; equals `2 · area`. Used by `shell_element_stiffness`
    /// as the shear-quadrature weight factor.
    pub det2: f64,
}

/// Compute shared MITC3 shell kinematics from physical node positions and the
/// pre-built local mid-surface frame.
///
/// Both `shell_element_stiffness` and `shell_element_stress` call this after
/// `build_shell_frame` and read the fields directly — no behaviour change,
/// identical floating-point output.
///
/// # Arguments
///
/// - `nodes` — three physical vertex positions in global coordinates.
/// - `frame` — pre-built local mid-surface frame (from `build_shell_frame`).
pub fn shell_kinematics(nodes: &[[f64; 3]; 3], frame: &ShellFrame) -> ShellKinematics {
    use crate::elements::mitc3_plus::Mitc3Plus;

    const NDOF: usize = Mitc3Plus::N_DOFS; // 18 total DOFs
    const NDP: usize = Mitc3Plus::N_DOFS_PER_NODE; // 6 DOFs per node
    const NN: usize = Mitc3Plus::N_NODES; // 3 nodes

    let r = frame.r;
    let area = frame.area;

    // --- Local 2D coordinates of nodes: xloc[i] = (R · (p_i − p0)).xy ---
    let mut local_nodes_2d = [[0.0_f64; 2]; 3];
    for i in 0..NN {
        let d = [
            nodes[i][0] - frame.origin[0],
            nodes[i][1] - frame.origin[1],
            nodes[i][2] - frame.origin[2],
        ];
        local_nodes_2d[i][0] = r[0][0] * d[0] + r[0][1] * d[1] + r[0][2] * d[2];
        local_nodes_2d[i][1] = r[1][0] * d[0] + r[1][1] * d[1] + r[1][2] * d[2];
    }

    // --- 2D shape gradients via the standard triangle formula ---
    // For nodes (x0,y0),(x1,y1),(x2,y2) with signed area A = area (positive):
    //   ∂N_i/∂x = (y_j − y_k) / (2·A)
    //   ∂N_i/∂y = (x_k − x_j) / (2·A)
    // cyclic: i→j→k = 0→1→2→0
    let two_a = 2.0 * area;
    let x = [local_nodes_2d[0][0], local_nodes_2d[1][0], local_nodes_2d[2][0]];
    let y = [local_nodes_2d[0][1], local_nodes_2d[1][1], local_nodes_2d[2][1]];
    let dn = [
        [(y[1] - y[2]) / two_a, (x[2] - x[1]) / two_a],
        [(y[2] - y[0]) / two_a, (x[0] - x[2]) / two_a],
        [(y[0] - y[1]) / two_a, (x[1] - x[0]) / two_a],
    ];

    // --- Local 2D Jacobian J2, determinant, and J2⁻ᵀ ---
    // J2 = [[∂x/∂ξ, ∂x/∂η], [∂y/∂ξ, ∂y/∂η]] = [[x1-x0, x2-x0], [y1-y0, y2-y0]]
    let jac2 = [[x[1] - x[0], x[2] - x[0]], [y[1] - y[0], y[2] - y[0]]];
    let det2 = jac2[0][0] * jac2[1][1] - jac2[0][1] * jac2[1][0];
    // J2⁻ᵀ: (J2⁻¹)ᵀ — maps covariant (ξ,η) components to physical (x,y)
    // J2⁻¹ = (1/det) · [[jac2[1][1], -jac2[0][1]], [-jac2[1][0], jac2[0][0]]]
    // J2⁻ᵀ[i][j] = J2⁻¹[j][i]
    let jac2_inv_t = [
        [jac2[1][1] / det2, -jac2[1][0] / det2],
        [-jac2[0][1] / det2, jac2[0][0] / det2],
    ];

    // --- Covariant shear B-matrix at each MITC3 tying point ---
    // b_cov_at_tying_points[tp_idx][cov_component][dof]
    // For each tying point (ξ_t, η_t):
    //   γ_ξζ = Σ_i dn_ref[i][0] * u_z_i + N_i(tp) * θ_y_i
    //   γ_ηζ = Σ_i dn_ref[i][1] * u_z_i - N_i(tp) * θ_x_i
    let tying_pts = Mitc3Plus.tying_points();
    let mut b_cov_at_tying_points = [[[0.0_f64; NDOF]; 2]; 3];
    for (tp_idx, tp) in tying_pts.iter().enumerate() {
        let n_at_tp = Mitc3Plus.shape_at(tp.coord);
        // shape_grad_at returns constant ∇N_0=(−1,−1), ∇N_1=(1,0), ∇N_2=(0,1)
        let dn_ref_tp = Mitc3Plus.shape_grad_at(tp.coord);
        for node in 0..NN {
            let dof_uz = NDP * node + 2;
            let dof_tx = NDP * node + 3;
            let dof_ty = NDP * node + 4;
            // γ_ξζ contribution: dn_ref[node][0]*u_z + N*θ_y
            b_cov_at_tying_points[tp_idx][0][dof_uz] += dn_ref_tp[node][0];
            b_cov_at_tying_points[tp_idx][0][dof_ty] += n_at_tp[node];
            // γ_ηζ contribution: dn_ref[node][1]*u_z - N*θ_x
            b_cov_at_tying_points[tp_idx][1][dof_uz] += dn_ref_tp[node][1];
            b_cov_at_tying_points[tp_idx][1][dof_tx] -= n_at_tp[node];
        }
    }

    ShellKinematics {
        local_nodes_2d,
        dn,
        b_cov_at_tying_points,
        jac2_inv_t,
        det2,
    }
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;
    use crate::elements::mitc3_plus::Mitc3Plus;
    use crate::shell_assembly::build_shell_frame;

    const UNIT_TRI: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    const WIDE_TRI: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 0.0]];

    /// For UNIT_TRI in the xy-plane, the local frame is the identity rotation,
    /// so local_nodes_2d must equal the xy projections of the physical nodes.
    #[test]
    fn shell_kinematics_unit_tri_local_coords_match_xy_plane() {
        let frame = build_shell_frame(&UNIT_TRI);
        let kin = shell_kinematics(&UNIT_TRI, &frame);
        let expected: [[f64; 2]; 3] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        for i in 0..3 {
            for j in 0..2 {
                assert!(
                    (kin.local_nodes_2d[i][j] - expected[i][j]).abs() < 1e-12,
                    "local_nodes_2d[{i}][{j}] = {}, expected {}",
                    kin.local_nodes_2d[i][j],
                    expected[i][j]
                );
            }
        }
    }

    /// For UNIT_TRI, the canonical P1 shape-function gradients are:
    ///   ∇N_0 = (−1, −1)  [two_a = 1, y[1]-y[2] = -1, x[2]-x[1] = -1]
    ///   ∇N_1 = ( 1,  0)
    ///   ∇N_2 = ( 0,  1)
    #[test]
    fn shell_kinematics_unit_tri_dn_matches_canonical_p1_gradients() {
        let frame = build_shell_frame(&UNIT_TRI);
        let kin = shell_kinematics(&UNIT_TRI, &frame);
        let expected: [[f64; 2]; 3] = [[-1.0, -1.0], [1.0, 0.0], [0.0, 1.0]];
        for i in 0..3 {
            for j in 0..2 {
                assert!(
                    (kin.dn[i][j] - expected[i][j]).abs() < 1e-12,
                    "dn[{i}][{j}] = {}, expected {}",
                    kin.dn[i][j],
                    expected[i][j]
                );
            }
        }
    }

    /// For UNIT_TRI, J2 = I (identity), so J2⁻ᵀ = I and det2 = 1.0.
    #[test]
    fn shell_kinematics_unit_tri_jac2_inv_t_is_identity_and_det2_is_one() {
        let frame = build_shell_frame(&UNIT_TRI);
        let kin = shell_kinematics(&UNIT_TRI, &frame);
        let identity = [[1.0_f64, 0.0], [0.0, 1.0]];
        for i in 0..2 {
            for j in 0..2 {
                assert!(
                    (kin.jac2_inv_t[i][j] - identity[i][j]).abs() < 1e-12,
                    "jac2_inv_t[{i}][{j}] = {}, expected {}",
                    kin.jac2_inv_t[i][j],
                    identity[i][j]
                );
            }
        }
        assert!(
            (kin.det2 - 1.0).abs() < 1e-12,
            "det2 = {}, expected 1.0",
            kin.det2
        );
    }

    /// For WIDE_TRI = [[0,0,0],[2,0,0],[0,3,0]]:
    ///   J2 = [[2,0],[0,3]], det(J2) = 6.0 = 2 × area(=3).
    #[test]
    fn shell_kinematics_wide_tri_det2_equals_two_a() {
        let frame = build_shell_frame(&WIDE_TRI);
        let kin = shell_kinematics(&WIDE_TRI, &frame);
        // area = 0.5 * |cross((2,0,0),(0,3,0))| = 0.5 * 6 = 3.0 → two_a = 6.0
        assert!(
            (kin.det2 - 6.0).abs() < 1e-12,
            "det2 = {}, expected 6.0",
            kin.det2
        );
    }

    /// The covariant shear B-matrix only activates the out-of-plane and rotation
    /// DOFs of each node (6n+2, 6n+3, 6n+4). All other DOF columns are identically
    /// zero. Furthermore:
    ///   - γ_ξζ (comp 0) contributes to u_z (6n+2) and θ_y (6n+4) only
    ///   - γ_ηζ (comp 1) contributes to u_z (6n+2) and θ_x (6n+3) only,
    ///     with the θ_x contribution having a negative sign (−N_i at tying point).
    #[test]
    fn shell_kinematics_b_cov_at_tying_points_only_writes_into_uz_tx_ty_dofs() {
        let frame = build_shell_frame(&UNIT_TRI);
        let kin = shell_kinematics(&UNIT_TRI, &frame);

        for tp in 0..3 {
            let n_at_tp = Mitc3Plus.shape_at(Mitc3Plus.tying_points()[tp].coord);
            for dof in 0..18 {
                let local_dof = dof % 6;
                let node = dof / 6;

                // DOFs outside {2,3,4} per node must be zero in both components
                if local_dof != 2 && local_dof != 3 && local_dof != 4 {
                    assert_eq!(
                        kin.b_cov_at_tying_points[tp][0][dof],
                        0.0,
                        "b_cov[{tp}][xi_zeta][{dof}] (local={local_dof}) must be 0"
                    );
                    assert_eq!(
                        kin.b_cov_at_tying_points[tp][1][dof],
                        0.0,
                        "b_cov[{tp}][eta_zeta][{dof}] (local={local_dof}) must be 0"
                    );
                }

                // γ_ξζ (comp 0): θ_x dofs (6n+3) must be zero
                if local_dof == 3 {
                    assert_eq!(
                        kin.b_cov_at_tying_points[tp][0][dof],
                        0.0,
                        "b_cov[{tp}][xi_zeta][{dof}] (theta_x@node{node}) must be 0"
                    );
                }

                // γ_ηζ (comp 1): θ_y dofs (6n+4) must be zero
                if local_dof == 4 {
                    assert_eq!(
                        kin.b_cov_at_tying_points[tp][1][dof],
                        0.0,
                        "b_cov[{tp}][eta_zeta][{dof}] (theta_y@node{node}) must be 0"
                    );
                }

                // γ_ηζ (comp 1): θ_x dofs (6n+3) must equal −N_i(tp) (negative sign)
                if local_dof == 3 {
                    let expected = -n_at_tp[node];
                    assert!(
                        (kin.b_cov_at_tying_points[tp][1][dof] - expected).abs() < 1e-12,
                        "b_cov[{tp}][eta_zeta][{dof}] (theta_x@node{node}) = {}, expected {}",
                        kin.b_cov_at_tying_points[tp][1][dof],
                        expected
                    );
                }
            }
        }
    }

    /// Tilting UNIT_TRI by any rotation Q must not change local_nodes_2d:
    ///   R_tilted = Qᵀ, so R_tilted · (Q·p_i − Q·p0) = Qᵀ·Q·(p_i−p0) = (p_i−p0).
    /// The rotated frame undoes itself, leaving the 2D coords identical to the
    /// un-tilted case (within floating-point rounding).
    #[test]
    fn shell_kinematics_tilted_triangle_local_coords_collapse_to_un_tilted() {
        let q = crate::shell_assembly::tilted_q_for_shell_tests();

        // Build tilted nodes: Q · UNIT_TRI
        let mut tilted = [[0.0_f64; 3]; 3];
        for (ni, node) in UNIT_TRI.iter().enumerate() {
            for i in 0..3 {
                tilted[ni][i] = q[i][0] * node[0] + q[i][1] * node[1] + q[i][2] * node[2];
            }
        }

        let frame_flat = build_shell_frame(&UNIT_TRI);
        let kin_flat = shell_kinematics(&UNIT_TRI, &frame_flat);

        let frame_tilted = build_shell_frame(&tilted);
        let kin_tilted = shell_kinematics(&tilted, &frame_tilted);

        for i in 0..3 {
            for j in 0..2 {
                assert!(
                    (kin_tilted.local_nodes_2d[i][j] - kin_flat.local_nodes_2d[i][j]).abs()
                        < 1e-12,
                    "local_nodes_2d[{i}][{j}]: tilted={}, flat={}",
                    kin_tilted.local_nodes_2d[i][j],
                    kin_flat.local_nodes_2d[i][j]
                );
            }
        }
    }
}
