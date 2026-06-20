//! Acceptance signal (PRD `docs/prds/v0_6/tensegrity-membrane.md` §5 / §10 / §11,
//! task η / layer M2): combined membrane + bar/cable load analysis with a
//! tension-only active set (slack cables + slack patches).
//!
//! # Cases
//!
//! 1. **Flat tent membrane under a transverse load** — a planar diamond of four
//!    anchored corners + one free center node, triangulated into four CST
//!    membrane patches under isotropic prestress `σ` (thickness `t`). A flat
//!    membrane's transverse stiffness comes **entirely** from `K_g`, so the
//!    center's out-of-plane deflection `u_z = P_z / K_zz` cross-checks against an
//!    INDEPENDENT in-test assembly of the four patches'
//!    `geometric_element_stiffness_membrane_cst` transverse diagonal — no frozen
//!    hand number.
//!
//! 2. **Combined pavilion** (added in step-7) — struts + cables + a membrane
//!    patch sharing nodes under one nodal load: one combined SPD solve.
//!
//! 3. **Membrane slack drop** (added in step-9) — an in-plane load drives a
//!    patch's minimum principal stress compressive; the tension-only active set
//!    drops it and re-solves to the reduced (patch-removed) system.
//!
//! # Tolerance
//!
//! Inner CG tolerance ≤ 1e-12; deflection assertions ≤ 1e-9 relative. The CST
//! membrane `K_g` transverse stiffening is the exact prestressed-membrane
//! transverse stiffness (the ζ element is patch-test-validated), so the
//! independent transverse-diagonal reference is method-exact.

use reify_solver_elastic::assembly::test_support::assert_close;
use reify_solver_elastic::{
    CgSolverOptions, IsotropicElastic, MembraneLoadOptions, MembranePatch, MembranePrestress,
    geometric_element_stiffness_membrane_cst, membrane_load_analysis,
};

/// Tight inner-CG options shared across the goldens.
fn tight_options() -> MembraneLoadOptions {
    MembraneLoadOptions {
        cg: CgSolverOptions {
            tolerance: 1.0e-12,
            max_iter: 1000,
        },
        ..MembraneLoadOptions::default()
    }
}

/// A ν = 0 plane-stress fabric so the in-plane elastic block is well-conditioned
/// without coupling into the (decoupled) transverse response.
fn fabric(e: f64) -> IsotropicElastic {
    IsotropicElastic {
        youngs_modulus: e,
        poisson_ratio: 0.0,
    }
}

/// Independent transverse stiffness `K_zz` at the free center node: the sum over
/// the four patches of `geometric_element_stiffness_membrane_cst`'s center-node
/// transverse diagonal, built WITHOUT touching `membrane_load_analysis`.
///
/// Each patch carries the center as its FIRST corner (local node 0), and every
/// patch is flat in the `z = 0` plane (local normal `e3 = ±z`), so the center's
/// global-z DOF is local index `2` and its transverse stiffness is `kg.get(2, 2)`.
/// `K_e` contributes nothing transverse and `K_g` nothing in-plane, so the global
/// center-z equation after pinning the corners is exactly `K_zz · u_z = P_z`.
fn independent_center_kzz(
    nodes: &[[f64; 3]],
    patches: &[(usize, usize, usize)],
    sigma: f64,
    thickness: f64,
) -> f64 {
    let mut kzz = 0.0;
    for &(a, b, c) in patches {
        let tri = [nodes[a], nodes[b], nodes[c]];
        let kg = geometric_element_stiffness_membrane_cst(
            &tri,
            &MembranePrestress::isotropic(sigma * thickness),
        );
        // Center is local node 0 ⇒ its global-z (flat patch) DOF is index 2.
        kzz += kg.get(2, 2);
    }
    kzz
}

/// (1) Flat tent membrane under a transverse point load.
///
/// A planar diamond — corners `0..3` at `(±1,0,0)` / `(0,±1,0)` anchored, center
/// node `4` at the origin free — triangulated into four CST membrane patches
/// `(4, i, j)` each under isotropic prestress `σ` of thickness `t`, no bar
/// members. A transverse load `P_z` at the center deflects it purely out of plane
/// by `u_z = P_z / K_zz`, where `K_zz` is the independent transverse-diagonal sum.
/// The in-plane response is exactly zero (the transverse load has no in-plane
/// component and `K_e ⊥ K_g` for a flat patch), so `Δσ ≈ 0` and every patch's
/// total stress stays the isotropic prestress `σ` — no patch slackens.
#[test]
fn flat_tent_membrane_transverse_load() {
    let sigma = 1000.0_f64; // isotropic membrane prestress [Pa]
    let t = 0.01_f64; // thickness [m]
    let e = 1.0e6_f64; // fabric Young's modulus [Pa]
    let p_z = 5.0_f64; // transverse tip load at the center [N]

    // Planar diamond: corners 0..3, free center node 4 at the origin.
    let nodes = vec![
        [1.0, 0.0, 0.0],  // 0 — corner (anchored)
        [0.0, 1.0, 0.0],  // 1 — corner (anchored)
        [-1.0, 0.0, 0.0], // 2 — corner (anchored)
        [0.0, -1.0, 0.0], // 3 — corner (anchored)
        [0.0, 0.0, 0.0],  // 4 — free center
    ];
    // Four patches fanning the center (local node 0) to each diamond edge.
    let tris = [(4, 0, 1), (4, 1, 2), (4, 2, 3), (4, 3, 0)];
    let patches: Vec<MembranePatch> = tris
        .iter()
        .map(|&(a, b, c)| MembranePatch {
            nodes: (a, b, c),
            thickness: t,
            material: fabric(e),
            prestress: sigma,
        })
        .collect();

    let mut loads = vec![[0.0, 0.0, 0.0]; nodes.len()];
    loads[4] = [0.0, 0.0, p_z]; // transverse load at the center
    let fixed_nodes = vec![0, 1, 2, 3];
    let options = tight_options();

    let solve = membrane_load_analysis(&nodes, &[], &patches, &loads, &fixed_nodes, &options)
        .expect("flat tent membrane must be feasible");

    // Independent transverse-stiffness cross-check: u_z = P_z / K_zz.
    let kzz = independent_center_kzz(&nodes, &tris, sigma, t);
    assert!(kzz > 0.0, "independent K_zz must be positive, got {kzz}");
    let uz_expected = p_z / kzz;
    assert_close(
        solve.displacements[4][2],
        uz_expected,
        1e-9,
        "u_z[center] = P_z / K_zz (independent transverse diagonal)",
    );
    // Pure transverse response: no in-plane motion at the center.
    assert_close(solve.displacements[4][0], 0.0, 1e-9, "u_x[center] = 0");
    assert_close(solve.displacements[4][1], 0.0, 1e-9, "u_y[center] = 0");
    // Anchored corners do not move.
    for &corner in &[0usize, 1, 2, 3] {
        for axis in 0..3 {
            assert_close(
                solve.displacements[corner][axis],
                0.0,
                1e-12,
                "anchored corner does not move",
            );
        }
    }

    // No bar members ⇒ the line-member result vectors are empty.
    assert!(solve.member_forces.is_empty(), "no bar members");
    assert!(solve.member_force_deltas.is_empty(), "no bar members");
    assert!(solve.member_slack.is_empty(), "no bar members");

    // Per-patch fields are populated (real f64, one entry per patch).
    assert_eq!(solve.surface_stress_deltas.len(), patches.len());
    assert_eq!(solve.surface_principal_stresses.len(), patches.len());
    assert_eq!(solve.surface_slack.len(), patches.len());
    for p in 0..patches.len() {
        // Pure transverse on a flat patch ⇒ in-plane strain ≈ 0 ⇒ Δσ ≈ 0.
        for i in 0..2 {
            for j in 0..2 {
                let v = solve.surface_stress_deltas[p][i][j];
                assert!(v.is_finite(), "Δσ[{p}][{i}][{j}] must be finite, got {v}");
                assert!(
                    v.abs() < 1e-3,
                    "Δσ[{p}][{i}][{j}] = {v} should be ≈0 for a pure-transverse flat patch",
                );
            }
        }
        // Total stress stays the isotropic prestress σ ⇒ both principals ≈ σ.
        assert_close(
            solve.surface_principal_stresses[p][0],
            sigma,
            1e-6,
            "min principal ≈ σ (taut)",
        );
        assert_close(
            solve.surface_principal_stresses[p][1],
            sigma,
            1e-6,
            "max principal ≈ σ (taut)",
        );
        assert!(!solve.surface_slack[p], "no patch slackens under transverse load");
    }

    assert!(solve.converged, "solve must converge");
    assert_eq!(
        solve.active_set_iterations, 1,
        "no drop ⇒ exactly one active-set pass",
    );
}
