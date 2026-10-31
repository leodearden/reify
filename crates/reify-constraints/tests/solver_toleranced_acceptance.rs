//! DimensionalSolver accepts an equality row, and a `!=` row as its
//! complement, under the same toleranced policy the `==` verdict uses (task
//! #6653), so a point the solver returns is one the verdict checker calls
//! Satisfied — while a genuine conflict stays Infeasible and reports its worst
//! REJECTED row, the early exit stays tight, an accepted seed still backs the
//! drift fallback, and no dimension's acceptance gets stricter than the
//! pre-#6653 gate.

use std::borrow::Cow;

use reify_constraints::{DimensionalSolver, SimpleConstraintChecker};
use reify_core::{ConstraintNodeId, DiagnosticCode, DimensionVector, Type};
use reify_ir::{
    AutoParam, BinOp, CompiledExpr, ConstraintChecker, ConstraintInput, ConstraintSolver,
    ObjectiveSense, ObjectiveSet, ResolutionProblem, Satisfaction, SolveResult, Value, ValueMap,
};
use reify_test_support::{
    and, binop, cnid, conditional_expr, eq, ge, le, literal, meters, mm, ne, or, value_ref_typed,
    vcid,
};

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

#[test]
fn or_of_equalities_is_accepted_through_the_verdict() {
    let length = Type::length();
    let either_root = problem(
        vec![auto("w", &length)],
        vec![
            ge(cell("w", &length), literal(mm(8.0))),
            or(
                eq(cell("w", &length), literal(mm(10.0))),
                eq(cell("w", &length), literal(mm(30.0))),
            ),
        ],
        ValueMap::new(),
    );
    let w = solved_si(&DimensionalSolver.solve(&either_root), "w");
    assert!(
        (w - 0.01).abs() <= 1e-6 || (w - 0.03).abs() <= 1e-6,
        "w = {w:e}, expected 1e-2 or 3e-2 within 1e-6"
    );
    assert_checker_satisfies_the_solved_point(&either_root, "w >= 8mm; w == 10mm or w == 30mm");
}

#[test]
fn and_composite_row_is_accepted_through_the_verdict() {
    let length = Type::length();
    let one_row = problem(
        vec![auto("w", &length)],
        vec![and(
            ge(cell("w", &length), literal(mm(8.0))),
            eq(cell("w", &length), literal(mm(10.0))),
        )],
        ValueMap::new(),
    );
    assert_solved_near(&DimensionalSolver.solve(&one_row), "w", 0.01, 1e-6);
    assert_checker_satisfies_the_solved_point(&one_row, "w >= 8mm and w == 10mm");
}

#[test]
fn or_of_equalities_missing_by_one_percent_stays_unsolved() {
    let length = Type::length();
    let pinned_w = || eq(cell("w", &length), literal(mm(10.0)));
    let near_misses_of = |subject: &str| {
        or(
            eq(cell(subject, &length), literal(mm(10.1))),
            eq(cell(subject, &length), literal(mm(9.9))),
        )
    };
    let mut fixed_p = ValueMap::new();
    fixed_p.insert(vcid(ENTITY, "p"), mm(10.0));
    let cases = [
        (
            "w == 10mm; w == 10.1mm or w == 9.9mm",
            problem(
                vec![auto("w", &length)],
                vec![pinned_w(), near_misses_of("w")],
                ValueMap::new(),
            ),
        ),
        (
            "w == 10mm; p == 10.1mm or p == 9.9mm with p = 10mm",
            problem(
                vec![auto("w", &length)],
                vec![pinned_w(), near_misses_of("p")],
                fixed_p,
            ),
        ),
    ];
    for (label, unsatisfiable) in &cases {
        let result = DimensionalSolver.solve(unsatisfiable);
        assert!(
            !matches!(result, SolveResult::Solved { .. }),
            "{label} must not be Solved; got {result:?}"
        );
    }
}

fn unsatisfiable_message(result: &SolveResult) -> String {
    let SolveResult::Infeasible { diagnostics } = result else {
        panic!("expected Infeasible; got {result:?}");
    };
    diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ConstraintUnsatisfiable))
        .map(|d| d.message.clone())
        .unwrap_or_else(|| {
            panic!("expected a ConstraintUnsatisfiable diagnostic; got {diagnostics:?}")
        })
}

/// Fixed cells `p == q` agree within the Length floor yet sit 5e-7 m apart,
/// so this row is accepted at every point while its residual stays well above
/// `1e-12`.
fn accepted_near_miss(current_values: &mut ValueMap) -> CompiledExpr {
    let length = Type::length();
    current_values.insert(vcid(ENTITY, "p"), mm(10.0));
    current_values.insert(vcid(ENTITY, "q"), meters(0.0100005));
    eq(cell("p", &length), cell("q", &length))
}

#[test]
fn drift_fallback_returns_a_seed_that_passes_acceptance() {
    let length = Type::length();
    let x = || cell("x", &length);
    let mut current_values = ValueMap::new();
    current_values.insert(vcid(ENTITY, "x"), mm(15.0));
    let near_miss = accepted_near_miss(&mut current_values);
    // Flat and costly wherever `x <= 20mm` holds, cheap past 22mm: the
    // optimizer is lured out of the feasible region.
    let lure = conditional_expr(le(x(), literal(mm(22.0))), literal(Value::Real(1e8)), x());
    let drifting = ResolutionProblem {
        dependent_cells: Vec::new(),
        auto_params: vec![AutoParam {
            id: vcid(ENTITY, "x"),
            param_type: length.clone(),
            bounds: Some((0.001, 0.1)),
            free: true,
        }],
        constraints: numbered(vec![le(x(), literal(mm(20.0))), near_miss]),
        current_values,
        objective: Some(ObjectiveSet::single(ObjectiveSense::Minimize, lure)),
        functions: vec![].into(),
    };
    assert_solved_near(&DimensionalSolver.solve(&drifting), "x", 0.015, 1e-10);
}

#[test]
fn infeasible_report_names_the_worst_rejected_row_not_an_accepted_near_miss() {
    let length = Type::length();
    let mut current_values = ValueMap::new();
    let near_miss = accepted_near_miss(&mut current_values);
    current_values.insert(vcid(ENTITY, "r"), meters(0.010000001));
    let result = DimensionalSolver.solve(&problem(
        vec![auto("w", &length)],
        vec![
            eq(cell("w", &length), literal(mm(10.0))),
            near_miss,
            le(cell("r", &length), literal(mm(10.0))),
        ],
        current_values,
    ));
    let message = unsatisfiable_message(&result);
    assert!(
        message.contains("max absolute residual: 1.00e-9"),
        "the report must quote the rejected `r <= 10mm` row (1e-9), not the accepted 5e-7 near miss; got {message:?}"
    );
}

#[test]
fn ne_within_tolerance_of_the_only_feasible_value_is_not_solved() {
    let length = Type::length();
    let w = || cell("w", &length);
    let result = DimensionalSolver.solve(&problem(
        vec![auto("w", &length)],
        vec![
            ge(w(), literal(mm(10.0))),
            le(w(), literal(mm(10.0))),
            ne(w(), literal(mm(10.0000005))),
        ],
        ValueMap::new(),
    ));
    assert!(
        !matches!(result, SolveResult::Solved { .. }),
        "w pinned to 10mm cannot satisfy w != 10.0000005mm, 5e-10 m away; got {result:?}"
    );
}

#[test]
fn optimizer_is_pushed_out_of_an_ne_band_around_the_centre() {
    let length = Type::length();
    let w = || cell("w", &length);
    // Centrality draws `w` to 10mm, within a micrometre of the `!=` target, so
    // only the toleranced `!=` cost can push it clear of the rejected band.
    let banded = problem(
        vec![AutoParam {
            free: true,
            ..auto("w", &length)
        }],
        vec![
            ge(w(), literal(mm(9.0))),
            le(w(), literal(mm(11.0))),
            ne(w(), literal(mm(10.0000005))),
        ],
        ValueMap::new(),
    );
    assert_checker_satisfies_the_solved_point(&banded, "9mm <= w <= 11mm; w != 10.0000005mm");
}

#[test]
fn ne_outside_tolerance_is_solved_and_satisfied() {
    let length = Type::length();
    let w = || cell("w", &length);
    let clear_of_the_target = problem(
        vec![auto("w", &length)],
        vec![eq(w(), literal(mm(10.0))), ne(w(), literal(mm(10.01)))],
        ValueMap::new(),
    );
    assert_solved_near(
        &DimensionalSolver.solve(&clear_of_the_target),
        "w",
        0.01,
        1e-6,
    );
    assert_checker_satisfies_the_solved_point(&clear_of_the_target, "w == 10mm; w != 10.01mm");
}
