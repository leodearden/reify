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

use reify_solver_elastic::{
    AssemblyElement, ElementOrder, ElementStiffness,
    assemble_global_stiffness, apply_dirichlet_row_elimination,
    DirichletBc, IsotropicElastic,
    StressElement, element_stress_p1, recover_nodal_stress_p1, tet_volume_p1,
    solve_cg, CgSolverOptions, CgResult,
    resolve_execution_modes,
};

// ─── constants ────────────────────────────────────────────────────────────────

/// CG solver tolerance for the determinism harness.
///
/// `1e-10` is tight enough to ensure the solver has converged well before
/// round-off between parallel and deterministic paths becomes relevant. The
/// gap between `CG_TOL = 1e-10` and `EQUIV_TOL = 1e-3` gives ≥ 2 orders of
/// headroom even at cond(K) ~ 1e6.
const CG_TOL: f64 = 1e-10;

/// Tolerance-equivalence bound for parallel-mode comparisons.
///
/// Basis: ‖Δu‖/‖u‖ ≲ cond(K)·`CG_TOL`. For the stocky ~50K-DOF box
/// cond(K) ~ O(1e3–1e4) → expected difference ≲ 1e-6–1e-5, giving ≥ 2 orders
/// headroom below 1e-3. The bound still detects any real non-determinism leak
/// (O(1) or NaN), which manifests orders of magnitude above this threshold.
const EQUIV_TOL: f64 = 1e-3;

/// Thread counts exercised by the harness.
///
/// `SolverMode::Parallel{threads}` spawns exactly `threads` workers via
/// `std::thread::scope` (chunk = n.div_ceil(threads)) regardless of physical
/// CPU count, so {1, 4, 16} is portable across all CI machines (16 merely
/// oversubscribes a 4-core box). `resolve_execution_modes(_, 1, _)` →
/// Deterministic (threads ≤ 1 policy), so the t=1 entry is bit-identical
/// to a deterministic solve.
const THREAD_COUNTS: [usize; 3] = [1, 4, 16];

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
#[allow(clippy::type_complexity)]
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
    /// Maximum nodal von Mises stress recovered from the computed displacement `u`.
    ///
    /// Computed via `recover_stress_field` + `max_von_mises` inside
    /// `solve_box_cantilever`.
    max_von_mises: f64,
}

/// Assemble, apply BCs, and CG-solve the ~50K-DOF cantilever in the mode
/// resolved from `deterministic` + `threads` via `resolve_execution_modes`.
///
/// Both assembly and solve use the matched `(AssemblyMode, SolverMode)` pair
/// from `resolve_execution_modes`, faithfully exercising PRD task #18's full
/// end-to-end contract (assembly determinism included).
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

    let opts = CgSolverOptions { tolerance: CG_TOL, max_iter: 5000 };
    let result: CgResult = solve_cg(&k, &f, opts, smode);

    let u_vec = result.u().to_vec();

    // Recover nodal stress field and compute max von Mises.
    let stress = recover_stress_field(&nodes, &conns, &u_vec, &MAT);
    let vm = max_von_mises(&stress);

    SolveOutput {
        u: u_vec,
        iterations: result.iterations,
        converged: result.converged,
        max_von_mises: vm,
    }
}

// ─── stress helpers ───────────────────────────────────────────────────────────

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
    field.iter().map(von_mises_of_tensor).fold(0.0_f64, f64::max)
}

// ─── comparison helpers (step-6 GREEN) ───────────────────────────────────────

/// Relative L2 norm of the difference between two equal-length vectors.
///
/// `‖a − b‖₂ / max(‖a‖₂, FLOOR)` where `FLOOR = 1e-30` prevents division
/// by zero when the reference is the zero vector (degenerate fixture).
fn rel_l2(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(a.len(), b.len(), "rel_l2: vectors must have equal length");
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let diff_norm: f64 = a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum::<f64>().sqrt();
    const FLOOR: f64 = 1e-30;
    diff_norm / norm_a.max(FLOOR)
}

/// Assert that `reference` and `candidate` solve outputs are tolerance-equivalent.
///
/// Checks:
/// 1. Relative-L2 of the displacement difference ≤ `EQUIV_TOL`.
/// 2. Relative difference of max von Mises ≤ `EQUIV_TOL`.
///
/// `label` is embedded in panic messages for identification.
/// Iteration counts are deliberately NOT compared here — in parallel mode
/// round-off can shift the convergence step across runs/thread counts.
fn assert_tolerance_equivalent(reference: &SolveOutput, candidate: &SolveOutput, label: &str) {
    let u_err = rel_l2(&reference.u, &candidate.u);
    assert!(
        u_err <= EQUIV_TOL,
        "{label}: displacement rel-L2 error {u_err:.3e} > EQUIV_TOL={EQUIV_TOL:.3e}",
    );

    const VM_FLOOR: f64 = 1e-30;
    let vm_ref = reference.max_von_mises.max(VM_FLOOR);
    let vm_diff = (reference.max_von_mises - candidate.max_von_mises).abs();
    let vm_err = vm_diff / vm_ref;
    assert!(
        vm_err <= EQUIV_TOL,
        "{label}: max von Mises rel error {vm_err:.3e} > EQUIV_TOL={EQUIV_TOL:.3e} \
         (ref={:.6e}, cand={:.6e})",
        reference.max_von_mises,
        candidate.max_von_mises,
    );
}

// ─── sweep helper (step-8 GREEN) ──────────────────────────────────────────────

/// Run `solve_box_cantilever` for each thread count in `threads` and collect
/// the outputs.
///
/// Note: `SolverMode::Parallel{threads}` spawns exactly the requested worker
/// count via `std::thread::scope` regardless of physical cores — {1, 4, 16}
/// is portable across all CI machines.
fn solve_sweep(deterministic: bool, threads: &[usize]) -> Vec<SolveOutput> {
    threads.iter().map(|&t| solve_box_cantilever(deterministic, t)).collect()
}

// ─── deterministic mode: displacement ────────────────────────────────────────

/// Byte-identical displacement across repeated runs and thread counts in
/// deterministic mode.
///
/// `resolve_execution_modes(true, t, ndof)` → `(Deterministic, Deterministic)`
/// for ALL t, so `THREAD_COUNTS = {1, 4, 16}` simultaneously exercises
/// "repeated runs" and "across thread counts". Bit-identical u implies an
/// identical FP-op sequence — the exactness guarantee of the pairwise-tree
/// Deterministic path.
#[test]
fn deterministic_displacement_bit_stable_across_repeats_and_thread_counts() {
    // Verify the fixture builder is callable.
    let _ = box_cantilever_fixture();

    // Sweep: t ∈ THREAD_COUNTS — all map to Deterministic mode.
    let outs = solve_sweep(true, &THREAD_COUNTS);
    let ref_out = &outs[0]; // t=1 is the reference

    // Sanity: verify the fixture produced a non-trivial solve. If the tip load is
    // accidentally zeroed, solve_cg short-circuits to u = 0 with iterations == 0
    // and converged == true — all determinism checks would then pass trivially.
    assert!(
        ref_out.u.iter().any(|&x| x != 0.0),
        "reference displacement is all zero — tip load may be missing",
    );
    assert!(ref_out.iterations > 0, "CG returned 0 iterations — RHS may be zero");

    for (i, out) in outs.iter().enumerate() {
        let t = THREAD_COUNTS[i];

        // Each run must converge.
        assert!(
            out.converged,
            "deterministic t={t} did not converge (iter={})",
            out.iterations,
        );

        // Iteration counts must equal the reference (same FP sequence).
        assert_eq!(
            ref_out.iterations, out.iterations,
            "deterministic iteration count differs between t=1 ({}) and t={t} ({})",
            ref_out.iterations, out.iterations,
        );

        // Displacement vector must be byte-identical.
        assert_eq!(ref_out.u.len(), out.u.len(), "u length differs for t={t}");
        for j in 0..ref_out.u.len() {
            assert_eq!(
                ref_out.u[j].to_bits(), out.u[j].to_bits(),
                "u[{j}] differs between t=1 ({}) and t={t} ({})",
                ref_out.u[j], out.u[j],
            );
        }
    }
}

// ─── default mode: thread counts ─────────────────────────────────────────────

/// Tolerance-equivalent results across thread counts {1, 4, 16} in default mode.
///
/// Uses `solve_sweep(false, &THREAD_COUNTS)` to sweep default-mode solves across
/// {1, 4, 16} threads, then compares each to a deterministic reference via
/// `assert_tolerance_equivalent`. `resolve_execution_modes(false, 1, ndof)` →
/// Deterministic (threads ≤ 1 policy), so the t=1 sweep entry is itself
/// bit-identical to a deterministic solve.
///
/// Iteration count equality is explicitly NOT asserted: in parallel mode
/// different thread counts use different FP reduction orders, which shifts the
/// convergence step (within a few iterations).
#[test]
fn default_parallel_tolerance_equivalent_across_thread_counts() {
    // Deterministic reference: bit-stable baseline.
    let det_ref = solve_box_cantilever(true, 1);
    assert!(det_ref.converged, "deterministic reference did not converge");
    // Sanity: a zeroed tip load would produce a trivial solve (u = 0, iters = 0,
    // converged = true), making all tolerance-equivalence checks pass spuriously.
    assert!(
        det_ref.u.iter().any(|&x| x != 0.0),
        "deterministic reference displacement is all zero — tip load may be missing",
    );
    assert!(
        det_ref.max_von_mises > 0.0,
        "deterministic reference max von Mises is zero — stress is unphysical",
    );

    // Sweep parallel-mode solves across THREAD_COUNTS.
    let outputs = solve_sweep(false, &THREAD_COUNTS);
    for (i, out) in outputs.iter().enumerate() {
        let t = THREAD_COUNTS[i];
        assert!(
            out.converged,
            "parallel mode t={t} did not converge (iter={})",
            out.iterations,
        );
        // Tolerance-equivalence against the deterministic reference.
        // Iteration equality deliberately NOT asserted (parallel mode).
        assert_tolerance_equivalent(&det_ref, out, &format!("threads={t}"));
    }
}

// ─── default mode: repeated runs ─────────────────────────────────────────────

/// Tolerance-equivalent displacement and von Mises across 3 repeated runs in
/// default / parallel mode at fixed threads=4.
///
/// `resolve_execution_modes(false, 4, ndof)` → `Parallel{4}` (ndof ≥ 10 000),
/// so the parallel SpMV/dot path engages. Three independent runs (separate
/// calls to `solve_box_cantilever`) produce the same physical answer up to
/// floating-point round-off from non-deterministic reduction order. The
/// `EQUIV_TOL = 1e-3` bound provides ≥ 2 orders headroom over the expected
/// difference ≲ cond(K)·cg_tol ~ 1e-6.
///
/// Iteration count equality is NOT asserted — in parallel mode round-off can
/// shift the convergence step across runs (within a few iterations).
#[test]
fn default_parallel_tolerance_equivalent_across_repeated_runs() {
    // Three repeated runs at fixed threads=4 in parallel mode.
    let run1 = solve_box_cantilever(false, 4);
    let run2 = solve_box_cantilever(false, 4);
    let run3 = solve_box_cantilever(false, 4);

    assert!(run1.converged, "parallel t=4 run1 did not converge (iter={})", run1.iterations);
    assert!(run2.converged, "parallel t=4 run2 did not converge (iter={})", run2.iterations);
    assert!(run3.converged, "parallel t=4 run3 did not converge (iter={})", run3.iterations);
    // Sanity: a zeroed tip load would yield a trivial all-zero solve, making all
    // three repeated runs trivially equivalent.
    assert!(
        run1.u.iter().any(|&x| x != 0.0),
        "parallel run1 displacement is all zero — tip load may be missing",
    );
    assert!(run1.max_von_mises > 0.0, "parallel run1 max von Mises is zero");

    // Tolerance-equivalence.
    assert_tolerance_equivalent(&run1, &run2, "run2_vs_run1");
    assert_tolerance_equivalent(&run1, &run3, "run3_vs_run1");
}

// ─── deterministic mode: stress and von Mises ────────────────────────────────

/// Byte-identical recovered stress field and max von Mises across thread counts
/// in deterministic mode.
///
/// Extends the deterministic comparison to the other "ElasticResult" fields:
/// the nodal stress field (volume-weighted averaging via `recover_nodal_stress_p1`)
/// and max von Mises. In Deterministic mode the fixed pairwise-tree reductions
/// produce an identical FP sequence → byte-identical output at every level.
///
/// Uses `solve_sweep(true, &THREAD_COUNTS)` to run all three thread counts in
/// one call. Max von Mises is taken from `SolveOutput.max_von_mises` (already
/// computed inside `solve_box_cantilever`); the per-component stress field is
/// recovered separately (not stored in `SolveOutput`) for the byte-wise check.
///
/// NOTE: This test calls `solve_sweep(true, &THREAD_COUNTS)` independently from
/// `deterministic_displacement_bit_stable_across_repeats_and_thread_counts`.
/// The 3 deterministic solves are byte-identical to those in the displacement
/// test; the duplication keeps each test self-contained so they can fail
/// independently. The stress-recovery step (`recover_stress_field`) is pure
/// computation over the already-computed `u` vectors — no extra linear solves
/// are incurred beyond the 3-solve sweep itself.
#[test]
fn deterministic_stress_field_and_von_mises_bit_stable_across_thread_counts() {
    // Sweep deterministic-mode solves across THREAD_COUNTS = {1, 4, 16}.
    let outs = solve_sweep(true, &THREAD_COUNTS);
    // Get fixture geometry for stress recovery (nodes/conns are identical across
    // calls — box_cantilever_fixture is deterministic and pure).
    let (nodes, conns, _, _) = box_cantilever_fixture();

    // All runs must converge.
    for (i, out) in outs.iter().enumerate() {
        let t = THREAD_COUNTS[i];
        assert!(out.converged, "deterministic t={t} did not converge");
    }

    // Recover nodal stress fields for each thread count.
    let stress_fields: Vec<Vec<[[f64; 3]; 3]>> = outs
        .iter()
        .map(|out| recover_stress_field(&nodes, &conns, &out.u, &MAT))
        .collect();

    // Max von Mises is already stored in SolveOutput (computed in solve_box_cantilever).
    let vms: Vec<f64> = outs.iter().map(|out| out.max_von_mises).collect();
    // Sanity: a zeroed tip load would produce trivial all-zero stress, making all
    // byte-identity checks pass spuriously.
    assert!(vms[0] > 0.0, "reference max von Mises is zero — stress is unphysical or load is missing");

    // Nodal stress field must be byte-identical across all thread counts.
    let ref_stress = &stress_fields[0];
    for (k, stress) in stress_fields.iter().enumerate().skip(1) {
        let t = THREAD_COUNTS[k];
        assert_eq!(
            ref_stress.len(), stress.len(),
            "stress field length differs t=1 vs t={t}",
        );
        for ni in 0..ref_stress.len() {
            for i in 0..3 {
                for j in 0..3 {
                    assert_eq!(
                        ref_stress[ni][i][j].to_bits(), stress[ni][i][j].to_bits(),
                        "stress[{ni}][{i}][{j}] differs between t=1 ({}) and t={t} ({})",
                        ref_stress[ni][i][j], stress[ni][i][j],
                    );
                }
            }
        }
    }

    // Max von Mises must be byte-identical across all thread counts.
    let vm_ref = vms[0];
    for (k, &vm) in vms.iter().enumerate().skip(1) {
        let t = THREAD_COUNTS[k];
        assert_eq!(
            vm_ref.to_bits(), vm.to_bits(),
            "max_von_mises differs between t=1 ({vm_ref}) and t={t} ({vm})",
        );
    }
}
