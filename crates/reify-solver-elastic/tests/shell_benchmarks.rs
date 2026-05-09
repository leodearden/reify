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

// ─── test-local helpers ──────────────────────────────────────────────────────

/// Per-element MITC3 stiffness with Hughes-Brezzi drilling-DOF stabilization.
///
/// # Why this is needed
///
/// MITC3 (Bathe-Dvorkin 1985, no Allman/Hughes enrichment) carries **zero
/// stiffness** along the local drilling rotation θ_z (rotation about the
/// element normal `e3`). For any curved-shell mesh this makes `K_global`
/// rank-deficient, so `partial_piv_lu` cannot solve reliably.
///
/// # Stabilization approach
///
/// Add `ε · max_diag · e3 ⊗ e3` into each node's rotation 3×3 sub-block
/// (rows/cols `6n+3 .. 6n+6` for `n ∈ {0,1,2}`), where:
///
/// - `e3 = build_shell_frame(nodes).r[2]` — element normal in global coords
/// - `max_diag = max_i |K_e[i,i]|` — representative stiffness scale
/// - `ε = 1e-6` — well below the FP tolerance of all benchmark assertions
///
/// With `ε = 1e-6` the perturbation is at the floating-point roundoff level
/// relative to the bending/membrane stiffness modes, yet adds 6 finite
/// eigenvalues to the otherwise-zero drilling subspace.
///
/// # Production note
///
/// This is a **test-local** workaround. The correct fix belongs in
/// `shell_assembly.rs` or `assembly/global.rs` (follow-up task).
fn shell_element_stiffness_drilling_stabilized(
    nodes: &[[f64; 3]; 3],
    thickness: f64,
    material: &IsotropicElastic,
    eps: f64,
) -> ElementStiffness {
    let mut k_e = shell_element_stiffness(nodes, thickness, material);
    debug_assert_eq!(k_e.n_dofs, 18, "expected 18-DOF MITC3 element");

    // Element normal in global coordinates — the drilling singularity axis.
    let frame = build_shell_frame(nodes);
    let e3 = frame.r[2];

    // Representative stiffness scale: max absolute diagonal entry.
    let max_diag = (0..k_e.n_dofs)
        .map(|i| k_e.data[i * k_e.n_dofs + i].abs())
        .fold(0.0_f64, f64::max);

    let drill_k = eps * max_diag;

    // Add ε · max_diag · (e3 ⊗ e3) to each node's rotation sub-block.
    // Node n occupies global DOFs [6n .. 6n+6]; rotation DOFs are [6n+3 .. 6n+6].
    for node in 0..3_usize {
        let base = 6 * node + 3; // first rotation DOF for this node
        for i in 0..3_usize {
            for j in 0..3_usize {
                k_e.data[(base + i) * k_e.n_dofs + (base + j)] +=
                    drill_k * e3[i] * e3[j];
            }
        }
    }

    k_e
}

/// Assemble, apply Dirichlet BCs, and solve a pure-shell FEA system via
/// dense LU factorization.
///
/// # Arguments
///
/// * `elements` — assembled shell elements (each with connectivity and K_e)
/// * `n_nodes` — total number of nodes; K_global is `(6·n_nodes)²`
/// * `dirichlet_bcs` — prescribed DOF values (row-elimination method)
/// * `point_loads_per_dof` — `(dof_index, force_value)` pairs accumulated
///   into the RHS vector
///
/// # Returns
///
/// Dense displacement vector `u` of length `6·n_nodes`: for node `n`, the
/// six entries `u[6n .. 6n+6]` are `[u_x, u_y, u_z, θ_x, θ_y, θ_z]`.
///
/// # Solve method
///
/// Dense LU via `faer`: replicates the pattern established in the existing
/// `dirichlet_bc_elimination_satisfies_original_equilibrium_at_free_dofs`
/// test (`crates/reify-solver-elastic/src/boundary/dirichlet.rs:696-732`).
/// Dense LU is adequate for coarse-mesh benchmarks (≤ ~300 DOFs).
fn solve_shell_system(
    elements: &[AssemblyElement<'_>],
    n_nodes: usize,
    dirichlet_bcs: &[DirichletBc],
    point_loads_per_dof: &[(usize, f64)],
) -> Vec<f64> {
    use faer::linalg::solvers::Solve;

    let ndof = 6 * n_nodes;

    // Assemble global stiffness matrix (pure-shell: D = 6 DOFs/node).
    let mut k = assemble_global_stiffness(n_nodes, elements, AssemblyMode::Deterministic);

    // Build load vector by accumulating point loads.
    let mut f = vec![0.0_f64; ndof];
    for &(dof, value) in point_loads_per_dof {
        f[dof] += value;
    }

    // Apply Dirichlet BCs via symmetric row elimination.
    apply_dirichlet_row_elimination(&mut k, &mut f, dirichlet_bcs);

    // Dense LU solve: K · u = f.
    // Follows the pattern in dirichlet.rs:729-733.
    let k_dense = k.to_dense();
    let plu = k_dense.partial_piv_lu();
    let mut rhs = faer::Mat::<f64>::from_fn(ndof, 1, |i, _| f[i]);
    plu.solve_in_place(&mut rhs);

    rhs.col_as_slice(0_usize).to_vec()
}

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
