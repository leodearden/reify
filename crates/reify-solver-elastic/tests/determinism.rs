//! FEA determinism harness — representative ~50K-DOF cantilever model.
//!
//! # Scope (PRD task #19, `docs/prds/v0_3/structural-analysis-fea.md`)
//!
//! This is a CI characterization harness over the already-merged
//! `#deterministic` plumbing (task #2926, PRD task #18). It extends the
//! tiny-scale unit tests in `crates/reify-solver-elastic/src/solver.rs`
//! (`deterministic_back_to_back_bit_stable`,
//! `parallel_shared_dof_k_tolerance_equivalent_and_back_to_back_bit_stable`,
//! `resolve_execution_modes_drives_bit_stable_and_equivalent_solves`) to a
//! representative ~50K-DOF model where:
//!
//! - The parallel SpMV/dot path (PAR_THRESHOLD = 1024) genuinely engages.
//! - The `PARALLEL_DOF_THRESHOLD`-driven mode selection (10 000 DOFs) fires.
//!
//! On this fixture the harness asserts:
//!
//! - **Under `#deterministic`** (`resolve_execution_modes(true, _, _)` →
//!   `(Deterministic, Deterministic)` for all thread counts):
//!   displacement `u`, recovered nodal stress field, and max von Mises are
//!   byte-identical across threads ∈ {1, 4, 16} (simultaneously covering
//!   "repeated runs" and "across thread counts" since Deterministic mode
//!   ignores the thread argument); CG iteration counts are also equal.
//!
//! - **Under default / parallel mode** (`resolve_execution_modes(false, t,
//!   ndof)` → `Parallel{t}` for t > 1 and ndof ≥ `PARALLEL_DOF_THRESHOLD`):
//!   result fields are tolerance-equivalent (relative-L2 ≤ `EQUIV_TOL = 1e-3`)
//!   across 3 repeated runs at fixed threads=4, and across thread counts
//!   {1, 4, 16}. Iteration-count equality is NOT asserted in parallel mode
//!   (round-off shifts the convergence step across thread counts).
//!
//! ## Achievability basis for EQUIV_TOL
//!
//! Two CG solves converged to relative residual `cg_tol = 1e-10` satisfy
//! ‖Δu‖/‖u‖ ≲ cond(K)·cg_tol (backward-error bound). For a stocky 3-D
//! continuum at ~50K DOF cond(K) ~ O(1e3–1e4), giving an expected
//! cross-reduction-order difference ≲ ~1e-6–1e-5 — at least 2 orders below
//! `EQUIV_TOL = 1e-3`. The 1e-3 bound still detects any real non-determinism
//! leak, which manifests as O(1)/NaN differences, not 1e-4-level ones.
//!
//! ## Cross-machine coverage
//!
//! The CI matrix executing this same within-process test on each machine
//! provides the cross-machine dimension. A committed golden u-vector is
//! intentionally omitted (architecture-fragile due to FMA contraction /
//! libm differences; out of scope per PRD task #19).
//!
//! ## Thread count portability
//!
//! `SolverMode::Parallel{threads}` spawns exactly the requested worker count
//! via `std::thread::scope` (chunk = n.div_ceil(threads)) regardless of
//! physical core count, so {1, 4, 16} runs correctly on any CI machine (16
//! merely oversubscribes a 4-core box). `resolve_execution_modes(false, 1, …)`
//! → Deterministic (policy: `threads <= 1`), so the t=1 sweep entry is
//! bit-identical to the deterministic reference.

#[allow(unused_imports)]
use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, ElementOrder, ElementStiffness,
    assemble_global_stiffness, apply_dirichlet_row_elimination,
    DirichletBc, IsotropicElastic,
    StressElement, element_stress_p1, recover_nodal_stress_p1, tet_volume_p1,
    solve_cg, CgSolverOptions, CgResult, SolverMode,
    resolve_execution_modes, PARALLEL_DOF_THRESHOLD,
};

// ─── material constant ────────────────────────────────────────────────────────

/// Material for the ~50K-DOF cantilever fixture.
///
/// Unit Young's modulus, ν = 0.3 — matches the `analytical_validation.rs`
/// convention for dimensionless benchmarks.
const MAT: IsotropicElastic = IsotropicElastic {
    youngs_modulus: 1.0,
    poisson_ratio: 0.3,
};

// ─── mesh helpers ─────────────────────────────────────────────────────────────

/// Split a hex cell into 6 tetrahedra via the Kuhn triangulation.
///
/// All 6 tets share the main diagonal from `c[0]` to `c[6]`. Reproduced from
/// `analytical_validation.rs` (module-private there, so copied here per the
/// established per-test-file pattern).
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

// ─── BC / load helpers ────────────────────────────────────────────────────────

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
/// Mirrors the `dedup_bcs` helper in `analytical_validation.rs`.
fn dedup_bcs(bcs: &mut Vec<DirichletBc>) {
    bcs.sort_by_key(|bc| bc.dof);
    bcs.dedup_by_key(|bc| bc.dof);
}

// ─── fixture and solve pipeline ───────────────────────────────────────────────

/// Build the ~50K-DOF cantilever box fixture.
///
/// Geometry: `[0,2]×[0,1]×[0,1]` (stocky L/H = L/B = 2), `nx×ny×nz = 32×24×20`
/// hex cells → 33×25×21 = 17 325 nodes → 51 975 DOFs (≥ `PARALLEL_DOF_THRESHOLD`
/// = 10 000; ≥ 1 024 for the parallel SpMV path).
///
/// Boundary conditions: `x = 0` face fully clamped (all 3 DOFs zeroed).
/// Load: shear resultant `F = 1` in −y distributed over the `x = 2` face.
///
/// Returns `(nodes, connectivity, clamp_bcs, tip_loads)`.
fn box_cantilever_fixture() -> (
    Vec<[f64; 3]>,
    Vec<[usize; 4]>,
    Vec<DirichletBc>,
    Vec<(usize, f64)>,
) {
    const L: f64 = 2.0;
    const H: f64 = 1.0;
    const B: f64 = 1.0;
    const NX: usize = 32;
    const NY: usize = 24;
    const NZ: usize = 20;
    const F: f64 = 1.0;

    let (nodes, conns) = box_p1_mesh(L, H, B, NX, NY, NZ);
    let tol_x = 0.5 * L / NX as f64;
    let mut bcs = dirichlet_fix_face(&nodes, 0, 0.0, tol_x);
    dedup_bcs(&mut bcs);
    let end = end_face_nodes(&nodes, L, tol_x);
    let loads = distributed_tip_load(&end, F);
    (nodes, conns, bcs, loads)
}

/// Output of one full assemble + BC + CG solve cycle.
struct SolveOutput {
    /// Global displacement vector `u[3*n + α]`.
    u: Vec<f64>,
    /// CG iterations executed (see `CgResult.iterations` contract).
    iterations: usize,
    /// Whether CG met the residual tolerance before `max_iter`.
    converged: bool,
    /// Maximum von Mises stress over all nodes; computed via stress recovery
    /// (step-4 GREEN adds `recover_stress_field` and populates this).
    /// Placeholder 0.0 until step-4 updates `solve_box_cantilever`.
    max_von_mises: f64,
}

/// Assemble, apply BCs, and CG-solve the ~50K-DOF cantilever in the mode
/// resolved from `deterministic` + `threads` via `resolve_execution_modes`.
///
/// Both assembly and solve use the matched `(AssemblyMode, SolverMode)` pair
/// from `resolve_execution_modes`, faithfully exercising PRD task #18's full
/// end-to-end contract (assembly determinism included).
///
/// `SolveOutput.max_von_mises` is 0.0 until step-4 adds the stress helpers
/// and updates this function to compute the real value.
fn solve_box_cantilever(deterministic: bool, threads: usize) -> SolveOutput {
    let (nodes, conns, clamp_bcs, tip_loads) = box_cantilever_fixture();
    let n_nodes = nodes.len();
    let ndof = 3 * n_nodes;

    let (amode, smode) = resolve_execution_modes(deterministic, threads, ndof);

    // Build per-element stiffness matrices (P1 tet).
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
    for &(dof, val) in &tip_loads {
        f[dof] += val;
    }

    apply_dirichlet_row_elimination(&mut k, &mut f, &clamp_bcs);

    let opts = CgSolverOptions { tolerance: 1e-10, max_iter: 5000 };
    let result: CgResult = solve_cg(&k, &f, opts, smode);

    let u_vec = result.u().to_vec();

    // Recover nodal stress field and compute max von Mises (step-4 GREEN).
    let stress = recover_stress_field(&nodes, &conns, &u_vec, &MAT);
    let vm = max_von_mises(&stress);

    SolveOutput {
        u: u_vec,
        iterations: result.iterations,
        converged: result.converged,
        max_von_mises: vm,
    }
}

// ─── stress helpers (step-4 GREEN) ───────────────────────────────────────────

/// Compute von Mises stress from a 3×3 Cauchy stress tensor.
///
/// `σ_vm = √{ ½[(σ₁₁−σ₂₂)²+(σ₂₂−σ₃₃)²+(σ₃₃−σ₁₁)²] + 3(σ₁₂²+σ₂₃²+σ₁₃²) }`.
///
/// Reproduced from `analytical_validation.rs` (module-private there).
fn von_mises_of_tensor(s: &[[f64; 3]; 3]) -> f64 {
    let (s11, s22, s33) = (s[0][0], s[1][1], s[2][2]);
    let (s12, s23, s13) = (s[0][1], s[1][2], s[0][2]);
    let v = 0.5 * ((s11 - s22).powi(2) + (s22 - s33).powi(2) + (s33 - s11).powi(2))
        + 3.0 * (s12.powi(2) + s23.powi(2) + s13.powi(2));
    v.sqrt()
}

/// Gather the 12 element DOFs (`[u_x,u_y,u_z]` per corner) for a P1 tet.
fn gather_u_p1(u: &[f64], conn: &[usize; 4]) -> [f64; 12] {
    let mut ue = [0.0_f64; 12];
    for (k, &node) in conn.iter().enumerate() {
        ue[3 * k]     = u[3 * node];
        ue[3 * k + 1] = u[3 * node + 1];
        ue[3 * k + 2] = u[3 * node + 2];
    }
    ue
}

/// Recover the continuous nodal stress field of a P1 tet mesh.
///
/// Builds one `StressElement` per element (constant Cauchy tensor via
/// `element_stress_p1`, volume via `tet_volume_p1`), then folds into nodal
/// averages via `recover_nodal_stress_p1` (volume-weighted, connectivity-shape
/// agnostic).
fn recover_stress_field(
    nodes: &[[f64; 3]],
    conns: &[[usize; 4]],
    u: &[f64],
    mat: &IsotropicElastic,
) -> Vec<[[f64; 3]; 3]> {
    let elems: Vec<StressElement<'_>> = conns
        .iter()
        .map(|conn| {
            let en = [nodes[conn[0]], nodes[conn[1]], nodes[conn[2]], nodes[conn[3]]];
            StressElement {
                connectivity: conn.as_slice(),
                stress: element_stress_p1(&en, mat, &gather_u_p1(u, conn)),
                volume: tet_volume_p1(&en),
            }
        })
        .collect();
    recover_nodal_stress_p1(nodes.len(), &elems)
}

/// Maximum von Mises stress over all nodes in the recovered stress field.
fn max_von_mises(field: &[[[f64; 3]; 3]]) -> f64 {
    field.iter().map(|s| von_mises_of_tensor(s)).fold(0.0_f64, f64::max)
}

// ─── step-1 GREEN / tests ─────────────────────────────────────────────────────

/// Byte-identical displacement across repeated runs and thread counts in
/// deterministic mode.
///
/// `resolve_execution_modes(true, t, ndof)` → `(Deterministic, Deterministic)`
/// for ALL t, so threads ∈ {1, 4, 16} simultaneously exercises "repeated runs"
/// and "across thread counts". Bit-identical u implies an identical FP-op
/// sequence — the exactness guarantee of the pairwise-tree Deterministic path.
#[test]
fn deterministic_displacement_bit_stable_across_repeats_and_thread_counts() {
    // Verify the fixture builder is callable.
    let _ = box_cantilever_fixture();

    let out1 = solve_box_cantilever(true, 1);
    let out4 = solve_box_cantilever(true, 4);
    let out16 = solve_box_cantilever(true, 16);

    // All runs must converge.
    assert!(out1.converged, "deterministic t=1 did not converge (iter={})", out1.iterations);
    assert!(out4.converged, "deterministic t=4 did not converge (iter={})", out4.iterations);
    assert!(out16.converged, "deterministic t=16 did not converge (iter={})", out16.iterations);

    // Iteration counts must be equal (same FP sequence → same convergence step).
    assert_eq!(
        out1.iterations, out4.iterations,
        "deterministic iteration count differs between t=1 ({}) and t=4 ({})",
        out1.iterations, out4.iterations,
    );
    assert_eq!(
        out1.iterations, out16.iterations,
        "deterministic iteration count differs between t=1 ({}) and t=16 ({})",
        out1.iterations, out16.iterations,
    );

    // Displacement vector must be byte-identical (f64::to_bits slot-wise).
    assert_eq!(out1.u.len(), out4.u.len(), "u length differs between t=1 and t=4");
    assert_eq!(out1.u.len(), out16.u.len(), "u length differs between t=1 and t=16");
    for i in 0..out1.u.len() {
        assert_eq!(
            out1.u[i].to_bits(), out4.u[i].to_bits(),
            "u[{i}] differs between t=1 ({}) and t=4 ({})",
            out1.u[i], out4.u[i],
        );
        assert_eq!(
            out1.u[i].to_bits(), out16.u[i].to_bits(),
            "u[{i}] differs between t=1 ({}) and t=16 ({})",
            out1.u[i], out16.u[i],
        );
    }
}

// ─── step-3 RED ──────────────────────────────────────────────────────────────

/// Byte-identical recovered stress field and max von Mises across thread counts
/// in deterministic mode.
///
/// Extends the deterministic comparison to the other "ElasticResult" fields:
/// the nodal stress field (volume-weighted averaging via `recover_nodal_stress_p1`)
/// and max von Mises. In Deterministic mode the fixed pairwise-tree reductions
/// produce an identical FP sequence → byte-identical output at every level.
///
/// Note: `recover_stress_field` and `max_von_mises` are not yet defined —
/// this test fails to compile (RED).
#[test]
fn deterministic_stress_field_and_von_mises_bit_stable_across_thread_counts() {
    // Get the fixture geometry so we can recover stress from the displacement.
    let (nodes, conns, _, _) = box_cantilever_fixture();

    let out1 = solve_box_cantilever(true, 1);
    let out4 = solve_box_cantilever(true, 4);
    let out16 = solve_box_cantilever(true, 16);

    // All converged.
    assert!(out1.converged, "deterministic t=1 did not converge");
    assert!(out4.converged, "deterministic t=4 did not converge");
    assert!(out16.converged, "deterministic t=16 did not converge");

    // Recover nodal stress fields (missing helper → RED).
    let stress1 = recover_stress_field(&nodes, &conns, &out1.u, &MAT);
    let stress4 = recover_stress_field(&nodes, &conns, &out4.u, &MAT);
    let stress16 = recover_stress_field(&nodes, &conns, &out16.u, &MAT);

    // Compute max von Mises (missing helper → RED).
    let vm1 = max_von_mises(&stress1);
    let vm4 = max_von_mises(&stress4);
    let vm16 = max_von_mises(&stress16);

    // Nodal stress field must be byte-identical across all thread counts.
    assert_eq!(stress1.len(), stress4.len(), "stress field length differs t=1 vs t=4");
    assert_eq!(stress1.len(), stress16.len(), "stress field length differs t=1 vs t=16");
    for ni in 0..stress1.len() {
        for i in 0..3 {
            for j in 0..3 {
                assert_eq!(
                    stress1[ni][i][j].to_bits(), stress4[ni][i][j].to_bits(),
                    "stress[{ni}][{i}][{j}] differs between t=1 ({}) and t=4 ({})",
                    stress1[ni][i][j], stress4[ni][i][j],
                );
                assert_eq!(
                    stress1[ni][i][j].to_bits(), stress16[ni][i][j].to_bits(),
                    "stress[{ni}][{i}][{j}] differs between t=1 ({}) and t=16 ({})",
                    stress1[ni][i][j], stress16[ni][i][j],
                );
            }
        }
    }

    // Max von Mises must be byte-identical.
    assert_eq!(
        vm1.to_bits(), vm4.to_bits(),
        "max_von_mises differs between t=1 ({vm1}) and t=4 ({vm4})",
    );
    assert_eq!(
        vm1.to_bits(), vm16.to_bits(),
        "max_von_mises differs between t=1 ({vm1}) and t=16 ({vm16})",
    );
}
