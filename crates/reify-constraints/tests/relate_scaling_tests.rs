//! Kernel-free tests for the relate-solve's dimensional conditioning contract —
//! PRD `docs/prds/v0_6/geometry-algebra-solver-unification.md` §7.3 (task #6673).
//!
//! The tolerance ladder is pinned BEHAVIOURALLY against #6653's `==` policy
//! (`scalars_equal`), never by restating a constant's literal value.

use reify_constraints::scaling::{
    RelateTolerance, ResidualUnit, Rung, ToleranceLadderError, ToleranceRungs,
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
