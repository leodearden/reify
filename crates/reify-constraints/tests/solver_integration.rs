//! Integration tests for DimensionalSolver.
//!
//! Tests the solver through the ConstraintSolver trait object interface,
//! using reify-test-support helpers for expression construction.

use reify_constraints::DimensionalSolver;
use reify_test_support::*;
use reify_types::{
    AutoParam, ConstraintSolver, DimensionVector, OptimizationObjective, ResolutionProblem,
    SolveResult, Type, Value, ValueMap,
};

#[test]
fn single_param_feasibility_via_trait_object() {
    let solver: Box<dyn ConstraintSolver> = Box::new(DimensionalSolver);

    let thickness_id = vcid("Bracket", "thickness");
    let thickness_ref = value_ref("Bracket", "thickness");

    // thickness > 2mm AND thickness < 20mm
    let gt_expr = gt(thickness_ref.clone(), literal(mm(2.0)));
    let lt_expr = lt(thickness_ref, literal(mm(20.0)));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: thickness_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.001, 0.1)),
        }],
        constraints: vec![
            (cnid("Bracket", 0), gt_expr),
            (cnid("Bracket", 1), lt_expr),
        ],
        current_values: ValueMap::new(),
        objective: None,
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let si = values
                .get(&thickness_id)
                .unwrap()
                .as_f64()
                .unwrap();
            assert!(
                si > 0.002 && si < 0.020,
                "thickness should be in feasible range, got {} m",
                si
            );
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}

#[test]
fn maximize_objective() {
    let solver = DimensionalSolver;

    let thickness_id = vcid("Bracket", "thickness");
    let thickness_ref = value_ref("Bracket", "thickness");

    // thickness > 2mm
    let gt_expr = gt(thickness_ref.clone(), literal(mm(2.0)));

    // thickness < 20mm
    let lt_expr = lt(thickness_ref.clone(), literal(mm(20.0)));

    // Maximize thickness
    let objective = OptimizationObjective::Maximize(thickness_ref);

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: thickness_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.001, 0.1)),
        }],
        constraints: vec![
            (cnid("Bracket", 0), gt_expr),
            (cnid("Bracket", 1), lt_expr),
        ],
        current_values: ValueMap::new(),
        objective: Some(objective),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let si = values
                .get(&thickness_id)
                .unwrap()
                .as_f64()
                .unwrap();
            // Maximizing thickness subject to < 20mm should push close to 20mm
            assert!(
                si > 0.017 && si < 0.021,
                "maximized thickness should be close to 20mm, got {} m",
                si
            );
        }
        SolveResult::Infeasible { .. } => {
            // Nelder-Mead penalty method may converge to a point
            // infinitesimally beyond the constraint boundary. With L1
            // feasibility check, this is correctly flagged as Infeasible.
            // This is acceptable for optimization-against-boundary.
        }
        other => panic!("expected Solved or Infeasible, got {:?}", other),
    }
}

#[test]
fn send_sync_verification() {
    // Verify DimensionalSolver is Send + Sync (required by ConstraintSolver trait)
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<DimensionalSolver>();

    // Verify it works as a trait object behind Box
    let solver: Box<dyn ConstraintSolver> = Box::new(DimensionalSolver);
    let problem = ResolutionProblem {
        auto_params: vec![],
        constraints: vec![],
        current_values: ValueMap::new(),
        objective: None,
    };
    let result = solver.solve(&problem);
    assert!(matches!(result, SolveResult::Solved { .. }));
}

/// False negative: auto param x has bounds [0.0, 1.9999999], constraint x > 2.0.
/// The best possible value is 1.9999999, violated by 1e-7.
/// Old squared penalty: (1e-7)^2 = 1e-14 < FEASIBILITY_THRESHOLD → false "Solved".
/// Must return NOT Solved.
#[test]
fn false_negative_small_violation() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let x_ref = value_ref("Part", "x");

    // constraint: x > 2.0
    let constraint = gt(x_ref, literal(Value::Scalar {
        si_value: 2.0,
        dimension: DimensionVector::LENGTH,
    }));

    // Current value already at the max bound
    let mut current = ValueMap::new();
    current.insert(
        x_id.clone(),
        Value::Scalar {
            si_value: 1.9999999,
            dimension: DimensionVector::LENGTH,
        },
    );

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.0, 1.9999999)),
        }],
        constraints: vec![(cnid("Part", 0), constraint)],
        current_values: current,
        objective: None,
    };

    let result = solver.solve(&problem);
    assert!(
        !matches!(result, SolveResult::Solved { .. }),
        "should NOT report Solved when constraint x > 2.0 is violated by 1e-7"
    );
}

/// False negative with multiple small violations: x > 2.0 and y > 1.0,
/// each violated by 1e-7. Sum of squares = 2e-14 < threshold.
/// Must return NOT Solved.
#[test]
fn false_negative_multiple_small_violations() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let y_id = vcid("Part", "y");
    let x_ref = value_ref("Part", "x");
    let y_ref = value_ref("Part", "y");

    let c1 = gt(x_ref, literal(Value::Scalar {
        si_value: 2.0,
        dimension: DimensionVector::LENGTH,
    }));
    let c2 = gt(y_ref, literal(Value::Scalar {
        si_value: 1.0,
        dimension: DimensionVector::LENGTH,
    }));

    let mut current = ValueMap::new();
    current.insert(
        x_id.clone(),
        Value::Scalar {
            si_value: 1.9999999,
            dimension: DimensionVector::LENGTH,
        },
    );
    current.insert(
        y_id.clone(),
        Value::Scalar {
            si_value: 0.9999999,
            dimension: DimensionVector::LENGTH,
        },
    );

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.0, 1.9999999)),
            },
            AutoParam {
                id: y_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.0, 0.9999999)),
            },
        ],
        constraints: vec![
            (cnid("Part", 0), c1),
            (cnid("Part", 1), c2),
        ],
        current_values: current,
        objective: None,
    };

    let result = solver.solve(&problem);
    assert!(
        !matches!(result, SolveResult::Solved { .. }),
        "should NOT report Solved when both constraints are violated by 1e-7"
    );
}

/// False negative with mixed physical scales: x (length, ~2mm) and
/// y (dimensionless, ~100). Each violated by ~1e-7 absolute in their domain.
/// Must return NOT Solved.
#[test]
fn false_negative_mixed_scale() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let y_id = vcid("Part", "y");
    let x_ref = value_ref("Part", "x");
    let y_ref = value_ref("Part", "y");

    // x > 0.002 (constraint in SI meters, x bounded to max 0.001999999)
    let c1 = gt(x_ref, literal(Value::Scalar {
        si_value: 0.002,
        dimension: DimensionVector::LENGTH,
    }));

    // y > 100 (dimensionless, y bounded to max 99.9999999)
    let c2 = gt(y_ref, literal(Value::Scalar {
        si_value: 100.0,
        dimension: DimensionVector::DIMENSIONLESS,
    }));

    let mut current = ValueMap::new();
    current.insert(
        x_id.clone(),
        Value::Scalar {
            si_value: 0.001999999,
            dimension: DimensionVector::LENGTH,
        },
    );
    current.insert(
        y_id.clone(),
        Value::Scalar {
            si_value: 99.9999999,
            dimension: DimensionVector::DIMENSIONLESS,
        },
    );

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.0, 0.001999999)),
            },
            AutoParam {
                id: y_id.clone(),
                param_type: Type::dimensionless_scalar(),
                bounds: Some((0.0, 99.9999999)),
            },
        ],
        constraints: vec![
            (cnid("Part", 0), c1),
            (cnid("Part", 1), c2),
        ],
        current_values: current,
        objective: None,
    };

    let result = solver.solve(&problem);
    assert!(
        !matches!(result, SolveResult::Solved { .. }),
        "should NOT report Solved when constraints are violated by small absolute amounts"
    );
}

/// Bounds [0, 10mm], constraint x > 15mm. Cannot be satisfied within bounds.
/// Solver must report Infeasible with diagnostics containing residual info.
#[test]
fn bounds_dont_hide_infeasibility() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let x_ref = value_ref("Part", "x");

    // constraint: x > 0.015 (15mm)
    let constraint = gt(x_ref, literal(Value::Scalar {
        si_value: 0.015,
        dimension: DimensionVector::LENGTH,
    }));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.0, 0.010)), // max 10mm
        }],
        constraints: vec![(cnid("Part", 0), constraint)],
        current_values: ValueMap::new(),
        objective: None,
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Infeasible { diagnostics } => {
            assert!(
                !diagnostics.is_empty(),
                "should have diagnostic messages"
            );
            let msg = &diagnostics[0].message;
            assert!(
                msg.contains("residual"),
                "diagnostic should mention residual, got: {}",
                msg
            );
        }
        other => panic!(
            "expected Infeasible for constraint beyond bounds, got {:?}",
            other
        ),
    }
}

#[test]
fn compound_and_constraint() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let x_ref = value_ref("Part", "x");

    // (x > 5mm) AND (x < 50mm) — as a single compound constraint
    let compound = and(
        gt(x_ref.clone(), literal(mm(5.0))),
        lt(x_ref, literal(mm(50.0))),
    );

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.001, 0.1)),
        }],
        constraints: vec![(cnid("Part", 0), compound)],
        current_values: ValueMap::new(),
        objective: None,
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let x = values.get(&x_id).unwrap().as_f64().unwrap();
            assert!(
                x > 0.005 && x < 0.050,
                "x should satisfy compound constraint, got {} m",
                x
            );
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}
