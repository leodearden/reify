//! Solver-level integration tests for [JOINT-DRIVE β] (task #5189): the
//! per-trial dependent-cell recompute and its consumers.
//!
//! PRD: `docs/prds/v0_6/whole-model-joint-drive-seam.md` §6.2 / §12 Q2.
//!
//! ## Why this file exists (and why here)
//!
//! §12 Q2 asks β to "confirm no start-set caching assumes a frozen objective".
//! It does. `multistart_points` SEEDING is pure and objective-independent, but
//! the post-solve SCORING is not: both the multistart scoring loop and
//! `rank_single` build `full = problem.current_values.clone()` + the solved
//! autos and call `eval_objective_set` with NO dependent-cell fold. Once the
//! per-trial fold lands, `ConstraintCostFunction::cost` minimises the FOLDED
//! objective while the ranker scores the STALE one — so the reported
//! `objective_score` disagrees with the optimum actually achieved, and the
//! ranking can prefer the wrong start.
//!
//! Placement: `reify-constraints` is absent from the consolidatable-crates list
//! in `tests/infra/harness-layout-lib.sh`, so a new top-level test file here
//! does not trip `check-harness-baseline-registration.sh`. The same file under
//! `crates/reify-eval/tests/` WOULD red the merge gate.
//!
//! Layering: precise-argmin assertions belong at this level, with explicitly
//! bounded autos (the sibling suites `cost_robustness_tradeoff_blend.rs` and
//! `robustness_floor.rs` follow the same rule). `.ri`-layer tests assert
//! ordering / strict inequality only.

use std::sync::Arc;

use reify_constraints::DimensionalSolver;
use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
use reify_ir::{
    AutoParam, BinOp, CompiledExpr, ConstraintSolver, ObjectiveSense, ObjectiveSet,
    RankedSolveResult, ResolutionProblem, Value, ValueMap,
};

/// The stale value seeded into `current_values` for the dependent cell.
/// Deliberately matches no reachable folded score, so a stale read is an
/// unmistakable wrong answer rather than a coincidentally-right one.
const STALE_TOTAL: f64 = 777.0;

/// Coefficient on `a` in the dependent cell `total = A_COEFF*a + B_COEFF*b`.
const A_COEFF: f64 = 1.0;
/// Coefficient on `b`. Distinct from [`A_COEFF`] so the two autos contribute
/// asymmetrically and the argmin is not degenerate along a diagonal.
const B_COEFF: f64 = 3.0;

const LO: f64 = 1.0;
const HI: f64 = 10.0;

fn dimensionless() -> Type {
    Type::dimensionless_scalar()
}

fn scalar(v: f64) -> Value {
    Value::Scalar {
        si_value: v,
        dimension: DimensionVector::DIMENSIONLESS,
    }
}

fn lit(v: f64) -> CompiledExpr {
    CompiledExpr::literal(scalar(v), dimensionless())
}

fn vref(id: &ValueCellId) -> CompiledExpr {
    CompiledExpr::value_ref(id.clone(), dimensionless())
}

/// The minimal multistart-eligible joint-drive problem.
///
/// * TWO autos `a`, `b` — `auto_params.len() >= 2` is half the multistart gate;
/// * an objective (the other half) that reads ONLY the dependent cell `total`,
///   never an auto directly — exactly the indirection the seam exists to drive
///   through, and the reason a stale scoring map cannot tell the candidates
///   apart;
/// * `dependent_cells = [(total, A_COEFF*a + B_COEFF*b)]`;
/// * `current_values` seeding `total` STALE;
/// * two trivially-satisfiable constraints so every start is feasible and the
///   scoring loop actually collects candidates to rank.
///
/// The objective is dimensionless, NOT Money, so `objective_is_money` is false
/// and the robustness floor stays out of the picture — this test is about the
/// scoring map, and a floor-infeasibility would only confound it.
fn multistart_joint_drive_problem() -> (ResolutionProblem, ValueCellId, ValueCellId, ValueCellId) {
    let a_id = ValueCellId::new("Part", "a");
    let b_id = ValueCellId::new("Part", "b");
    let total_id = ValueCellId::new("Part", "total");

    let mut current_values = ValueMap::new();
    current_values.insert(total_id.clone(), scalar(STALE_TOTAL));

    // total = A_COEFF*a + B_COEFF*b — strictly increasing in both autos, so the
    // argmin is the lower corner (LO, LO) and the folded optimum is analytic.
    let total_expr = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(BinOp::Mul, lit(A_COEFF), vref(&a_id), dimensionless()),
        CompiledExpr::binop(BinOp::Mul, lit(B_COEFF), vref(&b_id), dimensionless()),
        dimensionless(),
    );

    let problem = ResolutionProblem {
        auto_params: vec![
            AutoParam {
                id: a_id.clone(),
                param_type: dimensionless(),
                bounds: Some((LO, HI)),
                free: true,
            },
            AutoParam {
                id: b_id.clone(),
                param_type: dimensionless(),
                bounds: Some((LO, HI)),
                free: true,
            },
        ],
        constraints: vec![
            (
                ConstraintNodeId::new("Part", 0),
                CompiledExpr::binop(BinOp::Ge, vref(&a_id), lit(LO), Type::Bool),
            ),
            (
                ConstraintNodeId::new("Part", 1),
                CompiledExpr::binop(BinOp::Ge, vref(&b_id), lit(LO), Type::Bool),
            ),
        ],
        current_values,
        objective: Some(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            vref(&total_id),
        )),
        functions: Arc::from(Vec::new()),
        dependent_cells: vec![(total_id.clone(), total_expr)],
    };

    (problem, a_id, b_id, total_id)
}

/// Recompute the objective the way the SOLVER's cost surface sees it: fold the
/// dependent cell at the candidate's solved autos, then read `total`.
///
/// Written out longhand rather than reusing a solver-internal helper precisely
/// so the assertion is independent of the code under test.
fn folded_objective(a: f64, b: f64) -> f64 {
    A_COEFF * a + B_COEFF * b
}

/// §12 Q2 — the reported `objective_score` must be the FOLDED objective at the
/// winner's solved autos, and the winner must be the folded argmin.
///
/// RED before the scoring-map fix: both `rank_single` and the multistart
/// scoring loop score against `current_values` + solved autos with no fold, so
/// every candidate reads `total`'s stale seed. That has two visible
/// consequences, and this test pins both:
///
///   (a) the reported score is [`STALE_TOTAL`], not the achieved optimum; and
///   (b) because every candidate scores the SAME stale constant, the ranking
///       degenerates to a pure tie broken by start index — the optimum is
///       selected by accident rather than by measurement.
#[test]
fn multistart_objective_score_is_the_folded_objective_at_the_winner() {
    let (problem, a_id, b_id, total_id) = multistart_joint_drive_problem();

    let ranked = DimensionalSolver.solve_ranked(&problem);

    let RankedSolveResult::Ranked { candidates, .. } = ranked else {
        panic!("the problem is feasible and multistart-eligible, so it must rank: {ranked:?}");
    };
    let winner = candidates.first().expect("at least one ranked candidate");

    let a = winner
        .values
        .get(&a_id)
        .and_then(|v| v.as_f64())
        .expect("auto `a` solved");
    let b = winner
        .values
        .get(&b_id)
        .and_then(|v| v.as_f64())
        .expect("auto `b` solved");
    let score = winner
        .objective_score
        .expect("a BestFound candidate carries a score");

    let expected = folded_objective(a, b);

    // (a) the reported score must agree with the cost surface that produced it.
    assert!(
        (score - expected).abs() < 1e-6,
        "the reported objective_score must be the FOLDED objective at the \
         winner's solved autos (a={a}, b={b} ⇒ {expected}); got {score}. \
         A score of {STALE_TOTAL} means the ranker read `total` from the \
         unfolded `current_values`, so the optimiser and the ranker were \
         measuring different objectives."
    );
    assert!(
        (score - STALE_TOTAL).abs() > 1e-6,
        "the reported score must not be the stale seed {STALE_TOTAL}; got {score}"
    );

    // (b) the winner must be the true folded argmin. `total` is strictly
    // increasing in both autos over the box, so the argmin is the lower corner.
    let optimum = folded_objective(LO, LO);
    assert!(
        (score - optimum).abs() < 1e-3,
        "the winner must be the folded argmin — `total` is strictly increasing \
         in both autos, so the optimum is the lower corner (a={LO}, b={LO}) \
         ⇒ {optimum}; got {score} at (a={a}, b={b})"
    );

    // Sanity: the stale seed must still be sitting in `current_values`, so the
    // test cannot pass merely because the fixture forgot to seed it.
    assert_eq!(
        problem.current_values.get(&total_id).and_then(|v| v.as_f64()),
        Some(STALE_TOTAL),
        "fixture integrity: `total` must be seeded stale in current_values"
    );
}

/// The single-candidate path (`rank_single`) carries the SAME stale-scoring
/// bug, and it is reached whenever the multistart gate does not fire — which is
/// every dim-1 cluster, i.e. the shape `examples/whole_model_joint_drive.ri`
/// actually produces. Fixing only the multistart loop would leave the common
/// case broken, so both sites are pinned.
#[test]
fn rank_single_objective_score_is_the_folded_objective() {
    let (mut problem, a_id, _b_id, _total_id) = multistart_joint_drive_problem();

    // Drop to ONE auto so `auto_params.len() >= 2` fails and the solve takes
    // the `rank_single` path. `b` becomes a fixed value in `current_values`, so
    // the dependent-cell expression still evaluates.
    let b_id = ValueCellId::new("Part", "b");
    problem.auto_params.truncate(1);
    problem.constraints.truncate(1);
    problem.current_values.insert(b_id.clone(), scalar(2.0));

    let ranked = DimensionalSolver.solve_ranked(&problem);

    let RankedSolveResult::Ranked { candidates, .. } = ranked else {
        panic!("the problem is feasible, so it must rank: {ranked:?}");
    };
    let winner = candidates.first().expect("at least one ranked candidate");

    let a = winner
        .values
        .get(&a_id)
        .and_then(|v| v.as_f64())
        .expect("auto `a` solved");
    let score = winner
        .objective_score
        .expect("a BestFound candidate carries a score");

    let expected = folded_objective(a, 2.0);
    assert!(
        (score - expected).abs() < 1e-6,
        "rank_single must score the FOLDED objective at the solved auto \
         (a={a}, b=2.0 ⇒ {expected}); got {score}. A score of {STALE_TOTAL} \
         means the single-candidate path still reads `total` unfolded."
    );
}
