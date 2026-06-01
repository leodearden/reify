//! Flat-plate MITC3 cantilever shell driver (PRD
//! `docs/prds/v0_4/shell-extract-engine-bridge.md` task δ, §3/§5/§7).
//!
//! `solve_flat_plate_shell` synthesizes a structured triangulated mid-surface
//! mesh of an `length × width` rectangle in the XY plane (thickness `t`),
//! clamps the `x == 0` root edge, distributes a `-Z` tip load across the
//! `x == length` free edge, assembles + solves the MITC3 shell system, and
//! recovers per-element [`ShellElementStress`] + per-element [`ShellFrame`].
//!
//! This is the neutral-types kernel driver: it returns only solver-elastic
//! types (no `ShellChannels` — that glue lives in reify-eval, which depends on
//! this crate; see PRD §11 OQ-2 and the task δ design decisions). The recipe
//! is lifted from the proven end-to-end shell solve in
//! `tests/shell_benchmarks.rs` (the flat-plate cantilever sanity test).

use crate::assembly::{AssemblyElement, AssemblyMode, ElementStiffness, assemble_global_stiffness};
use crate::boundary::{DirichletBc, apply_dirichlet_row_elimination};
use crate::constitutive::IsotropicElastic;
use crate::shell_assembly::{
    ShellFrame, build_shell_frame, shell_element_stiffness, shell_element_stiffness_mitc3_plus,
};
use crate::shell_boundary::{SupportBodyKind, SupportKind, build_support_bcs};
use crate::shell_result::{ShellElementStress, shell_element_stress};
use crate::solver::{CgSolverOptions, SolverMode, solve_cg};

/// Per-element MITC3 stress + frame recovery from a flat-plate cantilever shell
/// solve. Neutral solver-elastic types only — reify-eval wraps these into the
/// DSL `ShellChannels` / `ShellStress` value (no dependency cycle).
///
/// No `derive`s: `ShellFrame` (shell_assembly.rs) implements none of
/// `Debug`/`Clone`/`PartialEq`, and the consumers (the inline test + the
/// reify-eval `solve_shell_static` glue) only read the fields by move/borrow.
pub struct FlatPlateShellSolve {
    /// Per-element through-thickness stress (top/mid/bottom 3×3 tensors in the
    /// element LOCAL frame). One entry per triangle, in element order.
    pub stresses: Vec<ShellElementStress>,
    /// Per-element local→global frame (carries `ShellFrame::local_to_global`).
    /// Same length / ordering as `stresses`.
    pub frames: Vec<ShellFrame>,
    /// Whether the CG solve converged within `max_iter`.
    pub converged: bool,
    /// Number of CG iterations performed.
    pub iterations: usize,
}

/// Solve a flat-plate MITC3 cantilever shell and recover per-element stress +
/// frames.
///
/// # Geometry / mesh
///
/// Synthesizes a structured triangulated mid-surface mesh of the
/// `length × width` rectangle in the XY plane at `z = 0` (an `nx × ny` grid of
/// near-square cells, two CCW triangles per cell, 6 DOF/node). The mesh is
/// trampoline-synthesized from the body scalars — NOT the live extracted
/// mid-surface (PRD §11 OQ-2; the shell-extract::extract node is still wired
/// upstream by the engine for the graph contract, but its mesh is not the
/// stress-solve geometry source in v0.4).
///
/// # Boundary conditions / load
///
/// - The `x == 0` root edge is fully clamped via
///   [`build_support_bcs`]`(.., Fixed, Shell)` (6 DOF/node).
/// - The drilling rotation θ_z is pinned to 0 at every non-root node: on a flat
///   patch every element shares the normal e₃ = (0,0,1), so the local θ_z DOF
///   coincides with the global one and MITC3 carries zero stiffness for it —
///   without the pin `K` is rank-deficient and the solve produces NaN (see the
///   `shell_benchmarks.rs` flat-plate sanity test for the same treatment).
/// - The transverse `-Z` `tip_force` is distributed equally across the
///   `x == length` free-edge nodes (all at `x = length`, so the root bending
///   moment is `tip_force · length`, matching the analytical tip-point load).
///
/// # Accuracy
///
/// Bare MITC3 on a flat facet has no curvature → no membrane locking; the
/// recovered root bending stress lands within one order of magnitude of the
/// analytical σ = 6PL/(bh²) (esc-3594-10 honest-accuracy contract). NOT gated
/// on MITC3+ tight accuracy (task 3392).
pub fn solve_flat_plate_shell(
    length: f64,
    width: f64,
    thickness: f64,
    material: &IsotropicElastic,
    tip_force: f64,
) -> FlatPlateShellSolve {
    // ── Mesh resolution ───────────────────────────────────────────────────────
    //
    // Near-square cells: ny ≈ nx · width/length. nx is chosen fine enough that
    // the root element's centroid-sampled (constant-strain) curvature captures
    // the peak bending moment to within the one-OOM band; bending is uniform
    // across the width, so ny can stay small.
    let nx: usize = 20;
    let ny: usize = ((nx as f64 * width / length).round() as usize).max(2);

    let nx1 = nx + 1;
    let ny1 = ny + 1;
    let n_nodes = nx1 * ny1;
    let node = |ix: usize, iy: usize| -> usize { iy * nx1 + ix };

    // ── Nodes: structured grid in the XY plane at z = 0 ──────────────────────
    let mut nodes: Vec<[f64; 3]> = Vec::with_capacity(n_nodes);
    for iy in 0..ny1 {
        let y = iy as f64 * width / ny as f64;
        for ix in 0..nx1 {
            let x = ix as f64 * length / nx as f64;
            nodes.push([x, y, 0.0]);
        }
    }

    // ── Connectivity: two CCW triangles per quad cell ────────────────────────
    let mut connectivity: Vec<[usize; 3]> = Vec::with_capacity(2 * nx * ny);
    for iy in 0..ny {
        for ix in 0..nx {
            let a = node(ix, iy);
            let b = node(ix + 1, iy);
            let c = node(ix, iy + 1);
            let d = node(ix + 1, iy + 1);
            connectivity.push([a, b, d]);
            connectivity.push([a, d, c]);
        }
    }

    // ── Per-element stiffness (production MITC3 path) ────────────────────────
    let stiffness: Vec<ElementStiffness> = connectivity
        .iter()
        .map(|conn| {
            let elem_nodes = [nodes[conn[0]], nodes[conn[1]], nodes[conn[2]]];
            shell_element_stiffness_mitc3_plus(&elem_nodes, thickness, material)
        })
        .collect();
    let elements: Vec<AssemblyElement<'_>> = connectivity
        .iter()
        .zip(stiffness.iter())
        .enumerate()
        .map(|(id, (conn, k_e))| AssemblyElement {
            id,
            connectivity: conn.as_slice(),
            k_e,
        })
        .collect();

    let mut k = assemble_global_stiffness(n_nodes, &elements, AssemblyMode::Deterministic);

    // ── Boundary conditions ──────────────────────────────────────────────────
    let root_nodes: Vec<usize> = (0..ny1).map(|iy| node(0, iy)).collect();
    let (mut bcs, _compat) =
        build_support_bcs(&root_nodes, SupportKind::Fixed, SupportBodyKind::Shell);
    // Pin θ_z (drilling) at every non-root node. Root nodes already have all 6
    // DOFs (incl. θ_z) clamped, so adding θ_z there would be a duplicate DOF
    // (apply_dirichlet_row_elimination panics on duplicates in debug builds).
    let is_root = |n: usize| n % nx1 == 0; // ix == 0
    for n in 0..n_nodes {
        if !is_root(n) {
            bcs.push(DirichletBc {
                dof: n * 6 + 5,
                value: 0.0,
            });
        }
    }

    // ── Load: distribute -Z tip_force across the x == length free edge ───────
    // apply_point_load is a 3-DOF/node helper; shell f is 6-DOF/node, so write
    // the transverse (DOF 2) component directly, mirroring shell_benchmarks.rs.
    let mut f = vec![0.0_f64; 6 * n_nodes];
    let tip_nodes: Vec<usize> = (0..ny1).map(|iy| node(nx, iy)).collect();
    let force_per_node = -tip_force / tip_nodes.len() as f64;
    for &tn in &tip_nodes {
        f[tn * 6 + 2] += force_per_node;
    }

    apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

    // ── Solve ─────────────────────────────────────────────────────────────────
    let opts = CgSolverOptions {
        tolerance: 1e-6,
        max_iter: 5000,
    };
    let cg = solve_cg(&k, &f, opts, SolverMode::Deterministic);
    let u = &cg.u;
    #[cfg(test)]
    {
        use faer::linalg::solvers::Solve;
        let ndof = 6 * n_nodes;
        let kd = k.to_dense();
        let plu = kd.partial_piv_lu();
        let mut rhs = faer::Mat::<f64>::from_fn(ndof, 1, |i, _| f[i]);
        plu.solve_in_place(&mut rhs);
        let ud = rhs.col_as_slice(0usize).to_vec();
        let tip_w_max = tip_nodes.iter().map(|&n| u[n * 6 + 2].abs()).fold(0.0_f64, f64::max);
        let center_iy = ny / 2;
        let tip_center = u[node(nx, center_iy) * 6 + 2].abs();
        let _ = ud;
        let i_beam = width * thickness.powi(3) / 12.0;
        let delta_eb = tip_force * length.powi(3) / (3.0 * material.youngs_modulus * i_beam);
        let iters = cg.iterations;
        eprintln!(
            "DIAG-DRV nx={nx} ny={ny} iters={iters} tip_w_max={tip_w_max:.4e} (r {:.2}) tip_center={tip_center:.4e} (r {:.2}) delta_eb={delta_eb:.4e}",
            tip_w_max / delta_eb, tip_center / delta_eb
        );
    }

    // ── Per-element stress + frame recovery ──────────────────────────────────
    let mut stresses: Vec<ShellElementStress> = Vec::with_capacity(connectivity.len());
    let mut frames: Vec<ShellFrame> = Vec::with_capacity(connectivity.len());
    for conn in &connectivity {
        let elem_nodes = [nodes[conn[0]], nodes[conn[1]], nodes[conn[2]]];
        // Gather the 18-DOF global displacement for this element's 3 nodes.
        let mut u_e = [0.0_f64; 18];
        for (local, &g) in conn.iter().enumerate() {
            for dof in 0..6 {
                u_e[local * 6 + dof] = u[g * 6 + dof];
            }
        }
        stresses.push(shell_element_stress(&elem_nodes, thickness, material, &u_e));
        frames.push(build_shell_frame(&elem_nodes));
    }

    FlatPlateShellSolve {
        stresses,
        frames,
        converged: cg.converged,
        iterations: cg.iterations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constitutive::IsotropicElastic;

    /// Local von Mises from a 3×3 Cauchy tensor (rotation-invariant, so it is
    /// correct to evaluate on the element-LOCAL stress without rotating to
    /// global first — see PRD §11 OQ-1 and the task δ design decision).
    fn von_mises(m: &[[f64; 3]; 3]) -> f64 {
        let (sxx, syy, szz) = (m[0][0], m[1][1], m[2][2]);
        let (sxy, syz, szx) = (m[0][1], m[1][2], m[2][0]);
        (0.5
            * ((sxx - syy).powi(2)
                + (syy - szz).powi(2)
                + (szz - sxx).powi(2)
                + 6.0 * (sxy * sxy + syz * syz + szx * szx)))
        .sqrt()
    }

    /// Max absolute component of a 3×3 tensor.
    fn max_abs(m: &[[f64; 3]; 3]) -> f64 {
        let mut mx = 0.0_f64;
        for row in m {
            for &v in row {
                mx = mx.max(v.abs());
            }
        }
        mx
    }

    #[test]
    fn diag_locate_max() {
        let length = 0.05_f64;
        let width = 0.01_f64;
        let thickness = 0.001_f64;
        let material = IsotropicElastic { youngs_modulus: 205e9, poisson_ratio: 0.29 };
        let solve = solve_flat_plate_shell(length, width, thickness, &material, 10.0);
        let mut idx = 0usize;
        let mut mx = 0.0f64;
        for (i, s) in solve.stresses.iter().enumerate() {
            let vm = von_mises(&s.top);
            if vm > mx { mx = vm; idx = i; }
        }
        let o = solve.frames[idx].origin;
        eprintln!("DIAG nx-elems={} max_top_vm={:.4e} at elem {} origin=[{:.4},{:.4},{:.4}]",
            solve.stresses.len(), mx, idx, o[0], o[1], o[2]);
        // Print a few high elements with their origins.
        let mut v: Vec<(f64,[f64;3])> = solve.stresses.iter().zip(solve.frames.iter())
            .map(|(s,f)| (von_mises(&s.top), f.origin)).collect();
        v.sort_by(|a,b| b.0.partial_cmp(&a.0).unwrap());
        for (vm,o) in v.iter().take(6) {
            eprintln!("  vm={:.4e} origin=[{:.4},{:.4},{:.4}]", vm, o[0],o[1],o[2]);
        }
    }

    /// RED (task δ step-1): pin the flat-plate MITC3 cantilever shell driver.
    ///
    /// Fixture mirrors `examples/fea_shell_flexure.ri`: a 50 mm × 10 mm × 1 mm
    /// steel flexure (E=205 GPa, ν=0.29) with a 10 N transverse tip load.
    ///
    /// # Accuracy basis (esc-3594-10 bare-MITC3 honest band)
    ///
    /// A flat-facet cantilever is the BENIGN MITC3 case: it has NO curvature,
    /// so none of the membrane-locking that drives the 1.7×–2200× errors on the
    /// curved MacNeal-Harder benchmarks (`shell_benchmarks.rs`) applies. The
    /// assertion is therefore a ONE-ORDER-OF-MAGNITUDE band [3e7, 3e9] Pa around
    /// the analytical σ = 6PL/(bh²) = 3e8 Pa — a 10× window, far wider than the
    /// flat-facet method error. No tight (5%) tolerance is asserted.
    #[test]
    fn flat_plate_shell_cantilever_top_von_mises_within_one_oom_of_analytical() {
        let length = 0.05_f64;
        let width = 0.01_f64;
        let thickness = 0.001_f64;
        let material = IsotropicElastic {
            youngs_modulus: 205e9,
            poisson_ratio: 0.29,
        };
        let tip_force = 10.0_f64;

        let solve = solve_flat_plate_shell(length, width, thickness, &material, tip_force);

        assert!(solve.converged, "flat-plate shell CG must converge");
        assert!(
            solve.iterations >= 1,
            "a cold-start CG solve must take at least one iteration, got {}",
            solve.iterations,
        );
        assert!(
            !solve.stresses.is_empty(),
            "driver must recover per-element stresses"
        );
        assert_eq!(
            solve.stresses.len(),
            solve.frames.len(),
            "exactly one local→global frame per element"
        );

        // Analytical reference: σ = 6PL/(bh²) = 6·10·0.05/(0.01·1e-6) = 3e8 Pa.
        let sigma_ref = 6.0 * tip_force * length / (width * thickness.powi(2));
        let lower = 0.1 * sigma_ref; // 3e7 Pa
        let upper = 10.0 * sigma_ref; // 3e9 Pa

        // Peak bending lives at the clamped root; assert max-over-elements (not a
        // fragile root-element index) of the .top layer von Mises.
        let max_top_vm = solve
            .stresses
            .iter()
            .map(|s| von_mises(&s.top))
            .fold(0.0_f64, f64::max);

        assert!(
            max_top_vm.is_finite(),
            "max top von Mises must be finite, got {max_top_vm}"
        );
        assert!(max_top_vm > 0.0, "max top von Mises must be > 0, got {max_top_vm}");
        assert!(
            (lower..=upper).contains(&max_top_vm),
            "max top von Mises {max_top_vm:.4e} Pa outside one-OOM band \
             [{lower:.1e}, {upper:.1e}] around σ=6PL/(bh²)={sigma_ref:.4e} Pa"
        );

        // Real through-thickness bending gradient: the top fibre must carry more
        // stress than the mid (neutral-plane) layer.
        let max_top_abs = solve
            .stresses
            .iter()
            .map(|s| max_abs(&s.top))
            .fold(0.0_f64, f64::max);
        let max_mid_abs = solve
            .stresses
            .iter()
            .map(|s| max_abs(&s.mid))
            .fold(0.0_f64, f64::max);
        assert!(
            max_top_abs > max_mid_abs,
            "expected a through-thickness bending gradient (max|top|={max_top_abs:.4e} \
             > max|mid|={max_mid_abs:.4e}); mid should sit near the neutral plane"
        );
    }
}

