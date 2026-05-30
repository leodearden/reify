//! P2-tet modal-frequency accuracy benchmarks (task 4066).
//!
//! # Goal
//!
//! Close the bending-lock gap on a slender beam to the **2%** aspirational
//! modal-frequency target carried off task 3819. The P1 constant-strain tet
//! locks in bending (`f ∝ √K`), flooring the first natural frequency of the
//! `L = 200 mm, h = 2 mm, L/r ≈ 346` beam several percent high at any
//! CI-practical mesh. The lever — exactly as the P2 buckling path
//! (`tests/euler_column_pin_pin.rs::fixed_guided_euler_column_p2_within_five_percent`,
//! task 4052) — is the P2 (quadratic, 10-node) tet, whose shape functions
//! resolve the bending curvature in both `K` and the consistent mass `M`.
//!
//! These benchmarks are the **headline accuracy gate** for the P2 modal path.
//! They are self-contained in `reify-solver-elastic` (which cannot depend on
//! `reify-eval`, where the modal orchestration lives): each test builds a P1
//! brick grid, promotes it to P2 (`promote_tets_to_p2`), assembles `K`
//! (`element_stiffness(ElementOrder::P2, …)`) and `M`
//! (`consistent_element_mass_tet_p2`), projects to the free-DOF subspace inline
//! (mirroring `modal_ops::project_free`), and calls `solve_eigen_dense` —
//! mirroring the self-contained euler P2 buckling benchmark.
//!
//! # Tolerance — the project-sanctioned escape hatch
//!
//! The RED test targets the aspirational **2%**. GREEN (step-4) calibrates the
//! `CANTILEVER_P2_REL_TOL` constant to the **honest measured P2 floor** at an
//! example-practical mesh: 2% if achievable, else the measured value with a
//! documented tuning history (and a bookmarked follow-up for a dedicated
//! lock-free 1-D beam/frame element — the only route to a true 2% on a
//! `L/r ≈ 346` beam, which `reify-solver-elastic` has no element for today).
//! This mirrors the euler P2 precedent and the structural-analysis-shells.md:18
//! thin-feature-penalty discipline: the bound is calibrated from a printed
//! measurement, never guessed.
//!
//! # Profile gating
//!
//! `cfg_attr(debug_assertions, ignore)` release-gates each benchmark (same
//! rationale as the euler P2 test): the dense generalized eigensolve is fast in
//! release but slow under the debug allocator, and `verify.sh` runs the release
//! pass without `--run-ignored`, so a bare `#[ignore]` would silently drop the
//! deliverable in both profiles.

use std::f64::consts::PI;

use faer::sparse::{SparseRowMat, Triplet};

use reify_solver_elastic::assembly::test_support::promote_tets_to_p2;
use reify_solver_elastic::{
    AssemblyElement, AssemblyMode, DirichletBc, EigenSolverOptions, ElementOrder, ElementStiffness,
    IsotropicElastic, assemble_global_stiffness, consistent_element_mass_tet_p2, element_stiffness,
    solve_eigen_dense,
};

// ---------------------------------------------------------------------------
// Material constants — Steel AISI 1045 (SI)
// ---------------------------------------------------------------------------

const STEEL_E_PA: f64 = 205.0e9;
const STEEL_NU: f64 = 0.29;
const STEEL_DENSITY: f64 = 7850.0; // kg/m³

// ---------------------------------------------------------------------------
// Beam-grid scaffolding (adapted from `tests/euler_column_pin_pin.rs`)
// ---------------------------------------------------------------------------

/// Brick-grid dimensions for a beam fixture spanning `[0, lx] × [0, ly] × [0, lz]`.
///
/// Convention (matching `modal_ops::build_beam_mesh`): X = beam axis (length),
/// Y = width, Z = height (the thin bending axis).
struct BeamFixture {
    nx: usize,
    ny: usize,
    nz: usize,
    lx: f64,
    ly: f64,
    lz: f64,
}

impl BeamFixture {
    fn n_nodes(&self) -> usize {
        (self.nx + 1) * (self.ny + 1) * (self.nz + 1)
    }

    /// Row-major `(k, j, i)` node linearisation (same as `euler_column_pin_pin.rs`).
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

    /// Second moment of area for bending in Z (deflection along the thin axis):
    /// `I = width · height³ / 12`.
    fn i_bending_z(&self) -> f64 {
        self.ly * self.lz.powi(3) / 12.0
    }

    /// Cross-section area `A = width · height`.
    fn area(&self) -> f64 {
        self.ly * self.lz
    }
}

/// Six-tet long-diagonal brick decomposition (verbatim from `euler_column_pin_pin.rs`).
const TET_DECOMPOSITION: [[usize; 4]; 6] = [
    [0, 1, 2, 6],
    [0, 2, 3, 6],
    [0, 3, 7, 6],
    [0, 7, 4, 6],
    [0, 4, 5, 6],
    [0, 5, 1, 6],
];

fn build_node_xyz(grid: &BeamFixture) -> Vec<[f64; 3]> {
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

fn build_tet_mesh(grid: &BeamFixture) -> Vec<[usize; 4]> {
    let mut tets = Vec::with_capacity(grid.nx * grid.ny * grid.nz * 6);
    for k in 0..grid.nz {
        for j in 0..grid.ny {
            for i in 0..grid.nx {
                let corner = [
                    grid.node_id(i,     j,     k),
                    grid.node_id(i + 1, j,     k),
                    grid.node_id(i + 1, j + 1, k),
                    grid.node_id(i,     j + 1, k),
                    grid.node_id(i,     j,     k + 1),
                    grid.node_id(i + 1, j,     k + 1),
                    grid.node_id(i + 1, j + 1, k + 1),
                    grid.node_id(i,     j + 1, k + 1),
                ];
                for split in TET_DECOMPOSITION {
                    tets.push([corner[split[0]], corner[split[1]], corner[split[2]], corner[split[3]]]);
                }
            }
        }
    }
    tets
}

// ---------------------------------------------------------------------------
// Inline free-DOF projection (mirrors `modal_ops::project_free`)
// ---------------------------------------------------------------------------

/// Extract the free×free submatrix of `full` over the non-Dirichlet DOFs.
///
/// `free_of_full[g]` maps full DOF `g` to its free-subspace index, or
/// `usize::MAX` if `g` is constrained. Verbatim Dirichlet-only projection from
/// `modal_ops::project_free` (the eval-side helper this self-contained kernel
/// test cannot reach across the crate boundary).
fn project_free(
    full: &SparseRowMat<usize, f64>,
    free_of_full: &[usize],
    n_free: usize,
) -> SparseRowMat<usize, f64> {
    let sym = full.symbolic();
    let mut trips: Vec<Triplet<usize, usize, f64>> = Vec::new();
    for g_row in 0..full.nrows() {
        let r = free_of_full[g_row];
        if r == usize::MAX {
            continue;
        }
        let cols = sym.col_idx_of_row_raw(g_row);
        let vals = full.val_of_row(g_row);
        for (col_raw, &val) in cols.iter().zip(vals.iter()) {
            let c = free_of_full[*col_raw];
            if c == usize::MAX || val == 0.0 {
                continue;
            }
            trips.push(Triplet::new(r, c, val));
        }
    }
    SparseRowMat::try_new_from_triplets(n_free, n_free, &trips)
        .expect("free-DOF submatrix construction must not violate CSR invariants")
}

/// Sum of all stored entries of a sparse matrix. For the consistent mass `M`
/// (axis-block-diagonal, each axis block summing by partition-of-unity to
/// `ρ·V`), `Σ_ij M[i,j] = 3·ρ·V_total` — a BC-independent total-mass sanity
/// check.
fn sum_all_entries(a: &SparseRowMat<usize, f64>) -> f64 {
    let mut sum = 0.0_f64;
    for r in 0..a.nrows() {
        for &val in a.val_of_row(r) {
            sum += val;
        }
    }
    sum
}

/// Assemble the global P2 stiffness `K` and consistent mass `M` for a promoted
/// P2 beam mesh, returning `(K, M)` as sparse matrices over `3·n_nodes_p2` DOFs.
fn assemble_p2_k_and_m(
    nodes_p2: &[[f64; 3]],
    tets_p2: &[[usize; 10]],
    material: &IsotropicElastic,
    density: f64,
) -> (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>) {
    let n_nodes = nodes_p2.len();

    // K — P2 element stiffness (30-DOF K_e), assembled via the opaque scatter.
    let k_elems: Vec<ElementStiffness> = tets_p2
        .iter()
        .map(|tet| {
            let phys: [[f64; 3]; 10] = std::array::from_fn(|i| nodes_p2[tet[i]]);
            element_stiffness(ElementOrder::P2, &phys[..], material)
        })
        .collect();
    let k_assembly: Vec<AssemblyElement<'_>> = tets_p2
        .iter()
        .zip(k_elems.iter())
        .enumerate()
        .map(|(id, (conn, k_e))| AssemblyElement { id, connectivity: conn, k_e })
        .collect();
    let k_full = assemble_global_stiffness(n_nodes, &k_assembly, AssemblyMode::Deterministic);

    // M — P2 consistent mass (30-DOF M_e), assembled the same way (the assembler
    // treats k_e opaquely — K vs M).
    let m_elems: Vec<ElementStiffness> = tets_p2
        .iter()
        .map(|tet| {
            let phys: [[f64; 3]; 10] = std::array::from_fn(|i| nodes_p2[tet[i]]);
            consistent_element_mass_tet_p2(&phys, density)
        })
        .collect();
    let m_assembly: Vec<AssemblyElement<'_>> = tets_p2
        .iter()
        .zip(m_elems.iter())
        .enumerate()
        .map(|(id, (conn, m_e))| AssemblyElement { id, connectivity: conn, k_e: m_e })
        .collect();
    let m_full = assemble_global_stiffness(n_nodes, &m_assembly, AssemblyMode::Deterministic);

    (k_full, m_full)
}

/// Build the free-DOF maps from a Dirichlet BC list over `n_dofs` total DOFs.
/// Returns `(free_of_full, n_free)`.
fn free_dof_map(n_dofs: usize, bcs: &[DirichletBc]) -> (Vec<usize>, usize) {
    let mut is_constrained = vec![false; n_dofs];
    for bc in bcs {
        if bc.dof < n_dofs {
            is_constrained[bc.dof] = true;
        }
    }
    let mut free_of_full = vec![usize::MAX; n_dofs];
    let mut n_free = 0_usize;
    for (g, &constrained) in is_constrained.iter().enumerate() {
        if !constrained {
            free_of_full[g] = n_free;
            n_free += 1;
        }
    }
    (free_of_full, n_free)
}

// ---------------------------------------------------------------------------
// Cantilever P2 modal measurement (shared by the benchmark + the tuning sweep)
// ---------------------------------------------------------------------------

/// Outcome of one cantilever P2 modal solve on a given mesh.
struct CantileverMeasurement {
    f1: f64,
    f1_analytic: f64,
    f2: f64,
    f3: f64,
    rel_err: f64,
    n_free: usize,
    n_nodes_p2: usize,
    mass_rel: f64,
    converged: bool,
    lambda_min: f64,
}

/// Build the cantilever beam at the given mesh, assemble (K, M) at P2, clamp the
/// `x ≈ 0` root face, project to free DOFs, dense-eigensolve, and return the
/// measured fundamental frequency vs the Euler-Bernoulli reference.
fn measure_cantilever(grid: &BeamFixture) -> CantileverMeasurement {
    let nodes_p1 = build_node_xyz(grid);
    let tets_p1 = build_tet_mesh(grid);
    let (nodes_p2, tets_p2) = promote_tets_to_p2(&nodes_p1, &tets_p1);
    let n_nodes_p2 = nodes_p2.len();
    let n_dofs = 3 * n_nodes_p2;

    let material = IsotropicElastic { youngs_modulus: STEEL_E_PA, poisson_ratio: STEEL_NU };

    // BCs: clamp the x ≈ 0 root face (all 3 DOFs, catches P2 edge-midpoints).
    let mut bcs: Vec<DirichletBc> = Vec::new();
    for (n, xyz) in nodes_p2.iter().enumerate() {
        if (xyz[0] - 0.0).abs() < 1e-10 {
            for axis in 0..3_usize {
                bcs.push(DirichletBc { dof: 3 * n + axis, value: 0.0 });
            }
        }
    }
    assert!(!bcs.is_empty(), "cantilever must clamp at least one root-face DOF");

    // Assemble (K, M) at P2.
    let (k_full, m_full) = assemble_p2_k_and_m(&nodes_p2, &tets_p2, &material, STEEL_DENSITY);

    // Total-mass sanity: Σ M = 3·ρ·V_total (V_total = L·b·h, exact box fill).
    let v_total = grid.lx * grid.ly * grid.lz;
    let total_mass_sum = sum_all_entries(&m_full);
    let expected_mass_sum = 3.0 * STEEL_DENSITY * v_total;
    let mass_rel = (total_mass_sum - expected_mass_sum).abs() / expected_mass_sum;

    // Project to the free-DOF subspace and solve K_free φ = λ M_free φ.
    let (free_of_full, n_free) = free_dof_map(n_dofs, &bcs);
    let k_free = project_free(&k_full, &free_of_full, n_free);
    let m_free = project_free(&m_full, &free_of_full, n_free);
    let opts = EigenSolverOptions { n_modes: 3, tol: 1e-9, max_iters: 200, sigma: 0.0 };
    let eig = solve_eigen_dense(&k_free, &m_free, opts);

    let lambda_min = eig.eigenvalues.first().copied().unwrap_or(f64::NAN);
    let to_hz = |l: f64| l.sqrt() / (2.0 * PI);
    let f1 = to_hz(lambda_min);
    let f2 = eig.eigenvalues.get(1).map(|&l| to_hz(l)).unwrap_or(f64::NAN);
    let f3 = eig.eigenvalues.get(2).map(|&l| to_hz(l)).unwrap_or(f64::NAN);

    // Analytic Euler-Bernoulli cantilever first frequency:
    // f₁ = (β₁L)²/(2π) · √(EI / (ρ A L⁴)), β₁L = 1.875104, I = b·h³/12, A = b·h.
    const BETA1_L: f64 = 1.875104;
    let f1_analytic = BETA1_L.powi(2) / (2.0 * PI)
        * (STEEL_E_PA * grid.i_bending_z() / (STEEL_DENSITY * grid.area() * grid.lx.powi(4)))
            .sqrt();
    let rel_err = (f1 - f1_analytic).abs() / f1_analytic;

    CantileverMeasurement {
        f1,
        f1_analytic,
        f2,
        f3,
        rel_err,
        n_free,
        n_nodes_p2,
        mass_rel,
        converged: eig.converged,
        lambda_min,
    }
}

// ---------------------------------------------------------------------------
// Step-3 (RED) / Step-4 (GREEN): cantilever P2 modal frequency within 2%
// ---------------------------------------------------------------------------

/// Calibrated relative-error bound on the cantilever fundamental frequency.
///
/// **Step-3 RED**: initialized to the aspirational 2% target. **Step-4 GREEN**
/// recalibrates to the honest measured P2 floor with a documented tuning
/// history.
const CANTILEVER_P2_REL_TOL: f64 = 0.02;

/// Cantilever (clamped-free) P2 modal-frequency accuracy benchmark — task 4066.
///
/// Slender steel beam: `L = 200 mm` (X span) × `b = 10 mm` (Y width) ×
/// `h = 2 mm` (Z height, the thin bending axis), `L/r ≈ 346`. AISI 1045
/// (E = 205 GPa, ν = 0.29, ρ = 7850 kg/m³). The fundamental is the first
/// transverse bending mode in the thin (Z) direction.
///
/// BCs: the `x ≈ 0` (root) face is fully clamped — all 3 DOFs at every P2 node
/// on the face, selected by x-coordinate so the P2 edge-midpoint nodes on the
/// face are caught too (the euler P2 coordinate-selection pattern).
///
/// Reference (Euler-Bernoulli cantilever first mode):
/// `f₁ = (1.875104² / 2π) · √(EI / (ρ A L⁴)) ≈ 41 Hz`, with
/// `I = b·h³/12`, `A = b·h`.
///
/// Passes when `|f₁ − f₁,analytic| / f₁,analytic < CANTILEVER_P2_REL_TOL`.
#[cfg_attr(
    debug_assertions,
    ignore = "heavy (dense modal eigensolve): release-only at the merge gate; debug skips it for per-task speed — task 4066"
)]
#[test]
fn cantilever_beam_p2_modal_within_two_percent() {
    // Step-3 RED: a coarse first-guess span mesh (nx=8). Measured rel_err 2.38%
    // > 2% — fails the target, motivating the step-4 mesh refinement.
    let grid = BeamFixture { nx: 8, ny: 1, nz: 1, lx: 0.2, ly: 0.01, lz: 0.002 };
    let m = measure_cantilever(&grid);

    eprintln!(
        "cantilever P2 (nx={}, ny={}, nz={}, P2 nodes={}, n_free={}): \
         f1 = {:.4} Hz, analytic = {:.4} Hz, rel_err = {:.4}% (f2 = {:.2} Hz, f3 = {:.2} Hz); \
         Σ M rel = {:.2e}",
        grid.nx, grid.ny, grid.nz, m.n_nodes_p2, m.n_free,
        m.f1, m.f1_analytic, m.rel_err * 100.0, m.f2, m.f3, m.mass_rel,
    );

    assert!(m.converged, "dense modal eigensolve must converge for the cantilever");
    assert!(m.lambda_min > 0.0, "λ_min = {} must be positive (free vibration)", m.lambda_min);
    assert!(
        m.mass_rel < 1e-9,
        "global consistent-mass total Σ M off by rel {:.2e} from 3ρV",
        m.mass_rel,
    );
    assert!(
        m.rel_err < CANTILEVER_P2_REL_TOL,
        "cantilever P2: f1 = {:.4} Hz, analytic = {:.4} Hz, rel_err = {:.4}% > {:.2}%",
        m.f1, m.f1_analytic, m.rel_err * 100.0, CANTILEVER_P2_REL_TOL * 100.0,
    );
}
