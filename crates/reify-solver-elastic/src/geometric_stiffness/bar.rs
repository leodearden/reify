//! Pin-jointed bar/cable geometric stiffness for `reify-solver-elastic`.
//!
//! Implements the geometric stiffness `K_g = (N/L)·(I − cc^T)` block kernel
//! for a 2-node, 6-DOF truss element, plus the per-member tangent stiffness
//! `bar_tangent_stiffness = K_e + K_g`.
//!
//! See PRD `docs/prds/v0_6/tensegrity-structures.md` §6 task T3a.

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use crate::geometric_stiffness::bar::geometric_element_stiffness_bar_p1;

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

        // Transverse diagonal entries: node0-y/z and node1-y/z
        assert_close(kg.get(1, 1), n_over_l, 1e-12, "K_g(1,1)=N/L");
        assert_close(kg.get(2, 2), n_over_l, 1e-12, "K_g(2,2)=N/L");
        assert_close(kg.get(4, 4), n_over_l, 1e-12, "K_g(4,4)=N/L");
        assert_close(kg.get(5, 5), n_over_l, 1e-12, "K_g(5,5)=N/L");
        // Cross-node transverse coupling: −N/L
        assert_close(kg.get(1, 4), -n_over_l, 1e-12, "K_g(1,4)=-N/L");
        assert_close(kg.get(2, 5), -n_over_l, 1e-12, "K_g(2,5)=-N/L");
        assert_close(kg.get(4, 1), -n_over_l, 1e-12, "K_g(4,1)=-N/L");
        assert_close(kg.get(5, 2), -n_over_l, 1e-12, "K_g(5,2)=-N/L");

        // Axial (x) row and column must be zero: T=(I-cc^T) has zero axial component
        for j in 0..6 {
            assert_close(kg.get(0, j), 0.0, 1e-12, &format!("K_g(0,{j}) axial=0"));
            assert_close(kg.get(j, 0), 0.0, 1e-12, &format!("K_g({j},0) axial=0"));
            assert_close(kg.get(3, j), 0.0, 1e-12, &format!("K_g(3,{j}) axial=0"));
            assert_close(kg.get(j, 3), 0.0, 1e-12, &format!("K_g({j},3) axial=0"));
        }
    }

    // (d) oblique 45° bar (0,0,0)→(d,d,0): T = I − cc^T = [[0.5,−0.5,0],[−0.5,0.5,0],[0,0,1]]
    #[test]
    fn kg_oblique_45deg_bar() {
        let d = 3.0_f64;
        let l = d * 2.0_f64.sqrt();
        let n = 50.0_f64;
        let nodes = [[0.0, 0.0, 0.0], [d, d, 0.0]];
        let kg = geometric_element_stiffness_bar_p1(&nodes, n);
        let scale = n / l;

        // T_xx = 1 − 0.5 = 0.5, T_yy = 0.5, T_xy = −0.5, T_zz = 1.0
        // node0-node0 block diagonal (T_ij * N/L):
        assert_close(kg.get(0, 0), 0.5 * scale, 1e-12, "K_g(0,0)=0.5·N/L");
        assert_close(kg.get(1, 1), 0.5 * scale, 1e-12, "K_g(1,1)=0.5·N/L");
        assert_close(kg.get(2, 2), 1.0 * scale, 1e-12, "K_g(2,2)=N/L");
        // off-diagonal in same block: T_xy = −0.5
        assert_close(kg.get(0, 1), -0.5 * scale, 1e-12, "K_g(0,1)=-0.5·N/L");
        assert_close(kg.get(1, 0), -0.5 * scale, 1e-12, "K_g(1,0)=-0.5·N/L");
        // Cross-node block: negated
        assert_close(kg.get(0, 3), -0.5 * scale, 1e-12, "K_g(0,3)=-0.5·N/L");
        assert_close(kg.get(1, 4), -0.5 * scale, 1e-12, "K_g(1,4)=-0.5·N/L");
        assert_close(kg.get(2, 5), -1.0 * scale, 1e-12, "K_g(2,5)=-N/L");
        // z-zero check for x-z cross terms (T_xz = T_yz = 0)
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

    // (f) linear in N: doubling N doubles every entry
    #[test]
    fn kg_linear_in_axial_force() {
        let nodes = [[0.0, 0.0, 0.0], [0.0, 5.0, 0.0]];
        let kg1 = geometric_element_stiffness_bar_p1(&nodes, 10.0);
        let kg2 = geometric_element_stiffness_bar_p1(&nodes, 20.0);
        for i in 0..36 {
            assert_close(kg2.data[i], 2.0 * kg1.data[i], 1e-12, &format!("linear-N idx {i}"));
        }
    }

    // (g) rigid-body translation null space: K_g · u = 0
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
}
