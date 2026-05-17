//! γ task observable signal (PRD `docs/prds/v0_5/buckling-eigensolver.md` §13):
//! P1-tet `K_g` element kernel + global assembly + coarse-mesh Euler-column
//! eigenvalue sanity at 10% tolerance.
//!
//! ## Per-element checks
//!
//! - `per_element_k_g_is_symmetric` — pins K_g symmetry on a single unit tet
//!   under non-trivial uniaxial stress (signal (a)).
//! - `per_element_k_g_with_zero_stress_is_identically_zero` — pins the
//!   rank-0-on-zero-stress contract (signal (b)).
//!
//! ## Euler-column buckling
//!
//! - `euler_column_pin_pin_within_ten_percent` — builds a 1×1×10 column meshed
//!   with `nx=10, ny=1, nz=40` bricks (each split into 6 tets ⇒ 2400 tets,
//!   2617 free DOFs), applies uniform compressive pre-stress `σ_zz = −1`,
//!   solves `K φ = λ (−K_g) φ` via shift-invert Lanczos
//!   (`solve_eigen_shift_invert`), and asserts the smallest |λ| matches the
//!   analytical Euler critical load `π²·E·I/L² = π²/12·100⁻¹` to within 10%
//!   relative error (signal (c)).
//!
//! ### Why shift-invert + anisotropic mesh
//!
//! Linear (constant-strain) P1 tetrahedra lock catastrophically in bending —
//! the slender Euler column with `L/r ≈ 35` needs ~2600 free DOFs to reach
//! 10% accuracy. At that size the dense generalized eigensolver (faer's QZ,
//! O(n³)) takes ~80s in release and minutes in debug, which would push the
//! test over the debug-profile "seconds" bar that the verify pipeline tracks.
//! Shift-invert Lanczos with sparse Cholesky (the `solve_eigen_shift_invert`
//! path when `n > 64`) handles n≈2600 in a few seconds.
//!
//! See PRD §14 open question (1) on factorization reuse and the design note
//! in `eigensolve.rs` on the dense-fallback boundary at `effective_max_dim ≥
//! n` (i.e. `n ≤ 64`).

use faer::sparse::{SparseRowMat, Triplet};
use reify_solver_elastic::{
    assemble_global_stiffness, element_stiffness, geometric_element_stiffness_tet_p1,
    solve_eigen_shift_invert, AssemblyElement, AssemblyMode, EigenSolverOptions, ElementOrder,
    ElementStiffness, InitialStress3, IsotropicElastic,
};

// ---------------------------------------------------------------------------
// (a, b) Per-element K_g signals
// ---------------------------------------------------------------------------

const UNIT_TET: [[f64; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
];

#[test]
fn per_element_k_g_is_symmetric() {
    // Non-trivial σ — uniaxial compression along z.
    let k_g = geometric_element_stiffness_tet_p1(&UNIT_TET, &InitialStress3::uniaxial_z(-1.0));
    assert_eq!(k_g.n_dofs, 12, "P1 tet K_g must be 12-DOF");
    for i in 0..12 {
        for j in 0..12 {
            let lhs = k_g.data[i * 12 + j];
            let rhs = k_g.data[j * 12 + i];
            let scale = lhs.abs().max(rhs.abs()).max(1.0);
            assert!(
                (lhs - rhs).abs() < 1e-12 * scale,
                "K_g asymmetry at ({i},{j}): {lhs} vs {rhs}",
            );
        }
    }
}

#[test]
fn per_element_k_g_with_zero_stress_is_identically_zero() {
    // Rank-0 contract: σ ≡ 0 ⇒ K_g ≡ 0 entrywise.
    let k_g = geometric_element_stiffness_tet_p1(&UNIT_TET, &InitialStress3::zero());
    for (idx, &v) in k_g.data.iter().enumerate() {
        assert_eq!(v, 0.0, "K_g[{idx}] = {v}, expected 0 under σ = 0");
    }
}

// ---------------------------------------------------------------------------
// (c) Coarse-mesh Euler-column buckling signal
// ---------------------------------------------------------------------------

/// Brick-grid mesh dimensions for the Euler-column fixture.
///
/// `nx=10, ny=1, nz=40` cells over the domain `[0,1]×[0,1]×[0,10]`. The
/// anisotropic refinement (more cells across the cross-section than along
/// the axis would normally need) reflects the linear-tet locking story —
/// see the module docstring.
struct ColumnGrid {
    nx: usize,
    ny: usize,
    nz: usize,
    /// Physical dimensions: column = `[0, lx] × [0, ly] × [0, lz]`.
    lx: f64,
    ly: f64,
    lz: f64,
}

impl ColumnGrid {
    fn euler_fixture() -> Self {
        Self {
            nx: 10,
            ny: 1,
            nz: 40,
            lx: 1.0,
            ly: 1.0,
            lz: 10.0,
        }
    }

    fn n_nodes(&self) -> usize {
        (self.nx + 1) * (self.ny + 1) * (self.nz + 1)
    }

    /// Linearize the `(i, j, k)` grid index to a single node id, row-major
    /// over `(k, j, i)`. (Ordering is arbitrary; this convention happens to
    /// keep bottom-face nodes `k=0` consecutive at id `< (nx+1)*(ny+1)`.)
    fn node_id(&self, i: usize, j: usize, k: usize) -> usize {
        k * (self.nx + 1) * (self.ny + 1) + j * (self.nx + 1) + i
    }

    fn node_xyz(&self, i: usize, j: usize, k: usize) -> [f64; 3] {
        [
            (i as f64) * (self.lx / self.nx as f64),
            (j as f64) * (self.ly / self.ny as f64),
            (k as f64) * (self.lz / self.nz as f64),
        ]
    }
}

/// Six-tet decomposition of a single brick.
///
/// Brick corner local-indexing convention (matches a right-handed cube
/// with the long diagonal running 0→6):
/// ```text
///   z=0 face (counter-clockwise from +z): 0=(i,j,k), 1=(i+1,j,k),
///                                          2=(i+1,j+1,k), 3=(i,j+1,k)
///   z=+1 face (same cyclic order):         4=(i,j,k+1), 5=(i+1,j,k+1),
///                                          6=(i+1,j+1,k+1), 7=(i,j+1,k+1)
/// ```
/// The 6 tets all share the brick's long diagonal `0–6` so cross-brick faces
/// triangulate consistently when every brick uses the same split.
const TET_DECOMPOSITION: [[usize; 4]; 6] = [
    [0, 1, 2, 6],
    [0, 2, 3, 6],
    [0, 3, 7, 6],
    [0, 7, 4, 6],
    [0, 4, 5, 6],
    [0, 5, 1, 6],
];

/// Build the connectivity of every tetrahedron in the column mesh, in
/// `(elem_id → [node_id; 4])` order. The handedness of each tet is checked
/// at K_g-assembly time via the `assemble_global_stiffness` det-J path —
/// here we simply emit the canonical 6-tet decomposition of each brick.
fn build_tet_mesh(grid: &ColumnGrid) -> Vec<[usize; 4]> {
    let mut tets = Vec::with_capacity(grid.nx * grid.ny * grid.nz * 6);
    for k in 0..grid.nz {
        for j in 0..grid.ny {
            for i in 0..grid.nx {
                let corner = [
                    grid.node_id(i, j, k),
                    grid.node_id(i + 1, j, k),
                    grid.node_id(i + 1, j + 1, k),
                    grid.node_id(i, j + 1, k),
                    grid.node_id(i, j, k + 1),
                    grid.node_id(i + 1, j, k + 1),
                    grid.node_id(i + 1, j + 1, k + 1),
                    grid.node_id(i, j + 1, k + 1),
                ];
                for split in TET_DECOMPOSITION {
                    tets.push([
                        corner[split[0]],
                        corner[split[1]],
                        corner[split[2]],
                        corner[split[3]],
                    ]);
                }
            }
        }
    }
    tets
}

/// Build the node-coordinate array.
fn build_node_xyz(grid: &ColumnGrid) -> Vec<[f64; 3]> {
    let mut nodes = Vec::with_capacity(grid.n_nodes());
    for k in 0..=grid.nz {
        for j in 0..=grid.ny {
            for i in 0..=grid.nx {
                nodes.push(grid.node_xyz(i, j, k));
            }
        }
    }
    nodes
}

/// Pin-pin BC for the buckling fixture:
///
/// - All nodes on the bottom face `z = 0` and top face `z = lz`: clamp
///   `u_x = u_y = 0` (lateral restraint at both ends, but axial slip is
///   free at the top — i.e. classical pin-pin column).
/// - One bottom corner (node `(0,0,0)`): clamp `u_z = 0` (anchors axial
///   rigid-body translation; rotation about the z-axis is already pinned
///   by the lateral clamps).
///
/// Returns `(free_map, n_free)`:
///   `free_map[g] = usize::MAX` if global DOF `g` is fixed,
///   `free_map[g] = f` for the free-DOF index `f < n_free` otherwise.
fn build_pin_pin_free_dof_map(grid: &ColumnGrid) -> (Vec<usize>, usize) {
    let n_dofs = 3 * grid.n_nodes();
    let mut fixed = vec![false; n_dofs];

    // Bottom & top face lateral clamps.
    for k in [0usize, grid.nz] {
        for j in 0..=grid.ny {
            for i in 0..=grid.nx {
                let n = grid.node_id(i, j, k);
                fixed[3 * n] = true; // u_x
                fixed[3 * n + 1] = true; // u_y
            }
        }
    }
    // Axial anchor at one bottom corner.
    let anchor = grid.node_id(0, 0, 0);
    fixed[3 * anchor + 2] = true; // u_z

    let mut free_map = vec![usize::MAX; n_dofs];
    let mut n_free = 0usize;
    for (g, &is_fixed) in fixed.iter().enumerate() {
        if !is_fixed {
            free_map[g] = n_free;
            n_free += 1;
        }
    }
    (free_map, n_free)
}

/// Build the global sparse matrix `M_free` of size `n_free × n_free` from a
/// per-element collection by emitting triplets only between free DOFs and
/// remapping global indices to the free-DOF index space.
fn assemble_free_dof_matrix<F>(
    n_nodes: usize,
    tets: &[[usize; 4]],
    free_map: &[usize],
    n_free: usize,
    element_matrix: F,
) -> SparseRowMat<usize, f64>
where
    F: Fn(usize) -> ElementStiffness,
{
    // Build per-element matrices into a Vec so AssemblyElement can borrow
    // their references in a single pass. (Computing per-element K is cheap;
    // memory is dominated by the assembled global matrix.)
    let elements_k: Vec<ElementStiffness> = (0..tets.len()).map(element_matrix).collect();
    let assembly_elems: Vec<AssemblyElement<'_>> = tets
        .iter()
        .zip(elements_k.iter())
        .enumerate()
        .map(|(id, (conn, k_e))| AssemblyElement {
            id,
            connectivity: conn,
            k_e,
        })
        .collect();
    let full = assemble_global_stiffness(n_nodes, &assembly_elems, AssemblyMode::Deterministic);

    // Walk full's CSR storage and emit free-DOF triplets.
    let sym = full.symbolic();
    let n_rows = full.nrows();
    let mut trips: Vec<Triplet<usize, usize, f64>> = Vec::new();
    for global_row in 0..n_rows {
        let r = free_map[global_row];
        if r == usize::MAX {
            continue;
        }
        let cols = sym.col_idx_of_row_raw(global_row);
        let vals = full.val_of_row(global_row);
        for (col_idx, &val) in cols.iter().zip(vals.iter()) {
            let c = free_map[*col_idx];
            if c == usize::MAX || val == 0.0 {
                continue;
            }
            trips.push(Triplet::new(r, c, val));
        }
    }
    SparseRowMat::try_new_from_triplets(n_free, n_free, &trips)
        .expect("free-DOF sub-matrix construction must not violate CSR invariants")
}

#[test]
fn euler_column_pin_pin_within_ten_percent() {
    // ---- 1. Mesh + nodes ----------------------------------------------------
    let grid = ColumnGrid::euler_fixture();
    let tets = build_tet_mesh(&grid);
    let nodes = build_node_xyz(&grid);
    assert_eq!(nodes.len(), grid.n_nodes());

    // ---- 2. Material --------------------------------------------------------
    // E = 1, ν = 0 makes the analytical Euler formula clean: P_cr = π²·E·I/L².
    // ν = 0 avoids Poisson coupling that would broaden the locking diagnosis.
    let material = IsotropicElastic {
        youngs_modulus: 1.0,
        poisson_ratio: 0.0,
    };

    // Uniform pre-stress: unit compression along the column axis.
    let sigma = InitialStress3::uniaxial_z(-1.0);

    // ---- 3. BCs / free-DOF map ---------------------------------------------
    let (free_map, n_free) = build_pin_pin_free_dof_map(&grid);
    // Sanity-check the free-DOF count against the design (see module
    // docstring's "2617 free DOFs"). If this number drifts, the test setup
    // changed — re-derive the expected critical-load tolerance band.
    assert_eq!(
        n_free, 2617,
        "free-DOF count drifted (got {n_free}); re-derive Euler-tolerance band",
    );

    // ---- 4. Assemble K and -K_g over free DOFs -----------------------------
    let phys_nodes_for_tet = |tet: &[usize; 4]| -> [[f64; 3]; 4] {
        [nodes[tet[0]], nodes[tet[1]], nodes[tet[2]], nodes[tet[3]]]
    };

    let k_free = assemble_free_dof_matrix(grid.n_nodes(), &tets, &free_map, n_free, |elem_id| {
        let p = phys_nodes_for_tet(&tets[elem_id]);
        element_stiffness(ElementOrder::P1, &p[..], &material)
    });
    // The buckling convention `K φ = λ B φ` uses `B = −K_g` (per
    // `eigensolve::solve_eigen_shift_invert` doc). Assemble `−K_g` directly
    // by feeding `−σ` to the element kernel — `K_g` is linear in `σ`, so
    // this avoids a post-pass over the global sparse matrix.
    let neg_sigma = InitialStress3 {
        sigma: [
            [-sigma.sigma[0][0], -sigma.sigma[0][1], -sigma.sigma[0][2]],
            [-sigma.sigma[1][0], -sigma.sigma[1][1], -sigma.sigma[1][2]],
            [-sigma.sigma[2][0], -sigma.sigma[2][1], -sigma.sigma[2][2]],
        ],
    };
    let neg_k_g_free =
        assemble_free_dof_matrix(grid.n_nodes(), &tets, &free_map, n_free, |elem_id| {
            let p = phys_nodes_for_tet(&tets[elem_id]);
            geometric_element_stiffness_tet_p1(&p, &neg_sigma)
        });

    // ---- 5. Eigensolve: smallest |λ| -----------------------------------------
    let opts = EigenSolverOptions {
        n_modes: 1,
        tol: 1e-8,
        max_iters: 30,
        sigma: 0.0,
    };
    let result = solve_eigen_shift_invert(&k_free, &neg_k_g_free, opts);
    assert!(
        result.converged,
        "shift-invert Lanczos must converge: n_converged={}, eigenvalues.len()={}",
        result.n_converged,
        result.eigenvalues.len(),
    );

    let lambda_min = result.eigenvalues[0];
    assert!(
        lambda_min > 0.0,
        "smallest |λ| must be positive for compressive σ (compression-stabilized B = −K_g is PSD), got {lambda_min}",
    );

    // ---- 6. Analytical Euler comparison -------------------------------------
    // P_cr = π² · E · I_min / L² for pin-pin column.
    // Cross-section is `lx × ly = 1 × 1` ⇒ I = b·h³/12 = 1/12 (both axes
    // equivalent for a square section; the column buckles in whichever
    // plane the mesh asymmetry favours).
    let e_modulus = material.youngs_modulus;
    let l = grid.lz;
    let i_min = (grid.lx * grid.ly.powi(3) / 12.0).min(grid.ly * grid.lx.powi(3) / 12.0);
    let p_cr = std::f64::consts::PI.powi(2) * e_modulus * i_min / (l * l);

    let rel_err = (lambda_min - p_cr).abs() / p_cr;
    assert!(
        rel_err < 0.10,
        "Euler buckling λ_min = {lambda_min}, P_cr = {p_cr}, rel_err = {:.3}% (must be < 10%)",
        rel_err * 100.0,
    );
}
