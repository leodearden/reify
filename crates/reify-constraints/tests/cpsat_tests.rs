//! Tests for the CpSatSolver — discrete/logical constraint solver.

use reify_constraints::CpSatSolver;
use reify_test_support::builders::*;
use reify_test_support::values::*;
use reify_types::{
    AutoParam, ConstraintSolver, Diagnostic, ResolutionProblem, SolveResult, Type, Value, ValueMap,
};

// ---------------------------------------------------------------------------
// step-1: boolean SAT with 3 Bool auto params
// ---------------------------------------------------------------------------

/// `a && (b || !c)` — should find a satisfying assignment.
#[test]
fn boolean_sat_3_params() {
    let solver = CpSatSolver;

    let a_id = vcid("Part", "a");
    let b_id = vcid("Part", "b");
    let c_id = vcid("Part", "c");

    let a_ref = value_ref_typed("Part", "a", Type::Bool);
    let b_ref = value_ref_typed("Part", "b", Type::Bool);
    let c_ref = value_ref_typed("Part", "c", Type::Bool);

    // constraint: a && (b || !c)
    let constraint_expr = and(a_ref, or(b_ref, not(c_ref)));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: a_id.clone(),
                param_type: Type::Bool,
                bounds: None,
            },
            AutoParam {
                id: b_id.clone(),
                param_type: Type::Bool,
                bounds: None,
            },
            AutoParam {
                id: c_id.clone(),
                param_type: Type::Bool,
                bounds: None,
            },
        ],
        constraints: vec![(cnid("Part", 0), constraint_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            // a must be true
            assert_eq!(values.get(&a_id), Some(&Value::Bool(true)));
            // b || !c must hold
            let b = values.get(&b_id).unwrap() == &Value::Bool(true);
            let c = values.get(&c_id).unwrap() == &Value::Bool(true);
            assert!(b || !c, "expected b || !c, got b={b}, c={c}");
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// step-3: infeasible boolean problem `a && !a`
// ---------------------------------------------------------------------------

/// `a && !a` — contradictory constraint, must return Infeasible.
#[test]
fn boolean_infeasible_contradiction() {
    let solver = CpSatSolver;

    let a_id = vcid("Part", "a");
    let a_ref = value_ref_typed("Part", "a", Type::Bool);

    // constraint: a && !a
    let constraint_expr = and(a_ref.clone(), not(a_ref));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: a_id.clone(),
            param_type: Type::Bool,
            bounds: None,
        }],
        constraints: vec![(cnid("Part", 0), constraint_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Infeasible { diagnostics } => {
            assert!(!diagnostics.is_empty(), "expected non-empty diagnostics");
        }
        other => panic!("expected Infeasible, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// step-5: implication `if a then b` encoded as Or(!a, b)
// ---------------------------------------------------------------------------

/// `!a || b` (implication: a → b) — solution must satisfy: if a then b.
#[test]
fn implication_if_a_then_b() {
    let solver = CpSatSolver;

    let a_id = vcid("Part", "a");
    let b_id = vcid("Part", "b");

    let a_ref = value_ref_typed("Part", "a", Type::Bool);
    let b_ref = value_ref_typed("Part", "b", Type::Bool);

    // constraint: !a || b  (a implies b)
    let constraint_expr = or(not(a_ref), b_ref);

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: a_id.clone(),
                param_type: Type::Bool,
                bounds: None,
            },
            AutoParam {
                id: b_id.clone(),
                param_type: Type::Bool,
                bounds: None,
            },
        ],
        constraints: vec![(cnid("Part", 0), constraint_expr)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let a = values.get(&a_id).unwrap() == &Value::Bool(true);
            let b = values.get(&b_id).unwrap() == &Value::Bool(true);
            assert!(!a || b, "implication violated: a={a}, b={b}");
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}

/// `a && (!a || b)` forces a=true, which means b must be true.
#[test]
fn implication_forced_a_true_implies_b_true() {
    let solver = CpSatSolver;

    let a_id = vcid("Part", "a");
    let b_id = vcid("Part", "b");

    let a_ref = value_ref_typed("Part", "a", Type::Bool);
    let b_ref = value_ref_typed("Part", "b", Type::Bool);

    // Two constraints: a, and !a || b
    let c1 = a_ref.clone();
    let c2 = or(not(a_ref), b_ref);

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: a_id.clone(),
                param_type: Type::Bool,
                bounds: None,
            },
            AutoParam {
                id: b_id.clone(),
                param_type: Type::Bool,
                bounds: None,
            },
        ],
        constraints: vec![(cnid("Part", 0), c1), (cnid("Part", 1), c2)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            assert_eq!(values.get(&a_id), Some(&Value::Bool(true)));
            assert_eq!(values.get(&b_id), Some(&Value::Bool(true)));
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// step-7: cardinality — at most 2 of [a, b, c, d] are true
// ---------------------------------------------------------------------------

/// At most 2 of 4 booleans are true — encoded by forbidding all 3-element subsets.
#[test]
fn cardinality_at_most_2_of_4() {
    let solver = CpSatSolver;

    let a_id = vcid("Part", "a");
    let b_id = vcid("Part", "b");
    let c_id = vcid("Part", "c");
    let d_id = vcid("Part", "d");

    let a_ref = value_ref_typed("Part", "a", Type::Bool);
    let b_ref = value_ref_typed("Part", "b", Type::Bool);
    let c_ref = value_ref_typed("Part", "c", Type::Bool);
    let d_ref = value_ref_typed("Part", "d", Type::Bool);

    // Forbid each 3-subset: !(a && b && c), !(a && b && d), !(a && c && d), !(b && c && d)
    let c1 = not(and(a_ref.clone(), and(b_ref.clone(), c_ref.clone())));
    let c2 = not(and(a_ref.clone(), and(b_ref.clone(), d_ref.clone())));
    let c3 = not(and(a_ref.clone(), and(c_ref.clone(), d_ref.clone())));
    let c4 = not(and(b_ref.clone(), and(c_ref.clone(), d_ref.clone())));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam { id: a_id.clone(), param_type: Type::Bool, bounds: None },
            AutoParam { id: b_id.clone(), param_type: Type::Bool, bounds: None },
            AutoParam { id: c_id.clone(), param_type: Type::Bool, bounds: None },
            AutoParam { id: d_id.clone(), param_type: Type::Bool, bounds: None },
        ],
        constraints: vec![
            (cnid("Part", 0), c1),
            (cnid("Part", 1), c2),
            (cnid("Part", 2), c3),
            (cnid("Part", 3), c4),
        ],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let count_true = [&a_id, &b_id, &c_id, &d_id]
                .iter()
                .filter(|id| values.get(id) == Some(&Value::Bool(true)))
                .count();
            assert!(
                count_true <= 2,
                "expected at most 2 true, got {count_true}"
            );
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// step-9: enum constraint
// ---------------------------------------------------------------------------

/// Enum param x (Material): x != A and (x == B or x == C). Expect x = B or C.
#[test]
fn enum_constraint_excludes_one_variant() {
    use reify_types::CompiledExpr;

    let solver = CpSatSolver;

    let x_id = vcid("Part", "x");
    let x_ref = value_ref_typed("Part", "x", Type::Enum("Material".into()));

    // Enum literals
    let enum_a = CompiledExpr::literal(
        Value::Enum { type_name: "Material".into(), variant: "A".into() },
        Type::Enum("Material".into()),
    );
    let enum_b = CompiledExpr::literal(
        Value::Enum { type_name: "Material".into(), variant: "B".into() },
        Type::Enum("Material".into()),
    );
    let enum_c = CompiledExpr::literal(
        Value::Enum { type_name: "Material".into(), variant: "C".into() },
        Type::Enum("Material".into()),
    );

    // Constraints: x != A, and (x == B or x == C)
    let c1 = ne(x_ref.clone(), enum_a);
    let c2 = or(eq(x_ref.clone(), enum_b), eq(x_ref.clone(), enum_c));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::Enum("Material".into()),
            bounds: None,
        }],
        constraints: vec![(cnid("Part", 0), c1), (cnid("Part", 1), c2)],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let x_val = values.get(&x_id).unwrap();
            match x_val {
                Value::Enum { variant, .. } => {
                    assert!(
                        variant == "B" || variant == "C",
                        "expected B or C, got {variant}"
                    );
                }
                other => panic!("expected Enum value, got {:?}", other),
            }
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// step-11: integer constraint x + y == 10
// ---------------------------------------------------------------------------

/// x + y == 10 with both in [0, 10]. Expect solved with x+y==10.
#[test]
fn integer_constraint_sum_equals_10() {
    use reify_types::{BinOp, CompiledExpr};

    let solver = CpSatSolver;

    let x_id = vcid("Part", "x");
    let y_id = vcid("Part", "y");

    let x_ref = value_ref_typed("Part", "x", Type::Int);
    let y_ref = value_ref_typed("Part", "y", Type::Int);

    // x + y
    let sum = CompiledExpr::binop(BinOp::Add, x_ref.clone(), y_ref.clone(), Type::Int);
    let ten = literal(Value::Int(10));

    // Constraints: x + y == 10
    let c1 = eq(sum, ten);
    // x >= 0, y >= 0, x <= 10, y <= 10
    let c2 = ge(x_ref.clone(), literal(Value::Int(0)));
    let c3 = ge(y_ref.clone(), literal(Value::Int(0)));
    let c4 = le(x_ref, literal(Value::Int(10)));
    let c5 = le(y_ref, literal(Value::Int(10)));

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: x_id.clone(),
                param_type: Type::Int,
                bounds: Some((0.0, 10.0)),
            },
            AutoParam {
                id: y_id.clone(),
                param_type: Type::Int,
                bounds: Some((0.0, 10.0)),
            },
        ],
        constraints: vec![
            (cnid("Part", 0), c1),
            (cnid("Part", 1), c2),
            (cnid("Part", 2), c3),
            (cnid("Part", 3), c4),
            (cnid("Part", 4), c5),
        ],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let x = match values.get(&x_id).unwrap() {
                Value::Int(v) => *v,
                other => panic!("expected Int for x, got {:?}", other),
            };
            let y = match values.get(&y_id).unwrap() {
                Value::Int(v) => *v,
                other => panic!("expected Int for y, got {:?}", other),
            };
            assert_eq!(x + y, 10, "expected x + y == 10, got x={x}, y={y}");
            assert!(x >= 0 && x <= 10, "x out of bounds: {x}");
            assert!(y >= 0 && y <= 10, "y out of bounds: {y}");
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// step-13: all-different on 3 Int auto params
// ---------------------------------------------------------------------------

/// x, y, z in [1,3], all different → must be a permutation of {1,2,3}.
#[test]
fn all_different_3_ints() {
    let solver = CpSatSolver;

    let x_id = vcid("Part", "x");
    let y_id = vcid("Part", "y");
    let z_id = vcid("Part", "z");

    let x_ref = value_ref_typed("Part", "x", Type::Int);
    let y_ref = value_ref_typed("Part", "y", Type::Int);
    let z_ref = value_ref_typed("Part", "z", Type::Int);

    // Constraints: x != y, x != z, y != z
    let c1 = ne(x_ref.clone(), y_ref.clone());
    let c2 = ne(x_ref, z_ref.clone());
    let c3 = ne(y_ref, z_ref);

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam { id: x_id.clone(), param_type: Type::Int, bounds: Some((1.0, 3.0)) },
            AutoParam { id: y_id.clone(), param_type: Type::Int, bounds: Some((1.0, 3.0)) },
            AutoParam { id: z_id.clone(), param_type: Type::Int, bounds: Some((1.0, 3.0)) },
        ],
        constraints: vec![
            (cnid("Part", 0), c1),
            (cnid("Part", 1), c2),
            (cnid("Part", 2), c3),
        ],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![],
    };

    let result = solver.solve(&problem);
    match result {
        SolveResult::Solved { values } => {
            let mut vals: Vec<i64> = [&x_id, &y_id, &z_id]
                .iter()
                .map(|id| match values.get(id).unwrap() {
                    Value::Int(v) => *v,
                    other => panic!("expected Int, got {:?}", other),
                })
                .collect();
            vals.sort();
            assert_eq!(vals, vec![1, 2, 3], "expected permutation of [1,2,3], got {:?}", vals);
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}
