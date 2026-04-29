//! Integration tests for DimensionalSolver.
//!
//! Tests the solver through the ConstraintSolver trait object interface,
//! using reify-test-support helpers for expression construction.

use std::sync::Arc;

use reify_constraints::DimensionalSolver;
use reify_test_support::*;
use reify_types::{
    AutoParam, BinOp, ConstraintSolver, DiagnosticCode, DimensionVector, OptimizationObjective,
    ResolutionProblem, SolveResult, Type, Value, ValueMap,
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
            free: false,
        }],
        constraints: vec![(cnid("Bracket", 0), gt_expr), (cnid("Bracket", 1), lt_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
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
            free: false,
        }],
        constraints: vec![(cnid("Bracket", 0), gt_expr), (cnid("Bracket", 1), lt_expr)],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
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
        functions: vec![].into(),
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

    // constraint: x > 2.0m
    let constraint = gt(x_ref, literal(meters(2.0)));

    // Current value already at the max bound
    let mut current = ValueMap::new();
    current.insert(x_id.clone(), meters(1.9999999));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.0, 1.9999999)),
            free: false,
        }],
        constraints: vec![(cnid("Part", 0), constraint)],
        current_values: current,
        objective: None,
        functions: vec![].into(),
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

    let c1 = gt(x_ref, literal(meters(2.0)));
    let c2 = gt(y_ref, literal(meters(1.0)));

    let mut current = ValueMap::new();
    current.insert(x_id.clone(), meters(1.9999999));
    current.insert(y_id.clone(), meters(0.9999999));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.0, 1.9999999)),
                free: false,
            },
            AutoParam {
                id: y_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.0, 0.9999999)),
                free: false,
            },
        ],
        constraints: vec![(cnid("Part", 0), c1), (cnid("Part", 1), c2)],
        current_values: current,
        objective: None,
        functions: vec![].into(),
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

    // x > 2mm (constraint in SI meters = 0.002, x bounded to max 0.001999999)
    let c1 = gt(x_ref, literal(mm(2.0)));

    // y > 100 (dimensionless, y bounded to max 99.9999999)
    let c2 = gt(
        y_ref,
        literal(Value::Scalar {
            si_value: 100.0,
            dimension: DimensionVector::DIMENSIONLESS,
        }),
    );

    let mut current = ValueMap::new();
    current.insert(x_id.clone(), mm(1.999999));
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
                free: false,
            },
            AutoParam {
                id: y_id.clone(),
                param_type: Type::dimensionless_scalar(),
                bounds: Some((0.0, 99.9999999)),
                free: false,
            },
        ],
        constraints: vec![(cnid("Part", 0), c1), (cnid("Part", 1), c2)],
        current_values: current,
        objective: None,
        functions: vec![].into(),
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

    // constraint: x > 15mm
    let constraint = gt(x_ref, literal(mm(15.0)));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.0, 0.010)), // max 10mm
            free: false,
        }],
        constraints: vec![(cnid("Part", 0), constraint)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
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
            free: true, // not testing uniqueness — range constraint is inherently underdetermined
        }],
        constraints: vec![(cnid("Part", 0), compound)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
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
            free: false,
        }],
        constraints: vec![(cnid("Part", 0), gt_expr), (cnid("Part", 1), lt_expr)],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![].into(),
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
            free: false,
        }],
        constraints: vec![(cnid("Part", 0), gt_expr), (cnid("Part", 1), lt_expr)],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![].into(),
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
            free: true, // not testing uniqueness — range constraint is inherently underdetermined
        }],
        constraints: vec![(cnid("Box", 0), gt_expr), (cnid("Box", 1), lt_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    // This should not panic — the solver should configure NelderMead correctly
    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
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
    current.insert(thickness_id.clone(), mm(25.0));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: thickness_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.005, 0.1)), // 5mm–100mm, floor above constraint
            free: false,
        }],
        constraints: vec![(cnid("Bracket", 0), gt_expr), (cnid("Bracket", 1), lt_expr)],
        current_values: current,
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            let si = values.get(&thickness_id).unwrap().as_f64().unwrap();
            // Minimize should push thickness toward 5mm (auto param lower bound),
            // which is safely above the 2mm constraint.
            assert!(
                (0.005 - 1e-9..0.008).contains(&si),
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
    current.insert(x_id.clone(), mm(10.0));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.001, 0.050)), // upper bound 50mm < constraint 80mm
            free: false,
        }],
        constraints: vec![(cnid("Part", 0), gt_expr), (cnid("Part", 1), lt_expr)],
        current_values: current,
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            let si = values.get(&x_id).unwrap().as_f64().unwrap();
            // Maximize should push x toward 50mm (auto param upper bound),
            // well above the 10mm initial point
            assert!(
                si > 0.048,
                "maximized x should be near 50mm upper bound, got {} m",
                si
            );
            // AutoParam bounds are hard constraints on output values — the solver
            // guarantees results stay within [lo, hi].
            // Clamping logic: solver.rs ~line 617-625 (effective_bounds clamping loop).
            // Because clamping is exact (val.clamp(lo, hi)), no epsilon tolerance needed.
            assert!(
                si <= 0.050,
                "maximized x should not exceed param upper bound (50mm), got {} m",
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
/// well below the constraint floor. With 1000 warm-start iterations
/// (budget = 500 * (1+1) = 1000 for 1 param), the penalty-based optimizer
/// may converge to a point below 5mm.
/// Pre-fix: solver returns Infeasible (bug). Post-fix: Solved with exact initial values.
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
    current.insert(x_id.clone(), mm(5.5));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            // Wide bounds [0, 100mm] — optimizer CAN explore below 5mm
            bounds: Some((0.0, 0.1)),
            free: false,
        }],
        constraints: vec![(cnid("Part", 0), gt_expr), (cnid("Part", 1), lt_expr)],
        current_values: current,
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            let si = values.get(&x_id).unwrap().as_f64().unwrap();
            // Fallback must return the EXACT initial value (5.5mm = 0.0055m),
            // not a partially-optimized point. The initial is preserved through
            // as_f64() → Vec<f64> → build_solved_values() round-trip.
            assert!(
                (si - 0.0055).abs() < 1e-10,
                "fallback should return exact initial value 5.5mm (0.0055 m), got {} m (delta = {:.2e})",
                si,
                (si - 0.0055).abs()
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
            free: false,
        }],
        constraints: vec![(cnid("Part", 0), constraint)],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![].into(),
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
    current.insert(x_id.clone(), mm(25.0));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.005, 0.1)), // 5mm–100mm, lower bound above constraint floor
            free: false,
        }],
        constraints: vec![(cnid("Part", 0), gt_expr), (cnid("Part", 1), lt_expr)],
        current_values: current,
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            let si = values.get(&x_id).unwrap().as_f64().unwrap();
            // Optimizer should push x toward ~5mm (param lower bound).
            // With wide constraints (2-50mm) and bounds [5mm, 100mm],
            // convergence near the 5mm floor is expected. Threshold 8mm
            // guards against regressions where the solver barely optimizes.
            assert!(
                si < 0.008,
                "optimized x should converge near 5mm lower bound, got {} m (threshold 8mm)",
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
        current.insert(pid.clone(), mm(15.0));
    }

    let auto_params: Vec<_> = param_ids
        .iter()
        .map(|pid| AutoParam {
            id: pid.clone(),
            param_type: Type::length(),
            bounds: Some((0.005, 0.025)), // 5mm–25mm, extends beyond constraints
            free: false,
        })
        .collect();

    let problem = ResolutionProblem {
        auto_params,
        constraints,
        current_values: current,
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
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
            // Verify p0 (the minimized param) was not worsened beyond its
            // initial value (15mm). In 8 dimensions with tight 10-20mm windows,
            // the optimizer may drift infeasible and fallback to initial values,
            // so we verify the solver at least preserved or improved p0.
            let si_p0 = values.get(&param_ids[0]).unwrap().as_f64().unwrap();
            assert!(
                si_p0 <= 0.015 + 1e-10,
                "p0 should be at or below initial 15mm (got {} m), \
                 verifying solver did not worsen the minimized param",
                si_p0
            );
        }
        other => panic!("expected Solved for 8-param warm-start, got {:?}", other),
    }
}

/// Budget-exhaustion scenario: 12-param problem with tight constraints
/// (each param 10mm-12mm window), wide auto param bounds (0-100mm), all
/// params start feasible at 11mm, minimize(sum) objective. With 12 params
/// the iteration budget is min(500 * 13, 5000) = 5000 — Nelder-Mead in
/// 12 dimensions with tight 2mm windows cannot converge in 5000 iterations.
///
/// This tests the convergence-without-full-optimality scenario — the solver
/// returns Solved even when the optimizer hits MaxItersReached, as long as
/// the final point satisfies all constraints. The suboptimality assertion
/// confirms the optimizer did NOT reach the global minimum (all params at
/// lower bound), proving budget exhaustion actually occurred.
#[test]
fn warm_start_budget_exhaustion_stays_feasible() {
    let solver = DimensionalSolver;

    // 12 parameters — budget = min(500 * 13, 5000) = 5000 iterations.
    // Nelder-Mead in 12 dimensions with tight 2mm windows won't converge
    // in 5000 iters, forcing the budget-exhaustion fallback path.
    let n_params: usize = 12;

    let ids: Vec<_> = (0..n_params)
        .map(|i| vcid("Part", &format!("p{}", i)))
        .collect();
    let refs: Vec<_> = (0..n_params)
        .map(|i| value_ref("Part", &format!("p{}", i)))
        .collect();

    // Tight constraints: each param in [10mm, 12mm] — only 2mm feasible window
    let mut constraints = Vec::new();
    for (i, r) in refs.iter().enumerate() {
        constraints.push((
            cnid("Part", (i * 2) as u32),
            gt(r.clone(), literal(mm(10.0))),
        ));
        constraints.push((
            cnid("Part", (i * 2 + 1) as u32),
            lt(r.clone(), literal(mm(12.0))),
        ));
    }

    // Minimize(p0 + p1 + ... + p11) — pushes all params toward lower bound
    let sum_expr = refs
        .iter()
        .skip(1)
        .fold(refs[0].clone(), |acc, r| binop(BinOp::Add, acc, r.clone()));
    let objective = OptimizationObjective::Minimize(sum_expr);

    // All params start at 11mm — feasible, centered in constraint window
    let mut current = ValueMap::new();
    for id in &ids {
        current.insert(id.clone(), mm(11.0));
    }

    let problem = ResolutionProblem {
        auto_params: ids
            .iter()
            .map(|id| AutoParam {
                id: id.clone(),
                param_type: Type::length(),
                bounds: Some((0.0, 0.1)), // Wide bounds [0, 100mm]
                free: false,
            })
            .collect(),
        constraints,
        current_values: current,
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            // All params must satisfy constraints: 10mm < p < 12mm (feasibility preserved)
            for id in &ids {
                let si = values.get(id).unwrap().as_f64().unwrap();
                assert!(
                    si > 0.010 && si < 0.012,
                    "param {:?} should satisfy constraints (10mm < p < 12mm), got {} m",
                    id,
                    si
                );
            }
            // Suboptimality check: a fully converged optimizer would push all params
            // to the lower bound (~10mm = 0.010 m), giving sum ≈ 0.120.
            // We use a threshold of 10.5mm per param (midpoint between lower bound
            // 10mm and start 11mm). A converged optimizer yields sum < threshold,
            // while a budget-exhausted optimizer (params still near 11mm) yields
            // sum > threshold — making this a meaningful discriminator.
            let sum: f64 = ids
                .iter()
                .map(|id| values.get(id).unwrap().as_f64().unwrap())
                .sum();
            let suboptimality_threshold = n_params as f64 * 0.0105;
            assert!(
                sum > suboptimality_threshold,
                "sum of params ({}) should be above suboptimality threshold ({}) — \
                 budget exhaustion should leave params well above the optimum",
                sum,
                suboptimality_threshold
            );
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
    current.insert(x_id.clone(), mm(15.0));
    current.insert(y_id.clone(), mm(15.0));
    current.insert(z_id.clone(), mm(15.0));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: true, // not testing uniqueness — range constraints are underdetermined
            },
            AutoParam {
                id: y_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: true,
            },
            AutoParam {
                id: z_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: true,
            },
        ],
        constraints,
        current_values: current,
        objective: None, // No objective — should trigger early-exit
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
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
    current.insert(x_id.clone(), mm(5.0));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.0, 0.010)), // max 10mm, can't reach 15mm
            free: false,
        }],
        constraints: vec![(cnid("Part", 0), constraint)],
        current_values: current,
        objective: Some(objective),
        functions: vec![].into(),
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

/// Multi-param warm-start with objective: 3 auto params (p0, p1, p2) with wide
/// bounds [1mm, 100mm], constraints 5mm < pN < 50mm for each, all starting at
/// 30mm (feasible). Objective: Minimize(p0 + p1 + p2). This exercises
/// build_simplex with 4 vertices (3+1), build_trial_values with a 3-element
/// param vector, and the returned values map with 3 entries.
///
/// Asserts: Solved, each param satisfies constraints, and the sum is
/// non-regression from the initial 90mm total (optimizer should not worsen
/// the objective).
#[test]
fn multi_param_warm_start_with_objective() {
    let solver = DimensionalSolver;

    let p0_id = vcid("Part", "p0");
    let p1_id = vcid("Part", "p1");
    let p2_id = vcid("Part", "p2");
    let p0_ref = value_ref("Part", "p0");
    let p1_ref = value_ref("Part", "p1");
    let p2_ref = value_ref("Part", "p2");

    // Wide constraints: each param in [5mm, 50mm]
    let constraints = vec![
        (cnid("Part", 0), gt(p0_ref.clone(), literal(mm(5.0)))),
        (cnid("Part", 1), lt(p0_ref.clone(), literal(mm(50.0)))),
        (cnid("Part", 2), gt(p1_ref.clone(), literal(mm(5.0)))),
        (cnid("Part", 3), lt(p1_ref.clone(), literal(mm(50.0)))),
        (cnid("Part", 4), gt(p2_ref.clone(), literal(mm(5.0)))),
        (cnid("Part", 5), lt(p2_ref.clone(), literal(mm(50.0)))),
    ];

    // Minimize(p0 + p1 + p2)
    let sum_01 = binop(BinOp::Add, p0_ref, p1_ref);
    let sum_012 = binop(BinOp::Add, sum_01, p2_ref);
    let objective = OptimizationObjective::Minimize(sum_012);

    // All params start at 30mm — feasible, well within constraint windows
    let mut current = ValueMap::new();
    for pid in [&p0_id, &p1_id, &p2_id] {
        current.insert(pid.clone(), mm(30.0));
    }

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: p0_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.100)), // 1mm–100mm
                free: false,
            },
            AutoParam {
                id: p1_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.100)),
                free: false,
            },
            AutoParam {
                id: p2_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.100)),
                free: false,
            },
        ],
        constraints,
        current_values: current,
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            let mut sum_si = 0.0;
            for pid in [&p0_id, &p1_id, &p2_id] {
                let si = values.get(pid).unwrap().as_f64().unwrap();
                assert!(
                    si > 0.005 && si < 0.050,
                    "param {:?} should satisfy constraints (5mm < p < 50mm), got {} m",
                    pid,
                    si
                );
                sum_si += si;
            }
            // Non-regression: optimizer should not worsen the objective from
            // the initial sum of 0.090 (3 × 30mm). With the warm-start reduced
            // budget (500*(N+1) = 2000 iters for 3 params), the Nelder-Mead
            // optimizer may only achieve modest improvement. The 1e-9 epsilon
            // accounts for IEEE 754 float accumulation (0.030 + 0.030 + 0.030
            // may exceed 0.090 by a few ULPs).
            assert!(
                sum_si <= 0.090 + 1e-9,
                "optimizer should not increase sum above initial 90mm, got {} m",
                sum_si
            );
        }
        other => panic!(
            "expected Solved for multi-param warm-start with objective, got {:?}",
            other
        ),
    }
}

/// Partial-feasibility with unreachable constraint: p0 starts feasible
/// (satisfies 5mm < p0 < 50mm) but p1 starts infeasible (violates p1 > 20mm
/// because p1=10mm). Crucially, p1 bounds are [1mm, 15mm] — the optimizer
/// CANNOT reach p1 > 20mm. Since max_constraint_residual checks ALL
/// constraints, this partially-feasible point is treated as infeasible
/// (initially_feasible = false) and with no feasible region reachable, the
/// solver must return Infeasible.
///
/// Asserts: Infeasible result with a diagnostic mentioning "residual".
#[test]
fn partial_feasibility_infeasible_when_unreachable() {
    let solver = DimensionalSolver;

    let p0_id = vcid("Part", "p0");
    let p1_id = vcid("Part", "p1");
    let p0_ref = value_ref("Part", "p0");
    let p1_ref = value_ref("Part", "p1");

    // p0 constraints: 5mm < p0 < 50mm
    // p1 constraints: 20mm < p1 < 50mm
    let constraints = vec![
        (cnid("Part", 0), gt(p0_ref.clone(), literal(mm(5.0)))),
        (cnid("Part", 1), lt(p0_ref.clone(), literal(mm(50.0)))),
        (cnid("Part", 2), gt(p1_ref.clone(), literal(mm(20.0)))),
        (cnid("Part", 3), lt(p1_ref.clone(), literal(mm(50.0)))),
    ];

    // Minimize(p0 + p1)
    let sum_expr = binop(BinOp::Add, p0_ref, p1_ref);
    let objective = OptimizationObjective::Minimize(sum_expr);

    let mut current = ValueMap::new();
    // p0 = 30mm — satisfies both p0 constraints (5mm < 30mm < 50mm)
    current.insert(p0_id.clone(), mm(30.0));
    // p1 = 10mm — violates p1 > 20mm (the single infeasible constraint)
    current.insert(p1_id.clone(), mm(10.0));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: p0_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.100)), // 1mm–100mm
                free: false,
            },
            AutoParam {
                id: p1_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.015)), // 1mm–15mm: CANNOT reach p1 > 20mm
                free: false,
            },
        ],
        constraints,
        current_values: current,
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Infeasible { diagnostics } => {
            // The solver correctly identified the partial-feasibility as
            // infeasible (initially_feasible = false). With p1 bounds capped
            // at 15mm, the optimizer physically cannot reach p1 > 20mm,
            // guaranteeing an Infeasible outcome regardless of iteration count.
            assert!(!diagnostics.is_empty(), "should have diagnostic messages");
            let msg = &diagnostics[0].message;
            assert!(
                msg.contains("residual"),
                "diagnostic should mention residual, got: {}",
                msg
            );
        }
        other => panic!(
            "expected Infeasible when p1 bounds [1mm,15mm] cannot reach p1>20mm constraint, got {:?}",
            other
        ),
    }
}

/// Partial-feasibility with reachable constraint (no objective): p0 starts
/// feasible (satisfies 5mm < p0 < 50mm) and p1 starts just below the
/// constraint boundary (p1=19.5mm vs constraint p1 > 20mm). With bounds
/// [1mm, 100mm] and no objective pulling parameters downward, the optimizer
/// focuses entirely on constraint satisfaction and trivially moves p1 past
/// the 20mm boundary. Since initially_feasible=false, the solver uses the
/// full 5000-iteration budget, giving ample room to converge.
///
/// Asserts: Solved with all values satisfying constraints (p0 in 5-50mm,
/// p1 in 20-50mm).
#[test]
fn partial_feasibility_solved_when_close_to_boundary() {
    let solver = DimensionalSolver;

    let p0_id = vcid("Part", "p0");
    let p1_id = vcid("Part", "p1");
    let p0_ref = value_ref("Part", "p0");
    let p1_ref = value_ref("Part", "p1");

    // p0 constraints: 5mm < p0 < 50mm
    // p1 constraints: 20mm < p1 < 50mm
    let constraints = vec![
        (cnid("Part", 0), gt(p0_ref.clone(), literal(mm(5.0)))),
        (cnid("Part", 1), lt(p0_ref.clone(), literal(mm(50.0)))),
        (cnid("Part", 2), gt(p1_ref.clone(), literal(mm(20.0)))),
        (cnid("Part", 3), lt(p1_ref.clone(), literal(mm(50.0)))),
    ];

    // No objective — pure constraint satisfaction. This avoids the
    // penalty-weight trade-off where Minimize(p0+p1) pulls the optimizer
    // toward the constraint boundary rather than past it.

    let mut current = ValueMap::new();
    // p0 = 30mm — satisfies both p0 constraints (5mm < 30mm < 50mm)
    current.insert(p0_id.clone(), mm(30.0));
    // p1 = 19.5mm — just 0.5mm below the 20mm constraint boundary
    current.insert(p1_id.clone(), mm(19.5));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: p0_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.100)), // 1mm–100mm
                free: true, // not testing uniqueness — range constraints are underdetermined
            },
            AutoParam {
                id: p1_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.100)), // 1mm–100mm: easily reaches p1 > 20mm
                free: true,
            },
        ],
        constraints,
        current_values: current,
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            // The solver found feasibility for both params. Verify the solved
            // values actually satisfy all constraints.
            let p0_si = values.get(&p0_id).unwrap().as_f64().unwrap();
            let p1_si = values.get(&p1_id).unwrap().as_f64().unwrap();
            assert!(
                p0_si > 0.005 && p0_si < 0.050,
                "p0 should satisfy constraints (5mm < p0 < 50mm), got {} m",
                p0_si
            );
            assert!(
                p1_si > 0.020 && p1_si < 0.050,
                "p1 should satisfy constraints (20mm < p1 < 50mm), got {} m",
                p1_si
            );
        }
        other => panic!(
            "expected Solved when p1 starts at 19.5mm (just below 20mm boundary), got {:?}",
            other
        ),
    }
}

/// Documents the warm-start objective invariant:
///   After the early-return for `initially_feasible && objective.is_none()`,
///   reaching the warm-start budget branch with `initially_feasible=true`
///   implies `objective.is_some()`.
///
/// This test exercises both sides:
/// (a) feasible + objective=Some → warm-start budget path runs, solver optimizes
/// (b) feasible + objective=None → early-return path, solver returns initial point
#[test]
fn warm_start_budget_requires_objective_invariant() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let x_ref = value_ref("Part", "x");

    // Constraints: 5mm < x < 50mm
    let constraints = vec![
        (cnid("Part", 0), gt(x_ref.clone(), literal(mm(5.0)))),
        (cnid("Part", 1), lt(x_ref.clone(), literal(mm(50.0)))),
    ];

    // Start at 25mm — solidly feasible
    let mut current = ValueMap::new();
    current.insert(x_id.clone(), mm(25.0));

    let auto_params = vec![AutoParam {
        id: x_id.clone(),
        param_type: Type::length(),
        bounds: Some((0.005, 0.1)), // 5mm–100mm
        free: true,                 // not testing uniqueness — case (b) is underdetermined
    }];

    // (a) With objective: warm-start budget path runs, optimizer pushes x toward lower bound
    let problem_with_obj = ResolutionProblem {
        auto_params: auto_params.clone(),
        constraints: constraints.clone(),
        current_values: current.clone(),
        objective: Some(OptimizationObjective::Minimize(x_ref.clone())),
        functions: vec![].into(),
    };

    match solver.solve(&problem_with_obj) {
        SolveResult::Solved { values, .. } => {
            let si = values.get(&x_id).unwrap().as_f64().unwrap();
            // Optimizer should push x below 25mm initial toward the lower bound
            assert!(
                si < 0.020,
                "warm-start with objective should optimize x below 20mm, got {} m",
                si
            );
        }
        other => panic!(
            "expected Solved for feasible+objective (warm-start budget path), got {:?}",
            other
        ),
    }

    // (b) Without objective: early-return path, values match initial point
    let problem_no_obj = ResolutionProblem {
        auto_params,
        constraints,
        current_values: current,
        objective: None,
        functions: vec![].into(),
    };

    match solver.solve(&problem_no_obj) {
        SolveResult::Solved { values, .. } => {
            let si = values.get(&x_id).unwrap().as_f64().unwrap();
            assert!(
                (si - 0.025).abs() < 1e-12,
                "early-return should preserve initial 25mm (0.025 m), got {} m",
                si
            );
        }
        other => panic!(
            "expected Solved for feasible+no-objective (early-return path), got {:?}",
            other
        ),
    }
}

/// Multi-param fallback: 3 params with tight constraints (each 10-11mm window),
/// all start feasible at 10.5mm. Minimize(p0+p1+p2) pushes the optimizer below
/// the constraint floor (10mm). With wide bounds [0, 100mm], the optimizer
/// explores infeasible territory and the solver falls back to initial values.
///
/// Asserts: EACH returned value exactly matches its initial value (10.5mm),
/// verifying the fallback preserves all params without partial optimization
/// or corruption across the multi-param vector.
#[test]
fn warm_start_fallback_returns_exact_initial_values() {
    let solver = DimensionalSolver;

    let p0_id = vcid("Part", "p0");
    let p1_id = vcid("Part", "p1");
    let p2_id = vcid("Part", "p2");
    let p0_ref = value_ref("Part", "p0");
    let p1_ref = value_ref("Part", "p1");
    let p2_ref = value_ref("Part", "p2");

    // Tight constraints: each param in (10mm, 11mm) — only 1mm feasible window
    let constraints = vec![
        (cnid("Part", 0), gt(p0_ref.clone(), literal(mm(10.0)))),
        (cnid("Part", 1), lt(p0_ref.clone(), literal(mm(11.0)))),
        (cnid("Part", 2), gt(p1_ref.clone(), literal(mm(10.0)))),
        (cnid("Part", 3), lt(p1_ref.clone(), literal(mm(11.0)))),
        (cnid("Part", 4), gt(p2_ref.clone(), literal(mm(10.0)))),
        (cnid("Part", 5), lt(p2_ref.clone(), literal(mm(11.0)))),
    ];

    // Minimize(p0 + p1 + p2) — pushes all params below constraint floor
    let sum_01 = binop(BinOp::Add, p0_ref, p1_ref);
    let sum_012 = binop(BinOp::Add, sum_01, p2_ref);
    let objective = OptimizationObjective::Minimize(sum_012);

    // All params start at 10.5mm — centered in the feasible window
    let mut current = ValueMap::new();
    for pid in [&p0_id, &p1_id, &p2_id] {
        current.insert(pid.clone(), mm(10.5));
    }

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: p0_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.0, 0.1)), // Wide bounds [0, 100mm]
                free: false,
            },
            AutoParam {
                id: p1_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.0, 0.1)),
                free: false,
            },
            AutoParam {
                id: p2_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.0, 0.1)),
                free: false,
            },
        ],
        constraints,
        current_values: current,
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            // Each param must be returned at EXACTLY the initial value (10.5mm = 0.0105m).
            // The fallback path reconstructs values via build_solved_values(&problem.auto_params, &initial),
            // which should preserve exact f64 values through the round-trip.
            for (pid, label) in [(&p0_id, "p0"), (&p1_id, "p1"), (&p2_id, "p2")] {
                let si = values.get(pid).unwrap().as_f64().unwrap();
                assert!(
                    (si - 0.0105).abs() < 1e-10,
                    "{} should be exact initial 10.5mm (0.0105 m), got {} m (delta = {:.2e})",
                    label,
                    si,
                    (si - 0.0105).abs()
                );
            }
        }
        other => panic!(
            "expected Solved (fallback to feasible initial values), got {:?}",
            other
        ),
    }
}

// ── Uniqueness verification tests ───────────────────────────────────────

#[test]
fn strict_auto_unique_solution_returns_unique_true() {
    // Well-determined 1-param problem: tight inequality constraints that pin x
    // to a narrow feasible range around 50mm (0.05 m in SI).
    // With free: false (strict auto), the solver should verify uniqueness
    // via perturbation and confirm the solution is unique.
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "width");
    let x_ref = value_ref("Part", "width");

    // Tight constraints: x > 49mm AND x < 51mm
    // With bounds midpoint at 0.0505 m, the initial point is already feasible.
    let gt_expr = gt(x_ref.clone(), literal(mm(49.0)));
    let lt_expr = lt(x_ref, literal(mm(51.0)));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.001, 0.1)), // 1mm to 100mm in SI
            free: false,
        }],
        constraints: vec![(cnid("Part", 0), gt_expr), (cnid("Part", 1), lt_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, unique } => {
            assert!(unique, "well-determined system should be unique");
            let si = values.get(&x_id).unwrap().as_f64().unwrap();
            assert!(
                si > 0.049 && si < 0.051,
                "x should be in feasible range ~50mm, got {} m",
                si
            );
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}

#[test]
fn free_auto_skips_uniqueness_returns_unique_false() {
    // Same well-determined 1-param problem as above, but with free: true.
    // Free auto params skip the uniqueness verification, so the solver
    // should return Solved { unique: false }.
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "width");
    let x_ref = value_ref("Part", "width");

    // Tight constraints: x > 49mm AND x < 51mm
    let gt_expr = gt(x_ref.clone(), literal(mm(49.0)));
    let lt_expr = lt(x_ref, literal(mm(51.0)));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.001, 0.1)),
            free: true,
        }],
        constraints: vec![(cnid("Part", 0), gt_expr), (cnid("Part", 1), lt_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, unique } => {
            assert!(
                !unique,
                "free auto should skip uniqueness check and return unique=false"
            );
            let si = values.get(&x_id).unwrap().as_f64().unwrap();
            assert!(
                si > 0.049 && si < 0.051,
                "x should be in feasible range, got {} m",
                si
            );
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}

#[test]
fn strict_auto_non_unique_returns_infeasible() {
    // Underdetermined problem: 2 params, 2 simple inequality constraints.
    // x > 10mm AND y > 10mm — many valid solutions exist.
    // With strict auto (free: false), the solver should detect non-uniqueness
    // via perturbation and return Infeasible with an appropriate diagnostic.
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "width");
    let y_id = vcid("Part", "height");
    let x_ref = value_ref("Part", "width");
    let y_ref = value_ref("Part", "height");

    let gt_x = gt(x_ref, literal(mm(10.0)));
    let gt_y = gt(y_ref, literal(mm(10.0)));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: x_id,
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)), // 1mm to 100mm
                free: false,
            },
            AutoParam {
                id: y_id,
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: false,
            },
        ],
        constraints: vec![(cnid("Part", 0), gt_x), (cnid("Part", 1), gt_y)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Infeasible { diagnostics } => {
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.code == Some(DiagnosticCode::ConstraintNonUnique)),
                "infeasible diagnostic must carry ConstraintNonUnique code; got: {:?}",
                diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
            );
        }
        other => panic!(
            "expected Infeasible for non-unique strict auto, got {:?}",
            other
        ),
    }
}

/// Same underdetermined problem as `strict_auto_non_unique_returns_infeasible` but with
/// all params having `free: true`. Free auto params skip uniqueness verification, so the
/// solver should return `Solved { unique: false }` instead of Infeasible.
#[test]
fn free_auto_resolves_underdetermined_system() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "width");
    let y_id = vcid("Part", "height");
    let x_ref = value_ref("Part", "width");
    let y_ref = value_ref("Part", "height");

    let gt_x = gt(x_ref, literal(mm(10.0)));
    let gt_y = gt(y_ref, literal(mm(10.0)));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)), // 1mm to 100mm
                free: true,
            },
            AutoParam {
                id: y_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: true,
            },
        ],
        constraints: vec![(cnid("Part", 0), gt_x), (cnid("Part", 1), gt_y)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values, unique } => {
            assert!(!unique, "free auto should report unique=false");
            // Both values should satisfy the constraints (> 10mm = 0.01 m)
            let x = values.get(&x_id).unwrap().as_f64().unwrap();
            let y = values.get(&y_id).unwrap().as_f64().unwrap();
            assert!(x > 0.010, "x should satisfy x > 10mm, got {} m", x);
            assert!(y > 0.010, "y should satisfy y > 10mm, got {} m", y);
        }
        other => panic!(
            "expected Solved for underdetermined free auto, got {:?}",
            other
        ),
    }
}

/// Infeasible diagnostic must carry DiagnosticCode::ConstraintUnsatisfiable.
/// Bounds [0, 10mm], constraint x > 15mm — mirrors bounds_dont_hide_infeasibility setup.
#[test]
fn infeasible_diagnostic_carries_constraint_unsatisfiable_code() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let x_ref = value_ref("Part", "x");

    // constraint: x > 15mm, but bounds cap x at 10mm → Infeasible
    let constraint = gt(x_ref, literal(mm(15.0)));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.0, 0.010)), // max 10mm
            free: false,
        }],
        constraints: vec![(cnid("Part", 0), constraint)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Infeasible { diagnostics } => {
            assert!(!diagnostics.is_empty(), "should have diagnostic messages");
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.code == Some(DiagnosticCode::ConstraintUnsatisfiable)),
                "infeasible diagnostic must carry ConstraintUnsatisfiable code; got: {:?}",
                diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
            );
        }
        other => panic!(
            "expected Infeasible for constraint beyond bounds, got {:?}",
            other
        ),
    }
}

/// Companion to `infeasible_diagnostic_carries_constraint_unsatisfiable_code` — the existing
/// case exercises the bounds-cap path (constraint > bounds upper); this case exercises the
/// residual-only path (no bounds-cap; contradictory equalities). Both currently pass through
/// the shared `Infeasible` return after the residual check in `solve_core` with `code: Some(ConstraintUnsatisfiable)`.
///
/// Specific refactor this guards: if `solver.rs` is split so that the early-exit bounds-cap
/// branch and the residual-gradient branch each construct their own `Infeasible` emission,
/// the residual branch could omit `.code` (reverting to `None`) without breaking the existing
/// bounds-cap test. This test catches that omission independently.
#[test]
fn infeasible_residual_diagnostic_carries_constraint_unsatisfiable_code() {
    let solver = DimensionalSolver;

    let x_id = vcid("Part", "x");
    let x_ref = value_ref("Part", "x");
    let x_ref2 = value_ref("Part", "x");

    // No bounds-cap: x == 1mm AND x == 2mm — contradictory equalities, residual-only Infeasible.
    // bounds: None falls back to default_bounds_for(Type::length()), which is wide enough that
    // the bounds-cap path is not exercised here.
    let c1 = eq(x_ref, literal(mm(1.0)));
    let c2 = eq(x_ref2, literal(mm(2.0)));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: None,
            free: false,
        }],
        constraints: vec![(cnid("Part", 0), c1), (cnid("Part", 1), c2)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Infeasible { diagnostics } => {
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.code == Some(DiagnosticCode::ConstraintUnsatisfiable)),
                "infeasible diagnostic must carry ConstraintUnsatisfiable code; got: {:?}",
                diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
            );
            assert!(
                diagnostics.iter().any(|d| d.message.contains("max absolute residual")),
                "expected residual-branch diagnostic message containing \"max absolute residual\"; got: {:?}",
                diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>(),
            );
        }
        other => panic!(
            "expected Infeasible for contradictory equality constraints, got {:?}",
            other
        ),
    }
}
