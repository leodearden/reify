//! Constant-strain-triangle (CST) **membrane** element for `reify-solver-elastic`.
//!
//! A dedicated 3-DOF/node (translation-only) flat membrane triangle — the 2-D
//! surface-element analogue of the pin-jointed bar (`assembly/bar.rs` `K_e` +
//! `geometric_stiffness/bar.rs` `K_g`). See PRD
//! `docs/prds/v0_6/tensegrity-membrane.md` §5 + D2, task ζ.
//!
//! The elastic stiffness `K_e` reuses the shell's patch-test-validated CST
//! membrane block (`shell_assembly::membrane_node_pair_block`), so the membrane
//! `K_e` is the *same* validated `Bₘᵀ(t·D_pl)Bₘ` triple-product as the MITC3
//! shell — a structural guarantee, not two copies kept in lockstep. The element
//! is assembled in the local mid-surface frame (`build_shell_frame` +
//! `shell_kinematics`) then block-rotated to global by `blockdiag(R)` over the
//! three three-DOF nodal blocks. A flat membrane has no rotational DOFs, so the
//! shell's drilling/SPD-suppression machinery does not apply.
//!
//! The companion geometric-stiffness kernel `K_g` and the per-element tangent
//! `K_t = K_e + K_g` live in [`crate::geometric_stiffness::membrane`].

use crate::assembly::ElementStiffness;
use crate::constitutive::IsotropicElastic;

/// Compute the 9×9 elastic stiffness `K_e` for a flat 3-node CST membrane
/// element (3 translational DOF/node, DOF layout `3·node + axis`).
///
/// `nodes` are the three physical vertex positions in global coordinates.
/// `thickness` is the constant membrane thickness `t`. `material` is the
/// isotropic linear-elastic constitutive law (plane stress).
///
/// Returns an [`ElementStiffness`] with `n_dofs = 9`, row-major, assemblable
/// through the unchanged [`crate::assemble_global_stiffness`] scatter
/// (`dofs_per_node = 9 / 3 = 3`).
pub fn element_stiffness_membrane_cst(
    nodes: &[[f64; 3]; 3],
    thickness: f64,
    material: &IsotropicElastic,
) -> ElementStiffness {
    todo!("element_stiffness_membrane_cst: implemented in S2/S4")
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;
    use crate::assembly::test_support::assert_close;

    /// Unit triangle in the xy-plane: R = I, area A = 0.5,
    /// dn = [(-1,-1), (1,0), (0,1)].
    const UNIT_TRI: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

    /// Collinear (degenerate) triangle — `build_shell_frame` rejects it.
    const COLLINEAR_TRI: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];

    /// ν = 0 ⇒ closed-form plane-stress D_pl = diag(E, E, E/2), so every
    /// membrane block entry has a hand-derivable value.
    fn nu_zero_material(e: f64) -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: e,
            poisson_ratio: 0.0,
        }
    }

    // (a) shape: n_dofs == 9 and data.len() == 81.
    #[test]
    fn ke_returns_9x9() {
        let k = element_stiffness_membrane_cst(&UNIT_TRI, 0.1, &nu_zero_material(2.0));
        assert_eq!(k.n_dofs, 9);
        assert_eq!(k.data.len(), 81);
    }

    // (b) representative in-plane entries vs hand values (in E·t·A).
    //
    // With ν = 0 ⇒ t·D_pl = diag(tE, tE, tE/2), A = 0.5, dn as above:
    //   K_e[3i+a][3j+b] = A · Σ_r bmi[r][a]·(t_dpl)_rr·bmj[r][b]
    // where the two B_m columns are bmi[·][0] = [dn_ix, 0, dn_iy] and
    // bmi[·][1] = [0, dn_iy, dn_ix]. Setting te = t·E:
    //   K_e[0][0] = 0.75·te   (u_x0·u_x0)   K_e[1][1] = 0.75·te (u_y0·u_y0)
    //   K_e[0][1] = 0.25·te   (u_x0·u_y0)   K_e[3][3] = 0.50·te (u_x1·u_x1)
    //   K_e[0][3] = -0.50·te  (u_x0·u_x1)   K_e[0][6] = -0.25·te (u_x0·u_x2)
    #[test]
    fn ke_unit_triangle_hand_values() {
        let e = 2.0_f64;
        let t = 0.1_f64;
        let te = t * e;
        let k = element_stiffness_membrane_cst(&UNIT_TRI, t, &nu_zero_material(e));

        assert_close(k.get(0, 0), 0.75 * te, 1e-12, "K_e[0][0] = 0.75·t·E");
        assert_close(k.get(1, 1), 0.75 * te, 1e-12, "K_e[1][1] = 0.75·t·E");
        assert_close(k.get(0, 1), 0.25 * te, 1e-12, "K_e[0][1] = 0.25·t·E");
        assert_close(k.get(3, 3), 0.50 * te, 1e-12, "K_e[3][3] = 0.50·t·E");
        assert_close(k.get(0, 3), -0.50 * te, 1e-12, "K_e[0][3] = -0.50·t·E (cross-node)");
        assert_close(k.get(0, 6), -0.25 * te, 1e-12, "K_e[0][6] = -0.25·t·E (cross-node)");
    }

    // (c) flat membrane K_e has no transverse stiffness: every local-z
    // (transverse) DOF row/col {2, 5, 8} is exactly 0. For the xy-plane
    // triangle local-z == global-z.
    #[test]
    fn ke_transverse_dofs_are_exactly_zero() {
        let k = element_stiffness_membrane_cst(&UNIT_TRI, 0.1, &nu_zero_material(2.0));
        for &z in &[2usize, 5, 8] {
            for j in 0..9 {
                assert_eq!(k.get(z, j), 0.0, "transverse row K_e[{z}][{j}] must be 0");
                assert_eq!(k.get(j, z), 0.0, "transverse col K_e[{j}][{z}] must be 0");
            }
        }
    }

    // (d) symmetry K_e[i][j] == K_e[j][i] to 1e-12.
    #[test]
    fn ke_is_symmetric() {
        let k = element_stiffness_membrane_cst(&UNIT_TRI, 0.1, &nu_zero_material(2.0));
        for i in 0..9 {
            for j in 0..9 {
                assert_close(k.get(i, j), k.get(j, i), 1e-12, &format!("sym({i},{j})"));
            }
        }
    }

    // Degeneracy guard: a collinear triangle panics via build_shell_frame.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "collinear")]
    fn ke_collinear_triangle_panics() {
        let _ = element_stiffness_membrane_cst(&COLLINEAR_TRI, 0.1, &nu_zero_material(2.0));
    }
}
