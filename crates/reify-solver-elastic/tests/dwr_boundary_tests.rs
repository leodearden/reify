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

use faer::sparse::SparseRowMat;

use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, CgResult, CgSolverOptions, DirichletBc, ElementOrder,
    ElementStiffness, IsotropicElastic, LocalDisplacementQoi, LocalNormalStressQoi, P1TetMeshRef,
    QuantityOfInterest, SolverMode, StressElement, ZzIndicator, apply_dirichlet_row_elimination,
    assemble_global_stiffness, compute_dual_weighted_indicator, compute_zz_indicator,
    element_stiffness, element_stress_p1, solve_cg, solve_dual_cg, tet_volume_p1,
};

use reify_solver_elastic::{
    AdaptiveEstimate, AdaptiveProblem, DORFLER_THETA, DualWeightedIndicator, QoiError, QoiEstimate,
    RefinementBudget, run_adaptive_refinement,
};
use reify_ir::{ElementOrderTag, VolumeConnectivity, VolumeMesh};

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

// ─── BT2/BT3 harness: one solve, both indicators ──────────────────────────

/// A completed primal+dual solve on one mesh, with everything both
/// indicators need.
struct DualSolve {
    nodes: Vec<[f64; 3]>,
    conns: Vec<[usize; 4]>,
    /// The ELIMINATED right-hand side.
    f: Vec<f64>,
    bcs: Vec<DirichletBc>,
    primal: CgResult,
    dual: CgResult,
}

/// Per-element `StressElement`s for a displacement field on this mesh.
///
/// Both indicators consume the same shape, so building them through one
/// function is what keeps the primal and dual sides of the contraction
/// strictly comparable — a difference in element ORDER or volume between the
/// two would silently pair up the wrong tensors.
fn stress_elements<'a>(
    nodes: &[[f64; 3]],
    conns: &'a [[usize; 4]],
    material: &IsotropicElastic,
    field: &[f64],
    stress: &'a mut Vec<[[f64; 3]; 3]>,
    volume: &'a mut Vec<f64>,
) -> Vec<StressElement<'a>> {
    stress.clear();
    volume.clear();
    for conn in conns {
        let elem_nodes = [
            nodes[conn[0]],
            nodes[conn[1]],
            nodes[conn[2]],
            nodes[conn[3]],
        ];
        stress.push(element_stress_p1(
            &elem_nodes,
            material,
            &gather_u_p1(field, conn),
        ));
        volume.push(tet_volume_p1(&elem_nodes));
    }
    conns
        .iter()
        .enumerate()
        .map(|(i, conn)| StressElement {
            connectivity: conn.as_slice(),
            stress: stress[i],
            volume: volume[i],
        })
        .collect()
}

/// The `VolumeMesh` the indicators read `n_nodes` from.
///
/// Only `vertices.len()` is consumed; the `f32` coordinates never enter the
/// arithmetic, which is why the f64 solve mesh is not degraded by passing
/// through here.
fn indicator_mesh(n_nodes: usize) -> VolumeMesh {
    VolumeMesh {
        vertices: vec![0.0_f32; 3 * n_nodes],
        connectivity: VolumeConnectivity::Tet {
            indices: Vec::new(),
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    }
}

/// `(Z-Z on the primal, dual-weighted from primal × dual)` for one solve.
fn dwr_and_zz_from_solve(
    solve: &DualSolve,
    material: &IsotropicElastic,
) -> (ZzIndicator, DualWeightedIndicator) {
    let mesh = indicator_mesh(solve.nodes.len());
    let (mut su, mut vu) = (Vec::new(), Vec::new());
    let primal = stress_elements(
        &solve.nodes,
        &solve.conns,
        material,
        solve.primal.u(),
        &mut su,
        &mut vu,
    );
    let (mut sz, mut vz) = (Vec::new(), Vec::new());
    let dual = stress_elements(
        &solve.nodes,
        &solve.conns,
        material,
        solve.dual.u(),
        &mut sz,
        &mut vz,
    );
    let zz = compute_zz_indicator(&primal, &mesh, material);
    let dwr = compute_dual_weighted_indicator(&primal, &dual, &mesh, material);
    (zz, dwr)
}

/// The Z-Z indicator of the DUAL field — the `η_z,K` of BT3's
/// Cauchy–Schwarz check.
fn zz_of_dual(solve: &DualSolve, material: &IsotropicElastic) -> ZzIndicator {
    let mesh = indicator_mesh(solve.nodes.len());
    let (mut s, mut v) = (Vec::new(), Vec::new());
    let dual = stress_elements(
        &solve.nodes,
        &solve.conns,
        material,
        solve.dual.u(),
        &mut s,
        &mut v,
    );
    compute_zz_indicator(&dual, &mesh, material)
}

/// Prescribe the full 3-DOF displacement `field(x)` on every node on any of
/// the six bounding planes of `[0,lx] × [0,ly] × [0,lz]`.
///
/// Ported from `tests/aposteriori_validation.rs`. Prescribing the exact
/// linear field on the WHOLE boundary is what makes the interior solution
/// that same field — P1 tets represent it exactly — giving the uniform
/// stress state BT3's patch exactness is stated against.
fn dirichlet_prescribe_boundary_field(
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
                bcs.push(DirichletBc {
                    dof: node * 3 + dof_idx,
                    value: val,
                });
            }
        }
    }
    bcs
}

/// A solve whose fixture also needs its raw dual load `g` inspected.
struct DualSolveWithLoad {
    solve: DualSolve,
    /// The QoI's raw `dual_load` output, BEFORE any BC zeroing.
    g: Vec<f64>,
    /// The ELIMINATED primal RHS.
    f: Vec<f64>,
    bcs: Vec<DirichletBc>,
}

/// BT2's `f = g` fixture: a clamped box whose PRIMAL load IS a QoI's own
/// dual load, at `F = 1`.
///
/// Constructible only because `dual_load` does not depend on `u`: `g` is
/// assembled against the mesh alone, then handed to the primal solve as its
/// right-hand side. Dirichlet data is homogeneous throughout, which is what
/// makes elimination degenerate to zeroing.
fn self_dual_box(material: &IsotropicElastic, opts: CgSolverOptions) -> DualSolveWithLoad {
    let (lx, ly, lz) = (2.0_f64, 1.0, 1.0);
    let (nodes, conns) = box_p1_mesh(lx, ly, lz, 4, 2, 2);
    let mesh = P1TetMeshRef {
        coords: &nodes,
        tets: &conns,
    };
    let qoi = LocalDisplacementQoi {
        at: [lx, ly / 2.0, lz / 2.0],
        radius: 0.6 * ly,
        direction: [0.0, -1.0, 0.0],
    };
    // `u` is unused by `dual_load`; a zero field makes that explicit.
    let zero = vec![0.0_f64; 3 * nodes.len()];
    let g = qoi
        .dual_load(mesh, material, &zero)
        .expect("the BT2 QoI must resolve on its own fixture");

    let bcs = dirichlet_fix_face(&nodes, 0, 0.0, 1e-9);
    let (k, f, bcs, primal) =
        assemble_eliminate_and_solve(&nodes, &conns, material, bcs, &g, opts.clone());
    let dual = solve_dual_cg(
        &k,
        &qoi,
        mesh,
        material,
        primal.u(),
        &bcs,
        opts,
        SolverMode::Deterministic,
    )
    .expect("the BT2 dual solve must resolve");

    DualSolveWithLoad {
        f: f.clone(),
        bcs: bcs.clone(),
        g,
        solve: DualSolve {
            nodes,
            conns,
            f,
            bcs,
            primal,
            dual,
        },
    }
}

/// BT3's constant-strain patch fixture: the unit cube with `u = (γ·y, 0, 0)`
/// prescribed on the whole boundary, plus a real dual for `qoi`.
///
/// 4³ cells: coarse enough to stay fast, fine enough that a ball of radius
/// ≤ 0.30 at the centre has only interior contributing nodes — the
/// precondition BT3 asserts.
fn patch_cube(
    material: &IsotropicElastic,
    gamma: f64,
    qoi: &LocalDisplacementQoi,
    opts: CgSolverOptions,
) -> DualSolveWithLoad {
    let (nodes, conns) = box_p1_mesh(1.0, 1.0, 1.0, 4, 4, 4);
    let mesh = P1TetMeshRef {
        coords: &nodes,
        tets: &conns,
    };
    let bcs = dirichlet_prescribe_boundary_field(&nodes, 1.0, 1.0, 1.0, 1e-9, |p| {
        [gamma * p[1], 0.0, 0.0]
    });
    let raw_f = vec![0.0_f64; 3 * nodes.len()];
    let (k, f, bcs, primal) =
        assemble_eliminate_and_solve(&nodes, &conns, material, bcs, &raw_f, opts.clone());

    let zero = vec![0.0_f64; 3 * nodes.len()];
    let g = qoi
        .dual_load(mesh, material, &zero)
        .expect("the BT3 QoI must resolve on the patch cube");
    let dual = solve_dual_cg(
        &k,
        qoi,
        mesh,
        material,
        primal.u(),
        &bcs,
        opts,
        SolverMode::Deterministic,
    )
    .expect("the BT3 dual solve must resolve");

    DualSolveWithLoad {
        f: f.clone(),
        bcs: bcs.clone(),
        g,
        solve: DualSolve {
            nodes,
            conns,
            f,
            bcs,
            primal,
            dual,
        },
    }
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
    // Captured before the by-value uses below: CgSolverOptions is Clone but
    // not Copy, and the bound must be derived from the SAME tolerance the
    // solves ran at.
    let cg_tolerance = opts.tolerance;

    for (nx, ny, nz) in [(4, 1, 1), (6, 2, 2), (8, 2, 2), (10, 3, 3)] {
        for youngs_modulus in [1.0_f64, 200.0e9] {
            let material = IsotropicElastic {
                youngs_modulus,
                poisson_ratio: 0.3,
            };
            let p = cantilever_pencil(nx, ny, nz, &material, opts.clone());
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
                    opts.clone(),
                    SolverMode::Deterministic,
                )
                .unwrap_or_else(|e| panic!("{case}: the dual solve must resolve, got {e}"));
                assert!(
                    dual.converged,
                    "{case}: the DUAL solve must converge too",
                );

                let f_dot_z: f64 = p.f.iter().zip(dual.u()).map(|(fi, zi)| fi * zi).sum();
                let bound = 10.0 * cg_tolerance * j.abs();
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

// ─── BT2: self-dual reduction (PRD §6 C3) ─────────────────────────────────

/// BT2 / C3 — with the primal load set to the QoI's OWN dual load (`f = g`,
/// `F = 1`), the dual solve reproduces the primal EXACTLY and the
/// dual-weighted indicator reduces to the Z-Z indicator.
///
/// This is the strongest available check that the goal-oriented machinery is
/// a genuine generalisation rather than a parallel implementation that merely
/// resembles one: in the self-dual case every stage — load assembly, BC
/// treatment, solve, recovery, contraction — must land on the energy-norm
/// answer to the BIT.
///
/// # Why a purpose-built fixture and not a cantilever pencil
///
/// BT1's pencils load the tip face with a DISTRIBUTED resultant, so no scalar
/// multiple of that `f` can equal a ball-mean QoI's `dual_load` — the two
/// have different supports. `f = g` therefore has to be built directly, by
/// taking a QoI's dual load and using it AS the primal load. That is
/// constructible only because `dual_load` is independent of `u` (pinned in
/// src/qoi.rs), so `g` can be assembled before any primal solve exists.
///
/// # Every claim here is bit-exact, and each has a reason
///
/// * The eliminated RHS equals `g` zeroed at the constrained DOFs, BITWISE.
///   With homogeneous data (`value = 0.0`) the column-into-RHS term of
///   `apply_dirichlet_row_elimination` contributes exactly `0`, so
///   elimination degenerates to zeroing. Measured true.
/// * `z_h` is bit-identical to `u_h`: both `solve_cg` calls receive
///   bit-identical `(K, rhs)`, and `solve_cg` is pure and deterministic under
///   `SolverMode::Deterministic` with a cold start. Measured true.
/// * `√(η_K)` is bit-identical to the Z-Z `η_e`. NEVER compare against
///   `zz.per_element[i].powi(2)` instead: `per_element` stores `η_e =
///   √(η_e²)` and squaring an already-rounded root is a lossy round-trip —
///   measured bit-exact on only 51 of 96 elements that way, against 96 of 96
///   (worst ulp difference 0) for the `√` form.
///
/// # This bit-exactness is a property of THIS configuration only
///
/// It holds because `F = 1`, the start is cold, and both solves share one
/// `(K, opts)`. Any other configuration — a scaled `F`, a warm start, a
/// re-assembled `K` — agrees only to `10·cg_tolerance` divided by the
/// relative magnitude of the resulting `Δσ`. Never describe that weaker case
/// as agreeing "to rounding".
///
/// # The sign claim is an energy inequality, not a convergence claim
///
/// `η_K ≥ 0` everywhere and `Σ η_K > 0` follow from the self-dual
/// contraction being a quadratic form in one tensor — the discrete
/// minimum-potential-energy inequality `fᵀu_h ≤ fᵀu`. Measured: 0 of 96
/// elements negative, `qoi_error_estimate` = 4.28. It says nothing about
/// bending lock or convergence rate.
///
/// # TDD red→green
///
/// **RED** (step-13): the `f = g` fixture builder and the
/// `dwr_and_zz_from_solve` harness do not exist. **GREEN** (step-14).
#[test]
fn bt2_a_self_dual_load_reproduces_the_primal_solve_and_the_zz_indicator_bitwise() {
    let material = IsotropicElastic {
        youngs_modulus: 1.0,
        poisson_ratio: 0.3,
    };
    let opts = CgSolverOptions::default();
    let fx = self_dual_box(&material, opts.clone());

    // (a) Elimination degenerated to zeroing, bitwise.
    let mut expected_rhs = fx.g.clone();
    for bc in &fx.bcs {
        expected_rhs[bc.dof] = 0.0;
    }
    assert_eq!(fx.f.len(), expected_rhs.len());
    for (i, (actual, want)) in fx.f.iter().zip(&expected_rhs).enumerate() {
        assert_eq!(
            actual.to_bits(),
            want.to_bits(),
            "RHS[{i}]: with homogeneous Dirichlet data, elimination must \
             reduce to zeroing g at the constrained DOFs; got {actual} vs \
             {want}",
        );
    }

    let (zz, dwr) = dwr_and_zz_from_solve(&fx.solve, &material);

    // (e) Non-vacuity: the primal field is not zero.
    let sum_abs_u: f64 = fx.solve.primal.u().iter().map(|x| x.abs()).sum();
    assert!(
        sum_abs_u > 0.0,
        "Σ|u_h| must be non-zero, or every bitwise claim below holds \
         vacuously on a zero field",
    );

    // (b) The dual solve reproduced the primal, bit for bit.
    let (u, z) = (fx.solve.primal.u(), fx.solve.dual.u());
    assert_eq!(u.len(), z.len());
    for (i, (ui, zi)) in u.iter().zip(z).enumerate() {
        assert_eq!(
            ui.to_bits(),
            zi.to_bits(),
            "DOF {i}: z_h must be BIT-identical to u_h when f = g; got {zi} \
             vs {ui}",
        );
    }

    // (c) The indicator reduced to Z-Z, bit for bit.
    assert_eq!(dwr.per_element_signed.len(), zz.per_element.len());
    for (i, (signed, eta)) in dwr
        .per_element_signed
        .iter()
        .zip(&zz.per_element)
        .enumerate()
    {
        assert_eq!(
            signed.sqrt().to_bits(),
            eta.to_bits(),
            "element {i}: √(η_K) must be bit-identical to the Z-Z η_e; got {} \
             vs {eta}",
            signed.sqrt(),
        );
    }

    // (d) The energy inequality.
    assert!(
        dwr.per_element_signed.iter().all(|&x| x >= 0.0),
        "a self-dual contraction is a quadratic form in one tensor, so no \
         η_K may be negative; {} of {} were",
        dwr.per_element_signed.iter().filter(|x| **x < 0.0).count(),
        dwr.per_element_signed.len(),
    );
    assert!(
        dwr.qoi_error_estimate > 0.0,
        "Σ η_K must be strictly positive here; got {}",
        dwr.qoi_error_estimate,
    );
}

// ─── BT3: constant-strain patch exactness with a real dual ────────────────

/// BT3 — on the Zienkiewicz constant-strain patch, every per-element
/// dual-weighted contribution vanishes against a REAL, non-zero dual.
///
/// The patch field `u = (γ·y, 0, 0)` is represented exactly by P1 tets, so
/// the recovered stress equals the discrete stress and the primal error is
/// identically zero. A correct estimator must then report zero error in ANY
/// quantity of interest — and the point of doing it against a real dual is
/// that a broken estimator returning a ZERO dual field would also report
/// zero. That silent failure is exactly what this PRD closes, which is why
/// `Σ|z_h| > 0` is asserted first (measured 4.89).
///
/// # Three measured preconditions, each binding
///
/// 1. **The fixture uses `tolerance: 1e-12`, not the crate default.** `η_K`
///    tracks the CG residual, not zero. At the default `1e-8` the measured
///    maximum `|η_K|` is 8.4e-12 — OVER the 1e-12 bound — while at `1e-12`
///    it is 1.1e-15, a ≥900× margin. The PRD's bound is achievable; it just
///    needs a fixture converged tightly enough to expose it.
/// 2. **The dual direction must be ALIGNED with the patch field.** `d =
///    [1,0,0]` gives `J = 5.0e-2` exactly (`= γ·0.5`, the analytic ball mean
///    of `γ·y` over a ball centred at `y = 0.5`). `d = [0,0,1]` makes
///    `J ≡ 0` identically and reciprocity degenerate — measured relative
///    error 0.99.
/// 3. **The dual load must touch no constrained DOF.** This fixture has
///    NON-homogeneous Dirichlet data, so BT1's derivation
///    (`J(u_h) = gᵀu_h = g_zeroedᵀu_h`) only licenses the reciprocity check
///    below if zeroing `g` at the constrained DOFs changes nothing. The test
///    ASSERTS that no-op bitwise rather than assuming it — measured true for
///    a ball at the cube centre with `radius ≤ 0.30` on a 4³ mesh, whose
///    contributing elements have only interior nodes.
///
/// The Cauchy–Schwarz check `|η_K| ≤ η_u,K · η_z,K` is scale-free and held on
/// every element of every measured configuration; it is asserted here as an
/// independent structural constraint on the bilinear form.
///
/// # TDD red→green
///
/// **RED** (step-13), **GREEN** (step-14) — as for BT2.
#[test]
fn bt3_the_constant_strain_patch_yields_zero_contributions_against_a_real_dual() {
    let material = IsotropicElastic {
        youngs_modulus: 1.0,
        poisson_ratio: 0.3,
    };
    // Precondition 1: tighter than the crate default, deliberately.
    let opts = CgSolverOptions {
        tolerance: 1e-12,
        max_iter: 5000,
    };
    let gamma = 0.1_f64;
    // Precondition 2: ALIGNED with the patch field's direction.
    let qoi = LocalDisplacementQoi {
        at: [0.5, 0.5, 0.5],
        radius: 0.30,
        direction: [1.0, 0.0, 0.0],
    };
    let fx = patch_cube(&material, gamma, &qoi, opts.clone());

    // Precondition 3: zeroing g at the constrained DOFs is a BITWISE no-op,
    // which is what licenses the reciprocity check on a non-homogeneous
    // fixture.
    for bc in &fx.solve.bcs {
        assert_eq!(
            fx.g[bc.dof].to_bits(),
            0.0_f64.to_bits(),
            "g[{}] = {} is non-zero at a CONSTRAINED DOF, so this fixture no \
             longer satisfies BT1's precondition; shrink the ball until its \
             contributing elements have only interior nodes",
            bc.dof,
            fx.g[bc.dof],
        );
    }

    // The dual is real and non-trivial — without this the test passes for a
    // zero dual, the silent failure the PRD exists to close.
    let sum_abs_z: f64 = fx.solve.dual.u().iter().map(|x| x.abs()).sum();
    assert!(
        sum_abs_z > 0.0,
        "Σ|z_h| must be non-zero; a zero dual would satisfy every assertion \
         below for the wrong reason",
    );
    assert!(fx.solve.primal.converged && fx.solve.dual.converged);

    // (b) Reciprocity on this fixture, with the analytic J as the anchor.
    let u = fx.solve.primal.u();
    let j = qoi
        .evaluate(
            P1TetMeshRef {
                coords: &fx.solve.nodes,
                tets: &fx.solve.conns,
            },
            &material,
            u,
        )
        .expect("the patch QoI must resolve");
    let analytic = gamma * 0.5;
    assert!(
        (j - analytic).abs() <= 1e-9 * analytic.abs(),
        "J must equal the analytic ball mean γ·0.5 = {analytic}, got {j}",
    );
    let f_dot_z: f64 = fx
        .solve
        .f
        .iter()
        .zip(fx.solve.dual.u())
        .map(|(fi, zi)| fi * zi)
        .sum();
    assert!(
        (j - f_dot_z).abs() <= 10.0 * opts.tolerance * j.abs(),
        "reciprocity must hold on the patch fixture too; J = {j}, fᵀz_h = \
         {f_dot_z}",
    );

    // (c) Patch exactness: every contribution vanishes.
    let (zz_u, dwr) = dwr_and_zz_from_solve(&fx.solve, &material);
    for (i, signed) in dwr.per_element_signed.iter().enumerate() {
        assert!(
            signed.abs() <= 1e-12,
            "element {i}: the primal error is identically zero on a patch \
             field, so η_K must vanish for any dual; got {signed}",
        );
    }

    // Cauchy–Schwarz on the bilinear form, element by element.
    let zz_z = zz_of_dual(&fx.solve, &material);
    for (i, signed) in dwr.per_element_signed.iter().enumerate() {
        let product = zz_u.per_element[i] * zz_z.per_element[i];
        assert!(
            signed.abs() <= product + 1e-15,
            "element {i}: Cauchy–Schwarz |η_K| <= η_u,K · η_z,K must hold; \
             got {} > {product}",
            signed.abs(),
        );
    }
}

// ─── §5.3: the dual is ALWAYS homogeneously constrained ───────────────────

/// `solve_dual_cg` zeroes `g` at every constrained DOF — the one line of
/// §5.3 that neither BT2 nor BT3 puts any load on.
///
/// Both of those fixtures place the QoI ball far from the clamped face, so
/// their `g` is already zero there (BT3 asserts exactly that, as the
/// precondition licensing its reciprocity check on non-homogeneous Dirichlet
/// data). Deleting the zeroing loop would leave both of them — and the rest
/// of the suite — green.
///
/// Here the ball is centred ON the clamped face, so `dual_load` puts real
/// weight on constrained DOFs. Row elimination leaves those rows as the
/// identity, which is what makes the second assertion a clean discriminator:
/// an unzeroed `g[dof]` comes straight back out as `z_h[dof] = g[dof]`, a
/// non-zero adjoint "displacement" at a DOF that is not an unknown.
///
/// The closing half covers the `# Errors` clause BT4 cannot reach: BT4
/// short-circuits on `evaluate`'s `?` before `solve_dual_cg` is ever called,
/// so "no dual solve is attempted" is pinned here instead.
#[test]
fn solve_dual_cg_zeroes_the_dual_load_at_constrained_dofs_and_refuses_an_unresolvable_qoi() {
    let material = IsotropicElastic {
        youngs_modulus: 1.0,
        poisson_ratio: 0.3,
    };
    let opts = CgSolverOptions::default();
    let (lx, ly, lz) = (2.0_f64, 1.0, 1.0);
    let (nodes, conns) = box_p1_mesh(lx, ly, lz, 4, 2, 2);
    let mesh = P1TetMeshRef {
        coords: &nodes,
        tets: &conns,
    };
    let tol = 1e-9;
    // ON the clamped face, unlike BT2's tip ball.
    let qoi = LocalDisplacementQoi {
        at: [0.0, ly / 2.0, lz / 2.0],
        radius: 0.6 * ly,
        direction: [0.0, -1.0, 0.0],
    };
    let end = end_face_nodes(&nodes, lx, tol);
    let raw_f = rhs_from_point_loads(nodes.len(), &distributed_tip_load(&end, 1.0e-3));
    let (k, _f, bcs, primal) = assemble_eliminate_and_solve(
        &nodes,
        &conns,
        &material,
        dirichlet_fix_face(&nodes, 0, 0.0, tol),
        &raw_f,
        opts.clone(),
    );

    // Premise: on THIS fixture the loop has something to do.
    let g = qoi
        .dual_load(mesh, &material, primal.u())
        .expect("the QoI must resolve on this fixture");
    let loaded: Vec<usize> = bcs
        .iter()
        .map(|bc| bc.dof)
        .filter(|&dof| g[dof] != 0.0)
        .collect();
    assert!(
        !loaded.is_empty(),
        "fixture premise: the RAW dual load must be non-zero at some \
         constrained DOF, or the zeroing loop has nothing to do here either \
         and this test is as vacuous as the ones it exists to complement",
    );

    let dual = solve_dual_cg(
        &k,
        &qoi,
        mesh,
        &material,
        primal.u(),
        &bcs,
        opts.clone(),
        SolverMode::Deterministic,
    )
    .expect("the dual solve must resolve");
    assert!(dual.converged, "the dual solve must converge on this fixture");
    for bc in &bcs {
        assert_eq!(
            dual.u()[bc.dof],
            0.0,
            "z_h[{}] must be zero at a constrained DOF — the adjoint of a \
             constrained problem is homogeneously constrained whatever the \
             primal prescribed — but the raw g carried {} there",
            bc.dof,
            g[bc.dof],
        );
    }

    // `# Errors`: an unresolvable QoI never reaches the solver.
    let outside = [10.0 * lx, ly / 2.0, lz / 2.0];
    let err = solve_dual_cg(
        &k,
        &LocalDisplacementQoi {
            at: outside,
            radius: 1e-3,
            direction: [0.0, -1.0, 0.0],
        },
        mesh,
        &material,
        primal.u(),
        &bcs,
        opts,
        SolverMode::Deterministic,
    )
    .expect_err("a QoI outside the body has no dual load to solve with");
    assert_eq!(
        err,
        QoiError::PointOutsideBody { at: outside },
        "the typed error must come back verbatim rather than as a zero g \
         solved to a zero adjoint field",
    );
}

/// A BC set that outruns the mesh is reported by name, not as a bare slice
/// index.
///
/// `solve_dual_cg` already asserts `k.nrows() == 3 · mesh.coords.len()`
/// precisely so the zeroing loop cannot index `g` out of bounds first — but
/// that assert cannot see THIS desync. `k` and `mesh` agree here perfectly;
/// it is `bcs` that disagrees, which is exactly the shape of the stale-state
/// case the function's own doc describes (a BC set carried over from a finer
/// pre-remesh mesh). Without the bound the next line panics inside the
/// standard library, naming neither the operator, the BC set nor the mesh.
///
/// The fixture keeps every real BC and appends one dof just past the last
/// one the mesh has, so the only thing wrong is the thing under test.
#[test]
#[should_panic(expected = "Dirichlet BC constrains dof")]
fn solve_dual_cg_names_a_bc_dof_beyond_the_mesh_rather_than_indexing_out_of_bounds() {
    let material = IsotropicElastic {
        youngs_modulus: 1.0,
        poisson_ratio: 0.3,
    };
    let opts = CgSolverOptions::default();
    let (lx, ly, lz) = (2.0_f64, 1.0, 1.0);
    let (nodes, conns) = box_p1_mesh(lx, ly, lz, 2, 1, 1);
    let mesh = P1TetMeshRef {
        coords: &nodes,
        tets: &conns,
    };
    let tol = 1e-9;
    let end = end_face_nodes(&nodes, lx, tol);
    let raw_f = rhs_from_point_loads(nodes.len(), &distributed_tip_load(&end, 1.0e-3));
    let (k, _f, mut bcs, primal) = assemble_eliminate_and_solve(
        &nodes,
        &conns,
        &material,
        dirichlet_fix_face(&nodes, 0, 0.0, tol),
        &raw_f,
        opts.clone(),
    );
    bcs.push(DirichletBc {
        dof: 3 * nodes.len(),
        value: 0.0,
    });

    let _ = solve_dual_cg(
        &k,
        &LocalDisplacementQoi {
            at: [lx, ly / 2.0, lz / 2.0],
            radius: 0.6 * ly,
            direction: [0.0, -1.0, 0.0],
        },
        mesh,
        &material,
        primal.u(),
        &bcs,
        opts,
        SolverMode::Deterministic,
    );
}

// ─── BT4 at the SEAM level: an unresolvable QoI ends the loop (§5.5) ───────

/// An [`AdaptiveProblem`] whose `refine` remeshes onto a SMALLER domain,
/// eventually moving the body out from under a coordinate-addressed QoI.
///
/// A compressed but faithful model of the failure §5.5 exists to handle: a
/// QoI is addressed by COORDINATES (C6) and re-resolved against every mesh
/// the loop produces, so a remesh can legitimately leave its point outside
/// the body. Real refinement does not shrink a domain, but it does replace
/// the mesh wholesale, and nothing guarantees the new one still covers the
/// query point.
///
/// `solve_and_estimate` does a REAL solve, a REAL dual solve and a REAL
/// dual-weighted indicator, so this also exercises β's pieces composing:
/// `solve_dual_cg` → `compute_dual_weighted_indicator` →
/// `marking_weights` → `AdaptiveEstimate`.
struct ShrinkingDomainProblem {
    lx: f64,
    ly: f64,
    lz: f64,
    material: IsotropicElastic,
    qoi: LocalDisplacementQoi,
    opts: CgSolverOptions,
    solves: usize,
    refines: usize,
}

impl AdaptiveProblem for ShrinkingDomainProblem {
    /// The QoI error IS the seam error here. γ widens the eval-side problems
    /// to a `RefineError | QoiError` union; β only needs the seam to admit a
    /// non-`Infallible` error at all.
    type Error = QoiError;

    fn solve_and_estimate(&mut self) -> Result<AdaptiveEstimate, Self::Error> {
        self.solves += 1;
        let (nodes, conns) = box_p1_mesh(self.lx, self.ly, self.lz, 4, 2, 2);
        let mesh = P1TetMeshRef {
            coords: &nodes,
            tets: &conns,
        };
        let tol = 1e-9;
        let bcs = dirichlet_fix_face(&nodes, 0, 0.0, tol);
        let end = end_face_nodes(&nodes, self.lx, tol);
        let raw_f = rhs_from_point_loads(nodes.len(), &distributed_tip_load(&end, 1.0e-3));
        let (k, f, bcs, primal) = assemble_eliminate_and_solve(
            &nodes,
            &conns,
            &self.material,
            bcs,
            &raw_f,
            self.opts.clone(),
        );

        // THE EXIT: on a mesh that no longer covers the QoI's point this
        // returns Err, and `?` carries it out of the loop.
        let value = self.qoi.evaluate(mesh, &self.material, primal.u())?;

        let dual = solve_dual_cg(
            &k,
            &self.qoi,
            mesh,
            &self.material,
            primal.u(),
            &bcs,
            self.opts.clone(),
            SolverMode::Deterministic,
        )?;

        let solve = DualSolve {
            nodes,
            conns,
            f,
            bcs,
            primal,
            dual,
        };
        let (_, dwr) = dwr_and_zz_from_solve(&solve, &self.material);

        Ok(AdaptiveEstimate {
            // A QoI-RELATIVE error: the dimensionless quantity §5.5 has the
            // loop compare against `target_accuracy` on the goal-oriented
            // path, in place of the energy-norm ratio.
            //
            // The zero-J guard is part of what this stub models for real
            // implementations: `J(u_h) == 0` is ordinary for a QoI (a
            // displacement projected on a direction at a symmetry point is
            // exactly zero), and the raw ratio is then `inf` or `NaN` — a
            // value that takes NEITHER the target nor the stall exit, so the
            // loop would burn its whole budget on garbage. See
            // `AdaptiveEstimate::relative_error`. With no scale to normalise
            // by, the bound itself is the conservative finite stand-in: it
            // never reads as converged.
            relative_error: if value.abs() > 0.0 {
                dwr.qoi_error_bound / value.abs()
            } else {
                dwr.qoi_error_bound
            },
            per_element: dwr.marking_weights(),
            n_dofs: 3 * solve.nodes.len(),
            qoi: Some(QoiEstimate {
                value,
                error_estimate: dwr.qoi_error_estimate,
                error_bound: dwr.qoi_error_bound,
            }),
        })
    }

    fn refine(&mut self, _marked: &[usize]) -> Result<(), Self::Error> {
        self.refines += 1;
        self.lx *= 0.2;
        self.ly *= 0.2;
        self.lz *= 0.2;
        Ok(())
    }
}

/// BT4 / §5.5 — a QoI that resolves on the seed mesh and becomes
/// unresolvable on a later one ends `run_adaptive_refinement` with the typed
/// `QoiError`, after the earlier iterations have run.
///
/// This is the whole point of making `solve_and_estimate` fallible. Before
/// it, an estimator in this position had two options, both bad: panic —
/// taking down a solve the user may have waited minutes for, with no partial
/// result — or invent a number, which for an ERROR ESTIMATE means reporting
/// a small value, which the loop reads as convergence. The refinement then
/// stops early and reports success on a mesh nobody checked.
///
/// The assertions pin all three halves of the claim: the seed mesh really
/// does resolve (so the failure is caused by the remesh, not by a QoI that
/// never worked); the earlier iteration really ran; and the error that comes
/// out is the QoI's own typed value, not a status or a panic.
#[test]
fn bt4_a_qoi_that_stops_resolving_mid_run_ends_the_loop_with_its_typed_error() {
    let at = [1.0, 0.5, 0.5];
    let mut problem = ShrinkingDomainProblem {
        lx: 2.0,
        ly: 1.0,
        lz: 1.0,
        material: IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.3,
        },
        qoi: LocalDisplacementQoi {
            at,
            radius: 0.3,
            direction: [0.0, -1.0, 0.0],
        },
        opts: CgSolverOptions::default(),
        solves: 0,
        refines: 0,
    };

    // Non-vacuity: the QoI resolves on the SEED mesh, so the failure below is
    // caused by the remesh rather than by a QoI that never worked.
    let seed = problem
        .solve_and_estimate()
        .expect("the QoI must resolve on the seed mesh");
    let seed_qoi = seed
        .qoi
        .as_ref()
        .expect("the stub fills `qoi` unconditionally");
    assert!(
        seed_qoi.value != 0.0 && seed.relative_error.is_finite(),
        "the seed solve must produce a USABLE goal-oriented estimate: a zero \
         J(u_h) makes the QoI-relative error non-finite, and the loop below \
         would then take neither the target nor the stall exit and reach its \
         second solve for the wrong reason; got J = {}, relative_error = {}",
        seed_qoi.value,
        seed.relative_error,
    );
    problem.solves = 0;

    // `target_accuracy` is unreachable, so the loop always refines and
    // reaches the second solve rather than converging first.
    let budget = RefinementBudget {
        target_accuracy: 1e-30,
        max_refinement_iterations: 5,
        max_dofs: usize::MAX,
    };
    let outcome = run_adaptive_refinement(&mut problem, &budget, DORFLER_THETA);

    assert_eq!(
        outcome,
        Err(QoiError::PointOutsideBody { at }),
        "the loop must surface the QoI's OWN typed error, not a status and \
         not a panic",
    );
    assert_eq!(
        (problem.solves, problem.refines),
        (2, 1),
        "the first iteration must have completed — solve, mark, refine — \
         before the second solve failed",
    );
}
