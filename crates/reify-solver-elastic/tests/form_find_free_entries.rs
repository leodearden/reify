//! The public-entry contract of the free-standing force-density form-finding
//! entries — `form_find_free` and `form_find_free_surfaces` — driven from the
//! crate root only: the explicit-q form-certification and recovery verdicts and
//! the result's force signs; the GroupRatios search's non-convergence verdicts;
//! and the surface entry's line-only equivalence. The input contract these
//! entries share is `tests/force_density_input.rs`. Moved from
//! `form_find_free.rs`'s unit tests by #9275; the GroupRatios closed-form
//! recovery golden is `tensegrity_t1b_form_find_free.rs`.

use reify_solver_elastic::{
    ForceDensitySpec, FormDefect, FreeFormError, FreeFormResult, MemberKind, form_find_free,
    form_find_free_surfaces,
};

#[allow(dead_code)] // each test binary uses a subset of the shared fixtures
mod common;

use common::{analytic_combined_q, triplex_group_ids, triplex_topology};
use reify_test_support::tensegrity_fixtures::{CANONICAL_TRIPLEX_BOTTOM_Z, triplex_node_coords};

/// A mildly perturbed (≈1e-3 per coordinate, deterministic) guess: the shared
/// canonical triplex (circumradius 1, unit height, 30° twist, node order
/// matching `triplex_topology`) plus fixed small offsets. The form-finding
/// convention refines a guess, so recovery should gauge-fix to it; fixed
/// offsets (no RNG) keep the dependent tests bit-stable.
///
/// Twin of `tensegrity_t1b_form_find_free.rs::perturbed_prism_guess`; hoist owned by #8152.
fn perturbed_twisted_prism() -> Vec<[f64; 3]> {
    const PERTURB: [[f64; 3]; 6] = [
        [0.0009, -0.0011, 0.0007],
        [-0.0013, 0.0006, 0.0010],
        [0.0012, 0.0008, -0.0009],
        [-0.0007, -0.0012, 0.0011],
        [0.0010, -0.0008, -0.0013],
        [-0.0011, 0.0013, 0.0006],
    ];
    triplex_node_coords(CANONICAL_TRIPLEX_BOTTOM_Z)
        .iter()
        .zip(PERTURB.iter())
        .map(|(p, d)| [p[0] + d[0], p[1] + d[1], p[2] + d[2]])
        .collect()
}

/// A *generic* admissible q: distinct per-member magnitudes (struts
/// negative, cables positive). The uniform all-magnitudes-1 assignment is
/// **not** generic — it is highly symmetric (C₃ × top/bottom) and, at the
/// strut/cable ratio −1, accidentally admits a second null mode (D nullity
/// 2). Distinct magnitudes break that symmetry, leaving only the all-ones
/// translation mode, so nullity collapses to exactly 1.
fn generic_admissible_q(kinds: &[MemberKind]) -> Vec<f64> {
    kinds
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let mag = 1.0 + 0.37 * (i as f64);
            match k {
                MemberKind::Cable => mag,
                MemberKind::Strut => -mag,
            }
        })
        .collect()
}

// ---- Explicit-mode feasibility diagnostics ----

#[test]
fn explicit_wrong_nullity_q_is_not_a_form_naming_its_nullity() {
    let (members, kinds) = triplex_topology();
    // Admissible (signs honour kinds) but non-special: nullity 1 (only the
    // translation mode), not the d+1 = 4 a valid 3-D form requires, on one
    // connected structure.
    let q = generic_admissible_q(&kinds);
    assert_eq!(
        form_find_free(
            &perturbed_twisted_prism(),
            &members,
            &kinds,
            &ForceDensitySpec::Explicit(q),
        )
        .unwrap_err(),
        FreeFormError::NotAForm(FormDefect {
            nullity: 1,
            components: 1,
        }),
    );
}

#[test]
fn explicit_coplanar_guess_is_singular_recovery() {
    let (members, kinds) = triplex_topology();
    // The valid nullity-4 closed-form prism q — identical to the healthy
    // explicit tests' input, so *only* the guess is degenerate here, isolating
    // the SingularRecovery guard. The explicit `D` does not depend on the
    // guess; the guess only gauge-fixes the recovery.
    let q = analytic_combined_q(0.0);

    // A *coplanar* (degenerate) guess: the prism's x/y arrangement
    // flattened into the z = 0 plane. Per-axis projection onto null(D) leaves
    // the all-zero z column at zero (a constant vector lies in null(D) via the
    // all-ones mode), so every recovered node has z = 0 — a strictly planar,
    // rank-deficient realisation. The centred-covariance det-vs-isotropic
    // guard must reject it as SingularRecovery rather than return a flat
    // "3-D" form. (Exercises the det branch; a regression that inverts the
    // condition or mis-scales SINGULAR_REL_TOL would let it through.)
    let coplanar_guess: Vec<[f64; 3]> = triplex_node_coords(CANONICAL_TRIPLEX_BOTTOM_Z)
        .iter()
        .map(|p| [p[0], p[1], 0.0])
        .collect();

    assert_eq!(
        form_find_free(
            &coplanar_guess,
            &members,
            &kinds,
            &ForceDensitySpec::Explicit(q),
        )
        .unwrap_err(),
        FreeFormError::SingularRecovery,
    );
}

// ---- Explicit-mode member forces + form_find_free entry point ----

#[test]
fn form_find_free_explicit_populates_result_with_correct_force_signs() {
    let (members, kinds) = triplex_topology();
    let q = analytic_combined_q(0.0);
    let guess = perturbed_twisted_prism();

    let result = form_find_free(
        &guess,
        &members,
        &kinds,
        &ForceDensitySpec::Explicit(q.clone()),
    )
    .expect("explicit closed-form prism q must form-find");

    // Spectrum / convergence metadata.
    assert_eq!(result.nullity, 4, "a valid 3-D form has nullity d+1 = 4");
    assert!(result.converged, "explicit closed-form solve must converge");
    assert_eq!(result.nodes.len(), 6, "one recovered coordinate per node");

    // force_densities echo the input q exactly.
    assert_eq!(
        result.force_densities, q,
        "force_densities must echo input q"
    );

    // member_forces: N_i = q_i · L_i in struts-then-cables (input) order.
    assert_eq!(result.member_forces.len(), members.len());
    // Struts carry compression (N < 0), cables tension (N > 0) — the sign of
    // N_i follows q_i since every recovered length L_i > 0.
    for (idx, (&kind, &n_i)) in kinds.iter().zip(result.member_forces.iter()).enumerate() {
        match kind {
            MemberKind::Strut => assert!(
                n_i < 0.0,
                "strut {idx} must be compressive (N < 0), got {n_i}",
            ),
            MemberKind::Cable => {
                assert!(n_i > 0.0, "cable {idx} must be tensile (N > 0), got {n_i}",)
            }
        }
    }

    // Every recovered member length is positive and finite: L_i = N_i / q_i
    // (q_i is non-zero for every member).
    for (&n_i, &qi) in result.member_forces.iter().zip(q.iter()) {
        let len = n_i / qi;
        assert!(
            len > 0.0 && len.is_finite(),
            "member length L = N/q must be positive & finite, got {len}",
        );
    }
}

// ---- Adaptive GroupRatios force-density search (GroupRatios mode) ----

#[test]
fn group_ratios_search_all_positive_does_not_converge() {
    let (members, _kinds) = triplex_topology();
    // Treat every member as a cable and seed every group positive. A
    // positive-only force-density assignment keeps D a connected-graph
    // weighted Laplacian, whose nullity is exactly 1 (the all-ones
    // translation mode) for *any* positive ratios — so no nullity-4 q exists
    // in the search space. The search must exhaust its budget and report
    // SearchDidNotConverge, never panic.
    let kinds = vec![MemberKind::Cable; members.len()];
    let guess = perturbed_twisted_prism();

    let spec = ForceDensitySpec::GroupRatios {
        group_ids: triplex_group_ids(),
        seed_ratios: vec![1.0, 1.0, 1.0], // all tension → nullity stuck at 1
        reference_group: 1,
    };

    assert_eq!(
        form_find_free(&guess, &members, &kinds, &spec).unwrap_err(),
        FreeFormError::SearchDidNotConverge {
            defect: Some(FormDefect {
                nullity: 1,
                components: 1,
            }),
        },
    );
}

/// Whether `result` is the search's non-convergence carrying a defect that
/// names two connected pieces — the observable a disconnected free-standing
/// model must produce, whatever nullity its search happened to stop at.
fn is_two_piece_non_convergence(result: &Result<FreeFormResult, FreeFormError>) -> bool {
    matches!(
        result,
        Err(FreeFormError::SearchDidNotConverge {
            defect: Some(FormDefect { components: 2, .. }),
        })
    )
}

#[test]
fn line_only_group_ratios_with_an_isolated_node_does_not_converge() {
    // The search drives D's 4 smallest eigenvalues to 0. The isolated node
    // contributes a free translation mode, so the triplex itself settles
    // FLAT (nullity 3) and the total is exactly 4; the extra node keeps the
    // recovery 3-D. Only the one-connected-piece condition sees through it.
    let (members, kinds) = triplex_topology();
    let mut guess = perturbed_twisted_prism();
    guess.push([0.3, 0.2, 0.4]);
    let spec = ForceDensitySpec::GroupRatios {
        group_ids: triplex_group_ids(),
        seed_ratios: vec![-1.0, 1.0, 1.0],
        reference_group: 1,
    };

    let result = form_find_free(&guess, &members, &kinds, &spec);
    assert!(is_two_piece_non_convergence(&result), "{result:?}");
}

#[test]
fn line_only_group_ratios_with_a_disconnected_cable_pair_does_not_converge() {
    let (mut members, mut kinds) = triplex_topology();
    let mut guess = perturbed_twisted_prism();
    guess.push([0.3, 0.2, 0.4]);
    guess.push([0.3, 0.2, 0.9]);
    members.push((6, 7));
    kinds.push(MemberKind::Cable);
    let mut group_ids = triplex_group_ids();
    group_ids.push(3);
    let spec = ForceDensitySpec::GroupRatios {
        group_ids,
        seed_ratios: vec![-1.0, 1.0, 1.0, 1.0],
        reference_group: 1,
    };

    let result = form_find_free(&guess, &members, &kinds, &spec);
    assert!(is_two_piece_non_convergence(&result), "{result:?}");
}

// ── δ (task 4415): surface-aware form_find_free_surfaces ──────────────────

// (d) Empty surfaces: form_find_free_surfaces with empty surfaces/stresses
// must return a result that matches form_find_free in all line-only fields
// (nodes / member_forces / force_densities / nullity / converged) and
// carries an empty (NEVER absent) surface_stresses echo.
#[test]
fn surfaces_free_empty_matches_line_only_form_find_free() {
    let (members, kinds) = triplex_topology();
    let guess = perturbed_twisted_prism();
    let spec = ForceDensitySpec::GroupRatios {
        group_ids: triplex_group_ids(),
        seed_ratios: vec![-1.0, 1.0, 1.0],
        reference_group: 1,
    };

    let line = form_find_free(&guess, &members, &kinds, &spec)
        .expect("line-only GroupRatios must form-find the prism");
    let surf = form_find_free_surfaces(&guess, &members, &kinds, &[], &[], &spec)
        .expect("empty-surface path must match the line-only result");

    // surface_stresses is an empty Vec, not absent.
    assert!(
        surf.surface_stresses.is_empty(),
        "empty surfaces ⇒ empty surface_stresses echo (got {:?})",
        surf.surface_stresses,
    );

    // Line-only fields must agree exactly (same D, same solve path).
    assert_eq!(surf.converged, line.converged);
    assert_eq!(surf.nullity, line.nullity);
    assert_eq!(surf.force_densities, line.force_densities);
    assert_eq!(surf.member_forces.len(), line.member_forces.len());
    for (a, b) in surf.member_forces.iter().zip(line.member_forces.iter()) {
        assert!((a - b).abs() < 1e-12, "member force mismatch: {a} vs {b}");
    }
    assert_eq!(surf.nodes.len(), line.nodes.len());
    let sub = |a: [f64; 3], b: [f64; 3]| -> f64 {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).abs())
            .fold(0.0_f64, f64::max)
    };
    for (a, b) in surf.nodes.iter().zip(line.nodes.iter()) {
        assert!(sub(*a, *b) < 1e-12, "node mismatch: {a:?} vs {b:?}");
    }
}
