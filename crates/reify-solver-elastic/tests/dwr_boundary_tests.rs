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
    ElementStiffness, IsotropicElastic, LocalDisplacementQoi, LocalNormalStressQoi, P1TetMeshRef,
    QuantityOfInterest, SolverMode, apply_dirichlet_row_elimination, assemble_global_stiffness,
    element_stiffness, solve_cg, solve_dual_cg,
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

// ─── BT1: reciprocity (PRD §6 C2) ─────────────────────────────────────────

/// A coarse cantilever box pencil: `[0,lx] × [0,ly] × [0,lz]`, clamped on
/// `x = 0`, with a transverse shear resultant distributed over the free end.
///
/// Homogeneous Dirichlet data throughout, which is what lets BT1 state
/// reciprocity against the ELIMINATED right-hand side: with all prescribed
/// values zero, elimination leaves `f` zeroed at constrained DOFs and
/// otherwise untouched.
struct CantileverPencil {
    nodes: Vec<[f64; 3]>,
    conns: Vec<[usize; 4]>,
    k: SparseRowMat<usize, f64>,
    /// The ELIMINATED right-hand side — the `f` of `J(u_h) == fᵀz_h`.
    f: Vec<f64>,
    bcs: Vec<DirichletBc>,
    primal: CgResult,
    lx: f64,
    ly: f64,
    lz: f64,
}

fn cantilever_pencil(
    nx: usize,
    ny: usize,
    nz: usize,
    material: &IsotropicElastic,
    opts: CgSolverOptions,
) -> CantileverPencil {
    let (lx, ly, lz) = (4.0_f64, 1.0, 1.0);
    let (nodes, conns) = box_p1_mesh(lx, ly, lz, nx, ny, nz);
    let tol = 1e-9;
    let bcs = dirichlet_fix_face(&nodes, 0, 0.0, tol);
    let end = end_face_nodes(&nodes, lx, tol);
    let raw_f = rhs_from_point_loads(nodes.len(), &distributed_tip_load(&end, 1.0e-3));
    let (k, f, bcs, primal) =
        assemble_eliminate_and_solve(&nodes, &conns, material, bcs, &raw_f, opts);
    CantileverPencil {
        nodes,
        conns,
        k,
        f,
        bcs,
        primal,
        lx,
        ly,
        lz,
    }
}

/// BT1 / C2 — RECIPROCITY: `J(u_h) == fᵀz_h` for both shipped QoI kinds.
///
/// The identity `J(u_h) = gᵀu_h = zᵀ_h K u_h = fᵀz_h` is the load-bearing
/// claim of the whole dual formulation: it is what says the adjoint solve
/// really computes the sensitivity of `J` to the residual, and therefore
/// that weighting the residual by `z_h` estimates the error in `J` rather
/// than in some unrelated functional. A dual load assembled with a
/// transposed index, a wrong volume weight, or the wrong BC treatment still
/// produces a plausible `z_h` — but not one satisfying this identity.
///
/// # The tolerance is derived, not tuned
///
/// `10 · cg_tolerance · |J|`. Both sides come from CG solves converged to a
/// relative residual of `CgSolverOptions::tolerance`, so the identity can
/// only hold to that accuracy; the factor 10 covers both solves plus the
/// contraction. It is computed from the SAME `opts` the solves used, so
/// tightening the fixture's tolerance tightens the assertion automatically
/// rather than silently leaving slack behind.
///
/// # Fixture placement is binding, and was measured
///
/// * **LocalDisplacement** — ball at the tip, `direction = [0,−1,0]`, along
///   the load. Measured relative error 7.3e-12 / 1.3e-12 / 6.5e-15 / 6.8e-13
///   across the four pencils below, against a 1e-7 bound: margin ≥ 1e4×.
/// * **LocalNormalStress** — the ball MUST sit OFF the bending neutral
///   axis. Measured 1.0e-10 / 1.0e-10 / 3.1e-11 / 4.6e-10, margin ≥ 200×,
///   and identical at `E = 1` and `E = 200e9`. A ball CENTRED on the
///   neutral axis is degenerate: the `σ_xx` mean cancels across the axis,
///   `J` collapses to ~1e-10, and the relative identity becomes
///   meaningless — measured relative error ≈ 1.0 there, i.e. a fixture that
///   looks like a broken estimator.
///
/// That degeneracy is why each case asserts a NON-DEGENERACY floor on `|J|`
/// before testing reciprocity. A future edit that re-centres a ball then
/// fails as "degenerate fixture", naming the real problem, instead of as
/// "reciprocity broken", which would send a reader hunting through the dual
/// assembler.
///
/// # TDD red→green
///
/// **RED** (step-11): `solve_dual_cg` does not exist, so this fails to
/// COMPILE. **GREEN** (step-12).
#[test]
fn bt1_reciprocity_holds_for_both_qoi_kinds_across_pencils_and_material_scales() {
    let opts = CgSolverOptions::default();

    for (nx, ny, nz) in [(4, 1, 1), (6, 2, 2), (8, 2, 2), (10, 3, 3)] {
        for youngs_modulus in [1.0_f64, 200.0e9] {
            let material = IsotropicElastic {
                youngs_modulus,
                poisson_ratio: 0.3,
            };
            let p = cantilever_pencil(nx, ny, nz, &material, opts);
            let mesh = P1TetMeshRef {
                coords: &p.nodes,
                tets: &p.conns,
            };
            let u = p.primal.u();
            assert!(
                p.primal.converged,
                "{nx}x{ny}x{nz} @ E={youngs_modulus}: the PRIMAL solve must \
                 converge before reciprocity means anything",
            );

            // Tip ball, along the load. Displacement scale for the
            // non-degeneracy floor: the largest nodal |u| on the mesh.
            let u_scale = u.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
            let displacement: Box<dyn QuantityOfInterest> = Box::new(LocalDisplacementQoi {
                at: [p.lx, p.ly / 2.0, p.lz / 2.0],
                radius: 0.6 * p.ly,
                direction: [0.0, -1.0, 0.0],
            });
            // OFF the bending neutral axis (y = ly/2) — see the doc above.
            let normal_stress: Box<dyn QuantityOfInterest> = Box::new(LocalNormalStressQoi {
                at: [p.lx / 8.0, 0.15 * p.ly, p.lz / 2.0],
                radius: 0.35,
                normal: [1.0, 0.0, 0.0],
            });

            for (qoi, floor, label) in [
                (&displacement, 1e-3 * u_scale, "LocalDisplacement"),
                (&normal_stress, 1e-6 * youngs_modulus * u_scale, "LocalNormalStress"),
            ] {
                let case = format!("{label} on {nx}x{ny}x{nz} @ E={youngs_modulus}");

                let j = qoi
                    .evaluate(mesh, &material, u)
                    .unwrap_or_else(|e| panic!("{case}: J(u_h) must resolve, got {e}"));
                assert!(
                    j.abs() > floor,
                    "{case}: DEGENERATE FIXTURE — |J(u_h)| = {} is at or below \
                     the fixture's own scale floor {floor}, so the relative \
                     reciprocity check below would be meaningless. For the \
                     stress QoI this is what a ball re-centred on the bending \
                     neutral axis looks like; move it off-axis rather than \
                     loosening the bound.",
                    j.abs(),
                );

                let dual = solve_dual_cg(
                    &p.k,
                    qoi.as_ref(),
                    mesh,
                    &material,
                    u,
                    &p.bcs,
                    opts,
                    SolverMode::Deterministic,
                )
                .unwrap_or_else(|e| panic!("{case}: the dual solve must resolve, got {e}"));
                assert!(
                    dual.converged,
                    "{case}: the DUAL solve must converge too",
                );

                let f_dot_z: f64 = p.f.iter().zip(dual.u()).map(|(fi, zi)| fi * zi).sum();
                let bound = 10.0 * opts.tolerance * j.abs();
                assert!(
                    (j - f_dot_z).abs() <= bound,
                    "{case}: reciprocity J(u_h) == fᵀz_h must hold to \
                     10·cg_tolerance relative; J = {j}, fᵀz_h = {f_dot_z}, \
                     |difference| = {} > {bound}",
                    (j - f_dot_z).abs(),
                );
            }
        }
    }
}
