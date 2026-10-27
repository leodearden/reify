//! Kernel-free tests for the relate-solve's dimensional conditioning contract —
//! PRD `docs/prds/v0_6/geometry-algebra-solver-unification.md` §7.3 (task #6673).
//!
//! The tolerance ladder is pinned BEHAVIOURALLY against #6653's `==` policy
//! (`scalars_equal`), never by restating a constant's literal value.

use reify_constraints::relate_solve::{
    FrameUnknown, Operand, Pose, RelationInstance, max_relation_exceedance, partition_driving_set,
    pose_from_frame, solve_frame, solve_frame_traced, solve_scaling,
};
use reify_constraints::scaling::{
    RelateTolerance, ResidualUnit, Rung, SolveScaling, ToleranceLadderError, ToleranceRungs,
    UnknownKind,
};
use reify_core::scalar_tolerance::{ANGULAR_REFERENCE_RADIUS_M, LENGTH_FLOOR_M, scalars_equal};
use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{SolveResult, Value};
use reify_test_support::{axis_val, meters, plane_val, point3, vec3_dimensionless};

// ── The tolerance ladder ─────────────────────────────────────────────────────

/// `kernel_local ≤ solver_convergence ≤ assertion/dedup` (PRD §7.1 coherence law),
/// with the metre-row floor under the convergence rung.
#[test]
fn kernel_default_ladder_is_ordered() {
    let tol = RelateTolerance::kernel_default();
    assert!(0.0 < tol.kernel_local(), "{tol:?}");
    assert!(tol.kernel_local() <= tol.solver_convergence(), "{tol:?}");
    assert!(tol.solver_convergence() <= tol.assertion(), "{tol:?}");
    assert!(0.0 < tol.solver_length_floor(), "{tol:?}");
    assert!(
        tol.solver_length_floor() <= tol.solver_convergence(),
        "{tol:?}"
    );
}

/// The solver's metre rung, read at the angular reference radius, IS the `==`
/// verdict's length floor: a gap just inside it compares equal, just outside it
/// does not.
#[test]
fn convergence_rung_is_the_equality_length_floor() {
    let s = RelateTolerance::kernel_default().solver_convergence();
    assert!(scalars_equal(0.0, 0.99 * s, DimensionVector::LENGTH));
    assert!(!scalars_equal(0.0, 1.01 * s, DimensionVector::LENGTH));
    assert_eq!(
        s, LENGTH_FLOOR_M,
        "the convergence rung is based on the `==` floor"
    );
}

/// ε_rel is the `==` verdict's angle floor (PRD §7.3: ε_rel = absolute_floor(ANGLE)).
#[test]
fn relative_convergence_is_the_equality_angle_floor() {
    let tol = RelateTolerance::kernel_default();
    let eps = tol.relative_convergence();
    assert!(scalars_equal(0.0, 0.99 * eps, DimensionVector::ANGLE));
    assert!(!scalars_equal(0.0, 1.01 * eps, DimensionVector::ANGLE));
    assert_eq!(eps, tol.solver_convergence() / ANGULAR_REFERENCE_RADIUS_M);
}

#[test]
fn custom_ladder_is_accepted_and_derives_coherently() {
    let tol = RelateTolerance::from_rungs(ToleranceRungs {
        kernel_local_m: 2e-7,
        solver_convergence_m: 2e-6,
        assertion_m: 2e-5,
        solver_length_floor_m: 1e-9,
    })
    .expect("an ordered, positive, finite ladder is accepted");

    assert_eq!(tol.kernel_local(), 2e-7);
    assert_eq!(tol.solver_convergence(), 2e-6);
    assert_eq!(tol.assertion(), 2e-5);
    assert_eq!(tol.solver_length_floor(), 1e-9);
    assert_eq!(
        tol.relative_convergence(),
        2e-6 / ANGULAR_REFERENCE_RADIUS_M,
        "ε_rel follows the CONFIGURED convergence rung"
    );
    assert_eq!(
        tol.assertion_angle(),
        tol.assertion() / ANGULAR_REFERENCE_RADIUS_M
    );
    assert_eq!(tol.assertion_dimensionless(), tol.assertion_angle().sin());
}

/// A healthy ladder with one rung replaced, so each case below breaks exactly one
/// invariant.
fn rungs() -> ToleranceRungs {
    ToleranceRungs {
        kernel_local_m: 1e-7,
        solver_convergence_m: 1e-6,
        assertion_m: 1e-5,
        solver_length_floor_m: 1e-9,
    }
}

fn refused(rungs: ToleranceRungs) -> ToleranceLadderError {
    match RelateTolerance::from_rungs(rungs) {
        Ok(tol) => panic!("{rungs:?} must be refused, got {tol:?}"),
        Err(e) => e,
    }
}

fn assert_display_names(err: &ToleranceLadderError, values: &[f64]) {
    let msg = err.to_string();
    for v in values {
        assert!(
            msg.contains(&format!("{v:e}")),
            "`{msg}` must state the offending value {v:e}"
        );
    }
}

#[test]
fn incoherent_ladders_are_refused_naming_the_rung() {
    // A non-positive rung.
    let err = refused(ToleranceRungs {
        kernel_local_m: 0.0,
        ..rungs()
    });
    assert_eq!(
        err,
        ToleranceLadderError::NotPositiveFinite {
            rung: Rung::KernelLocal,
            value: 0.0
        }
    );
    assert_display_names(&err, &[0.0]);

    let err = refused(ToleranceRungs {
        solver_length_floor_m: -1e-9,
        ..rungs()
    });
    assert_eq!(
        err,
        ToleranceLadderError::NotPositiveFinite {
            rung: Rung::SolverLengthFloor,
            value: -1e-9
        }
    );
    assert_display_names(&err, &[-1e-9]);

    // A non-finite rung.
    let err = refused(ToleranceRungs {
        assertion_m: f64::INFINITY,
        ..rungs()
    });
    assert_eq!(
        err,
        ToleranceLadderError::NotPositiveFinite {
            rung: Rung::Assertion,
            value: f64::INFINITY
        }
    );
    assert_display_names(&err, &[f64::INFINITY]);

    let err = refused(ToleranceRungs {
        solver_convergence_m: f64::NAN,
        ..rungs()
    });
    assert!(
        matches!(
            err,
            ToleranceLadderError::NotPositiveFinite { rung: Rung::SolverConvergence, value }
                if value.is_nan()
        ),
        "{err:?}"
    );

    // kernel_local > solver_convergence.
    let err = refused(ToleranceRungs {
        kernel_local_m: 2e-6,
        ..rungs()
    });
    assert_eq!(
        err,
        ToleranceLadderError::OutOfOrder {
            lower: Rung::KernelLocal,
            lower_value: 2e-6,
            upper: Rung::SolverConvergence,
            upper_value: 1e-6,
        }
    );
    assert_display_names(&err, &[2e-6, 1e-6]);

    // solver_convergence > assertion.
    let err = refused(ToleranceRungs {
        solver_convergence_m: 2e-5,
        ..rungs()
    });
    assert_eq!(
        err,
        ToleranceLadderError::OutOfOrder {
            lower: Rung::SolverConvergence,
            lower_value: 2e-5,
            upper: Rung::Assertion,
            upper_value: 1e-5,
        }
    );
    assert_display_names(&err, &[2e-5, 1e-5]);

    // solver_length_floor > solver_convergence.
    let err = refused(ToleranceRungs {
        solver_length_floor_m: 2e-6,
        ..rungs()
    });
    assert_eq!(
        err,
        ToleranceLadderError::OutOfOrder {
            lower: Rung::SolverLengthFloor,
            lower_value: 2e-6,
            upper: Rung::SolverConvergence,
            upper_value: 1e-6,
        }
    );
    assert_display_names(&err, &[2e-6, 1e-6]);
}

#[test]
fn absolute_assertion_rung_is_selected_by_row_unit() {
    let tol = RelateTolerance::kernel_default();
    assert_eq!(
        tol.absolute_assertion_rung(ResidualUnit::Length),
        tol.assertion()
    );
    assert_eq!(
        tol.absolute_assertion_rung(ResidualUnit::Angle),
        tol.assertion_angle()
    );
    assert_eq!(
        tol.absolute_assertion_rung(ResidualUnit::Dimensionless),
        tol.assertion_dimensionless()
    );
}

// ── Per-solve scales and tolerances ──────────────────────────────────────────

/// Characteristic lengths from 1e-7 m to 1e4 m, four per decade.
fn log_sweep() -> impl Iterator<Item = f64> {
    (-28..=16).map(|k| 10f64.powf(f64::from(k) / 4.0))
}

fn scaling_at(l: f64) -> SolveScaling {
    SolveScaling::new(l, RelateTolerance::kernel_default())
}

/// Rounding slack for comparing two products that associate differently.
fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * a.abs().max(b.abs())
}

/// A non-finite extent must not become an infinite `L`: that would make the
/// metre-row tolerance infinite and let every row converge vacuously.
#[test]
fn characteristic_length_is_floored_at_kernel_local() {
    let tol = RelateTolerance::kernel_default();
    assert_eq!(
        SolveScaling::new(0.0, tol).characteristic_length(),
        tol.kernel_local()
    );
    assert_eq!(SolveScaling::new(2.0, tol).characteristic_length(), 2.0);
    for extent in [f64::NAN, f64::INFINITY] {
        assert_eq!(
            SolveScaling::new(extent, tol).characteristic_length(),
            tol.kernel_local(),
            "extent {extent}"
        );
    }
    assert_eq!(SolveScaling::new(2.0, tol).tolerance(), tol);
}

#[test]
fn pose_unknowns_scale_by_length_and_one_radian() {
    for l in [1e-3, 0.05, 1.0, 20.0, 2048.0] {
        let scaling = scaling_at(l);
        assert_eq!(scaling.unknown_scale(UnknownKind::PoseTranslation), l);
        assert_eq!(scaling.unknown_scale(UnknownKind::PoseRotation), 1.0);
    }
}

/// PRD §7.3: `t_len = max(ε_rel·L, floor)`, with the crossover `floor / ε_rel` at
/// 1 mm and the as-authored absolute row recovered exactly at `L = R_ref`.
#[test]
fn metre_row_tolerance_is_scale_relative_down_to_one_millimetre() {
    let tol = RelateTolerance::kernel_default();
    let eps = tol.relative_convergence();
    for l in [1e-3 * (1.0 + 1e-9), 2e-3, 0.1, 1.0, 2048.0] {
        assert_eq!(
            scaling_at(l).row_tolerance(ResidualUnit::Length),
            eps * l,
            "L = {l}"
        );
    }
    for l in [9e-4, 1e-4, 1e-7] {
        assert_eq!(
            scaling_at(l).row_tolerance(ResidualUnit::Length),
            tol.solver_length_floor(),
            "L = {l}"
        );
    }
    assert!(
        (eps * 1e-3 - tol.solver_length_floor()).abs() <= 1e-12 * tol.solver_length_floor(),
        "the derived crossover floor / ε_rel is 1 mm"
    );
    assert_eq!(
        scaling_at(ANGULAR_REFERENCE_RADIUS_M).row_tolerance(ResidualUnit::Length),
        tol.solver_convergence()
    );
}

#[test]
fn angular_and_dimensionless_row_tolerance_is_scale_free() {
    let eps = RelateTolerance::kernel_default().relative_convergence();
    for l in log_sweep() {
        let scaling = scaling_at(l);
        assert_eq!(scaling.row_tolerance(ResidualUnit::Angle), eps, "L = {l}");
        assert_eq!(
            scaling.row_tolerance(ResidualUnit::Dimensionless),
            eps,
            "L = {l}"
        );
    }
}

/// PRD §7.3 "Coherence at every scale": the metre assertion rung stays at least
/// ten times the metre-row tolerance at every `L` (exactly ten from `R_ref` up),
/// so a solver-satisfied row never fails its own re-check.
#[test]
fn tolerances_cohere_at_every_scale() {
    for l in log_sweep() {
        let scaling = scaling_at(l);
        let ten_t_len = 10.0 * scaling.row_tolerance(ResidualUnit::Length);
        let rung = scaling.assertion_rung(ResidualUnit::Length);
        assert!(
            ten_t_len <= rung || close(ten_t_len, rung),
            "L = {l}: 10·t_len {ten_t_len:e} > rung {rung:e}"
        );
        if l >= ANGULAR_REFERENCE_RADIUS_M {
            assert!(close(ten_t_len, rung), "L = {l}: {ten_t_len:e} vs {rung:e}");
        }
        for unit in [ResidualUnit::Angle, ResidualUnit::Dimensionless] {
            assert!(
                scaling.row_tolerance(unit) <= scaling.assertion_rung(unit),
                "L = {l}, {unit:?}"
            );
        }
    }
}

#[test]
fn metre_assertion_rung_grows_with_l_above_r_ref() {
    let tol = RelateTolerance::kernel_default();
    assert_eq!(
        scaling_at(0.5).assertion_rung(ResidualUnit::Length),
        tol.assertion()
    );
    let large = scaling_at(20.0);
    assert_eq!(
        large.assertion_rung(ResidualUnit::Length),
        tol.assertion() * 20.0 / ANGULAR_REFERENCE_RADIUS_M
    );
    assert_eq!(
        large.assertion_rung(ResidualUnit::Angle),
        tol.assertion_angle(),
        "the angular rung is not scaled"
    );
    assert_eq!(
        large.assertion_rung(ResidualUnit::Dimensionless),
        tol.assertion_dimensionless(),
        "the dimensionless rung is not scaled"
    );
}

/// PRD §7.3 scalar-auto row (consumer ι #6677): box width if bounded, else
/// `|seed|`, else 1 in its own SI unit. Independent of `L`.
#[test]
fn scalar_auto_scale_follows_box_then_seed_then_unit() {
    for l in [1e-3, 1.0, 2048.0] {
        let scaling = scaling_at(l);
        let scale = |bounds, seed| scaling.unknown_scale(UnknownKind::ScalarAuto { bounds, seed });

        assert_eq!(scale(Some((2.0, 5.5)), None), 3.5);
        assert_eq!(scale(Some((-0.25, 0.75)), Some(40.0)), 1.0);

        assert_eq!(scale(None, Some(-0.02)), 0.02);
        assert_eq!(scale(Some((0.0, f64::INFINITY)), Some(3.0)), 3.0);
        assert_eq!(scale(Some((f64::NAN, 1.0)), Some(-7.0)), 7.0);

        assert_eq!(scale(Some((4.0, 4.0)), None), 1.0);
        assert_eq!(scale(Some((4.0, 4.0)), Some(0.0)), 1.0);
        assert_eq!(scale(None, None), 1.0);
    }
}

// ── Characteristic length of a relation set (PRD §11 Q4) ─────────────────────

/// The moving sub of every fixture below.
const MOVING: &str = "m";

fn moving_unknown() -> FrameUnknown {
    FrameUnknown {
        sub: MOVING.to_string(),
        free: false,
    }
}

fn moving(datum: Value) -> Operand {
    Operand {
        sub: Some(MOVING.to_string()),
        datum,
    }
}

fn anchor(datum: Value) -> Operand {
    Operand {
        sub: Some("a".to_string()),
        datum,
    }
}

fn metric(datum: Value) -> Operand {
    Operand { sub: None, datum }
}

fn rel(name: &str, operands: Vec<Operand>) -> RelationInstance {
    RelationInstance {
        name: name.to_string(),
        operands,
        nominal_delta_dof: None,
    }
}

fn z_axis_at(x: f64, y: f64, z: f64) -> Value {
    axis_val(point3(x, y, z), vec3_dimensionless(0.0, 0.0, 1.0))
}

fn z_plane_at(x: f64, y: f64, z: f64) -> Value {
    plane_val(point3(x, y, z), vec3_dimensionless(0.0, 0.0, 1.0))
}

fn direction(x: f64, y: f64, z: f64) -> Value {
    Value::Direction { x, y, z }
}

fn translated(translation: [f64; 3]) -> Pose {
    Pose {
        translation,
        rotation: [0.0; 3],
    }
}

fn length_of(relations: &[RelationInstance], witness: &Pose) -> f64 {
    solve_scaling(
        relations,
        &moving_unknown(),
        witness,
        RelateTolerance::kernel_default(),
    )
    .characteristic_length()
}

/// `L` is the bounding-box diagonal of the datum origins — moving operands at the
/// witness pose — together with the moving frame's origin, which is its rotation
/// centre.
#[test]
fn extent_is_the_bbox_diagonal_of_datum_origins_and_the_rotation_centre() {
    let relations = [rel(
        "parallel",
        vec![
            moving(z_axis_at(0.003, 0.0, 0.0)),
            anchor(z_plane_at(0.0, 0.004, 0.0)),
        ],
    )];

    // Box {(0.003, 0, 0), (0, 0.004, 0), centre (0, 0, 0)}: 3-4-5 mm.
    let at_identity = length_of(&relations, &Pose::identity());
    assert!(close(at_identity, 0.005), "L = {at_identity}");

    // The witness moves the moving origin to (0.003, 0, 0.012) and the centre to
    // (0, 0, 0.012): box 3 × 4 × 12 mm, diagonal 13 mm.
    let lifted = length_of(&relations, &translated([0.0, 0.0, 0.012]));
    assert!(close(lifted, 0.013), "L = {lifted}");
}

/// Coincident datums 0.05 m from the pose origin still give rotations a 0.05 m
/// lever arm, so `L` is 0.05 m, not the kernel floor.
#[test]
fn rotation_centre_counts_as_a_lever_arm() {
    let relations = [rel(
        "coincident",
        vec![
            moving(point3(0.05, 0.0, 0.0)),
            anchor(point3(0.05, 0.0, 0.0)),
        ],
    )];
    let l = length_of(&relations, &Pose::identity());
    assert!(close(l, 0.05), "L = {l}");
}

/// `L` is NOT invariant under a rigid translation of the model (PRD §11 Q4): the
/// same 10 mm cluster authored 50 m from the moving frame's origin has a 50 m
/// lever arm about it, so `L` is ≈ 50 m and the metre-row bar and metre assertion
/// rung loosen with it. The angular bars do not move.
#[test]
fn extent_spans_the_cluster_distance_from_the_rotation_centre() {
    let tol = RelateTolerance::kernel_default();
    let scaling_with_cluster_at = |x: f64| {
        let relations = [rel(
            "concentric",
            vec![
                moving(z_axis_at(x, 0.0, 0.0)),
                anchor(z_axis_at(x + 0.006, 0.008, 0.0)),
            ],
        )];
        scaling_for(&relations, &moving_unknown(), &Pose::identity())
    };
    let near = scaling_with_cluster_at(0.0);
    let far = scaling_with_cluster_at(50.0);

    let (near_l, far_l) = (near.characteristic_length(), far.characteristic_length());
    assert!(close(near_l, 0.010), "L = {near_l}");
    assert!(close(far_l, 50.006_f64.hypot(0.008)), "L = {far_l}");

    assert!(close(
        far.row_tolerance(ResidualUnit::Length),
        far_l / near_l * near.row_tolerance(ResidualUnit::Length)
    ));
    assert_eq!(near.assertion_rung(ResidualUnit::Length), tol.assertion());
    assert!(close(
        far.assertion_rung(ResidualUnit::Length),
        tol.assertion() * far_l / ANGULAR_REFERENCE_RADIUS_M
    ));
    for unit in [ResidualUnit::Angle, ResidualUnit::Dimensionless] {
        assert_eq!(
            far.row_tolerance(unit),
            near.row_tolerance(unit),
            "{unit:?}"
        );
        assert_eq!(
            far.assertion_rung(unit),
            near.assertion_rung(unit),
            "{unit:?}"
        );
    }
}

/// A DRIVE magnitude is a physical extent of the configuration the solve drives
/// to; an angle is not a length and does not contribute.
#[test]
fn length_metric_operands_set_a_minimum_extent() {
    let distance = [rel(
        "distance",
        vec![
            moving(point3(0.0, 0.0, 0.0)),
            anchor(point3(0.0, 0.0, 0.0)),
            metric(meters(20.0)),
        ],
    )];
    assert_eq!(length_of(&distance, &Pose::identity()), 20.0);

    let angle = [rel(
        "angle",
        vec![
            moving(z_axis_at(0.0, 0.0, 0.0)),
            anchor(z_axis_at(0.0, 0.0, 0.0)),
            metric(Value::Scalar {
                si_value: 0.5,
                dimension: DimensionVector::ANGLE,
            }),
        ],
    )];
    assert_eq!(
        length_of(&angle, &Pose::identity()),
        RelateTolerance::kernel_default().kernel_local()
    );
}

#[test]
fn direction_only_relation_sets_floor_at_kernel_local() {
    let relations = [rel(
        "parallel",
        vec![
            moving(direction(0.0, 0.0, 1.0)),
            anchor(direction(0.0, 0.0, 1.0)),
        ],
    )];
    assert_eq!(
        length_of(&relations, &Pose::identity()),
        RelateTolerance::kernel_default().kernel_local()
    );
}

/// Every term of `L` is degree-1 homogeneous in physical scale. A power-of-two
/// factor makes the rescale exact in IEEE-754, so the comparison is exact.
#[test]
fn extent_scales_exactly_with_physical_scale() {
    let at_scale = |k: f64| {
        let relations = [
            rel(
                "concentric",
                vec![
                    moving(z_axis_at(0.001 * k, 0.0005 * k, 0.002 * k)),
                    anchor(z_axis_at(-0.0015 * k, 0.002 * k, 0.0)),
                ],
            ),
            rel(
                "offset",
                vec![
                    moving(z_plane_at(0.0, 0.0, 0.0015 * k)),
                    anchor(z_plane_at(0.0, 0.0, -0.001 * k)),
                    metric(meters(0.0025 * k)),
                ],
            ),
        ];
        let witness = Pose {
            translation: [0.0005 * k, -0.0003 * k, 0.0002 * k],
            rotation: [0.1, -0.2, 0.15],
        };
        length_of(&relations, &witness)
    };
    let base = at_scale(1.0);
    assert!(base > RelateTolerance::kernel_default().kernel_local());
    assert_eq!(at_scale(1024.0), 1024.0 * base);
}

// ── B15: the scaled solve is invariant under physical rescaling (PRD §8) ─────

/// 2^10, about three decades. A power-of-two rescale is exact in IEEE-754, so each
/// comparison between the two scales below is an arithmetic identity rather than a
/// tuned tolerance: any mismatch is a real scale leak.
const K: f64 = 1024.0;

/// `concentric` (2 dimensionless + 2 metre rows) and `flush` (2 dimensionless + 1
/// metre row) at physical scale `k`. The moving datums sit ~1–1.5 mm off their
/// anchors and the seed is rotated ~0.2 rad about a skew axis, so the solve is
/// nonlinear in metre- AND radian-scale unknowns, with `L` ≈ 2 mm at `k = 1`.
fn b15_fixture(k: f64) -> (Vec<RelationInstance>, FrameUnknown, Pose) {
    let relations = vec![
        rel(
            "concentric",
            vec![
                moving(z_axis_at(0.0012 * k, -0.0008 * k, 0.0)),
                anchor(z_axis_at(0.0, 0.0, 0.0)),
            ],
        ),
        rel(
            "flush",
            vec![
                moving(z_plane_at(0.0012 * k, -0.0008 * k, 0.0015 * k)),
                anchor(z_plane_at(0.0, 0.0, 0.0005 * k)),
            ],
        ),
    ];
    let seed = Pose {
        translation: [0.0005 * k, -0.0003 * k, 0.0004 * k],
        rotation: [0.12, -0.15, 0.08],
    };
    (relations, moving_unknown(), seed)
}

/// The scaling a scope solve of `relations` from `seed` uses.
fn scaling_for(
    relations: &[RelationInstance],
    unknown: &FrameUnknown,
    seed: &Pose,
) -> SolveScaling {
    solve_scaling(relations, unknown, seed, RelateTolerance::kernel_default())
}

fn verdict(result: &SolveResult) -> &'static str {
    match result {
        SolveResult::Solved { .. } => "solved",
        SolveResult::Infeasible { .. } => "infeasible",
        _ => "other",
    }
}

/// The solved pose and `unique` flag of a `Solved` result.
fn solved(result: &SolveResult) -> (Pose, bool) {
    match result {
        SolveResult::Solved { values, unique } => {
            let frame = &values[&ValueCellId::new(MOVING, "pose")];
            (pose_from_frame(frame).expect("a solved Frame"), *unique)
        }
        other => panic!("expected Solved, got {other:?}"),
    }
}

/// Both instances sit above the 1 mm crossover, so the floor is inactive and the
/// fixture exercises the scaling, not the floor.
#[test]
fn b15_precondition_both_instances_are_above_the_crossover() {
    for k in [1.0, K] {
        let (relations, unknown, seed) = b15_fixture(k);
        let scaling = scaling_for(&relations, &unknown, &seed);
        let l = scaling.characteristic_length();
        assert!(l >= 1e-3, "k = {k}: L = {l}");
        assert_eq!(
            scaling.row_tolerance(ResidualUnit::Length),
            scaling.relative_convergence() * l,
            "k = {k}"
        );
    }
}

#[test]
fn b15_convergence_verdict_and_iteration_count_are_scale_invariant() {
    let run = |k: f64| {
        let (relations, unknown, seed) = b15_fixture(k);
        solve_frame_traced(
            &relations,
            &unknown,
            &seed,
            &scaling_for(&relations, &unknown, &seed),
        )
    };
    let (base, base_trace) = run(1.0);
    let (large, large_trace) = run(K);

    let (base_pose, base_unique) = solved(&base);
    let (large_pose, large_unique) = solved(&large);
    assert_eq!(base_unique, large_unique);
    assert_eq!(base_trace.iterations, large_trace.iterations);
    assert!(
        base_trace.iterations >= 2,
        "the stop test must fire mid-sequence, not at the seed: {base_trace:?}"
    );
    assert_eq!(large_pose.rotation, base_pose.rotation);
    for i in 0..3 {
        assert_eq!(
            large_pose.translation[i],
            K * base_pose.translation[i],
            "axis {i}"
        );
    }
}

/// Negative control for the test above: the scaled geometry solved against the
/// BASE scaling — an absolute bar — stops at a different iterate.
#[test]
fn b15_fixture_detects_a_scale_leak() {
    let (base_relations, base_unknown, base_seed) = b15_fixture(1.0);
    let base_scaling = scaling_for(&base_relations, &base_unknown, &base_seed);
    let (relations, unknown, seed) = b15_fixture(K);
    let own_scaling = scaling_for(&relations, &unknown, &seed);

    let (scaled, scaled_trace) = solve_frame_traced(&relations, &unknown, &seed, &own_scaling);
    let (leaky, leaky_trace) = solve_frame_traced(&relations, &unknown, &seed, &base_scaling);
    assert_ne!(
        (verdict(&leaky), leaky_trace.iterations),
        (verdict(&scaled), scaled_trace.iterations),
        "an absolute bar must change the stop iterate, or the B15 fixture cannot see a leak"
    );
}

#[test]
fn b15_infeasible_verdict_is_scale_invariant() {
    let run = |k: f64| {
        let relations = vec![
            rel(
                "concentric",
                vec![
                    moving(z_axis_at(0.0, 0.0012 * k, 0.0)),
                    anchor(z_axis_at(0.0, 0.0, 0.0)),
                ],
            ),
            rel(
                "concentric",
                vec![
                    moving(z_axis_at(0.0, 0.0012 * k, 0.0)),
                    anchor(z_axis_at(0.0008 * k, 0.0, 0.0)),
                ],
            ),
        ];
        let unknown = moving_unknown();
        let seed = Pose {
            translation: [0.0002 * k, -0.0004 * k, 0.0],
            rotation: [0.05, 0.1, -0.03],
        };
        let scaling = scaling_for(&relations, &unknown, &seed);
        assert!(scaling.characteristic_length() >= 1e-3, "k = {k}");
        solve_frame_traced(&relations, &unknown, &seed, &scaling)
    };
    let (base, base_trace) = run(1.0);
    let (large, large_trace) = run(K);
    assert_eq!(verdict(&base), "infeasible");
    assert_eq!(verdict(&large), "infeasible");
    assert_eq!(base_trace.iterations, large_trace.iterations);
}

#[test]
fn b15_partition_is_scale_invariant() {
    let run = |k: f64| {
        let (mut relations, unknown, seed) = b15_fixture(k);
        relations.push(rel(
            "parallel",
            vec![
                moving(z_axis_at(0.0012 * k, -0.0008 * k, 0.0)),
                anchor(z_axis_at(0.0, 0.0, 0.0)),
            ],
        ));
        let scaling = scaling_for(&relations, &unknown, &seed);
        partition_driving_set(&relations, &unknown, &seed, &scaling)
    };
    let base = run(1.0);
    let large = run(K);
    assert_eq!(
        base.redundant,
        vec![2],
        "`parallel` repeats concentric's tilt rows"
    );
    assert_eq!(large.driving, base.driving);
    assert_eq!(large.redundant, base.redundant);
    assert_eq!((large.spent, large.free), (base.spent, base.free));
    for (l, b) in large.per_relation.iter().zip(&base.per_relation) {
        assert_eq!(
            (l.individual_rank, l.rank_contribution),
            (b.individual_rank, b.rank_contribution),
            "{}",
            b.name
        );
    }
}

/// Below the 1 mm crossover the metre rows take the floor tolerance, and the
/// damping's nominal curvature over-states theirs. At `L` ≈ 0.18 mm the solve
/// still converges in no more steps than the same geometry above the crossover.
#[test]
fn sub_millimetre_scope_converges_as_fast_as_above_the_crossover() {
    let run = |k: f64| {
        let (relations, unknown, seed) = b15_fixture(k);
        let scaling = scaling_for(&relations, &unknown, &seed);
        let (result, trace) = solve_frame_traced(&relations, &unknown, &seed, &scaling);
        (relations, unknown, scaling, result, trace)
    };
    let (.., above) = run(1.0);
    let (relations, unknown, scaling, result, below) = run(1.0 / 16.0);

    let l = scaling.characteristic_length();
    assert!(l < 1e-3, "L = {l} must sit below the crossover");
    assert_eq!(
        scaling.row_tolerance(ResidualUnit::Length),
        scaling.tolerance().solver_length_floor(),
        "L = {l}: the metre rows must take the floor tolerance"
    );

    let (pose, _) = solved(&result);
    let exceedance =
        max_relation_exceedance(&relations, &unknown, &pose, |u| scaling.row_tolerance(u));
    assert!(exceedance <= 1.0, "max|r̃| = {exceedance}");
    assert!(
        below.iterations <= above.iterations,
        "sub-mm {below:?} vs above the crossover {above:?}"
    );
}

/// The scaled contract: a `Solved` pose has every row within its own nominal
/// tolerance, i.e. `max|r̃| ≤ 1`.
#[test]
fn solved_pose_meets_every_row_tolerance() {
    for k in [1.0, K] {
        let (relations, unknown, seed) = b15_fixture(k);
        let scaling = scaling_for(&relations, &unknown, &seed);
        let (pose, _) = solved(&solve_frame(&relations, &unknown, &seed, &scaling));
        let exceedance =
            max_relation_exceedance(&relations, &unknown, &pose, |u| scaling.row_tolerance(u));
        assert!(exceedance <= 1.0, "k = {k}: max|r̃| = {exceedance}");
    }
}

/// The redundant-remainder re-check's metre rung scales with `L`; its angular
/// rungs do not.
#[test]
fn redundant_metre_row_is_judged_against_the_scaled_assertion_rung() {
    let tol = RelateTolerance::kernel_default();
    let exceedance = |relation: RelationInstance, scaling: SolveScaling| {
        max_relation_exceedance(&[relation], &moving_unknown(), &Pose::identity(), |u| {
            scaling.assertion_rung(u)
        })
    };
    let offset_by = |moving_plane: Value| {
        rel(
            "offset",
            vec![
                moving(moving_plane),
                anchor(z_plane_at(0.0, 0.0, 0.0)),
                metric(meters(20.0)),
            ],
        )
    };

    // A 5e-5 m metre error with exact angular rows.
    let long = offset_by(z_plane_at(0.0, 0.0, 20.00005));
    assert!(exceedance(long.clone(), SolveScaling::new(20.0, tol)) <= 1.0);
    assert!(exceedance(long, SolveScaling::new(0.5, tol)) > 1.0);

    // A 5e-5 rad tilt with an exact metre row.
    let tilt = 5e-5_f64;
    let tilted = offset_by(plane_val(
        point3(0.0, 0.0, 20.0),
        vec3_dimensionless(tilt.sin(), 0.0, tilt.cos()),
    ));
    assert!(exceedance(tilted, SolveScaling::new(20.0, tol)) > 1.0);
}
