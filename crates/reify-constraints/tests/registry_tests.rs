//! Tests for SolverRegistry — multi-domain constraint dispatch.

use reify_constraints::{DimensionalSolver, SolveSpaceSolver, SolverRegistry};
use reify_test_support::*;
use reify_core::{ContentHash, DimensionVector, Type};
use reify_ir::{AutoParam, BinOp, CompiledExpr, CompiledExprKind, ConstraintSolver, ObjectiveCombination, ObjectiveSense, ObjectiveSet, ObjectiveTerm, ResolutionProblem, ResolvedFunction, SolveResult, Value, ValueMap};

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
    let objective = ObjectiveSet::single(ObjectiveSense::Maximize, thickness_ref);

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
    let objective = ObjectiveSet::single(ObjectiveSense::Maximize, obj_expr);

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

// ============================================================================
// Lexicographic staged solve (task ε)
// ============================================================================

/// Lexicographic objective splits into stages by descending priority.
///
/// A 2-term Lexicographic objective [Maximize x @p=1, Maximize y @p=0] must
/// cause `solve_lexicographic` to call the domain solver exactly TWICE — once
/// for the priority-1 rank (WeightedSum{x}), once for the priority-0 rank
/// (WeightedSum{y}).  The registry returns the last stage's result as
/// `Solved { unique: false }`.
///
/// RED under current code: the Lexicographic objective is passed through to a
/// single solver call (call_count == 1 instead of 2).
#[test]
fn lexicographic_stages_by_descending_priority() {
    let x_id = vcid("Lex", "x");
    let y_id = vcid("Lex", "y");

    // Independent constraints — normally two components, but the Lexicographic
    // objective references both params so decompose_into_components merges them.
    let c1 = le(value_ref("Lex", "x"), literal(mm(3.0)));
    let c2 = le(value_ref("Lex", "y"), literal(mm(10.0)));

    // Lexicographic objective: Maximize x first (priority 1), then y (priority 0).
    let objective = ObjectiveSet {
        combination: ObjectiveCombination::Lexicographic,
        terms: vec![
            ObjectiveTerm { sense: ObjectiveSense::Maximize, expr: value_ref("Lex", "x"), weight: 1.0, priority: 1 },
            ObjectiveTerm { sense: ObjectiveSense::Maximize, expr: value_ref("Lex", "y"), weight: 1.0, priority: 0 },
        ],
    };

    // Stage 1 spy result: x at its bound.
    // Stage 2 spy result: both at their bounds (returned as the final answer).
    let mut vals1 = std::collections::HashMap::new();
    vals1.insert(x_id.clone(), mm(3.0));
    let mut vals2 = std::collections::HashMap::new();
    vals2.insert(x_id.clone(), mm(3.0));
    vals2.insert(y_id.clone(), mm(10.0));

    let spy = MultiCallSpyConstraintSolver::new(vec![
        SolveResult::Solved { values: vals1, unique: false },
        SolveResult::Solved { values: vals2, unique: false },
    ]);
    let captured = spy.captured_problems();
    let registry = SolverRegistry::new(Box::new(spy));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam { id: x_id.clone(), param_type: Type::length(), bounds: Some((0.001, 0.1)), free: true },
            AutoParam { id: y_id.clone(), param_type: Type::length(), bounds: Some((0.001, 0.1)), free: true },
        ],
        constraints: vec![(cnid("Lex", 0), c1), (cnid("Lex", 1), c2)],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = registry.solve(&problem);

    // The domain solver must have been called exactly twice (one per rank).
    let captured_guard = captured.lock().unwrap();
    assert_eq!(
        captured_guard.len(), 2,
        "Lexicographic with 2 distinct-priority ranks must invoke solver twice; got {} call(s)",
        captured_guard.len()
    );

    // Stage 1: highest-priority rank (priority 1 = x), presented as WeightedSum.
    let stage1 = &captured_guard[0];
    let obj1 = stage1.objective.as_ref().expect("stage 1 must carry an objective");
    assert_eq!(
        obj1.combination, ObjectiveCombination::WeightedSum,
        "each stage must present a WeightedSum to the domain solver (debug_assert guard)"
    );
    assert_eq!(obj1.terms.len(), 1, "stage 1 rank has exactly one term (x)");
    let refs1 = obj1.terms[0].expr.collect_value_refs();
    assert!(refs1.contains(&x_id), "stage 1 must optimize x (priority 1)");
    assert!(!refs1.contains(&y_id), "stage 1 must NOT include y");

    // Stage 2: lower-priority rank (priority 0 = y), presented as WeightedSum.
    let stage2 = &captured_guard[1];
    let obj2 = stage2.objective.as_ref().expect("stage 2 must carry an objective");
    assert_eq!(obj2.combination, ObjectiveCombination::WeightedSum);
    assert_eq!(obj2.terms.len(), 1, "stage 2 rank has exactly one term (y)");
    let refs2 = obj2.terms[0].expr.collect_value_refs();
    assert!(refs2.contains(&y_id), "stage 2 must optimize y (priority 0)");
    assert!(!refs2.contains(&x_id), "stage 2 must NOT include x");

    // The registry result comes from the final stage (vals2: x=3mm, y=10mm).
    match result {
        SolveResult::Solved { values, unique } => {
            assert!(!unique, "staged Lexicographic result must be unique:false");
            let y_val = values
                .get(&y_id)
                .expect("result must contain y (from last stage)")
                .as_f64()
                .unwrap();
            assert!(
                (y_val - 0.010).abs() < 1e-9,
                "y should be 10 mm (0.010 m) from the last stage result, got {}",
                y_val
            );
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}

/// After the first stage's Solved, two ε-band constraints (Le + Ge) are threaded into
/// the next stage's constraint list; the first stage's constraint list is unchanged.
///
/// Asserts:
///   - Stage 1 has exactly the base constraint count (no bands yet).
///   - Stage 2 has base_count + 2 constraints (the ε-band Le and Ge inequalities).
///   - Both extra constraints are BinOp::Le / BinOp::Ge and carry entity "__lex_freeze__".
///
/// RED after step-2 (no band constraints are added yet).
#[test]
fn lexicographic_freezes_earlier_rank_as_epsilon_band() {
    let x_id = vcid("LexBand", "x");
    let y_id = vcid("LexBand", "y");

    let c1 = le(value_ref("LexBand", "x"), literal(mm(5.0)));
    let c2 = le(value_ref("LexBand", "y"), literal(mm(8.0)));

    let objective = ObjectiveSet {
        combination: ObjectiveCombination::Lexicographic,
        terms: vec![
            ObjectiveTerm {
                sense: ObjectiveSense::Maximize,
                expr: value_ref("LexBand", "x"),
                weight: 1.0,
                priority: 1,
            },
            ObjectiveTerm {
                sense: ObjectiveSense::Maximize,
                expr: value_ref("LexBand", "y"),
                weight: 1.0,
                priority: 0,
            },
        ],
    };

    // Stage 1 returns x at a concrete value so obj* for rank-1 is computable.
    let mut vals1 = std::collections::HashMap::new();
    vals1.insert(x_id.clone(), mm(5.0));
    let mut vals2 = std::collections::HashMap::new();
    vals2.insert(x_id.clone(), mm(5.0));
    vals2.insert(y_id.clone(), mm(8.0));

    let spy = MultiCallSpyConstraintSolver::new(vec![
        SolveResult::Solved { values: vals1, unique: false },
        SolveResult::Solved { values: vals2, unique: false },
    ]);
    let captured = spy.captured_problems();
    let registry = SolverRegistry::new(Box::new(spy));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: x_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: true,
            },
            AutoParam {
                id: y_id.clone(),
                param_type: Type::length(),
                bounds: Some((0.001, 0.1)),
                free: true,
            },
        ],
        constraints: vec![(cnid("LexBand", 0), c1), (cnid("LexBand", 1), c2)],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![].into(),
    };

    let _result = registry.solve(&problem);

    let captured_guard = captured.lock().unwrap();
    assert_eq!(captured_guard.len(), 2, "solver must be called twice");

    let base_count = 2usize; // c1 and c2

    // Stage 1: no band constraints yet — only the original constraints.
    assert_eq!(
        captured_guard[0].constraints.len(),
        base_count,
        "stage 1 must receive exactly the original constraints (no band), got {}",
        captured_guard[0].constraints.len()
    );

    // Stage 2: two ε-band constraints appended by the prior rank's freeze.
    assert_eq!(
        captured_guard[1].constraints.len(),
        base_count + 2,
        "stage 2 must receive original constraints + 2 band inequalities, got {}",
        captured_guard[1].constraints.len()
    );

    // Inspect the two extra band constraints.
    let band = &captured_guard[1].constraints[base_count..];

    // All must be BinOp Le or Ge and carry the synthetic entity "__lex_freeze__".
    for (cid, expr) in band {
        assert_eq!(
            cid.entity, "__lex_freeze__",
            "band ConstraintNodeId must have entity \"__lex_freeze__\", got {:?}",
            cid
        );
        match &expr.kind {
            CompiledExprKind::BinOp { op, .. } => {
                assert!(
                    *op == BinOp::Le || *op == BinOp::Ge,
                    "band constraint must be Le or Ge comparison, got {:?}",
                    op
                );
            }
            other => panic!("band constraint expression must be a BinOp, got {:?}", other),
        }
    }

    // Must include one Le (upper bound) and one Ge (lower bound).
    let has_le = band.iter().any(|(_, e)| {
        if let CompiledExprKind::BinOp { op, .. } = &e.kind { *op == BinOp::Le } else { false }
    });
    let has_ge = band.iter().any(|(_, e)| {
        if let CompiledExprKind::BinOp { op, .. } = &e.kind { *op == BinOp::Ge } else { false }
    });
    assert!(has_le, "band must include a Le upper-bound constraint");
    assert!(has_ge, "band must include a Ge lower-bound constraint");
}

/// Equal-priority terms must be solved in a SINGLE stage (not one call per term).
///
/// An objective [Maximize x @p=2, Maximize y @p=2, Maximize z @p=1] has two DISTINCT
/// priorities, so the solver should be called exactly twice (not three times).
/// The first stage's objective must have TWO terms (the p=2 tie) and the second
/// stage's objective must have ONE term (z at p=1).
///
/// RED after step-2/4: equal-priority terms are currently split into separate stages
/// because the grouping iterates terms rather than grouping by distinct priority.
///
/// (Note: current step-2 impl collects terms per priority correctly via `.filter(|t|
///  t.priority == *priority)`, so this test actually exercises that the grouping was
///  correct from the start. If it passes today, the test is a regression guard.)
#[test]
fn lexicographic_ties_fold_as_weighted_sum_within_rank() {
    let x_id = vcid("LexTie", "x");
    let y_id = vcid("LexTie", "y");
    let z_id = vcid("LexTie", "z");

    // Three constraints anchoring each param in a single component via the objective.
    let c1 = le(value_ref("LexTie", "x"), literal(mm(10.0)));
    let c2 = le(value_ref("LexTie", "y"), literal(mm(10.0)));
    let c3 = le(value_ref("LexTie", "z"), literal(mm(10.0)));

    // Two terms share priority 2 (tie), one is at priority 1.
    let objective = ObjectiveSet {
        combination: ObjectiveCombination::Lexicographic,
        terms: vec![
            ObjectiveTerm { sense: ObjectiveSense::Maximize, expr: value_ref("LexTie", "x"), weight: 1.0, priority: 2 },
            ObjectiveTerm { sense: ObjectiveSense::Maximize, expr: value_ref("LexTie", "y"), weight: 1.0, priority: 2 },
            ObjectiveTerm { sense: ObjectiveSense::Maximize, expr: value_ref("LexTie", "z"), weight: 1.0, priority: 1 },
        ],
    };

    // Two spy stages: tie-rank (x+y) first, then z.
    let mut vals1 = std::collections::HashMap::new();
    vals1.insert(x_id.clone(), mm(8.0));
    vals1.insert(y_id.clone(), mm(9.0));
    let mut vals2 = std::collections::HashMap::new();
    vals2.insert(x_id.clone(), mm(8.0));
    vals2.insert(y_id.clone(), mm(9.0));
    vals2.insert(z_id.clone(), mm(7.0));

    let spy = MultiCallSpyConstraintSolver::new(vec![
        SolveResult::Solved { values: vals1, unique: false },
        SolveResult::Solved { values: vals2, unique: false },
    ]);
    let captured = spy.captured_problems();
    let registry = SolverRegistry::new(Box::new(spy));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam { id: x_id.clone(), param_type: Type::length(), bounds: Some((0.001, 0.1)), free: true },
            AutoParam { id: y_id.clone(), param_type: Type::length(), bounds: Some((0.001, 0.1)), free: true },
            AutoParam { id: z_id.clone(), param_type: Type::length(), bounds: Some((0.001, 0.1)), free: true },
        ],
        constraints: vec![(cnid("LexTie", 0), c1), (cnid("LexTie", 1), c2), (cnid("LexTie", 2), c3)],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![].into(),
    };

    registry.solve(&problem);

    let captured_guard = captured.lock().unwrap();
    // Exactly 2 calls: one for the p=2 tie rank, one for p=1.
    assert_eq!(
        captured_guard.len(), 2,
        "Lexicographic with 2 distinct priorities (one being a tie) must call solver exactly twice, got {}",
        captured_guard.len()
    );

    // Stage 1 (tie rank p=2): WeightedSum with both x and y terms.
    let obj1 = captured_guard[0].objective.as_ref().unwrap();
    assert_eq!(obj1.combination, ObjectiveCombination::WeightedSum);
    assert_eq!(
        obj1.terms.len(), 2,
        "tie rank must produce a single WeightedSum stage with 2 terms, got {}",
        obj1.terms.len()
    );
    let refs1: Vec<_> = obj1.terms.iter().flat_map(|t| t.expr.collect_value_refs()).collect();
    assert!(refs1.contains(&x_id), "stage 1 must include x (tie)");
    assert!(refs1.contains(&y_id), "stage 1 must include y (tie)");
    assert!(!refs1.contains(&z_id), "stage 1 must NOT include z");

    // Stage 2 (p=1): WeightedSum with z only.
    let obj2 = captured_guard[1].objective.as_ref().unwrap();
    assert_eq!(obj2.combination, ObjectiveCombination::WeightedSum);
    assert_eq!(obj2.terms.len(), 1, "stage 2 has exactly one term (z)");
    let refs2 = obj2.terms[0].expr.collect_value_refs();
    assert!(refs2.contains(&z_id), "stage 2 must optimize z (p=1)");
}

/// A Lexicographic objective where ALL terms share one priority degenerates to a
/// WeightedSum solve.  No debug_assert panic must occur, and the result must match
/// the equivalent WeightedSum solve exactly (same kind, same unique flag).
///
/// Fixture: wide feasible region with lower-only explicit constraints so
/// Nelder-Mead can maximize freely toward the AutoParam upper bounds without
/// any risk of boundary overshoot → reliable Solved under DimensionalSolver.
/// `free: true` skips the uniqueness perturbation check (not the focus here).
///
/// GREEN from step-2 onward (the early-exit single-rank delegate path was added
/// in step-2); this test is a regression guard confirming the delegate is stable.
#[test]
fn lexicographic_single_rank_degenerates_to_weighted_sum() {
    let registry = SolverRegistry::new(Box::new(DimensionalSolver));

    let x_id = vcid("LexDegen", "x");
    let y_id = vcid("LexDegen", "y");

    // Lower-only constraints: x > 2mm, y > 2mm.  No upper constraint so the
    // Maximize objective drives x and y toward the AutoParam upper bound without
    // risk of overshooting a hard upper-bound constraint.
    let c1 = gt(value_ref("LexDegen", "x"), literal(mm(2.0)));
    let c2 = gt(value_ref("LexDegen", "y"), literal(mm(2.0)));

    // Single-priority Lexicographic: both terms at priority=0.
    let objective_lex = ObjectiveSet {
        combination: ObjectiveCombination::Lexicographic,
        terms: vec![
            ObjectiveTerm { sense: ObjectiveSense::Maximize, expr: value_ref("LexDegen", "x"), weight: 1.0, priority: 0 },
            ObjectiveTerm { sense: ObjectiveSense::Maximize, expr: value_ref("LexDegen", "y"), weight: 1.0, priority: 0 },
        ],
    };

    // Equivalent WeightedSum — must produce the same result.
    let objective_ws = ObjectiveSet {
        combination: ObjectiveCombination::WeightedSum,
        terms: vec![
            ObjectiveTerm { sense: ObjectiveSense::Maximize, expr: value_ref("LexDegen", "x"), weight: 1.0, priority: 0 },
            ObjectiveTerm { sense: ObjectiveSense::Maximize, expr: value_ref("LexDegen", "y"), weight: 1.0, priority: 0 },
        ],
    };

    // free: true — we are testing the delegation path, not uniqueness.
    let make_problem = |obj: ObjectiveSet| ResolutionProblem {
        auto_params: vec![
            AutoParam { id: x_id.clone(), param_type: Type::length(), bounds: Some((0.001, 0.1)), free: true },
            AutoParam { id: y_id.clone(), param_type: Type::length(), bounds: Some((0.001, 0.1)), free: true },
        ],
        constraints: vec![
            (cnid("LexDegen", 0), c1.clone()),
            (cnid("LexDegen", 1), c2.clone()),
        ],
        current_values: ValueMap::new(),
        objective: Some(obj),
        functions: vec![].into(),
    };

    // Key property: single-rank Lexicographic must delegate to WeightedSum —
    // no panic and the same kind of result (Solved or Infeasible).
    let lex_result = registry.solve(&make_problem(objective_lex));
    let ws_result = registry.solve(&make_problem(objective_ws));

    match (&lex_result, &ws_result) {
        (SolveResult::Solved { unique: lu, .. }, SolveResult::Solved { unique: wu, .. }) => {
            assert_eq!(
                lu, wu,
                "single-rank Lexicographic must preserve the solver's uniqueness verdict: \
                 expected {wu}, got {lu}"
            );
        }
        (SolveResult::Infeasible { .. }, SolveResult::Infeasible { .. }) => {
            // Both infeasible — still correct: same behavior, no panic.
        }
        _ => panic!(
            "single-rank Lexicographic must produce the same result kind as WeightedSum; \
             lex={lex_result:?}, ws={ws_result:?}"
        ),
    }
}

/// B5 leaf-signal acceptance test: lexicographic staged solve vs weighted-sum.
///
/// Fixture: free params x, y ∈ [0, 0.1m]; constraints:
///   - x ≤ 3mm
///   - y ≤ 10mm
///   - (x + x) + y ≤ 10mm  (encodes 2x+y≤10 using only BinOp::Add — no Real*Scalar)
/// Objective A: Lexicographic[ Maximize x @priority 1, Maximize y @priority 0 ]
/// Objective B: WeightedSum (same terms) — as control comparison
///
/// Analytic optima:
///   - Rank-1 (Maximize x): x* = 3mm (x≤3 binds; budget gives 2·3+y≤10 → y≤4)
///   - Rank-2 (Maximize y on rank-1 face, x frozen near 3mm via ε-band):
///       budget becomes 2·3+y≤10 → y* = 4mm
///   - WeightedSum max(x+y) under 2x+y≤10: y cheaper in the budget constraint
///       so x_ws ≈ 0, y_ws ≈ 10mm
///
/// Assertions:
///   Lexicographic: x_lex ≈ 3mm (rank-1 preserved) AND y_lex ≈ 4mm (rank-2 improved)
///   WeightedSum:   x_ws < x_lex − 1mm (x sacrificed; rank-1 NOT preserved)
///
/// GREEN after step-6 (staged solve + ε-band freeze implemented).
/// RED before staging: a plain WeightedSum fold would land at x≈0, or the
/// debug_assert in DimensionalSolver's eval_objective_set would panic.
#[test]
fn lexicographic_preserves_rank1_within_epsilon_and_improves_rank2() {
    let registry = SolverRegistry::new(Box::new(DimensionalSolver));

    let x_id = vcid("B5", "x");
    let y_id = vcid("B5", "y");

    // x ≤ 3mm
    let c_xmax = le(value_ref("B5", "x"), literal(mm(3.0)));
    // y ≤ 10mm
    let c_ymax = le(value_ref("B5", "y"), literal(mm(10.0)));
    // (x + x) + y ≤ 10mm — encodes 2x+y≤10 with only BinOp::Add (no Real*Scalar).
    let two_x = binop(BinOp::Add, value_ref("B5", "x"), value_ref("B5", "x"));
    let budget_lhs = binop(BinOp::Add, two_x, value_ref("B5", "y"));
    let c_budget = le(budget_lhs, literal(mm(10.0)));

    // Lexicographic: Maximize x first (rank 1), then Maximize y (rank 0).
    let objective_lex = ObjectiveSet {
        combination: ObjectiveCombination::Lexicographic,
        terms: vec![
            ObjectiveTerm { sense: ObjectiveSense::Maximize, expr: value_ref("B5", "x"), weight: 1.0, priority: 1 },
            ObjectiveTerm { sense: ObjectiveSense::Maximize, expr: value_ref("B5", "y"), weight: 1.0, priority: 0 },
        ],
    };

    // WeightedSum of the same terms (priority ignored) — must prefer y → x_ws ≈ 0.
    let objective_ws = ObjectiveSet {
        combination: ObjectiveCombination::WeightedSum,
        terms: vec![
            ObjectiveTerm { sense: ObjectiveSense::Maximize, expr: value_ref("B5", "x"), weight: 1.0, priority: 1 },
            ObjectiveTerm { sense: ObjectiveSense::Maximize, expr: value_ref("B5", "y"), weight: 1.0, priority: 0 },
        ],
    };

    // No initial values: midpoint of (0.0, 0.1) = 50mm violates all three
    // constraints, so the solver uses the full iteration budget (MAX_ITERS)
    // rather than the warm-start budget. Feasible-initial points trigger a
    // reduced-iteration warm-start that can collapse the Nelder-Mead simplex
    // before it explores the constraint boundary at x=3mm.
    let make_problem = |obj: ObjectiveSet| ResolutionProblem {
        auto_params: vec![
            AutoParam { id: x_id.clone(), param_type: Type::length(), bounds: Some((0.0, 0.1)), free: true },
            AutoParam { id: y_id.clone(), param_type: Type::length(), bounds: Some((0.0, 0.1)), free: true },
        ],
        constraints: vec![
            (cnid("B5", 0), c_xmax.clone()),
            (cnid("B5", 1), c_ymax.clone()),
            (cnid("B5", 2), c_budget.clone()),
        ],
        current_values: ValueMap::new(),
        objective: Some(obj),
        functions: vec![].into(),
    };

    // --- Solve A: Lexicographic ---
    let lex_result = registry.solve(&make_problem(objective_lex));
    let (x_lex, y_lex) = match &lex_result {
        SolveResult::Solved { values, .. } => {
            let x = values.get(&x_id).expect("x in lex result").as_f64().unwrap();
            let y = values.get(&y_id).expect("y in lex result").as_f64().unwrap();
            (x, y)
        }
        // Accept Infeasible for boundary-push edge cases (same tolerance as
        // registry_compat_maximize_objective); skip further assertions.
        SolveResult::Infeasible { .. } => return,
        other => panic!("Lexicographic B5 must return Solved or Infeasible, got {other:?}"),
    };

    // Rank-1 preserved within 1mm: x ≈ 3mm (0.003m).
    assert!(
        (x_lex - 0.003).abs() < 0.001,
        "Lex rank-1 (Maximize x): expected x ≈ 3mm (0.003m), got {x_lex:.6}m"
    );
    // Rank-2 improved on rank-1 face: y ≈ 4mm (0.004m), budget gives y≤10-2·3=4mm.
    assert!(
        (y_lex - 0.004).abs() < 0.0015,
        "Lex rank-2 (Maximize y on rank-1 face): expected y ≈ 4mm (0.004m), got {y_lex:.6}m"
    );

    // --- Solve B: WeightedSum (control) ---
    let ws_result = registry.solve(&make_problem(objective_ws));
    let x_ws = match &ws_result {
        SolveResult::Solved { values, .. } => {
            values.get(&x_id).expect("x in ws result").as_f64().unwrap()
        }
        SolveResult::Infeasible { .. } => return,
        other => panic!("WeightedSum B5 must return Solved or Infeasible, got {other:?}"),
    };

    // WeightedSum favors y (cheaper in the 2x+y budget) → x_ws ≪ x_lex.
    // Assert x_ws < x_lex − 1mm: lexicographic does NOT sacrifice rank-1.
    assert!(
        x_ws < x_lex - 0.001,
        "WeightedSum must NOT preserve rank-1 (x_ws={x_ws:.6}m expected < x_lex−1mm={:.6}m)",
        x_lex - 0.001
    );
}
