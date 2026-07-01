//! A-posteriori adaptive-refinement validation suite: analytical convergence
//! study.
//!
//! PRD reference: `docs/prds/v0_4/a-posteriori-error-estimation.md`.
//!
//! # Scope
//!
//! Kernel-level convergence study validating the v0.4 a-posteriori
//! adaptive-refinement stack now on `main`: A1 the Z-Z error indicator
//! ([`compute_zz_indicator`], task #2996), A2 the refinement loop control
//! ([`run_adaptive_refinement`] + [`ConvergenceStatus`] / [`BudgetReason`] +
//! [`mark_dorfler`], task #2997), and A4 the Gmsh size-field refiner
//! ([`refine_marked_elements`] -> `volume_refine::refine_with_size_field`,
//! task #2999). Three analytical reference cases exercise the full
//! `solve -> estimate -> mark -> refine -> re-solve` loop end to end:
//!
//! - **(a) L-shaped re-entrant-corner singularity** — per-element indicator
//!   localization at the concave edge, plus the directional adaptive-vs-uniform
//!   log-log convergence-rate gap (the PRD CI gate).
//! - **(b) Plate-with-hole stress concentration** — indicator localization on
//!   the hole perimeter, plus the recovered peak von Mises monotonically
//!   approaching the analytical Kirsch SCF (`≈ 3·σ_far`) from below.
//! - **(c) Cantilever smooth control** — monotone global-indicator drop and
//!   `convergence_status` termination-reason coverage; also the control that
//!   gives the L-shaped rate gap its meaning (a smooth solution should NOT be
//!   materially worse off adaptive-vs-uniform).
//!
//! # Out of scope: (d) auto-resolve and (e) morph composition
//!
//! The PRD's parts (d) (`thickness=auto` vs `max_von_mises`, per-probe
//! `target_accuracy`) and (e) (morph slides / cache-hits / refine-now /
//! morph-reestablish) are deliberately NOT covered here. Both require
//! DSL/eval/morph-cache infrastructure that is verified absent from `main`:
//! `reify-eval`'s `elastic_static` compute target returns a hardcoded
//! non-adaptive `Converged{0.0}` and never calls [`run_adaptive_refinement`];
//! there is no per-probe `target_accuracy` (0.01 near / 0.10 far)
//! classification anywhere; and there is no morph-cache-invalidation-on-
//! refinement. These are pending task 3000's deliverables (not a declared dep
//! of this task) — see escalation esc-3002-72 for the recommended
//! 3000-gated follow-up.
//!
//! # Gmsh gating: runtime `GMSH_AVAILABLE`, NOT `cfg(has_gmsh)`
//!
//! Adaptive LOCAL refinement is Gmsh-only ([`refine_marked_elements`] ->
//! `volume_refine::refine_with_size_field`; no procedural local-refine path
//! exists), so every test that drives a real refine needs libgmsh linked in.
//!
//! `crates/reify-kernel-gmsh/tests/refine_volume_tests.rs` gates on
//! `#![cfg(has_gmsh)]` — but that works there only because
//! `reify-kernel-gmsh`'s OWN `build.rs` detects libgmsh and emits
//! `cargo:rustc-cfg=has_gmsh` for ITS OWN crate. A `rustc-cfg` emitted by one
//! crate's build script does NOT propagate to dependent crates (confirmed by
//! `reify-eval/build.rs`, which independently re-derives `has_gmsh` for
//! itself for exactly this reason — see its comment "A cfg emitted by the
//! gmsh crate's build.rs does NOT propagate to dependents"). This file lives
//! in `reify-solver-elastic`, whose `build.rs` only emits test-binary RPATH
//! directives and does not re-derive `has_gmsh` — so a file-level
//! `#![cfg(has_gmsh)]` here would never be set and would silently compile
//! this entire suite to an empty binary on EVERY host, including ones with
//! libgmsh, defeating the suite's purpose.
//!
//! Instead, this file follows the established same-crate convention from
//! `tests/volume_refine_tests.rs`: tests that need a real Gmsh remesh
//! runtime-guard on `reify_kernel_gmsh::GMSH_AVAILABLE` and return early
//! (with an `eprintln!`) when it is `false`. This preserves the same "stub
//! build stays all-OK" posture while still actually exercising the real
//! pipeline on hosts where gmsh is present. Pure-math tests (e.g.
//! `loglog_slope`, `characteristic_size_from_volume`) run unconditionally —
//! no gmsh involved.
//!
//! # Cheap always-on gate vs. heavy `#[ignore]` rate studies
//!
//! Each adaptive iteration is a full Gmsh remesh + CG solve, so a multi-step
//! adaptive AND uniform sequence over three geometric cases is too heavy for
//! always-on CI. Localization, monotone-drop, and `convergence_status`
//! coverage assertions on small meshes / few iterations stay always-on and
//! carry the CI gate; the rigorous full log-log slope-gap rate studies are
//! `#[ignore]`'d for on-demand/nightly runs — mirroring
//! `tests/analytical_validation.rs::cantilever_faithful_convergence_study`.
//!
//! # Reused FEA harness
//!
//! The procedural box-mesh + Dirichlet/load helpers below are ported from
//! `tests/analytical_validation.rs` (task #2928's harness) rather than
//! re-implemented, so the solve pipeline shape (`element_stiffness` ->
//! `assemble_global_stiffness` -> `apply_dirichlet_row_elimination` ->
//! `solve_cg`) is identical to the rest of the FEA validation suite.

#[allow(unused_imports)] // consumed incrementally by steps 3-18 of this suite
use reify_ir::{ElementOrderTag, Mesh, VolumeMesh};
#[allow(unused_imports)] // consumed incrementally by steps 3-18 of this suite
use reify_kernel_gmsh::{MeshingOptions, refine_volume_with_size_field};
#[allow(unused_imports)] // consumed incrementally by steps 3-18 of this suite
use reify_solver_elastic::{
    AdaptiveEstimate, AdaptiveProblem, AssemblyElement, AssemblyMode, BudgetReason, CgResult,
    CgSolverOptions, ConvergenceStatus, DORFLER_THETA, DirichletBc, ElementOrder, ElementStiffness,
    IsotropicElastic, RefineError, RefinementBudget, SolverMode, StressElement, ZzIndicator,
    apply_dirichlet_row_elimination, assemble_global_stiffness, compute_zz_indicator,
    element_stiffness, element_stress_p1, mark_dorfler, refine_marked_elements,
    run_adaptive_refinement, solve_cg, tet_volume_p1,
};

// ─── ported FEA harness helpers (tests/analytical_validation.rs, task 2928) ─

/// Split a hex cell into 6 tetrahedra via the Kuhn triangulation.
///
/// Ported verbatim from `tests/analytical_validation.rs`; private dependency
/// of [`box_p1_mesh`].
#[allow(dead_code)] // consumed by box_p1_mesh, used starting step-3
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
/// Ported verbatim from `tests/analytical_validation.rs`. Used starting
/// step-3 both as the cantilever control fixture and as the procedural seed
/// for the cheap box-shaped `convergence_status` fixture (step-17/18).
#[allow(dead_code)] // used starting step-3
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

/// Build Dirichlet BCs to fix all 3 DOFs on nodes within `tol` of
/// `nodes[n][axis] == value`.
///
/// Ported verbatim from `tests/analytical_validation.rs`. Used starting
/// step-3 to clamp the cantilever's `x=0` face.
#[allow(dead_code)] // used starting step-3
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
/// Ported verbatim from `tests/analytical_validation.rs`. Used starting
/// step-3 to identify the cantilever's loaded/measured end face.
#[allow(dead_code)] // used starting step-3
fn end_face_nodes(nodes: &[[f64; 3]], l: f64, tol: f64) -> Vec<usize> {
    nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| (n[0] - l).abs() < tol)
        .map(|(i, _)| i)
        .collect()
}

/// Distribute a transverse shear resultant `f_mag` (in -y) equally over the
/// `end` nodes — a set of nodal point loads whose resultant is exactly
/// `-f_mag` at `x = l`. By Saint-Venant the equal split is immaterial to the
/// tip deflection (only the resultant matters far from the end).
///
/// Ported verbatim from `tests/analytical_validation.rs`. Used starting
/// step-3 to load the cantilever's free end.
#[allow(dead_code)] // used starting step-3
fn distributed_tip_load(end: &[usize], f_mag: f64) -> Vec<(usize, f64)> {
    let per = f_mag / end.len() as f64;
    end.iter().map(|&n| (n * 3 + 1, -per)).collect()
}

/// Compute von Mises stress from a 3x3 Cauchy stress tensor.
///
/// Ported verbatim from `tests/analytical_validation.rs`. Used starting
/// step-15/16 to track the peak von Mises across the plate-with-hole
/// refinement sequence (compared against the analytical Kirsch SCF).
#[allow(dead_code)] // used starting step-15/16
fn von_mises_of_tensor(s: &[[f64; 3]; 3]) -> f64 {
    let (s11, s22, s33) = (s[0][0], s[1][1], s[2][2]);
    let (s12, s23, s13) = (s[0][1], s[1][2], s[0][2]);
    let v = 0.5 * ((s11 - s22).powi(2) + (s22 - s33).powi(2) + (s33 - s11).powi(2))
        + 3.0 * (s12.powi(2) + s23.powi(2) + s13.powi(2));
    v.sqrt()
}

// ─── step-1/2: pure, no-FEA convergence-study helpers ──────────────────────
//
// `loglog_slope` and `characteristic_size_from_volume` have no FEA or gmsh
// dependency: they are exercised directly by the tests below.

/// Least-squares slope of `ln(error)` vs `ln(dof)` over `points` (`(dof,
/// error)` pairs in element order) — the convergence-rate exponent of an
/// `error ≈ C · dof^(slope)` power law. A more negative slope is a faster
/// (better) convergence rate; this is the measurement [`run_adaptive_refinement`]
/// sequences are reduced to for the adaptive-vs-uniform rate-gap studies.
///
/// # Algorithm
///
/// Ordinary least squares on `(x, y) = (ln(dof), ln(error))`:
/// `slope = Σ (x_i - x̄)(y_i - ȳ) / Σ (x_i - x̄)²`.
///
/// # Preconditions
///
/// `points` must have at least 2 entries with positive, finite `dof` and
/// `error` values, and not all-identical `dof` (else the denominator is
/// zero and the result is NaN — the adaptive/uniform sequences this suite
/// builds `points` from always vary `dof` across refinement iterations).
fn loglog_slope(points: &[(f64, f64)]) -> f64 {
    let n = points.len() as f64;
    let xs: Vec<f64> = points.iter().map(|&(dof, _)| dof.ln()).collect();
    let ys: Vec<f64> = points.iter().map(|&(_, err)| err.ln()).collect();
    let x_bar = xs.iter().sum::<f64>() / n;
    let y_bar = ys.iter().sum::<f64>() / n;

    let mut num = 0.0_f64;
    let mut den = 0.0_f64;
    for (&x, &y) in xs.iter().zip(ys.iter()) {
        num += (x - x_bar) * (y - y_bar);
        den += (x - x_bar) * (x - x_bar);
    }
    num / den
}

/// Cube-root "characteristic edge length" proxy for a tetrahedron of volume
/// `v`: `(6·v)^(1/3)`.
///
/// `6·v` undoes the canonical tet-volume formula `V = |det J| / 6` for a
/// unit-edge reference tet (see [`tet_volume_p1`]'s doc), so this recovers a
/// length on the same scale as the mesh's actual element sizes. Feeds the
/// per-element `current_sizes` recomputation after a Gmsh remesh: the new
/// element volumes are known from [`tet_volume_p1`], but
/// [`refine_marked_elements`]'s `current_sizes` argument wants a
/// characteristic *size* per element, not a volume.
fn characteristic_size_from_volume(v: f64) -> f64 {
    (6.0 * v).cbrt()
}

/// On exact power-law samples `error = C · dof^(-p)`, the least-squares
/// log-log slope must recover `-p` to within `1e-9` regardless of the `dof`
/// spacing.
///
/// Closed-form reasoning: `ln(error) = ln(C) - p·ln(dof)` is an EXACT line in
/// `(ln dof, ln error)` space, so an ordinary-least-squares fit through
/// perfectly collinear points has zero residual and recovers the line's true
/// slope to floating-point precision — no asymptotic / large-sample argument
/// needed. Checked at `p = 0.5` (the singularity-capped uniform rate) and
/// `p = 1.0` (a Dörfler-adaptive-optimal rate) — the two exponents this suite
/// later contrasts on the L-shaped case.
#[test]
fn loglog_slope_recovers_exact_exponent_for_power_law() {
    for &p in &[0.5_f64, 1.0_f64] {
        let c = 2.0_f64;
        let dofs = [10.0_f64, 100.0, 1_000.0, 10_000.0, 100_000.0];
        let points: Vec<(f64, f64)> = dofs.iter().map(|&d| (d, c * d.powf(-p))).collect();

        let slope = loglog_slope(&points);

        assert!(
            (slope - (-p)).abs() < 1e-9,
            "p={p}: loglog_slope = {slope}, expected ≈ {} (exact power-law collinear fit)",
            -p,
        );
    }
}

/// A strictly-decreasing (but not necessarily exact-power-law) error sequence
/// over increasing dof must yield a negative slope — the qualitative
/// "converging" signal the adaptive-loop instrumentation relies on.
#[test]
fn loglog_slope_strictly_decreasing_error_sequence_is_negative() {
    let points = [
        (10.0_f64, 1.0_f64),
        (50.0, 0.6),
        (200.0, 0.35),
        (1_000.0, 0.1),
    ];

    let slope = loglog_slope(&points);

    assert!(
        slope < 0.0,
        "strictly-decreasing error must give a negative log-log slope, got {slope}",
    );
}

/// `characteristic_size_from_volume(v) = (6·v)^(1/3)` is the cube-root "edge
/// length" proxy that undoes the canonical tet-volume formula `V = |det J| /
/// 6` for a unit reference tet. Pinned at the canonical unit tet (`V = 1/6`
/// ⇒ size `1.0`) and the edge-doubled tet (`V = 8/6` ⇒ size `2.0`, mirroring
/// `tet_volume_p1`'s own `V ∝ L³` scaling pin in `src/result.rs`).
#[test]
fn characteristic_size_from_volume_recovers_cube_root_edge_proxy_for_known_tet_volume() {
    let unit = characteristic_size_from_volume(1.0 / 6.0);
    assert!(
        (unit - 1.0).abs() < 1e-12,
        "characteristic_size_from_volume(1/6) = {unit}, expected 1.0 (unit tet)",
    );

    let doubled = characteristic_size_from_volume(8.0 / 6.0);
    assert!(
        (doubled - 2.0).abs() < 1e-12,
        "characteristic_size_from_volume(8/6) = {doubled}, expected 2.0 (edge-doubled tet)",
    );
}

// ─── step-4: FeaAdaptiveProblem — real solve → estimate wiring ─────────────
//
// GREEN (this commit): defines `FeaAdaptiveProblem` and wires
// `AdaptiveProblem::solve_and_estimate` through the real pipeline. `refine`
// is a `todo!()` stub until step-6 — step-3/4 is scoped to
// `solve_and_estimate` only (see plan step-5/6 for `refine`).

/// Gather the 12 element DOFs (`[u_x,u_y,u_z]` per corner) for a P1 tet from
/// the global displacement vector, in element-local node order.
///
/// Ported verbatim from `tests/analytical_validation.rs`.
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
/// Ported verbatim from `tests/analytical_validation.rs`.
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

/// Assemble, apply BCs, and CG-solve a P1 tetrahedral FEA system. Uses
/// `SolverMode::Deterministic` for bit-stable, CI-safe results.
///
/// Ported verbatim from `tests/analytical_validation.rs::solve_p1_pipeline`
/// — the exact pipeline shape [`FeaAdaptiveProblem::solve_and_estimate`]
/// reuses.
///
/// # Returns
///
/// Displacement vector `u` of length `3 * nodes.len()`.
fn solve_p1_pipeline(
    nodes: &[[f64; 3]],
    conns: &[[usize; 4]],
    bcs: &mut Vec<DirichletBc>,
    point_loads: &[(usize, f64)],
    mat: &IsotropicElastic,
) -> Vec<f64> {
    let n_nodes = nodes.len();
    let ndof = 3 * n_nodes;

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

    let mut f = vec![0.0_f64; ndof];
    for &(dof, val) in point_loads {
        f[dof] += val;
    }

    dedup_bcs(bcs);
    apply_dirichlet_row_elimination(&mut k, &mut f, bcs);

    let opts = CgSolverOptions::default();
    let result: CgResult = solve_cg(&k, &f, opts, SolverMode::Deterministic);
    assert!(
        result.converged,
        "solve_p1_pipeline: CG did not converge (iterations={})",
        result.iterations,
    );
    result.u().to_vec()
}

/// Pack `(nodes, conns)` (the procedural [`box_p1_mesh`] output format) into
/// a [`VolumeMesh`] (`f32` flat vertices + `u32` flat tet indices, `P1`
/// order).
///
/// Bridges the solve pipeline's native `(nodes, conns)` shape to the
/// [`VolumeMesh`] shape [`FeaAdaptiveProblem`] stores internally — needed by
/// both [`compute_zz_indicator`] (reads `mesh.vertices.len()`) and `refine`
/// (calls [`refine_marked_elements`], starting step-6).
fn volume_mesh_from_nodes_conns(nodes: &[[f64; 3]], conns: &[[usize; 4]]) -> VolumeMesh {
    let mut vertices = Vec::with_capacity(nodes.len() * 3);
    for n in nodes {
        vertices.push(n[0] as f32);
        vertices.push(n[1] as f32);
        vertices.push(n[2] as f32);
    }
    let mut tet_indices = Vec::with_capacity(conns.len() * 4);
    for c in conns {
        tet_indices.extend(c.iter().map(|&idx| idx as u32));
    }
    VolumeMesh {
        vertices,
        tet_indices,
        element_order: ElementOrderTag::P1,
        normals: None,
        boundary: None,
    }
}

/// Unpack a `P1` [`VolumeMesh`] into `(nodes, conns)` — the inverse of
/// [`volume_mesh_from_nodes_conns`]. `nodes` widen the mesh's `f32`
/// coordinates to `f64` (the solve pipeline's native precision).
///
/// # Panics
///
/// If `volume_mesh.element_order != ElementOrderTag::P1` — this suite (and
/// `compute_zz_indicator`) supports P1 tets only.
fn nodes_conns_from_volume_mesh(volume_mesh: &VolumeMesh) -> (Vec<[f64; 3]>, Vec<[usize; 4]>) {
    assert_eq!(
        volume_mesh.element_order,
        ElementOrderTag::P1,
        "FeaAdaptiveProblem supports P1 tets only",
    );
    let nodes: Vec<[f64; 3]> = volume_mesh
        .vertices
        .chunks_exact(3)
        .map(|c| [c[0] as f64, c[1] as f64, c[2] as f64])
        .collect();
    let conns: Vec<[usize; 4]> = volume_mesh
        .tet_indices
        .chunks_exact(4)
        .map(|c| [c[0] as usize, c[1] as usize, c[2] as usize, c[3] as usize])
        .collect();
    (nodes, conns)
}

/// Closure signature for [`FeaAdaptiveProblem`]'s Dirichlet-BC generator:
/// re-evaluated against the CURRENT node positions on every solve (see the
/// struct's "BCs/loads are geometric predicates" doc section below).
type BcsFn = Box<dyn Fn(&[[f64; 3]]) -> Vec<DirichletBc>>;

/// Closure signature for [`FeaAdaptiveProblem`]'s point-load generator; see
/// [`BcsFn`].
type LoadsFn = Box<dyn Fn(&[[f64; 3]]) -> Vec<(usize, f64)>>;

/// Test-local [`AdaptiveProblem`] implementation wiring the real FEA
/// pipeline into the a-posteriori loop:
/// [`element_stiffness`] -> [`assemble_global_stiffness`] ->
/// [`apply_dirichlet_row_elimination`] -> [`solve_cg`] (via
/// [`solve_p1_pipeline`]) -> [`element_stress_p1`] + [`tet_volume_p1`] ->
/// [`StressElement`] -> [`compute_zz_indicator`] for
/// [`AdaptiveProblem::solve_and_estimate`]; [`refine_marked_elements`] for
/// [`AdaptiveProblem::refine`] (wired starting step-6).
///
/// # BCs/loads are geometric predicates, not fixed node indices
///
/// `refine` performs a **full remesh from `surface`** (see the module doc of
/// `reify_solver_elastic::volume_refine`, whose entry point
/// [`refine_marked_elements`] calls): node indices are NOT preserved across
/// a refine. `bcs_fn`/`loads_fn` are therefore closures over the CURRENT
/// node *positions*, re-evaluated fresh on every `solve_and_estimate` call —
/// mirroring [`dirichlet_fix_face`]/[`distributed_tip_load`]'s own
/// position-based (not index-based) contract.
struct FeaAdaptiveProblem {
    /// Current volume mesh (`P1` tets). Replaced wholesale by `refine`.
    volume_mesh: VolumeMesh,
    /// The closed surface boundary `volume_mesh` was meshed from; forwarded
    /// unchanged to [`refine_marked_elements`] for the full remesh from
    /// surface.
    #[allow(dead_code)] // read starting step-6 (refine)
    surface: Mesh,
    material: IsotropicElastic,
    /// One characteristic size per element of `volume_mesh`, in element
    /// order; recomputed after every remesh via
    /// [`characteristic_size_from_volume`].
    #[allow(dead_code)] // read starting step-6 (refine)
    current_sizes: Vec<f64>,
    #[allow(dead_code)] // read starting step-6 (refine)
    meshing_options: MeshingOptions,
    bcs_fn: BcsFn,
    loads_fn: LoadsFn,
}

impl FeaAdaptiveProblem {
    /// Build a problem from an initial `(nodes, conns)` mesh (the procedural
    /// [`box_p1_mesh`] output format). `current_sizes` is computed fresh
    /// from each element's volume via [`characteristic_size_from_volume`],
    /// so it is correct from the very first solve — not just after the
    /// first `refine`.
    fn new(
        nodes: &[[f64; 3]],
        conns: &[[usize; 4]],
        surface: Mesh,
        material: IsotropicElastic,
        meshing_options: MeshingOptions,
        bcs_fn: BcsFn,
        loads_fn: LoadsFn,
    ) -> Self {
        let current_sizes = conns
            .iter()
            .map(|c| {
                let elem_nodes = [nodes[c[0]], nodes[c[1]], nodes[c[2]], nodes[c[3]]];
                characteristic_size_from_volume(tet_volume_p1(&elem_nodes))
            })
            .collect();
        Self {
            volume_mesh: volume_mesh_from_nodes_conns(nodes, conns),
            surface,
            material,
            current_sizes,
            meshing_options,
            bcs_fn,
            loads_fn,
        }
    }
}

impl AdaptiveProblem for FeaAdaptiveProblem {
    type Error = RefineError;

    fn solve_and_estimate(&mut self) -> AdaptiveEstimate {
        let (nodes, conns) = nodes_conns_from_volume_mesh(&self.volume_mesh);
        let n_nodes = nodes.len();

        let mut bcs = (self.bcs_fn)(&nodes);
        let point_loads = (self.loads_fn)(&nodes);
        let u = solve_p1_pipeline(&nodes, &conns, &mut bcs, &point_loads, &self.material);

        let mut per_stress: Vec<[[f64; 3]; 3]> = Vec::with_capacity(conns.len());
        let mut per_volume: Vec<f64> = Vec::with_capacity(conns.len());
        for conn in &conns {
            let elem_nodes = [
                nodes[conn[0]],
                nodes[conn[1]],
                nodes[conn[2]],
                nodes[conn[3]],
            ];
            let ue = gather_u_p1(&u, conn);
            per_stress.push(element_stress_p1(&elem_nodes, &self.material, &ue));
            per_volume.push(tet_volume_p1(&elem_nodes));
        }

        let stress_elements: Vec<StressElement<'_>> = conns
            .iter()
            .enumerate()
            .map(|(i, conn)| StressElement {
                connectivity: conn.as_slice(),
                stress: per_stress[i],
                volume: per_volume[i],
            })
            .collect();

        let zz = compute_zz_indicator(&stress_elements, &self.volume_mesh, &self.material);

        AdaptiveEstimate {
            global_indicator: zz.global_relative_energy_error,
            per_element: zz.per_element,
            n_dofs: 3 * n_nodes,
        }
    }

    fn refine(&mut self, _marked: &[usize]) -> Result<(), Self::Error> {
        // Wired in step-6 #3002 (this task): call refine_marked_elements,
        // replace self.volume_mesh, and recompute self.current_sizes from
        // the new mesh's per-element volumes.
        todo!("FeaAdaptiveProblem::refine wired in step-6 #3002")
    }
}

// ─── step-3/4: FeaAdaptiveProblem test fixtures ────────────────────────────

/// Minimal single-triangle placeholder surface. `solve_and_estimate` never
/// touches `surface` (only `refine` does, starting step-5/6), so a
/// non-degenerate closed geometry is not required here — mirrors
/// `dummy_surface` in `tests/adaptive_refinement_tests.rs` /
/// `tests/volume_refine_tests.rs`.
fn dummy_surface() -> Mesh {
    Mesh {
        vertices: vec![0.0_f32; 9],
        indices: vec![0, 1, 2],
        normals: None,
    }
}

/// Prescribe the full 3-DOF displacement `field(x)` on every node lying on
/// any of the six bounding planes of `[0,lx]x[0,ly]x[0,lz]` (within `tol`).
///
/// Ported verbatim from `tests/analytical_validation.rs`. Used starting
/// step-3 for the Zienkiewicz constant-strain patch-test fixture:
/// prescribing the exact linear field `u = (gamma·y, 0, 0)` on the whole
/// boundary makes the interior solution that same field (P1 tets represent
/// it exactly), yielding a uniform stress state and — the property this
/// suite's patch test checks — a ~zero Z-Z indicator.
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

/// Cantilever-style box fixture: `[0,2]x[0,1]x[0,1]`, clamped at `x=0`,
/// distributed shear load at the free end `x=2`. A non-uniform (bending)
/// stress state, exercising the "real" (not patch-test) wiring path.
fn cantilever_box_problem() -> FeaAdaptiveProblem {
    let (nodes, conns) = box_p1_mesh(2.0, 1.0, 1.0, 4, 2, 2);
    let tol_x = 0.5 * 2.0 / 4.0; // half the x-spacing
    let mat = IsotropicElastic {
        youngs_modulus: 1.0,
        poisson_ratio: 0.3,
    };
    FeaAdaptiveProblem::new(
        &nodes,
        &conns,
        dummy_surface(),
        mat,
        MeshingOptions::default(),
        Box::new(move |ns: &[[f64; 3]]| dirichlet_fix_face(ns, 0, 0.0, tol_x)),
        Box::new(move |ns: &[[f64; 3]]| {
            let end = end_face_nodes(ns, 2.0, tol_x);
            distributed_tip_load(&end, 1.0)
        }),
    )
}

/// Zienkiewicz constant-strain patch-test fixture: unit cube with the exact
/// linear field `u = (0.1·y, 0, 0)` prescribed on the whole boundary and no
/// other loads. P1 tets reproduce a linear field exactly in the interior, so
/// the solve is exact up to CG tolerance — a uniform stress state.
fn patch_test_box_problem() -> FeaAdaptiveProblem {
    let (nodes, conns) = box_p1_mesh(1.0, 1.0, 1.0, 2, 2, 2);
    let tol = 0.25 / 2.0; // < half the 1/N spacing
    const GAMMA: f64 = 0.1;
    let mat = IsotropicElastic {
        youngs_modulus: 1.0,
        poisson_ratio: 0.3,
    };
    FeaAdaptiveProblem::new(
        &nodes,
        &conns,
        dummy_surface(),
        mat,
        MeshingOptions::default(),
        Box::new(move |ns: &[[f64; 3]]| {
            dirichlet_prescribe_boundary_field(ns, 1.0, 1.0, 1.0, tol, |p| [GAMMA * p[1], 0.0, 0.0])
        }),
        Box::new(|_ns: &[[f64; 3]]| Vec::new()),
    )
}

/// `solve_and_estimate` under a non-uniform (cantilever bending) stress
/// state: the estimate's shape must match the mesh (`n_dofs = 3*n_nodes`,
/// one indicator per element) and the global indicator must be a finite,
/// strictly positive number.
#[test]
fn fea_adaptive_problem_solve_and_estimate_matches_mesh_shape_under_nonuniform_stress() {
    let mut problem = cantilever_box_problem();
    let n_nodes = 5 * 3 * 3; // (nx+1)*(ny+1)*(nz+1) for nx=4,ny=2,nz=2
    let n_elements = 6 * 4 * 2 * 2; // 6 tets per hex cell

    let estimate = problem.solve_and_estimate();

    assert_eq!(
        estimate.n_dofs,
        3 * n_nodes,
        "n_dofs must equal 3 * n_nodes"
    );
    assert_eq!(
        estimate.per_element.len(),
        n_elements,
        "per_element must have one entry per mesh element",
    );
    assert!(
        estimate.global_indicator.is_finite(),
        "global_indicator must be finite, got {}",
        estimate.global_indicator,
    );
    assert!(
        estimate.global_indicator > 0.0,
        "cantilever bending is a non-uniform stress state, so the ZZ \
         indicator must be strictly positive; got {}",
        estimate.global_indicator,
    );
}

/// Zienkiewicz patch-test sanity: under a prescribed exact linear
/// (constant-strain) boundary field, the solve→stress→ZZ wiring must
/// recover a ~zero global indicator, confirming the pipeline reproduces the
/// textbook patch-test property already pinned at the `compute_zz_indicator`
/// unit level
/// (`crate::error_estimator::tests::uniform_stress_field_yields_zero_per_element_indicator_and_zero_global_error`
/// in `src/error_estimator.rs`) end to end through a real CG solve.
#[test]
fn fea_adaptive_problem_solve_and_estimate_patch_test_yields_near_zero_indicator() {
    let mut problem = patch_test_box_problem();

    let estimate = problem.solve_and_estimate();

    // Conservative bound: CG converges to 1e-8 relative residual and the
    // patch-test property is coordinate-perturbation-agnostic (it holds for
    // any conforming P1 tessellation, so the VolumeMesh's f32 vertex
    // rounding does not break it) — the residual global_indicator should sit
    // many orders below this eps, dominated by CG's own tolerance.
    let eps = 1e-4;
    assert!(
        estimate.global_indicator.abs() <= eps,
        "Zienkiewicz patch test: prescribing an exact linear field on the \
         whole boundary must yield a ~zero global indicator; got {} (eps={eps})",
        estimate.global_indicator,
    );
}
