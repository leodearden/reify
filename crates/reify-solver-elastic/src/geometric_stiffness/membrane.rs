//! CST **membrane** geometric stiffness `K_g`, in-plane prestress resultant, and
//! per-element tangent `K_t = K_e + K_g` for `reify-solver-elastic`.
//!
//! The 2-D surface-element analogue of the bar `K_g`
//! (`geometric_stiffness/bar.rs`). Under an in-plane membrane prestress
//! resultant `S` (2×2, force/length, element local frame) the geometric
//! stiffness stiffens **only** the transverse/local-normal DOF:
//!
//! ```text
//! K_g_loc[3i+2][3j+2] = A · (dn_i · S · dn_j)
//! ```
//!
//! while the in-plane DOFs carry zero geometric stiffness — the 2-D analogue of
//! the bar's axial null. For an isotropic resultant `N = σ·t` this reduces to
//! `N·A·(∇N·∇N)`, the σt-scaled cotangent-Laplacian (consistent with the §4
//! NFDM reduction). See PRD `docs/prds/v0_6/tensegrity-membrane.md` §5 + D1/D2,
//! task ζ.
//!
//! The elastic stiffness `K_e` lives in
//! [`crate::elements::membrane_cst::element_stiffness_membrane_cst`].

use crate::assembly::ElementStiffness;
use crate::constitutive::IsotropicElastic;

/// In-plane membrane stress **resultant** (force/length) in the element local
/// frame, carried as a 2×2 tensor `S` indexed `resultant[i][j]`.
///
/// Mirrors [`crate::geometric_stiffness::InitialStress3`]: an isotropic
/// constructor ([`Self::isotropic`]) and a [`Self::zero`] trivial input, with
/// the kernel symmetrising `S` implicitly via the double sum (no `i ≤ j`
/// shortcut), so a slightly off-symmetric input yields the `K_g` of
/// `0.5·(S + Sᵀ)` rather than panicking.
///
/// Isotropic-first per PRD D1; the general 2×2 resultant keeps the anisotropic
/// warp/weft extension (task ε) a drop-in without re-architecting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MembranePrestress {
    /// 2×2 in-plane stress resultant `S` (force/length), element local frame.
    pub resultant: [[f64; 2]; 2],
}

impl MembranePrestress {
    /// Isotropic in-plane prestress resultant `N`: `S = N·I₂`
    /// (`[[N, 0], [0, N]]`). For `N = σ·t` this is a uniform membrane
    /// pretension of stress `σ` through thickness `t`.
    pub fn isotropic(n: f64) -> Self {
        Self {
            resultant: [[n, 0.0], [0.0, n]],
        }
    }

    /// Zero prestress — the trivial input that pins
    /// `geometric_element_stiffness_membrane_cst == 0` entrywise.
    pub const fn zero() -> Self {
        Self {
            resultant: [[0.0; 2]; 2],
        }
    }
}

/// Compute the 9×9 membrane geometric stiffness `K_g` for a flat 3-node CST
/// membrane under an in-plane prestress resultant.
///
/// `nodes` are the three physical vertex positions in global coordinates.
/// `prestress` is the in-plane stress resultant `S` (local frame). Returns an
/// [`ElementStiffness`] with `n_dofs = 9`, row-major, sharing the `3·node +
/// axis` DOF layout of [`ElementStiffness`].
pub fn geometric_element_stiffness_membrane_cst(
    nodes: &[[f64; 3]; 3],
    prestress: &MembranePrestress,
) -> ElementStiffness {
    todo!("geometric_element_stiffness_membrane_cst: implemented in S6")
}

/// Compute the per-element membrane tangent stiffness `K_t = K_e + K_g`.
///
/// A flat membrane's transverse stiffness comes **entirely** from `K_g`
/// (`K_e` is transversely singular), so a transverse/pressure solve requires
/// `K_t`. Mirrors [`crate::geometric_stiffness::bar_tangent_stiffness`].
pub fn membrane_tangent_stiffness(
    nodes: &[[f64; 3]; 3],
    thickness: f64,
    material: &IsotropicElastic,
    prestress: &MembranePrestress,
) -> ElementStiffness {
    todo!("membrane_tangent_stiffness: implemented in S8")
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;
    use crate::assembly::test_support::assert_close;

    /// Unit triangle in the xy-plane: R = I, area A = 0.5,
    /// dn = [(-1,-1), (1,0), (0,1)].
    const UNIT_TRI: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

    /// `K_g · u` for a 9-DOF membrane geometric-stiffness matrix.
    fn matvec9(k: &ElementStiffness, u: &[f64; 9]) -> [f64; 9] {
        let mut ku = [0.0_f64; 9];
        for i in 0..9 {
            for j in 0..9 {
                ku[i] += k.get(i, j) * u[j];
            }
        }
        ku
    }

    fn linf(v: &[f64]) -> f64 {
        v.iter().fold(0.0_f64, |acc, x| acc.max(x.abs()))
    }

    // (a) shape + (b) hand-computed transverse entries.
    //
    // K_g[3i+2][3j+2] = N·A·(dn_i·dn_j), A = 0.5, dn_0=(-1,-1), dn_1=(1,0),
    // dn_2=(0,1):
    //   (w0,w0)=N   (w1,w1)=(w2,w2)=0.5N   (w0,w1)=(w0,w2)=-0.5N   (w1,w2)=0
    // The flat xy-plane triangle has R = I, so w_i lands in global DOF 3i+2
    // (i.e. DOFs 2, 5, 8).
    #[test]
    fn kg_unit_triangle_hand_values() {
        let n = 1000.0_f64;
        let kg = geometric_element_stiffness_membrane_cst(&UNIT_TRI, &MembranePrestress::isotropic(n));
        assert_eq!(kg.n_dofs, 9);
        assert_eq!(kg.data.len(), 81);
        assert_close(kg.get(2, 2), n, 1e-12, "K_g(w0,w0)=N");
        assert_close(kg.get(5, 5), 0.5 * n, 1e-12, "K_g(w1,w1)=0.5N");
        assert_close(kg.get(8, 8), 0.5 * n, 1e-12, "K_g(w2,w2)=0.5N");
        assert_close(kg.get(2, 5), -0.5 * n, 1e-12, "K_g(w0,w1)=-0.5N");
        assert_close(kg.get(2, 8), -0.5 * n, 1e-12, "K_g(w0,w2)=-0.5N");
        assert_close(kg.get(5, 8), 0.0, 1e-12, "K_g(w1,w2)=0");
    }

    // (c) in-plane DOFs carry no geometric stiffness (the 2-D analogue of the
    // bar's axial null).
    #[test]
    fn kg_in_plane_dofs_are_exactly_zero() {
        let kg =
            geometric_element_stiffness_membrane_cst(&UNIT_TRI, &MembranePrestress::isotropic(750.0));
        for &p in &[0usize, 1, 3, 4, 6, 7] {
            for j in 0..9 {
                assert_eq!(kg.get(p, j), 0.0, "in-plane row K_g[{p}][{j}] must be 0");
                assert_eq!(kg.get(j, p), 0.0, "in-plane col K_g[{j}][{p}] must be 0");
            }
        }
    }

    // (d) zero prestress ⇒ all-zero matrix.
    #[test]
    fn kg_zero_prestress_yields_zero_matrix() {
        let kg = geometric_element_stiffness_membrane_cst(&UNIT_TRI, &MembranePrestress::zero());
        for (idx, &v) in kg.data.iter().enumerate() {
            assert_eq!(v, 0.0, "K_g[{idx}]={v} with zero prestress, expected 0");
        }
    }

    // (e) symmetry.
    #[test]
    fn kg_is_symmetric() {
        let kg =
            geometric_element_stiffness_membrane_cst(&UNIT_TRI, &MembranePrestress::isotropic(333.0));
        for i in 0..9 {
            for j in 0..9 {
                assert_close(kg.get(i, j), kg.get(j, i), 1e-12, &format!("sym({i},{j})"));
            }
        }
    }

    // (f) linearity in the resultant: K_g(2N) == 2·K_g(N).
    #[test]
    fn kg_linear_in_resultant() {
        let kg1 =
            geometric_element_stiffness_membrane_cst(&UNIT_TRI, &MembranePrestress::isotropic(100.0));
        let kg2 =
            geometric_element_stiffness_membrane_cst(&UNIT_TRI, &MembranePrestress::isotropic(200.0));
        for i in 0..81 {
            assert_close(kg2.data[i], 2.0 * kg1.data[i], 1e-12, &format!("linear idx {i}"));
        }
    }

    // (g) rigid-body translation null space.
    #[test]
    fn kg_translation_in_null_space() {
        let kg =
            geometric_element_stiffness_membrane_cst(&UNIT_TRI, &MembranePrestress::isotropic(500.0));
        for axis in 0..3 {
            let mut u = [0.0_f64; 9];
            for node in 0..3 {
                u[3 * node + axis] = 1.0;
            }
            let resid = linf(&matvec9(&kg, &u));
            assert!(resid < 1e-12, "translation axis {axis}: ‖K_g·u‖_∞ = {resid}");
        }
    }

    // Finite guard (debug-only), mirroring tet/bar K_g.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "finite")]
    fn kg_panics_on_non_finite_resultant() {
        let bad = MembranePrestress {
            resultant: [[f64::NAN, 0.0], [0.0, 0.0]],
        };
        let _ = geometric_element_stiffness_membrane_cst(&UNIT_TRI, &bad);
    }
}
