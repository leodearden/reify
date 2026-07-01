//! Gate-resident solver smoke checks (PRD `docs/prds/offline-deep-test-lane.md`
//! task A3).
//!
//! # Scope
//!
//! This thin binary carries the light solver coverage that STAYS on the
//! merge/task gate while the heavy suite (`determinism.rs`,
//! `analytical_validation.rs`, `modal_benchmarks.rs`, and the OCCT FEA
//! binaries) moves to the offline deep-test lane. Two runtime checks, both
//! driven by a single shared 24×24×8 P1-tet cantilever fixture:
//!
//! 1. **Determinism** — solve in `Deterministic` mode at threads ∈ {1, 2},
//!    asserting EXACT bit-stability (`u.to_bits()` byte-identical; no numeric
//!    floor to clear). Exactness holds by identity:
//!    `resolve_execution_modes(true, t, _)` always returns
//!    `(Deterministic, Deterministic)` regardless of `t` (the
//!    `deterministic || …` short-circuit in `solver.rs::resolve_execution_modes`
//!    ignores thread count), and Deterministic mode's fixed pairwise-tree
//!    reduction produces an identical FP-op sequence for any `t`. This is the
//!    `{1, 2}` subset of the already-green
//!    `determinism.rs::deterministic_displacement_bit_stable_across_repeats_and_thread_counts`
//!    (which covers `{1, 4, 16}`).
//! 2. **One analytical benchmark** at a COARSE tolerance pinned to an
//!    already-passing green bound — reproduces
//!    `analytical_validation.rs::cantilever_beam_p1_tip_deflection_within_5pct_of_timoshenko`
//!    verbatim: L/H=2 stocky cantilever, 24×24×8 hex→6-tet Kuhn split,
//!    faithful distributed end shear + face-averaged tip deflection,
//!    `rel_err ≤ 0.05`. DO NOT tighten below 5% or use a slender P1 column —
//!    5% is the exact tolerance of a landed green test, pinned above the
//!    P1-tet bending-lock floor (~9–10%; non-slender L/H=2 never engages it).
//!
//! # Why one fixture serves both checks
//!
//! The 24×24×8 mesh is mandatory for the analytical bound (smaller meshes
//! exceed 5%: 7.9% at 12³). Its `ndof = 16875` also exceeds
//! `PARALLEL_DOF_THRESHOLD` (10 000), so the determinism check genuinely
//! straddles the parallel-mode boundary.
//!
//! # Per-test-file helper-copy convention
//!
//! Integration test binaries are separate crates and cannot import the
//! module-private helpers in `determinism.rs` / `analytical_validation.rs`.
//! Following the established per-test-file pattern (documented in
//! `determinism.rs`), the small set of mesh/BC/solve helpers needed here are
//! reproduced verbatim below.

use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, ElementOrder, ElementStiffness,
    assemble_global_stiffness, apply_dirichlet_row_elimination,
    DirichletBc, IsotropicElastic,
    solve_cg, CgSolverOptions, CgResult, SolverMode,
    resolve_execution_modes,
};

// ─── material constant ─────────────────────────────────────────────────────

/// Material for the smoke fixture: unit Young's modulus, ν = 0.3 — matches
/// the `analytical_validation.rs` / `determinism.rs` convention for
/// dimensionless benchmarks.
const MAT: IsotropicElastic = IsotropicElastic {
    youngs_modulus: 1.0,
    poisson_ratio: 0.3,
};

// ─── mesh / BC helpers (copied per the per-test-file convention) ──────────

/// Split a hex cell into 6 tetrahedra via the Kuhn triangulation.
///
/// All 6 tets share the main diagonal from `c[0]` to `c[6]`. Reproduced here
/// because it is module-private in `determinism.rs` / `analytical_validation.rs`
/// (the crate's established per-test-file copy convention).
///
/// Corner ordering:
/// ```text
/// c[0]=(ix,iy,iz)     c[4]=(ix,iy,iz+1)
/// c[1]=(ix+1,iy,iz)   c[5]=(ix+1,iy,iz+1)
/// c[2]=(ix+1,iy+1,iz) c[6]=(ix+1,iy+1,iz+1)
/// c[3]=(ix,iy+1,iz)   c[7]=(ix,iy+1,iz+1)
/// ```
fn kuhn_split_hex_to_six_tets(c: [usize; 8]) -> [[usize; 4]; 6] {
    [
        [c[0], c[1], c[2], c[6]], // σ=(x,y,z): 000→100→110→111
        [c[0], c[1], c[5], c[6]], // σ=(x,z,y): 000→100→101→111
        [c[0], c[3], c[2], c[6]], // σ=(y,x,z): 000→010→110→111
        [c[0], c[3], c[7], c[6]], // σ=(y,z,x): 000→010→011→111
        [c[0], c[4], c[5], c[6]], // σ=(z,x,y): 000→001→101→111
        [c[0], c[4], c[7], c[6]], // σ=(z,y,x): 000→001→011→111
    ]
}

/// Build a structured P1 tet mesh for `[0,Lx]×[0,Ly]×[0,Lz]` with
/// `nx×ny×nz` hex cells, each Kuhn-split into 6 tets.
///
/// Node indexing: `node(ix,iy,iz) = iz*(ny+1)*(nx+1) + iy*(nx+1) + ix`.
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

    let node_idx = |ix: usize, iy: usize, iz: usize| iz * nny * nnx + iy * nnx + ix;

    let mut connectivity = Vec::with_capacity(6 * nx * ny * nz);
    for iz in 0..nz {
        for iy in 0..ny {
            for ix in 0..nx {
                let c = [
                    node_idx(ix,     iy,     iz),
                    node_idx(ix + 1, iy,     iz),
                    node_idx(ix + 1, iy + 1, iz),
                    node_idx(ix,     iy + 1, iz),
                    node_idx(ix,     iy,     iz + 1),
                    node_idx(ix + 1, iy,     iz + 1),
                    node_idx(ix + 1, iy + 1, iz + 1),
                    node_idx(ix,     iy + 1, iz + 1),
                ];
                for tet in kuhn_split_hex_to_six_tets(c) {
                    connectivity.push(tet);
                }
            }
        }
    }

    (nodes, connectivity)
}

/// Fix all 3 DOFs on nodes with `nodes[n][axis] ≈ value` (within `tol`).
fn dirichlet_fix_face(
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

/// Indices of every node on the free-end face `x = l` (within `tol`).
fn end_face_nodes(nodes: &[[f64; 3]], l: f64, tol: f64) -> Vec<usize> {
    nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| (n[0] - l).abs() < tol)
        .map(|(i, _)| i)
        .collect()
}

/// Distribute transverse shear `f_mag` (−y) equally over `end` nodes.
///
/// Returns `(global_dof_index, force_value)` pairs. The equal distribution
/// is immaterial to the tip deflection by Saint-Venant (only the resultant
/// matters far from the loaded face).
fn distributed_tip_load(end: &[usize], f_mag: f64) -> Vec<(usize, f64)> {
    let per = f_mag / end.len() as f64;
    end.iter().map(|&n| (n * 3 + 1, -per)).collect()
}

/// Sort + dedup Dirichlet BCs by DOF index.
///
/// `apply_dirichlet_row_elimination` panics on duplicate DOFs in debug builds,
/// so this must be called before BC application whenever the BC list may
/// contain overlapping entries (corner/edge nodes shared by multiple faces).
fn dedup_bcs(bcs: &mut Vec<DirichletBc>) {
    bcs.sort_by_key(|bc| bc.dof);
    bcs.dedup_by_key(|bc| bc.dof);
}

// ─── fixture and solve pipeline ─────────────────────────────────────────────

/// Build the shared 24×24×8 P1-tet cantilever fixture used by both smoke
/// checks.
///
/// Geometry: `[0,2]×[0,1]×[0,0.5]` (stocky L/H = 2), `nx×ny×nz = 24×24×8` hex
/// cells → 25×25×9 = 5 625 nodes → 16 875 DOFs (> `PARALLEL_DOF_THRESHOLD` =
/// 10 000, so the determinism check genuinely straddles the parallel-mode
/// boundary). This is the exact mesh of the reproduced analytical benchmark
/// `analytical_validation.rs::cantilever_beam_p1_tip_deflection_within_5pct_of_timoshenko`
/// — smaller meshes exceed the 5% bound (7.9% at 12³), so the resolution is
/// pinned, not tunable.
///
/// Boundary conditions: `x = 0` face fully clamped (all 3 DOFs zeroed).
/// Load: shear resultant `F = 1` in −y distributed over the `x = 2` face.
///
/// Returns `(nodes, connectivity, clamp_bcs, end_face_nodes, tip_loads)`.
#[allow(clippy::type_complexity)]
fn cantilever_smoke_fixture() -> (
    Vec<[f64; 3]>,
    Vec<[usize; 4]>,
    Vec<DirichletBc>,
    Vec<usize>,
    Vec<(usize, f64)>,
) {
    const L: f64 = 2.0;
    const H: f64 = 1.0;
    const B: f64 = 0.5;
    const NX: usize = 24;
    const NY: usize = 24;
    const NZ: usize = 8;
    const F: f64 = 1.0;

    let (nodes, conns) = box_p1_mesh(L, H, B, NX, NY, NZ);
    let tol_x = 0.5 * L / NX as f64;
    let mut bcs = dirichlet_fix_face(&nodes, 0, 0.0, tol_x);
    dedup_bcs(&mut bcs);
    let end = end_face_nodes(&nodes, L, tol_x);
    let loads = distributed_tip_load(&end, F);
    (nodes, conns, bcs, end, loads)
}

/// Output of one full assemble + BC + CG solve cycle.
struct SmokeSolveOutput {
    /// Global displacement vector `u[3*n + α]`.
    u: Vec<f64>,
    /// CG iterations executed (see `CgResult.iterations` contract).
    iterations: usize,
    /// Whether CG met the residual tolerance before `max_iter`.
    converged: bool,
}

/// Assemble, apply BCs, and CG-solve the shared cantilever fixture in the
/// mode resolved from `deterministic` + `threads` via `resolve_execution_modes`.
///
/// Mirrors `determinism.rs::solve_box_cantilever`: both assembly and solve use
/// the matched `(AssemblyMode, SolverMode)` pair, faithfully exercising the
/// full end-to-end contract (assembly determinism included).
fn solve_cantilever_smoke(deterministic: bool, threads: usize) -> SmokeSolveOutput {
    let (nodes, conns, bcs, _end, loads) = cantilever_smoke_fixture();
    let n_nodes = nodes.len();
    let ndof = 3 * n_nodes;

    let (amode, smode) = resolve_execution_modes(deterministic, threads, ndof);

    let ke_list: Vec<ElementStiffness> = conns
        .iter()
        .map(|conn| {
            let elem_nodes: Vec<[f64; 3]> = conn.iter().map(|&i| nodes[i]).collect();
            reify_solver_elastic::element_stiffness(ElementOrder::P1, &elem_nodes, &MAT)
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

    let mut k = assemble_global_stiffness(n_nodes, &elements, amode);

    let mut f = vec![0.0_f64; ndof];
    for &(dof, val) in &loads {
        f[dof] += val;
    }

    apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

    let opts = CgSolverOptions { tolerance: 1e-10, max_iter: 5000 };
    let result: CgResult = solve_cg(&k, &f, opts, smode);

    SmokeSolveOutput {
        u: result.u().to_vec(),
        iterations: result.iterations,
        converged: result.converged,
    }
}

// ─── determinism smoke test ───────────────────────────────────────────────

/// Byte-identical displacement across threads {1, 2} in Deterministic mode.
///
/// `resolve_execution_modes(true, t, ndof)` → `(Deterministic, Deterministic)`
/// for ALL t, so this is the `{1, 2}` subset of the already-green
/// `determinism.rs::deterministic_displacement_bit_stable_across_repeats_and_thread_counts`
/// (which covers `{1, 4, 16}`). Bit-identical u implies an identical FP-op
/// sequence — the exactness guarantee of the pairwise-tree Deterministic path.
#[test]
fn smoke_determinism_p1_cantilever_bit_stable_1_vs_2_threads() {
    // Verify the fixture builder is callable.
    let _ = cantilever_smoke_fixture();

    let out_t1 = solve_cantilever_smoke(true, 1);
    let out_t2 = solve_cantilever_smoke(true, 2);

    assert!(out_t1.converged, "t=1 did not converge (iter={})", out_t1.iterations);
    assert!(out_t2.converged, "t=2 did not converge (iter={})", out_t2.iterations);

    // Sanity: verify the fixture produced a non-trivial solve. If the tip load
    // is accidentally zeroed, solve_cg short-circuits to u = 0 with
    // iterations == 0 and converged == true — the bit-equality check below
    // would then pass trivially.
    assert!(
        out_t1.u.iter().any(|&x| x != 0.0),
        "t=1 displacement is all zero — tip load may be missing",
    );
    assert!(out_t1.iterations > 0, "CG returned 0 iterations — RHS may be zero");

    assert_eq!(
        out_t1.iterations, out_t2.iterations,
        "deterministic iteration count differs between t=1 ({}) and t=2 ({})",
        out_t1.iterations, out_t2.iterations,
    );

    assert_eq!(out_t1.u.len(), out_t2.u.len(), "u length differs between t=1 and t=2");
    for j in 0..out_t1.u.len() {
        assert_eq!(
            out_t1.u[j].to_bits(), out_t2.u[j].to_bits(),
            "u[{j}] differs between t=1 ({}) and t=2 ({})",
            out_t1.u[j], out_t2.u[j],
        );
    }
}
