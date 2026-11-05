//! Runtime arm of the one-sided strict-auto refusal (task #6692, P1-ε).
//!
//! PRD `docs/prds/v0_6/solver-driver-parity.md` §4.6 case 2 / §5 B4–B5: a
//! strict `auto` that the model bounds on ONE side only has no designed value.
//! With no explicit objective the solver synthesises a Chebyshev-centre
//! objective that is monotone in such a param, so its argmax is the edge of
//! the solver's internal default domain (`10 m` / `1 µm` for a Length). That
//! edge is an artifact, not a resolution: the solve must be refused with the
//! coded `ConstraintNonUnique` diagnostic naming the missing bound.
//!
//! Driven through the public seam only — `SolverRegistry::production()` (PRD
//! I2) and `DimensionalSolver` — on hand-built `ResolutionProblem`s in the
//! production shape (`AutoParam::bounds: None`, `objective: None`).

use reify_constraints::{DimensionalSolver, SolverRegistry};
use reify_core::{ConstraintNodeId, DiagnosticCode, DimensionVector, Severity, Type, ValueCellId};
use reify_ir::{
    AutoParam, BinOp, CompiledExpr, ConstraintSolver, ObjectiveSense, ObjectiveSet,
    RankedSolveResult, ResolutionProblem, SolveResult, Value, ValueMap,
};

const MM: f64 = 1e-3;
const NOT_DETERMINED: &str = "not uniquely determined";
const NO_BOUND_CLAUSE: &str = "no constraint bounds";

fn length_type() -> Type {
    Type::Scalar {
        dimension: DimensionVector::LENGTH,
    }
}

fn length_ref(id: &ValueCellId) -> CompiledExpr {
    CompiledExpr::value_ref(id.clone(), length_type())
}

fn length_mm(mm: f64) -> CompiledExpr {
    CompiledExpr::literal(
        Value::Scalar {
            si_value: mm * MM,
            dimension: DimensionVector::LENGTH,
        },
        length_type(),
    )
}

fn compare(op: BinOp, left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(op, left, right, Type::Bool)
}

fn strict_length_auto(id: &ValueCellId) -> AutoParam {
    AutoParam {
        id: id.clone(),
        param_type: length_type(),
        bounds: None,
        free: false,
    }
}

fn part_x() -> ValueCellId {
    ValueCellId::new("Part", "x")
}

/// `constraints` numbered in order under entity `Part`, no objective, no
/// dependent cells: the shape `build_solver_problem` emits for a template
/// whose only autos are `params`.
fn problem(params: Vec<AutoParam>, constraints: Vec<CompiledExpr>) -> ResolutionProblem {
    ResolutionProblem {
        auto_params: params,
        constraints: constraints
            .into_iter()
            .enumerate()
            .map(|(i, expr)| (ConstraintNodeId::new("Part", i as u32), expr))
            .collect(),
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
        dependent_cells: Vec::new(),
    }
}

fn x_ge_8mm() -> ResolutionProblem {
    let x = part_x();
    problem(
        vec![strict_length_auto(&x)],
        vec![compare(BinOp::Ge, length_ref(&x), length_mm(8.0))],
    )
}

fn result_mentions(result: &SolveResult, needle: &str) -> bool {
    match result {
        SolveResult::Infeasible { diagnostics } => {
            diagnostics.iter().any(|d| d.message.contains(needle))
        }
        SolveResult::NoProgress { reason } => reason.contains(needle),
        SolveResult::Solved { .. } => false,
    }
}

fn solved_x(case: &str, result: &SolveResult) -> f64 {
    match result {
        SolveResult::Solved { values, .. } => values
            .get(&part_x())
            .and_then(Value::as_f64)
            .unwrap_or_else(|| panic!("{case}: Solved without a numeric Part.x: {result:?}")),
        other => panic!("{case}: expected Solved, got {other:?}"),
    }
}

/// Asserts `result` is the one-sided refusal: `Infeasible` carrying exactly
/// one `ConstraintNonUnique` Error, whose message holds the diagnosis phrase
/// and `clause`.
fn assert_refused(case: &str, result: &SolveResult, clause: &str) {
    let SolveResult::Infeasible { diagnostics } = result else {
        panic!("{case}: expected Infeasible (one-sided strict auto refused), got {result:?}");
    };
    let non_unique: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ConstraintNonUnique))
        .collect();
    assert_eq!(
        non_unique.len(),
        1,
        "{case}: expected exactly one ConstraintNonUnique diagnostic, got {result:?}"
    );
    let diagnostic = non_unique[0];
    assert_eq!(
        diagnostic.severity,
        Severity::Error,
        "{case}: the refusal must be an Error, got {result:?}"
    );
    assert!(
        diagnostic.message.contains(NOT_DETERMINED),
        "{case}: message must carry the shared diagnosis phrase {NOT_DETERMINED:?}, got {result:?}"
    );
    assert!(
        diagnostic.message.contains(clause),
        "{case}: message must name the missing bound with {clause:?}, got {result:?}"
    );
}

/// R1 — a strict auto bounded ONLY below is refused, naming the missing upper
/// bound and the default the solve would have returned.
#[test]
fn r1_lower_only_strict_auto_is_refused() {
    let result = SolverRegistry::production().solve(&x_ge_8mm());
    assert_refused(
        "R1 (x >= 8mm)",
        &result,
        "no constraint bounds `Part.x` above (solver default 10 m)",
    );
}

/// R1 through the single-solver seam: the refusal lives in the dimensional
/// solver's own uniqueness verdict, not in registry routing.
#[test]
fn r1_lower_only_strict_auto_is_refused_by_dimensional_solver() {
    let result = DimensionalSolver.solve(&x_ge_8mm());
    assert_refused(
        "R1/DimensionalSolver (x >= 8mm)",
        &result,
        "no constraint bounds `Part.x` above (solver default 10 m)",
    );
}

/// R1 through `solve_ranked`, the entry the ranked consumers take: no
/// candidate may carry the corner value.
#[test]
fn r1_lower_only_strict_auto_is_refused_by_solve_ranked() {
    let result = match SolverRegistry::production().solve_ranked(&x_ge_8mm()) {
        RankedSolveResult::Infeasible { diagnostics } => SolveResult::Infeasible { diagnostics },
        other => panic!("R1/solve_ranked (x >= 8mm): expected Infeasible, got {other:?}"),
    };
    assert_refused(
        "R1/solve_ranked (x >= 8mm)",
        &result,
        "no constraint bounds `Part.x` above (solver default 10 m)",
    );
}

/// R2 — the mirror: bounded ONLY above, refused naming the missing lower bound.
#[test]
fn r2_upper_only_strict_auto_is_refused() {
    let x = part_x();
    let result = SolverRegistry::production().solve(&problem(
        vec![strict_length_auto(&x)],
        vec![compare(BinOp::Le, length_ref(&x), length_mm(12.0))],
    ));
    assert_refused(
        "R2 (x <= 12mm)",
        &result,
        "no constraint bounds `Part.x` below (solver default",
    );
}

/// R3 — the B5 discriminating negative: two-sided bounds resolve to the
/// Chebyshev centre with no diagnostic.
#[test]
fn r3_two_sided_strict_auto_resolves_to_the_centre() {
    let x = part_x();
    let result = SolverRegistry::production().solve(&problem(
        vec![strict_length_auto(&x)],
        vec![
            compare(BinOp::Ge, length_ref(&x), length_mm(8.0)),
            compare(BinOp::Le, length_ref(&x), length_mm(12.0)),
        ],
    ));
    let value = solved_x("R3 (8mm <= x <= 12mm)", &result);
    assert!(
        (value - 0.010).abs() < 1e-9,
        "R3: two-sided auto must resolve to the centre 0.010 m, got {result:?}"
    );
}

/// R4 — the F2 ruling: a one-sided `auto(free)` is never refused by this arm.
#[test]
fn r4_one_sided_free_auto_is_not_refused() {
    let mut free = x_ge_8mm();
    free.auto_params[0].free = true;
    let result = SolverRegistry::production().solve(&free);
    assert!(
        matches!(result, SolveResult::Solved { unique: false, .. }),
        "R4 (auto(free), x >= 8mm): expected Solved {{ unique: false }}, got {result:?}"
    );
}

/// R5 — the branch is gated on objective ABSENCE: an explicit objective
/// governs the solve, so the corner-pick refusal must not speak for it.
/// Deliberately does not pin `Solved` — that shape's own verdict belongs to
/// the #6901 family.
#[test]
fn r5_explicit_objective_bypasses_the_corner_pick_refusal() {
    let x = part_x();
    let mut with_objective = x_ge_8mm();
    with_objective.objective = Some(ObjectiveSet::single(
        ObjectiveSense::Minimize,
        length_ref(&x),
    ));
    let result = SolverRegistry::production().solve(&with_objective);
    assert!(
        !result_mentions(&result, NO_BOUND_CLAUSE),
        "R5 (minimize x, x >= 8mm): an explicit objective must bypass the one-sided \
         refusal, got {result:?}"
    );
}

/// R6 — the edge-confirmation guard. `x >= 1mm` plus `side = 3*x`,
/// `side <= 9mm` is two-sided THROUGH a derived cell, which the syntactic
/// derivation cannot see, so it reads `x` as bounded below only. Centrality
/// resolves it to ≈2.5 mm, off both default edges; only the solved-on-edge
/// filter keeps this design from a false refusal.
#[test]
fn r6_two_sided_through_derived_cell_is_not_refused() {
    let x = part_x();
    let side = ValueCellId::new("Part", "side");
    let three_x = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(
            Value::Scalar {
                si_value: 3.0,
                dimension: DimensionVector::DIMENSIONLESS,
            },
            Type::Scalar {
                dimension: DimensionVector::DIMENSIONLESS,
            },
        ),
        length_ref(&x),
        length_type(),
    );
    let mut derived = problem(
        vec![strict_length_auto(&x)],
        vec![
            compare(BinOp::Ge, length_ref(&x), length_mm(1.0)),
            compare(BinOp::Le, length_ref(&side), length_mm(9.0)),
        ],
    );
    derived.dependent_cells = vec![(side, three_x)];
    let result = SolverRegistry::production().solve(&derived);
    let value = solved_x("R6 (x >= 1mm, 3*x <= 9mm)", &result);
    assert!(
        value > 1.0 * MM && value < 3.0 * MM,
        "R6: expected x strictly inside (1mm, 3mm), off both default edges, got {result:?}"
    );
}

/// R7 — abstention preserved: an `Eq` conjunct the interval derivation cannot
/// read puts both autos in the underivable set, so the refusal abstains
/// exactly as the γ branch does. This is the task's stated "no equality"
/// boundary, recorded as measured behaviour.
#[test]
fn r7_equality_coupled_autos_abstain() {
    let x = part_x();
    let y = ValueCellId::new("Part", "y");
    let x_plus_2mm = CompiledExpr::binop(BinOp::Add, length_ref(&x), length_mm(2.0), length_type());
    let result = SolverRegistry::production().solve(&problem(
        vec![strict_length_auto(&x), strict_length_auto(&y)],
        vec![
            compare(BinOp::Ge, length_ref(&x), length_mm(8.0)),
            compare(BinOp::Eq, length_ref(&y), x_plus_2mm),
        ],
    ));
    assert!(
        !result_mentions(&result, NO_BOUND_CLAUSE),
        "R7 (x >= 8mm, y == x + 2mm): the underivable Eq conjunct must make the \
         one-sided refusal abstain, got {result:?}"
    );
}
