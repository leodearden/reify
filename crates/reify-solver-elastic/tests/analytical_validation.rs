//! FEA validation suite against analytical reference solutions.
//!
//! # Scope
//!
//! PRD `docs/prds/v0_3/structural-analysis-fea.md` task #20: validate the
//! linear-elastostatic solver against analytical references at both P1 and P2
//! element orders. All four of the PRD's reference cases are validated here:
//!
//! 1. Timoshenko cantilever beam tip deflection — ≤ 5% (P1 tet) / ≤ 3% (P2, stocky
//!    L/H=2) / **≤ 1% (P2, slender L/H=15)** — the aspirational 1% bound from
//!    the PRD is now validated at the slender fixture; see
//!    `cantilever_beam_p2_tip_deflection_slender_within_1pct_of_timoshenko`.
//! 2. Simple shear uniform stress — interior σ_xy spread ≤ 1% / von Mises ≤ 1%,
//!    both orders
//! 3. Boussinesq half-space point load, subsurface σ_z — ≤ 10% near load (the
//!    point-load singularity is probed off-axis at depth, not at the node),
//!    both orders
//! 4. **Pressurised thick-walled cylinder (Lamé)** — max von Mises at inner fibre
//!    ≤ 5% (P1 tet) / ≤ 2% (P2). Quarter-annulus plane-strain model (a=1, b=2,
//!    θ∈[0,π/2]): structured polar → Kuhn-tet mesh, pressure-as-traction inner-
//!    surface BC (per-face centroid radial traction via `apply_traction_load`),
//!    symmetry + plane-strain Dirichlet BCs suppress all 6 rigid-body modes
//!    exactly. Survey (`docs/architecture-audit/fea-accuracy-achievability-survey-2026-05-29.md`)
//!    rates both bounds ACHIEVABLE — smooth axisymmetric field, no bending lock
//!    (task 4113).
//!
//! The same cantilever and thick-walled-cylinder cases on P1 hex and wedge
//! meshes live in `analytical_validation_hex_wedge.rs`.
//!
//! # Cantilever load / measurement convention (why not a single-node point load)
//!
//! The PRD says "point load F at the free end" + "tip deflection". The naive
//! discretisation — one nodal force at the centroid of the end face, deflection
//! read at that same node — is **mesh-divergent**: the single-node force is a
//! discrete point singularity whose local displacement spike *grows* without
//! bound as the mesh refines (measured P1 error went 2.3 % → 3.4 % → 6.2 % →
//! 10.3 % from 12³ to 24³; P2 was 17 % at 12³). Any "pass" it produces is a
//! coarse-mesh coincidence — the spike happening to cancel the bending-lock
//! underestimate. By Saint-Venant the beam-theory reference only depends on the
//! end *resultant*, so the faithful discretisation applies the shear F
//! **distributed over the end-face nodes** (resultant F) and reads the tip
//! deflection as the **mean transverse displacement over the end face** (the
//! neutral-axis deflection). Under that convention the error converges
//! monotonically and is mesh-stable.
//!
//! # Accuracy-floor honesty (Leo's no-silent-relaxation rule)
//!
//! Fixture pinned to **L/H = 2** (stubby), faithful load/measurement as above:
//!
//! - **P1 ≤ 5 %** is *achievable* — the faithful error converges 7.9 % → 3.8 %
//!   over 12³ → 24³; the 24×24×8 mesh used below sits at ~3.8 %. The survey
//!   (§4.1) prescribes exactly this aspect-ratio pin so the bound stays inside
//!   the P1-tet bending-lock floor — **no relaxation**.
//! - **P2 ≤ 3 %** is the *reference-honest* bound at a **stocky (L/H = 2)**
//!   fixture. The converged 3-D (P2) deflection sits ~2.1 % from the 1-D
//!   Timoshenko reference — and that residual is **1-D beam theory's own
//!   inaccuracy vs 3-D elasticity at a stocky beam**, not a solver error (P2 is
//!   the *accurate* solution here). The PRD's aspirational 1 % therefore requires
//!   a **slender fixture (L/H = 15)** where 1-D theory is ~0.04 %-accurate, plus
//!   a raised CG cap (slender beams are ill-conditioned, cond ∝ (L/H)²). This is
//!   now **validated** by
//!   `cantilever_beam_p2_tip_deflection_slender_within_1pct_of_timoshenko`
//!   (task 4114), which uses `solve_p2_pipeline_with_opts` with `max_iter = 20_000`.
//!
//! Shear is P1-exact (constant-strain patch test) and Boussinesq's 10 % is
//! generous near a known singularity, so neither carries a formulation-floor
//! risk.
//!
//! # Design decisions
//!
//! - Structured tet meshes are generated procedurally (no Gmsh) for
//!   controlled, repeatable mesh resolution within narrow tolerance bands.
//!   Established pattern: `tests/shell_benchmarks.rs`.
//! - Solve pipeline: each element kind's own assembly fn feeds fea_harness's
//!   shared `rhs_from_point_loads` + `solve_displacement` tail.
//! - Stress validation reuses the kernel's `element_stress_p1` /
//!   `element_stress_p2` (per-element constant Cauchy tensor) and
//!   `recover_nodal_stress_p1` (volume-weighted nodal averaging, documented as
//!   connectivity-shape agnostic).

use faer::sparse::SparseRowMat;
use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, ElementOrder, ElementStiffness,
    assemble_global_stiffness,
    DirichletBc, FaceOrder, IsotropicElastic,
    StressElement, element_stress_p1, element_stress_p2,
    recover_nodal_stress_p1, tet_volume_p1,
    CgSolverOptions,
};

#[path = "common/fea_harness.rs"]
mod fea_harness;
use fea_harness::{
    box_p1_mesh, dirichlet_fix_face, dirichlet_prescribe_boundary_field,
    distributed_tip_load, end_face_nodes, gather_u_p1,
    rhs_from_point_loads, solve_displacement, solve_p1_pipeline, von_mises_of_tensor,
};
#[path = "common/analytical_fixtures.rs"]
mod analytical_fixtures;
use analytical_fixtures::{
    annular_p1_mesh, annular_symmetry_plane_strain_bcs, cylinder_p1_solve_and_recover,
    inner_pressure_loads_core, lame_stresses, lame_von_mises, max_inner_ring_von_mises,
    mean_tip_deflection, recover_nodal_p1, timoshenko_tip_deflection,
};

// ─── cantilever fixtures and local helpers ──────────────────────────────────

/// Find the index of the node closest to `target` within `tol`.
///
/// Panics if no node is within `tol` of `target`.
fn find_node_at(nodes: &[[f64; 3]], target: [f64; 3], tol: f64) -> usize {
    nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| {
            let dx = n[0] - target[0];
            let dy = n[1] - target[1];
            let dz = n[2] - target[2];
            (dx*dx + dy*dy + dz*dz).sqrt() < tol
        })
        .min_by(|(_, a), (_, b)| {
            let sq = |n: &&[f64; 3]| {
                let dx = n[0] - target[0];
                let dy = n[1] - target[1];
                let dz = n[2] - target[2];
                dx*dx + dy*dy + dz*dz
            };
            sq(a).partial_cmp(&sq(b)).unwrap()
        })
        .map(|(i, _)| i)
        .unwrap_or_else(|| {
            panic!(
                "find_node_at: no node within tol={tol:.2e} of \
                 [{:.4},{:.4},{:.4}]",
                target[0], target[1], target[2],
            )
        })
}

/// `(nodes, p1_conns, clamp_bcs, end_face_nodes)` — returned by
/// [`cantilever_clamped_p1`].
type ClampedCantileverP1 = (Vec<[f64; 3]>, Vec<[usize; 4]>, Vec<DirichletBc>, Vec<usize>);

/// `(p2_nodes, p2_conns, clamp_bcs, end_face_nodes)` — returned by
/// [`cantilever_clamped_p2`].
type ClampedCantileverP2 = (Vec<[f64; 3]>, Vec<[usize; 10]>, Vec<DirichletBc>, Vec<usize>);

/// Build a clamped cantilever P1 mesh and identify its free-end face.
///
/// The bar is `[0, L] × [0, H] × [0, B]` with `nx × ny × nz` hex cells → 6-tet
/// Kuhn split. The `x=0` face is fully clamped (all 3 DOFs). The returned
/// `end_nodes` are every node on the free end (`x = L`); the caller turns them
/// into a distributed shear load via [`distributed_tip_load`] and reads the tip
/// deflection via [`mean_tip_deflection`] (see the module-level note on why a
/// single-node point load is mesh-divergent).
///
/// # Returns
///
/// `(nodes, p1_conns, dirichlet_bcs, end_nodes)`.
fn cantilever_clamped_p1(
    l: f64, h: f64, b: f64,
    nx: usize, ny: usize, nz: usize,
) -> ClampedCantileverP1 {
    let (nodes, conns) = box_p1_mesh(l, h, b, nx, ny, nz);
    let tol_x = 0.5 * l / nx as f64; // half the x-spacing
    let bcs = dirichlet_fix_face(&nodes, 0, 0.0, tol_x);
    let end = end_face_nodes(&nodes, l, tol_x);
    (nodes, conns, bcs, end)
}

/// Build a clamped cantilever P2 mesh and identify its free-end face.
///
/// Same geometry / clamp convention as [`cantilever_clamped_p1`], but the P1
/// corner mesh is promoted to P2 by [`add_edge_midpoint_nodes`] before the clamp
/// and end-face nodes are resolved against the full (corner + edge-midpoint)
/// node set. Clamping the `x=0` face via [`dirichlet_fix_face`] therefore also
/// fixes the edge-midpoint nodes lying *in* that face (their `x` coordinate is
/// `0`), while the first interior plane of x-edge midpoints sits at
/// `x = ½·Lx/nx`, exactly on the `tol_x` boundary and so correctly excluded by
/// the strict `< tol_x` test. The free-end face likewise includes the
/// edge-midpoint nodes lying in the `x = L` plane.
///
/// # Returns
///
/// `(p2_nodes, p2_conns, dirichlet_bcs, end_nodes)`.
fn cantilever_clamped_p2(
    l: f64, h: f64, b: f64,
    nx: usize, ny: usize, nz: usize,
) -> ClampedCantileverP2 {
    let (corner_nodes, p1_conns) = box_p1_mesh(l, h, b, nx, ny, nz);
    let (p2_nodes, p2_conns) = add_edge_midpoint_nodes(&corner_nodes, &p1_conns);
    let tol_x = 0.5 * l / nx as f64; // half the x corner-spacing
    let bcs = dirichlet_fix_face(&p2_nodes, 0, 0.0, tol_x);
    let end = end_face_nodes(&p2_nodes, l, tol_x);
    (p2_nodes, p2_conns, bcs, end)
}

/// Gather the 30 element DOFs (`[u_x,u_y,u_z]` per node) for a P2 tet from the
/// global displacement vector, in element-local node order.
fn gather_u_p2(u: &[f64], conn: &[usize; 10]) -> [f64; 30] {
    let mut ue = [0.0_f64; 30];
    for (k, &node) in conn.iter().enumerate() {
        ue[3 * k] = u[3 * node];
        ue[3 * k + 1] = u[3 * node + 1];
        ue[3 * k + 2] = u[3 * node + 2];
    }
    ue
}

// ─── P2 helpers ─────────────────────────────────────────────────────────────

/// Add edge-midpoint nodes to a P1 tet mesh, producing a P2 mesh.
///
/// For each P1 tet in `p1_conns`, computes 6 edge midpoints in the canonical
/// Hughes/Gmsh EDGES order `[(0,1),(1,2),(2,0),(0,3),(1,3),(2,3)]` and
/// deduplicates shared midpoints via a `HashMap<(min, max), node_idx>`.
///
/// # Returns
///
/// `(p2_nodes, p2_conns)` where:
/// - `p2_nodes` extends `corner_nodes` with the deduplicated midpoint nodes
/// - `p2_conns[e]` is a 10-element array:
///   `[c0, c1, c2, c3, m_{01}, m_{12}, m_{20}, m_{03}, m_{13}, m_{23}]`
fn add_edge_midpoint_nodes(
    corner_nodes: &[[f64; 3]],
    p1_conns: &[[usize; 4]],
) -> (Vec<[f64; 3]>, Vec<[usize; 10]>) {
    use std::collections::HashMap;

    // Canonical Hughes/Gmsh edge ordering for P2 tet (tet_p2.rs:66).
    const EDGES: [(usize, usize); 6] = [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)];

    let mut p2_nodes: Vec<[f64; 3]> = corner_nodes.to_vec();
    let mut edge_to_mid: HashMap<(usize, usize), usize> = HashMap::new();
    let mut p2_conns: Vec<[usize; 10]> = Vec::with_capacity(p1_conns.len());

    for conn in p1_conns {
        let mut p2_conn = [0usize; 10];
        p2_conn[..4].copy_from_slice(conn);
        for (edge_idx, &(a, b)) in EDGES.iter().enumerate() {
            let ga = conn[a];
            let gb = conn[b];
            let key = (ga.min(gb), ga.max(gb));
            let mid_idx = *edge_to_mid.entry(key).or_insert_with(|| {
                let na = corner_nodes[ga];
                let nb = corner_nodes[gb];
                let mid = [
                    0.5 * (na[0] + nb[0]),
                    0.5 * (na[1] + nb[1]),
                    0.5 * (na[2] + nb[2]),
                ];
                let idx = p2_nodes.len();
                p2_nodes.push(mid);
                idx
            });
            p2_conn[4 + edge_idx] = mid_idx;
        }
        p2_conns.push(p2_conn);
    }

    (p2_nodes, p2_conns)
}

/// Assemble the global stiffness matrix of a P2 tetrahedral mesh, in
/// `AssemblyMode::Deterministic`; the P2 counterpart of
/// [`fea_harness::assemble_p1_stiffness`].
fn assemble_p2_stiffness(
    nodes: &[[f64; 3]],
    conns: &[[usize; 10]],
    mat: &IsotropicElastic,
) -> SparseRowMat<usize, f64> {
    let ke_list: Vec<ElementStiffness> = conns
        .iter()
        .map(|conn| {
            let elem_nodes: Vec<[f64; 3]> = conn.iter().map(|&i| nodes[i]).collect();
            reify_solver_elastic::element_stiffness(ElementOrder::P2, &elem_nodes, mat)
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

/// Assemble, apply BCs, and CG-solve a P2 tetrahedral FEA system:
/// [`rhs_from_point_loads`] and [`assemble_p2_stiffness`] fed to
/// [`solve_displacement`].
///
/// `opts` controls the CG solver's convergence criterion and iteration cap.
/// For slender or otherwise ill-conditioned problems, pass a raised `max_iter`.
#[track_caller]
fn solve_p2_pipeline_with_opts(
    nodes: &[[f64; 3]],
    conns: &[[usize; 10]],
    bcs: Vec<DirichletBc>,
    point_loads: &[(usize, f64)],
    mat: &IsotropicElastic,
    opts: CgSolverOptions,
) -> Vec<f64> {
    let f = rhs_from_point_loads(nodes.len(), point_loads);
    solve_displacement(assemble_p2_stiffness(nodes, conns, mat), &f, bcs, opts)
}

/// Assemble, apply BCs, and CG-solve a P2 tetrahedral FEA system using the
/// default CG options (`max_iter = 1000`, `tolerance = 1e-8`).
///
/// For slender or otherwise ill-conditioned problems use
/// [`solve_p2_pipeline_with_opts`] with a raised `max_iter`.
#[track_caller]
fn solve_p2_pipeline(
    nodes: &[[f64; 3]],
    conns: &[[usize; 10]],
    bcs: Vec<DirichletBc>,
    point_loads: &[(usize, f64)],
    mat: &IsotropicElastic,
) -> Vec<f64> {
    solve_p2_pipeline_with_opts(nodes, conns, bcs, point_loads, mat, CgSolverOptions::default())
}

// ─── cantilever beam P1 tip-deflection validation ───────────────────────────

/// Cantilever beam P1 tip-deflection validation against Timoshenko.
///
/// # Geometry / material
///
/// Rectangular bar `L × H × B = 2 × 1 × 0.5` (dimensionless), `E = 1`, `ν = 0.3`.
/// **L/H = 2** (stocky) keeps the P1 bound inside the bending-lock floor — see
/// the module header's accuracy-floor note and the survey (§4.1).
///
/// # Load / measurement (faithful — see module header)
///
/// `x=0` fully clamped; an end shear of resultant `F = 1` in −y **distributed
/// over the free-end face**, with the tip deflection read as the **mean −y
/// displacement over that face** (neutral-axis deflection). This avoids the
/// mesh-divergent single-node point-load artifact.
///
/// # Mesh / tolerance
///
/// 24×24×8 hex → 6-tet Kuhn split → P1. The faithful error converges
/// 7.9 % (12³) → 3.8 % (24×24×8), comfortably under the **≤ 5 %** bound.
#[test]
fn cantilever_beam_p1_tip_deflection_within_5pct_of_timoshenko() {
    const L: f64 = 2.0;
    const H: f64 = 1.0;
    const B: f64 = 0.5;
    const NX: usize = 24; // along x (length)
    const NY: usize = 24; // along y (height) — drives bending-lock relief
    const NZ: usize = 8; // along z (width)
    const F: f64 = 1.0;

    let mat = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.3 };

    let (nodes, conns, bcs, end) = cantilever_clamped_p1(L, H, B, NX, NY, NZ);
    let n_nodes = nodes.len();
    let loads = distributed_tip_load(&end, F);
    let u = solve_p1_pipeline(&nodes, &conns, bcs, &loads, &mat);

    let tip_disp = mean_tip_deflection(&u, &end);
    let delta_ref = timoshenko_tip_deflection(F, L, H, B, &mat);

    let rel_err = (tip_disp - delta_ref).abs() / delta_ref;
    assert!(
        rel_err <= 0.05,
        "cantilever P1: tip deflection {tip_disp:.6e} vs Timoshenko reference \
         {delta_ref:.6e} — relative error {:.2}% > 5% tolerance \
         (mesh: {NX}×{NY}×{NZ}, n_nodes={n_nodes})",
        rel_err * 100.0,
    );
}

// ─── cantilever beam P2 tip-deflection validation ───────────────────────────

/// Cantilever beam P2 tip-deflection validation against Timoshenko.
///
/// Same stocky `L/H = 2` fixture and faithful (distributed-load + face-averaged)
/// measurement as the P1 test, at second order on a 12×12×4 mesh (which
/// converges within the solver's default CG iteration cap).
///
/// # Tolerance — reference-honest ≤ 3 %, *not* the PRD's aspirational 1 %
///
/// The converged 3-D (P2) deflection sits ~2.1 % from the 1-D Timoshenko
/// reference. That residual is **1-D beam theory's own inaccuracy vs 3-D
/// elasticity at a stocky beam** — P2 is the *accurate* solution here, so the
/// gap is not a solver error and cannot be closed by mesh refinement. Reaching
/// 1 % would need a slender fixture where 1-D theory is 1 %-accurate, which
/// re-triggers P1 bending-lock and exceeds the hard-coded CG cap. Per Leo's
/// no-silent-relaxation rule the bound is set to the reference-honest 3 % and
/// the aspirational 1 % is re-homed to a slender-fixture P2 follow-up
/// (task 4114), mirroring 3819 → 4066. See
/// `docs/architecture-audit/fea-accuracy-achievability-survey-2026-05-29.md`.
#[test]
fn cantilever_beam_p2_tip_deflection_within_3pct_of_timoshenko() {
    const L: f64 = 2.0;
    const H: f64 = 1.0;
    const B: f64 = 0.5;
    const NX: usize = 12;
    const NY: usize = 12;
    const NZ: usize = 4;
    const F: f64 = 1.0;

    let mat = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.3 };

    let (p2_nodes, p2_conns, bcs, end) = cantilever_clamped_p2(L, H, B, NX, NY, NZ);
    let n_nodes = p2_nodes.len();
    let loads = distributed_tip_load(&end, F);
    let u = solve_p2_pipeline(&p2_nodes, &p2_conns, bcs, &loads, &mat);

    let tip_disp = mean_tip_deflection(&u, &end);
    let delta_ref = timoshenko_tip_deflection(F, L, H, B, &mat);

    let rel_err = (tip_disp - delta_ref).abs() / delta_ref;
    assert!(
        rel_err <= 0.03,
        "cantilever P2: tip deflection {tip_disp:.6e} vs Timoshenko reference \
         {delta_ref:.6e} — relative error {:.2}% > 3% tolerance \
         (mesh: {NX}×{NY}×{NZ}, n_nodes={n_nodes})",
        rel_err * 100.0,
    );
}

// ─── simple shear: uniform-stress constant-strain patch test ─────────────────

/// Simple-shear uniform-stress validation (P1).
///
/// Unit cube, `N³` hex → 6-tet Kuhn split. The exact linear field
/// `u = (γ·y, 0, 0)` is prescribed on the *whole* boundary — a constant-strain
/// patch test that P1 reproduces exactly in the interior, yielding a uniform
/// `σ_xy = G·γ` and von Mises `√3·G·γ`. Asserts per-element `σ_xy` spread ≤ 1 %,
/// value within 1 % of `G·γ`, and max von Mises within 1 %.
#[test]
fn simple_shear_uniform_stress_p1_within_1pct() {
    const N: usize = 6;
    const GAMMA: f64 = 0.1;

    let mat = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.3 };
    let g = mat.youngs_modulus / (2.0 * (1.0 + mat.poisson_ratio));

    let (nodes, conns) = box_p1_mesh(1.0, 1.0, 1.0, N, N, N);
    let tol = 0.25 / N as f64; // < half the 1/N spacing
    let field = |p: [f64; 3]| [GAMMA * p[1], 0.0, 0.0];
    let bcs = dirichlet_prescribe_boundary_field(&nodes, 1.0, 1.0, 1.0, tol, field);

    let u = solve_p1_pipeline(&nodes, &conns, bcs, &[], &mat);

    let expected_sxy = g * GAMMA;
    let expected_vm = 3.0_f64.sqrt() * expected_sxy;
    let (mut sxy_min, mut sxy_max, mut vm_max) = (f64::INFINITY, f64::NEG_INFINITY, 0.0_f64);
    for conn in &conns {
        let elem_nodes = [nodes[conn[0]], nodes[conn[1]], nodes[conn[2]], nodes[conn[3]]];
        let ue = gather_u_p1(&u, conn);
        let s = element_stress_p1(&elem_nodes, &mat, &ue);
        sxy_min = sxy_min.min(s[0][1]);
        sxy_max = sxy_max.max(s[0][1]);
        vm_max = vm_max.max(von_mises_of_tensor(&s));
    }

    assert_shear_uniform("P1", expected_sxy, expected_vm, sxy_min, sxy_max, vm_max);
}

/// Simple-shear uniform-stress validation (P2). Identical patch test at second
/// order; P2 likewise reproduces the linear field exactly.
#[test]
fn simple_shear_uniform_stress_p2_within_1pct() {
    const N: usize = 4;
    const GAMMA: f64 = 0.1;

    let mat = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.3 };
    let g = mat.youngs_modulus / (2.0 * (1.0 + mat.poisson_ratio));

    let (corner_nodes, p1_conns) = box_p1_mesh(1.0, 1.0, 1.0, N, N, N);
    let (nodes, conns) = add_edge_midpoint_nodes(&corner_nodes, &p1_conns);
    let tol = 0.25 / N as f64;
    let field = |p: [f64; 3]| [GAMMA * p[1], 0.0, 0.0];
    let bcs = dirichlet_prescribe_boundary_field(&nodes, 1.0, 1.0, 1.0, tol, field);

    let u = solve_p2_pipeline(&nodes, &conns, bcs, &[], &mat);

    let expected_sxy = g * GAMMA;
    let expected_vm = 3.0_f64.sqrt() * expected_sxy;
    let (mut sxy_min, mut sxy_max, mut vm_max) = (f64::INFINITY, f64::NEG_INFINITY, 0.0_f64);
    for conn in &conns {
        let mut elem_nodes = [[0.0_f64; 3]; 10];
        for (k, &nidx) in conn.iter().enumerate() {
            elem_nodes[k] = nodes[nidx];
        }
        let ue = gather_u_p2(&u, conn);
        let s = element_stress_p2(&elem_nodes, &mat, &ue);
        sxy_min = sxy_min.min(s[0][1]);
        sxy_max = sxy_max.max(s[0][1]);
        vm_max = vm_max.max(von_mises_of_tensor(&s));
    }

    assert_shear_uniform("P2", expected_sxy, expected_vm, sxy_min, sxy_max, vm_max);
}

/// Shared assertions for the simple-shear patch test: σ_xy spatial uniformity
/// ≤ 1 %, σ_xy value within 1 % of `G·γ`, and max von Mises within 1 %.
fn assert_shear_uniform(
    order: &str,
    expected_sxy: f64,
    expected_vm: f64,
    sxy_min: f64,
    sxy_max: f64,
    vm_max: f64,
) {
    let spread = (sxy_max - sxy_min).abs() / expected_sxy;
    assert!(
        spread <= 0.01,
        "simple shear {order}: σ_xy spatial spread {:.3}% > 1% \
         (min={sxy_min:.6e}, max={sxy_max:.6e}, expected={expected_sxy:.6e})",
        spread * 100.0,
    );
    let sxy_err = (sxy_max - expected_sxy)
        .abs()
        .max((sxy_min - expected_sxy).abs())
        / expected_sxy;
    assert!(
        sxy_err <= 0.01,
        "simple shear {order}: σ_xy deviates {:.3}% from G·γ={expected_sxy:.6e} > 1%",
        sxy_err * 100.0,
    );
    let vm_err = (vm_max - expected_vm).abs() / expected_vm;
    assert!(
        vm_err <= 0.01,
        "simple shear {order}: max von Mises {vm_max:.6e} vs √3·G·γ \
         {expected_vm:.6e} — {:.3}% > 1%",
        vm_err * 100.0,
    );
}

// ─── convergence study (ignored in CI) ──────────────────────────────────────
/// Faithful cantilever convergence study at L/H=2 (distributed end shear +
/// neutral-axis-averaged tip deflection). Documents that the P1 error converges
/// monotonically below the 5% bound, and that P2 floors at the ~2% reference
/// limit (1-D Timoshenko vs 3-D). Run with:
/// `cargo test -p reify-solver-elastic --test analytical_validation \
///   cantilever_faithful_convergence_study -- --ignored --nocapture`
#[test]
#[ignore = "convergence study; run explicitly with --ignored"]
fn cantilever_faithful_convergence_study() {
    let mat = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.3 };
    let (l, h, b, f) = (2.0_f64, 1.0_f64, 0.5_f64, 1.0_f64);
    let delta_ref = timoshenko_tip_deflection(f, l, h, b, &mat);
    println!("L/H=2 Timoshenko ref: {delta_ref:.6}");

    for &(nx, ny, nz) in &[(12usize, 12usize, 4usize), (16, 16, 6), (20, 20, 6), (24, 24, 8)] {
        let (nodes, conns, bcs, end) = cantilever_clamped_p1(l, h, b, nx, ny, nz);
        let loads = distributed_tip_load(&end, f);
        let u = solve_p1_pipeline(&nodes, &conns, bcs, &loads, &mat);
        let d = mean_tip_deflection(&u, &end);
        let err = (d - delta_ref).abs() / delta_ref * 100.0;
        println!("P1 {nx}×{ny}×{nz}: δ={d:.6} err={err:.2}% n_nodes={}", nodes.len());
    }
    // P2 only at the CG-convergent 12×12×4 mesh.
    let (p2n, p2c, p2b, end) = cantilever_clamped_p2(l, h, b, 12, 12, 4);
    let loads = distributed_tip_load(&end, f);
    let u = solve_p2_pipeline(&p2n, &p2c, p2b, &loads, &mat);
    let d = mean_tip_deflection(&u, &end);
    println!(
        "P2 12×12×4: δ={d:.6} err={:.2}% (reference-limited ~2%)",
        (d - delta_ref).abs() / delta_ref * 100.0,
    );
}

// ─── Boussinesq half-space point load ────────────────────────────────────────

/// Recover the continuous nodal stress field of a P2 tet mesh. `element_stress_p2`
/// gives the (centroid-constant) Cauchy tensor; the volume weight is the
/// straight-edge tet volume of the four corner nodes (the recovery routine is
/// documented as connectivity-shape agnostic and accepts the 10-node list).
fn recover_nodal_p2(
    nodes: &[[f64; 3]],
    conns: &[[usize; 10]],
    u: &[f64],
    mat: &IsotropicElastic,
) -> Vec<[[f64; 3]; 3]> {
    let elems: Vec<StressElement<'_>> = conns
        .iter()
        .map(|conn| {
            let mut en = [[0.0_f64; 3]; 10];
            for (k, &nidx) in conn.iter().enumerate() {
                en[k] = nodes[nidx];
            }
            let corners = [en[0], en[1], en[2], en[3]];
            StressElement {
                connectivity: conn.as_slice(),
                stress: element_stress_p2(&en, mat, &gather_u_p2(u, conn)),
                volume: tet_volume_p1(&corners),
            }
        })
        .collect();
    recover_nodal_stress_p1(nodes.len(), &elems)
}

/// Analytical Boussinesq vertical stress magnitude for a surface point load `p`
/// on a half-space: `|σ_z| = (3P/2π)·z³/R⁵`, `R = √(r²+z²)`, `z` = depth ≥ 0,
/// `r` = radial offset from the load axis.
fn boussinesq_sigma_z(p: f64, r: f64, z: f64) -> f64 {
    let rr = (r * r + z * z).sqrt();
    3.0 * p / (2.0 * std::f64::consts::PI) * z.powi(3) / rr.powi(5)
}

/// Mean recovered `|σ_z|` over the four axis-aligned probe nodes on a ring of
/// radius `r` at depth `z` about the load axis `(cx, cy)`. The four points land
/// exactly on grid nodes when `r` is a multiple of the spacing. Returned with
/// the analytical Boussinesq value at `(r, z)` for comparison.
fn boussinesq_ring(
    nodes: &[[f64; 3]],
    sigma: &[[[f64; 3]; 3]],
    center: [f64; 2],
    r: f64,
    z: f64,
    p: f64,
    snap_tol: f64,
) -> (f64, f64) {
    let [cx, cy] = center;
    let mut sum = 0.0;
    for &(dx, dy) in &[(r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r)] {
        let node = find_node_at(nodes, [cx + dx, cy + dy, z], snap_tol);
        sum += sigma[node][2][2].abs();
    }
    (sum / 4.0, boussinesq_sigma_z(p, r, z))
}

/// Boussinesq subsurface σ_z validation (P1).
///
/// Unit-cube block approximating a half-space: top surface `z=0`, bottom `z=1`
/// fully fixed, lateral faces traction-free. A unit point load presses into the
/// surface at the centre. The discrete single-node load is singular in its near
/// field, so σ_z is probed **off-axis at depth** — a ring at `z = 5h`, `r = 2h`
/// (`h = 1/N` the element size), where the recovered field has converged to the
/// analytical point-load solution (tuning sweep: ~2 %, vs the ≤ 10 % bound).
#[test]
fn boussinesq_subsurface_sigma_z_p1_within_10pct() {
    const N: usize = 20;
    const SIDE: f64 = 1.0;
    const P: f64 = 1.0;

    let mat = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.3 };
    let (nodes, conns) = box_p1_mesh(SIDE, SIDE, SIDE, N, N, N);
    let h = SIDE / N as f64;
    let (cx, cy) = (SIDE / 2.0, SIDE / 2.0);

    let load_node = find_node_at(&nodes, [cx, cy, 0.0], 0.4 * h);
    let bcs = dirichlet_fix_face(&nodes, 2, SIDE, 0.5 * h);
    let loads = vec![(load_node * 3 + 2, P)]; // +z, into the half-space
    let u = solve_p1_pipeline(&nodes, &conns, bcs, &loads, &mat);
    let sigma = recover_nodal_p1(&nodes, &conns, &u, &mat);

    let (fe, an) = boussinesq_ring(&nodes, &sigma, [cx, cy], 2.0 * h, 5.0 * h, P, 0.5 * h);
    let rel_err = (fe - an).abs() / an;
    assert!(
        rel_err <= 0.10,
        "Boussinesq P1: ring-mean σ_z {fe:.6e} vs analytical {an:.6e} at \
         (r=2h, z=5h) — relative error {:.2}% > 10% (N={N})",
        rel_err * 100.0,
    );
}

/// Boussinesq subsurface σ_z validation (P2).
///
/// Same half-space block at second order on a 12³ mesh (which converges within
/// the solver's default CG cap). P2's coarser absolute mesh keeps the probe ring
/// shallower — `z = 3h`, `r = 2h` — so the fixed bottom boundary stays far
/// (z is ¼ of the block depth); the tuning sweep puts this at ~5 %, vs ≤ 10 %.
#[test]
fn boussinesq_subsurface_sigma_z_p2_within_10pct() {
    const N: usize = 12;
    const SIDE: f64 = 1.0;
    const P: f64 = 1.0;

    let mat = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.3 };
    let (corner, p1_conns) = box_p1_mesh(SIDE, SIDE, SIDE, N, N, N);
    let (nodes, conns) = add_edge_midpoint_nodes(&corner, &p1_conns);
    let h = SIDE / N as f64;
    let (cx, cy) = (SIDE / 2.0, SIDE / 2.0);

    let load_node = find_node_at(&nodes, [cx, cy, 0.0], 0.4 * h);
    let bcs = dirichlet_fix_face(&nodes, 2, SIDE, 0.5 * h);
    let loads = vec![(load_node * 3 + 2, P)];
    let u = solve_p2_pipeline(&nodes, &conns, bcs, &loads, &mat);
    let sigma = recover_nodal_p2(&nodes, &conns, &u, &mat);

    let (fe, an) = boussinesq_ring(&nodes, &sigma, [cx, cy], 2.0 * h, 3.0 * h, P, 0.5 * h);
    let rel_err = (fe - an).abs() / an;
    assert!(
        rel_err <= 0.10,
        "Boussinesq P2: ring-mean σ_z {fe:.6e} vs analytical {an:.6e} at \
         (r=2h, z=3h) — relative error {:.2}% > 10% (N={N})",
        rel_err * 100.0,
    );
}

// ─── thick-walled cylinder (Lamé) validation ────────────────────────────────

/// Lamé closed-form stresses satisfy textbook invariants (non-circular check).
///
/// Tests `lame_stresses` and `lame_von_mises` against identities that are
/// independently derivable from first principles:
/// - σ_r(a) = −P_i (BC)
/// - σ_r(b) = 0 (BC)
/// - σ_r+σ_θ = 2·P_i·a²/(b²−a²) (constant in r)
/// - σ_θ−σ_r = 2·P_i·a²·b²/((b²−a²)·r²)
/// - σ_z = ν·(σ_r+σ_θ)  [plane-strain]
/// - σ_vm(a) > σ_vm(b)  [peak at inner fibre]
#[test]
fn lame_reference_satisfies_known_invariants() {
    const A: f64 = 1.0;
    const B: f64 = 2.0;
    const P_I: f64 = 1.0;
    const NU: f64 = 0.3;
    let tol = 1e-12;

    // σ_r(a) = −P_i
    let (sr_a, _, _) = lame_stresses(A, A, B, P_I, NU);
    assert!(
        (sr_a - (-P_I)).abs() < tol,
        "σ_r(a) = {sr_a:.15}, expected {}", -P_I,
    );

    // σ_r(b) = 0
    let (sr_b, _, _) = lame_stresses(B, A, B, P_I, NU);
    assert!(sr_b.abs() < tol, "σ_r(b) = {sr_b:.15e}, expected 0");

    // Constant sum and σ_z plane-strain at several r
    let sum_const = 2.0 * P_I * A * A / (B * B - A * A);
    for &r in &[A, 1.2_f64, 1.5_f64, 1.8_f64, B] {
        let (sr, st, sz) = lame_stresses(r, A, B, P_I, NU);
        assert!(
            (sr + st - sum_const).abs() < tol,
            "σ_r+σ_θ at r={r}: got {:.12}, expected {sum_const:.12}",
            sr + st,
        );
        assert!(
            (sz - NU * (sr + st)).abs() < tol,
            "σ_z at r={r}: got {sz:.12}, expected {:.12}",
            NU * (sr + st),
        );
        // σ_θ − σ_r = 2·P_i·a²·b²/((b²−a²)·r²)
        let diff_expected = 2.0 * P_I * A * A * B * B / ((B * B - A * A) * r * r);
        assert!(
            (st - sr - diff_expected).abs() < tol,
            "σ_θ−σ_r at r={r}: got {:.12}, expected {diff_expected:.12}",
            st - sr,
        );
    }

    // von Mises is higher at inner fibre than outer
    let vm_inner = lame_von_mises(A, A, B, P_I, NU);
    let vm_outer = lame_von_mises(B, A, B, P_I, NU);
    assert!(
        vm_inner > vm_outer,
        "expected vm(a)={vm_inner:.6} > vm(b)={vm_outer:.6}",
    );
}

/// Assemble inner-pressure nodal loads for P2 inner-surface triangles.
///
/// Same centroid-radial traction as `analytical_fixtures`' P1 inner-pressure
/// loads (both lower through [`inner_pressure_loads_core`]) but using the
/// 6-node `FaceOrder::P2Tri` quadrature for consistent P2 force assembly.
/// The corner-node centroid (`face[0..3]`) determines the traction direction.
fn inner_pressure_loads_p2(
    inner_faces: &[[usize; 6]],
    nodes: &[[f64; 3]],
    p_i: f64,
) -> Vec<(usize, f64)> {
    inner_pressure_loads_core(inner_faces, nodes, p_i, FaceOrder::P2Tri)
}

/// Thick-walled cylinder (Lamé) P1 max von Mises ≤ 5 % of reference.
///
/// Quarter-annulus model: `a=1, b=2, L=1.0`, `E=1, ν=0.3`, `P_i=1`.
/// Symmetry + plane-strain BCs suppress all 6 rigid-body modes; inner
/// pressure applied via per-face centroid-radial traction, lowered to
/// nodal point loads and fed into `solve_p1_pipeline`.
/// Secondary check: recovered σ_r at inner nodes ≈ −P_i.
#[test]
fn thick_walled_cylinder_p1_max_von_mises_within_5pct_of_lame() {
    const A: f64 = 1.0;
    const B: f64 = 2.0;
    const L: f64 = 1.0;
    const P_I: f64 = 1.0;
    const NR: usize = 24;
    const NTHETA: usize = 24;
    const NZ: usize = 2;
    const E: f64 = 1.0;
    const NU: f64 = 0.3;
    let tol_geom: f64 = 1e-9;

    let mat = IsotropicElastic { youngs_modulus: E, poisson_ratio: NU };
    let (nodes, sigma, max_vm) =
        cylinder_p1_solve_and_recover(A, B, L, NR, NTHETA, NZ, P_I, &mat, tol_geom);
    let n_nodes = nodes.len();
    let lame_ref = lame_von_mises(A, A, B, P_I, NU);

    // Secondary: min σ_r over inner-ring nodes (r ≈ a) — BC sanity check
    let mut min_sr_inner = f64::INFINITY;
    for (ni, &p) in nodes.iter().enumerate() {
        let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
        if (r - A).abs() < tol_geom {
            // σ_r at inner node: radial components from cylindrical coords
            let costh = p[0] / r;
            let sinth = p[1] / r;
            let s = &sigma[ni];
            let sr = costh * costh * s[0][0]
                + 2.0 * costh * sinth * s[0][1]
                + sinth * sinth * s[1][1];
            min_sr_inner = min_sr_inner.min(sr);
        }
    }

    let rel_err = (max_vm - lame_ref).abs() / lame_ref;
    assert!(
        rel_err <= 0.05,
        "cylinder P1: max von Mises {max_vm:.6} vs Lamé ref {lame_ref:.6} \
         — rel err {:.2}% > 5% (mesh: {NR}×{NTHETA}×{NZ}, n_nodes={n_nodes})",
        rel_err * 100.0,
    );

    // Secondary: σ_r at inner surface ≈ −P_i (BC sanity, loose 20%)
    assert!(
        (min_sr_inner + P_I).abs() / P_I <= 0.20,
        "cylinder P1: min σ_r at inner surface {min_sr_inner:.6}, expected ≈ {}", -P_I,
    );
}

/// Cylinder Lamé convergence study (P1) — print von Mises error vs mesh.
///
/// Run with:
/// `cargo test -p reify-solver-elastic --test analytical_validation \
///   cylinder_lame_convergence_study -- --ignored --nocapture`
#[test]
#[ignore = "convergence study; run explicitly with --ignored"]
fn cylinder_lame_convergence_study() {
    const A: f64 = 1.0;
    const B: f64 = 2.0;
    const L: f64 = 1.0;
    const P_I: f64 = 1.0;
    const NU: f64 = 0.3;
    let tol_geom = 1e-9;
    let mat = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: NU };
    let lame_ref = lame_von_mises(A, A, B, P_I, NU);
    println!("Lamé reference von Mises at r=a: {lame_ref:.6}");

    for &(nr, ntheta, nz) in &[
        (8usize, 8usize, 2usize),
        (12, 12, 2),
        (16, 16, 2),
        (20, 20, 2),
        (24, 24, 2),
    ] {
        let (nodes, _sigma, max_vm) =
            cylinder_p1_solve_and_recover(A, B, L, nr, ntheta, nz, P_I, &mat, tol_geom);
        let err = (max_vm - lame_ref).abs() / lame_ref * 100.0;
        println!("P1 {nr}×{ntheta}×{nz}: max_vm={max_vm:.6} err={err:.2}% n_nodes={}", nodes.len());
    }
}

/// Mesh validation: annular P1 quarter-cylinder mesh geometry.
///
/// Checks node count, positive tet volumes (polar Kuhn-split orientation
/// preserves right-handedness), and inner-face count + inner-radius
/// constraint.
#[test]
fn annular_polar_mesh_is_valid() {
    const A: f64 = 1.0;
    const B: f64 = 2.0;
    const L: f64 = 1.0;
    const NR: usize = 4;
    const NTHETA: usize = 4;
    const NZ: usize = 2;

    let (nodes, conns, inner_faces) = annular_p1_mesh(A, B, L, NR, NTHETA, NZ);

    // (a) node count
    assert_eq!(
        nodes.len(), (NR + 1) * (NTHETA + 1) * (NZ + 1),
        "node count mismatch",
    );

    // (b) every tet has positive volume
    for (i, conn) in conns.iter().enumerate() {
        let corners = [nodes[conn[0]], nodes[conn[1]], nodes[conn[2]], nodes[conn[3]]];
        let vol = tet_volume_p1(&corners);
        assert!(
            vol > 0.0,
            "tet {i} has non-positive volume {vol:.3e}",
        );
    }

    // (c) inner-face count and all nodes at r ≈ a
    assert_eq!(inner_faces.len(), 2 * NTHETA * NZ, "inner face count mismatch");
    let tol = 1e-10;
    for (fi, face) in inner_faces.iter().enumerate() {
        for &node in face.iter() {
            let p = nodes[node];
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            assert!(
                (r - A).abs() < tol,
                "inner face {fi}: node {node} at r={r:.6} not at a={A}",
            );
        }
    }
}

/// A wrong inner radius `a` selects no ring node. That is a fixture error the
/// helper must report, not mask as a `0.0` maximum.
#[test]
#[should_panic(
    expected = "max_inner_ring_von_mises: no node within tol_geom=0.000000001 of r=0.5 (n_nodes=8)"
)]
fn max_inner_ring_von_mises_panics_when_no_node_lies_on_the_ring() {
    let (nodes, _, _) = annular_p1_mesh(1.0, 2.0, 1.0, 1, 1, 1);
    let sigma = vec![[[0.0_f64; 3]; 3]; nodes.len()];
    max_inner_ring_von_mises(&nodes, &sigma, 0.5, 1e-9);
}

// ─── slender cantilever P2 tip-deflection ≤1% validation ────────────────────

/// Slender-cantilever P2 tip-deflection validation against Timoshenko (≤1%).
///
/// # Why this fixture achieves ≤1%
///
/// At L/H = 15 the 1-D Timoshenko reference is accurate vs 3-D elasticity to
/// ~0.04% (the residual from 2.1% at L/H=2 scales as ~(H/L)²). The remaining
/// error is P2-FEA discretisation (bending), which P2 tets suppress far better
/// than P1 (no locking) and which is mesh-reducible. Total error clears 1% with
/// wide margin. P1 is excluded: it bending-locks badly at this slenderness.
///
/// # Why a raised CG cap is required
///
/// Slender beams are ill-conditioned — the condition number of the stiffness
/// matrix grows as ~(L/H)², so at L/H=15 it is ~225× worse than L/H=1. The
/// default Jacobi-preconditioned CG cap of 1000 iterations is insufficient; a
/// raised cap (e.g. 20000) is needed. This is the blocker task 2928 documented
/// (CG non-convergence at L/H≥4) that this task resolves.
///
/// # Geometry / material
///
/// `L × H × B = 15 × 1 × 0.5` (dimensionless), `E = 1`, `ν = 0.3`.
///
/// # Load / measurement (faithful — see module header)
///
/// `x=0` fully clamped; end shear of resultant `F = 1` in −y **distributed
/// over the free-end face**; tip deflection = **mean −y displacement over
/// that face** (neutral-axis deflection, mesh-stable).
#[test]
fn cantilever_beam_p2_tip_deflection_slender_within_1pct_of_timoshenko() {
    const L: f64 = 15.0;
    const H: f64 = 1.0;
    const B: f64 = 0.5;
    const NX: usize = 40; // along x (length)
    const NY: usize = 6;  // along y (height) — bending resolution
    const NZ: usize = 3;  // along z (width)
    const F: f64 = 1.0;

    let mat = IsotropicElastic { youngs_modulus: 1.0, poisson_ratio: 0.3 };

    let (p2_nodes, p2_conns, bcs, end) = cantilever_clamped_p2(L, H, B, NX, NY, NZ);
    let n_nodes = p2_nodes.len();
    let loads = distributed_tip_load(&end, F);
    let u = solve_p2_pipeline_with_opts(
        &p2_nodes, &p2_conns, bcs, &loads, &mat,
        // Slender beams are ill-conditioned (cond ∝ (L/H)²); 20 000 iterations
        // provides the head-room the default-1000 cap lacks at L/H=15.
        // Tolerance stays at 1e-8 (same convergence criterion, only budget grows).
        CgSolverOptions { tolerance: 1e-8, max_iter: 20_000 },
    );

    let tip_disp = mean_tip_deflection(&u, &end);
    let delta_ref = timoshenko_tip_deflection(F, L, H, B, &mat);

    let rel_err = (tip_disp - delta_ref).abs() / delta_ref;
    assert!(
        rel_err <= 0.01,
        "cantilever P2 slender: tip deflection {tip_disp:.6e} vs Timoshenko reference \
         {delta_ref:.6e} — relative error {:.2}% > 1% tolerance \
         (mesh: {NX}×{NY}×{NZ}, n_nodes={n_nodes})",
        rel_err * 100.0,
    );
}

// ─── thick-walled cylinder P2 validation ────────────────────────────────────

/// Promote P1 inner-surface triangles to P2 6-node triangles.
///
/// For each P1 corner triple `(v0, v1, v2)`, the three edge-midpoint nodes
/// `m_{01}`, `m_{12}`, `m_{20}` are recovered from `p2_nodes` by coordinate
/// lookup (via [`find_node_at`]) at the exact geometric midpoints — no hash
/// map threading required (see design decision: coordinate-lookup midpoint
/// recovery).
///
/// Returns faces in canonical P2Tri order `[v0, v1, v2, m_{01}, m_{12}, m_{20}]`.
fn p2_inner_faces(
    p1_inner_faces: &[[usize; 3]],
    p2_nodes: &[[f64; 3]],
) -> Vec<[usize; 6]> {
    let tol = 1e-9;
    p1_inner_faces.iter().map(|&[v0, v1, v2]| {
        let midpoint = |a: usize, b: usize| -> [f64; 3] {
            let pa = p2_nodes[a];
            let pb = p2_nodes[b];
            [
                0.5 * (pa[0] + pb[0]),
                0.5 * (pa[1] + pb[1]),
                0.5 * (pa[2] + pb[2]),
            ]
        };
        let m01 = find_node_at(p2_nodes, midpoint(v0, v1), tol);
        let m12 = find_node_at(p2_nodes, midpoint(v1, v2), tol);
        let m20 = find_node_at(p2_nodes, midpoint(v2, v0), tol);
        [v0, v1, v2, m01, m12, m20]
    }).collect()
}

/// Thick-walled cylinder (Lamé) P2 max von Mises ≤ 2 % of reference.
///
/// Promotes the P1 corner mesh to P2 via [`add_edge_midpoint_nodes`];
/// builds P2 inner faces via `p2_inner_faces` (corner triple +
/// coordinate-looked-up edge midpoints in `[v0,v1,v2,m01,m12,m20]` order).
/// Inner pressure assembled with `FaceOrder::P2Tri`; solved via
/// `solve_p2_pipeline_with_opts` (raised CG cap — default 1000 iterations is
/// insufficient at the resolution required for ≤2%; the cylinder is
/// well-conditioned, b/a=2, so 5000 iterations comfortably suffices).
#[test]
fn thick_walled_cylinder_p2_max_von_mises_within_2pct_of_lame() {
    const A: f64 = 1.0;
    const B: f64 = 2.0;
    const L: f64 = 1.0;
    const P_I: f64 = 1.0;
    const NR: usize = 22;
    const NTHETA: usize = 22;
    const NZ: usize = 2;
    const E: f64 = 1.0;
    const NU: f64 = 0.3;
    let tol_geom: f64 = 1e-9;

    let mat = IsotropicElastic { youngs_modulus: E, poisson_ratio: NU };
    let (corner_nodes, p1_conns, p1_inner_faces) = annular_p1_mesh(A, B, L, NR, NTHETA, NZ);
    let (p2_nodes, p2_conns) = add_edge_midpoint_nodes(&corner_nodes, &p1_conns);
    let n_nodes = p2_nodes.len();

    // Promote corner triangles to 6-node P2 faces for consistent force assembly
    let p2_inner = p2_inner_faces(&p1_inner_faces, &p2_nodes);

    // Symmetry + plane-strain BCs on the P2 mesh
    let bcs = annular_symmetry_plane_strain_bcs(&p2_nodes, L, tol_geom);

    // Assemble inner-pressure loads via the P2 helper
    let loads = inner_pressure_loads_p2(&p2_inner, &p2_nodes, P_I);

    // Default CG cap (1000) is insufficient at the resolution needed for ≤2%;
    // use a raised cap. Cylinder is well-conditioned (b/a=2, no slenderness),
    // so 5000 iterations provides the required head-room.
    let u = solve_p2_pipeline_with_opts(
        &p2_nodes, &p2_conns, bcs, &loads, &mat,
        CgSolverOptions { tolerance: 1e-8, max_iter: 5_000 },
    );

    // Recover nodal stress + max von Mises over inner-ring nodes (r ≈ a).
    // Note: only corner nodes and axial-edge midpoints lie exactly at r=a.
    // Angular edge-midpoint nodes sit at the chord midpoint of two arc points
    // (radius = a·cos(Δθ/2) < a) and are excluded by tol_geom=1e-9.  The
    // Lamé peak is at r=a and is fully captured by the included corner nodes.
    let sigma = recover_nodal_p2(&p2_nodes, &p2_conns, &u, &mat);
    let lame_ref = lame_von_mises(A, A, B, P_I, NU);
    let max_vm = max_inner_ring_von_mises(&p2_nodes, &sigma, A, tol_geom);

    let rel_err = (max_vm - lame_ref).abs() / lame_ref;
    assert!(
        rel_err <= 0.02,
        "cylinder P2: max von Mises {max_vm:.6} vs Lamé ref {lame_ref:.6} \
         — rel err {:.2}% > 2% (mesh: {NR}×{NTHETA}×{NZ}, n_nodes={n_nodes})",
        rel_err * 100.0,
    );
}
