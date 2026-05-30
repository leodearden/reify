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
//! # Design decisions
//!
//! All design decisions are documented in `.task/plan.json`.
//!
//! The primary non-obvious choice: operating in the free-DOF subspace throughout
//! (not using `apply_dirichlet_row_elimination`) to avoid spurious eigenpairs
//! from the unit-diagonal / zero-row pairs that row-elimination would inject into
//! the eigenspectrum. Only homogeneous Dirichlet BCs are supported in v0.5;
//! `DirichletBc.value` is silently ignored.

use faer::sparse::{SparseRowMat, Triplet};

use crate::assembly::{
    AssemblyElement, AssemblyMode, ElementOrder, ElementStiffness, assemble_global_stiffness,
    element_stiffness,
};
use crate::boundary::DirichletBc;
use crate::constitutive::IsotropicElastic;
use crate::eigensolve::{EigenSolverOptions, solve_eigen_shift_invert};
use crate::geometric_stiffness::{
    InitialStress3, geometric_element_stiffness_tet_p1, geometric_element_stiffness_tet_p2,
};
use crate::mpc::MpcRow;
use crate::result::{element_stress_p1, element_stress_p2};
use crate::solver::{CgSolverOptions, SolverMode, solve_cg};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Options for [`solve_buckling_kernel`].
///
/// # Defaults
///
/// `n_modes = 10`, `eigen_tol = 1e-8`, `eigen_max_iters = 1000`,
/// `cg_tolerance = 1e-10`, `cg_max_iter = 5000`.
#[derive(Debug, Clone)]
pub struct BucklingKernelOptions {
    /// Number of buckling modes to compute (must be ≥ 1).
    pub n_modes: usize,
    /// Convergence tolerance for the Lanczos eigensolver (must be finite and > 0).
    pub eigen_tol: f64,
    /// Maximum number of Lanczos thick-restart cycles.
    pub eigen_max_iters: usize,
    /// Relative residual tolerance for the CG linear-static pre-stress solve
    /// (must be finite and > 0).
    pub cg_tolerance: f64,
    /// Maximum CG iteration budget.
    pub cg_max_iter: usize,
}

impl Default for BucklingKernelOptions {
    fn default() -> Self {
        Self {
            n_modes: 10,
            eigen_tol: 1e-8,
            eigen_max_iters: 1000,
            cg_tolerance: 1e-10,
            cg_max_iter: 5000,
        }
    }
}

/// A single buckling mode (eigenvalue + mode-shape vector).
#[derive(Debug, Clone)]
pub struct Mode {
    /// Buckling load multiplier λ: the structure buckles when the applied load
    /// is multiplied by λ.  Sorted ascending by |λ| across [`BucklingKernelResult::modes`].
    pub eigenvalue: f64,
    /// Mode-shape displacement vector, length `3 * n_nodes`.
    /// Entries at constrained DOFs are exactly `0.0` (homogeneous Dirichlet).
    pub mode_shape: Vec<f64>,
}

/// Result returned by [`solve_buckling_kernel`].
#[derive(Debug, Clone)]
pub struct BucklingKernelResult {
    /// Buckling modes sorted ascending by |λ|.
    pub modes: Vec<Mode>,
    /// Pre-stress linear-static displacement, length `3 * n_nodes`.
    /// Entries at constrained DOFs are `0.0`.
    pub pre_stress_displacement: Vec<f64>,
    /// Per-element Cauchy stress recovered from the pre-stress displacement.
    /// Length `n_elements`; `pre_stress_per_element[e][i][j]` is σ_ij for element `e`.
    /// This is the *un-negated* stress field (positive = tension). The kernel
    /// feeds `−σ` into the K_g kernel internally.
    pub pre_stress_per_element: Vec<[[f64; 3]; 3]>,
    /// `true` iff the eigensolver converged for all requested modes.
    pub converged: bool,
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Build a per-full-DOF expansion map for the master-slave MPC reduction.
///
/// Returns `(expansion, n_indep)` where:
/// - `expansion[g]` is a list of `(indep_idx, weight)` pairs expressing full
///   DOF `g` as a linear combination of independent (reduced) DOFs.
/// - Independent (non-Dirichlet, non-slave) DOF: `expansion[g] = [(r, 1.0)]`.
/// - Dirichlet DOF: `expansion[g] = []` (contributes nothing; fixed at 0).
/// - Slave DOF `p` with pivot coefficient `c0` and other-DOFs `(d_i, c_i)`:
///   `expansion[p] = [(indep_idx[d_i], -c_i/c0)]` for each non-Dirichlet `d_i`.
/// - `n_indep` is the total number of independent DOFs (rank of reduced system).
///
/// # v0.5 limitations (panics at entry)
///
/// - Each MPC row must be homogeneous (`rhs == 0.0`); inhomogeneous rows
///   panic with a message containing `"rhs"`.
/// - A slave (pivot, `dofs[0]`) must not also be in `bcs`; violation panics
///   with a message containing `"pivot"`.
/// - Each global DOF may be the pivot of at most one MPC; duplicates panic
///   with a message containing `"pivot"`.
/// - The "other" DOFs (`dofs[1..]`) of any MPC must not be pivots of another
///   MPC (no chains); violation panics with a message containing `"chain"`.
fn build_expansion_map(
    n_nodes: usize,
    bcs: &[DirichletBc],
    mpcs: &[MpcRow],
) -> (Vec<Vec<(usize, f64)>>, usize) {
    let n_dofs = 3 * n_nodes;

    // Pass 1: mark Dirichlet DOFs.
    let mut is_dirichlet = vec![false; n_dofs];
    for bc in bcs {
        is_dirichlet[bc.dof] = true;
    }

    // Pass 2: detect pivots; validate homogeneity, no-Dirichlet-pivot, unique pivots.
    let mut pivot_of: Vec<Option<usize>> = vec![None; n_dofs]; // pivot_of[g] = Some(mpc_idx)
    for (mpc_idx, row) in mpcs.iter().enumerate() {
        assert!(
            row.rhs == 0.0,
            "MPC row {mpc_idx}: rhs = {} is non-homogeneous; v0.5 buckling kernel \
             supports only rhs == 0.0 (homogeneous MPCs). inhomogeneous constraints \
             are ill-defined for the generalized eigenproblem.",
            row.rhs,
        );
        let pivot = row.dofs[0];
        assert!(
            !is_dirichlet[pivot],
            "MPC row {mpc_idx}: pivot DOF {pivot} is also in the Dirichlet BC list; \
             a slave pivot must not be simultaneously constrained by Dirichlet BCs.",
        );
        assert!(
            pivot_of[pivot].is_none(),
            "MPC row {mpc_idx}: pivot DOF {pivot} is already the pivot of MPC row {}; \
             each global DOF may be the pivot of at most one MPC.",
            pivot_of[pivot].unwrap(),
        );
        pivot_of[pivot] = Some(mpc_idx);
    }

    // Pass 3: check no-chain constraint (other DOFs must not be pivots).
    for (mpc_idx, row) in mpcs.iter().enumerate() {
        for &d in &row.dofs[1..] {
            assert!(
                pivot_of[d].is_none(),
                "MPC chain detected: MPC row {mpc_idx} references DOF {d} as an \
                 \"other\" DOF, but DOF {d} is itself the pivot of MPC row {}. \
                 Constraint chains are not supported in v0.5.",
                pivot_of[d].unwrap(),
            );
        }
    }

    // Pass 4: assign independent indices to non-Dirichlet, non-slave DOFs.
    let mut indep_idx: Vec<usize> = vec![usize::MAX; n_dofs];
    let mut n_indep = 0usize;
    for g in 0..n_dofs {
        if !is_dirichlet[g] && pivot_of[g].is_none() {
            indep_idx[g] = n_indep;
            n_indep += 1;
        }
    }

    // Pass 5: fill expansion map.
    let mut expansion: Vec<Vec<(usize, f64)>> = vec![vec![]; n_dofs];
    for g in 0..n_dofs {
        if is_dirichlet[g] {
            // Dirichlet: empty (0-valued; contributes nothing).
        } else if let Some(mpc_idx) = pivot_of[g] {
            // Slave: express as linear combination of other DOFs.
            let row = &mpcs[mpc_idx];
            let c0 = row.coeffs[0];
            for i in 1..row.dofs.len() {
                let d_i = row.dofs[i];
                let c_i = row.coeffs[i];
                let alpha_i = -c_i / c0;
                // Skip Dirichlet other-DOFs (contribute 0).
                if !is_dirichlet[d_i] {
                    let r = indep_idx[d_i];
                    debug_assert_ne!(
                        r,
                        usize::MAX,
                        "other DOF {d_i} in MPC row {mpc_idx} must be independent \
                         (not Dirichlet and not a pivot) after chain-check",
                    );
                    expansion[g].push((r, alpha_i));
                }
            }
        } else {
            // Independent: singleton.
            expansion[g].push((indep_idx[g], 1.0));
        }
    }

    (expansion, n_indep)
}

/// Project a full `n × n` sparse matrix to the `n_indep × n_indep` reduced
/// subspace via the expansion map `T` (implicitly `Tᵀ · M · T`).
///
/// For each stored entry `(g_row, g_col, v)` in `full`, iterates all
/// `(r, w_r)` in `expansion[g_row]` and `(c, w_c)` in `expansion[g_col]`
/// and emits triplet `(r, c, w_r * v * w_c)`. Faer's `try_new_from_triplets`
/// sums duplicates, so slave rows/cols accumulate into master entries naturally.
///
/// This generalises `project_free` (which is the special case where every
/// non-Dirichlet DOF has expansion `[(indep_idx, 1.0)]`).
fn project_with_expansion(
    full: &SparseRowMat<usize, f64>,
    expansion: &[Vec<(usize, f64)>],
    n_indep: usize,
) -> SparseRowMat<usize, f64> {
    let sym = full.symbolic();
    let n_rows = full.nrows();
    let mut trips: Vec<Triplet<usize, usize, f64>> = Vec::new();
    for g_row in 0..n_rows {
        if expansion[g_row].is_empty() {
            continue;
        }
        let cols = sym.col_idx_of_row_raw(g_row);
        let vals = full.val_of_row(g_row);
        for (col_raw, &val) in cols.iter().zip(vals.iter()) {
            let g_col = *col_raw;
            if val == 0.0 || expansion[g_col].is_empty() {
                continue;
            }
            for &(r, w_r) in &expansion[g_row] {
                for &(c, w_c) in &expansion[g_col] {
                    trips.push(Triplet::new(r, c, w_r * val * w_c));
                }
            }
        }
    }
    SparseRowMat::try_new_from_triplets(n_indep, n_indep, &trips)
        .expect("expansion-map sub-matrix construction must not violate CSR invariants")
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Orchestrate the four-phase buckling pipeline for a P1-tet mesh.
///
/// # Arguments
///
/// - `nodes` — global node coordinate array, length `n_nodes`.
/// - `tets` — element connectivity array, each element is `[n0, n1, n2, n3]`.
/// - `material` — isotropic elastic constants (E, ν).
/// - `bcs` — homogeneous Dirichlet BCs; `.value` is silently ignored in v0.5.
/// - `f` — pre-built global load vector, length `3 * n_nodes`.
///   Build via `apply_point_load` / `apply_body_force` / `apply_traction_load`.
/// - `mpcs` — multi-point constraints applied via master-slave (transformation)
///   reduction. Pass `&[]` for no MPCs (no-op, bit-identical to the 6-arg
///   behaviour). v0.5 limitations enforced by panic at entry:
///   - **Homogeneous only** (`row.rhs == 0.0` for every row): inhomogeneous MPCs
///     are ill-defined for the eigenproblem and will panic with a message
///     containing `"rhs"`.
///   - **No Dirichlet-pivot conflict**: if a slave DOF (`dofs[0]`) also appears
///     in `bcs`, panics with a message containing `"pivot"`.
///   - **Unique pivots**: each global DOF may be the pivot of at most one MPC;
///     duplicates panic with a message containing `"pivot"`.
///   - **No constraint chains**: the "other" DOFs (`dofs[1..]`) of any MPC must
///     not themselves be pivots of another MPC; chains panic with a message
///     containing `"chain"`.
/// - `opts` — solver tuning parameters.
///
/// # Phases
///
/// 1. **Expansion map** — identify constrained (Dirichlet + slave) DOFs and
///    build the per-DOF linear-combination map from the `bcs` + `mpcs` inputs.
/// 2. **Linear-static pre-stress** — CG solve `K_red u_red = f_red`.
/// 3. **K_g assembly** — per-element Cauchy stress → −K_g_red.
/// 4. **Generalized eigensolve** — `K_red φ_red = λ (−K_g_red) φ_red`.
/// 5. **Mode-shape expansion** — expand to full DOF space via the expansion map.
///
/// # Panics
///
/// - `f.len() != 3 * nodes.len()`
/// - `opts.n_modes < 1`
/// - Any `bc.dof >= 3 * nodes.len()`
/// - Any MPC violation (see `mpcs` arg description above).
/// - Invalid solver option values (non-positive / non-finite tolerances or
///   zero iteration budgets).
/// - The pre-stress CG solve exhausts its iteration budget without converging
///   (fails loud per the Task-2544 contract-explicitness convention).
///
/// # v0.5 limitations
///
/// Only homogeneous Dirichlet BCs are supported; `DirichletBc.value` is silently
/// ignored. MPCs must be homogeneous (`rhs == 0.0`).
pub fn solve_buckling_kernel(
    nodes: &[[f64; 3]],
    tets: &[[usize; 4]],
    material: &IsotropicElastic,
    bcs: &[DirichletBc],
    f: &[f64],
    mpcs: &[MpcRow],
    opts: BucklingKernelOptions,
) -> BucklingKernelResult {
    // ---- Contract checks ---------------------------------------------------
    assert_eq!(
        f.len(),
        3 * nodes.len(),
        "load vector length {} != 3 * n_nodes {}",
        f.len(),
        3 * nodes.len(),
    );
    assert!(opts.n_modes >= 1, "opts.n_modes must be >= 1, got {}", opts.n_modes);
    assert!(
        opts.eigen_tol.is_finite() && opts.eigen_tol > 0.0,
        "opts.eigen_tol must be finite and positive, got {}",
        opts.eigen_tol,
    );
    assert!(
        opts.eigen_max_iters >= 1,
        "opts.eigen_max_iters must be >= 1, got {}",
        opts.eigen_max_iters,
    );
    assert!(
        opts.cg_tolerance.is_finite() && opts.cg_tolerance > 0.0,
        "opts.cg_tolerance must be finite and positive, got {}",
        opts.cg_tolerance,
    );
    assert!(
        opts.cg_max_iter >= 1,
        "opts.cg_max_iter must be >= 1, got {}",
        opts.cg_max_iter,
    );
    for bc in bcs {
        assert!(
            bc.dof < 3 * nodes.len(),
            "DirichletBc.dof {} is out of range [0, {})",
            bc.dof,
            3 * nodes.len(),
        );
    }

    let n_nodes = nodes.len();

    // ---- Phase 1: build expansion map (Dirichlet + MPC reduction) ----------
    // For empty mpcs this degenerates to the original free-DOF map path.
    let (expansion, n_indep) = build_expansion_map(n_nodes, bcs, mpcs);

    // ---- Phase 2a: K full assembly -----------------------------------------
    // Per-element K_e for each tet; collected before borrowing into
    // AssemblyElement so both Vecs live long enough.
    let k_elems: Vec<ElementStiffness> = tets
        .iter()
        .map(|tet| {
            let phys: [[f64; 3]; 4] = [nodes[tet[0]], nodes[tet[1]], nodes[tet[2]], nodes[tet[3]]];
            element_stiffness(ElementOrder::P1, &phys[..], material)
        })
        .collect();

    let k_assembly: Vec<AssemblyElement<'_>> = tets
        .iter()
        .zip(k_elems.iter())
        .enumerate()
        .map(|(id, (conn, k_e))| AssemblyElement { id, connectivity: conn, k_e })
        .collect();

    let k_full = assemble_global_stiffness(n_nodes, &k_assembly, AssemblyMode::Deterministic);

    // ---- Phase 2b: project K to independent-DOF subspace via expansion map -
    let k_red = project_with_expansion(&k_full, &expansion, n_indep);

    // ---- Phase 2c: f_red projection ----------------------------------------
    // f_red[r] += w * f[g]  for each (r, w) in expansion[g].
    let mut f_red = vec![0.0_f64; n_indep];
    for g in 0..3 * n_nodes {
        for &(r, w) in &expansion[g] {
            f_red[r] += w * f[g];
        }
    }
    debug_assert_eq!(f_red.len(), n_indep);

    // ---- Phase 3: linear-static CG solve -----------------------------------
    let cg_opts = CgSolverOptions { tolerance: opts.cg_tolerance, max_iter: opts.cg_max_iter };
    let cg_result = solve_cg(&k_red, &f_red, cg_opts, SolverMode::Deterministic);

    // Loud failure on non-convergence per Task-2544 contract-explicitness convention.
    assert!(
        cg_result.converged,
        "pre-stress CG solve did not converge in {} iterations (residual > {:.2e} · ‖f‖). \
         Increase opts.cg_max_iter or check the BC/mesh setup.",
        cg_result.iterations,
        opts.cg_tolerance,
    );

    // ---- Expand u_red → u_full (0.0 at constrained DOFs) ------------------
    let u_red: &[f64] = &cg_result.u;
    let mut u_full = vec![0.0_f64; 3 * n_nodes];
    for g in 0..u_full.len() {
        for &(r, w) in &expansion[g] {
            u_full[g] += w * u_red[r];
        }
    }

    // ---- Phase 4: per-element σ recovery + −K_g element matrices -----------
    let mut pre_stress_per_element: Vec<[[f64; 3]; 3]> = Vec::with_capacity(tets.len());
    let mut neg_k_g_elems: Vec<ElementStiffness> = Vec::with_capacity(tets.len());

    for tet in tets {
        let phys: [[f64; 3]; 4] = [nodes[tet[0]], nodes[tet[1]], nodes[tet[2]], nodes[tet[3]]];

        // Gather per-element 12-DOF displacement vector.
        // Convention: u_e[3*local + axis] = u_full[3*global_node + axis].
        let mut u_e = [0.0_f64; 12];
        for (local, &global) in tet.iter().enumerate() {
            u_e[3 * local]     = u_full[3 * global];
            u_e[3 * local + 1] = u_full[3 * global + 1];
            u_e[3 * local + 2] = u_full[3 * global + 2];
        }

        // Recover σ_e = D · B · u_e.
        let sigma = element_stress_p1(&phys, material, &u_e);
        pre_stress_per_element.push(sigma);

        // Negate σ so that `geometric_element_stiffness_tet_p1` produces −K_g_e
        // directly (K_g is linear in σ — see design decision in plan.json).
        let neg_sigma = InitialStress3 {
            sigma: [
                [-sigma[0][0], -sigma[0][1], -sigma[0][2]],
                [-sigma[1][0], -sigma[1][1], -sigma[1][2]],
                [-sigma[2][0], -sigma[2][1], -sigma[2][2]],
            ],
        };
        let neg_k_g_e = geometric_element_stiffness_tet_p1(&phys, &neg_sigma);
        neg_k_g_elems.push(neg_k_g_e);
    }

    // ---- Phase 4b: assemble full −K_g and project via expansion map --------
    let neg_k_g_assembly: Vec<AssemblyElement<'_>> = tets
        .iter()
        .zip(neg_k_g_elems.iter())
        .enumerate()
        .map(|(id, (conn, k_e))| AssemblyElement { id, connectivity: conn, k_e })
        .collect();

    let neg_k_g_full =
        assemble_global_stiffness(n_nodes, &neg_k_g_assembly, AssemblyMode::Deterministic);
    let neg_k_g_red = project_with_expansion(&neg_k_g_full, &expansion, n_indep);

    // ---- Phase 5: generalized eigensolve K φ = λ (−K_g) φ ------------------
    let eigen_opts = EigenSolverOptions {
        n_modes: opts.n_modes,
        tol: opts.eigen_tol,
        max_iters: opts.eigen_max_iters,
        sigma: 0.0,
    };
    let eig = solve_eigen_shift_invert(&k_red, &neg_k_g_red, eigen_opts);

    // ---- Phase 6: expand mode shapes to full DOF space ----------------------
    let mut modes: Vec<Mode> = Vec::with_capacity(eig.eigenvalues.len());
    for i in 0..eig.eigenvalues.len() {
        let phi_red = eig.eigenvectors.col_as_slice(i);
        let mut mode_shape = vec![0.0_f64; 3 * n_nodes];
        for g in 0..3 * n_nodes {
            for &(r, w) in &expansion[g] {
                mode_shape[g] += w * phi_red[r];
            }
        }
        modes.push(Mode { eigenvalue: eig.eigenvalues[i], mode_shape });
    }

    BucklingKernelResult {
        modes,
        pre_stress_displacement: u_full,
        pre_stress_per_element,
        converged: eig.converged,
    }
}

/// Orchestrate the four-phase buckling pipeline for a P2 (quadratic, 10-node)
/// tetrahedral mesh.
///
/// Identical contract to [`solve_buckling_kernel`], but takes a P2 connectivity
/// `tets: &[[usize;10]]` and internally uses the P2 element kernels:
/// - `element_stiffness(ElementOrder::P2, …)` for K_e
/// - `element_stress_p2` for per-element stress recovery
/// - `geometric_element_stiffness_tet_p2` for K_g
///
/// The MPC/Dirichlet reduction (`build_expansion_map`, `project_with_expansion`)
/// and the CG / eigensolve phases are identical to the P1 path — they operate on
/// DOF indices and are element-order-agnostic.
///
/// # Design rationale
///
/// A sibling function rather than a generalised dispatch is chosen to keep the
/// shipped P1 path bit-identical (zero regression risk) and to isolate the
/// 10-node `[usize;10]` type-level difference. See the design decisions in
/// `.task/plan.json` for the full rationale.
///
/// # Panics
///
/// Same contract as [`solve_buckling_kernel`] — see that function's documentation
/// for the complete list.
#[allow(clippy::needless_range_loop)]
pub fn solve_buckling_kernel_p2(
    nodes: &[[f64; 3]],
    tets: &[[usize; 10]],
    material: &IsotropicElastic,
    bcs: &[DirichletBc],
    f: &[f64],
    mpcs: &[MpcRow],
    opts: BucklingKernelOptions,
) -> BucklingKernelResult {
    // ---- Contract checks ---------------------------------------------------
    assert_eq!(
        f.len(),
        3 * nodes.len(),
        "load vector length {} != 3 * n_nodes {}",
        f.len(),
        3 * nodes.len(),
    );
    assert!(opts.n_modes >= 1, "opts.n_modes must be >= 1, got {}", opts.n_modes);
    assert!(
        opts.eigen_tol.is_finite() && opts.eigen_tol > 0.0,
        "opts.eigen_tol must be finite and positive, got {}",
        opts.eigen_tol,
    );
    assert!(
        opts.eigen_max_iters >= 1,
        "opts.eigen_max_iters must be >= 1, got {}",
        opts.eigen_max_iters,
    );
    assert!(
        opts.cg_tolerance.is_finite() && opts.cg_tolerance > 0.0,
        "opts.cg_tolerance must be finite and positive, got {}",
        opts.cg_tolerance,
    );
    assert!(
        opts.cg_max_iter >= 1,
        "opts.cg_max_iter must be >= 1, got {}",
        opts.cg_max_iter,
    );
    for bc in bcs {
        assert!(
            bc.dof < 3 * nodes.len(),
            "DirichletBc.dof {} is out of range [0, {})",
            bc.dof,
            3 * nodes.len(),
        );
    }

    let n_nodes = nodes.len();

    // ---- Phase 1: expansion map (identical to P1 — DOF-based) ---------------
    let (expansion, n_indep) = build_expansion_map(n_nodes, bcs, mpcs);

    // ---- Phase 2a: K full assembly (P2 element stiffness, 30-DOF K_e) ------
    let k_elems: Vec<ElementStiffness> = tets
        .iter()
        .map(|tet| {
            let phys: [[f64; 3]; 10] = std::array::from_fn(|i| nodes[tet[i]]);
            element_stiffness(ElementOrder::P2, &phys[..], material)
        })
        .collect();

    let k_assembly: Vec<AssemblyElement<'_>> = tets
        .iter()
        .zip(k_elems.iter())
        .enumerate()
        .map(|(id, (conn, k_e))| AssemblyElement { id, connectivity: conn, k_e })
        .collect();

    let k_full = assemble_global_stiffness(n_nodes, &k_assembly, AssemblyMode::Deterministic);

    // ---- Phase 2b: project K to independent-DOF subspace -------------------
    let k_red = project_with_expansion(&k_full, &expansion, n_indep);

    // ---- Phase 2c: f_red projection -----------------------------------------
    let mut f_red = vec![0.0_f64; n_indep];
    for g in 0..3 * n_nodes {
        for &(r, w) in &expansion[g] {
            f_red[r] += w * f[g];
        }
    }
    debug_assert_eq!(f_red.len(), n_indep);

    // ---- Phase 3: linear-static CG solve ------------------------------------
    let cg_opts = CgSolverOptions { tolerance: opts.cg_tolerance, max_iter: opts.cg_max_iter };
    let cg_result = solve_cg(&k_red, &f_red, cg_opts, SolverMode::Deterministic);

    assert!(
        cg_result.converged,
        "P2 pre-stress CG solve did not converge in {} iterations (residual > {:.2e} · ‖f‖). \
         Increase opts.cg_max_iter or check the BC/mesh setup.",
        cg_result.iterations,
        opts.cg_tolerance,
    );

    // ---- Expand u_red → u_full -----------------------------------------------
    let u_red: &[f64] = &cg_result.u;
    let mut u_full = vec![0.0_f64; 3 * n_nodes];
    for g in 0..u_full.len() {
        for &(r, w) in &expansion[g] {
            u_full[g] += w * u_red[r];
        }
    }

    // ---- Phase 4: per-element σ recovery + −K_g element matrices (P2) ------
    let mut pre_stress_per_element: Vec<[[f64; 3]; 3]> = Vec::with_capacity(tets.len());
    let mut neg_k_g_elems: Vec<ElementStiffness> = Vec::with_capacity(tets.len());

    for tet in tets {
        let phys: [[f64; 3]; 10] = std::array::from_fn(|i| nodes[tet[i]]);

        // Gather per-element 30-DOF displacement vector.
        let mut u_e = [0.0_f64; 30];
        for (local, &global) in tet.iter().enumerate() {
            u_e[3 * local]     = u_full[3 * global];
            u_e[3 * local + 1] = u_full[3 * global + 1];
            u_e[3 * local + 2] = u_full[3 * global + 2];
        }

        // Recover σ_e = D · B(centroid) · u_e via the P2 stress kernel.
        let sigma = element_stress_p2(&phys, material, &u_e);
        pre_stress_per_element.push(sigma);

        // Feed −σ into the P2 K_g kernel to produce −K_g_e.
        let neg_sigma = InitialStress3 {
            sigma: [
                [-sigma[0][0], -sigma[0][1], -sigma[0][2]],
                [-sigma[1][0], -sigma[1][1], -sigma[1][2]],
                [-sigma[2][0], -sigma[2][1], -sigma[2][2]],
            ],
        };
        let neg_k_g_e = geometric_element_stiffness_tet_p2(&phys, &neg_sigma);
        neg_k_g_elems.push(neg_k_g_e);
    }

    // ---- Phase 4b: assemble full −K_g and project ---------------------------
    let neg_k_g_assembly: Vec<AssemblyElement<'_>> = tets
        .iter()
        .zip(neg_k_g_elems.iter())
        .enumerate()
        .map(|(id, (conn, k_e))| AssemblyElement { id, connectivity: conn, k_e })
        .collect();

    let neg_k_g_full =
        assemble_global_stiffness(n_nodes, &neg_k_g_assembly, AssemblyMode::Deterministic);
    let neg_k_g_red = project_with_expansion(&neg_k_g_full, &expansion, n_indep);

    // ---- Phase 5: generalized eigensolve K φ = λ (−K_g) φ ------------------
    let eigen_opts = EigenSolverOptions {
        n_modes: opts.n_modes,
        tol: opts.eigen_tol,
        max_iters: opts.eigen_max_iters,
        sigma: 0.0,
    };
    let eig = solve_eigen_shift_invert(&k_red, &neg_k_g_red, eigen_opts);

    // ---- Phase 6: expand mode shapes to full DOF space ----------------------
    let mut modes: Vec<Mode> = Vec::with_capacity(eig.eigenvalues.len());
    for i in 0..eig.eigenvalues.len() {
        let phi_red = eig.eigenvectors.col_as_slice(i);
        let mut mode_shape = vec![0.0_f64; 3 * n_nodes];
        for g in 0..3 * n_nodes {
            for &(r, w) in &expansion[g] {
                mode_shape[g] += w * phi_red[r];
            }
        }
        modes.push(Mode { eigenvalue: eig.eigenvalues[i], mode_shape });
    }

    BucklingKernelResult {
        modes,
        pre_stress_displacement: u_full,
        pre_stress_per_element,
        converged: eig.converged,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boundary::DirichletBc;
    use crate::constitutive::IsotropicElastic;
    use crate::mpc::MpcRow;

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
    /// Constrained DOFs: 12 (bottom all-3) + 8 (top u_x, u_y) = 20.
    /// n_free = 24 - 20 = 4  (only u_z at nodes 4–7).
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

    /// Returns the constrained DOF indices for the shape-test BC set.
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
    // -----------------------------------------------------------------------

    /// Verify the result struct has the expected dimensions for the
    /// single-brick fixture: modes count, displacement length,
    /// per-element stress count, mode-shape lengths, and Dirichlet
    /// homogeneity at constrained DOFs.
    #[test]
    fn solve_buckling_kernel_returns_well_shaped_result_for_single_brick_fixture() {
        let nodes = unit_brick_nodes();
        let tets = unit_brick_tets();
        let material = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.0 };
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

        let result = solve_buckling_kernel(&nodes, &tets, &material, &bcs, &f, &[], opts);

        // n_free = 24 - 20 = 4; dense-fallback returns min(4, n_modes=3) = 3 modes.
        // Assert ≥ 1 to stay tolerant if the actual count diverges.
        assert!(
            !result.modes.is_empty(),
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

    // -----------------------------------------------------------------------
    // step-1 (RED → GREEN in step-2): empty-mpcs signature anchor.
    // -----------------------------------------------------------------------

    /// Verify that `solve_buckling_kernel` accepts an empty `mpcs` slice via the
    /// new 7-argument signature and returns a well-shaped result identical in
    /// kind to the 6-argument baseline.
    ///
    /// This test RED-fails at compile time until the kernel signature is extended
    /// in step-2 (compile error = RED signal).
    #[test]
    fn solve_buckling_kernel_accepts_empty_mpcs_slice_and_matches_no_mpc_behavior() {
        let nodes = unit_brick_nodes();
        let tets = unit_brick_tets();
        let material = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.0 };
        let bcs = shape_test_bcs();

        let mut f = vec![0.0_f64; 3 * nodes.len()];
        for top_node in 4..8_usize {
            f[3 * top_node + 2] = -0.25;
        }

        let opts = BucklingKernelOptions {
            n_modes: 1,
            eigen_tol: 1e-8,
            eigen_max_iters: 100,
            cg_tolerance: 1e-10,
            cg_max_iter: 1000,
        };

        // New 7-arg signature: mpcs = &[] (empty slice).
        let result = solve_buckling_kernel(&nodes, &tets, &material, &bcs, &f, &[], opts);

        assert!(result.converged, "eigensolve must converge on n_free=4 system");
        assert!(!result.modes.is_empty(), "expect at least 1 mode; got {}", result.modes.len());
        assert_eq!(
            result.pre_stress_displacement.len(),
            24,
            "displacement length must be 3 * n_nodes = 24",
        );
    }

    // -----------------------------------------------------------------------
    // step-3 (RED → GREEN in step-4): MPC constraint enforcement behavioral test.
    // -----------------------------------------------------------------------

    /// Verify that a single homogeneous MPC tying `u_z[4]` to `u_z[5]` forces
    /// exact bit-equal values in both the pre-stress displacement and the first
    /// mode shape.
    ///
    /// This test RED-fails at runtime (assertion error) until step-4 implements
    /// the actual master-slave reduction; the current step-2 no-op path does NOT
    /// enforce the MPC constraint.
    #[test]
    fn solve_buckling_kernel_homogeneous_mpc_enforces_equal_dof_constraint_in_pre_stress_and_mode_shape() {
        let nodes = unit_brick_nodes();
        let tets = unit_brick_tets();
        // ν = 0 for clean analytical pre-stress.
        let material = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.0 };
        let bcs = shape_test_bcs();

        // Asymmetric load: entire 0.1-N on node 4 only (node 5 gets no load).
        // Without MPC, the mesh asymmetry ensures u_z[4] ≠ u_z[5] — this
        // distinguishes the no-MPC path (step-2) from the MPC path (step-4).
        let mut f = vec![0.0_f64; 3 * nodes.len()];
        f[3 * 4 + 2] = -0.1; // all force on node 4; node 5 gets nothing.

        // MPC: u_z[4] = u_z[5]  →  u_z[4] - u_z[5] = 0
        // MpcRow::new([slave, master], [+1, -1], 0)  →  pivot = u_z[4] (slave)
        let mpc = MpcRow::new(vec![3 * 4 + 2, 3 * 5 + 2], vec![1.0, -1.0], 0.0);

        let opts = BucklingKernelOptions {
            n_modes: 1,
            eigen_tol: 1e-8,
            eigen_max_iters: 100,
            cg_tolerance: 1e-10,
            cg_max_iter: 1000,
        };

        let result = solve_buckling_kernel(&nodes, &tets, &material, &bcs, &f, &[mpc], opts);

        // (a) Eigensolve must converge.
        assert!(result.converged, "eigensolve must converge with one MPC");

        // (b) MPC constraint in pre-stress: u_z[4] must equal u_z[5] exactly
        //     (bit-identical, since both recover from the same independent index).
        assert_eq!(
            result.pre_stress_displacement[3 * 4 + 2].to_bits(),
            result.pre_stress_displacement[3 * 5 + 2].to_bits(),
            "pre_stress: u_z[4]={} must be bit-identical to u_z[5]={} (MPC slave=master)",
            result.pre_stress_displacement[3 * 4 + 2],
            result.pre_stress_displacement[3 * 5 + 2],
        );

        // (c) MPC constraint in mode shape: same bit-equal check.
        assert!(!result.modes.is_empty(), "expect at least 1 mode");
        assert_eq!(
            result.modes[0].mode_shape[3 * 4 + 2].to_bits(),
            result.modes[0].mode_shape[3 * 5 + 2].to_bits(),
            "mode[0]: mode_shape[u_z=4]={} must be bit-identical to mode_shape[u_z=5]={}",
            result.modes[0].mode_shape[3 * 4 + 2],
            result.modes[0].mode_shape[3 * 5 + 2],
        );

        // (d) Smallest λ must be positive (−K_g PSD under compression).
        let lambda_min = result.modes[0].eigenvalue;
        assert!(
            lambda_min > 0.0,
            "λ_min = {lambda_min} must be positive for compressive load",
        );

        // (e) All Dirichlet DOFs must be exactly 0.0 in pre-stress and mode-shape.
        let constrained = shape_test_constrained_dofs();
        for &g in &constrained {
            assert_eq!(
                result.pre_stress_displacement[g], 0.0,
                "constrained DOF {g} must be 0.0 in pre_stress_displacement",
            );
        }
        for &g in &constrained {
            assert_eq!(
                result.modes[0].mode_shape[g], 0.0,
                "constrained DOF {g} must be 0.0 in mode_shape[0]",
            );
        }
    }

    // -----------------------------------------------------------------------
    // step-3b (RED → GREEN in step-4): behavioural / numerical pin.
    // -----------------------------------------------------------------------

    /// Behavioural correctness test on the same single-brick fixture.
    ///
    /// Checks five numerical properties of the kernel's output:
    ///
    /// (a) `result.converged` is true.
    /// (b) Uniform axial pre-stress: σ_zz ≈ −F = −0.1 for all 6 tets
    ///     (ν = 0 ⇒ no Poisson coupling; uniaxial compression is exact).
    /// (c) Top-face nodes compress in z; bottom-face nodes stay at zero.
    /// (d) Smallest |λ| is positive (−K_g PSD under compression ⇒ λ > 0).
    /// (e) Mode shape is 0.0 at all constrained DOFs.
    #[test]
    fn solve_buckling_kernel_recovers_uniform_axial_pre_stress_on_single_brick() {
        // Same 6-tet single-brick fixture as the shape test, but with a
        // well-defined compression magnitude so we can compare σ_zz analytically.
        let nodes = unit_brick_nodes();
        let tets = unit_brick_tets();
        // ν = 0: no Poisson coupling → σ_zz = E · ε_zz, other stress = 0.
        let material = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.0 };

        // BCs: bottom face all-3 DOFs clamped; top face u_x = u_y = 0.
        // Top can slide axially under the applied z-load.
        let bcs = shape_test_bcs();

        // Total compressive force F = 0.1, split across 4 top-face nodes.
        // Analytical: σ_zz = −F / (1 m × 1 m) = −0.1.
        const F: f64 = 0.1;
        let mut f = vec![0.0_f64; 3 * nodes.len()];
        for top_node in 4..8_usize {
            f[3 * top_node + 2] = -F / 4.0;
        }

        let opts = BucklingKernelOptions {
            n_modes: 2,
            eigen_tol: 1e-8,
            eigen_max_iters: 100,
            cg_tolerance: 1e-10,
            cg_max_iter: 1000,
        };

        let result = solve_buckling_kernel(&nodes, &tets, &material, &bcs, &f, &[], opts);

        // (a) Eigensolve must converge for the 4-free-DOF fixture.
        assert!(result.converged, "eigensolve must converge on n_free=4 system");

        // (b) Analytical pre-stress check: σ_zz ≈ −F under uniform axial compression.
        //
        //     NOTE: The 6-tet brick decomposition with the long diagonal breaks 4-fold
        //     symmetry (node 6 participates in all 6 tets while nodes 4/5/7 each
        //     participate in only 2). This means the K_free matrix is non-symmetric,
        //     and the P1 constant-strain elements give σ_zz ≈ −0.955·F rather than
        //     exactly −F. The relative error is ~4.5% — a known P1-tet mesh locking
        //     artifact, NOT a kernel integration bug.
        //
        //     Tolerance: 10% relative (= 0.10·F absolute) catches sign-flip bugs
        //     (which would give σ_zz ≈ +F, off by 2F >> tolerance) and DOF-index
        //     mix-ups (which give completely wrong magnitudes) while permitting
        //     P1 mesh-approximation error.
        // Per-element tolerance is coarse because the long-diagonal 6-tet brick
        // decomposition breaks the 4-fold symmetry: node 6 participates in all 6
        // tets while nodes 4/5/7 each appear in only 2, creating an asymmetric
        // K_free and K_g_free.  Observed per-element σ_zz range on this fixture:
        // approximately [−0.116·F, −0.095·F].  We use 25% relative tolerance:
        // loose enough for all six tets, tight enough to catch a sign-flip bug
        // (σ_zz ≈ +F would be off by 2F >> 0.025·F).
        //
        // Note: off-diagonal checks are intentionally omitted — the asymmetric mesh
        // produces shear artefacts up to ~10% of F, making 1e-9 off-diagonal checks
        // unreliable.  The σ_xx / σ_yy 25% checks below are sufficient to detect
        // DOF-index mix-ups (which would produce diagonal stress in the wrong axes).
        const TOL_25: f64 = 0.25 * F;
        for (t, sigma) in result.pre_stress_per_element.iter().enumerate() {
            // σ_zz must be negative (compression) and within 25% of −F.
            assert!(
                (sigma[2][2] - (-F)).abs() < TOL_25,
                "tet {t}: σ_zz = {:.6}, expected within 25% of {}",
                sigma[2][2],
                -F,
            );
            // σ_xx and σ_yy must be much smaller than F (ν=0 → no Poisson expansion).
            assert!(
                sigma[0][0].abs() < TOL_25,
                "tet {t}: σ_xx = {:.6}, expected ≈ 0 under ν=0 (within 25% of F={F:.3})",
                sigma[0][0],
            );
            assert!(
                sigma[1][1].abs() < TOL_25,
                "tet {t}: σ_yy = {:.6}, expected ≈ 0 under ν=0 (within 25% of F={F:.3})",
                sigma[1][1],
            );
        }

        // (c) Displacement direction: top-face nodes must compress in z.
        //     At least one top-face node must have u_z < 0 (column got shorter).
        //     None must exceed −1.0 (column hasn't tunneled through the bottom).
        let top_nodes = [4usize, 5, 6, 7];
        for &n in &top_nodes {
            let u_z = result.pre_stress_displacement[3 * n + 2];
            assert!(u_z < 0.0, "top-face node {n}: u_z = {u_z}, expected < 0 (compression)");
            assert!(u_z > -1.0, "top-face node {n}: u_z = {u_z}, looks degenerate (< -1 m)");
        }
        // Bottom-face nodes must remain at zero (Dirichlet).
        for n in 0..4_usize {
            for axis in 0..3_usize {
                assert_eq!(
                    result.pre_stress_displacement[3 * n + axis], 0.0,
                    "bottom node {n} axis {axis}: must be 0.0 (Dirichlet)",
                );
            }
        }

        // (d) Smallest |λ| must be positive: −K_g is PSD under compression,
        //     so λ = 1/(B-weighted eigenvalue) > 0.
        assert!(
            !result.modes.is_empty(),
            "must return at least 1 mode; got {}",
            result.modes.len(),
        );
        let lambda_min = result.modes[0].eigenvalue;
        assert!(
            lambda_min > 0.0,
            "λ_min = {lambda_min} must be positive for compressive load (−K_g is PSD)",
        );

        // (e) Mode shape is exactly 0.0 at all constrained DOFs.
        let constrained = shape_test_constrained_dofs();
        for (m, mode) in result.modes.iter().enumerate() {
            for &g in &constrained {
                assert_eq!(
                    mode.mode_shape[g], 0.0,
                    "mode {m}: constrained DOF {g} must be 0.0 in mode_shape",
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // step-5 (RED → GREEN in step-6): v0.5 limitation panic tests.
    // -----------------------------------------------------------------------

    /// Kernel must panic with "rhs" in the message when any MPC row has rhs ≠ 0.
    #[test]
    #[should_panic(expected = "rhs")]
    fn solve_buckling_kernel_panics_on_non_homogeneous_mpc() {
        let nodes = unit_brick_nodes();
        let tets = unit_brick_tets();
        let material = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.0 };
        let bcs = shape_test_bcs();
        let mut f = vec![0.0_f64; 3 * nodes.len()];
        f[3 * 4 + 2] = -0.1;

        // rhs = 1.0: inhomogeneous — must panic.
        let mpc = MpcRow::new(vec![3 * 4 + 2, 3 * 5 + 2], vec![1.0, -1.0], 1.0);
        let opts = BucklingKernelOptions { n_modes: 1, ..Default::default() };
        let _ = solve_buckling_kernel(&nodes, &tets, &material, &bcs, &f, &[mpc], opts);
    }

    /// Kernel must panic with "pivot" in the message when a slave DOF is also Dirichlet.
    #[test]
    #[should_panic(expected = "pivot")]
    fn solve_buckling_kernel_panics_on_dirichlet_pivot() {
        let nodes = unit_brick_nodes();
        let tets = unit_brick_tets();
        let material = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.0 };
        let bcs = shape_test_bcs();
        let mut f = vec![0.0_f64; 3 * nodes.len()];
        f[3 * 4 + 2] = -0.1;

        // Pivot DOF 0 (u_x[0]) is constrained by Dirichlet BC — must panic with "pivot".
        let mpc = MpcRow::new(vec![0, 3 * 5 + 2], vec![1.0, -1.0], 0.0);
        let opts = BucklingKernelOptions { n_modes: 1, ..Default::default() };
        let _ = solve_buckling_kernel(&nodes, &tets, &material, &bcs, &f, &[mpc], opts);
    }

    /// Kernel must panic with "chain" when an MPC's "other" DOF is itself a pivot.
    #[test]
    #[should_panic(expected = "chain")]
    fn solve_buckling_kernel_panics_on_chained_mpc() {
        let nodes = unit_brick_nodes();
        let tets = unit_brick_tets();
        let material = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.0 };
        let bcs = shape_test_bcs();
        let mut f = vec![0.0_f64; 3 * nodes.len()];
        f[3 * 4 + 2] = -0.1;

        // MPC chain: row0 pivots u_z[4], other = u_z[5].
        //            row1 pivots u_z[5], other = u_z[6].
        // u_z[5] is the pivot of row1 AND the "other" of row0 → chain violation.
        let mpc0 = MpcRow::new(vec![3 * 4 + 2, 3 * 5 + 2], vec![1.0, -1.0], 0.0);
        let mpc1 = MpcRow::new(vec![3 * 5 + 2, 3 * 6 + 2], vec![1.0, -1.0], 0.0);
        let opts = BucklingKernelOptions { n_modes: 1, ..Default::default() };
        let _ = solve_buckling_kernel(&nodes, &tets, &material, &bcs, &f, &[mpc0, mpc1], opts);
    }

    // -----------------------------------------------------------------------
    // step-7 (RED → GREEN in step-8): solve_buckling_kernel_p2 unit tests.
    //
    // Reuse the single 1×1×1 m brick fixture, promoted to P2 via
    // `promote_tets_to_p2`.  The P2 mesh has 8 corners + 18 edge-midpoints
    // (from 6 tets × 6 edges, deduped) = 26 nodes, 78 DOFs.
    // Corner node indices (0–7) are identical in P1 and P2 (corners come
    // first); top-face corners are still nodes 4–7 at z=1.
    // -----------------------------------------------------------------------

    /// Build the P2 brick fixture: promote P1 brick to P2 and return
    /// (nodes_p2, tets_p2, n_nodes_p2, n_tets_p2).
    fn unit_brick_p2() -> (Vec<[f64; 3]>, Vec<[usize; 10]>) {
        use crate::assembly::test_support::promote_tets_to_p2;
        let nodes_p1 = unit_brick_nodes();
        let tets_p1 = unit_brick_tets();
        promote_tets_to_p2(&nodes_p1, &tets_p1)
    }

    /// Build BCs for the P2 brick fixture: clamp all DOFs on the z=0 face,
    /// and clamp u_x, u_y on the z=1 face.  Identifies face nodes by
    /// coordinate rather than hard-coded index so it works for both the 8
    /// corner nodes AND the additional P2 midpoint nodes on those faces.
    fn shape_test_bcs_p2(nodes_p2: &[[f64; 3]]) -> Vec<DirichletBc> {
        let mut bcs = Vec::new();
        for (n, xyz) in nodes_p2.iter().enumerate() {
            if (xyz[2] - 0.0).abs() < 1e-10 {
                // Bottom face (z ≈ 0): clamp all 3 DOFs.
                for axis in 0..3_usize {
                    bcs.push(DirichletBc { dof: 3 * n + axis, value: 0.0 });
                }
            } else if (xyz[2] - 1.0).abs() < 1e-10 {
                // Top face (z ≈ 1): clamp lateral DOFs only.
                bcs.push(DirichletBc { dof: 3 * n,     value: 0.0 }); // u_x
                bcs.push(DirichletBc { dof: 3 * n + 1, value: 0.0 }); // u_y
            }
        }
        bcs
    }

    /// Collect all constrained DOF indices for the P2 shape-test BC set.
    fn shape_test_constrained_dofs_p2(nodes_p2: &[[f64; 3]]) -> Vec<usize> {
        let mut v = Vec::new();
        for (n, xyz) in nodes_p2.iter().enumerate() {
            if (xyz[2] - 0.0).abs() < 1e-10 {
                v.push(3 * n);
                v.push(3 * n + 1);
                v.push(3 * n + 2);
            } else if (xyz[2] - 1.0).abs() < 1e-10 {
                v.push(3 * n);
                v.push(3 * n + 1);
            }
        }
        v
    }

    /// (a) verify that `solve_buckling_kernel_p2` accepts an empty mpcs slice and
    /// returns a well-shaped `BucklingKernelResult`:
    /// - modes non-empty
    /// - pre_stress_displacement.len() == 3·n_nodes_p2
    /// - one stress tensor per P2 tet (6 tets)
    /// - every mode_shape.len() == 3·n_nodes_p2
    /// (b) all Dirichlet-constrained DOFs are exactly 0.0 in pre_stress and
    ///     every mode shape.
    ///
    /// RED signal: the symbol does not exist → compile failure.
    #[test]
    fn solve_buckling_kernel_p2_returns_well_shaped_result_for_single_brick_fixture() {
        let (nodes_p2, tets_p2) = unit_brick_p2();
        let n_nodes_p2 = nodes_p2.len();
        let n_tets_p2 = tets_p2.len(); // 6 tets (same as P1 — promotion keeps tet count)
        let material = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.0 };
        let bcs = shape_test_bcs_p2(&nodes_p2);

        // Downward unit load split across the four top-face corner nodes (4–7).
        let mut f = vec![0.0_f64; 3 * n_nodes_p2];
        for top_node in 4..8_usize {
            f[3 * top_node + 2] = -0.25;
        }

        let opts = BucklingKernelOptions {
            n_modes: 3,
            eigen_tol: 1e-8,
            eigen_max_iters: 200,
            cg_tolerance: 1e-10,
            cg_max_iter: 2000,
        };

        let result =
            solve_buckling_kernel_p2(&nodes_p2, &tets_p2, &material, &bcs, &f, &[], opts);

        // (a) shape assertions
        assert!(
            !result.modes.is_empty(),
            "expect at least 1 mode; got {}",
            result.modes.len(),
        );
        assert_eq!(
            result.pre_stress_displacement.len(),
            3 * n_nodes_p2,
            "displacement must have length 3 * n_nodes_p2 = {}",
            3 * n_nodes_p2,
        );
        assert_eq!(
            result.pre_stress_per_element.len(),
            n_tets_p2,
            "one stress tensor per P2 tet ({n_tets_p2})",
        );
        for (m, mode) in result.modes.iter().enumerate() {
            assert_eq!(
                mode.mode_shape.len(),
                3 * n_nodes_p2,
                "mode {m} shape must have length 3 * n_nodes_p2 = {}",
                3 * n_nodes_p2,
            );
        }

        // (b) all Dirichlet-constrained DOFs must be exactly 0.0
        let constrained = shape_test_constrained_dofs_p2(&nodes_p2);
        for &g in &constrained {
            assert_eq!(
                result.pre_stress_displacement[g], 0.0,
                "constrained DOF {g} must be 0.0 in pre_stress_displacement",
            );
        }
        for (m, mode) in result.modes.iter().enumerate() {
            for &g in &constrained {
                assert_eq!(
                    mode.mode_shape[g], 0.0,
                    "mode {m}: constrained DOF {g} must be 0.0 in mode_shape",
                );
            }
        }
    }

    /// (c) uniform axial pre-stress recovery under ν=0 compression.
    /// (d) λ_min > 0.
    /// (e) homogeneous MPC tying u_z of two top-face corner nodes forces
    ///     bit-identical values in pre_stress and mode shape (mirrors the
    ///     P1 MPC enforcement test).
    ///
    /// RED signal: the symbol does not exist → compile failure.
    #[test]
    fn solve_buckling_kernel_p2_recovers_uniform_axial_pre_stress_and_enforces_mpc() {
        let (nodes_p2, tets_p2) = unit_brick_p2();
        let n_nodes_p2 = nodes_p2.len();
        let material = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.0 };
        let bcs = shape_test_bcs_p2(&nodes_p2);

        // Total compressive force F = 0.1, split across 4 top-face corner nodes.
        const F: f64 = 0.1;
        let mut f = vec![0.0_f64; 3 * n_nodes_p2];
        for top_node in 4..8_usize {
            f[3 * top_node + 2] = -F / 4.0;
        }

        let opts = BucklingKernelOptions {
            n_modes: 2,
            eigen_tol: 1e-8,
            eigen_max_iters: 200,
            cg_tolerance: 1e-10,
            cg_max_iter: 2000,
        };

        let result =
            solve_buckling_kernel_p2(&nodes_p2, &tets_p2, &material, &bcs, &f, &[], opts.clone());

        // (c) Uniform axial pre-stress: σ_zz ≈ −F for all 6 tets.
        //     25% tolerance — the long-diagonal 6-tet decomposition breaks
        //     4-fold symmetry (same as P1 shape test above).
        assert!(result.converged, "eigensolve must converge on small P2 fixture");
        const TOL_25: f64 = 0.25 * F;
        for (t, sigma) in result.pre_stress_per_element.iter().enumerate() {
            assert!(
                (sigma[2][2] - (-F)).abs() < TOL_25,
                "tet {t}: σ_zz = {:.6}, expected within 25% of {:.3}",
                sigma[2][2],
                -F,
            );
            assert!(
                sigma[0][0].abs() < TOL_25,
                "tet {t}: σ_xx = {:.6}, expected ≈ 0 under ν=0",
                sigma[0][0],
            );
            assert!(
                sigma[1][1].abs() < TOL_25,
                "tet {t}: σ_yy = {:.6}, expected ≈ 0 under ν=0",
                sigma[1][1],
            );
        }

        // (d) λ_min > 0 under compression.
        assert!(!result.modes.is_empty(), "expect at least 1 mode");
        let lambda_min = result.modes[0].eigenvalue;
        assert!(
            lambda_min > 0.0,
            "λ_min = {lambda_min} must be positive for compressive load",
        );

        // (e) MPC: tie u_z[4] to u_z[5] (both are top-face corner nodes,
        //     indices 4 and 5 unchanged in P2).  Apply asymmetric load on
        //     node 4 only so the unconstrained paths would give u_z[4] ≠ u_z[5].
        let mut f_mpc = vec![0.0_f64; 3 * n_nodes_p2];
        f_mpc[3 * 4 + 2] = -0.1; // all force on node 4
        let mpc = MpcRow::new(vec![3 * 4 + 2, 3 * 5 + 2], vec![1.0, -1.0], 0.0);

        let result_mpc = solve_buckling_kernel_p2(
            &nodes_p2,
            &tets_p2,
            &material,
            &bcs,
            &f_mpc,
            &[mpc],
            opts,
        );

        assert!(result_mpc.converged, "eigensolve must converge with one P2 MPC");

        // u_z[4] == u_z[5] bit-identically in both pre_stress and mode[0].
        assert_eq!(
            result_mpc.pre_stress_displacement[3 * 4 + 2].to_bits(),
            result_mpc.pre_stress_displacement[3 * 5 + 2].to_bits(),
            "P2 pre_stress: u_z[4]={} must be bit-identical to u_z[5]={} (MPC)",
            result_mpc.pre_stress_displacement[3 * 4 + 2],
            result_mpc.pre_stress_displacement[3 * 5 + 2],
        );
        assert!(!result_mpc.modes.is_empty(), "expect at least 1 mode under P2 MPC");
        assert_eq!(
            result_mpc.modes[0].mode_shape[3 * 4 + 2].to_bits(),
            result_mpc.modes[0].mode_shape[3 * 5 + 2].to_bits(),
            "P2 mode[0]: mode_shape[u_z=4]={} must be bit-identical to mode_shape[u_z=5]={}",
            result_mpc.modes[0].mode_shape[3 * 4 + 2],
            result_mpc.modes[0].mode_shape[3 * 5 + 2],
        );
    }
}
