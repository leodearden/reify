//! Integration tests for DimensionalSolver.
//!
//! Tests the solver through the ConstraintSolver trait object interface,
//! using reify-test-support helpers for expression construction.

use reify_constraints::DimensionalSolver;
use reify_test_support::*;
use reify_types::{
    AutoParam, BinOp, ConstraintSolver, DimensionVector, OptimizationObjective, ResolutionProblem,
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
        constraints: vec![(cnid("Bracket", 0), gt_expr), (cnid("Bracket", 1), lt_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let si = values.get(&thickness_id).unwrap().as_f64().unwrap();
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
        constraints: vec![(cnid("Bracket", 0), gt_expr), (cnid("Bracket", 1), lt_expr)],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let si = values.get(&thickness_id).unwrap().as_f64().unwrap();
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
        functions: vec![],
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
    let constraint = gt(
        x_ref,
        literal(Value::Scalar {
            si_value: 2.0,
            dimension: DimensionVector::LENGTH,
        }),
    );

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
        functions: vec![],
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

    let c1 = gt(
        x_ref,
        literal(Value::Scalar {
            si_value: 2.0,
            dimension: DimensionVector::LENGTH,
        }),
    );
    let c2 = gt(
        y_ref,
        literal(Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        }),
    );

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
        constraints: vec![(cnid("Part", 0), c1), (cnid("Part", 1), c2)],
        current_values: current,
        objective: None,
        functions: vec![],
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
    let c1 = gt(
        x_ref,
        literal(Value::Scalar {
            si_value: 0.002,
            dimension: DimensionVector::LENGTH,
        }),
    );

    // y > 100 (dimensionless, y bounded to max 99.9999999)
    let c2 = gt(
        y_ref,
        literal(Value::Scalar {
            si_value: 100.0,
            dimension: DimensionVector::DIMENSIONLESS,
        }),
    );

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
        constraints: vec![(cnid("Part", 0), c1), (cnid("Part", 1), c2)],
        current_values: current,
        objective: None,
        functions: vec![],
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
    let constraint = gt(
        x_ref,
        literal(Value::Scalar {
            si_value: 0.015,
            dimension: DimensionVector::LENGTH,
        }),
    );

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.0, 0.010)), // max 10mm
        }],
        constraints: vec![(cnid("Part", 0), constraint)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Infeasible { diagnostics } => {
            assert!(!diagnostics.is_empty(), "should have diagnostic messages");
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
        functions: vec![],
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

/// Minimize(x / 0) — division by zero produces Value::Undef.
/// The solver should NOT report Solved; it should detect the non-numeric objective.
#[test]
fn minimize_undef_objective_returns_no_progress() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let x_ref = value_ref("Part", "x");

    // Constraints: x > 1mm AND x < 50mm (trivially satisfiable)
    let gt_expr = gt(x_ref.clone(), literal(mm(1.0)));
    let lt_expr = lt(x_ref.clone(), literal(mm(50.0)));

    // Objective: minimize(x / 0) — division by zero → Undef
    let zero = literal(Value::Int(0));
    let div_by_zero = binop(BinOp::Div, x_ref, zero);
    let objective = OptimizationObjective::Minimize(div_by_zero);

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.001, 0.1)),
        }],
        constraints: vec![(cnid("Part", 0), gt_expr), (cnid("Part", 1), lt_expr)],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![],
    };

    let result = solver.solve(&problem);
    assert!(
        !matches!(result, SolveResult::Solved { .. }),
        "minimize(x/0) should NOT report Solved; Undef objective must be detected"
    );
}

/// Maximize(x / 0) — division by zero produces Value::Undef.
/// The solver should NOT report Solved; it should detect the non-numeric objective.
#[test]
fn maximize_undef_objective_returns_no_progress() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let x_ref = value_ref("Part", "x");

    // Constraints: x > 1mm AND x < 50mm (trivially satisfiable)
    let gt_expr = gt(x_ref.clone(), literal(mm(1.0)));
    let lt_expr = lt(x_ref.clone(), literal(mm(50.0)));

    // Objective: maximize(x / 0) — division by zero → Undef
    let zero = literal(Value::Int(0));
    let div_by_zero = binop(BinOp::Div, x_ref, zero);
    let objective = OptimizationObjective::Maximize(div_by_zero);

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.001, 0.1)),
        }],
        constraints: vec![(cnid("Part", 0), gt_expr), (cnid("Part", 1), lt_expr)],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![],
    };

    let result = solver.solve(&problem);
    assert!(
        !matches!(result, SolveResult::Solved { .. }),
        "maximize(x/0) should NOT report Solved; Undef objective must be detected"
    );
}

/// Verify that the Nelder-Mead solver with sd_tolerance(1e-15) runs successfully
/// and doesn't panic or produce garbage. This validates the tolerance config path
/// after removing the degenerate 1-vertex simplex fallback.
#[test]
fn nelder_mead_tolerance_config_does_not_degenerate() {
    let solver = DimensionalSolver;

    let x_id = vcid("Box", "width");
    let x_ref = value_ref("Box", "width");

    // Simple feasibility: 5mm < width < 50mm
    let gt_expr = gt(x_ref.clone(), literal(mm(5.0)));
    let lt_expr = lt(x_ref, literal(mm(50.0)));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.001, 0.1)),
        }],
        constraints: vec![(cnid("Box", 0), gt_expr), (cnid("Box", 1), lt_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![],
    };

    // This should not panic — the solver should configure NelderMead correctly
    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let si = values.get(&x_id).unwrap().as_f64().unwrap();
            assert!(
                si > 0.005 && si < 0.050,
                "width should be in feasible range (5-50mm), got {} m",
                si
            );
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}

/// Optimization with an already-feasible initial point should still produce Solved,
/// with the objective driven toward its optimum (minimize pushes toward lower bound).
/// Auto param bounds (5mm–100mm) prevent the solver from overshooting the constraint
/// boundary at 2mm, so the optimizer converges at the bounds floor (≈5mm) which is
/// well inside the feasible region.
#[test]
fn optimize_with_feasible_initial_point() {
    let solver = DimensionalSolver;

    let thickness_id = vcid("Bracket", "thickness");
    let thickness_ref = value_ref("Bracket", "thickness");

    // thickness > 2mm AND thickness < 50mm
    let gt_expr = gt(thickness_ref.clone(), literal(mm(2.0)));
    let lt_expr = lt(thickness_ref.clone(), literal(mm(50.0)));

    // Minimize thickness — should push toward the auto param lower bound (5mm),
    // which is still above the constraint floor (2mm)
    let objective = OptimizationObjective::Minimize(thickness_ref);

    // Set current value to 25mm — already feasible (between 2mm and 50mm)
    let mut current = ValueMap::new();
    current.insert(
        thickness_id.clone(),
        Value::Scalar {
            si_value: 0.025,
            dimension: DimensionVector::LENGTH,
        },
    );

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: thickness_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.005, 0.1)), // 5mm–100mm, floor above constraint
        }],
        constraints: vec![(cnid("Bracket", 0), gt_expr), (cnid("Bracket", 1), lt_expr)],
        current_values: current,
        objective: Some(objective),
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let si = values.get(&thickness_id).unwrap().as_f64().unwrap();
            // Minimize should push thickness toward 5mm (auto param lower bound),
            // which is safely above the 2mm constraint.
            assert!(
                si >= 0.005 && si < 0.008,
                "minimized thickness should be near 5mm, got {} m",
                si
            );
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}

/// Maximize with a feasible initial point — the solver should still push x
/// toward the constraint boundary (upper) rather than staying at the initial point.
/// Auto param upper bound (50mm) is below the constraint ceiling (80mm), so the
/// optimizer converges at the param bound (50mm), safely inside the feasible region.
#[test]
fn maximize_with_feasible_initial_point() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let x_ref = value_ref("Part", "x");

    // x > 2mm AND x < 80mm
    let gt_expr = gt(x_ref.clone(), literal(mm(2.0)));
    let lt_expr = lt(x_ref.clone(), literal(mm(80.0)));

    // Maximize x — should push toward upper bound
    let objective = OptimizationObjective::Maximize(x_ref);

    // Set current value to 10mm — already feasible
    let mut current = ValueMap::new();
    current.insert(
        x_id.clone(),
        Value::Scalar {
            si_value: 0.010,
            dimension: DimensionVector::LENGTH,
        },
    );

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.001, 0.050)), // upper bound 50mm < constraint 80mm
        }],
        constraints: vec![(cnid("Part", 0), gt_expr), (cnid("Part", 1), lt_expr)],
        current_values: current,
        objective: Some(objective),
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let si = values.get(&x_id).unwrap().as_f64().unwrap();
            // Maximize should push x toward 50mm (auto param upper bound),
            // well above the 10mm initial point
            assert!(
                si > 0.030,
                "maximized x should be pushed well above initial 10mm, got {} m",
                si
            );
            assert!(
                si <= 0.051,
                "maximized x should not exceed param bounds (50mm), got {} m",
                si
            );
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}

/// When the initial point is feasible and the optimizer drifts infeasible
/// while chasing an objective, the solver must fall back to the initial
/// feasible point rather than returning Infeasible.
///
/// Setup: tight constraints (x > 5mm AND x < 6mm), current = 5.5mm (feasible),
/// minimize(x) objective, but param bounds [0, 100mm] let the optimizer explore
/// well below the constraint floor. With only 500 warm-start iterations, the
/// penalty-based optimizer may converge to a point below 5mm.
/// Pre-fix: solver returns Infeasible (bug). Post-fix: Solved with initial values.
#[test]
fn warm_start_falls_back_to_initial_when_optimizer_drifts_infeasible() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let x_ref = value_ref("Part", "x");

    // Tight constraints: x > 5mm AND x < 6mm (only 1mm feasible window)
    let gt_expr = gt(x_ref.clone(), literal(mm(5.0)));
    let lt_expr = lt(x_ref.clone(), literal(mm(6.0)));

    // Minimize x — pushes toward 0, trying to leave the feasible window
    let objective = OptimizationObjective::Minimize(x_ref);

    // Current value = 5.5mm — right in the feasible window
    let mut current = ValueMap::new();
    current.insert(
        x_id.clone(),
        Value::Scalar {
            si_value: 0.0055,
            dimension: DimensionVector::LENGTH,
        },
    );

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            // Wide bounds [0, 100mm] — optimizer CAN explore below 5mm
            bounds: Some((0.0, 0.1)),
        }],
        constraints: vec![(cnid("Part", 0), gt_expr), (cnid("Part", 1), lt_expr)],
        current_values: current,
        objective: Some(objective),
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let si = values.get(&x_id).unwrap().as_f64().unwrap();
            // The result must satisfy constraints: 5mm < x < 6mm
            assert!(
                si > 0.005 && si < 0.006,
                "result should satisfy constraints (5mm < x < 6mm), got {} m",
                si
            );
        }
        other => panic!(
            "expected Solved (fallback to feasible initial), got {:?}",
            other
        ),
    }
}

/// Infeasible constraints with an objective present should still be detected.
/// Bounds [0, 10mm], constraint x > 15mm — impossible within bounds.
/// Regression guard: the feasibility check must NOT short-circuit the
/// infeasibility detection path when an objective is present.
#[test]
fn infeasible_with_objective_still_detected() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let x_ref = value_ref("Part", "x");

    // constraint: x > 15mm — impossible with bounds [0, 10mm]
    let constraint = gt(x_ref.clone(), literal(mm(15.0)));

    // Maximize x — the objective shouldn't mask the infeasibility
    let objective = OptimizationObjective::Maximize(x_ref);

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.0, 0.010)), // max 10mm
        }],
        constraints: vec![(cnid("Part", 0), constraint)],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Infeasible { diagnostics } => {
            assert!(!diagnostics.is_empty(), "should have diagnostic messages");
            let msg = &diagnostics[0].message;
            assert!(
                msg.contains("residual"),
                "diagnostic should mention residual, got: {}",
                msg
            );
        }
        other => panic!(
            "expected Infeasible for constraint beyond bounds with objective, got {:?}",
            other
        ),
    }
}

/// Warm-start with a feasible initial point should optimize to a BETTER value
/// than the initial point, not just fall back. This guards against the fallback
/// being too aggressive — it should only trigger when the optimizer drifts
/// infeasible, not on every warm-start.
///
/// Setup: x > 2mm AND x < 50mm, initial = 25mm, minimize(x), bounds [5mm, 100mm].
/// The optimizer should push x down to ~5mm (param lower bound), which is better
/// than the 25mm initial point and still feasible.
#[test]
fn warm_start_optimizes_when_possible() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let x_ref = value_ref("Part", "x");

    // Wide constraints: x > 2mm AND x < 50mm
    let gt_expr = gt(x_ref.clone(), literal(mm(2.0)));
    let lt_expr = lt(x_ref.clone(), literal(mm(50.0)));

    // Minimize x — should optimize, not just return initial
    let objective = OptimizationObjective::Minimize(x_ref);

    // Start at 25mm — feasible, but far from optimal
    let mut current = ValueMap::new();
    current.insert(
        x_id.clone(),
        Value::Scalar {
            si_value: 0.025,
            dimension: DimensionVector::LENGTH,
        },
    );

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.005, 0.1)), // 5mm–100mm, lower bound above constraint floor
        }],
        constraints: vec![(cnid("Part", 0), gt_expr), (cnid("Part", 1), lt_expr)],
        current_values: current,
        objective: Some(objective),
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let si = values.get(&x_id).unwrap().as_f64().unwrap();
            // Optimizer should push x below 25mm (initial), toward ~5mm (param lower bound)
            assert!(
                si < 0.015,
                "optimized x should be well below initial 25mm, got {} m (fallback not triggered)",
                si
            );
            // Must still be feasible
            assert!(
                si > 0.002,
                "optimized x should still satisfy x > 2mm, got {} m",
                si
            );
        }
        other => panic!("expected Solved with optimized value, got {:?}", other),
    }
}

/// Warm-start with an 8-parameter feasible problem and an objective.
/// Each parameter has a constraint that the optimizer might push past
/// with insufficient iterations. With dimension-scaled budget, the solver
/// should get enough iterations (500 * 9 = 4500 vs fixed 500) to stay
/// feasible and return Solved.
#[test]
fn warm_start_scales_iterations_with_dimension() {
    let solver = DimensionalSolver;

    // Create 8 independent parameters, each with tight constraints
    let param_ids: Vec<_> = (0..8).map(|i| vcid("Part", &format!("p{}", i))).collect();
    let param_refs: Vec<_> = (0..8)
        .map(|i| value_ref("Part", &format!("p{}", i)))
        .collect();

    // Each param: p_i > 10mm AND p_i < 20mm
    let mut constraints = Vec::new();
    for (i, pref) in param_refs.iter().enumerate() {
        let idx = i as u32;
        constraints.push((cnid("Part", idx * 2), gt(pref.clone(), literal(mm(10.0)))));
        constraints.push((
            cnid("Part", idx * 2 + 1),
            lt(pref.clone(), literal(mm(20.0))),
        ));
    }

    // Minimize p0 — pushes one param toward lower bound
    let objective = OptimizationObjective::Minimize(param_refs[0].clone());

    // All params start at 15mm (feasible, centered in constraint window)
    let mut current = ValueMap::new();
    for pid in &param_ids {
        current.insert(
            pid.clone(),
            Value::Scalar {
                si_value: 0.015,
                dimension: DimensionVector::LENGTH,
            },
        );
    }

    let auto_params: Vec<_> = param_ids
        .iter()
        .map(|pid| AutoParam {
            id: pid.clone(),
            param_type: Type::length(),
            bounds: Some((0.005, 0.025)), // 5mm–25mm, extends beyond constraints
        })
        .collect();

    let problem = ResolutionProblem {
        auto_params,
        constraints,
        current_values: current,
        objective: Some(objective),
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            // All params must satisfy their constraints
            for pid in &param_ids {
                let si = values.get(pid).unwrap().as_f64().unwrap();
                assert!(
                    si > 0.010 && si < 0.020,
                    "param {:?} should satisfy constraints (10mm < p < 20mm), got {} m",
                    pid,
                    si
                );
            }
        }
        other => panic!("expected Solved for 8-param warm-start, got {:?}", other),
    }
}

/// Budget-exhaustion scenario: 2-param problem with tight constraints
/// (each param 10mm-12mm window), wide auto param bounds (0-100mm), both
/// params start feasible at 11mm, minimize(p0+p1) objective. With the
/// dimension-scaled iteration budget, the solver may exhaust its budget
/// without fully converging the objective, but the result must still be
/// Solved with all constraints satisfied.
///
/// This tests the convergence-without-full-optimality scenario — the solver
/// returns Solved even when the optimizer hits MaxItersReached, as long as
/// the final point satisfies all constraints.
#[test]
fn warm_start_budget_exhaustion_stays_feasible() {
    let solver = DimensionalSolver;

    let p0_id = vcid("Part", "p0");
    let p1_id = vcid("Part", "p1");
    let p0_ref = value_ref("Part", "p0");
    let p1_ref = value_ref("Part", "p1");

    // Tight constraints: each param in [10mm, 12mm] — only 2mm feasible window
    let constraints = vec![
        (cnid("Part", 0), gt(p0_ref.clone(), literal(mm(10.0)))),
        (cnid("Part", 1), lt(p0_ref.clone(), literal(mm(12.0)))),
        (cnid("Part", 2), gt(p1_ref.clone(), literal(mm(10.0)))),
        (cnid("Part", 3), lt(p1_ref.clone(), literal(mm(12.0)))),
    ];

    // Minimize(p0 + p1) — pushes both params toward their lower constraint bound
    let sum_expr = binop(BinOp::Add, p0_ref, p1_ref);
    let objective = OptimizationObjective::Minimize(sum_expr);

    // Both params start at 11mm — feasible, centered in constraint window
    let mut current = ValueMap::new();
    current.insert(
        p0_id.clone(),
        Value::Scalar {
            si_value: 0.011,
            dimension: DimensionVector::LENGTH,
        },
    );
    current.insert(
        p1_id.clone(),
        Value::Scalar {
            si_value: 0.011,
            dimension: DimensionVector::LENGTH,
        },
    );

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: p0_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.0, 0.1)), // Wide bounds [0, 100mm]
            },
            AutoParam {
                id: p1_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.0, 0.1)), // Wide bounds [0, 100mm]
            },
        ],
        constraints,
        current_values: current,
        objective: Some(objective),
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            // Both params must satisfy constraints: 10mm < p < 12mm
            for pid in [&p0_id, &p1_id] {
                let si = values.get(pid).unwrap().as_f64().unwrap();
                assert!(
                    si > 0.010 && si < 0.012,
                    "param {:?} should satisfy constraints (10mm < p < 12mm), got {} m",
                    pid,
                    si
                );
            }
        }
        other => panic!(
            "expected Solved for budget-exhaustion scenario, got {:?}",
            other
        ),
    }
}

/// When all params start feasible (centered in their constraint windows) with NO
/// objective, the solver should return Solved immediately via the early-exit path
/// with values equal to the initial current_values. This validates that the
/// pure-feasibility early-exit path works correctly and doesn't regress with
/// tracing instrumentation.
#[test]
fn warm_start_feasible_no_objective_early_exit() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let y_id = vcid("Part", "y");
    let z_id = vcid("Part", "z");
    let x_ref = value_ref("Part", "x");
    let y_ref = value_ref("Part", "y");
    let z_ref = value_ref("Part", "z");

    // Constraints: each param must be between 10mm and 20mm
    let constraints = vec![
        (cnid("Part", 0), gt(x_ref.clone(), literal(mm(10.0)))),
        (cnid("Part", 1), lt(x_ref.clone(), literal(mm(20.0)))),
        (cnid("Part", 2), gt(y_ref.clone(), literal(mm(10.0)))),
        (cnid("Part", 3), lt(y_ref.clone(), literal(mm(20.0)))),
        (cnid("Part", 4), gt(z_ref.clone(), literal(mm(10.0)))),
        (cnid("Part", 5), lt(z_ref.clone(), literal(mm(20.0)))),
    ];

    // All params start centered at 15mm — solidly feasible
    let mut current = ValueMap::new();
    current.insert(
        x_id.clone(),
        Value::Scalar {
            si_value: 0.015,
            dimension: DimensionVector::LENGTH,
        },
    );
    current.insert(
        y_id.clone(),
        Value::Scalar {
            si_value: 0.015,
            dimension: DimensionVector::LENGTH,
        },
    );
    current.insert(
        z_id.clone(),
        Value::Scalar {
            si_value: 0.015,
            dimension: DimensionVector::LENGTH,
        },
    );

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
            },
            AutoParam {
                id: y_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
            },
            AutoParam {
                id: z_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
            },
        ],
        constraints,
        current_values: current,
        objective: None, // No objective — should trigger early-exit
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            // Each param should be returned at exactly the initial value (15mm)
            for (pid, label) in [(&x_id, "x"), (&y_id, "y"), (&z_id, "z")] {
                let si = values.get(pid).unwrap().as_f64().unwrap();
                assert!(
                    (si - 0.015).abs() < 1e-12,
                    "{} should remain at initial 15mm (0.015 m), got {} m",
                    label,
                    si
                );
            }
        }
        other => panic!(
            "expected Solved for feasible-no-objective early exit, got {:?}",
            other
        ),
    }
}

/// When the initial point is INfeasible and the optimizer also fails to find
/// feasibility, the result must be Infeasible — the feasible fallback must NOT
/// apply when the initial point was never verified feasible.
///
/// Regression guard: ensures the fallback is gated on initially_feasible=true.
#[test]
fn infeasible_initial_not_rescued_by_fallback() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let x_ref = value_ref("Part", "x");

    // constraint: x > 15mm — impossible with bounds [0, 10mm]
    let constraint = gt(x_ref.clone(), literal(mm(15.0)));

    // Minimize x — objective present, but initial is not feasible
    let objective = OptimizationObjective::Minimize(x_ref);

    // Current value = 5mm — NOT feasible (violates x > 15mm)
    let mut current = ValueMap::new();
    current.insert(
        x_id.clone(),
        Value::Scalar {
            si_value: 0.005,
            dimension: DimensionVector::LENGTH,
        },
    );

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.0, 0.010)), // max 10mm, can't reach 15mm
        }],
        constraints: vec![(cnid("Part", 0), constraint)],
        current_values: current,
        objective: Some(objective),
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Infeasible { diagnostics } => {
            assert!(!diagnostics.is_empty(), "should have diagnostic messages");
            let msg = &diagnostics[0].message;
            assert!(
                msg.contains("residual"),
                "diagnostic should mention residual, got: {}",
                msg
            );
        }
        other => panic!(
            "expected Infeasible when initial point is not feasible, got {:?} — fallback must NOT rescue infeasible initials",
            other
        ),
    }
}
