//! Integration tests for DimensionalSolver.
//!
//! Tests the solver through the ConstraintSolver trait object interface,
//! using reify-test-support helpers for expression construction.

use reify_constraints::DimensionalSolver;
use reify_test_support::*;
use reify_types::{
    AutoParam, ConstraintSolver, OptimizationObjective, ResolutionProblem, SolveResult, Type,
    ValueMap,
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
        other => panic!("expected Solved, got {:?}", other),
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
