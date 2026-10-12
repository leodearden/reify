//! Shared P1 structured-box FEA harness for the `reify-solver-elastic` test
//! binaries `analytical_validation`, `aposteriori_validation` and
//! `dwr_boundary_tests`: the Kuhn-split box mesh generator, the cantilever
//! boundary-condition / load builders, and the P1 assemble-and-CG-solve
//! pipeline.
//!
//! Not a test binary: it is pulled into each consumer with
//! `#[path = "common/p1_box.rs"] mod p1_box;` (and is deliberately NOT
//! declared from `common/mod.rs`, so the tensegrity binaries that import
//! `common` do not compile it). Each consumer uses a subset of the helpers,
//! hence the `dead_code` allowance.

#![allow(dead_code)]

use reify_ir::{ElementOrderTag, VolumeConnectivity, VolumeMesh};
use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, CgResult, CgSolverOptions, DirichletBc, ElementOrder,
    ElementStiffness, IsotropicElastic, SolverMode, apply_dirichlet_row_elimination,
    assemble_global_stiffness, solve_cg,
};

/// Split a hex cell into 6 tetrahedra via the Kuhn triangulation.
///
/// All 6 tets share the main diagonal from `c[0]` to `c[6]`. This follows
/// the 6 permutations of coordinate increments (Kuhn 1960); each tet's
/// winding matches the corner ordering below (positive volume for an
/// axis-aligned hex with all positive edge lengths).
///
/// # Corner ordering
///
/// ```text
/// c[0] = (ix,   iy,   iz)    c[4] = (ix,   iy,   iz+1)
/// c[1] = (ix+1, iy,   iz)    c[5] = (ix+1, iy,   iz+1)
/// c[2] = (ix+1, iy+1, iz)    c[6] = (ix+1, iy+1, iz+1)
/// c[3] = (ix,   iy+1, iz)    c[7] = (ix,   iy+1, iz+1)
/// ```
pub fn kuhn_split_hex_to_six_tets(c: [usize; 8]) -> [[usize; 4]; 6] {
    // The 6 permutations of (Δx, Δy, Δz) define the 6 tets.
    // All start at c[0] = (000) and end at c[6] = (111).
    // Ordering: path 000 → σ₁ → σ₁σ₂ → 111.
    [
        [c[0], c[1], c[2], c[6]], // σ=(x,y,z): 000→100→110→111
        [c[0], c[1], c[5], c[6]], // σ=(x,z,y): 000→100→101→111
        [c[0], c[3], c[2], c[6]], // σ=(y,x,z): 000→010→110→111
        [c[0], c[3], c[7], c[6]], // σ=(y,z,x): 000→010→011→111
        [c[0], c[4], c[5], c[6]], // σ=(z,x,y): 000→001→101→111
        [c[0], c[4], c[7], c[6]], // σ=(z,y,x): 000→001→011→111
    ]
}

/// Build a structured P1 tet mesh for a rectangular box
/// `[0,Lx] × [0,Ly] × [0,Lz]` with `nx × ny × nz` hex cells.
///
/// Node indexing: `node(ix, iy, iz) = iz*(ny+1)*(nx+1) + iy*(nx+1) + ix`.
/// Each hex is Kuhn-split into 6 tets (see [`kuhn_split_hex_to_six_tets`]).
pub fn box_p1_mesh(
    lx: f64, ly: f64, lz: f64,
    nx: usize, ny: usize, nz: usize,
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

    let node_idx = |ix: usize, iy: usize, iz: usize| -> usize {
        iz * nny * nnx + iy * nnx + ix
    };

    let mut connectivity = Vec::with_capacity(6 * nx * ny * nz);
    for iz in 0..nz {
        for iy in 0..ny {
            for ix in 0..nx {
                let c = [
                    node_idx(ix,   iy,   iz),
                    node_idx(ix+1, iy,   iz),
                    node_idx(ix+1, iy+1, iz),
                    node_idx(ix,   iy+1, iz),
                    node_idx(ix,   iy,   iz+1),
                    node_idx(ix+1, iy,   iz+1),
                    node_idx(ix+1, iy+1, iz+1),
                    node_idx(ix,   iy+1, iz+1),
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
pub fn dirichlet_fix_face(
    nodes: &[[f64; 3]],
    axis: usize,
    value: f64,
    tol: f64,
) -> Vec<DirichletBc> {
    let mut bcs = Vec::new();
    for (node, n) in nodes.iter().enumerate() {
        if (n[axis] - value).abs() < tol {
            for dof_idx in 0..3_usize {
                bcs.push(DirichletBc { dof: node * 3 + dof_idx, value: 0.0 });
            }
        }
    }
    bcs
}

/// Prescribe the full 3-DOF displacement `field(x)` on every node lying on any
/// of the six bounding planes of `[0,lx]×[0,ly]×[0,lz]` (within `tol`).
///
/// Used by the simple-shear constant-strain patch test: prescribing the exact
/// linear field `u = (γ·y, 0, 0)` on the whole boundary makes the interior
/// solution that same field — which P1/P2 tets represent *exactly* — yielding a
/// uniform `σ_xy = G·γ` everywhere.
pub fn dirichlet_prescribe_boundary_field(
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
                bcs.push(DirichletBc { dof: node * 3 + dof_idx, value: val });
            }
        }
    }
    bcs
}

/// Indices of every node on the free-end face `x = l` (within `tol`).
pub fn end_face_nodes(nodes: &[[f64; 3]], l: f64, tol: f64) -> Vec<usize> {
    nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| (n[0] - l).abs() < tol)
        .map(|(i, _)| i)
        .collect()
}

/// Distribute a transverse shear resultant `f_mag` (in −y) equally over the
/// `end` nodes → a set of nodal point loads whose resultant is exactly `−f_mag`
/// at `x = l`. By Saint-Venant the equal split is immaterial to the tip
/// deflection (only the resultant matters far from the end).
pub fn distributed_tip_load(end: &[usize], f_mag: f64) -> Vec<(usize, f64)> {
    let per = f_mag / end.len() as f64;
    end.iter().map(|&n| (n * 3 + 1, -per)).collect()
}

/// Deduplicate Dirichlet BCs.
///
/// `apply_dirichlet_row_elimination` panics on duplicate DOF indices (debug
/// builds). This helper mirrors the dedup pattern from `shell_benchmarks.rs`.
/// In debug builds, asserts that conflicting values at the same DOF surface
/// rather than being silently dropped.
pub fn dedup_bcs(bcs: &mut Vec<DirichletBc>) {
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

/// Assemble, apply BCs, and CG-solve a P1 tetrahedral FEA system.
///
/// Uses `SolverMode::Deterministic` for bit-stable, CI-safe results.
///
/// # Returns
///
/// Displacement vector `u` of length `3 * nodes.len()`:
/// `u[3*n + α]` is displacement of node `n` in axis `α ∈ {0=x, 1=y, 2=z}`.
pub fn solve_p1_pipeline(
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
            reify_solver_elastic::element_stiffness(ElementOrder::P1, &elem_nodes, mat)
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

/// Gather the 12 element DOFs (`[u_x,u_y,u_z]` per corner) for a P1 tet from the
/// global displacement vector, in element-local node order.
pub fn gather_u_p1(u: &[f64], conn: &[usize; 4]) -> [f64; 12] {
    let mut ue = [0.0_f64; 12];
    for (k, &node) in conn.iter().enumerate() {
        ue[3 * k] = u[3 * node];
        ue[3 * k + 1] = u[3 * node + 1];
        ue[3 * k + 2] = u[3 * node + 2];
    }
    ue
}

/// Compute von Mises stress from a 3×3 Cauchy stress tensor.
pub fn von_mises_of_tensor(s: &[[f64; 3]; 3]) -> f64 {
    let (s11, s22, s33) = (s[0][0], s[1][1], s[2][2]);
    let (s12, s23, s13) = (s[0][1], s[1][2], s[0][2]);
    let v = 0.5 * ((s11 - s22).powi(2) + (s22 - s33).powi(2) + (s33 - s11).powi(2))
        + 3.0 * (s12.powi(2) + s23.powi(2) + s13.powi(2));
    v.sqrt()
}

/// Pack `(nodes, conns)` (the procedural [`box_p1_mesh`] output format) into
/// a [`VolumeMesh`] (`f32` flat vertices + `u32` flat tet indices, `P1`
/// order).
///
/// Bridges the solve pipeline's native `(nodes, conns)` shape to the
/// [`VolumeMesh`] shape `FeaAdaptiveProblem` stores internally — needed by
/// both `compute_zz_indicator` (reads `mesh.vertices.len()`) and `refine`
/// (calls `refine_marked_elements`, starting step-6).
pub fn volume_mesh_from_nodes_conns(nodes: &[[f64; 3]], conns: &[[usize; 4]]) -> VolumeMesh {
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
        connectivity: VolumeConnectivity::Tet {
            indices: tet_indices,
            order: ElementOrderTag::P1,
        },
        normals: None,
        boundary: None,
    }
}

/// Unpack a `P1` [`VolumeMesh`] into `(nodes, conns)` — the inverse of
/// [`volume_mesh_from_nodes_conns`]. `nodes` widen the mesh's `f32`
/// coordinates to `f64` (the solve pipeline's native precision).
///
/// Compacts away vertices NOT referenced by any tet element before
/// returning (and remaps `conns` to the compacted indices). A real Gmsh
/// remesh can emit boundary vertices that survive `classify_surfaces` but
/// are not incident to any volume tet — the same orphaned-vertex artifact
/// `reify_kernel_gmsh::BackgroundSizeField::from_tet_mesh` already tolerates
/// (there, by reading sizes only at vertices a tet references; here, by
/// dropping the vertex outright). Left in `nodes`, an orphaned vertex gets no
/// stiffness-matrix contribution at all, silently inflating `n_dofs` and — if
/// any Dirichlet BC or point load ever targets it — panicking downstream in
/// `apply_dirichlet_row_elimination` ("no explicit diagonal entry"). The
/// procedural [`box_p1_mesh`] fixtures have no orphans (every vertex is a
/// hex-grid corner referenced by 6 tets), so this compaction is a no-op for
/// them — it only does real work on Gmsh-produced meshes (starting step-5).
///
/// # Panics
///
/// If `volume_mesh.element_order != ElementOrderTag::P1` — this suite (and
/// `compute_zz_indicator`) supports P1 tets only.
pub fn nodes_conns_from_volume_mesh(volume_mesh: &VolumeMesh) -> (Vec<[f64; 3]>, Vec<[usize; 4]>) {
    assert_eq!(
        volume_mesh.element_order(),
        Some(ElementOrderTag::P1),
        "FeaAdaptiveProblem supports P1 tets only",
    );
    let n_verts = volume_mesh.vertices.len() / 3;
    let raw_conns: Vec<[usize; 4]> = volume_mesh
        .tet_indices()
        .expect("checked P1 above")
        .chunks_exact(4)
        .map(|c| [c[0] as usize, c[1] as usize, c[2] as usize, c[3] as usize])
        .collect();

    let mut referenced = vec![false; n_verts];
    for conn in &raw_conns {
        for &v in conn {
            referenced[v] = true;
        }
    }

    // old index -> compacted index; only populated for referenced vertices.
    let mut remap = vec![usize::MAX; n_verts];
    let mut nodes = Vec::with_capacity(n_verts);
    for (old, &is_ref) in referenced.iter().enumerate() {
        if is_ref {
            remap[old] = nodes.len();
            nodes.push([
                volume_mesh.vertices[old * 3] as f64,
                volume_mesh.vertices[old * 3 + 1] as f64,
                volume_mesh.vertices[old * 3 + 2] as f64,
            ]);
        }
    }

    let conns: Vec<[usize; 4]> = raw_conns
        .iter()
        .map(|c| [remap[c[0]], remap[c[1]], remap[c[2]], remap[c[3]]])
        .collect();

    (nodes, conns)
}
