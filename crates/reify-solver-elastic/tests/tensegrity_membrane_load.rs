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
    AssemblyElement, AssemblyMode, BarMember, BarSection, CgSolverOptions, DirichletBc,
    IsotropicElastic, MemberKind, MembraneLoadOptions, MembranePatch, MembranePrestress, SolverMode,
    apply_dirichlet_row_elimination, apply_point_load, assemble_global_stiffness,
    geometric_element_stiffness_membrane_cst, membrane_load_analysis, membrane_tangent_stiffness,
    solve_cg,
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

/// (2) Combined pavilion: struts + cables AND a membrane patch sharing a node,
/// solved as ONE combined SPD system.
///
/// Free center node `2 = (0,0,0)`; a flat membrane patch `(2,0,1)` in `z=0`
/// (corners `0=(1,0,0)`, `1=(0,1,0)` anchored), a vertical CABLE `(2,3)` up to
/// anchor `3=(0,0,1)`, and a vertical STRUT `(2,4)` down to anchor `4=(0,0,-1)`.
/// A purely transverse load `[0,0,−P]` at the center pulls it straight down: the
/// in-plane DOFs decouple from `z` (membrane `K_e ⊥ K_g`; vertical bars couple
/// `z` only through `K_e`), so node 2 moves purely `−z` by `u_z = −P / K_zz`,
/// where `K_zz = kg_membrane(center) + EA/L_cable + EA/L_strut` — BOTH the
/// membrane (via `K_g`) and the bars (via `K_e`) contribute to the transverse
/// stiffness. The cable stretches (tension up ⇒ taut), the strut is never
/// dropped, and the membrane stays taut.
#[test]
fn combined_pavilion_struts_cables_membrane() {
    let sigma = 1.0e5_f64; // membrane prestress [Pa]
    let t = 0.01_f64; // thickness [m]
    let e_fab = 1.0e6_f64; // fabric Young's modulus [Pa]
    let e_bar = 2.0e9_f64; // bar Young's modulus [Pa]
    let a_bar = 1.0e-4_f64; // bar area [m²]
    let n_cable = 3000.0_f64; // cable tension prestress [N]
    let n_strut = -1000.0_f64; // strut compression prestress [N]
    let p = 4010.0_f64; // downward (−z) load at the center [N]

    let nodes = vec![
        [1.0, 0.0, 0.0],  // 0 — membrane corner (anchored)
        [0.0, 1.0, 0.0],  // 1 — membrane corner (anchored)
        [0.0, 0.0, 0.0],  // 2 — free center (membrane + cable + strut)
        [0.0, 0.0, 1.0],  // 3 — cable anchor (above)
        [0.0, 0.0, -1.0], // 4 — strut anchor (below)
    ];
    let patches = vec![MembranePatch {
        nodes: (2, 0, 1),
        thickness: t,
        material: fabric(e_fab),
        prestress: sigma,
    }];
    let bars = vec![
        BarMember {
            nodes: (2, 3),
            kind: MemberKind::Cable,
            section: BarSection {
                youngs_modulus: e_bar,
                area: a_bar,
            },
            prestress: n_cable,
        },
        BarMember {
            nodes: (2, 4),
            kind: MemberKind::Strut,
            section: BarSection {
                youngs_modulus: e_bar,
                area: a_bar,
            },
            prestress: n_strut,
        },
    ];
    let mut loads = vec![[0.0, 0.0, 0.0]; nodes.len()];
    loads[2] = [0.0, 0.0, -p];
    let fixed_nodes = vec![0, 1, 3, 4];
    let options = tight_options();

    let solve = membrane_load_analysis(&nodes, &bars, &patches, &loads, &fixed_nodes, &options)
        .expect("combined pavilion must be feasible");

    // Independent combined transverse stiffness: membrane K_g (center is local
    // node 0 ⇒ flat-patch z-DOF index 2) + each vertical bar's axial EA/L.
    let kg = geometric_element_stiffness_membrane_cst(
        &[nodes[2], nodes[0], nodes[1]],
        &MembranePrestress::isotropic(sigma * t),
    );
    let l_cable = 1.0_f64;
    let l_strut = 1.0_f64;
    let kzz = kg.get(2, 2) + e_bar * a_bar / l_cable + e_bar * a_bar / l_strut;
    let uz_expected = -p / kzz; // = −0.01
    assert_close(
        solve.displacements[2][2],
        uz_expected,
        1e-9,
        "u_z[center] = −P / (K_g_membrane + EA/L_cable + EA/L_strut)",
    );
    // Purely transverse: no in-plane motion (the {x,y} and {z} blocks decouple).
    assert_close(solve.displacements[2][0], 0.0, 1e-9, "u_x[center] = 0");
    assert_close(solve.displacements[2][1], 0.0, 1e-9, "u_y[center] = 0");
    assert!(solve.displacements[2][2] < 0.0, "center moves toward the −z load");
    // All displacements finite.
    for d in &solve.displacements {
        for &v in d {
            assert!(v.is_finite(), "displacement must be finite, got {v}");
        }
    }

    // Line-member results are populated for EVERY bar (length == bar_members.len()).
    assert_eq!(solve.member_forces.len(), bars.len(), "member_forces per bar");
    assert_eq!(
        solve.member_force_deltas.len(),
        bars.len(),
        "member_force_deltas per bar",
    );
    assert_eq!(solve.member_slack.len(), bars.len(), "member_slack per bar");

    // Cable (index 0) stretches under the downward pull: dN = (EA/L)·(−u_z) ⇒
    // total N0 + dN stays positive (taut). The strut (index 1) is never dropped.
    let dn_cable = e_bar * a_bar / l_cable * (-uz_expected);
    assert_close(
        solve.member_force_deltas[0],
        dn_cable,
        1e-6,
        "cable dN = (EA/L)·(−u_z)",
    );
    assert_close(
        solve.member_forces[0],
        n_cable + dn_cable,
        1e-6,
        "cable force = N0 + dN (taut)",
    );
    assert!(solve.member_forces[0] > 0.0, "cable stays in tension");
    assert_eq!(solve.member_slack, vec![false, false], "no line member slack");

    // Patch stress fields populated; no patch slackens.
    assert_eq!(solve.surface_stress_deltas.len(), patches.len());
    assert_eq!(solve.surface_principal_stresses.len(), patches.len());
    assert_eq!(solve.surface_slack, vec![false], "no patch slack");
    for row in &solve.surface_stress_deltas[0] {
        for &v in row {
            assert!(v.is_finite(), "Δσ must be finite, got {v}");
        }
    }

    assert!(solve.converged, "combined solve must converge");
    assert_eq!(
        solve.active_set_iterations, 1,
        "no drop ⇒ exactly one active-set pass",
    );
}

/// Independent reduced-topology reference: the free node `F`'s displacement once
/// the slack patch is removed and only the taut patch `A` (corners `a0`, `a1`)
/// holds it. A 3-node membrane solve (F free + two pinned anchors) under the same
/// in-plane load, assembled WITHOUT touching `membrane_load_analysis`.
fn reduced_single_patch_uf(
    f: [f64; 3],
    a0: [f64; 3],
    a1: [f64; 3],
    sigma: f64,
    t: f64,
    e: f64,
    load_x: f64,
) -> [f64; 3] {
    let ref_nodes = [f, a0, a1]; // F is reference node 0; a0/a1 pinned
    let kt = membrane_tangent_stiffness(
        &ref_nodes,
        t,
        &fabric(e),
        &MembranePrestress::isotropic(sigma * t),
    );
    let conn = [0usize, 1, 2];
    let elem = AssemblyElement {
        id: 0,
        connectivity: &conn,
        k_e: &kt,
    };
    let mut k = assemble_global_stiffness(3, &[elem], AssemblyMode::Deterministic);
    let mut ff = vec![0.0_f64; 9];
    apply_point_load(&mut ff, 0, [load_x, 0.0, 0.0]);
    let mut bcs: Vec<DirichletBc> = Vec::new();
    for node in [1usize, 2] {
        for axis in 0..3 {
            bcs.push(DirichletBc {
                dof: 3 * node + axis,
                value: 0.0,
            });
        }
    }
    apply_dirichlet_row_elimination(&mut k, &mut ff, &bcs);
    let result = solve_cg(
        &k,
        &ff,
        CgSolverOptions {
            tolerance: 1.0e-12,
            max_iter: 1000,
        },
        SolverMode::Deterministic,
    );
    assert!(result.converged, "reduced single-patch solve must converge");
    let u = result.u();
    [u[0], u[1], u[2]]
}

/// (3) Membrane slack active-set drop — the headline η signal.
///
/// Two membrane patches share the free center node `F=2=(0,0,0)`, symmetric about
/// the x-axis: patch `A=(2,0,1)` to the left (anchors `(−1,±0.5,0)`), patch
/// `B=(2,3,4)` to the right (anchors `(1,±0.5,0)`). An in-plane `+x` load on `F`
/// STRETCHES `A` (its loading principal goes tensile, the transverse principal
/// stays `σ₀` — the x-axis symmetry kills the shear, so A NEVER slackens) and
/// COMPRESSES `B` (its loading principal `σ₀ − E·δ` goes below zero). The
/// tension-only active set drops `B`, re-solves with only `A` holding `F`, and
/// confirms the fixed point — two passes. The dropped patch carries nothing
/// (total stress 0 ⇒ principals `[0,0]`, the 2-D analogue of T3b's slack cable),
/// and the post-drop deflection matches an independent patch-A-only solve.
#[test]
fn membrane_slack_active_set_drop() {
    let sigma = 1.0_f64; // small prestress [Pa] so a modest load slackens B
    let t = 1.0_f64; // unit thickness
    let e = 100.0_f64; // soft fabric
    let p = 5.0_f64; // +x in-plane load at F

    let nodes = vec![
        [-1.0, 0.5, 0.0],  // 0 — A anchor
        [-1.0, -0.5, 0.0], // 1 — A anchor
        [0.0, 0.0, 0.0],   // 2 — F (free center)
        [1.0, 0.5, 0.0],   // 3 — B anchor
        [1.0, -0.5, 0.0],  // 4 — B anchor
    ];
    let patches = vec![
        MembranePatch {
            nodes: (2, 0, 1), // A (left) — stretches, stays taut
            thickness: t,
            material: fabric(e),
            prestress: sigma,
        },
        MembranePatch {
            nodes: (2, 3, 4), // B (right) — compresses, goes slack
            thickness: t,
            material: fabric(e),
            prestress: sigma,
        },
    ];
    let mut loads = vec![[0.0, 0.0, 0.0]; nodes.len()];
    loads[2] = [p, 0.0, 0.0];
    let fixed_nodes = vec![0, 1, 3, 4];
    let options = tight_options();

    let solve = membrane_load_analysis(&nodes, &[], &patches, &loads, &fixed_nodes, &options)
        .expect("post-drop system (patch A holds F) is feasible");

    // Patch B (index 1) slackens; patch A (index 0) stays taut.
    assert_eq!(
        solve.surface_slack,
        vec![false, true],
        "compressed patch B drops; stretched patch A stays taut",
    );
    // Two passes: pass 1 drops B; pass 2 confirms the fixed point.
    assert_eq!(
        solve.active_set_iterations, 2,
        "one drop ⇒ two active-set passes",
    );

    // The dropped patch carries nothing: total stress 0 ⇒ principals [0, 0] and a
    // delta that exactly cancels the prestress (σ₀·I + Δσ = 0).
    assert_close(
        solve.surface_principal_stresses[1][0],
        0.0,
        1e-9,
        "dropped patch B min principal = 0",
    );
    assert_close(
        solve.surface_principal_stresses[1][1],
        0.0,
        1e-9,
        "dropped patch B max principal = 0",
    );
    assert_close(
        solve.surface_stress_deltas[1][0][0],
        -sigma,
        1e-9,
        "dropped patch B Δσxx = −σ₀ (total → 0)",
    );
    assert_close(
        solve.surface_stress_deltas[1][1][1],
        -sigma,
        1e-9,
        "dropped patch B Δσyy = −σ₀ (total → 0)",
    );

    // The taut patch A keeps a positive minimum principal ≈ σ₀ (its transverse
    // principal is unaffected by the in-plane stretch).
    assert!(
        solve.surface_principal_stresses[0][0] > 0.0,
        "taut patch A min principal stays positive, got {}",
        solve.surface_principal_stresses[0][0],
    );
    assert_close(
        solve.surface_principal_stresses[0][0],
        sigma,
        1e-6,
        "taut patch A min principal ≈ σ₀",
    );

    // Post-drop deflection matches the independent patch-A-only reduced system.
    let uf_cross = reduced_single_patch_uf(nodes[2], nodes[0], nodes[1], sigma, t, e, p);
    for axis in 0..3 {
        assert_close(
            solve.displacements[2][axis],
            uf_cross[axis],
            1e-9,
            "F displacement matches the reduced patch-A-only solve",
        );
    }
    assert!(solve.displacements[2][0] > 0.0, "F moves toward the +x load");
    assert!(solve.converged, "post-drop solve must converge");

    // No bar members in this case.
    assert!(solve.member_forces.is_empty(), "no bar members");
}
