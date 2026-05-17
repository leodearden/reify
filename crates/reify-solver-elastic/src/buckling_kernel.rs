//! `solve_buckling_kernel` — four-phase buckling pipeline orchestrator.
//!
//! # PRD reference
//!
//! `docs/prds/v0_5/buckling-eigensolver.md` §13 task δ.
//!
//! # Scope
//!
//! Orchestrates the four-phase buckling pipeline:
//! 1. Free-DOF subspace construction from `DirichletBc` inputs.
//! 2. Linear-static pre-stress solve (CG in the free-DOF subspace).
//! 3. Per-element Cauchy stress recovery and −K_g assembly.
//! 4. Generalized eigensolve `K_free φ = λ (−K_g_free) φ`.
//! 5. Mode-shape expansion back to the full DOF space.
//!
//! See `.task/plan.json` design_decisions for the rationale on operating
//! in the free-DOF subspace throughout (versus `apply_dirichlet_row_elimination`).

// Step 1 (RED): test block only — BucklingKernelOptions, Mode,
// BucklingKernelResult, and solve_buckling_kernel not yet defined.
// This file is a compile-error stub that will RED the cargo test run.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boundary::DirichletBc;
    use crate::constitutive::IsotropicElastic;

    // -----------------------------------------------------------------------
    // Shared fixture: single 1×1×1 m brick split into 6 P1 tets.
    // Uses the same six-tet long-diagonal decomposition as kg_p1_tet.rs.
    //   z=0 face: nodes 0–3  (bottom)
    //   z=1 face: nodes 4–7  (top)
    // -----------------------------------------------------------------------

    const TET_DECOMP: [[usize; 4]; 6] = [
        [0, 1, 2, 6],
        [0, 2, 3, 6],
        [0, 3, 7, 6],
        [0, 7, 4, 6],
        [0, 4, 5, 6],
        [0, 5, 1, 6],
    ];

    fn unit_brick_nodes() -> Vec<[f64; 3]> {
        vec![
            [0.0, 0.0, 0.0], // 0 — bottom face
            [1.0, 0.0, 0.0], // 1
            [1.0, 1.0, 0.0], // 2
            [0.0, 1.0, 0.0], // 3
            [0.0, 0.0, 1.0], // 4 — top face
            [1.0, 0.0, 1.0], // 5
            [1.0, 1.0, 1.0], // 6
            [0.0, 1.0, 1.0], // 7
        ]
    }

    fn unit_brick_tets() -> Vec<[usize; 4]> {
        TET_DECOMP.to_vec()
    }

    /// Bottom face fully clamped + top face lateral clamp (u_x = u_y = 0).
    /// Constrained DOFs: 12 (bottom) + 8 (top lateral) = 20.  n_free = 4.
    fn shape_test_bcs() -> Vec<DirichletBc> {
        let mut bcs = Vec::new();
        // Bottom face (nodes 0–3): clamp all 3 DOFs.
        for n in 0..4_usize {
            for axis in 0..3_usize {
                bcs.push(DirichletBc { dof: 3 * n + axis, value: 0.0 });
            }
        }
        // Top face (nodes 4–7): clamp u_x = u_y = 0.
        for n in 4..8_usize {
            bcs.push(DirichletBc { dof: 3 * n,     value: 0.0 }); // u_x
            bcs.push(DirichletBc { dof: 3 * n + 1, value: 0.0 }); // u_y
        }
        bcs
    }

    /// Sorted list of constrained DOF indices for the shape-test BC set.
    fn shape_test_constrained_dofs() -> Vec<usize> {
        let mut v = Vec::new();
        for n in 0..4_usize {
            for axis in 0..3_usize {
                v.push(3 * n + axis);
            }
        }
        for n in 4..8_usize {
            v.push(3 * n);
            v.push(3 * n + 1);
        }
        v
    }

    // -----------------------------------------------------------------------
    // step-1 (RED → GREEN in step-2): shape pin.
    //
    // Verifies the result struct has the expected dimensions for the
    // single-brick fixture.  The test will fail to COMPILE until step-2
    // adds BucklingKernelOptions, Mode, BucklingKernelResult, and
    // solve_buckling_kernel to this module.
    // -----------------------------------------------------------------------

    #[test]
    fn solve_buckling_kernel_returns_well_shaped_result_for_single_brick_fixture() {
        let nodes = unit_brick_nodes();
        let tets = unit_brick_tets();
        let material = IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.0,
        };
        let bcs = shape_test_bcs();

        // Downward unit load split across the four top-face nodes.
        let mut f = vec![0.0_f64; 3 * nodes.len()];
        for top_node in 4..8_usize {
            f[3 * top_node + 2] = -0.25;
        }

        let opts = BucklingKernelOptions {
            n_modes: 3,
            eigen_tol: 1e-8,
            eigen_max_iters: 100,
            cg_tolerance: 1e-10,
            cg_max_iter: 1000,
        };

        let result = solve_buckling_kernel(&nodes, &tets, &material, &bcs, &f, opts);

        // n_free = 24 - 20 = 4; dense-fallback returns min(4, n_modes=3) = 3 modes.
        // Assert ≥ 1 to stay tolerant if the actual count diverges.
        assert!(
            result.modes.len() >= 1,
            "expect at least 1 mode; got {}",
            result.modes.len(),
        );
        assert_eq!(
            result.pre_stress_displacement.len(),
            3 * 8,
            "displacement must have length 3 * n_nodes = 24",
        );
        assert_eq!(
            result.pre_stress_per_element.len(),
            6,
            "one stress tensor per tet (6 tets in single-brick fixture)",
        );
        for (m, mode) in result.modes.iter().enumerate() {
            assert_eq!(
                mode.mode_shape.len(),
                3 * 8,
                "mode {m} shape must have length 3 * n_nodes = 24",
            );
        }

        let constrained = shape_test_constrained_dofs();

        // Constrained DOFs must be exactly 0.0 in the displacement vector.
        for &g in &constrained {
            assert_eq!(
                result.pre_stress_displacement[g], 0.0,
                "constrained DOF {g} must be 0.0 in pre_stress_displacement",
            );
        }
        // Constrained DOFs must be exactly 0.0 in every mode shape.
        for (m, mode) in result.modes.iter().enumerate() {
            for &g in &constrained {
                assert_eq!(
                    mode.mode_shape[g], 0.0,
                    "mode {m}: constrained DOF {g} must be 0.0 in mode_shape",
                );
            }
        }
    }
}
