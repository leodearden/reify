//! The FEA harness shared by this crate's FEA integration-test binaries: a
//! structured box mesh generator and its Kuhn hex-to-tet split, Dirichlet-BC
//! and point-load builders, the element-agnostic retaining eliminate → CG tail
//! and its displacement-only projection, P1 stiffness assembly and the two P1
//! compositions built from them, and the element-level post-processing
//! (`gather_u_p1`, `von_mises_of_tensor`) that consumes a solve's output.
//!
//! Not a test binary. A consumer includes it with
//! `#[path = "common/fea_harness.rs"] mod fea_harness;` rather than through
//! `common/mod.rs`, so the binaries that import `common` for its tensegrity
//! fixtures do not compile it. Each consumer uses a subset of the helpers,
//! hence the `dead_code` allowance.

#![allow(dead_code)]

use faer::sparse::SparseRowMat;
use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, CgResult, CgSolverOptions, DirichletBc, ElementOrder,
    ElementStiffness, IsotropicElastic, SolverMode, apply_dirichlet_row_elimination,
    assemble_global_stiffness, solve_cg,
};

/// Split a hex cell into 6 tetrahedra via the Kuhn triangulation.
///
/// All 6 tets share the main diagonal from `c[0]` to `c[6]`. This follows
/// the 6 permutations of coordinate increments (Kuhn 1960); each tet's
/// winding matches the corner ordering below (positive volume for an
/// axis-aligned hex with all positive edge lengths).
///
/// # Corner ordering
///
/// ```text
/// c[0] = (ix,   iy,   iz)    c[4] = (ix,   iy,   iz+1)
/// c[1] = (ix+1, iy,   iz)    c[5] = (ix+1, iy,   iz+1)
/// c[2] = (ix+1, iy+1, iz)    c[6] = (ix+1, iy+1, iz+1)
/// c[3] = (ix,   iy+1, iz)    c[7] = (ix,   iy+1, iz+1)
/// ```
pub fn kuhn_split_hex_to_six_tets(c: [usize; 8]) -> [[usize; 4]; 6] {
    // The 6 permutations of (Δx, Δy, Δz) define the 6 tets.
    // All start at c[0] = (000) and end at c[6] = (111).
    // Ordering: path 000 → σ₁ → σ₁σ₂ → 111.
    [
        [c[0], c[1], c[2], c[6]], // σ=(x,y,z): 000→100→110→111
        [c[0], c[1], c[5], c[6]], // σ=(x,z,y): 000→100→101→111
        [c[0], c[3], c[2], c[6]], // σ=(y,x,z): 000→010→110→111
        [c[0], c[3], c[7], c[6]], // σ=(y,z,x): 000→010→011→111
        [c[0], c[4], c[5], c[6]], // σ=(z,x,y): 000→001→101→111
        [c[0], c[4], c[7], c[6]], // σ=(z,y,x): 000→001→011→111
    ]
}

/// Build a structured P1 tet mesh for a rectangular box
/// `[0,Lx] × [0,Ly] × [0,Lz]` with `nx × ny × nz` hex cells.
///
/// Node indexing: `node(ix, iy, iz) = iz*(ny+1)*(nx+1) + iy*(nx+1) + ix`.
/// Each hex is Kuhn-split into 6 tets (see [`kuhn_split_hex_to_six_tets`]).
pub fn box_p1_mesh(
    lx: f64, ly: f64, lz: f64,
    nx: usize, ny: usize, nz: usize,
) -> (Vec<[f64; 3]>, Vec<[usize; 4]>) {
    let nnx = nx + 1;
    let nny = ny + 1;
    let nnz = nz + 1;

    let mut nodes = Vec::with_capacity(nnx * nny * nnz);
    for iz in 0..nnz {
        for iy in 0..nny {
            for ix in 0..nnx {
                nodes.push([
                    ix as f64 * lx / nx as f64,
                    iy as f64 * ly / ny as f64,
                    iz as f64 * lz / nz as f64,
                ]);
            }
        }
    }

    let node_idx = |ix: usize, iy: usize, iz: usize| -> usize {
        iz * nny * nnx + iy * nnx + ix
    };

    let mut connectivity = Vec::with_capacity(6 * nx * ny * nz);
    for iz in 0..nz {
        for iy in 0..ny {
            for ix in 0..nx {
                let c = [
                    node_idx(ix,   iy,   iz),
                    node_idx(ix+1, iy,   iz),
                    node_idx(ix+1, iy+1, iz),
                    node_idx(ix,   iy+1, iz),
                    node_idx(ix,   iy,   iz+1),
                    node_idx(ix+1, iy,   iz+1),
                    node_idx(ix+1, iy+1, iz+1),
                    node_idx(ix,   iy+1, iz+1),
                ];
                for tet in kuhn_split_hex_to_six_tets(c) {
                    connectivity.push(tet);
                }
            }
        }
    }

    (nodes, connectivity)
}

/// Build Dirichlet BCs to fix all 3 DOFs on nodes within `tol` of
/// `nodes[n][axis] == value`.
pub fn dirichlet_fix_face(
    nodes: &[[f64; 3]],
    axis: usize,
    value: f64,
    tol: f64,
) -> Vec<DirichletBc> {
    let mut bcs = Vec::new();
    for (node, n) in nodes.iter().enumerate() {
        if (n[axis] - value).abs() < tol {
            for dof_idx in 0..3_usize {
                bcs.push(DirichletBc { dof: node * 3 + dof_idx, value: 0.0 });
            }
        }
    }
    bcs
}

/// Prescribe the full 3-DOF displacement `field(x)` on every node lying on any
/// of the six bounding planes of `[0,lx]×[0,ly]×[0,lz]` (within `tol`).
///
/// Prescribing an affine `field` on the whole boundary of a homogeneous body,
/// with no other loads, makes the interior P1/P2 solution that same field: the
/// affine field is the exact continuum solution (uniform strain, hence uniform
/// stress, so div σ = 0), and P1/P2 tets represent it exactly. This is the
/// constant-strain patch-test property.
pub fn dirichlet_prescribe_boundary_field(
    nodes: &[[f64; 3]],
    lx: f64,
    ly: f64,
    lz: f64,
    tol: f64,
    field: impl Fn([f64; 3]) -> [f64; 3],
) -> Vec<DirichletBc> {
    let mut bcs = Vec::new();
    for (node, &p) in nodes.iter().enumerate() {
        let on_boundary = p[0].abs() < tol
            || (p[0] - lx).abs() < tol
            || p[1].abs() < tol
            || (p[1] - ly).abs() < tol
            || p[2].abs() < tol
            || (p[2] - lz).abs() < tol;
        if on_boundary {
            for (dof_idx, &val) in field(p).iter().enumerate() {
                bcs.push(DirichletBc { dof: node * 3 + dof_idx, value: val });
            }
        }
    }
    bcs
}

/// Indices of every node on the free-end face `x = l` (within `tol`).
pub fn end_face_nodes(nodes: &[[f64; 3]], l: f64, tol: f64) -> Vec<usize> {
    nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| (n[0] - l).abs() < tol)
        .map(|(i, _)| i)
        .collect()
}

/// Distribute a transverse shear resultant `f_mag` (in −y) equally over the
/// `end` nodes → a set of nodal point loads whose resultant is exactly `−f_mag`
/// at `x = l`. By Saint-Venant the equal split is immaterial to the tip
/// deflection (only the resultant matters far from the end).
pub fn distributed_tip_load(end: &[usize], f_mag: f64) -> Vec<(usize, f64)> {
    let per = f_mag / end.len() as f64;
    end.iter().map(|&n| (n * 3 + 1, -per)).collect()
}

/// Deduplicate Dirichlet BCs.
///
/// `apply_dirichlet_row_elimination` panics on duplicate DOF indices (debug
/// builds). This helper mirrors the dedup pattern from `shell_benchmarks.rs`.
/// In debug builds, asserts that conflicting values at the same DOF surface
/// rather than being silently dropped.
pub fn dedup_bcs(bcs: &mut Vec<DirichletBc>) {
    bcs.sort_by_key(|bc| bc.dof);
    if cfg!(debug_assertions) {
        for w in bcs.windows(2) {
            if w[0].dof == w[1].dof {
                assert_eq!(
                    w[0].value, w[1].value,
                    "dedup_bcs: conflicting values at DOF {} ({} vs {})",
                    w[0].dof, w[0].value, w[1].value,
                );
            }
        }
    }
    bcs.dedup_by_key(|bc| bc.dof);
}

/// Assemble the global stiffness matrix of a P1 tetrahedral mesh, in
/// `AssemblyMode::Deterministic` for bit-stable, CI-safe results.
pub fn assemble_p1_stiffness(
    nodes: &[[f64; 3]],
    conns: &[[usize; 4]],
    mat: &IsotropicElastic,
) -> SparseRowMat<usize, f64> {
    let ke_list: Vec<ElementStiffness> = conns
        .iter()
        .map(|conn| {
            let elem_nodes: Vec<[f64; 3]> = conn.iter().map(|&i| nodes[i]).collect();
            reify_solver_elastic::element_stiffness(ElementOrder::P1, &elem_nodes, mat)
        })
        .collect();

    let elements: Vec<AssemblyElement<'_>> = conns
        .iter()
        .zip(ke_list.iter())
        .enumerate()
        .map(|(i, (conn, ke))| AssemblyElement {
            id: i,
            connectivity: conn.as_slice(),
            k_e: ke,
        })
        .collect();

    assemble_global_stiffness(nodes.len(), &elements, AssemblyMode::Deterministic)
}

/// Scatter nodal point loads into a freshly zeroed global RHS of length
/// `3 * n_nodes`.
///
/// [`eliminate_and_solve`] takes a whole RHS rather than a
/// `(dof, value)` list so that a caller can equally pass it a load assembled
/// another way, such as a quantity of interest's `dual_load`.
pub fn rhs_from_point_loads(n_nodes: usize, point_loads: &[(usize, f64)]) -> Vec<f64> {
    let mut f = vec![0.0_f64; 3 * n_nodes];
    for &(dof, val) in point_loads {
        f[dof] += val;
    }
    f
}

/// What [`eliminate_and_solve`] retains of a solve: everything a dual
/// (adjoint) solve over the same system needs, so that it repeats neither the
/// assembly nor the elimination.
pub struct RetainedSolve {
    /// `K`, already row-eliminated, so a dual solve reuses it verbatim.
    pub k_eliminated: SparseRowMat<usize, f64>,
    /// The ELIMINATED right-hand side: the `f` that the reciprocity identity
    /// `J(u_h) == fᵀz_h` is stated against (`J` a quantity of interest, `u_h`
    /// the primal solution, `z_h` the dual solution), not the raw one.
    pub f_eliminated: Vec<f64>,
    /// The deduplicated BC list, i.e. the DOFs at which a dual load is zeroed:
    /// exactly those that were actually constrained.
    pub bcs_deduped: Vec<DirichletBc>,
    /// The primal CG solve. It is not asserted on, so the caller decides what
    /// an unconverged solve means.
    pub primal: CgResult,
}

/// Eliminate Dirichlet BCs from an assembled system and CG-solve it, retaining
/// everything a dual (adjoint) solve over the same system needs (see
/// [`RetainedSolve`]). Element-agnostic: `k` may come from any assembly.
///
/// Runs [`dedup_bcs`] → `apply_dirichlet_row_elimination` → `solve_cg`.
/// `SolverMode::Deterministic` is fixed (not a parameter) for bit-stable,
/// CI-safe results; `opts` IS a parameter so a caller can tighten the tolerance
/// below the crate default or raise the iteration cap.
///
/// # Panics
///
/// If `raw_f.len() != k.nrows()`.
pub fn eliminate_and_solve(
    mut k: SparseRowMat<usize, f64>,
    raw_f: &[f64],
    mut bcs: Vec<DirichletBc>,
    opts: CgSolverOptions,
) -> RetainedSolve {
    assert_eq!(
        raw_f.len(),
        k.nrows(),
        "raw_f.len() = {} but the system has {} DOFs",
        raw_f.len(),
        k.nrows(),
    );

    let mut f = raw_f.to_vec();
    dedup_bcs(&mut bcs);
    apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

    let primal = solve_cg(&k, &f, opts, SolverMode::Deterministic);

    RetainedSolve {
        k_eliminated: k,
        f_eliminated: f,
        bcs_deduped: bcs,
        primal,
    }
}

/// Assemble, eliminate and CG-solve a P1 tetrahedral FEA system, retaining
/// everything a dual (adjoint) solve over the same system needs:
/// [`assemble_p1_stiffness`] + [`eliminate_and_solve`].
///
/// # Panics
///
/// As [`eliminate_and_solve`]; for P1, `K`'s dimension is `3 * nodes.len()`.
pub fn assemble_eliminate_and_solve(
    nodes: &[[f64; 3]],
    conns: &[[usize; 4]],
    mat: &IsotropicElastic,
    bcs: Vec<DirichletBc>,
    raw_f: &[f64],
    opts: CgSolverOptions,
) -> RetainedSolve {
    eliminate_and_solve(assemble_p1_stiffness(nodes, conns, mat), raw_f, bcs, opts)
}

/// [`eliminate_and_solve`] with an asserted convergence, keeping only the
/// displacement `u`.
///
/// `bcs` is deduplicated in place: afterwards the caller's `Vec` holds the
/// sorted, deduplicated list.
///
/// # Returns
///
/// Displacement vector `u` of length `k.nrows()`.
///
/// # Panics
///
/// As [`eliminate_and_solve`], and if the CG solve does not converge within
/// `opts.max_iter` iterations. The convergence panic is `#[track_caller]`, so
/// it names the line that called the pipeline; a wrapper that carries the
/// attribute too passes that line through.
#[track_caller]
pub fn solve_displacement(
    k: SparseRowMat<usize, f64>,
    raw_f: &[f64],
    bcs: &mut Vec<DirichletBc>,
    opts: CgSolverOptions,
) -> Vec<f64> {
    let max_iter = opts.max_iter;
    let solved = eliminate_and_solve(k, raw_f, std::mem::take(bcs), opts);
    *bcs = solved.bcs_deduped;
    assert!(
        solved.primal.converged,
        "solve_displacement: CG did not converge after {} iterations (cap={max_iter}, ndof={})",
        solved.primal.iterations,
        solved.k_eliminated.nrows(),
    );
    solved.primal.u().to_vec()
}

/// Assemble, apply BCs, and CG-solve a P1 tetrahedral FEA system, keeping only
/// the displacement: [`rhs_from_point_loads`] and [`assemble_p1_stiffness`]
/// fed to [`solve_displacement`] with the crate-default `CgSolverOptions`.
/// `bcs` is deduplicated in place: afterwards the caller's `Vec` holds the
/// sorted, deduplicated list.
///
/// # Returns
///
/// Displacement vector `u` of length `3 * nodes.len()`:
/// `u[3*n + α]` is displacement of node `n` in axis `α ∈ {0=x, 1=y, 2=z}`.
#[track_caller]
pub fn solve_p1_pipeline(
    nodes: &[[f64; 3]],
    conns: &[[usize; 4]],
    bcs: &mut Vec<DirichletBc>,
    point_loads: &[(usize, f64)],
    mat: &IsotropicElastic,
) -> Vec<f64> {
    let f = rhs_from_point_loads(nodes.len(), point_loads);
    solve_displacement(
        assemble_p1_stiffness(nodes, conns, mat),
        &f,
        bcs,
        CgSolverOptions::default(),
    )
}

/// Gather the 12 element DOFs (`[u_x,u_y,u_z]` per corner) for a P1 tet from the
/// global displacement vector, in element-local node order.
pub fn gather_u_p1(u: &[f64], conn: &[usize; 4]) -> [f64; 12] {
    let mut ue = [0.0_f64; 12];
    for (k, &node) in conn.iter().enumerate() {
        ue[3 * k] = u[3 * node];
        ue[3 * k + 1] = u[3 * node + 1];
        ue[3 * k + 2] = u[3 * node + 2];
    }
    ue
}

/// Compute von Mises stress from a 3×3 Cauchy stress tensor.
pub fn von_mises_of_tensor(s: &[[f64; 3]; 3]) -> f64 {
    let (s11, s22, s33) = (s[0][0], s[1][1], s[2][2]);
    let (s12, s23, s13) = (s[0][1], s[1][2], s[0][2]);
    let v = 0.5 * ((s11 - s22).powi(2) + (s22 - s33).powi(2) + (s33 - s11).powi(2))
        + 3.0 * (s12.powi(2) + s23.powi(2) + s13.powi(2));
    v.sqrt()
}
