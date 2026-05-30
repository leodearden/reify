//! Pin-jointed bar/cable geometric stiffness for `reify-solver-elastic`.
//!
//! Implements the geometric stiffness `K_g = (N/L)·(I − cc^T)` block kernel
//! for a 2-node, 6-DOF truss element, plus the per-member tangent stiffness
//! `bar_tangent_stiffness = K_e + K_g`.
//!
//! See PRD `docs/prds/v0_6/tensegrity-structures.md` §6 task T3a.

use crate::assembly::{BarSection, ElementStiffness, element_stiffness_bar_p1};

/// Minimum bar length guard — mirrors `MIN_JACOBIAN_DET` degeneracy convention
/// in the tet elastic and tet geometric-stiffness modules.
const MIN_BAR_LENGTH: f64 = 1.0e-30;

/// Compute the 6×6 geometric stiffness matrix `K_g` for a 2-node pin-jointed
/// bar under an axial member force `N` (tension positive).
///
/// `phys_nodes` are the two endpoint positions in physical (global) coordinates.
/// `axial_force` is the pre-existing member force `N`; positive = tension
/// (stiffens transverse DOFs), negative = compression (softens transverse DOFs).
///
/// # Formula
///
/// With unit direction vector `c = (node1 − node0) / L` and transverse
/// projector `T = I₃ − cc^T`:
///
/// ```text
/// K_g = (N/L) · [[T,  −T],
///                 [−T, T]]
/// ```
///
/// The axial direction carries no geometric stiffness: `T·c = (I − cc^T)·c = 0`.
/// The derivation follows `∂(N·c)/∂x = (N/L)(I₃ − cc^T)` from the derivative
/// of the normalized bar direction with respect to endpoint displacement.
///
/// # DOF layout
///
/// `dof = 3 * node_idx + axis`, matching [`ElementStiffness`]'s `3·node+axis`
/// convention. Node 0 → DOFs 0–2, node 1 → DOFs 3–5.
///
/// # Panics
///
/// Panics under `debug_assertions` when:
/// - `axial_force` is non-finite (NaN or ±∞),
/// - `L <= MIN_BAR_LENGTH` (degenerate/zero-length bar).
pub fn geometric_element_stiffness_bar_p1(
    phys_nodes: &[[f64; 3]; 2],
    axial_force: f64,
) -> ElementStiffness {
    debug_assert!(
        axial_force.is_finite(),
        "axial_force must be finite, got {}",
        axial_force,
    );

    let r = [
        phys_nodes[1][0] - phys_nodes[0][0],
        phys_nodes[1][1] - phys_nodes[0][1],
        phys_nodes[1][2] - phys_nodes[0][2],
    ];
    let l = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt();

    debug_assert!(
        l > MIN_BAR_LENGTH,
        "degenerate bar: L = {} (must be > {})",
        l,
        MIN_BAR_LENGTH,
    );

    let c = [r[0] / l, r[1] / l, r[2] / l];
    let scale = axial_force / l;

    // Transverse projector T_ij = δ_ij − c_i·c_j
    let mut t = [[0.0_f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let delta = if i == j { 1.0 } else { 0.0 };
            t[i][j] = delta - c[i] * c[j];
        }
    }

    let mut kg = ElementStiffness::zeros(6);
    // Block pattern: [[(N/L)·T, −(N/L)·T], [−(N/L)·T, (N/L)·T]]
    for a in 0..2usize {
        for b in 0..2usize {
            let sign = if a == b { 1.0 } else { -1.0 };
            for (i, t_row) in t.iter().enumerate() {
                for (j, &t_val) in t_row.iter().enumerate() {
                    let row = 3 * a + i;
                    let col = 3 * b + j;
                    kg.data[row * 6 + col] = sign * scale * t_val;
                }
            }
        }
    }
    kg
}

/// Compute the per-member static tangent stiffness `K_t = K_e + K_g`.
///
/// This is the linearised stiffness seen by an incremental displacement around
/// a pre-stressed state with member force `N`. The T3b load solver assembles
/// `K_t` over all members to form the global tangent.
///
/// `doc-note:` tension-only/active-set cable enforcement and load-solve
/// orchestration are task T3b's domain.
pub fn bar_tangent_stiffness(
    phys_nodes: &[[f64; 3]; 2],
    section: &BarSection,
    axial_force: f64,
) -> ElementStiffness {
    let ke = element_stiffness_bar_p1(phys_nodes, section);
    let kg = geometric_element_stiffness_bar_p1(phys_nodes, axial_force);
    let mut kt = ElementStiffness::zeros(6);
    for i in 0..36 {
        kt.data[i] = ke.data[i] + kg.data[i];
    }
    kt
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::{bar_tangent_stiffness, geometric_element_stiffness_bar_p1};
    use crate::assembly::{BarSection, element_stiffness_bar_p1};

    fn assert_close(lhs: f64, rhs: f64, tol: f64, label: &str) {
        let scale = lhs.abs().max(rhs.abs()).max(1.0);
        assert!(
            (lhs - rhs).abs() < tol * scale,
            "{label}: |{lhs} − {rhs}| = {} ≥ tol·scale = {}",
            (lhs - rhs).abs(),
            tol * scale,
        );
    }

    // (a) returns 6×6
    #[test]
    fn kg_returns_6x6() {
        let nodes = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
        let kg = geometric_element_stiffness_bar_p1(&nodes, 10.0);
        assert_eq!(kg.n_dofs, 6);
        assert_eq!(kg.data.len(), 36);
    }

    // (b) zero force → all-zero matrix
    #[test]
    fn kg_zero_force_yields_zero_matrix() {
        let nodes = [[0.0, 0.0, 0.0], [2.0, 3.0, 1.0]];
        let kg = geometric_element_stiffness_bar_p1(&nodes, 0.0);
        for (idx, &v) in kg.data.iter().enumerate() {
            assert_eq!(v, 0.0, "kg[{idx}] = {v} with N=0, expected 0");
        }
    }

    // (c) axial bar (0,0,0)→(L,0,0), force N: T=diag(0,1,1) → transverse entries N/L, axial 0
    #[test]
    fn kg_axial_bar_transverse_entries_and_axial_zero() {
        let l = 4.0_f64;
        let n = 100.0_f64;
        let nodes = [[0.0, 0.0, 0.0], [l, 0.0, 0.0]];
        let kg = geometric_element_stiffness_bar_p1(&nodes, n);
        let n_over_l = n / l;

        assert_close(kg.get(1, 1), n_over_l, 1e-12, "K_g(1,1)=N/L");
        assert_close(kg.get(2, 2), n_over_l, 1e-12, "K_g(2,2)=N/L");
        assert_close(kg.get(4, 4), n_over_l, 1e-12, "K_g(4,4)=N/L");
        assert_close(kg.get(5, 5), n_over_l, 1e-12, "K_g(5,5)=N/L");
        assert_close(kg.get(1, 4), -n_over_l, 1e-12, "K_g(1,4)=-N/L");
        assert_close(kg.get(2, 5), -n_over_l, 1e-12, "K_g(2,5)=-N/L");
        assert_close(kg.get(4, 1), -n_over_l, 1e-12, "K_g(4,1)=-N/L");
        assert_close(kg.get(5, 2), -n_over_l, 1e-12, "K_g(5,2)=-N/L");

        for j in 0..6 {
            assert_close(kg.get(0, j), 0.0, 1e-12, &format!("K_g(0,{j}) axial=0"));
            assert_close(kg.get(j, 0), 0.0, 1e-12, &format!("K_g({j},0) axial=0"));
            assert_close(kg.get(3, j), 0.0, 1e-12, &format!("K_g(3,{j}) axial=0"));
            assert_close(kg.get(j, 3), 0.0, 1e-12, &format!("K_g({j},3) axial=0"));
        }
    }

    // (d) oblique 45° bar: T = I − cc^T = [[0.5,−0.5,0],[−0.5,0.5,0],[0,0,1]]
    #[test]
    fn kg_oblique_45deg_bar() {
        let d = 3.0_f64;
        let l = d * 2.0_f64.sqrt();
        let n = 50.0_f64;
        let nodes = [[0.0, 0.0, 0.0], [d, d, 0.0]];
        let kg = geometric_element_stiffness_bar_p1(&nodes, n);
        let s = n / l;

        assert_close(kg.get(0, 0), 0.5 * s, 1e-12, "K_g(0,0)=0.5·N/L");
        assert_close(kg.get(1, 1), 0.5 * s, 1e-12, "K_g(1,1)=0.5·N/L");
        assert_close(kg.get(2, 2), 1.0 * s, 1e-12, "K_g(2,2)=N/L");
        assert_close(kg.get(0, 1), -0.5 * s, 1e-12, "K_g(0,1)=-0.5·N/L");
        assert_close(kg.get(1, 0), -0.5 * s, 1e-12, "K_g(1,0)=-0.5·N/L");
        assert_close(kg.get(0, 3), -0.5 * s, 1e-12, "K_g(0,3)=-0.5·N/L");
        assert_close(kg.get(1, 4), -0.5 * s, 1e-12, "K_g(1,4)=-0.5·N/L");
        assert_close(kg.get(2, 5), -s, 1e-12, "K_g(2,5)=-N/L");
        assert_close(kg.get(0, 2), 0.0, 1e-12, "K_g(0,2)=0");
        assert_close(kg.get(1, 2), 0.0, 1e-12, "K_g(1,2)=0");
    }

    // (e) symmetry
    #[test]
    fn kg_is_symmetric() {
        let nodes = [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]];
        let kg = geometric_element_stiffness_bar_p1(&nodes, 42.0);
        for i in 0..6 {
            for j in 0..6 {
                assert_close(kg.get(i, j), kg.get(j, i), 1e-12, &format!("sym({i},{j})"));
            }
        }
    }

    // (f) linear in N
    #[test]
    fn kg_linear_in_axial_force() {
        let nodes = [[0.0, 0.0, 0.0], [0.0, 5.0, 0.0]];
        let kg1 = geometric_element_stiffness_bar_p1(&nodes, 10.0);
        let kg2 = geometric_element_stiffness_bar_p1(&nodes, 20.0);
        for i in 0..36 {
            assert_close(kg2.data[i], 2.0 * kg1.data[i], 1e-12, &format!("linear-N idx {i}"));
        }
    }

    // (g) rigid-body translation null space
    #[test]
    fn kg_translation_in_null_space() {
        let nodes = [[0.0, 0.0, 0.0], [3.0, 4.0, 0.0]];
        let kg = geometric_element_stiffness_bar_p1(&nodes, 100.0);
        for axis in 0..3 {
            let mut u = [0.0_f64; 6];
            for node in 0..2 {
                u[3 * node + axis] = 1.0;
            }
            let mut ku = [0.0_f64; 6];
            for i in 0..6 {
                for j in 0..6 {
                    ku[i] += kg.get(i, j) * u[j];
                }
            }
            let linf = ku.iter().fold(0.0_f64, |acc, x| acc.max(x.abs()));
            assert!(linf < 1e-12, "rigid-body translation axis {axis}: ‖K_g·u‖_∞ = {linf}");
        }
    }

    // (h) degenerate zero-length bar
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "degenerate bar")]
    fn kg_zero_length_bar_panics() {
        let nodes = [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
        let _ = geometric_element_stiffness_bar_p1(&nodes, 10.0);
    }

    // ---------- bar_tangent_stiffness tests (step-5 cases, placed here per plan) ----------

    // (a) tangent returns 6×6
    #[test]
    fn kt_returns_6x6() {
        let nodes = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
        let section = BarSection { youngs_modulus: 1.0, area: 1.0 };
        let kt = bar_tangent_stiffness(&nodes, &section, 0.0);
        assert_eq!(kt.n_dofs, 6);
        assert_eq!(kt.data.len(), 36);
    }

    // (b) zero force → K_t equals K_e entrywise
    #[test]
    fn kt_zero_force_equals_ke() {
        let nodes = [[0.0, 0.0, 0.0], [3.0, 4.0, 0.0]];
        let section = BarSection { youngs_modulus: 200.0e9, area: 1e-4 };
        let ke = element_stiffness_bar_p1(&nodes, &section);
        let kt = bar_tangent_stiffness(&nodes, &section, 0.0);
        for i in 0..36 {
            assert_close(kt.data[i], ke.data[i], 1e-12, &format!("kt==ke idx {i}"));
        }
    }

    // (c) tension N>0, axial bar: axial DOF = EA/L (K_g zero axially), transverse = N/L (K_e zero transversely)
    #[test]
    fn kt_axial_bar_decoupled_superposition() {
        let e = 1.0e6_f64;
        let a = 0.01_f64;
        let l = 2.0_f64;
        let n = 500.0_f64;
        let nodes = [[0.0, 0.0, 0.0], [l, 0.0, 0.0]];
        let section = BarSection { youngs_modulus: e, area: a };
        let kt = bar_tangent_stiffness(&nodes, &section, n);
        let ea_over_l = e * a / l;
        let n_over_l = n / l;

        // axial DOF: K_e contributes EA/L, K_g contributes 0
        assert_close(kt.get(0, 0), ea_over_l, 1e-12, "K_t(0,0)=EA/L");
        // transverse DOF: K_e contributes 0, K_g contributes N/L
        assert_close(kt.get(1, 1), n_over_l, 1e-12, "K_t(1,1)=N/L");
        assert_close(kt.get(2, 2), n_over_l, 1e-12, "K_t(2,2)=N/L");
    }

    // (d) entrywise equals K_e + K_g for oblique bar
    #[test]
    fn kt_entrywise_equals_ke_plus_kg() {
        let d = 2.0_f64;
        let nodes = [[0.0, 0.0, 0.0], [d, d, 0.0]];
        let section = BarSection { youngs_modulus: 1e5, area: 0.05 };
        let n = 200.0;
        let ke = element_stiffness_bar_p1(&nodes, &section);
        let kg = geometric_element_stiffness_bar_p1(&nodes, n);
        let kt = bar_tangent_stiffness(&nodes, &section, n);
        for i in 0..36 {
            assert_close(
                kt.data[i],
                ke.data[i] + kg.data[i],
                1e-12,
                &format!("kt==ke+kg idx {i}"),
            );
        }
    }
}
