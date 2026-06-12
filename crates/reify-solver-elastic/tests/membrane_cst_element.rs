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

use reify_solver_elastic::assembly::test_support::assert_close;
use reify_solver_elastic::constitutive::IsotropicElastic;
use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, CgSolverOptions, DirichletBc, SolverMode,
    apply_dirichlet_row_elimination, assemble_global_stiffness, element_stiffness_membrane_cst,
    solve_cg,
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
