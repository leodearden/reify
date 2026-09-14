//! Goal-oriented (dual-weighted residual) boundary tests BT1–BT4.
//!
//! PRD reference: `docs/prds/v0_6/goal-oriented-error-estimation.md`
//! §6 "Two-way boundary tests" (BT1 reciprocity, BT2 self-dual reduction,
//! BT3 patch exactness with a real dual, BT4 unresolvable-is-typed at the
//! seam level), task 7452 leaf β.
//!
//! # Why a separate binary
//!
//! BT1–BT3 each need a *real* assemble → eliminate → CG pipeline run twice
//! (primal and dual) over the same stiffness matrix, which neither
//! `src/qoi.rs`'s nor `src/error_estimator.rs`'s `#[cfg(test)]` mod can host
//! — those pin closed-form algebra over hand-built fixtures. Keeping the
//! boundary tests in their own integration binary also keeps them separable
//! from the a-posteriori convergence suite, whose goldens this task only
//! touches mechanically.
//!
//! # Ported FEA harness, and the debt that comes with it
//!
//! The box-mesh / BC / load helpers below are ported from
//! `tests/aposteriori_validation.rs` (itself a verbatim port of
//! `tests/analytical_validation.rs`'s task-2928 harness), so the solve
//! pipeline shape is identical to the rest of the FEA validation suite.
//! This is the THIRD copy. Extracting the shared harness is the real SPOT
//! fix, but it is a large mechanical diff across 2500+ lines of landed
//! goldens and is out of this leaf's scope; it is recorded as a filed
//! low-priority follow-up (`agent-followup-7452`) rather than done here or
//! silently ignored.
//!
//! One deliberate change from the port: [`assemble_eliminate_and_solve`]
//! RETAINS the assembled `K`, the eliminated RHS and the deduplicated BC
//! list, all three of which `solve_p1_pipeline` drops. The dual solve
//! (§5.3) reuses exactly those three, so retaining them is the whole reason
//! this copy exists rather than a call into the original.

// The harness helpers below are consumed by BT1–BT4, which land in later
// steps of this task. Removed once every helper has a caller.
#![allow(dead_code)]

use faer::sparse::SparseRowMat;

use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, CgResult, CgSolverOptions, DirichletBc, ElementOrder,
    ElementStiffness, IsotropicElastic, SolverMode, apply_dirichlet_row_elimination,
    assemble_global_stiffness, element_stiffness, solve_cg,
};

// ─── ported FEA harness helpers (tests/aposteriori_validation.rs) ──────────

/// Split a hex cell into 6 tetrahedra via the Kuhn triangulation.
///
/// Ported verbatim; private dependency of [`box_p1_mesh`].
fn kuhn_split_hex_to_six_tets(c: [usize; 8]) -> [[usize; 4]; 6] {
    [
        [c[0], c[1], c[2], c[6]],
        [c[0], c[1], c[5], c[6]],
        [c[0], c[3], c[2], c[6]],
        [c[0], c[3], c[7], c[6]],
        [c[0], c[4], c[5], c[6]],
        [c[0], c[4], c[7], c[6]],
    ]
}

/// Build a structured P1 tet mesh for a rectangular box `[0,Lx] x [0,Ly] x
/// [0,Lz]` with `nx x ny x nz` hex cells (each Kuhn-split into 6 tets).
///
/// Ported verbatim. Supplies every fixture in this file: the cantilever
/// pencils (BT1), the `f = g` self-dual box (BT2) and the unit-cube patch
/// (BT3).
fn box_p1_mesh(
    lx: f64,
    ly: f64,
    lz: f64,
    nx: usize,
    ny: usize,
    nz: usize,
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

    let node_idx = |ix: usize, iy: usize, iz: usize| -> usize { iz * nny * nnx + iy * nnx + ix };

    let mut connectivity = Vec::with_capacity(6 * nx * ny * nz);
    for iz in 0..nz {
        for iy in 0..ny {
            for ix in 0..nx {
                let c = [
                    node_idx(ix, iy, iz),
                    node_idx(ix + 1, iy, iz),
                    node_idx(ix + 1, iy + 1, iz),
                    node_idx(ix, iy + 1, iz),
                    node_idx(ix, iy, iz + 1),
                    node_idx(ix + 1, iy, iz + 1),
                    node_idx(ix + 1, iy + 1, iz + 1),
                    node_idx(ix, iy + 1, iz + 1),
                ];
                for tet in kuhn_split_hex_to_six_tets(c) {
                    connectivity.push(tet);
                }
            }
        }
    }

    (nodes, connectivity)
}

/// Build Dirichlet BCs fixing all 3 DOFs on nodes within `tol` of
/// `nodes[n][axis] == value`.
///
/// Ported verbatim. Clamps the cantilever's `x = 0` face.
fn dirichlet_fix_face(nodes: &[[f64; 3]], axis: usize, value: f64, tol: f64) -> Vec<DirichletBc> {
    let mut bcs = Vec::new();
    for (node, n) in nodes.iter().enumerate() {
        if (n[axis] - value).abs() < tol {
            for dof_idx in 0..3_usize {
                bcs.push(DirichletBc {
                    dof: node * 3 + dof_idx,
                    value: 0.0,
                });
            }
        }
    }
    bcs
}

/// Indices of every node on the free-end face `x = l` (within `tol`).
///
/// Ported verbatim. Identifies the cantilever's loaded end face.
fn end_face_nodes(nodes: &[[f64; 3]], l: f64, tol: f64) -> Vec<usize> {
    nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| (n[0] - l).abs() < tol)
        .map(|(i, _)| i)
        .collect()
}

/// Distribute a transverse shear resultant `f_mag` (in -y) equally over the
/// `end` nodes — nodal point loads whose resultant is exactly `-f_mag` at
/// `x = l`.
///
/// Ported verbatim. Because the load is DISTRIBUTED over the whole tip face,
/// no scalar multiple of it can equal a ball-mean QoI's `dual_load` — which
/// is why BT2 builds its `f = g` fixture directly rather than reusing a
/// cantilever pencil.
fn distributed_tip_load(end: &[usize], f_mag: f64) -> Vec<(usize, f64)> {
    let per = f_mag / end.len() as f64;
    end.iter().map(|&n| (n * 3 + 1, -per)).collect()
}

/// Gather the 12 element DOFs (`[u_x,u_y,u_z]` per corner) for a P1 tet from
/// the global displacement vector, in element-local node order.
///
/// Ported verbatim.
fn gather_u_p1(u: &[f64], conn: &[usize; 4]) -> [f64; 12] {
    let mut ue = [0.0_f64; 12];
    for (k, &node) in conn.iter().enumerate() {
        ue[3 * k] = u[3 * node];
        ue[3 * k + 1] = u[3 * node + 1];
        ue[3 * k + 2] = u[3 * node + 2];
    }
    ue
}

/// Deduplicate Dirichlet BCs (`apply_dirichlet_row_elimination` panics on
/// duplicate DOF indices in debug builds).
///
/// Ported verbatim.
fn dedup_bcs(bcs: &mut Vec<DirichletBc>) {
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

/// Scatter nodal point loads into a freshly zeroed global RHS of length
/// `3 * n_nodes`.
///
/// The `raw_f` [`assemble_eliminate_and_solve`] wants is a whole RHS vector
/// rather than a `(dof, value)` list, because BT2's fixture feeds it a QoI's
/// `dual_load` output directly.
fn rhs_from_point_loads(n_nodes: usize, point_loads: &[(usize, f64)]) -> Vec<f64> {
    let mut f = vec![0.0_f64; 3 * n_nodes];
    for &(dof, val) in point_loads {
        f[dof] += val;
    }
    f
}

/// Assemble, eliminate and CG-solve a P1 tetrahedral FEA system, RETAINING
/// everything the dual solve needs.
///
/// Same pipeline shape as `aposteriori_validation.rs::solve_p1_pipeline`
/// (`element_stiffness` → `assemble_global_stiffness` →
/// `apply_dirichlet_row_elimination` → `solve_cg`), with one difference: it
/// returns `K`, the ELIMINATED RHS and the deduplicated BC list alongside
/// the `CgResult`, all three of which `solve_p1_pipeline` drops.
///
/// Each retained value has a caller in this file:
/// * `K` — the dual solve reuses the *already row-eliminated* primal matrix
///   verbatim (PRD §5.3), so no second assembly and no second elimination.
/// * the eliminated RHS — BT1's reciprocity identity is stated against `fᵀ
///   z_h` with `f` the eliminated RHS, not the raw one.
/// * the deduplicated BC list — the dual load is zeroed at exactly the DOFs
///   that were actually constrained, which is the deduplicated set.
///
/// `SolverMode::Deterministic` is fixed (not a parameter) so every fixture in
/// this file is bit-stable and CI-safe; `opts` IS a parameter because BT3's
/// patch fixture needs a tolerance tighter than the crate default (see its
/// test doc).
///
/// # Panics
///
/// If `raw_f.len() != 3 * nodes.len()`.
fn assemble_eliminate_and_solve(
    nodes: &[[f64; 3]],
    conns: &[[usize; 4]],
    mat: &IsotropicElastic,
    bcs: Vec<DirichletBc>,
    raw_f: &[f64],
    opts: CgSolverOptions,
) -> (
    SparseRowMat<usize, f64>,
    Vec<f64>,
    Vec<DirichletBc>,
    CgResult,
) {
    let n_nodes = nodes.len();
    assert_eq!(
        raw_f.len(),
        3 * n_nodes,
        "raw_f.len() = {} but the system has 3 * {n_nodes} = {} DOFs",
        raw_f.len(),
        3 * n_nodes,
    );

    let ke_list: Vec<ElementStiffness> = conns
        .iter()
        .map(|conn| {
            let elem_nodes: Vec<[f64; 3]> = conn.iter().map(|&i| nodes[i]).collect();
            element_stiffness(ElementOrder::P1, &elem_nodes, mat)
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

    let mut k = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);

    let mut f = raw_f.to_vec();
    let mut bcs = bcs;
    dedup_bcs(&mut bcs);
    apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

    let result = solve_cg(&k, &f, opts, SolverMode::Deterministic);

    (k, f, bcs, result)
}
