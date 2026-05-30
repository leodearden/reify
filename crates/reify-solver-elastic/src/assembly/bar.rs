//! Pin-jointed bar/cable element stiffness for `reify-solver-elastic`.
//!
//! Implements the elastic stiffness `K_e = (EA/L)·cc^T` block kernel for a
//! 2-node, 6-DOF truss element, plus the `BarSection` input struct.
//!
//! See PRD `docs/prds/v0_6/tensegrity-structures.md` §6 task T3a.

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use crate::assembly::{ElementStiffness, bar::{BarSection, element_stiffness_bar_p1}};

    /// Relative-tolerance assert matching the crate convention.
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
    fn ke_returns_6x6() {
        let nodes = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
        let section = BarSection { youngs_modulus: 1.0, area: 1.0 };
        let ke = element_stiffness_bar_p1(&nodes, &section);
        assert_eq!(ke.n_dofs, 6);
        assert_eq!(ke.data.len(), 36);
    }

    // (b) axial bar (0,0,0)→(L,0,0), c=(1,0,0)
    #[test]
    fn ke_axial_bar_x_entries_and_transverse_zero() {
        let e = 2.0e11_f64;
        let a = 1.5e-4_f64;
        let l = 3.0_f64;
        let nodes = [[0.0, 0.0, 0.0], [l, 0.0, 0.0]];
        let section = BarSection { youngs_modulus: e, area: a };
        let ke = element_stiffness_bar_p1(&nodes, &section);
        let ea_over_l = e * a / l;

        assert_close(ke.get(0, 0), ea_over_l, 1e-12, "K(0,0)=EA/L");
        assert_close(ke.get(3, 3), ea_over_l, 1e-12, "K(3,3)=EA/L");
        assert_close(ke.get(0, 3), -ea_over_l, 1e-12, "K(0,3)=-EA/L");
        assert_close(ke.get(3, 0), -ea_over_l, 1e-12, "K(3,0)=-EA/L");

        // All transverse (y,z) entries must be zero
        for i in 0..6 {
            for j in 0..6 {
                if (i == 0 || i == 3) && (j == 0 || j == 3) {
                    continue;
                }
                assert_close(ke.get(i, j), 0.0, 1e-12, &format!("K({i},{j}) transverse=0"));
            }
        }
    }

    // (c) oblique 45° bar (0,0,0)→(d,d,0)
    #[test]
    fn ke_oblique_45deg_bar() {
        let d = 2.0_f64;
        let l = d * 2.0_f64.sqrt();
        let e = 1.0_f64;
        let a = 1.0_f64;
        let nodes = [[0.0, 0.0, 0.0], [d, d, 0.0]];
        let section = BarSection { youngs_modulus: e, area: a };
        let ke = element_stiffness_bar_p1(&nodes, &section);
        let k = e * a / l;
        let half_k = 0.5 * k;

        assert_close(ke.get(0, 0), half_k, 1e-12, "K(0,0)=.5·EA/L");
        assert_close(ke.get(1, 1), half_k, 1e-12, "K(1,1)=.5·EA/L");
        assert_close(ke.get(0, 1), half_k, 1e-12, "K(0,1)=.5·EA/L");
        assert_close(ke.get(0, 3), -half_k, 1e-12, "K(0,3)=-.5·EA/L");
        assert_close(ke.get(1, 4), -half_k, 1e-12, "K(1,4)=-.5·EA/L");
        assert_close(ke.get(0, 4), -half_k, 1e-12, "K(0,4)=-.5·EA/L");
        for j in 0..6 {
            assert_close(ke.get(2, j), 0.0, 1e-12, &format!("K(2,{j})=0"));
            assert_close(ke.get(j, 2), 0.0, 1e-12, &format!("K({j},2)=0"));
            assert_close(ke.get(5, j), 0.0, 1e-12, &format!("K(5,{j})=0"));
            assert_close(ke.get(j, 5), 0.0, 1e-12, &format!("K({j},5)=0"));
        }
    }

    // (d) symmetry
    #[test]
    fn ke_is_symmetric() {
        let nodes = [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]];
        let section = BarSection { youngs_modulus: 200.0e9, area: 1e-4 };
        let ke = element_stiffness_bar_p1(&nodes, &section);
        for i in 0..6 {
            for j in 0..6 {
                assert_close(ke.get(i, j), ke.get(j, i), 1e-12, &format!("sym({i},{j})"));
            }
        }
    }

    // (e) rigid-body translation null space
    #[test]
    fn ke_translation_in_null_space() {
        let nodes = [[0.0, 0.0, 0.0], [3.0, 4.0, 0.0]];
        let section = BarSection { youngs_modulus: 1.0, area: 1.0 };
        let ke = element_stiffness_bar_p1(&nodes, &section);
        for axis in 0..3 {
            let mut u = [0.0_f64; 6];
            for node in 0..2 {
                u[3 * node + axis] = 1.0;
            }
            let mut ku = [0.0_f64; 6];
            for i in 0..6 {
                for j in 0..6 {
                    ku[i] += ke.get(i, j) * u[j];
                }
            }
            let linf = ku.iter().fold(0.0_f64, |acc, x| acc.max(x.abs()));
            assert!(linf < 1e-12, "rigid-body translation axis {axis}: ‖K_e·u‖_∞ = {linf}");
        }
    }

    // (f) scaling: doubling area doubles every entry; doubling L halves axial
    #[test]
    fn ke_scales_with_area_and_length() {
        let e = 1.0_f64;
        let a = 1.0_f64;
        let l = 2.0_f64;
        let nodes = [[0.0, 0.0, 0.0], [l, 0.0, 0.0]];
        let ke1 = element_stiffness_bar_p1(&nodes, &BarSection { youngs_modulus: e, area: a });
        let ke2 = element_stiffness_bar_p1(&nodes, &BarSection { youngs_modulus: e, area: 2.0 * a });
        for i in 0..36 {
            assert_close(ke2.data[i], 2.0 * ke1.data[i], 1e-12, &format!("area·2 idx {i}"));
        }
        let nodes_2l = [[0.0, 0.0, 0.0], [2.0 * l, 0.0, 0.0]];
        let ke3 = element_stiffness_bar_p1(&nodes_2l, &BarSection { youngs_modulus: e, area: a });
        assert_close(ke3.get(0, 0), 0.5 * ke1.get(0, 0), 1e-12, "double-L halves axial");
    }

    // degeneracy guard
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "degenerate bar")]
    fn ke_zero_length_bar_panics() {
        let nodes = [[1.0, 2.0, 3.0], [1.0, 2.0, 3.0]];
        let _ = element_stiffness_bar_p1(&nodes, &BarSection { youngs_modulus: 1.0, area: 1.0 });
    }
}
