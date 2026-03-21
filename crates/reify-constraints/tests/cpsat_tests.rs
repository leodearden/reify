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
