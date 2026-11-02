//! The public-entry contract of the free-standing force-density form-finding
//! entries — `form_find_free` and `form_find_free_surfaces` — driven from the
//! crate root only: the explicit-q sign, length, index, form-certification and
//! recovery verdicts and the result's force signs; the GroupRatios search's
//! closed-form recovery, non-convergence verdicts and group-spec guards; and the
//! surface entry's input guards and line-only equivalence. Moved from
//! `form_find_free.rs`'s unit tests by #9275.

use reify_solver_elastic::{
    ForceDensityInputError, ForceDensitySpec, FormDefect, FreeFormError, FreeFormResult,
    GroupSpecDefect, IndexedInput, MemberKind, form_find_free, form_find_free_surfaces,
};

#[allow(dead_code)] // each test binary uses a subset of the shared fixtures
mod common;

use common::{prism_surfaces, triplex_group_ids, triplex_topology};

/// The symmetric triplex prism consistent with [`closed_form_prism_q`]
/// (derived from the per-node FD equilibrium): equal top/bottom circumradius
/// R = 1, unit height, twist α = 30°. Node order matches `triplex_topology`:
/// 0,1,2 top (z = 1) at azimuth 120°·i; 3,4,5 bottom (z = 0) at 120°·i + 30°.
/// Not `common::canonical_prism`, which is a different realisation (z = ±1).
fn twisted_prism() -> Vec<[f64; 3]> {
    let deg = std::f64::consts::PI / 180.0;
    let top = |i: usize| {
        let a = 120.0 * (i as f64) * deg;
        [a.cos(), a.sin(), 1.0]
    };
    let bot = |i: usize| {
        let a = (120.0 * (i as f64) + 30.0) * deg;
        [a.cos(), a.sin(), 0.0]
    };
    vec![top(0), top(1), top(2), bot(0), bot(1), bot(2)]
}

/// A mildly perturbed (≈1e-3 per coordinate, deterministic) [`twisted_prism`]
/// guess: the prism plus fixed small offsets. The form-finding convention
/// refines a guess, so recovery should gauge-fix to it; fixed offsets (no RNG)
/// keep the dependent tests bit-stable.
fn perturbed_twisted_prism() -> Vec<[f64; 3]> {
    const PERTURB: [[f64; 3]; 6] = [
        [0.0009, -0.0011, 0.0007],
        [-0.0013, 0.0006, 0.0010],
        [0.0012, 0.0008, -0.0009],
        [-0.0007, -0.0012, 0.0011],
        [0.0010, -0.0008, -0.0013],
        [-0.0011, 0.0013, 0.0006],
    ];
    twisted_prism()
        .iter()
        .zip(PERTURB.iter())
        .map(|(p, d)| [p[0] + d[0], p[1] + d[1], p[2] + d[2]])
        .collect()
}

/// Closed-form force densities for the symmetric prism, struts-then-cables
/// order: struts −√3, the six horizontals +1, verticals +√3. These make `D`
/// rank-deficient by exactly 4 (D eigenvalues 0,0,0,0,6,6).
fn closed_form_prism_q() -> Vec<f64> {
    let s = 3.0_f64.sqrt();
    vec![
        -s, -s, -s, // struts
        1.0, 1.0, 1.0, // top horizontals
        1.0, 1.0, 1.0, // bottom horizontals
        s, s, s, // verticals
    ]
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
fn explicit_cable_with_nonpositive_q_is_sign_violation() {
    let (members, kinds) = triplex_topology();
    let mut q = closed_form_prism_q();
    // Force a top cable (member 3) to a non-positive density: a cable must
    // carry tension (q > 0), so this is infeasible input.
    q[3] = -1.0;
    assert_eq!(
        form_find_free(
            &perturbed_twisted_prism(),
            &members,
            &kinds,
            &ForceDensitySpec::Explicit(q),
        )
        .unwrap_err(),
        FreeFormError::Input(ForceDensityInputError::SignViolation { member: 3 }),
    );
}

#[test]
fn explicit_strut_with_nonnegative_q_is_sign_violation() {
    let (members, kinds) = triplex_topology();
    let mut q = closed_form_prism_q();
    // Force a strut (member 0) to a non-negative density: a strut must carry
    // compression (q < 0), so this is infeasible input.
    q[0] = 1.0;
    assert_eq!(
        form_find_free(
            &perturbed_twisted_prism(),
            &members,
            &kinds,
            &ForceDensitySpec::Explicit(q),
        )
        .unwrap_err(),
        FreeFormError::Input(ForceDensityInputError::SignViolation { member: 0 }),
    );
}

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
fn explicit_length_mismatch_is_dimension_mismatch() {
    let (members, kinds) = triplex_topology();
    // One density short of the 12 members → dimension mismatch, caught up
    // front before any sign or nullity work.
    let q = vec![1.0; members.len() - 1];
    assert_eq!(
        form_find_free(
            &perturbed_twisted_prism(),
            &members,
            &kinds,
            &ForceDensitySpec::Explicit(q),
        )
        .unwrap_err(),
        FreeFormError::Input(ForceDensityInputError::DimensionMismatch),
    );
}

#[test]
fn explicit_out_of_range_member_index_is_index_out_of_range() {
    let (mut members, kinds) = triplex_topology();
    let q = closed_form_prism_q();
    // Point a member at node 6 — out of range for the 6-node (0..=5) problem.
    // `D` is 6×6, so without the bounds guard this panics on the `d[(j, j)]`
    // index in `assemble_d`; the guard turns it into a
    // clean indexed input error through the public entry point (no panic).
    members[0] = (0, 6);
    assert_eq!(
        form_find_free(
            &perturbed_twisted_prism(),
            &members,
            &kinds,
            &ForceDensitySpec::Explicit(q),
        )
        .unwrap_err(),
        FreeFormError::Input(ForceDensityInputError::IndexOutOfRange {
            what: IndexedInput::MemberEndpoint { member: 0 },
            index: 6,
            len: 6,
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
    let q = closed_form_prism_q();

    // A *coplanar* (degenerate) guess: the prism's x/y arrangement
    // flattened into the z = 0 plane. Per-axis projection onto null(D) leaves
    // the all-zero z column at zero (a constant vector lies in null(D) via the
    // all-ones mode), so every recovered node has z = 0 — a strictly planar,
    // rank-deficient realisation. The centred-covariance det-vs-isotropic
    // guard must reject it as SingularRecovery rather than return a flat
    // "3-D" form. (Exercises the det branch; a regression that inverts the
    // condition or mis-scales SINGULAR_REL_TOL would let it through.)
    let coplanar_guess: Vec<[f64; 3]> = twisted_prism().iter().map(|p| [p[0], p[1], 0.0]).collect();

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
    let q = closed_form_prism_q();
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
fn group_ratios_search_recovers_closed_form_prism_relative_q() {
    let (members, kinds) = triplex_topology();
    let guess = perturbed_twisted_prism();

    // Seed near the *signs* of the closed form — struts compressive (−1),
    // horizontals (the reference) and verticals tensile (+1) — but with
    // magnitudes all 1, not the √3 closed form. The adaptive search must
    // discover the relative magnitudes on its own.
    let spec = ForceDensitySpec::GroupRatios {
        group_ids: triplex_group_ids(),
        seed_ratios: vec![-1.0, 1.0, 1.0],
        reference_group: 1, // horizontals held fixed as the scale gauge
    };

    let result = form_find_free(&guess, &members, &kinds, &spec)
        .expect("group-ratio search must form-find the prism");

    // Spectrum / convergence metadata: a valid 3-D form has nullity d+1 = 4.
    assert_eq!(result.nullity, 4, "form-found prism must have nullity 4");
    assert!(result.converged, "group-ratio solve must converge");

    // Recovered *relative* densities: struts −√3, verticals +√3, horizontals
    // pinned at the +1 reference. Every member in a group shares its ratio.
    let s = 3.0_f64.sqrt();
    for i in 0..3 {
        assert!(
            (result.force_densities[i] - (-s)).abs() < 1e-6,
            "strut {i} relative q must be ≈ −√3, got {}",
            result.force_densities[i],
        );
    }
    for i in 3..9 {
        assert!(
            (result.force_densities[i] - 1.0).abs() < 1e-12,
            "horizontal {i} is the reference group, must stay = 1, got {}",
            result.force_densities[i],
        );
    }
    for i in 9..12 {
        assert!(
            (result.force_densities[i] - s).abs() < 1e-6,
            "vertical {i} relative q must be ≈ +√3, got {}",
            result.force_densities[i],
        );
    }

    // Force signs follow the recovered q: struts compressive, cables tensile.
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
}

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

#[test]
fn group_ratios_out_of_range_member_index_is_index_out_of_range() {
    let (mut members, kinds) = triplex_topology();
    let guess = perturbed_twisted_prism();
    // An out-of-range node index (6 ≥ the 6-node problem) must be rejected
    // before the search assembles `D`, not panic mid-search.
    members[0] = (0, 6);
    let spec = ForceDensitySpec::GroupRatios {
        group_ids: triplex_group_ids(),
        seed_ratios: vec![-1.0, 1.0, 1.0],
        reference_group: 1,
    };
    assert_eq!(
        form_find_free(&guess, &members, &kinds, &spec).unwrap_err(),
        FreeFormError::Input(ForceDensityInputError::IndexOutOfRange {
            what: IndexedInput::MemberEndpoint { member: 0 },
            index: 6,
            len: 6,
        }),
    );
}

// ---- GroupRatios-mode group-spec guards (GroupSpec::new) ----

#[test]
fn group_ratios_zero_seed_ratio_is_a_zero_seed() {
    let (members, kinds) = triplex_topology();
    let guess = perturbed_twisted_prism();
    // A zero seed has no sign to hold fixed while the search varies its
    // magnitude, so a zero entry in `seed_ratios` is rejected up front.
    let spec = ForceDensitySpec::GroupRatios {
        group_ids: triplex_group_ids(),
        seed_ratios: vec![-1.0, 0.0, 1.0],
        reference_group: 1,
    };
    assert_eq!(
        form_find_free(&guess, &members, &kinds, &spec).unwrap_err(),
        FreeFormError::Input(ForceDensityInputError::MalformedGroupSpec(
            GroupSpecDefect::ZeroSeed { group: 1 }
        )),
    );
}

#[test]
fn group_ratios_reference_group_out_of_range_is_unseeded() {
    let (members, kinds) = triplex_topology();
    let guess = perturbed_twisted_prism();
    // `seed_ratios` is indexed by group id (3 groups ⇒ valid ids 0..=2), so a
    // `reference_group` of 3 is out of range.
    let spec = ForceDensitySpec::GroupRatios {
        group_ids: triplex_group_ids(),
        seed_ratios: vec![-1.0, 1.0, 1.0],
        reference_group: 3,
    };
    assert_eq!(
        form_find_free(&guess, &members, &kinds, &spec).unwrap_err(),
        FreeFormError::Input(ForceDensityInputError::MalformedGroupSpec(
            GroupSpecDefect::UnseededReferenceGroup
        )),
    );
}

#[test]
fn group_ratios_group_id_out_of_range_is_an_unseeded_group() {
    let (members, kinds) = triplex_topology();
    let guess = perturbed_twisted_prism();
    // A `group_ids` entry referencing a group with no seed ratio (id 3 with
    // only 3 groups defined) is out of range.
    let mut group_ids = triplex_group_ids();
    group_ids[0] = 3; // no seed_ratios[3]
    let spec = ForceDensitySpec::GroupRatios {
        group_ids,
        seed_ratios: vec![-1.0, 1.0, 1.0],
        reference_group: 1,
    };
    assert_eq!(
        form_find_free(&guess, &members, &kinds, &spec).unwrap_err(),
        FreeFormError::Input(ForceDensityInputError::MalformedGroupSpec(
            GroupSpecDefect::UnseededGroup { member: 0 }
        )),
    );
}

#[test]
fn group_ratios_group_ids_length_mismatch_is_not_per_member() {
    let (members, kinds) = triplex_topology();
    let guess = perturbed_twisted_prism();
    // `group_ids` must be parallel to `members`; one short is caught before
    // any search work.
    let mut group_ids = triplex_group_ids();
    group_ids.pop();
    let spec = ForceDensitySpec::GroupRatios {
        group_ids,
        seed_ratios: vec![-1.0, 1.0, 1.0],
        reference_group: 1,
    };
    assert_eq!(
        form_find_free(&guess, &members, &kinds, &spec).unwrap_err(),
        FreeFormError::Input(ForceDensityInputError::MalformedGroupSpec(
            GroupSpecDefect::GroupIdsNotPerMember
        )),
    );
}

// ── δ (task 4415): surface-aware form_find_free_surfaces ──────────────────

// (a) `surfaces` and `surface_stresses` must agree in length — each triangle
// needs exactly one isotropic σ. One stress short ⇒ SurfaceCountMismatch.
#[test]
fn surfaces_free_count_mismatch_is_surface_count_mismatch() {
    let (members, kinds) = triplex_topology();
    let guess = perturbed_twisted_prism();
    let surfaces = prism_surfaces();
    let sigmas = vec![0.2]; // 2 surfaces, 1 stress → mismatch
    let spec = ForceDensitySpec::GroupRatios {
        group_ids: triplex_group_ids(),
        seed_ratios: vec![-1.0, 1.0, 1.0],
        reference_group: 1,
    };

    assert_eq!(
        form_find_free_surfaces(&guess, &members, &kinds, &surfaces, &sigmas, &spec).unwrap_err(),
        FreeFormError::Input(ForceDensityInputError::SurfaceCountMismatch),
    );
}

// (b) A non-positive surface stress σ ≤ 0 is infeasible — the surface
// analogue of a cable with q ≤ 0. Must return NonTensionSurfaceStress.
#[test]
fn surfaces_free_nonpositive_sigma_is_non_tension() {
    let (members, kinds) = triplex_topology();
    let guess = perturbed_twisted_prism();
    let surfaces = prism_surfaces();
    let sigmas = vec![0.2, -0.1]; // second triangle σ < 0 → infeasible
    let spec = ForceDensitySpec::GroupRatios {
        group_ids: triplex_group_ids(),
        seed_ratios: vec![-1.0, 1.0, 1.0],
        reference_group: 1,
    };

    assert_eq!(
        form_find_free_surfaces(&guess, &members, &kinds, &surfaces, &sigmas, &spec).unwrap_err(),
        FreeFormError::Input(ForceDensityInputError::NonTensionSurfaceStress { surface: 1 }),
    );
}

// (c) A surface triangle corner that indexes past the node array is
// infeasible input — `assemble_surface_matrix` would panic on its
// `nodes[gi]` index. The module contract promises infeasible input becomes
// a clean typed error, never a panic.
#[test]
fn surfaces_free_out_of_range_index_is_index_out_of_range() {
    let (members, kinds) = triplex_topology();
    let guess = perturbed_twisted_prism();
    // Boundary index: 6 is the FIRST invalid index for the 6-node triplex,
    // so this pins the `≥ n` comparison that a `> n` typo would let pass.
    // The predicate ANDs three comparisons, so each sibling test puts the
    // bad index in a different corner — THIRD here, first and second in the
    // two anchored tests in tests/form_find_anchored_entries.rs — pinning all
    // three between them.
    let surfaces = vec![(0usize, 1usize, 6usize)];
    let sigmas = vec![0.2];
    let spec = ForceDensitySpec::GroupRatios {
        group_ids: triplex_group_ids(),
        seed_ratios: vec![-1.0, 1.0, 1.0],
        reference_group: 1,
    };

    assert_eq!(
        form_find_free_surfaces(&guess, &members, &kinds, &surfaces, &sigmas, &spec).unwrap_err(),
        FreeFormError::Input(ForceDensityInputError::IndexOutOfRange {
            what: IndexedInput::SurfaceCorner { surface: 0 },
            index: 6,
            len: 6,
        }),
    );
}

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

// A degenerate (collinear / zero-area) INPUT surface triangle is an input
// error, reported instead of assembling a NaN/∞ stencil.
#[test]
fn surfaces_free_degenerate_triangle_is_degenerate_triangle() {
    let (members, kinds) = triplex_topology();
    let guess = perturbed_twisted_prism();
    // Collinear triangle: all three corners on the x-axis → zero area.
    let degenerate_surfaces = vec![(0, 1, 2)]; // nodes 0,1,2 will be collinear
    // Override the guess so nodes 0,1,2 are collinear on the x-axis.
    let mut linear_guess = guess.clone();
    linear_guess[0] = [0.0, 0.0, 0.0];
    linear_guess[1] = [1.0, 0.0, 0.0];
    linear_guess[2] = [2.0, 0.0, 0.0];
    linear_guess[3] = [0.5, 1.0, 0.0];
    linear_guess[4] = [1.5, 1.0, 0.0];
    linear_guess[5] = [1.0, 2.0, 0.0];

    let spec = ForceDensitySpec::GroupRatios {
        group_ids: triplex_group_ids(),
        seed_ratios: vec![-1.0, 1.0, 1.0],
        reference_group: 1,
    };

    assert_eq!(
        form_find_free_surfaces(
            &linear_guess,
            &members,
            &kinds,
            &degenerate_surfaces,
            &[0.2],
            &spec,
        )
        .unwrap_err(),
        FreeFormError::Input(ForceDensityInputError::DegenerateTriangle { surface: 0 }),
    );
}
