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
    IsotropicElastic, RefinementBudget, SolverMode, StressElement, ZzIndicator,
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
