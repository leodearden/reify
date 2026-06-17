//! Acceptance signals (PRD `docs/prds/v0_6/tensegrity-membrane.md` §5 + §8.2 G6,
//! task ζ) for the dedicated CST membrane element.
//!
//! Two integration goldens, mirroring `tests/bar_axial_deflection.rs`:
//!
//! 1. **In-plane multi-element patch test** — a CST exactly reproduces a
//!    constant-strain (linear) in-plane displacement field, so the interior-node
//!    displacement matches the exact field to machine precision (an EXACTNESS
//!    identity, asserted at 1e-9).
//! 2. **Pretensioned-membrane-under-pressure center deflection** (S11/S12) — a
//!    MESH-CONVERGENCE bound of the `N∇²w=−p` Fourier closed form, NOT an exact
//!    value (G6 honesty: a CST membrane under transverse pressure is an
//!    O(h²)-convergent approximation, not nodally exact).
//!
//! # In-plane patch test
//!
//! A flat unit-square patch `[0,1]²` triangulated into 4 CSTs fanning from a
//! non-centered interior vertex. The exact linear field `u_x=αx+βy`,
//! `u_y=γx+δy` is imposed on every boundary-node in-plane DOF; the out-of-plane
//! (z) DOF of every node is pinned (membrane `K_e` alone is transversely
//! singular). Assembling the membrane `K_e` and solving leaves the interior node
//! free in-plane; a CST reproduces constant strain exactly, so the solved
//! interior displacement equals the exact field to 1e-9.

use std::f64::consts::PI;

use reify_solver_elastic::assembly::test_support::assert_close;
use reify_solver_elastic::constitutive::IsotropicElastic;
use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, CgSolverOptions, DirichletBc, MembranePrestress, SolverMode,
    apply_dirichlet_row_elimination, assemble_global_stiffness, element_stiffness_membrane_cst,
    membrane_tangent_stiffness, solve_cg,
};

/// (1) In-plane multi-element patch test: a CST exactly reproduces a constant-
/// strain linear field, so the interior node matches the exact field to 1e-9.
#[test]
fn in_plane_patch_reproduces_linear_field() {
    run_in_plane_patch_test();
}

/// Index of the interior (free in-plane) node in [`patch_mesh`].
const INTERIOR_NODE: usize = 4;

/// Unit-square patch `[0,1]²` triangulated into 4 CSTs fanning from a
/// non-centered interior vertex (node 4). Boundary nodes 0–3, interior node 4.
/// Every node lies in the `z = 0` plane (a flat in-plane patch). Returns
/// `(node coordinates, triangle connectivity)`.
fn patch_mesh() -> (Vec<[f64; 3]>, Vec<[usize; 3]>) {
    let nodes = vec![
        [0.0, 0.0, 0.0],   // 0 — corner (boundary)
        [1.0, 0.0, 0.0],   // 1 — corner (boundary)
        [1.0, 1.0, 0.0],   // 2 — corner (boundary)
        [0.0, 1.0, 0.0],   // 3 — corner (boundary)
        [0.37, 0.53, 0.0], // 4 — interior, deliberately off-center
    ];
    // Fan the four triangles around the interior node (CCW).
    let tris = vec![[0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4]];
    (nodes, tris)
}

/// Exact constant-strain (linear) in-plane displacement field
/// `u_x = αx + βy`, `u_y = γx + δy`. Returns `[u_x, u_y]`. A CST's
/// approximation space contains this field exactly, which is what the patch
/// test verifies.
fn linear_field(coord: [f64; 3]) -> [f64; 2] {
    const A: f64 = 0.012; // α
    const B: f64 = 0.005; // β
    const G: f64 = -0.008; // γ
    const D: f64 = 0.009; // δ
    let (x, y) = (coord[0], coord[1]);
    [A * x + B * y, G * x + D * y]
}

/// Dirichlet BCs for the in-plane patch test:
/// - the exact linear field on every **boundary**-node in-plane DOF (`u_x`, `u_y`);
/// - the out-of-plane (z) DOF of **every** node pinned to 0 — membrane `K_e`
///   is transversely singular (zero transverse stiffness), so the z DOFs must
///   be constrained for the system to be non-singular and CG to converge.
///
/// The interior node keeps its two in-plane DOFs free; the patch test asserts
/// the solver recovers the exact field there.
fn in_plane_patch_bcs(nodes: &[[f64; 3]]) -> Vec<DirichletBc> {
    let mut bcs = Vec::new();
    for (n, coord) in nodes.iter().enumerate() {
        // Pin the transverse (z) DOF on every node.
        bcs.push(DirichletBc { dof: 3 * n + 2, value: 0.0 });
        if n != INTERIOR_NODE {
            // Boundary node: prescribe the exact linear field in-plane.
            let uf = linear_field(*coord);
            bcs.push(DirichletBc { dof: 3 * n, value: uf[0] });
            bcs.push(DirichletBc { dof: 3 * n + 1, value: uf[1] });
        }
    }
    bcs
}

/// Assemble the membrane `K_e` patch, impose the linear-field BCs, solve, and
/// assert the interior node reproduces the exact constant-strain field to 1e-9.
fn run_in_plane_patch_test() {
    let (nodes, tris) = patch_mesh();
    let n_nodes = nodes.len();
    let material = IsotropicElastic {
        youngs_modulus: 70.0e9,
        poisson_ratio: 0.3,
    };
    let thickness = 0.01;

    // Per-triangle membrane K_e (elastic only — this is the in-plane patch test,
    // no prestress / geometric stiffness).
    let kes: Vec<_> = tris
        .iter()
        .map(|t| {
            element_stiffness_membrane_cst(
                &[nodes[t[0]], nodes[t[1]], nodes[t[2]]],
                thickness,
                &material,
            )
        })
        .collect();
    let elems: Vec<AssemblyElement> = tris
        .iter()
        .zip(kes.iter())
        .enumerate()
        .map(|(id, (t, ke))| AssemblyElement {
            id,
            connectivity: t.as_slice(),
            k_e: ke,
        })
        .collect();

    // Global membrane stiffness (dofs_per_node derived as 9/3 = 3) + zero load.
    let mut k_global = assemble_global_stiffness(n_nodes, &elems, AssemblyMode::Deterministic);
    let mut f = vec![0.0_f64; 3 * n_nodes];

    let bcs = in_plane_patch_bcs(&nodes);
    apply_dirichlet_row_elimination(&mut k_global, &mut f, &bcs);

    let opts = CgSolverOptions {
        tolerance: 1.0e-12,
        max_iter: 5000,
    };
    let result = solve_cg(&k_global, &f, opts, SolverMode::Deterministic);
    assert!(
        result.converged,
        "patch CG did not converge in {} iters",
        result.iterations,
    );
    let u = result.u();

    // A CST reproduces the constant-strain field exactly, so the free interior
    // node must equal the exact linear field to machine precision (1e-9).
    let exact = linear_field(nodes[INTERIOR_NODE]);
    assert_close(
        u[3 * INTERIOR_NODE],
        exact[0],
        1e-9,
        "interior u_x = exact linear field",
    );
    assert_close(
        u[3 * INTERIOR_NODE + 1],
        exact[1],
        1e-9,
        "interior u_y = exact linear field",
    );
}

// ───────────────────── Pretensioned-membrane pressure golden (S11/S12) ──────
//
// A square membrane `[0, SIDE]²` with all four edges fixed, under uniform
// isotropic pretension `N = σ·t` and a uniform transverse pressure `p`, obeys
// the membrane (Poisson) equation `N·∇²w = −p` with `w = 0` on the boundary.
// Its center deflection has the convergent Fourier closed form
//
//   w_center = (16 p a²)/(π⁴ N) · Σ_{m,n odd} (−1)^((m+n)/2−1) / (m·n·(m²+n²)).
//
// G6 honesty: unlike the in-plane patch test (an EXACTNESS identity — a CST
// reproduces constant strain to machine precision), a flat CST membrane under
// transverse pressure is an O(h²)-convergent APPROXIMATION of this closed form,
// NOT nodally exact. The test asserts a MESH-CONVERGENCE bound:
//   (a) the center-deflection relative error DECREASES monotonically under
//       refinement (the O(h²) signature), and
//   (b) the finest mesh's relative error is below a generous bound derived from
//       the CST O(h²) discretization floor — comfortably above it, never a
//       post-hoc-tuned constant.
//
// A flat membrane's transverse stiffness comes ENTIRELY from K_g (K_e is
// transversely singular), so the assembled operator is the tangent
// K_t = K_e + K_g. The in-plane (K_e) and transverse (K_g) DOF blocks decouple
// for a flat xy-plane membrane, so the pure-z pressure load excites only the
// transverse Poisson / cotangent-Laplacian block.

/// Square side length `a` [m].
const SIDE: f64 = 2.0;
/// Membrane pretension stress σ [Pa]; the isotropic resultant is `N = σ·t`.
const PRETENSION_STRESS: f64 = 4.0e6;
/// Uniform transverse pressure `p` [Pa].
const PRESSURE: f64 = 1.0e3;
/// Membrane thickness `t` [m]; with `PRETENSION_STRESS` gives `N = σ·t`.
const THICKNESS: f64 = 1.0e-3;

/// (2) Pretensioned-membrane-under-pressure center deflection: a MESH-CONVERGENCE
/// bound (NOT an exact value) of the `N∇²w=−p` Fourier closed form (G6).
#[test]
fn pressure_center_deflection_converges_to_fourier_reference() {
    let pretension = PRETENSION_STRESS * THICKNESS; // N = σ·t
    let w_ref = fourier_center_deflection(SIDE, pretension, PRESSURE);

    // ≥2 resolutions; even divisions ⇒ a node lands exactly at the center.
    let resolutions = [4usize, 8, 16];
    let rel_errors: Vec<f64> = resolutions
        .iter()
        .map(|&divs| {
            let w_fe = solve_pressure_center_deflection(divs);
            (w_fe - w_ref).abs() / w_ref.abs()
        })
        .collect();

    // (a) O(h²) signature: relative error strictly decreases under refinement.
    for k in 1..rel_errors.len() {
        assert!(
            rel_errors[k] < rel_errors[k - 1],
            "center-deflection error must decrease under refinement, got {rel_errors:?} \
             for resolutions {resolutions:?}",
        );
    }

    // (b) Finest-mesh error below a generous O(h²)-derived bound. The CST Poisson
    // center-value error follows err ≈ C·(h/a)² with C of order 1; at divs=16
    // (h/a = 1/16 ⇒ (h/a)² ≈ 3.9e-3) that is a few tenths of a percent. The 3%
    // bound sits ~10× above this discretization floor — a comfortably generous
    // ceiling derived from the O(h²) estimate, NOT a value tuned to barely pass
    // (the error reduces ≈4× per refinement here, the second-order signature).
    const FINEST_REL_BOUND: f64 = 0.03;
    let finest = *rel_errors.last().unwrap();
    assert!(
        finest < FINEST_REL_BOUND,
        "finest-mesh relative error {finest} exceeds generous O(h²) bound {FINEST_REL_BOUND} \
         (rel errors {rel_errors:?})",
    );
}

/// In-plane elastic material for `K_e`. The flat membrane decouples in-plane and
/// transverse DOFs, so this does not affect the transverse center deflection — it
/// is included only because `K_t = K_e + K_g` assembles the in-plane block too.
fn membrane_material() -> IsotropicElastic {
    IsotropicElastic { youngs_modulus: 70.0e9, poisson_ratio: 0.3 }
}

/// Area of a triangle from its three vertices (`½·|(p1−p0)×(p2−p0)|`).
fn triangle_area(p: &[[f64; 3]; 3]) -> f64 {
    let d01 = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
    let d02 = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
    let cx = d01[1] * d02[2] - d01[2] * d02[1];
    let cy = d01[2] * d02[0] - d01[0] * d02[2];
    let cz = d01[0] * d02[1] - d01[1] * d02[0];
    0.5 * (cx * cx + cy * cy + cz * cz).sqrt()
}

/// (1) Structured triangulation of `[0, SIDE]²` into `divs × divs` square cells,
/// each split into two CCW (`+z` normal) CSTs. `divs` must be even (so a node
/// lands at the center `(SIDE/2, SIDE/2)`). Returns `(nodes, tris, center_node)`.
fn square_membrane_mesh(divs: usize) -> (Vec<[f64; 3]>, Vec<[usize; 3]>, usize) {
    assert!(divs >= 2 && divs.is_multiple_of(2), "divs must be even and ≥2 for a center node");
    let n = divs + 1; // nodes per side
    let h = SIDE / divs as f64;
    let mut nodes = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            nodes.push([i as f64 * h, j as f64 * h, 0.0]);
        }
    }
    let node = |i: usize, j: usize| j * n + i;
    let mut tris = Vec::with_capacity(2 * divs * divs);
    for j in 0..divs {
        for i in 0..divs {
            let (v00, v10, v11, v01) =
                (node(i, j), node(i + 1, j), node(i + 1, j + 1), node(i, j + 1));
            // Split each cell along the v00–v11 diagonal; both CCW ⇒ +z normal.
            tris.push([v00, v10, v11]);
            tris.push([v00, v11, v01]);
        }
    }
    (nodes, tris, node(divs / 2, divs / 2))
}

/// Dirichlet BCs pinning ALL DOFs of every boundary node to zero (fixed edges).
/// Interior in-plane DOFs are unforced (⇒ 0) and the interior transverse DOFs
/// carry the membrane deflection, so no interior pin is needed — the in-plane
/// (`K_e`) and transverse (`K_g`) interior blocks are each SPD under fixed edges.
fn fixed_edge_bcs(divs: usize) -> Vec<DirichletBc> {
    let n = divs + 1;
    let mut bcs = Vec::new();
    for j in 0..n {
        for i in 0..n {
            if i == 0 || i == divs || j == 0 || j == divs {
                let base = 3 * (j * n + i);
                for d in 0..3 {
                    bcs.push(DirichletBc { dof: base + d, value: 0.0 });
                }
            }
        }
    }
    bcs
}

/// (2) Consistent transverse nodal loads for uniform pressure `p`: each triangle
/// contributes `p·A_tri/3` to each of its three nodes' transverse (global-z) DOF
/// (`∫ p·N_i dA = p·A/3` for a linear CST). The flat membrane lies in the
/// xy-plane, so the transverse direction is global z.
fn pressure_nodal_loads(nodes: &[[f64; 3]], tris: &[[usize; 3]], pressure: f64) -> Vec<f64> {
    let mut f = vec![0.0_f64; 3 * nodes.len()];
    for t in tris {
        let area = triangle_area(&[nodes[t[0]], nodes[t[1]], nodes[t[2]]]);
        let nodal = pressure * area / 3.0;
        for &nd in t {
            f[3 * nd + 2] += nodal;
        }
    }
    f
}

/// (3) Center deflection `w_center` of `N∇²w=−p` on the fixed-edge square `[0,a]²`,
/// by summing the convergent Fourier series. The `1/(m·n·(m²+n²))` terms decay
/// fast, so truncating the odd indices at 999 is exact to far below the FE
/// discretization error.
fn fourier_center_deflection(a: f64, pretension: f64, pressure: f64) -> f64 {
    let mut sum = 0.0_f64;
    let max_odd = 999usize;
    let mut m = 1usize;
    while m <= max_odd {
        let mut nn = 1usize;
        while nn <= max_odd {
            // sign = (−1)^((m+n)/2 − 1); m,n odd ⇒ (m+n)/2 is an integer.
            let k = (m + nn) / 2 - 1;
            let sign = if k.is_multiple_of(2) { 1.0 } else { -1.0 };
            sum += sign / ((m as f64) * (nn as f64) * ((m * m + nn * nn) as f64));
            nn += 2;
        }
        m += 2;
    }
    16.0 * pressure * a * a / (PI.powi(4) * pretension) * sum
}

/// (4) Assemble `K_t = K_e + K_g` over the `divs×divs` membrane mesh, apply the
/// uniform-pressure nodal loads and fixed-edge BCs, solve, and return the center
/// node's transverse (global-z) deflection.
fn solve_pressure_center_deflection(divs: usize) -> f64 {
    let (nodes, tris, center) = square_membrane_mesh(divs);
    let n_nodes = nodes.len();
    let material = membrane_material();
    let prestress = MembranePrestress::isotropic(PRETENSION_STRESS * THICKNESS);

    // Per-triangle tangent K_t = K_e + K_g (the transverse stiffness is entirely
    // K_g; K_e alone is transversely singular).
    let kts: Vec<_> = tris
        .iter()
        .map(|t| {
            membrane_tangent_stiffness(
                &[nodes[t[0]], nodes[t[1]], nodes[t[2]]],
                THICKNESS,
                &material,
                &prestress,
            )
        })
        .collect();
    let elems: Vec<AssemblyElement> = tris
        .iter()
        .zip(kts.iter())
        .enumerate()
        .map(|(id, (t, kt))| AssemblyElement { id, connectivity: t.as_slice(), k_e: kt })
        .collect();

    let mut k_global = assemble_global_stiffness(n_nodes, &elems, AssemblyMode::Deterministic);
    let mut f = pressure_nodal_loads(&nodes, &tris, PRESSURE);

    let bcs = fixed_edge_bcs(divs);
    apply_dirichlet_row_elimination(&mut k_global, &mut f, &bcs);

    let opts = CgSolverOptions { tolerance: 1.0e-12, max_iter: 50_000 };
    let result = solve_cg(&k_global, &f, opts, SolverMode::Deterministic);
    assert!(
        result.converged,
        "pressure CG did not converge in {} iters at divs={divs}",
        result.iterations,
    );
    result.u()[3 * center + 2]
}
