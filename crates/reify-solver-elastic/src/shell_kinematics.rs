//! Shared MITC3 shell kinematics shared by `shell_assembly` and `shell_result`.
//!
//! Both `shell_element_stiffness` and `shell_element_stress` need the same
//! local 2D nodal coordinates, constant 2D shape gradients, covariant shear
//! B-matrix at the three MITC3 tying points, and the J2⁻ᵀ covariant→physical
//! map. This module is the single source of truth for those four quantities
//! (plus the J2 determinant `det2` used by the shear quadrature weight in the
//! stiffness path), so a sign-convention or tying-point-ordering change made
//! here propagates to both consumers atomically.
//!
//! # Implementation note
//!
//! `ShellKinematics` struct and `shell_kinematics` function are defined here.
//! See the `#[cfg(test)]` module below for the contract pins.

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
