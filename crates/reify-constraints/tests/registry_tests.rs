//! Tests for SolverRegistry — multi-domain constraint dispatch.

use reify_constraints::{DimensionalSolver, SolveSpaceSolver, SolverRegistry};
use reify_test_support::*;
use reify_types::{
    AutoParam, BinOp, CompiledExpr, CompiledExprKind, ConstraintSolver, ContentHash,
    DimensionVector, OptimizationObjective, ResolutionProblem, ResolvedFunction, SolveResult, Type,
    Value, ValueMap,
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
            free: false,
        }],
        constraints: vec![(cnid("Bracket", 0), gt_expr), (cnid("Bracket", 1), lt_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    // Both should produce Solved
    let dim_result = dim_solver.solve(&problem);
    let reg_result = registry.solve(&problem);

    match (&dim_result, &reg_result) {
        (SolveResult::Solved { values: v1, .. }, SolveResult::Solved { values: v2, .. }) => {
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
                free: true, // not testing uniqueness — range constraint is underdetermined
            },
            AutoParam {
                id: b_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: true,
            },
        ],
        constraints: vec![(cnid("Part", 0), c1), (cnid("Part", 1), c2)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = registry.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
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
            free: true, // not testing uniqueness — range constraint is underdetermined
        }],
        constraints: vec![(cnid("Part", 0), c1), (cnid("Part", 1), c2)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
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
                free: true, // not testing uniqueness — range constraints are underdetermined
            },
            AutoParam {
                id: b_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: true,
            },
        ],
        constraints: vec![
            (cnid("Part", 0), c1),
            (cnid("Part", 1), c2),
            (cnid("Part", 2), c3),
        ],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = registry.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
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
            free: true, // not testing uniqueness — compound range constraint is underdetermined
        }],
        constraints: vec![(cnid("Part", 0), compound)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = registry.solve(&problem);
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
            free: false,
        }],
        constraints: vec![(cnid("Part", 0), constraint)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = registry.solve(&problem);
    match result {
        SolveResult::Infeasible { diagnostics } => {
            assert!(!diagnostics.is_empty(), "should have diagnostics");
        }
        other => panic!("expected Infeasible through registry, got {:?}", other),
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
            free: false,
        }],
        constraints: vec![(cnid("Part", 0), constraint)],
        current_values: current,
        objective: None,
        functions: vec![].into(),
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
            free: false,
        }],
        constraints: vec![(cnid("Bracket", 0), gt_expr), (cnid("Bracket", 1), lt_expr)],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = registry.solve(&problem);
    // Either Solved (near 20mm) or Infeasible (boundary edge case) —
    // same as DimensionalSolver behavior
    match result {
        SolveResult::Solved { values, .. } => {
            let si = values.get(&thickness_id).unwrap().as_f64().unwrap();
            assert!(
                si > 0.017,
                "maximized value should be near 20mm, got {}",
                si
            );
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
        functions: vec![].into(),
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
/// objective `Maximize(a + b)` references both, so the components must
/// be merged. With the bug, only the first matching component gets the
/// objective and the other param is solved purely for feasibility — it
/// won't be maximized toward its upper bound.
///
/// Uses Maximize (not Minimize) because the Nelder-Mead solver handles
/// interior-directed optimization more reliably than boundary optimization.
#[test]
fn objective_spanning_independent_components_merges_them() {
    let registry = SolverRegistry::new(Box::new(DimensionalSolver));

    let a_id = vcid("Part", "a");
    let b_id = vcid("Part", "b");

    // Independent constraints: a > 2mm, b > 2mm, a < 20mm, b < 20mm
    let c1 = gt(value_ref("Part", "a"), literal(mm(2.0)));
    let c2 = gt(value_ref("Part", "b"), literal(mm(2.0)));
    let c3 = lt(value_ref("Part", "a"), literal(mm(20.0)));
    let c4 = lt(value_ref("Part", "b"), literal(mm(20.0)));

    // Objective: Maximize(a + b) — references BOTH params
    let obj_expr = binop(BinOp::Add, value_ref("Part", "a"), value_ref("Part", "b"));
    let objective = OptimizationObjective::Maximize(obj_expr);

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: a_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: false,
            },
            AutoParam {
                id: b_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: false,
            },
        ],
        constraints: vec![
            (cnid("Part", 0), c1),
            (cnid("Part", 1), c2),
            (cnid("Part", 2), c3),
            (cnid("Part", 3), c4),
        ],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = registry.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            let a_val = values.get(&a_id).unwrap().as_f64().unwrap();
            let b_val = values.get(&b_id).unwrap().as_f64().unwrap();
            // Both should be maximized toward 20mm (upper feasible bound).
            // Without the fix, only one param would be maximized and the
            // other would sit near the middle of its feasible range.
            assert!(
                a_val > 0.015,
                "a should be near 20mm when maximized, got {} m",
                a_val
            );
            assert!(
                b_val > 0.015,
                "b should be near 20mm when maximized, got {} m",
                b_val
            );
        }
        SolveResult::Infeasible { .. } => {
            // Acceptable for optimization-against-boundary
            // (same tolerance as registry_compat_maximize_objective)
        }
        other => panic!("expected Solved or Infeasible, got {:?}", other),
    }
}

// === SolveSpace geometric solver integration via registry ===

/// Build a geometry function call expression (e.g., std::geo::pt_pt_distance).
fn geo_fn(name: &str, args: Vec<CompiledExpr>, result_type: Type) -> CompiledExpr {
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: name.to_string(),
                qualified_name: format!("std::geo::{}", name),
            },
            args,
        },
        result_type,
        content_hash: ContentHash::of(format!("geo_{}", name).as_bytes()),
    }
}

/// SolverRegistry dispatches geometric constraints to SolveSpaceSolver.
///
/// Creates a registry with DimensionalSolver + SolveSpaceSolver, then sends
/// a geometric constraint (pt_pt_distance via std::geo::*). The classifier
/// identifies it as Geometric, and the registry dispatches to SolveSpaceSolver.
#[test]
fn registry_dispatches_geometric_to_solvespace() {
    let registry = SolverRegistry::with_solvers(
        Box::new(DimensionalSolver),
        Some(Box::new(SolveSpaceSolver)),
        None,
        None,
    );

    let x_id = vcid("Point", "x");
    let y_id = vcid("Point", "y");

    let zero = literal(Value::Scalar {
        si_value: 0.0,
        dimension: DimensionVector::LENGTH,
    });

    // point3d(x, y, 0) — auto params for x, y
    let pt = geo_fn(
        "point3d",
        vec![
            value_ref_typed("Point", "x", Type::length()),
            value_ref_typed("Point", "y", Type::length()),
            zero.clone(),
        ],
        Type::dimensionless_scalar(),
    );

    // origin at (0, 0, 0)
    let origin = geo_fn(
        "point3d",
        vec![zero.clone(), zero.clone(), zero],
        Type::dimensionless_scalar(),
    );

    // distance(pt, origin) == 10mm
    let dist_call = geo_fn("pt_pt_distance", vec![pt, origin], Type::length());
    let constraint_expr = eq(dist_call, literal(mm(10.0)));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            },
            AutoParam {
                id: y_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            },
        ],
        constraints: vec![(cnid("Point", 0), constraint_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = registry.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            let x_val = values.get(&x_id).unwrap().as_f64().unwrap();
            let y_val = values.get(&y_id).unwrap().as_f64().unwrap();
            let actual_dist = (x_val * x_val + y_val * y_val).sqrt();
            assert!(
                (actual_dist - 0.01).abs() < 1e-6,
                "registry should dispatch to SolveSpaceSolver: distance should be ~10mm (0.01m), got {} m",
                actual_dist,
            );
        }
        other => panic!(
            "expected Solved via SolveSpaceSolver dispatch, got {:?}",
            other
        ),
    }
}

/// Mixed dimensional + geometric constraints solved through SolverRegistry.
///
/// Two independent sub-problems:
/// 1. Dimensional: thickness > 2mm AND thickness < 20mm (handled by DimensionalSolver)
/// 2. Geometric: distance(point, origin) == 10mm (handled by SolveSpaceSolver)
///
/// The registry decomposes them, dispatches each to the appropriate solver,
/// and merges the results.
#[test]
fn registry_mixed_dimensional_and_geometric() {
    let registry = SolverRegistry::with_solvers(
        Box::new(DimensionalSolver),
        Some(Box::new(SolveSpaceSolver)),
        None,
        None,
    );

    // --- Dimensional sub-problem ---
    let thickness_id = vcid("Bracket", "thickness");
    let gt_expr = gt(value_ref("Bracket", "thickness"), literal(mm(2.0)));
    let lt_expr = lt(value_ref("Bracket", "thickness"), literal(mm(20.0)));

    // --- Geometric sub-problem ---
    let x_id = vcid("Point", "x");
    let y_id = vcid("Point", "y");

    let zero = literal(Value::Scalar {
        si_value: 0.0,
        dimension: DimensionVector::LENGTH,
    });

    let pt = geo_fn(
        "point3d",
        vec![
            value_ref_typed("Point", "x", Type::length()),
            value_ref_typed("Point", "y", Type::length()),
            zero.clone(),
        ],
        Type::dimensionless_scalar(),
    );

    let origin = geo_fn(
        "point3d",
        vec![zero.clone(), zero.clone(), zero],
        Type::dimensionless_scalar(),
    );

    let dist_call = geo_fn("pt_pt_distance", vec![pt, origin], Type::length());
    let geo_expr = eq(dist_call, literal(mm(15.0)));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: thickness_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: false,
            },
            AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            },
            AutoParam {
                id: y_id.clone(),
                param_type: Type::length(),
                bounds: None,
                free: false,
            },
        ],
        constraints: vec![
            (cnid("Bracket", 0), gt_expr),
            (cnid("Bracket", 1), lt_expr),
            (cnid("Point", 0), geo_expr),
        ],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = registry.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            // Dimensional: thickness in [2mm, 20mm]
            let thickness = values.get(&thickness_id).unwrap().as_f64().unwrap();
            assert!(
                thickness > 0.002 && thickness < 0.020,
                "thickness should be in [2mm, 20mm], got {} m",
                thickness,
            );

            // Geometric: distance ~15mm from origin
            let x_val = values.get(&x_id).unwrap().as_f64().unwrap();
            let y_val = values.get(&y_id).unwrap().as_f64().unwrap();
            let actual_dist = (x_val * x_val + y_val * y_val).sqrt();
            assert!(
                (actual_dist - 0.015).abs() < 1e-6,
                "point distance should be ~15mm (0.015m), got {} m",
                actual_dist,
            );
        }
        other => panic!(
            "expected Solved for mixed dimensional+geometric, got {:?}",
            other
        ),
    }
}

/// Registry merges uniqueness across sub-problems via conjunction:
/// - all_unique=true  → unique=true
/// - any_non_unique   → unique=false
///
/// Uses SequencedMockConstraintSolver to return different `unique` values
/// for each decomposed sub-problem.
#[test]
fn registry_merges_unique_flag() {
    let a_id = vcid("Part", "a");
    let b_id = vcid("Part", "b");

    // Two independent constraints → decomposed into 2 sub-problems
    let c1 = gt(value_ref("Part", "a"), literal(mm(5.0)));
    let c2 = gt(value_ref("Part", "b"), literal(mm(10.0)));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: a_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: true,
            },
            AutoParam {
                id: b_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: true,
            },
        ],
        constraints: vec![(cnid("Part", 0), c1), (cnid("Part", 1), c2)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    // Case 1: first sub-problem unique, second NOT unique → merged = NOT unique
    {
        let mut vals_a = std::collections::HashMap::new();
        vals_a.insert(a_id.clone(), mm(6.0));
        let mut vals_b = std::collections::HashMap::new();
        vals_b.insert(b_id.clone(), mm(11.0));

        let mock = SequencedMockConstraintSolver::new(vec![
            SolveResult::Solved {
                values: vals_a,
                unique: true,
            },
            SolveResult::Solved {
                values: vals_b,
                unique: false,
            },
        ]);
        let registry = SolverRegistry::new(Box::new(mock));
        let result = registry.solve(&problem);
        match result {
            SolveResult::Solved { unique, .. } => {
                assert!(
                    !unique,
                    "any sub-problem with unique=false should make merged unique=false"
                );
            }
            other => panic!("expected Solved, got {:?}", other),
        }
    }

    // Case 2: both sub-problems unique → merged = unique
    {
        let mut vals_a = std::collections::HashMap::new();
        vals_a.insert(a_id.clone(), mm(6.0));
        let mut vals_b = std::collections::HashMap::new();
        vals_b.insert(b_id.clone(), mm(11.0));

        let mock = SequencedMockConstraintSolver::new(vec![
            SolveResult::Solved {
                values: vals_a,
                unique: true,
            },
            SolveResult::Solved {
                values: vals_b,
                unique: true,
            },
        ]);
        let registry = SolverRegistry::new(Box::new(mock));
        let result = registry.solve(&problem);
        match result {
            SolveResult::Solved { unique, .. } => {
                assert!(
                    unique,
                    "all sub-problems unique=true should make merged unique=true"
                );
            }
            other => panic!("expected Solved, got {:?}", other),
        }
    }
}
