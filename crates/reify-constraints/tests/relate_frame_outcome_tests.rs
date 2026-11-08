//! The relate Frame solve's typed outcome — task #6675 (solver-unification η),
//! PRD `docs/prds/v0_6/geometry-algebra-solver-unification.md` §7.5 / §7.7.
//!
//! Kernel-free, over synthetic realized datums: why a Frame solve stopped (a typed
//! `SolveFailureCause`), the rank and null-space basis a converged solve
//! publishes, the rotation-magnitude wrap, and stationary maxima that are not
//! infeasibility certificates.

mod common;

use common::{
    axis, b5_relations, bolt_unknown, datum, dir, fully_determined_relations, plane, point3,
    relation, scaling_for, solve_frame_result, unknown,
};
use reify_constraints::relate_solve::{
    FrameSolveOutcome, FrameUnknown, Pose, RelationInstance, Twist, max_relation_exceedance,
    pose_from_frame, solve_frame_outcome,
};
use reify_constraints::scaling::SolveScaling;
use reify_constraints::trust_region::{RANGE_COSINE_TOLERANCE, SolveBudget};
use reify_core::DiagnosticCode;
use reify_ir::{NonFiniteEntry, ResidualSource, SolveFailureCause, SolveResult, SolveStall, Value};

// ── η #6675: typed termination causes and the null-space basis ───────────────
//
// `solve_frame_outcome` exposes WHY a Frame solve stopped as a typed
// `SolveFailureCause`, and a converged solve's rank and null space from the same
// factorization that produced its last step.

/// Two `concentric` relations from the SAME moving axis to two PARALLEL anchor
/// axes 0.8 mm apart — no pose satisfies both (relate_scaling_tests.rs's B15
/// infeasible geometry). The residual is linear in translation, so the solve
/// reaches the least-squares minimum (the midpoint), where the gradient vanishes.
fn two_parallel_concentrics() -> (Vec<RelationInstance>, FrameUnknown, Pose) {
    let moving = axis((0.0, 0.0012, 0.0), (0.0, 0.0, 1.0));
    let relations = vec![
        relation(
            "concentric",
            vec![
                datum("bolt", moving.clone()),
                datum("plate", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            ],
            4,
        ),
        relation(
            "concentric",
            vec![
                datum("bolt", moving),
                datum("plate", axis((0.0008, 0.0, 0.0), (0.0, 0.0, 1.0))),
            ],
            4,
        ),
    ];
    let seed = Pose {
        translation: [0.0002, -0.0004, 0.0],
        rotation: [0.05, 0.1, -0.03],
    };
    (relations, bolt_unknown(), seed)
}

/// `concentric` + `flush` with the moving datums ~1–1.5 mm off their anchors and a
/// ~0.2 rad skew seed rotation: feasible, but nonlinear in both translation and
/// rotation (relate_scaling_tests.rs's B15 fixture at `k = 1`, which needs ≥ 2
/// trial steps).
fn skewed_concentric_flush() -> (Vec<RelationInstance>, FrameUnknown, Pose) {
    let relations = vec![
        relation(
            "concentric",
            vec![
                datum("bolt", axis((0.0012, -0.0008, 0.0), (0.0, 0.0, 1.0))),
                datum("plate", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            ],
            4,
        ),
        relation(
            "flush",
            vec![
                datum("bolt", plane((0.0012, -0.0008, 0.0015), (0.0, 0.0, 1.0))),
                datum("plate", plane((0.0, 0.0, 0.0005), (0.0, 0.0, 1.0))),
            ],
            3,
        ),
    ];
    let seed = Pose {
        translation: [0.0005, -0.0003, 0.0004],
        rotation: [0.12, -0.15, 0.08],
    };
    (relations, bolt_unknown(), seed)
}

fn outcome_of(
    (relations, unknown, seed): &(Vec<RelationInstance>, FrameUnknown, Pose),
    budget: SolveBudget,
) -> FrameSolveOutcome {
    let scaling = scaling_for(relations, unknown, seed);
    solve_frame_outcome(relations, unknown, seed, &scaling, budget)
}

fn failure_of(outcome: &FrameSolveOutcome) -> &SolveFailureCause {
    match &outcome.result {
        Err(cause) => cause,
        Ok(solution) => panic!("expected a failed solve, got {solution:?}"),
    }
}

/// The adapter reports a non-Infeasible failure as `NoProgress`, rendering the
/// cause and naming the unknown's sub; returns the reason.
fn assert_reports_no_progress(outcome: FrameSolveOutcome, unknown: &FrameUnknown) -> String {
    let cause = failure_of(&outcome).to_string();
    match outcome.into_solve_result(unknown) {
        SolveResult::NoProgress { reason } => {
            assert!(
                reason.contains(&cause),
                "the reason renders the cause's own figures: {reason}"
            );
            assert!(
                reason.contains(&format!("`{}`", unknown.sub)),
                "the reason names the sub: {reason}"
            );
            reason
        }
        other => panic!("expected NoProgress, got {other:?}"),
    }
}

/// B9 at the Frame-solve seam: an inconsistent driving set is CERTIFIED
/// infeasible, and both relations are blamed.
#[test]
fn inconsistent_driving_set_is_certified_infeasible() {
    let outcome = outcome_of(&two_parallel_concentrics(), SolveBudget::default());
    let SolveFailureCause::Infeasible(cert) = failure_of(&outcome) else {
        panic!("expected Infeasible, got {outcome:?}");
    };
    assert!(
        cert.range_cosine <= RANGE_COSINE_TOLERANCE,
        "the certificate must clear the published range-cosine tolerance: {cert:?}"
    );
    assert!(cert.max_scaled_residual > 1.0, "{cert:?}");
    for relation in [0, 1] {
        assert!(
            cert.blame
                .iter()
                .any(|b| b.source == ResidualSource::Relation(relation)),
            "relation {relation} must be blamed: {cert:?}"
        );
    }
}

/// B9 at the Frame-solve seam: a feasible set that needs more than one step,
/// given one, exhausts its budget — a different cause, with a different code,
/// from the infeasibility above.
#[test]
fn a_one_step_budget_is_exhausted_not_infeasible() {
    let fixture = skewed_concentric_flush();
    let outcome = outcome_of(&fixture, SolveBudget { max_iterations: 1 });
    let SolveFailureCause::BudgetExhausted(exhaustion) = failure_of(&outcome) else {
        panic!("expected BudgetExhausted, got {outcome:?}");
    };
    assert_eq!(exhaustion.iterations, 1);
    assert_eq!(exhaustion.budget, 1);
    assert_eq!(exhaustion.step_norms.len(), 1);
    assert_eq!(outcome.iterations, 1);

    let unbounded = outcome_of(&fixture, SolveBudget::default());
    assert!(unbounded.result.is_ok(), "{unbounded:?}");

    let infeasible = outcome_of(&two_parallel_concentrics(), SolveBudget::default());
    let infeasible = failure_of(&infeasible);
    assert!(
        matches!(infeasible, SolveFailureCause::Infeasible(_)),
        "{infeasible:?}"
    );
    assert_ne!(infeasible.code(), failure_of(&outcome).code());
}

/// The `SolveResult` adapter keeps that contract: a certified infeasibility is a
/// CODED `Infeasible`; budget exhaustion is `NoProgress` naming the counts.
#[test]
fn solve_result_adapter_codes_infeasible_and_reports_budget_as_no_progress() {
    let (relations, unknown, seed) = two_parallel_concentrics();
    let scaling = scaling_for(&relations, &unknown, &seed);
    match solve_frame_result(&relations, &unknown, &seed, &scaling) {
        SolveResult::Infeasible { diagnostics } => {
            assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
            assert_eq!(diagnostics[0].code, Some(DiagnosticCode::SolverInfeasible));
        }
        other => panic!("expected Infeasible, got {other:?}"),
    }

    let fixture = skewed_concentric_flush();
    let outcome = outcome_of(&fixture, SolveBudget { max_iterations: 1 });
    let reason = assert_reports_no_progress(outcome, &fixture.1);
    assert!(
        reason.contains("1 of 1"),
        "the reason names the counts: {reason}"
    );
}

/// A stall is a search failure, so the adapter reports `NoProgress` rendering
/// it, never a coded `Infeasible`.
#[test]
fn solve_result_adapter_reports_a_stall_as_no_progress() {
    let outcome = FrameSolveOutcome {
        iterations: 7,
        result: Err(SolveFailureCause::Stalled(SolveStall {
            iterations: 7,
            step_norm: 1e-12,
            range_cosine: 0.5,
            max_scaled_residual: 40.0,
        })),
    };
    assert_reports_no_progress(outcome, &bolt_unknown());
}

/// The off-axis size of a 3-vector expected to lie along `+z`.
fn off_z(v: [f64; 3]) -> f64 {
    v[0].hypot(v[1])
}

/// The null-space basis #4388's DOF ledger names: strict `concentric` alone leaves
/// slide along and spin about the axis, and nothing else.
#[test]
fn concentric_alone_publishes_slide_and_spin_as_its_null_space() {
    let relations = b5_relations();
    let unknown = bolt_unknown();
    let seed = Pose::identity();
    let scaling = scaling_for(&relations, &unknown, &seed);
    let outcome = solve_frame_outcome(
        &relations,
        &unknown,
        &seed,
        &scaling,
        SolveBudget::default(),
    );
    let solution = outcome.result.expect("a lone concentric is feasible");
    assert_eq!(solution.rank, 4);
    assert_eq!(solution.null_space.len(), 2);
    let length = scaling.characteristic_length();
    for Twist {
        translation,
        rotation,
    } in &solution.null_space
    {
        assert!(off_z(*translation) <= 1e-9 * length, "{translation:?}");
        assert!(off_z(*rotation) <= 1e-9, "{rotation:?}");
    }
    let slide = solution
        .null_space
        .iter()
        .map(|t| t.translation[2].abs())
        .fold(0.0, f64::max);
    let spin = solution
        .null_space
        .iter()
        .map(|t| t.rotation[2].abs())
        .fold(0.0, f64::max);
    assert!(
        slide > 0.1 * length && spin > 0.1,
        "the twists span slide AND spin: {solution:?}"
    );
}

/// The twists are geometric, not exp-map chart increments: with the anchor axis
/// tilted 40° about x the solve lands on a non-zero rotation, and every twist
/// still slides along / spins about the TILTED axis (a chart increment would sit
/// θ/2 = 20° off it).
#[test]
fn a_rotated_solution_publishes_twists_about_the_anchor_axis() {
    let tilt = 40f64.to_radians();
    let anchor = (0.0, -tilt.sin(), tilt.cos());
    let relations = vec![relation(
        "concentric",
        vec![
            datum("bolt", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            datum("plate", axis((0.0, 0.0, 0.0), anchor)),
        ],
        4,
    )];
    let unknown = bolt_unknown();
    let seed = Pose::identity();
    let scaling = scaling_for(&relations, &unknown, &seed);
    let outcome = solve_frame_outcome(
        &relations,
        &unknown,
        &seed,
        &scaling,
        SolveBudget::default(),
    );
    let solution = outcome.result.expect("a lone concentric is feasible");
    assert!(
        solution.pose.rotation[0].abs() > 0.5,
        "the fixture must solve to a non-trivial rotation: {solution:?}"
    );
    assert_eq!(solution.null_space.len(), 2);
    let axis = [anchor.0, anchor.1, anchor.2];
    let off_axis = |v: [f64; 3]| {
        let along = v[0] * axis[0] + v[1] * axis[1] + v[2] * axis[2];
        let rest = [
            v[0] - along * axis[0],
            v[1] - along * axis[1],
            v[2] - along * axis[2],
        ];
        (rest[0] * rest[0] + rest[1] * rest[1] + rest[2] * rest[2]).sqrt()
    };
    let length = scaling.characteristic_length();
    for twist in &solution.null_space {
        assert!(off_axis(twist.rotation) <= 1e-6, "{twist:?}");
        assert!(off_axis(twist.translation) <= 1e-6 * length, "{twist:?}");
    }
    let spin = solution
        .null_space
        .iter()
        .map(|t| norm(t.rotation))
        .fold(0.0, f64::max);
    assert!(spin > 0.1, "the twists span a spin: {solution:?}");
}

fn norm(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

#[test]
fn fully_determined_set_has_rank_six_and_no_null_space() {
    let relations = fully_determined_relations();
    for free in [false, true] {
        let unknown = unknown("m", free);
        let seed = Pose::identity();
        let scaling = scaling_for(&relations, &unknown, &seed);
        let outcome = solve_frame_outcome(
            &relations,
            &unknown,
            &seed,
            &scaling,
            SolveBudget::default(),
        );
        let solution = outcome.result.expect("feasible");
        assert_eq!(solution.rank, 6);
        assert!(solution.null_space.is_empty());
        match solve_frame_result(&relations, &unknown, &seed, &scaling) {
            SolveResult::Solved { unique, .. } => assert_eq!(unique, !free),
            other => panic!("expected Solved, got {other:?}"),
        }
    }
}

// ── η #6675: the rotation-magnitude wrap ─────────────────────────────────────
//
// The exponential map is singular at |θ| = 2π, and a magnitude past π names the
// same rotation as its complement. The solve keeps its rotation in (−π, π], so
// the published Frame is canonical and the rank is never read at the singular
// radius — observed through the solved Frame and the uniqueness verdict only.

/// The `[w, x, y, z]` quaternion of a solved `Value::Frame`.
fn frame_quaternion(frame: &Value) -> [f64; 4] {
    match frame {
        Value::Frame { basis, .. } => match **basis {
            Value::Orientation { w, x, y, z } => [w, x, y, z],
            ref other => panic!("a Frame basis is an Orientation, got {other:?}"),
        },
        other => panic!("expected a Frame, got {other:?}"),
    }
}

/// `v` rotated by the unit quaternion `q`.
fn quaternion_rotate(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let [w, x, y, z] = q;
    let u = [x, y, z];
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let t = cross(u, v).map(|c| 2.0 * c);
    let ut = cross(u, t);
    [0, 1, 2].map(|i| v[i] + w * t[i] + ut[i])
}

/// `v` rotated by the exponential-map vector `theta`.
fn exp_map_rotate(theta: [f64; 3], v: [f64; 3]) -> [f64; 3] {
    let angle = (theta[0] * theta[0] + theta[1] * theta[1] + theta[2] * theta[2]).sqrt();
    let half = 0.5 * angle;
    let s = half.sin() / angle;
    quaternion_rotate([half.cos(), theta[0] * s, theta[1] * s, theta[2] * s], v)
}

fn solved_frame(result: SolveResult, sub: &str) -> (Value, bool) {
    match result {
        SolveResult::Solved { values, unique } => {
            let frame = values
                .get(&reify_core::ValueCellId::new(sub, "pose"))
                .expect("the solved Frame")
                .clone();
            (frame, unique)
        }
        other => panic!("expected Solved, got {other:?}"),
    }
}

/// A gauge spin seeded past π comes back as the same physical rotation with
/// magnitude ≤ π, and a quaternion with `w ≥ 0`.
#[test]
fn a_gauge_rotation_seeded_past_pi_is_published_canonically() {
    let relations = b5_relations();
    let unknown = bolt_unknown();
    let seed = Pose {
        translation: [0.0, 0.0, 0.0],
        rotation: [0.0, 0.0, 3.5],
    };
    let scaling = scaling_for(&relations, &unknown, &seed);
    let (frame, _) = solved_frame(
        solve_frame_result(&relations, &unknown, &seed, &scaling),
        "bolt",
    );

    let q = frame_quaternion(&frame);
    assert!(q[0] >= 0.0, "a canonical quaternion has w ≥ 0: {q:?}");
    let rotation = pose_from_frame(&frame).expect("a Pose").rotation;
    let magnitude = rotation.iter().map(|c| c * c).sum::<f64>().sqrt();
    assert!(
        magnitude <= std::f64::consts::PI,
        "|θ| = {magnitude}: {rotation:?}"
    );
    assert!(
        (rotation[2] - (3.5 - 2.0 * std::f64::consts::PI)).abs() <= 1e-9,
        "{rotation:?}"
    );

    for probe in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
        let solved = quaternion_rotate(q, probe);
        let seeded = exp_map_rotate(seed.rotation, probe);
        for i in 0..3 {
            assert!(
                (solved[i] - seeded[i]).abs() <= 1e-9,
                "the physical placement is the seed's: {solved:?} vs {seeded:?}"
            );
        }
    }
}

/// A fully pinning strict set seeded 0.05 rad short of a full turn is solved
/// uniquely: the exp-map singularity at |θ| = 2π never reaches the rank verdict.
#[test]
fn a_seed_near_a_full_turn_is_not_misreported_as_under_determined() {
    let relations = vec![
        relation(
            "concentric",
            vec![
                datum("bolt", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
                datum("plate", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            ],
            4,
        ),
        relation(
            "flush",
            vec![
                datum("bolt", plane((0.0, 0.0, 0.005), (0.0, 0.0, 1.0))),
                datum("plate", plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            ],
            3,
        ),
        relation(
            "perpendicular",
            vec![
                datum("bolt", dir(1.0, 0.0, 0.0)),
                datum("plate", dir(0.0, 1.0, 0.0)),
            ],
            1,
        ),
    ];
    let unknown = bolt_unknown();
    let seed = Pose {
        translation: [0.0, 0.0, 0.0],
        rotation: [0.0, 0.0, 2.0 * std::f64::consts::PI - 0.05],
    };
    let scaling = scaling_for(&relations, &unknown, &seed);

    let (_, unique) = solved_frame(
        solve_frame_result(&relations, &unknown, &seed, &scaling),
        "bolt",
    );
    assert!(unique, "concentric + flush + perpendicular pins all 6 DOF");
    let solution = solve_frame_outcome(
        &relations,
        &unknown,
        &seed,
        &scaling,
        SolveBudget::default(),
    )
    .result
    .expect("feasible");
    assert_eq!(solution.rank, 6, "{solution:?}");
}

// ── η #6675: stationary maxima are not certificates ─────────────────────────
//
// A relation's tilt row is bounded (a sine or cosine of the misalignment), so a
// seed that puts it at its MAXIMUM is a first-order stationary point that is not
// a root, although a feasible pose exists. Only a genuine least-squares minimum
// may be certified infeasible.

/// Solve `relations` for the bolt from the identity, with the default budget.
fn solve_from_identity(relations: &[RelationInstance]) -> (FrameSolveOutcome, SolveScaling) {
    let unknown = bolt_unknown();
    let seed = Pose::identity();
    let scaling = scaling_for(relations, &unknown, &seed);
    let outcome = solve_frame_outcome(relations, &unknown, &seed, &scaling, SolveBudget::default());
    (outcome, scaling)
}

/// The solve converged, and every relation holds at the solved pose.
fn assert_seated(
    relations: &[RelationInstance],
    outcome: &FrameSolveOutcome,
    scaling: &SolveScaling,
) {
    let solution = outcome
        .result
        .as_ref()
        .unwrap_or_else(|cause| panic!("a feasible set must converge, got {cause:?}"));
    let exceedance = max_relation_exceedance(relations, &bolt_unknown(), &solution.pose, |u| {
        scaling.row_tolerance(u)
    });
    assert!(exceedance <= 1.0, "{exceedance} at {solution:?}");
}

/// The shank along `z` concentric with a hole drilled along `x`: the offsets
/// already hold, and the tilt row sits at its maximum.
#[test]
fn a_side_drilled_hole_is_not_certified_infeasible() {
    let relations = vec![relation(
        "concentric",
        vec![
            datum("bolt", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            datum("plate", axis((0.0, 0.0, 0.0), (1.0, 0.0, 0.0))),
        ],
        4,
    )];
    let (outcome, scaling) = solve_from_identity(&relations);
    assert_seated(&relations, &outcome, &scaling);
}

/// A seat facing `z` flush to a wall facing `x`: the normal row sits at its
/// maximum.
#[test]
fn a_seat_flush_to_a_side_wall_is_not_certified_infeasible() {
    let relations = vec![relation(
        "flush",
        vec![
            datum("bolt", plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            datum("plate", plane((0.0, 0.0, 0.0), (1.0, 0.0, 0.0))),
        ],
        3,
    )];
    let (outcome, scaling) = solve_from_identity(&relations);
    assert_seated(&relations, &outcome, &scaling);
}

/// GUARD: one seat flush to BOTH a top facing `z` and a wall facing `x`. Its tilt
/// cost (sin² + cos² of the rotation about `y`) is exactly flat along the valley,
/// so the stationary point is a genuine minimum and stays certified.
#[test]
fn perpendicular_top_and_wall_flush_stays_certified() {
    let seat = plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0));
    let relations = vec![
        relation(
            "flush",
            vec![
                datum("bolt", seat.clone()),
                datum("plate", plane((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            ],
            3,
        ),
        relation(
            "flush",
            vec![
                datum("bolt", seat),
                datum("plate", plane((0.0, 0.0, 0.0), (1.0, 0.0, 0.0))),
            ],
            3,
        ),
    ];
    let (outcome, _) = solve_from_identity(&relations);
    let SolveFailureCause::Infeasible(cert) = failure_of(&outcome) else {
        panic!("expected Infeasible, got {outcome:?}");
    };
    assert!(cert.range_cosine <= RANGE_COSINE_TOLERANCE, "{cert:?}");
    assert!(
        cert.blame
            .iter()
            .any(|b| b.source == ResidualSource::Relation(1)),
        "the wall flush must be blamed: {cert:?}"
    );
}

// ── η #6675: a non-finite evaluation names its relation ──────────────────────
//
// A residual row that evaluates to NaN ends the solve as a typed `NonFinite`
// cause naming the relation that produced the row. The solve scaling drops
// non-finite points, so a NaN anchor datum reaches no row but its own relation's.

/// A `concentric` already seated at the seed, beside a `coincident` whose plate
/// point has a NaN coordinate: relation 0's four rows are finite, so the first
/// non-finite row is relation 1's first.
fn nan_anchor_beside_a_seated_concentric() -> (Vec<RelationInstance>, FrameUnknown, Pose) {
    let relations = vec![
        relation(
            "concentric",
            vec![
                datum("bolt", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
                datum("plate", axis((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            ],
            4,
        ),
        relation(
            "coincident",
            vec![
                datum("bolt", point3(0.0, 0.0, 0.0)),
                datum("plate", point3(f64::NAN, 0.0, 0.0)),
            ],
            3,
        ),
    ];
    (relations, bolt_unknown(), Pose::identity())
}

/// The non-finite row is attributed to the relation that produced it. Row 4 is the
/// first row past the seated `concentric`'s four, the boundary where an off-by-one
/// in the cumulative row count would name relation 0 instead.
#[test]
fn a_nan_anchor_is_a_non_finite_evaluation_of_its_own_relation() {
    let outcome = outcome_of(
        &nan_anchor_beside_a_seated_concentric(),
        SolveBudget::default(),
    );
    let SolveFailureCause::NonFinite(found) = failure_of(&outcome) else {
        panic!("expected NonFinite, got {outcome:?}");
    };
    assert_eq!(found.source, ResidualSource::Relation(1), "{found:?}");
    assert_eq!(found.row, 4, "past relation 0's four rows: {found:?}");
    assert_eq!(found.entry, NonFiniteEntry::Residual, "{found:?}");
    assert!(found.value.is_nan(), "{found:?}");
    assert_eq!(found.iterations, 0, "{found:?}");
    assert_eq!(outcome.iterations, 0, "{outcome:?}");
}

/// A non-finite evaluation is a search failure, so the adapter reports
/// `NoProgress` rendering it, never a coded `Infeasible`.
#[test]
fn solve_result_adapter_reports_a_non_finite_evaluation_as_no_progress() {
    let fixture = nan_anchor_beside_a_seated_concentric();
    let outcome = outcome_of(&fixture, SolveBudget::default());
    let cause = failure_of(&outcome);
    assert!(
        matches!(cause, SolveFailureCause::NonFinite(_)),
        "the fixture must end non-finite: {cause:?}"
    );
    assert_reports_no_progress(outcome, &fixture.1);
}
