//! Integration tests for the shell FEA pipeline (PRD v0.4 task #21).
//!
//! This file exercises the end-to-end shell assembly pipeline
//! (`shell_element_stiffness` → `assemble_global_stiffness` →
//! `apply_dirichlet_row_elimination` → dense-LU solve) on four canonical
//! shell-formulation benchmarks from MacNeal & Harder (1985):
//!
//! 1. **Pinched cylinder** (§3.3): 1/8 octant by symmetry, 4×4 elements.
//! 2. **Scordelis-Lo roof** (§3.4): 1/4 quadrant by symmetry.
//! 3. **Hemisphere with point loads** (§3.5): 1/4 by symmetry.
//! 4. **Twisted beam** (§3.6): full 12×2 element mesh.
//!
//! Plus a **locking-detection** test verifying that MITC3 does not collapse
//! under decreasing thickness (the signature of shear-locking in naive
//! Reissner-Mindlin elements).
//!
//! # Drilling-DOF stabilization
//!
//! MITC3 carries zero stiffness along the local drilling rotation (θ_z,
//! rotation about the element normal). For curved-shell meshes this makes
//! K_global rank-deficient. All tests use the test-local helper
//! `shell_element_stiffness_drilling_stabilized` (Hughes-Brezzi penalty,
//! ε = 1e-6) to add a small, physically inert stabilization term before
//! global assembly. The production fix is tracked as a follow-up task.
//!
//! # PRD reference
//!
//! `docs/prds/v0_4/structural-analysis-shells.md`, task #21
//! ("Validation & polish").

use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, ElementStiffness, assemble_global_stiffness,
    apply_dirichlet_row_elimination, DirichletBc,
    shell_element_stiffness, build_shell_frame, IsotropicElastic,
};

// ─── step-1: sanity check ────────────────────────────────────────────────────

/// Sanity check for the end-to-end shell pipeline.
///
/// A 2-element flat unit-square cantilever clamped at x=0 with a transverse
/// (+z) tip point load at the free edge (x=1) must develop a positive
/// transverse displacement at each free-edge node.
///
/// Mesh layout (XY plane):
/// ```text
///   2---3
///   |\ |
///   | \|
///   0---1
/// ```
/// - Nodes: 0:(0,0,0), 1:(1,0,0), 2:(0,1,0), 3:(1,1,0)
/// - Elements: [0,1,3] and [0,3,2]
/// - BCs: clamp nodes 0 and 2 (x=0 edge), all 6 DOFs fixed
/// - Load: Fz = 1 at nodes 1 and 3 (x=1 edge)
#[test]
fn flat_plate_cantilever_under_tip_load_displaces_in_load_direction() {
    let nodes: Vec<[f64; 3]> = vec![
        [0.0, 0.0, 0.0], // node 0 — clamped
        [1.0, 0.0, 0.0], // node 1 — free, loaded
        [0.0, 1.0, 0.0], // node 2 — clamped
        [1.0, 1.0, 0.0], // node 3 — free, loaded
    ];
    let connectivity: Vec<[usize; 3]> = vec![[0, 1, 3], [0, 3, 2]];
    let mat = IsotropicElastic {
        youngs_modulus: 1e6,
        poisson_ratio: 0.3,
    };
    let thickness = 0.1;
    let n_nodes = nodes.len();

    // Build per-element stiffness matrices with drilling stabilization.
    let stiffness: Vec<ElementStiffness> = connectivity
        .iter()
        .map(|conn| {
            let elem_nodes = [nodes[conn[0]], nodes[conn[1]], nodes[conn[2]]];
            shell_element_stiffness_drilling_stabilized(&elem_nodes, thickness, &mat, 1e-6)
        })
        .collect();

    // Build AssemblyElement slice — &[usize; 3] coerces to &[usize] at the
    // struct literal (coercion site per the Rust Reference).
    let elements: Vec<AssemblyElement<'_>> = connectivity
        .iter()
        .zip(stiffness.iter())
        .enumerate()
        .map(|(i, (conn, k_e))| AssemblyElement {
            id: i,
            connectivity: conn,
            k_e,
        })
        .collect();

    // BCs: clamp nodes 0 and 2 (x=0 edge), all 6 DOFs = 0.
    let mut bcs: Vec<DirichletBc> = Vec::new();
    for node in [0_usize, 2_usize] {
        for dof in 0..6_usize {
            bcs.push(DirichletBc { dof: node * 6 + dof, value: 0.0 });
        }
    }

    // Load: transverse (+z) unit load at free-edge nodes 1 and 3.
    let point_loads: Vec<(usize, f64)> = vec![
        (1 * 6 + 2, 1.0), // node 1, z-DOF
        (3 * 6 + 2, 1.0), // node 3, z-DOF
    ];

    // Solve: returns 6 * n_nodes displacement vector.
    let u = solve_shell_system(&elements, n_nodes, &bcs, &point_loads);

    // Sign-only assertion: free-edge nodes must deflect in the +z direction.
    assert!(
        u[1 * 6 + 2] > 0.0,
        "node 1 z-displacement must be positive under +z load (got {})",
        u[1 * 6 + 2]
    );
    assert!(
        u[3 * 6 + 2] > 0.0,
        "node 3 z-displacement must be positive under +z load (got {})",
        u[3 * 6 + 2]
    );
}
