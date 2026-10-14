//! DimensionalSolver accepts an equality row under the same toleranced policy
//! the `==` verdict uses (task #6653), so a point the solver returns is one
//! the verdict checker calls Satisfied — while a genuine conflict stays
//! Infeasible, the early exit stays tight, and no dimension's acceptance gets
//! stricter than the pre-#6653 gate.

use std::borrow::Cow;

use reify_constraints::{DimensionalSolver, SimpleConstraintChecker};
use reify_core::{ConstraintNodeId, DiagnosticCode, DimensionVector, Type};
use reify_ir::{
    AutoParam, BinOp, CompiledExpr, ConstraintChecker, ConstraintInput, ConstraintSolver,
    ResolutionProblem, Satisfaction, SolveResult, Value, ValueMap,
};
use reify_test_support::{binop, cnid, eq, ge, literal, meters, mm, value_ref_typed, vcid};

const ENTITY: &str = "Part";
const THIRTY_DEG_RAD: f64 = 0.5235987755982988;
const TEN_DEG_RAD: f64 = 0.17453292519943295;

fn angle(radians: f64) -> Value {
    Value::Scalar {
        si_value: radians,
        dimension: DimensionVector::ANGLE,
    }
}

fn cell(member: &str, ty: &Type) -> CompiledExpr {
    value_ref_typed(ENTITY, member, ty.clone())
}

fn auto(member: &str, ty: &Type) -> AutoParam {
    AutoParam {
        id: vcid(ENTITY, member),
        param_type: ty.clone(),
        bounds: None,
        free: false,
    }
}

fn numbered(constraints: Vec<CompiledExpr>) -> Vec<(ConstraintNodeId, CompiledExpr)> {
    constraints
        .into_iter()
        .zip(0..)
        .map(|(expr, index)| (cnid(ENTITY, index), expr))
        .collect()
}

fn problem(
    auto_params: Vec<AutoParam>,
    constraints: Vec<CompiledExpr>,
    current_values: ValueMap,
) -> ResolutionProblem {
    ResolutionProblem {
        dependent_cells: Vec::new(),
        auto_params,
        constraints: numbered(constraints),
        current_values,
        objective: None,
        functions: vec![].into(),
    }
}

fn solved_si(result: &SolveResult, member: &str) -> f64 {
    match result {
        SolveResult::Solved { values, .. } => match values.get(&vcid(ENTITY, member)) {
            Some(Value::Scalar { si_value, .. }) => *si_value,
            other => panic!("{member} must be solved to a Scalar; got {other:?}"),
        },
        other => panic!("expected Solved; got {other:?}"),
    }
}

fn assert_solved_near(result: &SolveResult, member: &str, expected: f64, tol: f64) {
    let got = solved_si(result, member);
    assert!(
        (got - expected).abs() <= tol,
        "{member} = {got:e}, expected {expected:e} within {tol:e}"
    );
}

/// `w >= 8mm; w == 10mm` — the ineq_eq probe.
fn length_inequality_plus_equality() -> ResolutionProblem {
    let length = Type::length();
    problem(
        vec![auto("w", &length)],
        vec![
            ge(cell("w", &length), literal(mm(8.0))),
            eq(cell("w", &length), literal(mm(10.0))),
        ],
        ValueMap::new(),
    )
}

/// `a + b == 30mm; a - b == 7mm` — the multi_auto2 probe.
fn two_coupled_autos() -> ResolutionProblem {
    let length = Type::length();
    let a = || cell("a", &length);
    let b = || cell("b", &length);
    problem(
        vec![auto("a", &length), auto("b", &length)],
        vec![
            eq(binop(BinOp::Add, a(), b()), literal(mm(30.0))),
            eq(binop(BinOp::Sub, a(), b()), literal(mm(7.0))),
        ],
        ValueMap::new(),
    )
}

fn single_length_auto_pinned_twice(first_mm: f64, second_mm: f64) -> ResolutionProblem {
    let length = Type::length();
    problem(
        vec![auto("w", &length)],
        vec![
            eq(cell("w", &length), literal(mm(first_mm))),
            eq(cell("w", &length), literal(mm(second_mm))),
        ],
        ValueMap::new(),
    )
}

fn assert_checker_satisfies_the_solved_point(problem: &ResolutionProblem, label: &str) {
    let result = DimensionalSolver.solve(problem);
    let SolveResult::Solved { values: solved, .. } = &result else {
        panic!("{label}: expected Solved; got {result:?}");
    };
    let mut values = problem.current_values.clone();
    for (id, value) in solved {
        values.insert(id.clone(), value.clone());
    }
    let constraints: Vec<(ConstraintNodeId, &CompiledExpr)> = problem
        .constraints
        .iter()
        .map(|(id, expr)| (id.clone(), expr))
        .collect();
    let input = ConstraintInput {
        constraints: Cow::Owned(constraints),
        values: &values,
        functions: &[],
        determinacy: None,
    };
    for verdict in SimpleConstraintChecker.check(&input) {
        assert_eq!(
            verdict.satisfaction,
            Satisfaction::Satisfied,
            "{label}: the verdict checker must accept the solver's point at {:?}; solved {solved:?}",
            verdict.id
        );
    }
}

#[test]
fn feasible_inequality_plus_equality_is_solved() {
    let result = DimensionalSolver.solve(&length_inequality_plus_equality());
    assert_solved_near(&result, "w", 0.01, 1e-6);
}

#[test]
fn angle_inequality_plus_equality_is_solved() {
    let angle_type = Type::angle();
    let result = DimensionalSolver.solve(&problem(
        vec![auto("t", &angle_type)],
        vec![
            ge(cell("t", &angle_type), literal(angle(TEN_DEG_RAD))),
            eq(cell("t", &angle_type), literal(angle(THIRTY_DEG_RAD))),
        ],
        ValueMap::new(),
    ));
    assert_solved_near(&result, "t", THIRTY_DEG_RAD, 1e-6);
}

#[test]
fn solved_point_is_satisfied_by_the_verdict_checker() {
    assert_checker_satisfies_the_solved_point(
        &length_inequality_plus_equality(),
        "w >= 8mm; w == 10mm",
    );
    assert_checker_satisfies_the_solved_point(&two_coupled_autos(), "a + b == 30mm; a - b == 7mm");
}

#[test]
fn contradictory_equality_pair_stays_infeasible_with_raw_residual() {
    let result = DimensionalSolver.solve(&single_length_auto_pinned_twice(10.0, 20.0));
    let SolveResult::Infeasible { diagnostics } = &result else {
        panic!("w == 10mm; w == 20mm must be Infeasible; got {result:?}");
    };
    let unsatisfiable: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ConstraintUnsatisfiable))
        .collect();
    assert!(
        unsatisfiable
            .iter()
            .any(|d| d.message.contains("max absolute residual: 5.00e-3")),
        "the raw residual must still be reported; got {diagnostics:?}"
    );
}

#[test]
fn one_percent_equality_conflict_stays_infeasible() {
    let result = DimensionalSolver.solve(&single_length_auto_pinned_twice(10.0, 10.1));
    assert!(
        !matches!(result, SolveResult::Solved { .. }),
        "w == 10mm; w == 10.1mm must not be Solved; got {result:?}"
    );
}

#[test]
fn seed_within_tolerance_still_converges_tightly() {
    let length = Type::length();
    let mut seed = ValueMap::new();
    seed.insert(vcid(ENTITY, "w"), meters(0.0100004));
    let result = DimensionalSolver.solve(&problem(
        vec![auto("w", &length)],
        vec![eq(cell("w", &length), literal(mm(10.0)))],
        seed,
    ));
    assert_solved_near(&result, "w", 0.01, 1e-11);
}

#[test]
fn mm3_volume_equality_is_never_stricter_than_before() {
    let volume = Type::Scalar {
        dimension: DimensionVector::VOLUME,
    };
    let one_thousand_mm3 = Value::Scalar {
        si_value: 1e-6,
        dimension: DimensionVector::VOLUME,
    };
    let result = DimensionalSolver.solve(&problem(
        vec![auto("v", &volume)],
        vec![eq(cell("v", &volume), literal(one_thousand_mm3))],
        ValueMap::new(),
    ));
    assert_solved_near(&result, "v", 1e-6, 1e-12);
}
