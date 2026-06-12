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
use crate::shell_assembly::{build_shell_frame, membrane_node_pair_block, plane_stress_d};
use crate::shell_kinematics::shell_kinematics;

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
#[allow(clippy::needless_range_loop)]
pub fn element_stiffness_membrane_cst(
    nodes: &[[f64; 3]; 3],
    thickness: f64,
    material: &IsotropicElastic,
) -> ElementStiffness {
    // Local mid-surface frame + constant local shape gradients. `build_shell_frame`
    // panics on a degenerate (collinear/zero-edge) triangle — reused as the
    // degeneracy guard.
    let frame = build_shell_frame(nodes);
    let area = frame.area;
    let dn = shell_kinematics(nodes, &frame).dn;

    // Pre-scale the plane-stress constitutive matrix by thickness: t · D_pl.
    let d_pl = plane_stress_d(material);
    let mut t_dpl = [[0.0_f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            t_dpl[i][j] = thickness * d_pl[i][j];
        }
    }

    // Assemble the local 9×9 in-plane membrane block at local DOFs 3i + {0, 1}
    // using the shell's patch-test-validated CST strain-displacement core.
    let mut k_loc = [[0.0_f64; 9]; 9];
    for ni in 0..3 {
        for nj in 0..3 {
            let blk = membrane_node_pair_block(dn[ni], dn[nj], &t_dpl);
            for a in 0..2 {
                for b in 0..2 {
                    k_loc[3 * ni + a][3 * nj + b] += blk[a][b] * area;
                }
            }
        }
    }

    // Rotate the local-frame block into the global frame:
    // K_glob[3a..,3b..] = Rᵀ·K_loc[3a..,3b..]·R over the three 3-DOF nodal blocks.
    // For the xy-plane triangle R = I, so this is a no-op (S1 stays exact).
    let k_glob = rotate_membrane_local_to_global(&k_loc, &frame.r);

    // Symmetrize on the way out — the BᵀDB block is symmetric in form, so the two
    // triangles agree to within floating-point rounding; averaging minimises the
    // residual asymmetry.
    let mut ke = ElementStiffness::zeros(9);
    for i in 0..9 {
        for j in 0..9 {
            ke.data[i * 9 + j] = 0.5 * (k_glob[i][j] + k_glob[j][i]);
        }
    }
    ke
}

/// Rotate a local-frame 9×9 membrane element matrix into the global frame:
/// `K_glob[3a..3a+3, 3b..3b+3] = Rᵀ · K_loc[3a.., 3b..] · R` over the 3×3 grid of
/// 3-DOF nodal blocks (a `blockdiag(R)` congruence).
///
/// `r` is `frame.r` — rows are the local basis vectors in global coordinates (the
/// global→local rotation `R·v_global = v_local`), matching the shell's
/// `rotate_local_to_global` convention. A global force/displacement therefore
/// maps as `K_glob = Tᵀ K_loc T` with `T = blockdiag(R)`, so `u_globᵀ K_glob u_glob
/// = u_locᵀ K_loc u_loc` — the strain-energy invariance the objectivity test pins.
///
/// Kept `pub(crate)` so the membrane K_g kernel
/// ([`crate::geometric_stiffness::membrane`]) reuses the identical rotation.
#[allow(clippy::needless_range_loop)]
pub(crate) fn rotate_membrane_local_to_global(
    k_loc: &[[f64; 9]; 9],
    r: &[[f64; 3]; 3],
) -> [[f64; 9]; 9] {
    // Rᵀ (local-to-global): rt[i][j] = r[j][i].
    let rt = [
        [r[0][0], r[1][0], r[2][0]],
        [r[0][1], r[1][1], r[2][1]],
        [r[0][2], r[1][2], r[2][2]],
    ];
    let mut k_glob = [[0.0_f64; 9]; 9];
    for bi in 0..3 {
        for bj in 0..3 {
            // Extract the 3×3 nodal block K_loc[3bi.., 3bj..].
            let mut sub = [[0.0_f64; 3]; 3];
            for p in 0..3 {
                for q in 0..3 {
                    sub[p][q] = k_loc[3 * bi + p][3 * bj + q];
                }
            }
            // rt_sub = Rᵀ · sub.
            let mut rt_sub = [[0.0_f64; 3]; 3];
            for p in 0..3 {
                for q in 0..3 {
                    let mut s = 0.0;
                    for m in 0..3 {
                        s += rt[p][m] * sub[m][q];
                    }
                    rt_sub[p][q] = s;
                }
            }
            // K_glob block = rt_sub · R.
            for p in 0..3 {
                for q in 0..3 {
                    let mut s = 0.0;
                    for m in 0..3 {
                        s += rt_sub[p][m] * r[m][q];
                    }
                    k_glob[3 * bi + p][3 * bj + q] = s;
                }
            }
        }
    }
    k_glob
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

    // ---------- S3: tilted-frame objectivity + 3D rigid-body null spaces ----------

    /// Apply a 3×3 rotation `q` to a global 3-vector.
    fn apply_q(q: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
        [
            q[0][0] * v[0] + q[0][1] * v[1] + q[0][2] * v[2],
            q[1][0] * v[0] + q[1][1] * v[1] + q[1][2] * v[2],
            q[2][0] * v[0] + q[2][1] * v[1] + q[2][2] * v[2],
        ]
    }

    /// `K · u` for a 9-DOF membrane element matrix.
    fn matvec9(k: &ElementStiffness, u: &[f64; 9]) -> [f64; 9] {
        let mut ku = [0.0_f64; 9];
        for i in 0..9 {
            for j in 0..9 {
                ku[i] += k.get(i, j) * u[j];
            }
        }
        ku
    }

    /// Strain energy `0.5 · uᵀ K u`.
    fn energy(k: &ElementStiffness, u: &[f64; 9]) -> f64 {
        let ku = matvec9(k, u);
        0.5 * (0..9).map(|i| ku[i] * u[i]).sum::<f64>()
    }

    /// L∞ norm of a fixed-size slice.
    fn linf(v: &[f64]) -> f64 {
        v.iter().fold(0.0_f64, |acc, x| acc.max(x.abs()))
    }

    /// Q · UNIT_TRI — a rigidly rotated (tilted) copy of the unit triangle.
    fn tilted_unit_tri(q: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
        [
            apply_q(q, UNIT_TRI[0]),
            apply_q(q, UNIT_TRI[1]),
            apply_q(q, UNIT_TRI[2]),
        ]
    }

    /// Constant-strain (linear) in-plane displacement field, evaluated in the
    /// flat triangle's xy-plane: ε_xx = 0.01, ε_yy = -0.005, γ_xy = 0.005.
    fn const_strain_field(x: f64, y: f64) -> [f64; 3] {
        [0.01 * x + 0.002 * y, 0.003 * x - 0.005 * y, 0.0]
    }

    /// Frame objectivity: strain energy is a rigid-rotation invariant. Rotating
    /// both the geometry (UNIT_TRI → Q·UNIT_TRI) and the displacement field
    /// (d → Q·d) must leave `0.5·uᵀKu` unchanged. This is true ONLY once the
    /// local→global block rotation is applied (S4) — against the S2 local-as-global
    /// packing the tilted energy differs ⇒ RED.
    #[test]
    fn ke_frame_objectivity_tilted_energy_matches_flat() {
        let t = 0.1_f64;
        let mat = nu_zero_material(2.0);

        // Flat reference.
        let k_flat = element_stiffness_membrane_cst(&UNIT_TRI, t, &mat);
        let mut u_flat = [0.0_f64; 9];
        for i in 0..3 {
            let d = const_strain_field(UNIT_TRI[i][0], UNIT_TRI[i][1]);
            for c in 0..3 {
                u_flat[3 * i + c] = d[c];
            }
        }
        let e_flat = energy(&k_flat, &u_flat);
        assert!(e_flat > 1e-12, "reference strain energy must be nonzero, got {e_flat}");

        // Tilted: Q·geometry, Q·displacement.
        let q = crate::shell_assembly::tilted_q_for_shell_tests();
        let tilted = tilted_unit_tri(&q);
        let k_tilted = element_stiffness_membrane_cst(&tilted, t, &mat);
        let mut u_tilted = [0.0_f64; 9];
        for i in 0..3 {
            let d = const_strain_field(UNIT_TRI[i][0], UNIT_TRI[i][1]);
            let dq = apply_q(&q, d);
            for c in 0..3 {
                u_tilted[3 * i + c] = dq[c];
            }
        }
        let e_tilted = energy(&k_tilted, &u_tilted);

        let scale = e_flat.abs().max(1e-30);
        assert!(
            (e_flat - e_tilted).abs() < 1e-9 * scale,
            "frame objectivity: U_flat={e_flat}, U_tilted={e_tilted}, rel_err={}",
            (e_flat - e_tilted).abs() / scale,
        );
    }

    /// 3D rigid-body translation null space: a uniform per-node translation along
    /// any global axis produces zero strain ⇒ `‖K_e·u‖_∞ < 1e-9·max|K_e|`. Holds
    /// for both the flat and the tilted triangle.
    #[test]
    fn ke_translation_in_null_space() {
        let t = 0.1_f64;
        let mat = nu_zero_material(2.0);
        let q = crate::shell_assembly::tilted_q_for_shell_tests();
        let tilted = tilted_unit_tri(&q);

        for nodes in [&UNIT_TRI, &tilted] {
            let k = element_stiffness_membrane_cst(nodes, t, &mat);
            let max_abs = linf(&k.data).max(1.0);
            for axis in 0..3 {
                let mut u = [0.0_f64; 9];
                for node in 0..3 {
                    u[3 * node + axis] = 1.0;
                }
                let ku = matvec9(&k, &u);
                let resid = linf(&ku);
                assert!(
                    resid < 1e-9 * max_abs,
                    "translation axis {axis}: ‖K_e·u‖_∞ = {resid} (max|K_e|={max_abs})",
                );
            }
        }
    }
}
