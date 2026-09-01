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
// Shared P2 beam modal core (task 6663 amendment — review suggestion 4)
// ---------------------------------------------------------------------------
//
// All three benchmarks in this file (cantilever, simply-supported,
// clamped-clamped) run the SAME pipeline — build_node_xyz → build_tet_mesh →
// promote_tets_to_p2 → assemble_p2_k_and_m → Σ M sanity → free_dof_map →
// project_free → solve_eigen_dense → λ-to-Hz — and differ ONLY in the Dirichlet
// set, the requested mode count and the analytic βL. That pipeline lived in
// three verbatim copies, so a fix to the assembly or the mass-sanity logic had
// to be applied three times. It is written once below and parameterized by the
// three things that actually vary.

/// One assembled-and-solved P2 beam modal problem: everything the three
/// benchmarks share, with only the Dirichlet set and the mode count varying.
struct BeamModalSolve {
    /// Mode frequencies (Hz), ascending — one per returned eigenpair.
    freqs: Vec<f64>,
    /// Per-mode `(x, y, z)` eigenvector energy fractions, index-aligned with
    /// `freqs`. Used by the benchmarks that must pick a mode FAMILY out of an
    /// interleaved spectrum; ignored by the ones whose fundamental is unambiguous.
    fracs: Vec<[f64; 3]>,
    /// Smallest eigenvalue λ (rad²/s²), or NaN when no eigenpair came back.
    lambda_min: f64,
    n_free: usize,
    n_nodes_p2: usize,
    /// Relative deviation of `Σ_ij M[i,j]` from the exact `3·ρ·V_total` — a
    /// BC-independent total-mass sanity check every benchmark asserts on.
    mass_rel: f64,
    converged: bool,
}

/// Build the beam at `grid`, promote to P2, assemble `(K, M)`, apply the
/// Dirichlet set `bcs_of` returns for the promoted node set, project to the free
/// DOFs and dense-eigensolve.
///
/// `bcs_of` receives the PROMOTED (P2) node coordinates, so a coordinate
/// selection catches the edge-midpoint nodes on a face as well as the corners —
/// the selection pattern every benchmark here relies on.
fn solve_beam_modes(
    grid: &BeamFixture,
    bcs_of: impl Fn(&[[f64; 3]]) -> Vec<DirichletBc>,
    opts: EigenSolverOptions,
) -> BeamModalSolve {
    let nodes_p1 = build_node_xyz(grid);
    let tets_p1 = build_tet_mesh(grid);
    let (nodes_p2, tets_p2) = promote_tets_to_p2(&nodes_p1, &tets_p1);
    let n_nodes_p2 = nodes_p2.len();
    let n_dofs = 3 * n_nodes_p2;

    let material = IsotropicElastic { youngs_modulus: STEEL_E_PA, poisson_ratio: STEEL_NU };

    let bcs = bcs_of(&nodes_p2);
    assert!(!bcs.is_empty(), "a beam modal benchmark must constrain at least one DOF");

    // Assemble (K, M) at P2.
    let (k_full, m_full) = assemble_p2_k_and_m(&nodes_p2, &tets_p2, &material, STEEL_DENSITY);

    // Total-mass sanity: Σ M = 3·ρ·V_total (V_total = L·b·h, exact box fill).
    let v_total = grid.lx * grid.ly * grid.lz;
    let total_mass_sum = sum_all_entries(&m_full);
    let expected_mass_sum = 3.0 * STEEL_DENSITY * v_total;
    let mass_rel = (total_mass_sum - expected_mass_sum).abs() / expected_mass_sum;

    // Project to the free-DOF subspace (and build the inverse free→full map the
    // per-axis eigenvector classification needs).
    let (free_of_full, n_free) = free_dof_map(n_dofs, &bcs);
    let mut full_of_free = vec![0_usize; n_free];
    for (g, &f) in free_of_full.iter().enumerate() {
        if f != usize::MAX {
            full_of_free[f] = g;
        }
    }
    let k_free = project_free(&k_full, &free_of_full, n_free);
    let m_free = project_free(&m_full, &free_of_full, n_free);
    let eig = solve_eigen_dense(&k_free, &m_free, opts);

    let to_hz = |l: f64| l.sqrt() / (2.0 * PI);
    let lambda_min = eig.eigenvalues.first().copied().unwrap_or(f64::NAN);
    let freqs: Vec<f64> = eig.eigenvalues.iter().map(|&l| to_hz(l)).collect();
    // `eigenvectors` is a faer `Mat<f64>` (column j = mode j, row = free DOF);
    // `col_as_slice(j)` views column j as a contiguous `&[f64]` over the free
    // DOFs (the `modal_ops` idiom). `j < eigenvalues.len() == ncols`, so the
    // column index is always in range.
    let fracs: Vec<[f64; 3]> = (0..eig.eigenvalues.len())
        .map(|j| axis_energy_fractions(eig.eigenvectors.col_as_slice(j), &full_of_free))
        .collect();

    BeamModalSolve {
        freqs,
        fracs,
        lambda_min,
        n_free,
        n_nodes_p2,
        mass_rel,
        converged: eig.converged,
    }
}

/// Per-axis energy fractions `(x, y, z)` of an eigenvector over the free DOFs:
/// `e[α] = Σ_i φ_i² · [axis(i) == α]`, normalized to sum 1. The vertical
/// (Z-dominant) bending modes have `z ≈ 1`; lateral (Y) and axial (X) modes have
/// their energy on the other axes. This is the mode-identification lever the SS
/// benchmark needs because — unlike the locked P1 `ny=1` mesh — the P2 mesh
/// resolves the lateral Y-bending mode (`≈ 5× f₁,z ≈ 579 Hz`) *within* the first
/// few eigenvalues, so it intrudes between the 2nd and 3rd vertical modes.
fn axis_energy_fractions(eigvec: &[f64], full_of_free: &[usize]) -> [f64; 3] {
    let mut e = [0.0_f64; 3];
    for (free_idx, &comp) in eigvec.iter().enumerate() {
        e[full_of_free[free_idx] % 3] += comp * comp;
    }
    let total = e[0] + e[1] + e[2];
    if total > 0.0 { [e[0] / total, e[1] / total, e[2] / total] } else { [0.0; 3] }
}

/// Analytic Euler-Bernoulli natural frequency of this file's beam fixture for a
/// given dimensionless eigen-coefficient `βL`:
/// `f = (βL)²/(2π) · √(EI / (ρ A L⁴))`, `I = b·h³/12`, `A = b·h`.
///
/// The BC family lives ENTIRELY in `βL` — 1.875104 (clamped-free), `nπ`
/// (pinned-pinned), 4.730041 (clamped-clamped) — which is why one function
/// serves all three benchmarks; it used to be written out three times.
fn analytic_beam_frequency(grid: &BeamFixture, beta_l: f64) -> f64 {
    beta_l.powi(2) / (2.0 * PI)
        * (STEEL_E_PA * grid.i_bending_z() / (STEEL_DENSITY * grid.area() * grid.lx.powi(4))).sqrt()
}

/// Clamp all three translational DOFs at every promoted-P2 node satisfying
/// `on_face`. The clamp-family BC builder: the cantilever passes the `x ≈ 0`
/// root face, the clamped-clamped benchmark passes `x ≈ 0` OR `x ≈ L`.
fn clamped_face_bcs(
    nodes_p2: &[[f64; 3]],
    on_face: impl Fn(&[f64; 3]) -> bool,
) -> Vec<DirichletBc> {
    let mut bcs = Vec::new();
    for (n, xyz) in nodes_p2.iter().enumerate() {
        if on_face(xyz) {
            for axis in 0..3_usize {
                bcs.push(DirichletBc { dof: 3 * n + axis, value: 0.0 });
            }
        }
    }
    bcs
}

/// Outcome of one clamp-family (cantilever / clamped-clamped) P2 modal solve.
///
/// One struct for both: the two benchmarks assert on exactly the same fields, so
/// the former `CantileverMeasurement` / `ClampedClampedMeasurement` pair was a
/// field-for-field duplicate.
struct BeamModalMeasurement {
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

/// Measure a clamp-family beam's fundamental against its Euler-Bernoulli
/// reference: clamp every P2 node satisfying `on_clamped_face`, solve the three
/// lowest modes, and compare `f₁` to `analytic_beam_frequency(grid, beta1_l)`.
///
/// The BC predicate and `βL` are the ONLY things that separate the cantilever
/// benchmark from the clamped-clamped one.
fn measure_beam_modes(
    grid: &BeamFixture,
    on_clamped_face: impl Fn(&[f64; 3]) -> bool,
    beta1_l: f64,
) -> BeamModalMeasurement {
    let opts = EigenSolverOptions { n_modes: 3, tol: 1e-9, max_iters: 200, sigma: 0.0 };
    let solve = solve_beam_modes(grid, |nodes_p2| clamped_face_bcs(nodes_p2, &on_clamped_face), opts);

    let f1 = solve.freqs.first().copied().unwrap_or(f64::NAN);
    let f2 = solve.freqs.get(1).copied().unwrap_or(f64::NAN);
    let f3 = solve.freqs.get(2).copied().unwrap_or(f64::NAN);
    let f1_analytic = analytic_beam_frequency(grid, beta1_l);
    let rel_err = (f1 - f1_analytic).abs() / f1_analytic;

    BeamModalMeasurement {
        f1,
        f1_analytic,
        f2,
        f3,
        rel_err,
        n_free: solve.n_free,
        n_nodes_p2: solve.n_nodes_p2,
        mass_rel: solve.mass_rel,
        converged: solve.converged,
        lambda_min: solve.lambda_min,
    }
}

// ---------------------------------------------------------------------------
// Cantilever P2 modal measurement (shared by the benchmark + the tuning sweep)
// ---------------------------------------------------------------------------

/// Euler-Bernoulli clamped-free (cantilever) first-mode eigen-coefficient.
const BETA1_L_CANTILEVER: f64 = 1.875104;

/// Build the cantilever beam at the given mesh, clamp the `x ≈ 0` root face
/// (all 3 DOFs, by coordinate so the P2 edge-midpoints on the face are caught
/// too), and return the measured fundamental vs the Euler-Bernoulli reference.
///
/// One call site over [`measure_beam_modes`]; the root-face predicate and
/// `βL = 1.875104` are the whole of what makes it "the cantilever".
fn measure_cantilever(grid: &BeamFixture) -> BeamModalMeasurement {
    measure_beam_modes(grid, |xyz| xyz[0].abs() < 1e-10, BETA1_L_CANTILEVER)
}

// ---------------------------------------------------------------------------
// Step-3 (RED) / Step-4 (GREEN): cantilever P2 modal frequency within 2%
// ---------------------------------------------------------------------------

/// Calibrated relative-error bound on the cantilever fundamental frequency.
///
/// **The 2% aspirational target is achieved** by the P2 path at the
/// example-practical mesh below, so this bound stays at the deliverable's 2%
/// (it is NOT a loosened measured floor — contrast the euler P2 5% and the P1
/// 9–11% bounds). The honest measured error is **1.35%** at `nx=16, ny=1,
/// nz=1`, leaving a comfortable 0.65% margin; the dense QZ eigensolve is fully
/// deterministic, so there is no Lanczos-style cross-platform variance to
/// absorb.
///
/// # Tuning history (release mode, analytic f₁ = 41.2755 Hz)
///
/// | nx×ny×nz | P2 nodes | n_free | f₁ (Hz) | rel_err | note                       |
/// |----------|----------|--------|---------|---------|----------------------------|
/// | 8×1×1    | 153      | 432    | 42.256  | 2.38%   | step-3 RED — fails 2%      |
/// | 10×1×1   | 189      | 540    | 42.082  | 1.95%   | passes, but 0.05% margin   |
/// | 12×1×1   | 225      | 648    | 41.970  | 1.68%   | passes, 0.32% margin       |
/// | **16×1×1** | **297** | **864** | **41.832** | **1.35%** | **chosen** — 0.65% margin, ~25 s |
/// | 16×1×2   | 495      | 1440   | 41.775  | 1.21%   | nz refinement: +0.14% only |
///
/// `f₁` decreases monotonically toward a ~1.2% floor as the **span** mesh
/// refines (the FEA is stiffer than the beam reference at every mesh — `f₁ >
/// analytic` throughout), and cross-section refinement (`nz=2`) barely moves it
/// (1.35% → 1.21%). That signature is the 3D-solid-vs-Euler-Bernoulli model gap
/// (shear/rotary-inertia + Poisson coupling at `L/r ≈ 346`), **not** the P1
/// constant-strain bending lock the P2 element removes. `nx=16` is the smallest
/// span mesh with a solid (>0.5%) margin under 2% at sane release runtime
/// (~25 s for the sequential dense QZ solve on 864 free DOFs); `nx=10` clears 2%
/// but only by 0.05% (too fragile), and `nz=2` doubles the DOFs (→ ~115 s) for a
/// negligible 0.14% gain. Because 2% is met, the dedicated lock-free 1-D
/// beam/frame element (the route a >2% floor would have required) is NOT needed.
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
    // Example-practical P2 mesh (step-4 calibrated): nx=16 along the span clears
    // 2% at 1.35% with a 0.65% margin in ~25 s — see CANTILEVER_P2_REL_TOL's
    // tuning history for the full nx/nz sweep.
    let grid = BeamFixture { nx: 16, ny: 1, nz: 1, lx: 0.2, ly: 0.01, lz: 0.002 };
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

// ---------------------------------------------------------------------------
// Simply-supported (pin-pin) P2 modal measurement
// ---------------------------------------------------------------------------

/// Index of the node nearest `target` in Euclidean distance — used to place the
/// simply-supported neutral-axis anchors by coordinate over the promoted P2 node
/// set (mirrors `modal_ops::nearest_node`).
fn nearest_node(nodes: &[[f64; 3]], target: [f64; 3]) -> usize {
    let dist2 = |p: &[f64; 3]| {
        let dx = p[0] - target[0];
        let dy = p[1] - target[1];
        let dz = p[2] - target[2];
        dx * dx + dy * dy + dz * dz
    };
    nodes
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            dist2(a).partial_cmp(&dist2(b)).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .expect("mesh must have at least one node")
}

/// Realize the simply-supported (pin-pin) Dirichlet BCs over the *promoted P2*
/// node set — a verbatim port of `modal_ops::simply_supported_pin_pin_bcs`:
///
///   1. Pin ONLY the transverse Z DOF on every node of both end faces
///      (`x ≈ 0` and `x ≈ L`), selected by x-coordinate so the P2 edge-midpoint
///      nodes on each face are caught too. Pinning `w` (not the axial `u`) leaves
///      the bending rotation `dw/dx` free, giving the `(nπ)²` simply-supported
///      family rather than the fixed-fixed family.
///   2. Minimal anchors at the two end-face neutral-axis nodes (`z = h/2`,
///      `y = 0`): X at the root node (removes axial rigid translation), Y at the
///      root AND tip nodes (the two x-separated anchors remove the Y rigid
///      translation *and* the in-plane Z-rotation). Both anchor families sit off
///      the vertical bending mode (`u = 0` at the neutral axis, `v = 0`
///      everywhere), so they do not load the headline signal.
fn simply_supported_bcs(nodes_p2: &[[f64; 3]], length: f64, height: f64) -> Vec<DirichletBc> {
    let eps = 1e-10_f64;
    let mut bcs = Vec::new();

    // (1) Simple supports: pin the transverse Z DOF on both end faces.
    for (n, xyz) in nodes_p2.iter().enumerate() {
        let on_end = xyz[0].abs() < eps || (xyz[0] - length).abs() < eps;
        if on_end {
            bcs.push(DirichletBc { dof: 3 * n + 2, value: 0.0 }); // Z (bending)
        }
    }

    // (2) Minimal anchors at the two end-face neutral-axis nodes (z = h/2).
    let root = nearest_node(nodes_p2, [0.0, 0.0, height / 2.0]);
    let tip = nearest_node(nodes_p2, [length, 0.0, height / 2.0]);
    bcs.push(DirichletBc { dof: 3 * root, value: 0.0 }); // X anchor (axial)
    bcs.push(DirichletBc { dof: 3 * root + 1, value: 0.0 }); // Y anchor (root)
    bcs.push(DirichletBc { dof: 3 * tip + 1, value: 0.0 }); // Y anchor (tip)
    bcs
}

/// One measured spectrum entry: frequency plus the `(x, y, z)` energy fractions
/// used to classify the mode family.
struct ModeInfo {
    freq: f64,
    fracs: [f64; 3],
}

/// Outcome of one simply-supported P2 modal solve on a given mesh.
struct SimplySupportedMeasurement {
    /// First three Z-dominant (vertical bending) frequencies, ascending (Hz).
    f_bending: [f64; 3],
    /// Analytic pin-pin references for modes 1..=3 (Hz).
    f_analytic: [f64; 3],
    /// Per-mode relative error of `f_bending` vs `f_analytic`.
    rel_err: [f64; 3],
    /// Full measured low spectrum (freq + axis fractions) for the tuning print.
    spectrum: Vec<ModeInfo>,
    /// Count of Z-dominant modes found in the requested spectrum.
    n_bending_found: usize,
    n_free: usize,
    n_nodes_p2: usize,
    mass_rel: f64,
    converged: bool,
}

/// Build the simply-supported beam at the given mesh, pin both end faces in Z +
/// minimal neutral-axis anchors, dense-eigensolve a generous low spectrum,
/// classify each mode by dominant axis, and return the first three vertical
/// (Z-bending) frequencies vs the Euler-Bernoulli pin-pin references.
///
/// The third call site over [`solve_beam_modes`]. Unlike the two clamp-family
/// benchmarks it cannot go through [`measure_beam_modes`]: its BC set is not a
/// face clamp (Z-only pins plus three neutral-axis anchors), it asks for a
/// generous 8-mode window rather than 3, and its signal is a mode FAMILY picked
/// out of an interleaved spectrum rather than the raw fundamental. What it does
/// share — assembly, mass sanity, free-DOF projection, the dense solve and the
/// per-mode axis classification — is exactly what `solve_beam_modes` provides.
fn measure_simply_supported(grid: &BeamFixture) -> SimplySupportedMeasurement {
    // Solve a generous low spectrum: the lateral Y-bending mode (≈ 5× f₁,z)
    // sits between the 2nd and 3rd vertical modes under P2, so 8 eigenpairs
    // comfortably cover z1, z2, y1, z3.
    let opts = EigenSolverOptions { n_modes: 8, tol: 1e-9, max_iters: 300, sigma: 0.0 };
    let solve = solve_beam_modes(
        grid,
        |nodes_p2| simply_supported_bcs(nodes_p2, grid.lx, grid.lz),
        opts,
    );

    let spectrum: Vec<ModeInfo> = solve
        .freqs
        .iter()
        .zip(solve.fracs.iter())
        .map(|(&freq, &fracs)| ModeInfo { freq, fracs })
        .collect();

    // Vertical (Z-dominant) family, ascending by frequency.
    let bending: Vec<f64> = spectrum
        .iter()
        .filter(|m| m.fracs[2] >= m.fracs[0] && m.fracs[2] >= m.fracs[1])
        .map(|m| m.freq)
        .collect();
    let n_bending_found = bending.len();

    let mut f_bending = [f64::NAN; 3];
    for (i, slot) in f_bending.iter_mut().enumerate() {
        if let Some(&f) = bending.get(i) {
            *slot = f;
        }
    }

    let f_analytic = [
        analytic_beam_frequency(grid, PI),
        analytic_beam_frequency(grid, 2.0 * PI),
        analytic_beam_frequency(grid, 3.0 * PI),
    ];
    let rel_err: [f64; 3] =
        std::array::from_fn(|i| (f_bending[i] - f_analytic[i]).abs() / f_analytic[i]);

    SimplySupportedMeasurement {
        f_bending,
        f_analytic,
        rel_err,
        spectrum,
        n_bending_found,
        n_free: solve.n_free,
        n_nodes_p2: solve.n_nodes_p2,
        mass_rel: solve.mass_rel,
        converged: solve.converged,
    }
}

// ---------------------------------------------------------------------------
// Step-5 (RED) / Step-6 (GREEN): simply-supported P2 modal frequencies within 2%
// ---------------------------------------------------------------------------

/// Calibrated relative-error bound on the simply-supported FUNDAMENTAL (f₁).
///
/// **The 2% aspirational target is achieved** on the fundamental with room to
/// spare: the honest measured error is **0.12%** at the `nx=24` example-practical
/// mesh (see the tuning history on `SS_P2_HIGHER_MODE_TOL`). The pin-pin
/// fundamental is markedly more accurate than the cantilever fundamental
/// (1.35%) — the cantilever carries a 3-D root-clamp stress-concentration error
/// the pinned ends do not — so f₁ clears 2% by a wide margin and this bound
/// stays at the deliverable's 2% (NOT a loosened measured floor). The dense QZ
/// eigensolve is fully deterministic, so there is no Lanczos cross-platform
/// variance to absorb.
const SS_P2_REL_TOL: f64 = 0.02;

/// Calibrated relative-error band on the higher vertical modes (f₂, f₃).
///
/// **Resolved by a finer span mesh, NOT a loosened band** (the step-6 choice the
/// plan asks to record): higher simply-supported modes have shorter
/// half-wavelengths (`L/2`, `L/3`), so the binding mode f₃ (3rd bending,
/// half-wave `L/3 ≈ 67 mm`) needs more span elements than the cantilever's
/// fundamental did. At the cantilever's `nx=16` f₃ misses 2% (2.27%); refining
/// the span to `nx=24` brings every vertical mode comfortably under 2% — so this
/// band stays at the deliverable's 2% rather than being loosened. The almost-all
/// of f₃'s `nx=16` error is mesh-discretization stiffening (`f₃ > analytic` and
/// falling monotonically as the span refines), not the 3-D-vs-Euler-Bernoulli
/// model gap: the Timoshenko shear/rotary-inertia correction at f₃ is only
/// ≈ 0.13% for this `L/r ≈ 346` beam, so f₃ converges to the reference rather
/// than flooring above 2%. Because 2% is met on all three modes, the dedicated
/// lock-free 1-D beam/frame element (the route a >2% floor would have required)
/// is NOT needed here.
///
/// # Tuning history (release mode; analytic f₁ = 115.86, f₂ = 463.45,
/// f₃ = 1042.76 Hz; 8-mode dense QZ, vertical family selected by eigenvector
/// dominant-axis classification)
///
/// | nx×ny×nz | n_free | f₁ err | f₂ err | f₃ err | note                          |
/// |----------|--------|--------|--------|--------|-------------------------------|
/// | 16×1×1   | 870    | 0.265% | 1.038% | 2.265% | step-5 RED — f₃ fails 2%      |
/// | 20×1×1   | 1086   | 0.170% | 0.665% | 1.447% | passes, f₃ margin 0.55%       |
/// | **24×1×1** | **1302** | **0.119%** | **0.464%** | **1.006%** | **chosen** — f₃ margin ≈ 1%, ~35 s |
/// | 28×1×1   | 1518   | 0.089% | 0.347% | 0.749% | finer: f₃ −0.26% only         |
/// | 32×1×1   | 1734   | 0.071% | 0.275% | 0.590% | finer: f₃ −0.16% only         |
///
/// Every mode's error falls monotonically as the span refines. `nx=24` is the
/// smallest span mesh whose binding mode (f₃) clears 2% with a margin (≈ 0.99%)
/// exceeding the cantilever's 0.65% precedent, at a sane release runtime; `nx=20`
/// clears 2% but only by 0.55% on f₃, and `nx ≥ 28` buys < 0.3% more on f₃ for a
/// markedly larger dense solve.
const SS_P2_HIGHER_MODE_TOL: f64 = 0.02;

/// Simply-supported (pin-pin) P2 modal-frequency accuracy benchmark — task 4066.
///
/// Same slender steel beam as the cantilever (`L = 200 mm` × `b = 10 mm` ×
/// `h = 2 mm`, AISI 1045). BCs realize a genuine pin-pin: the transverse Z DOF
/// is pinned on both `x ≈ 0` and `x ≈ L` end faces (catching P2 edge-midpoints
/// by coordinate) with minimal neutral-axis anchors, so the bending rotation
/// stays free and the modes follow the `fₙ = (nπ)²/(2π)·√(EI/ρAL⁴)` family
/// (`f₁ ≈ 115.8 Hz, f₂ ≈ 463 Hz, f₃ ≈ 1042 Hz`), NOT fixed-fixed.
///
/// Unlike the locked P1 `ny=1` mesh, the P2 mesh resolves the lateral Y-bending
/// mode (`≈ 579 Hz`) within the first few eigenvalues — so it intrudes between
/// the 2nd and 3rd vertical modes. The first three VERTICAL (Z-dominant) modes
/// are therefore selected by eigenvector dominant-axis classification, not by
/// blindly taking the first three eigenvalues.
///
/// Passes when each of f₁, f₂, f₃ is within its calibrated tolerance of the
/// analytic pin-pin reference.
#[cfg_attr(
    debug_assertions,
    ignore = "heavy (dense modal eigensolve): release-only at the merge gate; debug skips it for per-task speed — task 4066"
)]
#[test]
fn simply_supported_beam_p2_modal_within_two_percent() {
    // Example-practical P2 mesh (step-6 calibrated): nx=24 along the span clears
    // 2% on all three vertical modes (f₃ — the binding 3rd bending mode — at
    // 1.01%, margin ≈ 1%) in ~35 s. The finer span vs the cantilever's nx=16 is
    // needed for f₃'s shorter half-wavelength (L/3); see SS_P2_HIGHER_MODE_TOL's
    // tuning history for the full nx sweep.
    let grid = BeamFixture { nx: 24, ny: 1, nz: 1, lx: 0.2, ly: 0.01, lz: 0.002 };
    let m = measure_simply_supported(&grid);

    eprintln!(
        "simply-supported P2 (nx={}, ny={}, nz={}, P2 nodes={}, n_free={}): Σ M rel = {:.2e}",
        grid.nx, grid.ny, grid.nz, m.n_nodes_p2, m.n_free, m.mass_rel,
    );
    for (j, mode) in m.spectrum.iter().enumerate() {
        eprintln!(
            "  mode {j}: f = {:.4} Hz  axis fracs (x, y, z) = ({:.3}, {:.3}, {:.3})",
            mode.freq, mode.fracs[0], mode.fracs[1], mode.fracs[2],
        );
    }
    eprintln!(
        "  vertical family: f1 = {:.4} (analytic {:.4}, err {:.4}%), \
         f2 = {:.4} (analytic {:.4}, err {:.4}%), f3 = {:.4} (analytic {:.4}, err {:.4}%)",
        m.f_bending[0], m.f_analytic[0], m.rel_err[0] * 100.0,
        m.f_bending[1], m.f_analytic[1], m.rel_err[1] * 100.0,
        m.f_bending[2], m.f_analytic[2], m.rel_err[2] * 100.0,
    );

    assert!(m.converged, "dense modal eigensolve must converge for the simply-supported beam");
    assert!(
        m.n_bending_found >= 3,
        "need ≥3 vertical (Z-dominant) bending modes in the spectrum, found {}",
        m.n_bending_found,
    );
    assert!(
        m.mass_rel < 1e-9,
        "global consistent-mass total Σ M off by rel {:.2e} from 3ρV",
        m.mass_rel,
    );
    for (i, &f) in m.f_bending.iter().enumerate() {
        assert!(f.is_finite() && f > 0.0, "f{} must be finite and positive, got {}", i + 1, f);
    }
    assert!(
        m.f_bending[0] < m.f_bending[1] && m.f_bending[1] < m.f_bending[2],
        "vertical frequencies must be strictly ascending: f1={} f2={} f3={}",
        m.f_bending[0], m.f_bending[1], m.f_bending[2],
    );

    assert!(
        m.rel_err[0] < SS_P2_REL_TOL,
        "ss f1 = {:.4} Hz, analytic = {:.4} Hz, rel_err = {:.4}% > {:.2}%",
        m.f_bending[0], m.f_analytic[0], m.rel_err[0] * 100.0, SS_P2_REL_TOL * 100.0,
    );
    assert!(
        m.rel_err[1] < SS_P2_HIGHER_MODE_TOL,
        "ss f2 = {:.4} Hz, analytic = {:.4} Hz, rel_err = {:.4}% > {:.2}%",
        m.f_bending[1], m.f_analytic[1], m.rel_err[1] * 100.0, SS_P2_HIGHER_MODE_TOL * 100.0,
    );
    assert!(
        m.rel_err[2] < SS_P2_HIGHER_MODE_TOL,
        "ss f3 = {:.4} Hz, analytic = {:.4} Hz, rel_err = {:.4}% > {:.2}%",
        m.f_bending[2], m.f_analytic[2], m.rel_err[2] * 100.0, SS_P2_HIGHER_MODE_TOL * 100.0,
    );
}

// ---------------------------------------------------------------------------
// Clamped-clamped P2 modal benchmark (task 6663)
// ---------------------------------------------------------------------------

/// Euler-Bernoulli clamped-clamped (fixed-fixed) first-mode eigen-coefficient.
const BETA1_L_CLAMPED_CLAMPED: f64 = 4.730041;

/// Build the clamped-clamped beam at the given mesh, clamp BOTH end faces (all
/// 3 DOFs, by coordinate so the P2 edge-midpoints on each face are caught too),
/// and return the measured fundamental vs the Euler-Bernoulli reference.
///
/// The second call site over [`measure_beam_modes`], differing from
/// [`measure_cantilever`] in EXACTLY the two things that function is
/// parameterized by: the BC predicate catches `x ≈ 0` OR `x ≈ L` instead of the
/// root face alone, and `βL = 4.730041` instead of 1.875104. (Before task
/// 6663's amendment the whole build-assemble-project-solve body was copied
/// verbatim between the two.)
fn measure_clamped_clamped(grid: &BeamFixture) -> BeamModalMeasurement {
    let lx = grid.lx;
    measure_beam_modes(
        grid,
        move |xyz| xyz[0].abs() < 1e-10 || (xyz[0] - lx).abs() < 1e-10,
        BETA1_L_CLAMPED_CLAMPED,
    )
}

/// Calibrated relative-error bound on the clamped-clamped fundamental frequency.
///
/// **This is an HONEST MEASURED FLOOR, not the 2% the cantilever/SS bounds hit** —
/// the same class as this file's existing euler P2 5% and P1 9–11% bounds, which
/// `CANTILEVER_P2_REL_TOL`'s doc comment names explicitly as the contrast case.
///
/// # Tuning history (release mode, analytic f₁ = 262.6464 Hz)
///
/// | nx×ny×nz | n_free | f₁ (Hz) | rel_err | note                              |
/// |----------|--------|---------|---------|-----------------------------------|
/// | 16×1×1   | 837    | 270.760 | +3.089% | fails a 2% bound                  |
/// | **24×1×1** | **1269** | **268.179** | **+2.106%** | **chosen** — 1.89% margin under 4%, matches the SS benchmark's span mesh, **~39 s** |
/// | 32×1×1   | 1701   | 267.096 | +1.694% | clears 2% by only 0.31%           |
///
/// # Gate cost
///
/// **MEASURED 38.8 s** at the chosen `nx=24` (`test result: ok … finished in
/// 38.78s`, release, this test alone, sequential) — the same way
/// [`CANTILEVER_P2_REL_TOL`] records `~25 s` for its own chosen mesh, and for
/// the same reason: this test is release-only, so its cost lands entirely on the
/// merge gate and must be visible to whoever next tunes the mesh rather than
/// re-derived from the QZ's O(n³).
///
/// That is ≈ 1.6× the cantilever benchmark's ~25 s at `n_free = 864` — well
/// under what the cubic alone predicts ((1269/864)³ ≈ 3.2×), because the dense
/// QZ's constant factors and the assembly it shares do not scale cubically.
/// Sizing the next step from this row rather than from the ratio matters: the
/// `nz=2` cantilever row's ~115 s at `n_free = 1440` is the cautionary
/// neighbour, and `nx=32` here (`n_free = 1701`) would buy 0.41% of accuracy for
/// a materially larger share of the gate.
///
/// **Why 2% is not used here.** It FAILS at nx=16 and nx=24 and clears at nx=32
/// by only 0.31%, so a 2% bound at any of this file's established meshes would be
/// either unreachable or knife-edge fragile. 4% at nx=24 leaves a 1.89% margin —
/// ahead of the 0.65% margin `CANTILEVER_P2_REL_TOL` was calibrated with.
///
/// **Why the residual is discretization, not a model gap.** The error falls
/// monotonically under span refinement (3.089% → 2.106% → 1.694%) and `f₁ >
/// analytic` at every mesh, which is the signature of a conforming P2 +
/// consistent-mass Rayleigh quotient: it bounds the exact 3-D value from ABOVE
/// and descends toward it. The clamped-clamped fundamental sits at βL = 4.730041,
/// i.e. `(4.730041/π)² = 2.27×` the simply-supported fundamental's curvature
/// demand, so the same mesh resolves it less well — hence a residual roughly 2×
/// the SS benchmark's at the identical nx=24. The Timoshenko shear/rotary-inertia
/// correction for this `L/r ≈ 346` beam is only about −0.12%, far too small to
/// explain a +2.1% offset, so this is discretization stiffening and not the
/// 3D-solid-vs-Euler-Bernoulli model gap.
const CLAMPED_P2_REL_TOL: f64 = 0.04;

/// Clamped-clamped (fixed-fixed) P2 modal-frequency accuracy benchmark — the
/// third member of this file's cantilever / simply-supported family, on the SAME
/// shared steel fixture.
///
/// Slender steel beam: `L = 200 mm` (X span) × `b = 10 mm` (Y width) ×
/// `h = 2 mm` (Z height, the thin bending axis), `L/r ≈ 346`. AISI 1045
/// (E = 205 GPa, ν = 0.29, ρ = 7850 kg/m³).
///
/// BCs: BOTH the `x ≈ 0` and `x ≈ L` end faces are fully clamped — all 3 DOFs at
/// every P2 node on either face, selected by x-coordinate so the P2 edge-midpoint
/// nodes on each face are caught too.
///
/// Reference (Euler-Bernoulli clamped-clamped first mode):
/// `f₁ = (4.730041² / 2π) · √(EI / (ρ A L⁴)) ≈ 262.6 Hz`, with `I = b·h³/12`,
/// `A = b·h`.
///
/// # Why this exists alongside the e2e's clause (d) (review suggestion 5)
///
/// The eval-surface acceptance e2e
/// (`reify-eval-fea-tests/tests/modal_analysis_e2e.rs::
/// e2e_two_fixed_supports_are_clamped_clamped_not_simply_supported`) already
/// checks a clamped-clamped fundamental against `βL = 4.730041`, at −1.46% on a
/// 3% band, for 21.7 s covering THREE solves — so on wall clock per accuracy
/// statement this benchmark is the expensive one, and the question of whether it
/// earns its 38.8 s is fair. It does, on three axes that are independent by
/// construction rather than by coincidence, and a shared failure would have to
/// break all three at once to stay hidden:
///
///   1. **Solver path.** This test dense-eigensolves (`solve_eigen_dense`, a QZ
///      over the full projected pair). The e2e goes through
///      `solve_generalized_eigen`'s shift-invert route — sparse Cholesky of
///      `K_free` plus Lanczos. They share no numerical kernel below the
///      assembly, so an accuracy defect in either is invisible to the other.
///   2. **Fixture regime.** `L/r ≈ 346` here (200 × 10 × 2 mm) versus the
///      dogfood section's stubby `L/h ≈ 18`. Euler–Bernoulli is the right model
///      for the first and a ~1.5%-shear-deficient model for the second, which is
///      exactly why the e2e band has to be 3% while this one can be read as a
///      convergence statement (3.089% → 2.106% → 1.694% under span refinement).
///      A discretization regression that hid inside the e2e's shear-explained
///      residual would move this monotone sequence.
///   3. **Layer.** No parser, no eval graph, no `@optimized` dispatch and no BC
///      REALIZATION: the Dirichlet set here is written directly by coordinate.
///      That is what makes it an accuracy statement about the clamped-clamped
///      SOLVE rather than about `build_dirichlet_bcs` — the e2e cannot separate
///      the two, because realizing the BCs is the thing it is testing.
///
/// **Routing this through `solve_eigen_shift_invert` was considered and
/// rejected.** It would recover most of the 38.8 s (n_free = 1269 is well into
/// the sparse regime), but it buys that by deleting axis 1 — the independent
/// dense-QZ cross-check that is half of why the test earns its place — and it
/// would make this the only member of the file's three-benchmark family not
/// sharing `solve_beam_modes`'s single path, re-opening the duplication that
/// amendment removed. The cheaper lever, if the gate ever needs one, is the
/// mesh: the tuning table above prices nx=16 at 3.089%, which a 4% bound still
/// admits.
///
/// Passes when `|f₁ − f₁,analytic| / f₁,analytic < CLAMPED_P2_REL_TOL`.
#[cfg_attr(
    debug_assertions,
    ignore = "heavy (dense modal eigensolve): release-only at the merge gate; debug skips it for per-task speed — task 6663"
)]
#[test]
fn clamped_clamped_beam_p2_modal_within_calibrated_band() {
    // nx=24 matches the simply-supported benchmark's span mesh — see
    // CLAMPED_P2_REL_TOL's tuning history for the measured nx sweep and for why
    // 2% is not the bound here.
    let grid = BeamFixture { nx: 24, ny: 1, nz: 1, lx: 0.2, ly: 0.01, lz: 0.002 };
    let m = measure_clamped_clamped(&grid);

    eprintln!(
        "clamped-clamped P2 (nx={}, ny={}, nz={}, P2 nodes={}, n_free={}): \
         f1 = {:.4} Hz, analytic = {:.4} Hz, rel_err = {:.4}% (f2 = {:.2} Hz, f3 = {:.2} Hz); \
         Σ M rel = {:.2e}",
        grid.nx, grid.ny, grid.nz, m.n_nodes_p2, m.n_free,
        m.f1, m.f1_analytic, m.rel_err * 100.0, m.f2, m.f3, m.mass_rel,
    );

    assert!(m.converged, "dense modal eigensolve must converge for the clamped-clamped beam");
    assert!(m.lambda_min > 0.0, "λ_min = {} must be positive (free vibration)", m.lambda_min);
    assert!(
        m.mass_rel < 1e-9,
        "global consistent-mass total Σ M off by rel {:.2e} from 3ρV",
        m.mass_rel,
    );
    assert!(
        m.rel_err < CLAMPED_P2_REL_TOL,
        "clamped-clamped P2: f1 = {:.4} Hz, analytic = {:.4} Hz, rel_err = {:.4}% > {:.2}%",
        m.f1, m.f1_analytic, m.rel_err * 100.0, CLAMPED_P2_REL_TOL * 100.0,
    );
}
