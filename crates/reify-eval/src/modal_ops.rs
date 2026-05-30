//! Compute trampoline for `modal::free_vibration` — the `fn modal_analysis`
//! `@optimized` target (task ζ, docs/prds/v0_3/modal-analysis.md §10).
//!
//! Hosts the modal free-vibration FEA solve (assemble K + M, free-DOF
//! eigensolve via `reify-solver-elastic`) and the `Value`-shaping trampoline.
//! Lives in `reify-eval` — not `reify-stdlib` — because the solve needs
//! `reify-solver-elastic` (which `reify-stdlib` does not depend on); `reify-eval`
//! depends on both. Mirrors `compute_targets/buckling.rs`.
//!
//! `solve_modal_core` (step 4) is the core FEA eigensolve; the public
//! `solve_modal_analysis_trampoline` (step 14) is what wires it into the
//! `@optimized` dispatch path. Until that lands, the core solver + its mesh /
//! projection helpers have no non-test caller, so they carry `#[allow(dead_code)]`
//! (removed once the trampoline consumes them).

use faer::sparse::{SparseRowMat, Triplet};

use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, DirichletBc, EigenSolverOptions, EigenSolverResult, ElementOrder,
    ElementStiffness, IsotropicElastic, assemble_global_stiffness, consistent_element_mass_tet_p1,
    element_stiffness, solve_eigen_dense, solve_eigen_shift_invert,
};
use reify_stdlib::modal::free_vibration::{eigenvalue_to_frequency_hz, mass_normalization_scale};

// ---------------------------------------------------------------------------
// Beam mesh
// ---------------------------------------------------------------------------

/// P1-tet beam mesh shared by [`solve_modal_core`] and its unit tests.
///
/// Layout: X = beam axis (length), Y = width, Z = height (bending axis) —
/// identical to `compute_targets::elastic_static::solve_cantilever_fea`.
#[allow(dead_code)]
pub(crate) struct BeamMesh {
    /// Node coordinates `[x, y, z]`, length `n_nodes`.
    pub(crate) nodes: Vec<[f64; 3]>,
    /// Element connectivity; each tet is `[n0, n1, n2, n3]` (positive Jacobian).
    pub(crate) tets: Vec<[usize; 4]>,
}

/// Build a Freudenthal hex-split P1-tet beam mesh with shear-locking-aware `nx`
/// scaling.
///
/// `nz = 6` fixed; `nx ∝ nz·(L/h)` (rounded, clamped ≥ 1) keeps the bending-plane
/// (XZ) elements near-cubic so the P1 constant-strain tets do not lock in
/// bending; `ny = 1` (bending is about Y). This mirrors `solve_cantilever_fea`'s
/// meshing so the modal mesh matches the validated elastic-static pattern.
#[allow(dead_code)]
pub(crate) fn build_beam_mesh(length: f64, width: f64, height: f64) -> BeamMesh {
    let nz: usize = 6;
    // Clamp to ≥ 1 to handle degenerate geometry (height ≈ or ≫ length).
    let nx: usize = ((length / height * nz as f64).round() as usize).max(1);
    let ny: usize = 1;
    let nx1 = nx + 1;
    let ny1 = ny + 1;
    let nz1 = nz + 1;
    let n_nodes = nx1 * ny1 * nz1;

    let node_idx = |ix: usize, iy: usize, iz: usize| -> usize { iz * ny1 * nx1 + iy * nx1 + ix };
    let node_coord = |ix: usize, iy: usize, iz: usize| -> [f64; 3] {
        [
            ix as f64 * length / nx as f64,
            iy as f64 * width / ny as f64,
            iz as f64 * height / nz as f64,
        ]
    };

    let mut nodes = vec![[0.0_f64; 3]; n_nodes];
    for iz in 0..nz1 {
        for iy in 0..ny1 {
            for ix in 0..nx1 {
                nodes[node_idx(ix, iy, iz)] = node_coord(ix, iy, iz);
            }
        }
    }

    // Freudenthal 6-tet decomposition of each hex sharing the body diagonal
    // c[0]→c[6]; node order chosen for a positive Jacobian (cf. elastic_static).
    let mut tets: Vec<[usize; 4]> = Vec::with_capacity(nx * ny * nz * 6);
    for hz in 0..nz {
        for hy in 0..ny {
            for hx in 0..nx {
                let c = [
                    node_idx(hx, hy, hz),
                    node_idx(hx + 1, hy, hz),
                    node_idx(hx + 1, hy + 1, hz),
                    node_idx(hx, hy + 1, hz),
                    node_idx(hx, hy, hz + 1),
                    node_idx(hx + 1, hy, hz + 1),
                    node_idx(hx + 1, hy + 1, hz + 1),
                    node_idx(hx, hy + 1, hz + 1),
                ];
                tets.extend_from_slice(&[
                    [c[0], c[1], c[2], c[6]],
                    [c[0], c[2], c[3], c[6]],
                    [c[0], c[5], c[1], c[6]],
                    [c[0], c[3], c[7], c[6]],
                    [c[0], c[4], c[5], c[6]],
                    [c[0], c[7], c[4], c[6]],
                ]);
            }
        }
    }

    BeamMesh { nodes, tets }
}

// ---------------------------------------------------------------------------
// Core modal solve
// ---------------------------------------------------------------------------

/// Output of [`solve_modal_core`].
///
/// Field consumption is staged: `frequencies` / `phi_full` / `n_nodes` are
/// pinned now (step 3/4); `eigenvalues` / `phi_free` / `m_free` feed mass
/// normalization + participation mass (steps 5–8); `converged` / `n_converged`
/// feed the convergence diagnostics (steps 9–10) and the trampoline outcome
/// (step 14). `#[allow(dead_code)]` covers the not-yet-read fields during that
/// staged build-up.
#[allow(dead_code)]
pub(crate) struct ModalCoreResult {
    /// Natural frequencies (Hz), ascending. One per returned mode.
    pub(crate) frequencies: Vec<f64>,
    /// Eigenvalues `λ = ω²` (rad²/s²), ascending by |λ|. One per mode.
    pub(crate) eigenvalues: Vec<f64>,
    /// Free-DOF mode shapes (length `n_free`), one per mode.
    pub(crate) phi_free: Vec<Vec<f64>>,
    /// Full-DOF mode shapes (length `3·n_nodes`, `0.0` at constrained DOFs).
    pub(crate) phi_full: Vec<Vec<f64>>,
    /// Free×free mass matrix `M_free` (feeds mass normalization + participation).
    pub(crate) m_free: SparseRowMat<usize, f64>,
    /// Mesh node count.
    pub(crate) n_nodes: usize,
    /// `true` iff the eigensolver returned all requested modes.
    pub(crate) converged: bool,
    /// Number of eigenpairs the underlying solver reported converged.
    pub(crate) n_converged: usize,
}

/// Core free-vibration FEA eigensolve: build the beam mesh, assemble `K` and the
/// consistent mass `M`, project to the free-DOF subspace, solve
/// `K_free φ = λ M_free φ`, and scatter the mode shapes back to the full DOF
/// space.
///
/// Operates in the free-DOF subspace (extracting `K_free` / `M_free` over the
/// non-Dirichlet DOFs) rather than via row elimination, which would inject
/// spurious unit-diagonal eigenpairs (design_decision #3, mirroring
/// `buckling_kernel`). Homogeneous Dirichlet BCs only; `DirichletBc.value` is
/// ignored.
#[allow(dead_code)]
pub(crate) fn solve_modal_core(
    density: f64,
    material: &IsotropicElastic,
    length: f64,
    width: f64,
    height: f64,
    bcs: &[DirichletBc],
    eigen_opts: &EigenSolverOptions,
) -> ModalCoreResult {
    let mesh = build_beam_mesh(length, width, height);
    let n_nodes = mesh.nodes.len();
    let n_dofs = 3 * n_nodes;

    // ---- Assemble K (P1 element stiffness) --------------------------------
    let k_elems: Vec<ElementStiffness> = mesh
        .tets
        .iter()
        .map(|tet| {
            let phys: [[f64; 3]; 4] =
                [mesh.nodes[tet[0]], mesh.nodes[tet[1]], mesh.nodes[tet[2]], mesh.nodes[tet[3]]];
            element_stiffness(ElementOrder::P1, &phys[..], material)
        })
        .collect();
    let k_assembly: Vec<AssemblyElement<'_>> = mesh
        .tets
        .iter()
        .zip(k_elems.iter())
        .enumerate()
        .map(|(id, (conn, k_e))| AssemblyElement { id, connectivity: conn, k_e })
        .collect();
    let k_full = assemble_global_stiffness(n_nodes, &k_assembly, AssemblyMode::Deterministic);

    // ---- Assemble M (consistent P1 mass) ----------------------------------
    let m_elems: Vec<ElementStiffness> = mesh
        .tets
        .iter()
        .map(|tet| {
            let phys: [[f64; 3]; 4] =
                [mesh.nodes[tet[0]], mesh.nodes[tet[1]], mesh.nodes[tet[2]], mesh.nodes[tet[3]]];
            consistent_element_mass_tet_p1(&phys, density)
        })
        .collect();
    let m_assembly: Vec<AssemblyElement<'_>> = mesh
        .tets
        .iter()
        .zip(m_elems.iter())
        .enumerate()
        .map(|(id, (conn, m_e))| AssemblyElement { id, connectivity: conn, k_e: m_e })
        .collect();
    let m_full = assemble_global_stiffness(n_nodes, &m_assembly, AssemblyMode::Deterministic);

    // ---- Free-DOF subspace map (Dirichlet-only; no MPC) -------------------
    let mut is_constrained = vec![false; n_dofs];
    for bc in bcs {
        if bc.dof < n_dofs {
            is_constrained[bc.dof] = true;
        }
    }
    let mut free_of_full = vec![usize::MAX; n_dofs]; // full DOF → free index
    let mut full_of_free: Vec<usize> = Vec::new(); // free index → full DOF
    for (g, &constrained) in is_constrained.iter().enumerate() {
        if !constrained {
            free_of_full[g] = full_of_free.len();
            full_of_free.push(g);
        }
    }
    let n_free = full_of_free.len();

    // ---- Extract free×free submatrices ------------------------------------
    let k_free = project_free(&k_full, &free_of_full, n_free);
    let m_free = project_free(&m_full, &free_of_full, n_free);

    // ---- Generalized eigensolve  K_free φ = λ M_free φ --------------------
    let eig = solve_generalized_eigen(&k_free, &m_free, eigen_opts.clone());

    // ---- Convert λ→f and scatter φ_free → φ_full --------------------------
    let n_modes_out = eig.eigenvalues.len();
    let mut frequencies = Vec::with_capacity(n_modes_out);
    let mut eigenvalues = Vec::with_capacity(n_modes_out);
    let mut phi_free = Vec::with_capacity(n_modes_out);
    let mut phi_full = Vec::with_capacity(n_modes_out);
    for i in 0..n_modes_out {
        let lambda = eig.eigenvalues[i];
        eigenvalues.push(lambda);
        frequencies.push(eigenvalue_to_frequency_hz(lambda));

        // Mass-normalize so that φᵀ·M_free·φ = 1 (PRD §7.5): scale the raw
        // eigenvector by 1/√(generalized mass). A degenerate (≤ 0) generalized
        // mass yields a 0.0 scale (the helper's guard) — the mode collapses to
        // zero rather than producing NaN/∞.
        let mut phi_f: Vec<f64> = eig.eigenvectors.col_as_slice(i).to_vec();
        let m_phi = m_matvec(&m_free, &phi_f);
        let generalized_mass: f64 =
            phi_f.iter().zip(m_phi.iter()).map(|(a, b)| a * b).sum();
        let scale = mass_normalization_scale(generalized_mass);
        for x in &mut phi_f {
            *x *= scale;
        }

        let mut phi_u = vec![0.0_f64; n_dofs];
        for (free_i, &g) in full_of_free.iter().enumerate() {
            phi_u[g] = phi_f[free_i];
        }
        phi_free.push(phi_f);
        phi_full.push(phi_u);
    }

    ModalCoreResult {
        frequencies,
        eigenvalues,
        phi_free,
        phi_full,
        m_free,
        n_nodes,
        converged: eig.converged,
        n_converged: eig.n_converged,
    }
}

/// Extract the free×free submatrix of `full` over the non-Dirichlet DOFs.
///
/// `free_of_full[g]` maps full DOF `g` to its free-subspace index, or
/// `usize::MAX` if `g` is constrained. This is the Dirichlet-only specialization
/// of `buckling_kernel`'s `project_with_expansion`: every free DOF expands to
/// itself with weight 1.0 and every constrained DOF to nothing. `faer`'s
/// `try_new_from_triplets` sums duplicate triplets, preserving CSR invariants.
#[allow(dead_code)]
fn project_free(
    full: &SparseRowMat<usize, f64>,
    free_of_full: &[usize],
    n_free: usize,
) -> SparseRowMat<usize, f64> {
    let sym = full.symbolic();
    let mut trips: Vec<Triplet<usize, usize, f64>> = Vec::new();
    for g_row in 0..full.nrows() {
        let r = free_of_full[g_row];
        if r == usize::MAX {
            continue;
        }
        let cols = sym.col_idx_of_row_raw(g_row);
        let vals = full.val_of_row(g_row);
        for (col_raw, &val) in cols.iter().zip(vals.iter()) {
            let c = free_of_full[*col_raw];
            if c == usize::MAX || val == 0.0 {
                continue;
            }
            trips.push(Triplet::new(r, c, val));
        }
    }
    SparseRowMat::try_new_from_triplets(n_free, n_free, &trips)
        .expect("free-DOF submatrix construction must not violate CSR invariants")
}

/// Sparse matvec `M · v` over the free×free mass matrix (CSR row dot products).
///
/// The reusable mass-metric primitive: the generalized mass `φᵀMφ` (step 6
/// normalization) and the participation factor `φᵀMd` (step 8) are both
/// `dot(·, M··)`.
#[allow(dead_code)]
fn m_matvec(m: &SparseRowMat<usize, f64>, v: &[f64]) -> Vec<f64> {
    let sym = m.symbolic();
    let mut out = vec![0.0_f64; m.nrows()];
    for (r, out_r) in out.iter_mut().enumerate() {
        let cols = sym.col_idx_of_row_raw(r);
        let vals = m.val_of_row(r);
        let mut acc = 0.0_f64;
        for (col_raw, &val) in cols.iter().zip(vals.iter()) {
            acc += val * v[*col_raw];
        }
        *out_r = acc;
    }
    out
}

/// Solve the generalized symmetric eigenproblem `K_free φ = λ M_free φ`,
/// returning eigenvalues ascending by |λ| with column-major eigenvectors.
///
/// Dispatches to the dense path directly in the small regime instead of always
/// going through [`solve_eigen_shift_invert`], which unconditionally
/// Cholesky-factors `K` up front and would panic on a singular / near-singular
/// `K_free` (e.g. an unconstrained fixture's rigid-body modes). The dense-regime
/// predicate `n ≤ max(64, 2·n_modes)` mirrors the wrapper's own internal
/// dense-fallback threshold, so the numerical path is identical to what the
/// wrapper would pick — minus the premature factorization. Larger constrained
/// problems (`K_free` SPD after BCs) take the shift-invert Lanczos path
/// (design_decision #4).
#[allow(dead_code)]
fn solve_generalized_eigen(
    k_free: &SparseRowMat<usize, f64>,
    m_free: &SparseRowMat<usize, f64>,
    opts: EigenSolverOptions,
) -> EigenSolverResult {
    let n = k_free.nrows();
    if n <= 64_usize.max(2 * opts.n_modes) {
        solve_eigen_dense(k_free, m_free, opts)
    } else {
        solve_eigen_shift_invert(k_free, m_free, opts)
    }
}

#[cfg(test)]
mod tests {
    use faer::sparse::SparseRowMat;
    use reify_solver_elastic::{DirichletBc, EigenSolverOptions, IsotropicElastic};

    use super::{ModalCoreResult, build_beam_mesh, solve_modal_core};

    /// `aᵀ · M · b` for the free×free mass matrix `M` (sparse CSR row matvec then
    /// dot). Test-local invariant probe; the production normalization path
    /// computes the same generalized mass via its own helper in step 6.
    fn m_quadratic_form(m: &SparseRowMat<usize, f64>, a: &[f64], b: &[f64]) -> f64 {
        let sym = m.symbolic();
        let mut acc = 0.0_f64;
        for (r, &a_r) in a.iter().enumerate() {
            let cols = sym.col_idx_of_row_raw(r);
            let vals = m.val_of_row(r);
            let mut mb_r = 0.0_f64;
            for (col_raw, &v) in cols.iter().zip(vals.iter()) {
                mb_r += v * b[*col_raw];
            }
            acc += a_r * mb_r;
        }
        acc
    }

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

    /// step-5 (RED → GREEN in step-6): mass-normalization invariant.
    ///
    /// On the same coarse root-clamped fixture, after normalization each mode
    /// must have unit M-generalized mass `φ_free_iᵀ·M_free·φ_free_i = 1` (sound
    /// by construction: φ is divided by √(generalized mass) — pinned at 1e-12),
    /// and distinct modes must be M-orthogonal `φ_iᵀ·M_free·φ_j ≈ 0` (looser
    /// 1e-8: depends on the solver's orthogonalization, not a by-construction
    /// identity). RED: the raw eigenvectors carry arbitrary scale, so the
    /// diagonal generalized mass is not 1.
    #[test]
    fn solve_modal_core_modes_are_mass_normalized() {
        let length = 0.02_f64;
        let width = 0.05_f64;
        let height = 0.1_f64;

        let mesh = build_beam_mesh(length, width, height);
        let (bcs, _constrained) = clamp_x_min_face(&mesh.nodes);
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

        assert!(!result.phi_free.is_empty(), "expect at least 1 mode");

        // (a) Diagonal: unit M-generalized mass (by construction, 1e-12).
        for (i, phi_i) in result.phi_free.iter().enumerate() {
            let m_ii = m_quadratic_form(&result.m_free, phi_i, phi_i);
            assert!(
                (m_ii - 1.0).abs() < 1e-12,
                "mode {i}: φᵀMφ = {m_ii}, expected 1.0 within 1e-12",
            );
        }

        // (b) Off-diagonal: cross-mode M-orthogonality (solver-dependent, 1e-8).
        for i in 0..result.phi_free.len() {
            for j in (i + 1)..result.phi_free.len() {
                let m_ij =
                    m_quadratic_form(&result.m_free, &result.phi_free[i], &result.phi_free[j]);
                assert!(
                    m_ij.abs() < 1e-8,
                    "modes {i},{j}: φ_iᵀMφ_j = {m_ij}, expected ≈ 0 within 1e-8",
                );
            }
        }
    }
}
