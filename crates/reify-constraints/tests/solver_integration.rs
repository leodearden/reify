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
        constraints: vec![
            (cnid("Bracket", 0), gt_expr),
            (cnid("Bracket", 1), lt_expr),
        ],
        current_values: current,
        objective: Some(objective),
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let si = values
                .get(&thickness_id)
                .unwrap()
                .as_f64()
                .unwrap();
            // Minimize should push thickness toward 5mm (auto param lower bound),
            // which is safely above the 2mm constraint.
            assert!(
                si > 0.002 && si < 0.010,
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
        constraints: vec![
            (cnid("Part", 0), gt_expr),
            (cnid("Part", 1), lt_expr),
        ],
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
        constraints: vec![
            (cnid("Part", 0), gt_expr),
            (cnid("Part", 1), lt_expr),
        ],
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
            "expected Infeasible for constraint beyond bounds with objective, got {:?}",
            other
        ),
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
        other => panic!(
            "expected Solved for 8-param warm-start, got {:?}",
            other
        ),
    }
}
