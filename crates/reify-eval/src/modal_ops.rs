//! Compute trampoline for `modal::free_vibration` — the `fn modal_analysis`
//! `@optimized` target (task ζ, docs/prds/v0_3/modal-analysis.md §10).
//!
//! Hosts the modal free-vibration FEA solve (assemble K + M, free-DOF
//! eigensolve via `reify-solver-elastic`) and the `Value`-shaping trampoline.
//! Lives in `reify-eval` — not `reify-stdlib` — because the solve needs
//! `reify-solver-elastic` (which `reify-stdlib` does not depend on); `reify-eval`
//! depends on both. Mirrors `compute_targets/buckling.rs`. Implementations land
//! in steps 4+.

#[cfg(test)]
mod tests {
    use reify_solver_elastic::{DirichletBc, EigenSolverOptions, IsotropicElastic};

    use super::{ModalCoreResult, build_beam_mesh, solve_modal_core};

    /// Steel-like isotropic material (E = 205 GPa, ν = 0.29) shared across the
    /// modal core-solver fixtures.
    fn steel() -> IsotropicElastic {
        IsotropicElastic { youngs_modulus: 205e9, poisson_ratio: 0.29 }
    }

    /// Steel density (kg/m³) — feeds the consistent mass matrix.
    const STEEL_DENSITY: f64 = 7850.0;

    /// Build homogeneous Dirichlet BCs clamping every DOF on the x_min (root)
    /// face — the cantilever root clamp. Returns the BC list together with the
    /// constrained-DOF index list (for the zeroed-shape assertion).
    ///
    /// The face is identified by node coordinate (`x ≈ 0`) read from the shared
    /// [`build_beam_mesh`] mesh, so the test stays robust to the internal node
    /// numbering of `solve_modal_core` (which meshes via the same helper).
    fn clamp_x_min_face(nodes: &[[f64; 3]]) -> (Vec<DirichletBc>, Vec<usize>) {
        let mut bcs = Vec::new();
        let mut dofs = Vec::new();
        for (n, coord) in nodes.iter().enumerate() {
            if coord[0] <= 1e-9 {
                for axis in 0..3 {
                    bcs.push(DirichletBc { dof: 3 * n + axis, value: 0.0 });
                    dofs.push(3 * n + axis);
                }
            }
        }
        (bcs, dofs)
    }

    /// step-3 (RED → GREEN in step-4): shape + sanity pin for `solve_modal_core`.
    ///
    /// Coarse root-clamped block fixture (X = length = 20 mm beam axis,
    /// Y = width = 50 mm, Z = height = 100 mm bending axis). The internal
    /// shear-locking-aware mesh yields nx=1, ny=1, nz=6 → 28 nodes, 42 free DOFs
    /// — small enough for the eigensolver's dense fallback (fast, deterministic).
    /// This is a structural pin, NOT an accuracy check (frequency accuracy is
    /// the e2e test's job, steps 15/17).
    #[test]
    fn solve_modal_core_returns_well_shaped_result_for_coarse_cantilever() {
        let length = 0.02_f64; // X — beam axis (short → coarse mesh)
        let width = 0.05_f64; // Y — width
        let height = 0.1_f64; // Z — bending axis

        let mesh = build_beam_mesh(length, width, height);
        let (bcs, constrained_dofs) = clamp_x_min_face(&mesh.nodes);
        assert!(
            !constrained_dofs.is_empty(),
            "fixture must clamp at least one face DOF",
        );

        let eigen_opts =
            EigenSolverOptions { n_modes: 3, tol: 1e-8, max_iters: 200, sigma: 0.0 };

        let result: ModalCoreResult = solve_modal_core(
            STEEL_DENSITY,
            &steel(),
            length,
            width,
            height,
            &bcs,
            &eigen_opts,
        );

        // (a) n_nodes matches the shared mesh; ≥ 1 mode returned.
        assert_eq!(
            result.n_nodes,
            mesh.nodes.len(),
            "result.n_nodes must equal the shared mesh node count",
        );
        assert!(
            !result.frequencies.is_empty(),
            "expect at least 1 mode; got {}",
            result.frequencies.len(),
        );

        // (b) frequencies finite, strictly positive, sorted ascending.
        for (i, &f) in result.frequencies.iter().enumerate() {
            assert!(
                f.is_finite() && f > 0.0,
                "frequency[{i}] = {f} must be finite and strictly positive",
            );
        }
        for w in result.frequencies.windows(2) {
            assert!(
                w[0] <= w[1],
                "frequencies must be sorted ascending: {} > {}",
                w[0],
                w[1],
            );
        }

        // (c) one full-DOF mode shape per frequency, each length 3·n_nodes.
        assert_eq!(
            result.phi_full.len(),
            result.frequencies.len(),
            "one full mode shape per returned frequency",
        );
        for (i, phi) in result.phi_full.iter().enumerate() {
            assert_eq!(
                phi.len(),
                3 * result.n_nodes,
                "mode {i} shape length must be 3·n_nodes = {}",
                3 * result.n_nodes,
            );
        }

        // (d) every constrained (Dirichlet) DOF is exactly 0.0 in every
        //     scattered mode shape (free-DOF subspace scatter-back).
        for (i, phi) in result.phi_full.iter().enumerate() {
            for &g in &constrained_dofs {
                assert_eq!(
                    phi[g], 0.0,
                    "mode {i}: constrained DOF {g} must be exactly 0.0",
                );
            }
        }
    }
}
