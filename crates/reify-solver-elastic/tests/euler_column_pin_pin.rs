//! Integration tests for the full Euler-column buckling pipeline (PRD §13 task δ).
//!
//! # PRD reference
//!
//! `docs/prds/v0_5/buckling-eigensolver.md` §13 task δ observable signal +
//! §9.1 boundary-test rows for pinned-pinned, fixed-free, and fixed-fixed columns.
//!
//! All three BC variants live in this file per the PRD §13 wording:
//! "Same test file also covers fixed-free and fixed-fixed BC variants."
//!
//! # Geometry & material
//!
//! Steel AISI 1045 square box column, 20 × 20 × 800 mm (SI units throughout).
//! - E = 205 GPa, ν = 0.3.
//! - 1 kN total axial compressive load, split uniformly across top-face nodes.
//!
//! # Tolerance
//!
//! 5% relative: `|λ · F − P_cr| / P_cr < 0.05`, tighter than the γ-task's 10%
//! because we exercise the full `solve_buckling_kernel` pipeline (linear-static
//! pre-stress → K_g assembly → eigensolve) rather than a direct K_g injection.
//!
//! # Mesh
//!
//! Initial density: `nx=ny=4, nz=40` bricks → 5×5×41 = 1025 nodes, 3075 DOFs.
//! Tuned in step-6 / step-8 / step-10 against the 5% bound if the initial density
//! is insufficient. P1-tet bending lock on slender columns (L/r ≈ 138 for this
//! geometry) may require finer axial refinement.
//!
//! # Mesh-building scaffolding
//!
//! Duplicated from `tests/kg_p1_tet.rs` because integration tests compile as
//! separate binaries and cannot share Rust modules between test files. Acceptable
//! duplication for v0.5; a future refactor can consolidate via `tests/common/`.

use std::f64::consts::PI;

use reify_solver_elastic::{
    BucklingKernelOptions, DirichletBc, IsotropicElastic, apply_point_load, solve_buckling_kernel,
};

// ---------------------------------------------------------------------------
// Material constants — Steel AISI 1045 (SI)
// ---------------------------------------------------------------------------

const STEEL_E_PA: f64 = 205.0e9;
const STEEL_NU: f64 = 0.3;
/// Total axial compressive load applied to the column (Newtons).
const APPLIED_LOAD_NEWTONS: f64 = 1000.0; // 1 kN

// ---------------------------------------------------------------------------
// Mesh scaffolding (adapted from `tests/kg_p1_tet.rs::ColumnGrid`)
// ---------------------------------------------------------------------------

/// Brick-grid dimensions for the Euler-column fixture.
struct ColumnFixture {
    nx: usize,
    ny: usize,
    nz: usize,
    /// Physical half-extents: column spans `[0, lx] × [0, ly] × [0, lz]`.
    lx: f64,
    ly: f64,
    lz: f64,
}

impl ColumnFixture {
    /// Steel AISI 1045 box column, 20 × 20 × 800 mm.
    ///
    /// Tuned 2026-05-26 against the 5% PRD §13 task δ bound.
    /// Starting mesh `nx=ny=4, nz=40` gave 16.77% error at nz=160 (P1-tet
    /// bending locking dominates; error ∝ C/N² where N = cross-section elements).
    /// Cross-section refinement to `nx=ny=8` reduces the error by ≈4× (to ~4.2%).
    fn steel_aisi_1045_800mm() -> Self {
        Self { nx: 8, ny: 8, nz: 160, lx: 0.02, ly: 0.02, lz: 0.8 }
    }

    fn n_nodes(&self) -> usize {
        (self.nx + 1) * (self.ny + 1) * (self.nz + 1)
    }

    /// Row-major `(k, j, i)` node linearisation (same as `kg_p1_tet.rs`).
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

    /// Minimum second moment of area (square section, both axes equivalent).
    fn i_min(&self) -> f64 {
        self.lx * self.ly.powi(3) / 12.0
    }
}

/// Six-tet long-diagonal brick decomposition (verbatim from `kg_p1_tet.rs`).
const TET_DECOMPOSITION: [[usize; 4]; 6] = [
    [0, 1, 2, 6],
    [0, 2, 3, 6],
    [0, 3, 7, 6],
    [0, 7, 4, 6],
    [0, 4, 5, 6],
    [0, 5, 1, 6],
];

fn build_node_xyz(grid: &ColumnFixture) -> Vec<[f64; 3]> {
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

fn build_tet_mesh(grid: &ColumnFixture) -> Vec<[usize; 4]> {
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
// Step-5 (RED): Pin-pin Euler column within 5%
// ---------------------------------------------------------------------------

/// Pin-pin Euler column integration test — PRD §13 task δ canonical signal.
///
/// BCs: lateral u_x = u_y = 0 at both end faces (classical pin-pin: ends can
/// rotate about transverse axes but cannot translate laterally); plus ONE axial
/// anchor at the bottom corner to prevent rigid-body z-translation (which would
/// leave the pre-stress CG system singular).
///
/// Analytical critical load: `P_cr = π²·E·I / L² ≈ 42.15 kN` (k=1, pin-pin).
/// Test passes when `|λ·F − P_cr| / P_cr < 5%`.
#[test]
fn pin_pin_euler_column_within_five_percent() {
    let grid = ColumnFixture::steel_aisi_1045_800mm();
    let nodes = build_node_xyz(&grid);
    let tets = build_tet_mesh(&grid);

    let material = IsotropicElastic { youngs_modulus: STEEL_E_PA, poisson_ratio: STEEL_NU };

    // BCs: pin-pin — lateral clamp at both end faces + one axial anchor.
    let mut bcs: Vec<DirichletBc> = Vec::new();
    for k_face in [0usize, grid.nz] {
        for j in 0..=grid.ny {
            for i in 0..=grid.nx {
                let n = grid.node_id(i, j, k_face);
                bcs.push(DirichletBc { dof: 3 * n,     value: 0.0 }); // u_x
                bcs.push(DirichletBc { dof: 3 * n + 1, value: 0.0 }); // u_y
            }
        }
    }
    // Axial anchor at one bottom corner to anchor rigid-body z-translation.
    let anchor = grid.node_id(0, 0, 0);
    bcs.push(DirichletBc { dof: 3 * anchor + 2, value: 0.0 }); // u_z

    // Load: 1 kN distributed uniformly across top-face nodes.
    let n_top_nodes = (grid.nx + 1) * (grid.ny + 1);
    let mut f = vec![0.0_f64; 3 * nodes.len()];
    for j in 0..=grid.ny {
        for i in 0..=grid.nx {
            let n = grid.node_id(i, j, grid.nz);
            apply_point_load(&mut f, n, [0.0, 0.0, -APPLIED_LOAD_NEWTONS / n_top_nodes as f64]);
        }
    }

    let opts = BucklingKernelOptions {
        n_modes: 1,
        eigen_tol: 1e-8,
        eigen_max_iters: 100,
        cg_tolerance: 1e-10,
        cg_max_iter: 10_000,
    };

    let result = solve_buckling_kernel(&nodes, &tets, &material, &bcs, &f, opts);

    assert!(result.converged, "eigensolve must converge for pin-pin column");
    assert!(!result.modes.is_empty(), "must return at least 1 mode");

    let lambda_min = result.modes[0].eigenvalue;
    assert!(
        lambda_min > 0.0,
        "λ_min = {lambda_min} must be positive for compressive load",
    );

    // Analytical pin-pin Euler critical load (k=1): P_cr = π²·E·I / L².
    let i_min = grid.i_min();
    let p_cr = PI.powi(2) * STEEL_E_PA * i_min / (grid.lz * grid.lz);

    let lambda_x_load = lambda_min * APPLIED_LOAD_NEWTONS;
    let rel_err = (lambda_x_load - p_cr).abs() / p_cr;
    eprintln!("pin-pin: λ·F = {lambda_x_load:.2} N, P_cr = {p_cr:.2} N, rel_err = {:.2}%", rel_err * 100.0);
    assert!(
        rel_err < 0.05,
        "pin-pin Euler: λ·F = {lambda_x_load:.2} N, P_cr = {p_cr:.2} N, \
         rel_err = {:.2}% > 5%",
        rel_err * 100.0,
    );
}

// ---------------------------------------------------------------------------
// Step-7 (RED): Fixed-free (cantilever) Euler column within 5%
// ---------------------------------------------------------------------------

/// Fixed-free (cantilever) Euler column integration test — PRD §9.1 / §13 task δ.
///
/// In P1-tet meshes a "fixed" BC is realized by clamping all 3 displacement
/// DOFs at every node of the constrained face; without rotational DOFs, this is
/// the closest equivalent to the classical clamped/fixed boundary condition.
///
/// BCs: bottom face (k=0): all 3 DOFs clamped per node. Top face: completely free.
/// The bottom fully constrains axial translation, so no separate axial anchor
/// is needed.
///
/// Analytical critical load: `P_cr = π²·E·I / (2L)² ≈ 10.54 kN` (k=2, fixed-free).
/// Test passes when `|λ·F − P_cr| / P_cr < 5%`.
#[test]
fn fixed_free_euler_column_within_five_percent() {
    let grid = ColumnFixture::steel_aisi_1045_800mm();
    let nodes = build_node_xyz(&grid);
    let tets = build_tet_mesh(&grid);

    let material = IsotropicElastic { youngs_modulus: STEEL_E_PA, poisson_ratio: STEEL_NU };

    // BCs: fixed-free — bottom face fully clamped; top face free.
    let mut bcs: Vec<DirichletBc> = Vec::new();
    for j in 0..=grid.ny {
        for i in 0..=grid.nx {
            let n = grid.node_id(i, j, 0);
            bcs.push(DirichletBc { dof: 3 * n,     value: 0.0 }); // u_x
            bcs.push(DirichletBc { dof: 3 * n + 1, value: 0.0 }); // u_y
            bcs.push(DirichletBc { dof: 3 * n + 2, value: 0.0 }); // u_z
        }
    }

    // Load: 1 kN distributed uniformly across top-face nodes.
    let n_top_nodes = (grid.nx + 1) * (grid.ny + 1);
    let mut f = vec![0.0_f64; 3 * nodes.len()];
    for j in 0..=grid.ny {
        for i in 0..=grid.nx {
            let n = grid.node_id(i, j, grid.nz);
            apply_point_load(&mut f, n, [0.0, 0.0, -APPLIED_LOAD_NEWTONS / n_top_nodes as f64]);
        }
    }

    let opts = BucklingKernelOptions {
        n_modes: 1,
        eigen_tol: 1e-8,
        eigen_max_iters: 100,
        cg_tolerance: 1e-10,
        cg_max_iter: 10_000,
    };

    let result = solve_buckling_kernel(&nodes, &tets, &material, &bcs, &f, opts);

    assert!(result.converged, "eigensolve must converge for fixed-free column");
    assert!(!result.modes.is_empty(), "must return at least 1 mode");

    let lambda_min = result.modes[0].eigenvalue;
    assert!(
        lambda_min > 0.0,
        "λ_min = {lambda_min} must be positive for compressive load",
    );

    // Analytical fixed-free Euler critical load (k=2): P_cr = π²·E·I / (2L)².
    let i_min = grid.i_min();
    let p_cr = PI.powi(2) * STEEL_E_PA * i_min / (2.0 * grid.lz).powi(2);

    let lambda_x_load = lambda_min * APPLIED_LOAD_NEWTONS;
    let rel_err = (lambda_x_load - p_cr).abs() / p_cr;
    eprintln!("fixed-free: λ·F = {lambda_x_load:.2} N, P_cr = {p_cr:.2} N, rel_err = {:.2}%", rel_err * 100.0);
    assert!(
        rel_err < 0.05,
        "fixed-free Euler: λ·F = {lambda_x_load:.2} N, P_cr = {p_cr:.2} N, \
         rel_err = {:.2}% > 5%",
        rel_err * 100.0,
    );
}

// ---------------------------------------------------------------------------
// Step-9 (RED): Fixed-fixed (fixed-guided) Euler column within 5%
// ---------------------------------------------------------------------------

/// Fixed-fixed (fixed-guided) Euler column — PRD §9.1 / §13 task δ.
///
/// **Why "fixed-guided" not strictly "fixed-fixed"**: classical Euler
/// "fixed-fixed" clamps both ends against translation AND rotation. In a P1-tet
/// mesh, fully clamping both end faces (all 3 DOFs at every node) leaves the
/// column with no axial DOF to slide under the applied compressive load — the
/// pre-stress linear-static system would have a degenerate (zero) response.
///
/// The standard FEA workaround is "fixed-guided": bottom fully clamped (u_x=u_y=u_z=0
/// per node) and top laterally clamped (u_x=u_y=0 per node) but axially free
/// (u_z unclamped). This gives the same Euler effective-length factor k=0.5 and
/// the same critical load `P_cr = 4·π²·E·I / L²`. The PRD §13 task δ signal uses
/// "fixed-fixed" loosely; the FEA semantics are fixed-guided. No separate axial
/// anchor is needed: the bottom fully constrains axial translation already.
///
/// Analytical critical load: `P_cr = 4·π²·E·I / L² ≈ 168.6 kN` (k=0.5).
/// Test passes when `|λ·F − P_cr| / P_cr < 5%`.
#[test]
fn fixed_fixed_euler_column_within_five_percent() {
    let grid = ColumnFixture::steel_aisi_1045_800mm();
    let nodes = build_node_xyz(&grid);
    let tets = build_tet_mesh(&grid);

    let material = IsotropicElastic { youngs_modulus: STEEL_E_PA, poisson_ratio: STEEL_NU };

    // BCs: fixed-guided — bottom face fully clamped, top face laterally clamped.
    let mut bcs: Vec<DirichletBc> = Vec::new();
    // Bottom face: all 3 DOFs clamped.
    for j in 0..=grid.ny {
        for i in 0..=grid.nx {
            let n = grid.node_id(i, j, 0);
            bcs.push(DirichletBc { dof: 3 * n,     value: 0.0 }); // u_x
            bcs.push(DirichletBc { dof: 3 * n + 1, value: 0.0 }); // u_y
            bcs.push(DirichletBc { dof: 3 * n + 2, value: 0.0 }); // u_z
        }
    }
    // Top face: lateral clamp only (u_z free, so top can slide axially under load).
    for j in 0..=grid.ny {
        for i in 0..=grid.nx {
            let n = grid.node_id(i, j, grid.nz);
            bcs.push(DirichletBc { dof: 3 * n,     value: 0.0 }); // u_x
            bcs.push(DirichletBc { dof: 3 * n + 1, value: 0.0 }); // u_y
        }
    }

    // Load: 1 kN distributed uniformly across top-face nodes.
    let n_top_nodes = (grid.nx + 1) * (grid.ny + 1);
    let mut f = vec![0.0_f64; 3 * nodes.len()];
    for j in 0..=grid.ny {
        for i in 0..=grid.nx {
            let n = grid.node_id(i, j, grid.nz);
            apply_point_load(&mut f, n, [0.0, 0.0, -APPLIED_LOAD_NEWTONS / n_top_nodes as f64]);
        }
    }

    let opts = BucklingKernelOptions {
        n_modes: 1,
        eigen_tol: 1e-8,
        eigen_max_iters: 100,
        cg_tolerance: 1e-10,
        cg_max_iter: 10_000,
    };

    let result = solve_buckling_kernel(&nodes, &tets, &material, &bcs, &f, opts);

    assert!(result.converged, "eigensolve must converge for fixed-guided column");
    assert!(!result.modes.is_empty(), "must return at least 1 mode");

    let lambda_min = result.modes[0].eigenvalue;
    assert!(
        lambda_min > 0.0,
        "λ_min = {lambda_min} must be positive for compressive load",
    );

    // Analytical fixed-fixed Euler critical load (k=0.5): P_cr = 4·π²·E·I / L².
    let i_min = grid.i_min();
    let p_cr = PI.powi(2) * STEEL_E_PA * i_min / (0.5 * grid.lz).powi(2);

    let lambda_x_load = lambda_min * APPLIED_LOAD_NEWTONS;
    let rel_err = (lambda_x_load - p_cr).abs() / p_cr;
    eprintln!("fixed-fixed: λ·F = {lambda_x_load:.2} N, P_cr = {p_cr:.2} N, rel_err = {:.2}%", rel_err * 100.0);
    assert!(
        rel_err < 0.05,
        "fixed-fixed Euler: λ·F = {lambda_x_load:.2} N, P_cr = {p_cr:.2} N, \
         rel_err = {:.2}% > 5%",
        rel_err * 100.0,
    );
}
