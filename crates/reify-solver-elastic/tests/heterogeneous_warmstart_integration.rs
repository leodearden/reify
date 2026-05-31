//! Integration gate ε/3781 — rows 1+2 (solver level).
//!
//! Row 1: Heterogeneous two-zone solve — deflection between homogeneous
//!        bounds + stress concentrates in the stiff zone.
//!
//! Row 2: Warm-start across field refinement drops CG iterations and
//!        matches the cold solution within tolerance.
//!
//! Both tests exercise the foundation seam at the reify-solver-elastic level
//! via `element_stiffness_p1_with_field` + `DiscreteCellField`/`ConstantField`
//! + `solve_cg`/`solve_cg_warm` — the solver path proven by β/3778 and the
//!   v0.3 warm-start CG.
//!
//! Design decisions (ε/3781):
//! - Two-zone fixture: parallel (shared-strain) cross-section split in Z,
//!   axial +x tip load, clamped root face. Compliance monotonicity and stress
//!   concentration are mathematical guarantees for this geometry (Loewner
//!   order). See plan design_decisions #3+4.
//! - Assertions are relational/relative (bounds, inequalities, 1e-9 tol);
//!   no absolute magnitudes — P1-tet bending-lock doesn't affect axial loading.
//!
//! Expected GREEN on write against shipped β/3778+δ/3780+v0.3 warm-start code.
//! A RED outcome signals a regression in the owning crate; escalate, don't patch.

use reify_solver_elastic::{
    AnisotropicMaterial, AssemblyElement, AssemblyMode, CgResult, CgSolverOptions, ConstantField,
    DirichletBc, DiscreteCellField, IsotropicElastic, MaterialField, SolverMode,
    apply_dirichlet_row_elimination, apply_point_load, assemble_global_stiffness,
    element_stiffness_p1_with_field, element_stress_p1, solve_cg_warm,
};

// ─── Identity material frame ──────────────────────────────────────────────────
const IDENTITY_3X3: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

// ─────────────────────────────────────────────────────────────────────────────
// Shared helpers: mesh, loads, BCs, solve
// ─────────────────────────────────────────────────────────────────────────────

/// Freudenthal hex→6-tet structured bar mesh.
///
/// Returns `(coords, tet_conn)`:
/// - `coords[n]` = `[x, y, z]` for node n
/// - `tet_conn[e]` = `[n0, n1, n2, n3]` for tet e (positive-Jacobian ordering)
///
/// Bar axes: X = length (beam axis), Y = width, Z = height (bending direction).
fn build_bar_mesh(
    nx: usize,
    ny: usize,
    nz: usize,
    length: f64,
    width: f64,
    height: f64,
) -> (Vec<[f64; 3]>, Vec<[usize; 4]>) {
    let nx1 = nx + 1;
    let ny1 = ny + 1;
    let nz1 = nz + 1;
    let n_nodes = nx1 * ny1 * nz1;

    let node_idx = |ix: usize, iy: usize, iz: usize| -> usize {
        iz * ny1 * nx1 + iy * nx1 + ix
    };

    let mut coords = vec![[0.0f64; 3]; n_nodes];
    for iz in 0..nz1 {
        for iy in 0..ny1 {
            for ix in 0..nx1 {
                coords[node_idx(ix, iy, iz)] = [
                    ix as f64 * length / nx as f64,
                    iy as f64 * width / ny as f64,
                    iz as f64 * height / nz as f64,
                ];
            }
        }
    }

    let mut tet_conn: Vec<[usize; 4]> = Vec::new();
    for hz in 0..nz {
        for hy in 0..ny {
            for hx in 0..nx {
                let c = [
                    node_idx(hx,     hy,     hz    ),  // c[0]
                    node_idx(hx + 1, hy,     hz    ),  // c[1]
                    node_idx(hx + 1, hy + 1, hz    ),  // c[2]
                    node_idx(hx,     hy + 1, hz    ),  // c[3]
                    node_idx(hx,     hy,     hz + 1),  // c[4]
                    node_idx(hx + 1, hy,     hz + 1),  // c[5]
                    node_idx(hx + 1, hy + 1, hz + 1),  // c[6]
                    node_idx(hx,     hy + 1, hz + 1),  // c[7]
                ];
                // Freudenthal 6-tet decomposition sharing main body diagonal c[0]→c[6].
                // Each tet has a positive Jacobian det (+dx·dy·dz).
                let tets: [[usize; 4]; 6] = [
                    [c[0], c[1], c[2], c[6]],  // T0
                    [c[0], c[2], c[3], c[6]],  // T1
                    [c[0], c[5], c[1], c[6]],  // T2
                    [c[0], c[3], c[7], c[6]],  // T3
                    [c[0], c[4], c[5], c[6]],  // T4
                    [c[0], c[7], c[4], c[6]],  // T5
                ];
                for conn in tets {
                    tet_conn.push(conn);
                }
            }
        }
    }

    (coords, tet_conn)
}

/// Build an axial (+x) tip load vector distributed equally over tip-face nodes (ix=nx).
fn build_axial_tip_load(
    n_nodes: usize,
    nx: usize,
    ny: usize,
    nz: usize,
    total_force: f64,
) -> Vec<f64> {
    let nx1 = nx + 1;
    let ny1 = ny + 1;
    let nz1 = nz + 1;
    let node_idx = |ix: usize, iy: usize, iz: usize| -> usize {
        iz * ny1 * nx1 + iy * nx1 + ix
    };

    let tip_nodes: Vec<usize> = (0..nz1)
        .flat_map(|iz| (0..ny1).map(move |iy| node_idx(nx, iy, iz)))
        .collect();

    let mut f = vec![0.0f64; 3 * n_nodes];
    let force_per = total_force / tip_nodes.len() as f64;
    for &tn in &tip_nodes {
        apply_point_load(&mut f, tn, [force_per, 0.0, 0.0]);  // +x direction
    }
    f
}

/// Build Dirichlet BCs clamping all three DOFs on the root face (ix=0).
fn build_root_clamp_bcs(nx: usize, ny: usize, nz: usize) -> Vec<DirichletBc> {
    let nx1 = nx + 1;
    let ny1 = ny + 1;
    let nz1 = nz + 1;
    let node_idx = |iy: usize, iz: usize| -> usize {
        iz * ny1 * nx1 + iy * nx1  // ix = 0
    };

    let mut bcs = Vec::new();
    for iz in 0..nz1 {
        for iy in 0..ny1 {
            let rn = node_idx(iy, iz);
            for axis in 0..3usize {
                bcs.push(DirichletBc { dof: 3 * rn + axis, value: 0.0 });
            }
        }
    }
    bcs
}

/// Assemble global K with the given material field, apply BCs, and solve.
///
/// - `initial_guess`: `None` for a cold start, `Some(&u0)` for warm start.
///   The vector must have the same length as `f_load` (3 × n_nodes).
///
/// Returns the `CgResult` (u, iterations, converged).
fn assemble_and_solve<F: MaterialField>(
    n_nodes: usize,
    coords: &[[f64; 3]],
    tet_conn: &[[usize; 4]],
    field: &F,
    f_load: &[f64],
    bcs: &[DirichletBc],
    initial_guess: Option<&[f64]>,
) -> CgResult {
    // Build per-element stiffness matrices.
    let elem_stiffness: Vec<_> = tet_conn
        .iter()
        .map(|conn| {
            let phys4: [[f64; 3]; 4] = [
                coords[conn[0]],
                coords[conn[1]],
                coords[conn[2]],
                coords[conn[3]],
            ];
            element_stiffness_p1_with_field(&phys4, field)
        })
        .collect();

    let assembly_elements: Vec<AssemblyElement<'_>> = tet_conn
        .iter()
        .zip(elem_stiffness.iter())
        .enumerate()
        .map(|(id, (conn, k_e))| AssemblyElement {
            id,
            connectivity: conn.as_slice(),
            k_e,
        })
        .collect();

    let mut k = assemble_global_stiffness(n_nodes, &assembly_elements, AssemblyMode::Deterministic);
    let mut f = f_load.to_vec();
    apply_dirichlet_row_elimination(&mut k, &mut f, bcs);

    let opts = CgSolverOptions { tolerance: 1e-10, max_iter: 2000 };
    solve_cg_warm(&k, &f, initial_guess, opts, SolverMode::Deterministic)
}

/// Compute von Mises stress from a 3×3 symmetric Cauchy stress tensor.
///
/// Voigt formula: √(½·[(σxx−σyy)²+(σyy−σzz)²+(σzz−σxx)²+6·(σxy²+σyz²+σzx²)])
fn von_mises_from_sigma(sigma: [[f64; 3]; 3]) -> f64 {
    let (sxx, syy, szz) = (sigma[0][0], sigma[1][1], sigma[2][2]);
    let (sxy, syz, szx) = (sigma[0][1], sigma[1][2], sigma[0][2]);
    f64::sqrt(
        0.5 * ((sxx - syy).powi(2) + (syy - szz).powi(2) + (szz - sxx).powi(2)
            + 6.0 * (sxy * sxy + syz * syz + szx * szx)),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Row 1 — Heterogeneous two-zone solve
// ─────────────────────────────────────────────────────────────────────────────

/// PRD ε/3781 row 1 primary signal.
///
/// # Fixture
///
/// Bar (10×1×2 hexes / 120 P1 tets) along x, clamped at x=0, axial tip
/// load P=1 kN in +x. Two zones split at z=h/2 (parallel/shared-strain):
/// - z < h/2: soft zone  (E=40 GPa,  ν=0.3)
/// - z ≥ h/2: stiff zone (E=200 GPa, ν=0.3) — 5× stiffness contrast
///
/// # Assertions
///
/// (a) DEFLECTION-BETWEEN-BOUNDS: compliance C=fᵀu satisfies
///     C_allstiff < C_twozone < C_allsoft
///     (Loewner-monotone: K_soft⪯K_mixed⪯K_stiff ⇒ C_stiff≤C_mixed≤C_soft)
///
/// (b) STRESS-CONCENTRATION: max von Mises in the stiff zone > max von
///     Mises in the soft zone (σ=D·ε, same ε across cross-section ⇒ σ∝E)
#[test]
fn two_zone_heterogeneous_solve_deflection_between_bounds_and_stress_concentrates_in_stiff_zone() {
    let (nx, ny, nz) = (10, 1, 2);
    let (length, width, height) = (1.0_f64, 0.1_f64, 0.1_f64);

    // Materials: 5× stiffness contrast, same Poisson's ratio.
    let iso_stiff = IsotropicElastic { youngs_modulus: 200e9, poisson_ratio: 0.3 };
    let iso_soft  = IsotropicElastic { youngs_modulus:  40e9, poisson_ratio: 0.3 };
    let mat_stiff = AnisotropicMaterial::from_law(&iso_stiff, IDENTITY_3X3);
    let mat_soft  = AnisotropicMaterial::from_law(&iso_soft,  IDENTITY_3X3);

    // Build mesh once; all three solves use the same mesh, loads, and BCs.
    let (coords, tet_conn) = build_bar_mesh(nx, ny, nz, length, width, height);
    let n_nodes = (nx + 1) * (ny + 1) * (nz + 1);
    let f_load   = build_axial_tip_load(n_nodes, nx, ny, nz, 1000.0_f64);
    let bcs      = build_root_clamp_bcs(nx, ny, nz);

    // ── Three solves ──────────────────────────────────────────────────────────
    let field_stiff = ConstantField { material: mat_stiff };
    let r_stiff = assemble_and_solve(
        n_nodes, &coords, &tet_conn, &field_stiff, &f_load, &bcs, None,
    );
    assert!(r_stiff.converged, "all-stiff solve did not converge ({} iters)", r_stiff.iterations);

    let field_soft = ConstantField { material: mat_soft };
    let r_soft = assemble_and_solve(
        n_nodes, &coords, &tet_conn, &field_soft, &f_load, &bcs, None,
    );
    assert!(r_soft.converged, "all-soft solve did not converge ({} iters)", r_soft.iterations);

    let z_split = height / 2.0;
    let field_two = DiscreteCellField {
        cells: vec![mat_soft, mat_stiff],  // cell 0 = soft, cell 1 = stiff
        locator: Box::new(move |p: [f64; 3]| {
            if p[2] < z_split { Some(0) } else { Some(1) }
        }),
    };
    let r_two = assemble_and_solve(
        n_nodes, &coords, &tet_conn, &field_two, &f_load, &bcs, None,
    );
    assert!(r_two.converged, "two-zone solve did not converge ({} iters)", r_two.iterations);

    // ── (a) Deflection between bounds ─────────────────────────────────────────
    // Compliance C = fᵀu (Loewner-monotone in element stiffness).
    let compliance = |u: &[f64]| -> f64 {
        f_load.iter().zip(u.iter()).map(|(fi, ui)| fi * ui).sum()
    };
    let c_stiff = compliance(&r_stiff.u);
    let c_two   = compliance(&r_two.u);
    let c_soft  = compliance(&r_soft.u);

    assert!(
        c_stiff < c_two,
        "compliance: C_allstiff={c_stiff:.4e} must be < C_twozone={c_two:.4e} \
         (Loewner monotonicity: adding soft zone elements increases compliance)",
    );
    assert!(
        c_two < c_soft,
        "compliance: C_twozone={c_two:.4e} must be < C_allsoft={c_soft:.4e} \
         (Loewner monotonicity: replacing remaining soft elements with stiff reduces compliance)",
    );

    // ── (b) Stress concentration ──────────────────────────────────────────────
    // For the two-zone solve, recover per-element von Mises via element_stress_p1.
    // Zone identified by element centroid z vs z_split.
    let mut max_stiff_vm = 0.0_f64;
    let mut max_soft_vm  = 0.0_f64;

    for conn in &tet_conn {
        let phys4: [[f64; 3]; 4] = [
            coords[conn[0]], coords[conn[1]], coords[conn[2]], coords[conn[3]],
        ];
        let u_e: [f64; 12] = [
            r_two.u[3 * conn[0]],     r_two.u[3 * conn[0] + 1], r_two.u[3 * conn[0] + 2],
            r_two.u[3 * conn[1]],     r_two.u[3 * conn[1] + 1], r_two.u[3 * conn[1] + 2],
            r_two.u[3 * conn[2]],     r_two.u[3 * conn[2] + 1], r_two.u[3 * conn[2] + 2],
            r_two.u[3 * conn[3]],     r_two.u[3 * conn[3] + 1], r_two.u[3 * conn[3] + 2],
        ];

        // Identify zone by centroid z.
        let cz = (phys4[0][2] + phys4[1][2] + phys4[2][2] + phys4[3][2]) / 4.0;
        let (iso_zone, is_stiff) = if cz < z_split {
            (&iso_soft, false)
        } else {
            (&iso_stiff, true)
        };

        let sigma = element_stress_p1(&phys4, iso_zone, &u_e);
        let vm = von_mises_from_sigma(sigma);

        if is_stiff {
            max_stiff_vm = max_stiff_vm.max(vm);
        } else {
            max_soft_vm = max_soft_vm.max(vm);
        }
    }

    assert!(
        max_stiff_vm > max_soft_vm,
        "max von Mises in stiff zone ({max_stiff_vm:.3e} Pa) must exceed soft zone \
         ({max_soft_vm:.3e} Pa); same axial ε across cross-section ⇒ σ=E·ε higher in stiff zone",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Row 2 — Warm-start across field refinement
// ─────────────────────────────────────────────────────────────────────────────

/// PRD ε/3781 row 2 primary signal.
///
/// # Fixture
///
/// Same 10×1×2 bar mesh, BCs, and axial load as row 1.
///
/// Field v1: constant field (E=200 GPa everywhere).
/// Field v2: v1 with a small R0→R1 refinement — interior strip
///   x ∈ [0.4L, 0.6L) nudged to E=204 GPa (+2%).
///   Keeps u1≈u2 so warm-start from u1 is effective.
///
/// # Procedure
///
/// 1. Cold-solve K1·u=f → u1 (warm-start seed).
/// 2. Cold-solve K2·u=f from zero → iters_cold, u_cold.
/// 3. Warm-solve K2·u=f from u1 → iters_warm, u_warm.
///
/// # Assertions
///
/// (a) ITERATION-DROP: iters_warm < iters_cold (strict inequality).
/// (b) RESULT-WITHIN-TOL: |u_warm[i]−u_cold[i]| < 1e-9·max(1, |u_cold[i]|).
///
/// Directly mirrors `solver::tests::warm_start_with_perturbed_rhs_reduces_iteration_count`
/// with K perturbed via a small field refinement instead of f.
#[test]
fn warm_start_across_field_refinement_drops_cg_iterations_and_matches_cold_solution() {
    let (nx, ny, nz) = (10, 1, 2);
    let (length, width, height) = (1.0_f64, 0.1_f64, 0.1_f64);

    let (coords, tet_conn) = build_bar_mesh(nx, ny, nz, length, width, height);
    let n_nodes = (nx + 1) * (ny + 1) * (nz + 1);
    let f_load = build_axial_tip_load(n_nodes, nx, ny, nz, 1000.0_f64);
    let bcs    = build_root_clamp_bcs(nx, ny, nz);

    // v1: constant field — all elements E=200 GPa.
    let e1   = 200e9_f64;
    let iso1 = IsotropicElastic { youngs_modulus: e1, poisson_ratio: 0.3 };
    let mat1 = AnisotropicMaterial::from_law(&iso1, IDENTITY_3X3);
    let field_v1 = ConstantField { material: mat1 };

    // Cold-solve K1 to get u1 (will be used as the warm-start initial guess).
    let r1 = assemble_and_solve(n_nodes, &coords, &tet_conn, &field_v1, &f_load, &bcs, None);
    assert!(r1.converged, "v1 cold solve did not converge ({} iters)", r1.iterations);

    // v2: small refinement — interior strip x ∈ [0.4L, 0.6L) nudged +2%.
    let e2_interior = e1 * 1.02;
    let iso2 = IsotropicElastic { youngs_modulus: e2_interior, poisson_ratio: 0.3 };
    let mat2 = AnisotropicMaterial::from_law(&iso2, IDENTITY_3X3);
    let x_lo = length * 0.4;
    let x_hi = length * 0.6;
    let field_v2 = DiscreteCellField {
        cells: vec![mat1, mat2],  // cell 0 = baseline, cell 1 = perturbed
        locator: Box::new(move |p: [f64; 3]| {
            if p[0] >= x_lo && p[0] < x_hi { Some(1) } else { Some(0) }
        }),
    };

    // Cold-solve K2 from zero → iters_cold baseline.
    let r2_cold = assemble_and_solve(
        n_nodes, &coords, &tet_conn, &field_v2, &f_load, &bcs, None,
    );
    assert!(r2_cold.converged, "v2 cold solve did not converge ({} iters)", r2_cold.iterations);
    let iters_cold = r2_cold.iterations;

    // Warm-solve K2 from u1 → iters_warm.
    let r2_warm = assemble_and_solve(
        n_nodes, &coords, &tet_conn, &field_v2, &f_load, &bcs,
        Some(&r1.u),  // u1 as CG initial guess
    );
    assert!(r2_warm.converged, "v2 warm solve did not converge ({} iters)", r2_warm.iterations);
    let iters_warm = r2_warm.iterations;

    // ── (a) Iteration drop ────────────────────────────────────────────────────
    assert!(
        iters_warm < iters_cold,
        "warm ({iters_warm} iters) must use fewer CG iterations than cold ({iters_cold} iters) \
         when starting from u1 (solution of the nearby K1 problem)",
    );

    // ── (b) Tolerance-equivalence ─────────────────────────────────────────────
    // Both solves converged to the same SPD system K2·u=f within tolerance 1e-10,
    // so they must agree component-wise to within 1e-9·max(1,|u_cold[i]|).
    assert_eq!(
        r2_cold.u.len(),
        r2_warm.u.len(),
        "cold and warm displacement vectors must have the same length",
    );
    for i in 0..r2_cold.u.len() {
        let u_cold = r2_cold.u[i];
        let u_warm = r2_warm.u[i];
        let tol  = 1e-9 * u_cold.abs().max(1.0);
        let diff = (u_warm - u_cold).abs();
        assert!(
            diff < tol,
            "tolerance-equivalence at i={i}: |u_warm−u_cold|={diff:.3e} ≥ tol={tol:.3e} \
             (u_cold={u_cold:.3e}, u_warm={u_warm:.3e})",
        );
    }
}
