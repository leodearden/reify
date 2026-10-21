//! Kernel-free tests for the relate-solve's dimensional conditioning contract —
//! PRD `docs/prds/v0_6/geometry-algebra-solver-unification.md` §7.3 (task #6673).
//!
//! The tolerance ladder is pinned BEHAVIOURALLY against #6653's `==` policy
//! (`scalars_equal`), never by restating a constant's literal value.

use reify_constraints::scaling::{
    RelateTolerance, ResidualUnit, Rung, SolveScaling, ToleranceLadderError, ToleranceRungs,
    UnknownKind,
};
use reify_core::DimensionVector;
use reify_core::scalar_tolerance::{ANGULAR_REFERENCE_RADIUS_M, LENGTH_FLOOR_M, scalars_equal};

// ── The tolerance ladder ─────────────────────────────────────────────────────

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
fn assertion_rung_is_selected_by_row_unit() {
    let tol = RelateTolerance::kernel_default();
    assert_eq!(tol.assertion_rung(ResidualUnit::Length), tol.assertion());
    assert_eq!(
        tol.assertion_rung(ResidualUnit::Angle),
        tol.assertion_angle()
    );
    assert_eq!(
        tol.assertion_rung(ResidualUnit::Dimensionless),
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
