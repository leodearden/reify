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

use std::f64::consts::PI;

use faer::sparse::{SparseRowMat, Triplet};

use reify_core::Diagnostic;
use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, DirichletBc, EigenSolverOptions, EigenSolverResult, ElementOrder,
    ElementStiffness, IsotropicElastic, assemble_global_stiffness, consistent_element_mass_tet_p1,
    element_stiffness, solve_eigen_dense, solve_eigen_shift_invert,
};
use reify_stdlib::modal::free_vibration::{
    eigenvalue_to_frequency_hz, is_rigid_body_mode, mass_normalization_scale,
    modal_participation_mass,
};

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
/// normalization + participation mass (steps 5–8); `participation_mass` is the
/// per-mode effective mass along the reference direction (step 8);
/// `converged` / `n_converged` feed the convergence diagnostics (steps 9–10) and
/// the trampoline outcome (step 14). `#[allow(dead_code)]` covers the
/// not-yet-read fields during that staged build-up.
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
    /// Effective modal participation mass `m_eff,i = (φ_iᵀ·M_free·d_free)²`
    /// along the reference direction (φ mass-normalized), one per mode. Summed
    /// over a complete basis it equals the total translational mass along the
    /// reference direction (the completeness identity, PRD §4.1/§4.3).
    pub(crate) participation_mass: Vec<f64>,
    /// Free×free mass matrix `M_free` (feeds mass normalization + participation).
    pub(crate) m_free: SparseRowMat<usize, f64>,
    /// Mesh node count.
    pub(crate) n_nodes: usize,
    /// `true` iff the eigensolver returned all requested modes.
    pub(crate) converged: bool,
    /// Number of eigenpairs the underlying solver reported converged.
    pub(crate) n_converged: usize,
    /// Non-fatal diagnostics surfaced by the solve: `W_ModalRigidBodyMode` (a
    /// near-zero / rigid-body mode → possible under-constraint) and
    /// `W_ModalConvergence` (fewer modes converged than requested). Message-
    /// based (`code: None`) per design_decision #6; the trampoline forwards
    /// these into the `ComputeOutcome` (step 14).
    pub(crate) diagnostics: Vec<Diagnostic>,
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
///
/// `reference_direction` is the (unit) direction along which the per-mode
/// effective participation mass `m_eff,i = (φ_iᵀ·M_free·d_free)²` is computed;
/// it is broadcast to every free node's three translational DOFs to form
/// `d_free` (the caller is responsible for supplying a unit vector — see the
/// trampoline). It does not affect the eigensolve, only the participation field.
// 8 args: density + material + 3 geometry scalars + reference_direction + bcs +
// eigen_opts — the flat physical-input surface mirrors solve_cantilever_fea /
// solve_buckling_kernel; bundling them into a struct would just relocate the
// fan-out. (dead_code until the step-14 trampoline consumes this.)
#[allow(dead_code, clippy::too_many_arguments)]
pub(crate) fn solve_modal_core(
    density: f64,
    material: &IsotropicElastic,
    length: f64,
    width: f64,
    height: f64,
    reference_direction: [f64; 3],
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

    // ---- Participation metric  md = M_free · d_free -----------------------
    // d_free broadcasts the reference direction to every free node's three
    // translational DOFs (axis = full DOF index mod 3). Precomputing
    // md = M_free·d_free once lets the per-mode participation factor be a single
    // dot product p_i = φ_iᵀ·M_free·d_free = φ_i·md (M_free symmetric).
    let d_free: Vec<f64> =
        full_of_free.iter().map(|&g| reference_direction[g % 3]).collect();
    let md = m_matvec(&m_free, &d_free);

    // ---- Generalized eigensolve  K_free φ = λ M_free φ --------------------
    let eig = solve_generalized_eigen(&k_free, &m_free, eigen_opts.clone());

    // ---- Convert λ→f and scatter φ_free → φ_full --------------------------
    let n_modes_out = eig.eigenvalues.len();
    let mut frequencies = Vec::with_capacity(n_modes_out);
    let mut eigenvalues = Vec::with_capacity(n_modes_out);
    let mut phi_free = Vec::with_capacity(n_modes_out);
    let mut phi_full = Vec::with_capacity(n_modes_out);
    let mut participation_mass = Vec::with_capacity(n_modes_out);
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

        // Effective participation mass along the reference direction (φ now
        // mass-normalized): factor p_i = φ_iᵀ·M_free·d_free = φ_i·md, then
        // m_eff,i = p_i² (PRD §4.1/§4.3). Summed over a complete basis this
        // equals the total translational mass along d (completeness identity).
        let p_i: f64 = phi_f.iter().zip(md.iter()).map(|(a, b)| a * b).sum();
        participation_mass.push(modal_participation_mass(p_i));

        let mut phi_u = vec![0.0_f64; n_dofs];
        for (free_i, &g) in full_of_free.iter().enumerate() {
            phi_u[g] = phi_f[free_i];
        }
        phi_free.push(phi_f);
        phi_full.push(phi_u);
    }

    // ---- Diagnostics (message-based, code: None; design_decision #6) ------
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // Rigid-body / spurious near-zero modes: ω ≈ 0 signals an under-constrained
    // model. RIGID_BODY_OMEGA_TOL sits in the wide gap between rigid modes
    // (ω → 0) and the lowest flexible angular frequency of any realistic stiff
    // metal part (≫ 1 rad/s ≈ 0.16 Hz) — see step-9's measured spectrum.
    const RIGID_BODY_OMEGA_TOL: f64 = 1.0; // rad/s
    for (i, &f) in frequencies.iter().enumerate() {
        let omega = 2.0 * PI * f;
        if is_rigid_body_mode(omega, RIGID_BODY_OMEGA_TOL) {
            diagnostics.push(Diagnostic::warning(format!(
                "W_ModalRigidBodyMode: mode {i} has near-zero angular frequency \
                 ω = {omega:.3e} rad/s (≤ {RIGID_BODY_OMEGA_TOL:.1e}); the model \
                 may be under-constrained (rigid-body or spurious mode)."
            )));
        }
    }

    // Convergence shortfall: `eig.converged` is false iff fewer modes were
    // returned than requested (holds for both the dense and shift-invert paths).
    if !eig.converged {
        diagnostics.push(Diagnostic::warning(format!(
            "W_ModalConvergence: eigensolver returned {} of {} requested modes; \
             the result is partial (raise max_iters/tol or lower n_modes).",
            n_modes_out, eigen_opts.n_modes,
        )));
    }

    ModalCoreResult {
        frequencies,
        eigenvalues,
        phi_free,
        phi_full,
        participation_mass,
        m_free,
        n_nodes,
        converged: eig.converged,
        n_converged: eig.n_converged,
        diagnostics,
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
    use reify_core::{DimensionVector, Severity};
    use reify_ir::{StructureInstanceData, StructureTypeId, Value};
    use reify_solver_elastic::{DirichletBc, EigenSolverOptions, IsotropicElastic};
    use reify_stdlib::modal::free_vibration::is_rigid_body_mode;

    use super::{ModalCoreResult, build_beam_mesh, extract_density_or_degenerate, solve_modal_core};
    use crate::ComputeOutcome;

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
            [0.0, 0.0, 1.0], // reference_direction; unused by this assertion
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
            [0.0, 0.0, 1.0], // reference_direction; unused by this assertion
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

    /// step-7 (RED → GREEN in step-8): participation-mass completeness identity.
    ///
    /// On the coarse root-clamped fixture solved for the FULL spectrum
    /// (`n_modes = n_free`, dense path), the per-mode effective masses must
    /// satisfy the Parseval/completeness identity for the reference direction
    /// `d`:
    ///
    /// ```text
    /// Σ_i (φ_free_iᵀ·M_free·d_free)²  =  d_freeᵀ·M_free·d_free
    /// ```
    ///
    /// i.e. `Σ_i participation_mass[i]` equals the total translational mass of
    /// the free DOFs along `d` — EXACTLY, because a complete M-orthonormal basis
    /// resolves the identity `Σ_i φ_i φ_iᵀ M = I`. Each φ is mass-normalized
    /// (φᵀMφ = 1, step 6) and the clamped fixture's eigenvalues are distinct, so
    /// the eigenvectors are mutually M-orthogonal and the basis is M-orthonormal.
    /// This pins the participation computation and the normalization together
    /// with a deterministic, by-construction-exact assertion (design_decision
    /// #5; avoids the fuzzy "≥99% capture" bound).
    ///
    /// RED: `participation_mass` (and the `reference_direction` parameter) are
    /// absent until step 8.
    #[test]
    fn solve_modal_core_participation_mass_satisfies_completeness() {
        let length = 0.02_f64;
        let width = 0.05_f64;
        let height = 0.1_f64;
        // Bending (Z) direction; a unit vector so the identity's RHS is the
        // exact total Z-translational mass of the free DOFs.
        let reference_direction = [0.0_f64, 0.0, 1.0];

        let mesh = build_beam_mesh(length, width, height);
        let (bcs, constrained_dofs) = clamp_x_min_face(&mesh.nodes);
        let n_dofs = 3 * mesh.nodes.len();
        let n_free = n_dofs - constrained_dofs.len();
        assert!(n_free > 0, "fixture must leave at least one free DOF");

        // Full spectrum: request every free mode so {φ_i} is a complete basis.
        let eigen_opts =
            EigenSolverOptions { n_modes: n_free, tol: 1e-8, max_iters: 200, sigma: 0.0 };

        let result: ModalCoreResult = solve_modal_core(
            STEEL_DENSITY,
            &steel(),
            length,
            width,
            height,
            reference_direction,
            &bcs,
            &eigen_opts,
        );

        // Precondition: the dense path returned the entire free spectrum (so the
        // completeness sum is over a complete basis, not a truncated one).
        assert_eq!(
            result.participation_mass.len(),
            n_free,
            "full-spectrum solve must return n_free = {n_free} effective masses",
        );

        // Rebuild d_free (the reference direction broadcast to each free node's
        // translational DOFs; axis = full DOF index mod 3) from the same clamp,
        // independently of the solver's internal map.
        let mut is_constrained = vec![false; n_dofs];
        for &g in &constrained_dofs {
            is_constrained[g] = true;
        }
        let full_of_free: Vec<usize> =
            (0..n_dofs).filter(|&g| !is_constrained[g]).collect();
        let d_free: Vec<f64> =
            full_of_free.iter().map(|&g| reference_direction[g % 3]).collect();

        // RHS: total translational mass of the free DOFs along d.
        let total_mass = m_quadratic_form(&result.m_free, &d_free, &d_free);
        assert!(total_mass > 0.0, "reference-direction mass must be positive");

        // LHS: Σ_i participation_mass[i] = Σ_i (φ_iᵀ M d)².
        let captured: f64 = result.participation_mass.iter().sum();

        assert!(
            (captured - total_mass).abs() < 1e-9,
            "completeness identity: Σ participation = {captured}, total mass = \
             {total_mass}, |Δ| = {} exceeds 1e-9",
            (captured - total_mass).abs(),
        );
    }

    /// step-9 (RED → GREEN in step-10): rigid-body-mode diagnostic.
    ///
    /// An UNCONSTRAINED fixture (empty BCs) admits the six rigid-body modes of a
    /// free 3-D body (ω ≈ 0). The dense generalized path handles the singular
    /// `K_free` (no up-front Cholesky), so requesting `n_modes = n_free/2`
    /// (≥ 42, forcing the dense regime for this `n_free = 84` mesh) returns them
    /// as the lowest modes. `solve_modal_core` must (a) return ≥ 1 mode with
    /// ω ≈ 0 and (b) surface a `Warning` diagnostic whose message starts
    /// `"W_ModalRigidBodyMode"`.
    ///
    /// The near-zero tolerance (1.0 rad/s ≈ 0.16 Hz) sits in the measured
    /// 7-decade gap between the rigid modes (ω ≤ ~1e-2 rad/s) and the first
    /// flexible mode (ω ≥ ~1e5 rad/s). RED: the `diagnostics` field is absent.
    #[test]
    fn solve_modal_core_flags_rigid_body_modes_when_unconstrained() {
        let length = 0.02_f64;
        let width = 0.05_f64;
        let height = 0.1_f64;

        let mesh = build_beam_mesh(length, width, height);
        let n_free = 3 * mesh.nodes.len(); // empty BCs → all DOFs free
        // n_modes ≥ n_free/2 forces solve_generalized_eigen's dense regime
        // (n ≤ max(64, 2·n_modes)), avoiding the shift-invert Cholesky panic on
        // the singular (rigid-body) K_free.
        let eigen_opts = EigenSolverOptions {
            n_modes: n_free / 2,
            tol: 1e-8,
            max_iters: 200,
            sigma: 0.0,
        };

        let result: ModalCoreResult = solve_modal_core(
            STEEL_DENSITY,
            &steel(),
            length,
            width,
            height,
            [0.0, 0.0, 1.0],
            &[], // unconstrained
            &eigen_opts,
        );

        // (a) at least one returned mode is a rigid-body mode (ω ≈ 0).
        let omega = |f: f64| 2.0 * std::f64::consts::PI * f;
        let rigid_count = result
            .frequencies
            .iter()
            .filter(|&&f| is_rigid_body_mode(omega(f), 1.0))
            .count();
        assert!(
            rigid_count >= 1,
            "unconstrained body must expose ≥1 rigid-body mode (ω≈0); got {rigid_count}",
        );

        // (b) a W_ModalRigidBodyMode Warning is surfaced.
        let has_rigid_warning = result.diagnostics.iter().any(|d| {
            d.severity == Severity::Warning && d.message.starts_with("W_ModalRigidBodyMode")
        });
        assert!(
            has_rigid_warning,
            "expected a Warning starting \"W_ModalRigidBodyMode\"; got {:?}",
            result.diagnostics,
        );
    }

    /// step-9 (RED → GREEN in step-10): convergence-shortfall diagnostic.
    ///
    /// Requesting more modes than the free-DOF count can yield (`n_modes` ≫
    /// `n_free` on the clamped fixture) makes the eigensolver return fewer modes
    /// than requested (`converged == false`). `solve_modal_core` must surface a
    /// `Warning` diagnostic whose message starts `"W_ModalConvergence"`. The
    /// clamped fixture has no rigid-body modes, isolating this signal. RED: the
    /// `diagnostics` field is absent.
    #[test]
    fn solve_modal_core_flags_convergence_shortfall_when_over_requested() {
        let length = 0.02_f64;
        let width = 0.05_f64;
        let height = 0.1_f64;

        let mesh = build_beam_mesh(length, width, height);
        let (bcs, constrained) = clamp_x_min_face(&mesh.nodes);
        let n_free = 3 * mesh.nodes.len() - constrained.len();

        // Request far more modes than exist → the dense path returns only n_free.
        let eigen_opts = EigenSolverOptions {
            n_modes: n_free + 64,
            tol: 1e-8,
            max_iters: 200,
            sigma: 0.0,
        };

        let result: ModalCoreResult = solve_modal_core(
            STEEL_DENSITY,
            &steel(),
            length,
            width,
            height,
            [0.0, 0.0, 1.0],
            &bcs,
            &eigen_opts,
        );

        assert!(
            result.frequencies.len() < eigen_opts.n_modes,
            "fixture must return fewer modes than requested to trigger the warning",
        );

        let has_conv_warning = result.diagnostics.iter().any(|d| {
            d.severity == Severity::Warning && d.message.starts_with("W_ModalConvergence")
        });
        assert!(
            has_conv_warning,
            "expected a Warning starting \"W_ModalConvergence\"; got {:?}",
            result.diagnostics,
        );
    }

    /// Build a minimal `ElasticMaterial`-shaped `Value::StructureInstance` with
    /// the usual elastic fields, optionally carrying a `density` scalar. Mirrors
    /// the runtime material shape the trampoline reads (cf. buckling's
    /// `extract_material`): `youngs_modulus : Scalar(PRESSURE)`,
    /// `poisson_ratio : Real`, and (when `Some`) `density : Scalar(MASS_DENSITY)`.
    fn material_with_density(density: Option<f64>) -> Value {
        let mut fields: Vec<(String, Value)> = vec![
            (
                "youngs_modulus".to_string(),
                Value::Scalar { si_value: 205e9, dimension: DimensionVector::PRESSURE },
            ),
            ("poisson_ratio".to_string(), Value::Real(0.29)),
        ];
        if let Some(d) = density {
            fields.push((
                "density".to_string(),
                Value::Scalar { si_value: d, dimension: DimensionVector::MASS_DENSITY },
            ));
        }
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "ElasticMaterial".to_string(),
            version: 1,
            fields: fields.into_iter().collect(),
        }))
    }

    /// Assert the density-guard short-circuit: the returned outcome is a
    /// `Completed` carrying (a) an `Error` diagnostic whose message starts
    /// `"E_ModalNoMassMatrix"` and (b) a degenerate `ModalResult` whose `modes`
    /// list is empty (no eigenproblem was solved). No panic on any path.
    fn assert_no_mass_degenerate(outcome: ComputeOutcome) {
        let ComputeOutcome::Completed { result, diagnostics, .. } = outcome else {
            panic!("expected a Completed degenerate outcome, got a non-Completed variant");
        };

        // (a) an Error diagnostic identifies the no-mass-matrix condition.
        let has_err = diagnostics.iter().any(|d| {
            d.severity == Severity::Error && d.message.starts_with("E_ModalNoMassMatrix")
        });
        assert!(
            has_err,
            "expected an Error starting \"E_ModalNoMassMatrix\"; got {diagnostics:?}",
        );

        // (b) the result is a degenerate ModalResult with an empty modes list.
        let data = match &result {
            Value::StructureInstance(d) => d,
            other => panic!("expected a ModalResult StructureInstance, got {other:?}"),
        };
        assert_eq!(
            data.type_name, "ModalResult",
            "degenerate result must be a ModalResult, got {}",
            data.type_name,
        );
        match data.fields.get(&"modes".to_string()) {
            Some(Value::List(modes)) => assert!(
                modes.is_empty(),
                "degenerate ModalResult.modes must be empty; got {} modes",
                modes.len(),
            ),
            other => {
                panic!("expected ModalResult.modes to be an (empty) Value::List, got {other:?}")
            }
        }
    }

    /// step-11 (RED → GREEN in step-12): no-mass-matrix density guard at the
    /// trampoline boundary.
    ///
    /// The consistent mass matrix `M` cannot be assembled without a positive
    /// mass density, and `Kφ = λMφ` is meaningless with no `M`. So the
    /// trampoline's density-extraction entry must short-circuit — emit an
    /// `E_ModalNoMassMatrix` Error and a degenerate empty-modes `ModalResult` —
    /// when the material carries no usable `density` (missing or ≤ 0), rather
    /// than panicking or assembling/eigensolving. A positive density passes the
    /// guard and yields `Ok(density)` (PRD diagnostics; design_decision #6:
    /// message-based, `code: None`).
    ///
    /// RED: `extract_density_or_degenerate` is absent until step 12.
    #[test]
    fn trampoline_density_guard_flags_missing_or_nonpositive_density() {
        // (a) missing `density` field → degenerate + E_ModalNoMassMatrix.
        match extract_density_or_degenerate(&material_with_density(None)) {
            Err(outcome) => assert_no_mass_degenerate(outcome),
            Ok(d) => panic!("missing density must short-circuit; got Ok({d})"),
        }

        // (b) zero density → degenerate (≤ 0 fails the guard).
        match extract_density_or_degenerate(&material_with_density(Some(0.0))) {
            Err(outcome) => assert_no_mass_degenerate(outcome),
            Ok(d) => panic!("zero density must short-circuit; got Ok({d})"),
        }

        // (c) negative density → degenerate.
        match extract_density_or_degenerate(&material_with_density(Some(-1.0))) {
            Err(outcome) => assert_no_mass_degenerate(outcome),
            Ok(d) => panic!("negative density must short-circuit; got Ok({d})"),
        }

        // (d) positive density → Ok(density), no short-circuit.
        match extract_density_or_degenerate(&material_with_density(Some(7850.0))) {
            Ok(got) => assert!(
                (got - 7850.0).abs() < 1e-9,
                "positive density must pass through unchanged; got {got}",
            ),
            Err(_) => panic!("positive density must pass the guard"),
        }
    }
}
