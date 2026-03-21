//! Tests for the CpSatSolver — discrete/logical constraint solver.

use reify_constraints::CpSatSolver;
use reify_test_support::builders::*;
use reify_test_support::values::*;
use reify_types::{
    AutoParam, ConstraintSolver, ResolutionProblem, SolveResult, Type, Value, ValueMap,
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
