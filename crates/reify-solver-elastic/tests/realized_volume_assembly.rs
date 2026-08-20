//! C-3 (task 4986) integration test: `assemble_volume_mesh_stiffness`
//! dispatches a realized Hex/Wedge `VolumeMesh` to the matching
//! element-stiffness path, and the resulting global system solves to CG
//! convergence.
//!
//! PRD `docs/prds/v0_3/hex-wedge-meshing.md` Addendum (2026-07-04), contract
//! C-3. Assertions are scoped to dispatch + convergence ONLY: no analytical
//! displacement magnitude/tolerance is asserted here (that belongs to the FEA
//! validation suite, PRD task 12). This substrate test proves a *realized*
//! hex/wedge `VolumeMesh` (not a `SweptMesh3d`) can be assembled and solved,
//! generalizing the tet-only `element_stiffness` -> `assemble_global_stiffness`
//! -> `apply_dirichlet_row_elimination` -> `solve_cg` pipeline exercised
//! elsewhere in this suite (e.g. `aposteriori_validation.rs`).

use faer::sparse::SparseRowMat;
use reify_ir::{VolumeConnectivity, VolumeMesh};
use reify_solver_elastic::{
    AssemblyMode, CgSolverOptions, DirichletBc, IsotropicElastic, SolverMode,
    apply_dirichlet_row_elimination, apply_point_load, assemble_volume_mesh_stiffness, solve_cg,
};

/// Steel-like dimensionless material: E = 1, ν = 0.3 (matches the crate-wide
/// `dimensionless_steel_like` fixture convention in `assembly::test_support`).
fn material() -> IsotropicElastic {
    IsotropicElastic {
        youngs_modulus: 1.0,
        poisson_ratio: 0.3,
    }
}

/// Read entry `(i, j)` of a `SparseRowMat<usize, f64>`, defaulting to `0.0`
/// for an unstored entry (mirrors `assembly::global::tests::read`).
fn read(k: &SparseRowMat<usize, f64>, i: usize, j: usize) -> f64 {
    k.get(i, j).copied().unwrap_or(0.0)
}

/// Assert `k` is symmetric within FP tolerance (mirrors
/// `assembly::global::tests::global_k_is_symmetric_within_fp_tolerance`).
fn assert_symmetric(k: &SparseRowMat<usize, f64>, dim: usize, label: &str) {
    for i in 0..dim {
        for j in i..dim {
            let kij = read(k, i, j);
            let kji = read(k, j, i);
            let tol = 1e-9 * kij.abs().max(kji.abs()).max(1.0);
            assert!(
                (kij - kji).abs() <= tol,
                "{label}: K[{i}][{j}] = {kij}, K[{j}][{i}] = {kji}; |Δ| > tol = {tol}"
            );
        }
    }
}

/// Unit-cube P1 hex `VolumeMesh` fixture (canonical Hughes/Gmsh hex8 node
/// order — bottom face (z=0) CCW, then top face (z=1) in the same cyclic
/// order; matches `assembly::hex::element_stiffness_hex_p1`'s documented
/// node ordering).
fn unit_hex_volume_mesh() -> VolumeMesh {
    VolumeMesh {
        vertices: vec![
            0.0, 0.0, 0.0, // v0
            1.0, 0.0, 0.0, // v1
            1.0, 1.0, 0.0, // v2
            0.0, 1.0, 0.0, // v3
            0.0, 0.0, 1.0, // v4
            1.0, 0.0, 1.0, // v5
            1.0, 1.0, 1.0, // v6
            0.0, 1.0, 1.0, // v7
        ],
        connectivity: VolumeConnectivity::Hex {
            indices: vec![0, 1, 2, 3, 4, 5, 6, 7],
        },
        normals: None,
        boundary: None,
    }
}

/// Unit triangular-prism P1 wedge `VolumeMesh` fixture (canonical Gmsh PRI6
/// node order — bottom triangle (z=0), then top triangle (z=1) in the same
/// cyclic order; matches `assembly::wedge::element_stiffness_wedge_p1`'s
/// documented node ordering).
fn unit_wedge_volume_mesh() -> VolumeMesh {
    VolumeMesh {
        vertices: vec![
            0.0, 0.0, 0.0, // v0
            1.0, 0.0, 0.0, // v1
            0.0, 1.0, 0.0, // v2
            0.0, 0.0, 1.0, // v3
            1.0, 0.0, 1.0, // v4
            0.0, 1.0, 1.0, // v5
        ],
        connectivity: VolumeConnectivity::Wedge {
            indices: vec![0, 1, 2, 3, 4, 5],
        },
        normals: None,
        boundary: None,
    }
}

/// Fully clamp all 3 DOFs at each of `nodes` — removes the 6 rigid-body
/// modes for a single-element mesh whenever `nodes` are 3 non-collinear
/// points (over-constrained by 3 DOFs vs. the minimal "3-2-1" scheme, but
/// still a consistent essential-BC set: no analytical tolerance is asserted
/// downstream, only convergence, so the extra constraints are harmless).
fn fully_clamp(nodes: &[usize]) -> Vec<DirichletBc> {
    nodes
        .iter()
        .flat_map(|&n| {
            (0..3).map(move |axis| DirichletBc {
                dof: 3 * n + axis,
                value: 0.0,
            })
        })
        .collect()
}

#[test]
fn hex_volume_mesh_assembles_and_solves_via_assemble_volume_mesh_stiffness() {
    let vm = unit_hex_volume_mesh();
    let mat = material();
    let n_nodes = vm.vertices.len() / 3;
    assert_eq!(n_nodes, 8);

    // (a) dispatch: assembly succeeds and produces a correctly-sized,
    // symmetric, nonzero global K (per-element DOF = 24 for a single hex
    // element covering all 8 nodes, so 3N == 24 pins that stride too).
    let (mut k, returned_n_nodes) =
        assemble_volume_mesh_stiffness(&vm, &mat, AssemblyMode::Deterministic)
            .expect("hex VolumeMesh must assemble");
    assert_eq!(returned_n_nodes, n_nodes);

    let dim = 3 * n_nodes;
    assert_eq!(dim, 24, "single hex element ⇒ 24 global DOFs");
    assert_eq!(k.nrows(), dim, "hex global K must be 3N x 3N");
    assert_eq!(k.ncols(), dim);
    assert_symmetric(&k, dim, "hex");
    assert!(
        (0..dim).any(|i| read(&k, i, i) > 0.0),
        "hex global K must have at least one positive diagonal entry"
    );

    // (b) solve completes: clamp 3 non-collinear bottom-face nodes (v0, v1,
    // v3) to remove the 6 rigid-body modes, apply a point load at the
    // unclamped opposite top corner (v6), and solve to CG convergence.
    let bcs = fully_clamp(&[0, 1, 3]);
    let mut f = vec![0.0_f64; dim];
    apply_point_load(&mut f, 6, [0.01, 0.02, 0.05]);
    apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

    let result = solve_cg(
        &k,
        &f,
        CgSolverOptions::default(),
        SolverMode::Deterministic,
    );
    assert!(result.converged, "hex single-element solve must converge");
    let u = result.u();
    assert_eq!(u.len(), dim);
    for (i, &ui) in u.iter().enumerate() {
        assert!(ui.is_finite(), "u[{i}] = {ui} must be finite");
    }
    let node6 = [u[18], u[19], u[20]];
    assert!(
        node6.iter().any(|c| c.abs() > 0.0),
        "node 6 displacement must be nonzero under a nonzero point load: {node6:?}"
    );
}

#[test]
fn wedge_volume_mesh_assembles_and_solves_via_assemble_volume_mesh_stiffness() {
    let vm = unit_wedge_volume_mesh();
    let mat = material();
    let n_nodes = vm.vertices.len() / 3;
    assert_eq!(n_nodes, 6);

    // (a) dispatch: assembly succeeds and produces a correctly-sized,
    // symmetric, nonzero global K (per-element DOF = 18 for a single wedge
    // element covering all 6 nodes, so 3N == 18 pins that stride too).
    let (mut k, returned_n_nodes) =
        assemble_volume_mesh_stiffness(&vm, &mat, AssemblyMode::Deterministic)
            .expect("wedge VolumeMesh must assemble");
    assert_eq!(returned_n_nodes, n_nodes);

    let dim = 3 * n_nodes;
    assert_eq!(dim, 18, "single wedge element ⇒ 18 global DOFs");
    assert_eq!(k.nrows(), dim, "wedge global K must be 3N x 3N");
    assert_eq!(k.ncols(), dim);
    assert_symmetric(&k, dim, "wedge");
    assert!(
        (0..dim).any(|i| read(&k, i, i) > 0.0),
        "wedge global K must have at least one positive diagonal entry"
    );

    // (b) solve completes: clamp the entire bottom triangle (v0, v1, v2 —
    // non-collinear) to remove the 6 rigid-body modes, load the unclamped
    // top-far corner (v5), and solve to CG convergence.
    let bcs = fully_clamp(&[0, 1, 2]);
    let mut f = vec![0.0_f64; dim];
    apply_point_load(&mut f, 5, [0.01, 0.02, 0.05]);
    apply_dirichlet_row_elimination(&mut k, &mut f, &bcs);

    let result = solve_cg(
        &k,
        &f,
        CgSolverOptions::default(),
        SolverMode::Deterministic,
    );
    assert!(result.converged, "wedge single-element solve must converge");
    let u = result.u();
    assert_eq!(u.len(), dim);
    for (i, &ui) in u.iter().enumerate() {
        assert!(ui.is_finite(), "u[{i}] = {ui} must be finite");
    }
    let node5 = [u[15], u[16], u[17]];
    assert!(
        node5.iter().any(|c| c.abs() > 0.0),
        "node 5 displacement must be nonzero under a nonzero point load: {node5:?}"
    );
}
