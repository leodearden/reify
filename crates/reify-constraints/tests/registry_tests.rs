//! Tests for SolverRegistry — multi-domain constraint dispatch.

use reify_constraints::{DimensionalSolver, SolverRegistry};
use reify_test_support::*;
use reify_types::{
    AutoParam, BinOp, ConstraintSolver, DimensionVector, OptimizationObjective, ResolutionProblem,
    SolveResult, Type, Value, ValueMap,
};

/// Basic dispatch: SolverRegistry with DimensionalSolver as fallback
/// produces same results as DimensionalSolver alone for a simple problem.
#[test]
fn registry_matches_dimensional_solver_simple_feasibility() {
    let dim_solver = DimensionalSolver;
    let registry = SolverRegistry::new(Box::new(DimensionalSolver));

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

    // Both should produce Solved
    let dim_result = dim_solver.solve(&problem);
    let reg_result = registry.solve(&problem);

    match (&dim_result, &reg_result) {
        (SolveResult::Solved { values: v1 }, SolveResult::Solved { values: v2 }) => {
            let si1 = v1.get(&thickness_id).unwrap().as_f64().unwrap();
            let si2 = v2.get(&thickness_id).unwrap().as_f64().unwrap();
            // Both should be in feasible range
            assert!(si1 > 0.002 && si1 < 0.020, "dim_solver: got {}", si1);
            assert!(si2 > 0.002 && si2 < 0.020, "registry: got {}", si2);
        }
        _ => panic!(
            "expected both Solved, got dim={:?}, reg={:?}",
            dim_result, reg_result
        ),
    }
}

/// Problem decomposed into 2 independent sub-problems → both solved,
/// merged result contains all param values.
#[test]
fn registry_solves_independent_subproblems() {
    let registry = SolverRegistry::new(Box::new(DimensionalSolver));

    let a_id = vcid("Part", "a");
    let b_id = vcid("Part", "b");

    // a > 5mm (independent sub-problem 1)
    let c1 = gt(value_ref("Part", "a"), literal(mm(5.0)));
    // b > 10mm (independent sub-problem 2)
    let c2 = gt(value_ref("Part", "b"), literal(mm(10.0)));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: a_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
            },
            AutoParam {
                id: b_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
            },
        ],
        constraints: vec![
            (cnid("Part", 0), c1),
            (cnid("Part", 1), c2),
        ],
        current_values: ValueMap::new(),
        objective: None,
    };

    let result = registry.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            assert!(values.contains_key(&a_id), "should have value for a");
            assert!(values.contains_key(&b_id), "should have value for b");

            let a_val = values.get(&a_id).unwrap().as_f64().unwrap();
            let b_val = values.get(&b_id).unwrap().as_f64().unwrap();
            assert!(a_val > 0.005, "a should satisfy > 5mm, got {}", a_val);
            assert!(b_val > 0.010, "b should satisfy > 10mm, got {}", b_val);
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}

/// Fallback solver is used when no specialized solver is registered
/// for a domain (here using dimensional solver as universal fallback).
#[test]
fn registry_uses_fallback_for_all_domains() {
    let registry = SolverRegistry::new(Box::new(DimensionalSolver));

    let x_id = vcid("Part", "x");

    // Simple constraint through fallback
    let c1 = gt(value_ref("Part", "x"), literal(mm(1.0)));
    let c2 = lt(value_ref("Part", "x"), literal(mm(50.0)));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.001, 0.1)),
        }],
        constraints: vec![
            (cnid("Part", 0), c1),
            (cnid("Part", 1), c2),
        ],
        current_values: ValueMap::new(),
        objective: None,
    };

    let result = registry.solve(&problem);
    assert!(
        matches!(result, SolveResult::Solved { .. }),
        "fallback should solve simple feasibility"
    );
}

/// Cross-domain iteration: 2 constraints sharing param b — they merge
/// into a single component and solve correctly via fallback (dimensional).
#[test]
fn cross_domain_shared_param_solved_via_fallback() {
    let registry = SolverRegistry::new(Box::new(DimensionalSolver));

    let a_id = vcid("Part", "a");
    let b_id = vcid("Part", "b");

    // C1: a > b (Dimensional — both are length-typed value refs)
    let c1 = gt(value_ref("Part", "a"), value_ref("Part", "b"));

    // C2: b > 5mm (also Dimensional, shares param b with C1)
    let c2 = gt(value_ref("Part", "b"), literal(mm(5.0)));

    // C3: a < 50mm (bounds on a)
    let c3 = lt(value_ref("Part", "a"), literal(mm(50.0)));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: a_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
            },
            AutoParam {
                id: b_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
            },
        ],
        constraints: vec![
            (cnid("Part", 0), c1),
            (cnid("Part", 1), c2),
            (cnid("Part", 2), c3),
        ],
        current_values: ValueMap::new(),
        objective: None,
    };

    let result = registry.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let a_val = values.get(&a_id).unwrap().as_f64().unwrap();
            let b_val = values.get(&b_id).unwrap().as_f64().unwrap();
            // a > b, b > 5mm, a < 50mm
            assert!(a_val > b_val, "a ({}) should be > b ({})", a_val, b_val);
            assert!(b_val > 0.005, "b should satisfy > 5mm, got {}", b_val);
            assert!(a_val < 0.050, "a should satisfy < 50mm, got {}", a_val);
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}

/// Backward compatibility: existing solver_integration test scenarios
/// produce identical behavior through SolverRegistry.
#[test]
fn registry_backward_compat_compound_constraint() {
    let registry = SolverRegistry::new(Box::new(DimensionalSolver));

    let x_id = vcid("Part", "x");

    // (x > 5mm) AND (x < 50mm) — as a single compound constraint
    let compound = and(
        gt(value_ref("Part", "x"), literal(mm(5.0))),
        lt(value_ref("Part", "x"), literal(mm(50.0))),
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

    let result = registry.solve(&problem);
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

// === Backward-compatibility integration tests ===
// These mirror scenarios from solver_integration.rs to verify
// SolverRegistry produces identical behavior to DimensionalSolver.

/// Infeasible problem: bounds [0, 10mm], constraint x > 15mm.
/// SolverRegistry must report Infeasible just like DimensionalSolver.
#[test]
fn registry_compat_infeasible_bounds() {
    let registry = SolverRegistry::new(Box::new(DimensionalSolver));

    let x_id = vcid("Part", "x");
    let constraint = gt(
        value_ref("Part", "x"),
        literal(Value::Scalar {
            si_value: 0.015,
            dimension: DimensionVector::LENGTH,
        }),
    );

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::length(),
            bounds: Some((0.0, 0.010)),
        }],
        constraints: vec![(cnid("Part", 0), constraint)],
        current_values: ValueMap::new(),
        objective: None,
    };

    let result = registry.solve(&problem);
    match result {
        SolveResult::Infeasible { diagnostics } => {
            assert!(!diagnostics.is_empty(), "should have diagnostics");
        }
        other => panic!(
            "expected Infeasible through registry, got {:?}",
            other
        ),
    }
}

/// Small violation: x > 2.0 but bounds max at 1.9999999.
/// Registry must NOT report Solved (same as DimensionalSolver).
#[test]
fn registry_compat_false_negative_small_violation() {
    let registry = SolverRegistry::new(Box::new(DimensionalSolver));

    let x_id = vcid("Part", "x");
    let constraint = gt(
        value_ref("Part", "x"),
        literal(Value::Scalar {
            si_value: 2.0,
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

    let result = registry.solve(&problem);
    assert!(
        !matches!(result, SolveResult::Solved { .. }),
        "should NOT report Solved when constraint is violated by 1e-7 (through registry)"
    );
}

/// Maximize objective through registry: thickness constrained, maximize.
#[test]
fn registry_compat_maximize_objective() {
    let registry = SolverRegistry::new(Box::new(DimensionalSolver));

    let thickness_id = vcid("Bracket", "thickness");
    let thickness_ref = value_ref("Bracket", "thickness");

    let gt_expr = gt(thickness_ref.clone(), literal(mm(2.0)));
    let lt_expr = lt(thickness_ref.clone(), literal(mm(20.0)));
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

    let result = registry.solve(&problem);
    // Either Solved (near 20mm) or Infeasible (boundary edge case) —
    // same as DimensionalSolver behavior
    match result {
        SolveResult::Solved { values } => {
            let si = values.get(&thickness_id).unwrap().as_f64().unwrap();
            assert!(si > 0.017, "maximized value should be near 20mm, got {}", si);
        }
        SolveResult::Infeasible { .. } => {
            // Acceptable for optimization-against-boundary
        }
        other => panic!("expected Solved or Infeasible, got {:?}", other),
    }
}

/// Empty problem: no auto params → trivially solved.
#[test]
fn registry_compat_empty_problem() {
    let registry = SolverRegistry::new(Box::new(DimensionalSolver));

    let problem = ResolutionProblem {
        auto_params: vec![],
        constraints: vec![],
        current_values: ValueMap::new(),
        objective: None,
    };

    let result = registry.solve(&problem);
    assert!(
        matches!(result, SolveResult::Solved { .. }),
        "empty problem should trivially solve"
    );
}

/// Static assertion: SolverRegistry must be Send + Sync.
///
/// Since `ConstraintSolver: Send + Sync` (supertrait bound), all
/// `Box<dyn ConstraintSolver>` fields are automatically Send + Sync,
/// and the compiler auto-derives Send + Sync for the struct.
/// This test verifies that property holds without relying on manual
/// `unsafe impl` blocks.
#[test]
fn solver_registry_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SolverRegistry>();
}

/// Objective spanning independent components must merge them.
///
/// Without objective-aware decomposition, params `a` and `b` end up in
/// separate components because their constraints are independent. The
/// objective `Minimize(a + b)` references both, so the components must
/// be merged. With the bug, only the first matching component gets the
/// objective and the other param is solved purely for feasibility — it
/// won't be minimized toward its lower bound.
#[test]
fn objective_spanning_independent_components_merges_them() {
    let registry = SolverRegistry::new(Box::new(DimensionalSolver));

    let a_id = vcid("Part", "a");
    let b_id = vcid("Part", "b");

    // Independent constraints: a > 5mm, b > 5mm
    let c1 = gt(value_ref("Part", "a"), literal(mm(5.0)));
    let c2 = gt(value_ref("Part", "b"), literal(mm(5.0)));

    // Objective: Minimize(a + b) — references BOTH params
    let obj_expr = binop(
        BinOp::Add,
        value_ref("Part", "a"),
        value_ref("Part", "b"),
    );
    let objective = OptimizationObjective::Minimize(obj_expr);

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: a_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
            },
            AutoParam {
                id: b_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
            },
        ],
        constraints: vec![
            (cnid("Part", 0), c1),
            (cnid("Part", 1), c2),
        ],
        current_values: ValueMap::new(),
        objective: Some(objective),
    };

    let result = registry.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let a_val = values.get(&a_id).unwrap().as_f64().unwrap();
            let b_val = values.get(&b_id).unwrap().as_f64().unwrap();
            // Both should be minimized toward 5mm (lower feasible bound)
            assert!(
                a_val < 0.010,
                "a should be near 5mm when minimized, got {} m",
                a_val
            );
            assert!(
                b_val < 0.010,
                "b should be near 5mm when minimized, got {} m",
                b_val
            );
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}
