//! Flat-plate MITC3 cantilever shell driver (PRD
//! `docs/prds/v0_4/shell-extract-engine-bridge.md` task δ, §3/§5/§7).
//!
//! `solve_flat_plate_shell` synthesizes a structured triangulated mid-surface
//! mesh of an `length × width` rectangle in the XY plane (thickness `t`),
//! clamps the `x == 0` root edge, applies a `-Z` tip load as a consistent
//! traction over the free-tip element column, assembles + solves the flat-facet
//! MITC3+ shell system, and recovers per-element [`ShellElementStress`] +
//! per-element [`ShellFrame`].
//!
//! This is the neutral-types kernel driver: it returns only solver-elastic
//! types (no `ShellChannels` — that glue lives in reify-eval, which depends on
//! this crate; see PRD §11 OQ-2 and the task δ design decisions). The recipe
//! is lifted from the proven end-to-end shell solve in
//! `tests/shell_benchmarks.rs` (the flat-plate cantilever sanity test).
//!
//! # Accuracy contract (esc-3594-10 re-spec)
//!
//! The recovered stress is held to the **bare-MITC3 honest band**, NOT a tight
//! tolerance: the user-observable signal (e2e in
//! `reify-eval/tests/shell_solve_e2e.rs`) asserts the max top-channel von Mises
//! is finite, non-zero, and within ONE ORDER OF MAGNITUDE of the analytical
//! `σ = 6PL/(bh²)`. A flat-facet cantilever is the benign MITC3 case (no
//! curvature → no membrane locking), so the 1-OOM band is met deterministically;
//! this driver is intentionally NOT gated on MITC3+ tight accuracy (task 3392),
//! matching the `shell_benchmarks.rs` "smoke tests, NOT validated benchmarks"
//! convention.

use crate::assembly::{AssemblyElement, AssemblyMode, ElementStiffness, assemble_global_stiffness};
use crate::boundary::{DirichletBc, apply_dirichlet_row_elimination};
use crate::constitutive::IsotropicElastic;
use crate::shell_assembly::{ShellFrame, build_shell_frame, shell_element_stiffness_mitc3_plus};
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

/// Solve a flat-plate MITC3+ cantilever shell and recover per-element stress +
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
/// - The transverse `-Z` `tip_force` is applied as a consistent traction over
///   the last element column (a one-cell-wide patch at the free tip), NOT a
///   pure edge-line load. A pure edge load concentrates the whole force on the
///   `x == length` node ring, whose two doubly-free corner nodes each belong to
///   a single triangle and act as near-mechanisms (they "flap" and manufacture
///   a spurious local stress concentration). The patch shares the load with the
///   inboard node column, cutting the corner over-load while keeping the root
///   bending moment at `tip_force · (length − Δx/2)` — within ≈2.5% of the
///   ideal tip-point moment `tip_force · length`.
///
/// # Element choice / accuracy
///
/// Assembly uses the flat-facet MITC3+ element
/// ([`shell_element_stiffness_mitc3_plus`], task 3392): bare MITC3 transverse-
/// SHEAR-locks on this thin plate (`L/t = 50`), collapsing the bending response
/// ~100× and pushing the recovered stress below the band; the MITC3+ nodal
/// assumed-shear field relieves that locking and restores a tip deflection
/// within ~30% of Euler-Bernoulli. A flat facet has no curvature, so the
/// membrane-locking that drives the large curved-benchmark errors
/// (`shell_benchmarks.rs`) is absent. The recovered top-channel von Mises lands
/// within one order of magnitude of the analytical σ = 6PL/(bh²) (esc-3594-10
/// honest-accuracy contract; band `[3e7, 3e9]` Pa around `3e8`). This is NOT a
/// tight-accuracy (5%) contract.
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
    let is_root = |n: usize| n.is_multiple_of(nx1); // ix == 0
    for n in 0..n_nodes {
        if !is_root(n) {
            bcs.push(DirichletBc {
                dof: n * 6 + 5,
                value: 0.0,
            });
        }
    }

    // ── Load: distribute -Z tip_force as a consistent traction over the LAST
    // COLUMN of elements (a one-element-wide patch at the free tip), NOT a pure
    // edge-line load. f is 6-DOF/node, so the transverse (DOF 2) component is
    // written directly (apply_point_load is a 3-DOF/node helper).
    //
    // A pure edge-line load concentrates the whole tip force on the x==length
    // node ring, whose two doubly-free corner nodes each belong to a single
    // triangle and behave as near-mechanisms — they "flap" and manufacture a
    // spurious local bending stress that swamps the physical root stress. The
    // patch shares the load with the inboard (x==length-Δx) node column, cutting
    // each corner node's share ~3× and reusing the corner's second triangle.
    //
    // For a uniform downward pressure whose resultant is `tip_force`, the
    // consistent (constant-pressure) nodal force on a linear triangle is
    // p·area/3 per vertex; on the structured grid this is exactly
    // `tip_force / (6·ny)` per vertex regardless of the cell aspect ratio.
    // Summed over the column's 2·ny triangles it recovers `tip_force` exactly.
    // The patch centroid sits at x = length − Δx/2, so the root bending moment
    // is `tip_force · (length − Δx/2)` — within Δx/2length (≈2.5%) of the ideal
    // tip-point moment, well inside the one-OOM accuracy band.
    let mut f = vec![0.0_f64; 6 * n_nodes];
    let per_vertex = -tip_force / (6.0 * ny as f64);
    for iy in 0..ny {
        let a = node(nx - 1, iy);
        let b = node(nx, iy);
        let c = node(nx - 1, iy + 1);
        let d = node(nx, iy + 1);
        // Same two CCW triangles as this cell's connectivity split.
        for tri in [[a, b, d], [a, d, c]] {
            for &n in &tri {
                f[n * 6 + 2] += per_vertex;
            }
        }
    }

    apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

    // ── Solve ─────────────────────────────────────────────────────────────────
    let opts = CgSolverOptions {
        tolerance: 1e-6,
        max_iter: 5000,
    };
    let cg = solve_cg(&k, &f, opts, SolverMode::Deterministic);
    let u = &cg.u;

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

