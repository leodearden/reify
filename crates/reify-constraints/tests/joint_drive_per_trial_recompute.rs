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

use reify_constraints::{DimensionalSolver, build_centrality_objective};
use reify_core::{ConstraintNodeId, DimensionVector, Type, ValueCellId};
use reify_ir::{
    AutoParam, BinOp, CompiledExpr, ConstraintSolver, ObjectiveCombination, ObjectiveSense,
    ObjectiveSet, ObjectiveTerm, RankedSolveResult, ResolutionProblem, SolveResult, Value,
    ValueMap,
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

// ════════════════════════════════════════════════════════════════════════════
// BT-11 (PRD §7; renumbered from a §7-collision by task #5764) —
// `cost_robustness_tradeoff` is a THIRD, unconverted scoring site
// ════════════════════════════════════════════════════════════════════════════
//
// `solve_cost_robustness_tradeoff` materialises its two ANCHOR-SCORING maps
// inline (`problem.current_values.clone()` + the solved autos from `x_cost` /
// `x_rob`) and never folds `dependent_cells`. The two anchor SOLVES do fold —
// they go through `solve_core_with_sd_tolerance`, and `..problem.clone()`
// inherits `dependent_cells` — so the anchors genuinely move; but `cost_expr`
// and `min_slack_expr` are then evaluated at both anchors against STALE maps.
//
// In the joint-drive shape the money expression IS a read of a dependent cell
// (`RivetedPanel.rivets.line_cost`), so both anchor evaluations return the
// IDENTICAL stale base number ⇒ `cost_max - cost_min == 0` ⇒
// `normalised_blend_term`'s `TRADEOFF_NORMALISATION_RANGE_EPS` guard collapses
// the cost term to a literal `0.0`. Net: `minimize cost_robustness_tradeoff(…)`
// over a joint-drive cluster silently drops the cost axis FOR EVERY λ and
// degenerates to pure robustness, with no diagnostic.
//
// The indirection is the whole point of the fixture below: an objective that
// read the autos INLINE would fold trivially inside the anchor solves and the
// bug would not reproduce. `SolveResult::Solved.values` carries only the AUTOS
// (`build_solved_values`), so overlaying it onto `current_values` leaves the
// dependent cell at its seed — that is the exact gap.
//
// These assertions go through the PUBLIC `DimensionalSolver` API because that
// is what a user actually observes. An in-crate probe of `cost_max != cost_min`
// would pin the named line more directly, but reconstructing the two anchor
// evaluations in a test just re-implements the code under test, so the
// end-to-end behavioural assertions are the load-bearing ones.

/// Lower / upper CONSTRAINT bracket on `t` (metres). Chebyshev centre 2.5mm.
const T_CONSTRAINT_LO_M: f64 = 0.001;
const T_CONSTRAINT_HI_M: f64 = 0.004;

/// `AutoParam` bounds — deliberately wider and asymmetric relative to the
/// constraint bracket, so the bounds-midpoint seed (3mm) coincides with NEITHER
/// the cost argmin (1mm) NOR the Chebyshev centre (2.5mm). A passing assertion
/// can only come from the solver actually moving, never from a seed/target
/// coincidence. Mirrors `cost_robustness_tradeoff_blend.rs`'s `base_problem`.
const T_BOUND_LO_M: f64 = 0.001;
const T_BOUND_HI_M: f64 = 0.005;

/// Per-unit price read by the dependent cell. Lives in `current_values`, NOT in
/// the objective — so the objective's only route to it is through the fold.
const UNIT_COST_USD: f64 = 5.0;

/// Stale seed for the dependent cell `line_cost`. Deliberately far outside the
/// achievable folded range (`[5, 25] USD` over the bounds box) so a stale read
/// is unmistakable, and so `cost_min == cost_max == STALE` is reached by the
/// diagnosed collapse rather than by numerical coincidence.
const STALE_LINE_COST_USD: f64 = 999.0;

/// λ near 1 ⇒ cost-dominant blend; λ near 0 ⇒ robustness-dominant. Kept off the
/// exact endpoints so neither term is switched off by its own weight — the
/// separation must come from the blend being LIVE on both axes, not from a
/// degenerate λ=1 / λ=0 special case.
const LAMBDA_COST_DOMINANT: f64 = 0.9;
const LAMBDA_ROBUSTNESS_DOMINANT: f64 = 0.1;

fn money_ty() -> Type {
    Type::Scalar { dimension: DimensionVector::MONEY }
}

fn length_ty() -> Type {
    Type::Scalar { dimension: DimensionVector::LENGTH }
}

fn length_lit(si_m: f64) -> CompiledExpr {
    CompiledExpr::literal(
        Value::Scalar { si_value: si_m, dimension: DimensionVector::LENGTH },
        length_ty(),
    )
}

/// The tradeoff-path joint-drive problem: `1mm < t < 4mm`, minimising a Money
/// objective that reads ONLY the dependent cell `line_cost`.
///
/// `line_cost = unit_cost × (t / 1mm)` — strictly increasing in `t`, so the cost
/// argmin is the LOWER constraint bracket while the robustness argmax is the
/// Chebyshev centre. That non-degenerate separation is what makes the blend's
/// two axes distinguishable at all; it is the same argument BT-5 rests on, not a
/// tuned constant.
fn tradeoff_joint_drive_problem(lambda: f64) -> (ResolutionProblem, ValueCellId) {
    let t_id = ValueCellId::new("RivetedPanel", "t");
    let unit_cost_id = ValueCellId::new("RivetedPanel", "unit_cost");
    let line_cost_id = ValueCellId::new("RivetedPanel", "line_cost");

    let mut current_values = ValueMap::new();
    current_values.insert(
        unit_cost_id.clone(),
        Value::Scalar { si_value: UNIT_COST_USD, dimension: DimensionVector::MONEY },
    );
    current_values.insert(
        line_cost_id.clone(),
        Value::Scalar { si_value: STALE_LINE_COST_USD, dimension: DimensionVector::MONEY },
    );

    // line_cost = unit_cost * (t / 1mm)
    let t_per_mm = CompiledExpr::binop(
        BinOp::Div,
        CompiledExpr::value_ref(t_id.clone(), length_ty()),
        length_lit(0.001),
        Type::dimensionless_scalar(),
    );
    let line_cost_expr = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(unit_cost_id.clone(), money_ty()),
        t_per_mm,
        money_ty(),
    );

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: t_id.clone(),
            param_type: length_ty(),
            bounds: Some((T_BOUND_LO_M, T_BOUND_HI_M)),
            free: true,
        }],
        constraints: vec![
            (
                ConstraintNodeId::new("RivetedPanel", 0),
                CompiledExpr::binop(
                    BinOp::Gt,
                    CompiledExpr::value_ref(t_id.clone(), length_ty()),
                    length_lit(T_CONSTRAINT_LO_M),
                    Type::Bool,
                ),
            ),
            (
                ConstraintNodeId::new("RivetedPanel", 1),
                CompiledExpr::binop(
                    BinOp::Lt,
                    CompiledExpr::value_ref(t_id.clone(), length_ty()),
                    length_lit(T_CONSTRAINT_HI_M),
                    Type::Bool,
                ),
            ),
        ],
        current_values,
        // The objective is a bare ValueRef to the dependent cell — the whole
        // point. Inline arithmetic over `t` would fold trivially.
        objective: Some(ObjectiveSet::cost_robustness_tradeoff(
            CompiledExpr::value_ref(line_cost_id.clone(), money_ty()),
            lambda,
        )),
        functions: Arc::from(Vec::new()),
        dependent_cells: vec![(line_cost_id, line_cost_expr)],
    };

    (problem, t_id)
}

/// Solve and extract `t`'s SI value, panicking on any non-`Solved` result — this
/// 1-D two-sided problem is well-posed at every point in the file's lifecycle,
/// so Infeasible/NoProgress is a bug, not an expected branch.
fn solve_tradeoff_t(problem: &ResolutionProblem, t_id: &ValueCellId) -> f64 {
    match DimensionalSolver.solve(problem) {
        SolveResult::Solved { values, .. } => values
            .get(t_id)
            .and_then(|v| v.as_f64())
            .expect("solved value for t missing or non-numeric"),
        other => panic!("expected Solved for the tradeoff joint-drive problem, got {other:?}"),
    }
}

/// Independent reference for the ROBUSTNESS anchor: solve the same auto param /
/// constraints under the plain `Maximize(min_slack)` centrality objective, with
/// no tradeoff marker at all. Computed structurally rather than hardcoded, so
/// the assertions below never depend on a hand-derived Chebyshev centre.
fn robustness_reference_t() -> f64 {
    let (problem, t_id) = tradeoff_joint_drive_problem(LAMBDA_COST_DOMINANT);
    let centrality = build_centrality_objective(&problem.auto_params, &problem.constraints).expect(
        "two-sided inequalities on a Scalar auto param must synthesise a centrality objective",
    );
    let reference = ResolutionProblem { objective: Some(centrality), ..problem };
    solve_tradeoff_t(&reference, &t_id)
}

/// BT-11 (a), PRIMARY — a cost-dominant λ and a robustness-dominant λ must
/// produce MATERIALLY different solutions.
///
/// RED before the fix: both anchor-scoring maps read `line_cost` unfolded, so
/// `cost_max - cost_min == 0`, the cost term collapses to a literal `0.0`, and
/// the blend reduces to `-(1-λ)·robustness` — whose argmin is the Chebyshev
/// centre for EVERY λ. The two solves therefore coincide exactly and the
/// separation below is 0.
#[test]
fn tradeoff_cost_axis_is_live_across_lambda() {
    let (cost_dominant, t_id) = tradeoff_joint_drive_problem(LAMBDA_COST_DOMINANT);
    let (robustness_dominant, _) = tradeoff_joint_drive_problem(LAMBDA_ROBUSTNESS_DOMINANT);

    let t_cost_dominant = solve_tradeoff_t(&cost_dominant, &t_id);
    let t_robustness_dominant = solve_tradeoff_t(&robustness_dominant, &t_id);

    // The scale to judge "material" against is measured, not guessed: the gap
    // between the two ANCHORS this blend interpolates. The cost argmin is the
    // lower constraint bracket analytically (`line_cost` is strictly increasing
    // in `t`); the robustness argmax comes from an independent centrality solve.
    let anchor_separation = (robustness_reference_t() - T_CONSTRAINT_LO_M).abs();
    let observed = (t_cost_dominant - t_robustness_dominant).abs();

    // On this 1-D linear blend the two λ values are analytically bang-bang —
    // λ>0.5 pushes to the cost argmin, λ<0.5 to the centre — so `observed`
    // should be ≈ 1.0 × `anchor_separation`. Half of that is a floor that leaves
    // ample room for anchor-solve numerics while excluding the RED behaviour,
    // which is exactly 0.
    assert!(
        observed > 0.5 * anchor_separation,
        "a cost-dominant (λ={LAMBDA_COST_DOMINANT}) and a robustness-dominant \
         (λ={LAMBDA_ROBUSTNESS_DOMINANT}) solve must differ materially: got \
         t={t_cost_dominant:.6e} m vs t={t_robustness_dominant:.6e} m \
         (separation {observed:.6e} m, anchor gap {anchor_separation:.6e} m). \
         A separation of ~0 means both anchor-scoring maps read `line_cost` \
         from the unfolded `current_values`, so `cost_max - cost_min` collapsed \
         to 0 and the cost axis was dropped from the blend entirely."
    );
}

/// BT-11 (b), SECONDARY — pin the DIRECTION of the blend, so a future
/// regression that makes the axes merely differ (rather than differ correctly)
/// still fails.
///
/// `line_cost` is strictly increasing in `t`, so the cost argmin is the lower
/// constraint bracket. The cost-dominant λ must land strictly nearer to it.
#[test]
fn tradeoff_cost_dominant_lambda_lands_nearer_the_cost_argmin() {
    let (cost_dominant, t_id) = tradeoff_joint_drive_problem(LAMBDA_COST_DOMINANT);
    let (robustness_dominant, _) = tradeoff_joint_drive_problem(LAMBDA_ROBUSTNESS_DOMINANT);

    let t_cost_dominant = solve_tradeoff_t(&cost_dominant, &t_id);
    let t_robustness_dominant = solve_tradeoff_t(&robustness_dominant, &t_id);

    let d_cost = (t_cost_dominant - T_CONSTRAINT_LO_M).abs();
    let d_robustness = (t_robustness_dominant - T_CONSTRAINT_LO_M).abs();

    assert!(
        d_cost < d_robustness,
        "the cost-dominant solve (λ={LAMBDA_COST_DOMINANT}) must land strictly \
         nearer the cost argmin ({T_CONSTRAINT_LO_M:.6e} m) than the \
         robustness-dominant one (λ={LAMBDA_ROBUSTNESS_DOMINANT}): got \
         |Δ|={d_cost:.6e} m vs |Δ|={d_robustness:.6e} m \
         (t={t_cost_dominant:.6e} m vs t={t_robustness_dominant:.6e} m). \
         Equal distances mean the cost axis is not participating in the blend."
    );
}

// ---------------------------------------------------------------------------
// Registry propagation — the PRODUCTION dispatch path (esc-5189-5)
// ---------------------------------------------------------------------------
//
// Every other test in this file calls `DimensionalSolver` directly. That is the
// right layer for argmin precision, but it is NOT the solver a user reaches:
// `reify-cli`'s `configured_eval_engine` wires
// `.with_solver(Box::new(SolverRegistry::production()))`, so a real
// `reify eval` goes Engine → SolverRegistry → component decomposition →
// DimensionalSolver. `SolverRegistry::solve_inner` rebuilds a fresh
// `ResolutionProblem` per connected component, and it used to enumerate that
// struct's fields by hand with `dependent_cells: Vec::new()` — silently zeroing
// the field this whole task adds. `fold_dependent_cells` then took its
// empty-vector early return and the per-trial recompute never ran for anyone
// outside these tests.
//
// This test closes that gap: it drives a joint-drive problem through
// `SolverRegistry::production()` and asserts the SOLVED AUTO, which only the
// folded objective can explain.

/// Coefficient on the single auto `t` in the dependent cell `total = T_COEFF*t`.
/// Positive, so `total` is strictly increasing in `t` and the argmin is `t`'s
/// lower bound — an analytic corner, no optimizer-quality assumption needed.
const T_COEFF: f64 = 2.0;

/// The one-dimensional joint-drive problem: ONE auto `t`, an objective that
/// reads ONLY the dependent cell `total`, and `current_values` seeding `total`
/// at [`STALE_TOTAL`].
///
/// Deliberately 1-D rather than reusing [`multistart_joint_drive_problem`].
/// `auto_params.len() >= 2` is the multistart gate, and on a 2-D fixture the
/// per-start Nelder-Mead runs terminate at assorted non-global points — a real
/// optimizer-quality property, but one that would make this test's verdict
/// depend on NM convergence rather than on the thing under test. At dim<=1 the
/// solver takes the single-candidate `rank_single` path and descends a
/// monotone 1-D cost surface straight to the bracket, so "did the fold reach
/// the domain solver?" is the ONLY variable left.
fn registry_joint_drive_problem() -> (ResolutionProblem, ValueCellId) {
    let t_id = ValueCellId::new("Part", "t");
    let total_id = ValueCellId::new("Part", "total");

    let mut current_values = ValueMap::new();
    current_values.insert(total_id.clone(), scalar(STALE_TOTAL));

    let problem = ResolutionProblem {
        auto_params: vec![AutoParam {
            id: t_id.clone(),
            param_type: dimensionless(),
            bounds: Some((LO, HI)),
            free: true,
        }],
        constraints: vec![(
            ConstraintNodeId::new("Part", 0),
            CompiledExpr::binop(BinOp::Ge, vref(&t_id), lit(LO), Type::Bool),
        )],
        current_values,
        objective: Some(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            vref(&total_id),
        )),
        functions: Arc::from(Vec::new()),
        dependent_cells: vec![(
            total_id,
            CompiledExpr::binop(BinOp::Mul, lit(T_COEFF), vref(&t_id), dimensionless()),
        )],
    };

    (problem, t_id)
}

/// esc-5189-5 — `SolverRegistry` must propagate `dependent_cells` into every
/// per-component sub-problem, so the per-trial fold actually runs on the
/// production dispatch path.
///
/// RED before the registry fix: `solve_inner` hands the domain solver an empty
/// `dependent_cells`, so `total` stays pinned at its [`STALE_TOTAL`] seed at
/// every trial point. A constant objective has no gradient, so the solve
/// degenerates to pure feasibility and settles at its already-feasible start
/// (the midpoint of `[LO, HI]`) instead of descending to the bracket at `LO`.
///
/// The paired direct-`DimensionalSolver` assertion is what makes the verdict
/// specific: it pins the fold as working one layer down, so a failure here can
/// only be the registry's per-component rebuild dropping the field.
#[test]
fn solver_registry_propagates_dependent_cells_into_component_subproblems() {
    let (problem, t_id) = registry_joint_drive_problem();

    let solved_t = |result: SolveResult| -> f64 {
        let SolveResult::Solved { values, .. } = result else {
            panic!("`t = {LO}` is feasible, so the solve must succeed; got: {result:?}");
        };
        values
            .get(&t_id)
            .and_then(|v| v.as_f64())
            .expect("auto `t` solved")
    };

    // Baseline: the fold demonstrably works when the domain solver is handed
    // the problem directly, so this fixture is a valid probe.
    let direct_t = solved_t(DimensionalSolver.solve(&problem));
    assert!(
        (direct_t - LO).abs() < 1e-3,
        "precondition — the bare `DimensionalSolver` must fold `total` and \
         descend to the bracket t={LO}; got t={direct_t:.6}. If THIS fails, the \
         per-trial fold itself regressed, not the registry."
    );

    // The production dispatch path must reach the same answer.
    let registry_t = solved_t(reify_constraints::SolverRegistry::production().solve(&problem));
    assert!(
        (registry_t - LO).abs() < 1e-3,
        "the production path (`SolverRegistry::production()`, what \
         `reify-cli`'s `configured_eval_engine` wires) must reach the same \
         folded argmin as the bare solver: expected t={LO} (as the direct solve \
         got, t={direct_t:.6}); got t={registry_t:.6}. A divergence means \
         `SolverRegistry::solve_inner` rebuilt the per-component sub-problem \
         with an empty `dependent_cells`, so `total` stayed pinned at its stale \
         seed {STALE_TOTAL} and the objective was constant across every trial \
         point."
    );
}

// ---------------------------------------------------------------------------
// Lexicographic ε-band — both sides of the band must be measured folded
// ---------------------------------------------------------------------------
//
// `solve_lexicographic`'s staged loop freezes each rank's realized optimum
// `obj*` as an ε-band constraint for the next stage. Once the stage problem
// inherits `dependent_cells`, the band has TWO sides measured on two different
// value maps unless `obj*` is folded too:
//
//   * `build_band_constraints` bakes `obj*` in as a LITERAL, computed by
//     `eval_rank_cost` against `current_values` — which carries the warm-started
//     solved autos but leaves every dependent cell at its stale base value,
//     because `SolveResult::Solved` returns only the AUTOS;
//   * the band's `cost_expr` is built from the rank's term exprs, which read the
//     dependent cell, and the NEXT stage's solver evaluates it FOLDED.
//
// Stale literal vs folded expression. The gap surfaces as a bogus
// `ConstraintUnsatisfiable` on a trivially feasible model when the stale value
// sits on the restrictive side, and as a silently-dropped rank ordering (a band
// that is trivially satisfied) when it sits on the permissive side.

/// Priority of the rank whose realized optimum gets frozen into the ε-band.
/// Must be strictly greater than [`P_LOW`] so `priority_order.len() == 2` and
/// the multi-rank staged loop runs — at one distinct priority
/// `solve_lexicographic` delegates to the degenerate WeightedSum path and never
/// builds a band at all.
const P_HIGH: u32 = 1;
/// Priority of the second rank — the one that must solve UNDER the first rank's
/// frozen band.
const P_LOW: u32 = 0;

/// A Lexicographic joint-drive problem whose FIRST rank scores through the
/// dependent cell.
///
/// `total` is the coupling: the high-priority rank minimises it, so `obj*` for
/// that rank can only be computed correctly by folding. The low-priority rank
/// reads the auto `b` directly, so the second stage is well-posed on its own and
/// any failure is attributable to the band rather than to the rank itself.
///
/// The single `a + b >= A_PLUS_B_FLOOR` constraint keeps both autos in ONE
/// connected component, so the whole objective reaches a single staged solve
/// instead of being split across components.
fn lexicographic_joint_drive_problem() -> (ResolutionProblem, ValueCellId, ValueCellId) {
    const A_PLUS_B_FLOOR: f64 = 4.0;

    let a_id = ValueCellId::new("Part", "a");
    let b_id = ValueCellId::new("Part", "b");
    let total_id = ValueCellId::new("Part", "total");

    let mut current_values = ValueMap::new();
    current_values.insert(total_id.clone(), scalar(STALE_TOTAL));

    let auto = |id: &ValueCellId| AutoParam {
        id: id.clone(),
        param_type: dimensionless(),
        bounds: Some((LO, HI)),
        free: true,
    };

    let problem = ResolutionProblem {
        auto_params: vec![auto(&a_id), auto(&b_id)],
        constraints: vec![(
            ConstraintNodeId::new("Part", 0),
            CompiledExpr::binop(
                BinOp::Ge,
                CompiledExpr::binop(BinOp::Add, vref(&a_id), vref(&b_id), dimensionless()),
                lit(A_PLUS_B_FLOOR),
                Type::Bool,
            ),
        )],
        current_values,
        objective: Some(ObjectiveSet {
            terms: vec![
                ObjectiveTerm {
                    sense: ObjectiveSense::Minimize,
                    expr: vref(&total_id),
                    weight: 1.0,
                    priority: P_HIGH,
                },
                ObjectiveTerm {
                    sense: ObjectiveSense::Minimize,
                    expr: vref(&b_id),
                    weight: 1.0,
                    priority: P_LOW,
                },
            ],
            combination: ObjectiveCombination::Lexicographic,
            cost_robustness_lambda: None,
        }),
        functions: Arc::from(Vec::new()),
        dependent_cells: vec![(
            total_id,
            CompiledExpr::binop(BinOp::Add, vref(&a_id), vref(&b_id), dimensionless()),
        )],
    };

    (problem, a_id, b_id)
}

/// esc-5189-7 — the ε-band anchor must be folded, so both sides of the band
/// constraint are measured on the same value map.
///
/// RED before the `eval_rank_cost` fix: `obj*` is read unfolded, so the band
/// literal is [`STALE_TOTAL`] while the band's own `cost_expr` evaluates folded
/// (to `a + b`, at most `2·HI`). The band becomes unsatisfiable by a margin of
/// roughly `STALE_TOTAL − (a + b)`, and a model with an obviously feasible
/// solution reports `ConstraintUnsatisfiable` to the user.
#[test]
fn lexicographic_epsilon_band_anchor_is_folded_like_the_stage_it_constrains() {
    let (problem, a_id, b_id) = lexicographic_joint_drive_problem();

    let result = reify_constraints::SolverRegistry::production().solve(&problem);

    let SolveResult::Solved { values, .. } = result else {
        panic!(
            "every point with a+b >= 4 inside [{LO}, {HI}]² is feasible — e.g. \
             (a, b) = (3, 1) — so the staged lexicographic solve must succeed. \
             Got: {result:?}\n\n\
             An `Infeasible` with a residual near {STALE_TOTAL} is the ε-band \
             defect: `eval_rank_cost` measured the rank's obj* against \
             `current_values`, where `total` is still its stale seed, and \
             `build_band_constraints` froze that stale number in as a literal — \
             while the next stage evaluates the band's `cost_expr` FOLDED. The \
             two sides of the band are then measured on different value maps."
        );
    };

    let a = values.get(&a_id).and_then(|v| v.as_f64()).expect("`a` solved");
    let b = values.get(&b_id).and_then(|v| v.as_f64()).expect("`b` solved");

    // The band must also still BIND — otherwise the mirror failure (a band
    // frozen on the PERMISSIVE side of stale) passes silently: the second rank
    // would be free to sacrifice the first entirely, and the solve would still
    // report Solved.
    //
    // The anchor is COMPARATIVE, deliberately. The obvious assertion — "a+b must
    // equal its analytic argmin 4.0" — is wrong here, because it pins Nelder-Mead
    // convergence quality rather than band correctness. Stage 1 realizes a
    // non-global point on this 2-D surface (~11 as authored), and freezing THAT
    // is precisely what the band is specified to do: it freezes the rank's
    // REALIZED optimum, not its theoretical one. So the reference is what the
    // same registry achieves on rank P_HIGH alone — which is exactly the
    // WeightedSum sub-problem the staged loop builds for stage 1.
    let (mut reference, ref_a, ref_b) = lexicographic_joint_drive_problem();
    reference.objective = Some(ObjectiveSet::single(
        ObjectiveSense::Minimize,
        // Same expr the P_HIGH term carries: a read of the dependent cell.
        vref(&ValueCellId::new("Part", "total")),
    ));
    let SolveResult::Solved { values: ref_values, .. } =
        reify_constraints::SolverRegistry::production().solve(&reference)
    else {
        panic!("the single-rank reference solve must succeed");
    };
    let ref_total = ref_values.get(&ref_a).and_then(|v| v.as_f64()).unwrap()
        + ref_values.get(&ref_b).and_then(|v| v.as_f64()).unwrap();

    // Tolerance covers the ε-band half-width (LEX_EPSILON_BAND_REL = 1e-3
    // relative) plus solver numerics, and is far below the stale-vs-folded gap a
    // permissive band would open.
    assert!(
        a + b <= ref_total * 1.01 + 1e-2,
        "the first rank's realized optimum must still bind the second rank: the \
         lexicographic solve landed at a={a:.6}, b={b:.6} ⇒ a+b={:.6}, but a \
         single-rank solve of the SAME high-priority objective achieves \
         {ref_total:.6}. A materially larger sum means the ε-band was frozen at \
         a value so permissive it stopped enforcing the ordering for that rank \
         — the mirror of the stale-restrictive failure, and silent.",
        a + b
    );
}

// ---------------------------------------------------------------------------
// Cross-component joint drive — decomposition must follow `dependent_cells`
// ---------------------------------------------------------------------------
//
// β made `dependent_cells` reach the domain solver, but `SolverRegistry::
// solve_inner` still decomposes as if the objective read only the autos it
// mentions SYNTACTICALLY. The canonical β shape is a BARE read of a derived
// cell, so `obj_refs` holds no auto ids at all: `decompose_into_components`'
// objective-union step unions nothing, and two autos coupled ONLY through a
// dependent cell land in SEPARATE components. Two consequences, both here:
//
//   * the objective is attached by the hardcoded `0` fallthrough (registry.rs
//     :202-211) to a NONDETERMINISTIC component, so the other component's
//     autos are solved feasibility-only against stale seeds; and
//   * each sub-problem is handed `dependent_cells` WHOLESALE, so the component
//     owning only `a` folds `total = A_COEFF*a + B_COEFF*b` with `b` unbound —
//     the fold writes `Undef` and the objective read yields
//     `NoProgress { reason: "objective expression evaluated to undefined at
//     solution point" }`.

/// A joint-drive problem whose autos are coupled ONLY through a dependent cell.
///
/// * autos `a`, `b`, each bounded on `[LO, HI]`;
/// * TWO constraints that each touch exactly ONE auto, so
///   `decompose_into_components` splits them into two components on the
///   constraint graph alone. Deliberately NO constraint mentions both autos —
///   the split is the entire point of the fixture;
/// * `dependent_cells = [(total, A_COEFF*a + B_COEFF*b)]` — the only coupling;
/// * the objective is a BARE read of `total` (the canonical β shape), so
///   `obj_refs` contains no auto ids;
/// * `current_values` seeds ONLY `total`, at [`STALE_TOTAL`] — deliberately NOT
///   `a`/`b`, so a cross-component fold hits an unbound ref and yields `Undef`
///   rather than a coincidentally-plausible stale number.
fn cross_component_joint_drive_problem() -> (ResolutionProblem, ValueCellId, ValueCellId) {
    let a_id = ValueCellId::new("Part", "a");
    let b_id = ValueCellId::new("Part", "b");
    let total_id = ValueCellId::new("Part", "total");

    let mut current_values = ValueMap::new();
    current_values.insert(total_id.clone(), scalar(STALE_TOTAL));

    let total_expr = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::binop(BinOp::Mul, lit(A_COEFF), vref(&a_id), dimensionless()),
        CompiledExpr::binop(BinOp::Mul, lit(B_COEFF), vref(&b_id), dimensionless()),
        dimensionless(),
    );

    let auto = |id: &ValueCellId| AutoParam {
        id: id.clone(),
        param_type: dimensionless(),
        bounds: Some((LO, HI)),
        free: true,
    };

    let problem = ResolutionProblem {
        auto_params: vec![auto(&a_id), auto(&b_id)],
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
        dependent_cells: vec![(total_id, total_expr)],
    };

    (problem, a_id, b_id)
}

/// Decomposition must follow `dependent_cells`: two autos coupled only through
/// a derived cell the objective reads must be solved JOINTLY, at the true
/// folded argmin.
///
/// RED before the fix: the registry splits `a` and `b` into two components,
/// hands each the WHOLE `dependent_cells` list, and attaches the objective to
/// an arbitrary one. The component that owns only one auto folds `total` with
/// the other auto unbound → `Undef` → `NoProgress`. (Where the fold happens to
/// stay evaluable, the milder manifestation is a constant stale objective — no
/// gradient, so a silently suboptimal feasibility-only answer.)
///
/// No assertion here touches component ORDER or WHICH component receives the
/// objective: `decompose_into_components` iterates its component map, a
/// `HashMap`, so that is nondeterministic. The bad outcome is deterministic
/// regardless of which component wins, because NEITHER owns both autos.
#[test]
fn cross_component_dependent_cell_resolves_to_the_joint_argmin() {
    let (problem, a_id, b_id) = cross_component_joint_drive_problem();
    let total_id = ValueCellId::new("Part", "total");

    let solved_pair = |result: SolveResult, who: &str| -> (f64, f64) {
        let SolveResult::Solved { values, .. } = result else {
            panic!(
                "({who}) `(a, b) = ({LO}, {LO})` is feasible, so the solve must \
                 succeed; got: {result:?}\n\n\
                 A `NoProgress {{ reason: \"objective expression evaluated to \
                 undefined at solution point\" }}` is THE cross-component fold \
                 defect: the component solving `a` does not own `b`, but it was \
                 handed the whole `dependent_cells` list, so folding \
                 `total = {A_COEFF}*a + {B_COEFF}*b` read an unbound `b`, \
                 evaluated to `Undef`, and the objective's read of `total` \
                 inherited it."
            );
        };
        let a = values
            .get(&a_id)
            .and_then(|v| v.as_f64())
            .expect("auto `a` solved");
        let b = values
            .get(&b_id)
            .and_then(|v| v.as_f64())
            .expect("auto `b` solved");
        (a, b)
    };

    // Precondition: the fold demonstrably works one layer down, on the
    // UNDECOMPOSED problem. This also MEASURES the achievable tolerance rather
    // than assuming it, so a failure here is attributable to the per-trial fold
    // itself and not to the registry.
    let (direct_a, direct_b) = solved_pair(DimensionalSolver.solve(&problem), "direct");
    assert!(
        (direct_a - LO).abs() < 1e-3 && (direct_b - LO).abs() < 1e-3,
        "precondition — the bare `DimensionalSolver` on the UNDECOMPOSED \
         problem must fold `total` and descend to the lower corner \
         (a={LO}, b={LO}); got (a={direct_a:.6}, b={direct_b:.6}). If THIS \
         fails, the per-trial fold itself regressed, not the registry."
    );

    // The production dispatch path must reach the same JOINT argmin.
    let (a, b) = solved_pair(
        reify_constraints::SolverRegistry::production().solve(&problem),
        "registry",
    );
    assert!(
        (a - LO).abs() < 1e-3 && (b - LO).abs() < 1e-3,
        "the production path must reach the JOINT folded argmin \
         (a={LO}, b={LO}) — as the direct solve did \
         (a={direct_a:.6}, b={direct_b:.6}); got (a={a:.6}, b={b:.6}). \
         An auto sitting anywhere else in [{LO}, {HI}] means its component was \
         solved FEASIBILITY-ONLY: `obj_refs` holds no auto ids (the objective \
         is a bare read of `total`), so `decompose_into_components` unioned \
         nothing, `a` and `b` landed in separate components, and \
         `objective_component`'s lookup fell through to the hardcoded `0` \
         in `SolverRegistry::solve_inner` — attaching the objective to an arbitrary \
         component of a nondeterministic `HashMap` iteration and leaving the \
         other component's auto pinned near its stale start."
    );

    // Fixture integrity: the stale seed must still be sitting in
    // `current_values`, so the test cannot pass merely because the fixture
    // forgot to seed it.
    assert_eq!(
        problem.current_values.get(&total_id).and_then(|v| v.as_f64()),
        Some(STALE_TOTAL),
        "fixture integrity: `total` must be seeded stale in current_values"
    );
    assert_eq!(
        problem.current_values.get(&a_id).and_then(|v| v.as_f64()),
        None,
        "fixture integrity: `a` must NOT be seeded — an unbound auto is what \
         makes a cross-component fold yield `Undef` instead of a plausible \
         stale number"
    );
    assert_eq!(
        problem.current_values.get(&b_id).and_then(|v| v.as_f64()),
        None,
        "fixture integrity: `b` must NOT be seeded — see the `a` assertion"
    );
}

// ---------------------------------------------------------------------------
// Per-component `dependent_cells` filter
// ---------------------------------------------------------------------------
//
// `solve_inner` builds each component's sub-problem from `..problem.clone()`,
// which hands every component the WHOLE `dependent_cells` list. That is both
// the cross-component `Undef` source (a component folds a cell reading an auto
// it does not own) and the O(#components × |dependent_cells| × NM_iterations ×
// multistart_K) cost that per-trial folding pays for cells it can never move.

/// Coefficient on `c` in the second component's dependent cell `side`.
const SIDE_COEFF: f64 = 2.0;

/// The stale seed for `side`. Distinct from [`STALE_TOTAL`] so a mixed-up
/// assertion cannot pass by coincidence.
const STALE_SIDE: f64 = 555.0;

/// A problem that decomposes into TWO components even AFTER the `obj_refs`
/// expansion.
///
/// * component X — autos `a`, `b`, each with its own single-auto constraint;
///   dependent cells `mid = A_COEFF*a` then `total = mid + B_COEFF*b`; the
///   objective is a bare `Minimize total`, so the expansion unions `a` and `b`
///   into ONE component;
/// * component Y — auto `c` with its own constraint `c >= LO`, plus a dependent
///   cell `side = SIDE_COEFF*c` feeding the constraint `side >= LO` and nothing
///   in the objective. Feeding a constraint is what keeps `side` legitimately
///   inside `dependent_cells` under the documented membership invariant
///   (transitively feeds objective OR constraint exprs AND transitively reads
///   ≥1 auto). Decomposition itself skips `side >= LO` — it references no auto
///   directly — which is pre-existing behaviour and not what these tests probe.
/// * neither — `mix = mid + side`, which transitively reads `{a, c}` and so
///   STRADDLES both components.
///
/// The objective never reaches `c`, so component Y is what proves the filter
/// actually filters rather than being vacuously the identity.
///
/// Two shapes here exist purely to make the guard suite discriminating, and
/// both were added after review found the assertions unfalsifiable without
/// them:
///
/// * `mid` makes component X retain TWO cells. With one cell apiece the
///   subsequence/order assertions are vacuously true for ANY output ordering,
///   so a filter rebuilt from a `HashMap` iteration — the exact regression the
///   assertion messages describe — would go green. `mid` also exercises the
///   TRANSITIVE path end-to-end through the registry: `total` reaches `a` only
///   through `mid`, so a non-transitive `dependent_cell_auto_reads` would
///   report `total` as reading `{b}` alone.
/// * `mix` is the only cell whose auto set is neither owned nor disjoint from a
///   component. Without it every cell is either fully owned or fully disjoint,
///   and `is_subset` is indistinguishable from `!is_disjoint` — the mutation
///   that reintroduces the cross-component `Undef` fold this task removes.
///   `mix` is deliberately read by NO constraint; see the note at
///   ConstraintNodeId("Part", 3) for why a constraint reading it would be a
///   genuine a↔c coupling under task #5467's transitive decomposition.
struct TwoComponentFixture {
    problem: ResolutionProblem,
    a: ValueCellId,
    b: ValueCellId,
    c: ValueCellId,
    mid: ValueCellId,
    total: ValueCellId,
    side: ValueCellId,
    mix: ValueCellId,
}

fn two_component_dependent_cell_problem() -> TwoComponentFixture {
    let a = ValueCellId::new("Part", "a");
    let b = ValueCellId::new("Part", "b");
    let c = ValueCellId::new("Part", "c");
    let mid = ValueCellId::new("Part", "mid");
    let total = ValueCellId::new("Part", "total");
    let side = ValueCellId::new("Part", "side");
    let mix = ValueCellId::new("Part", "mix");

    let mut current_values = ValueMap::new();
    current_values.insert(mid.clone(), scalar(STALE_TOTAL));
    current_values.insert(total.clone(), scalar(STALE_TOTAL));
    current_values.insert(side.clone(), scalar(STALE_SIDE));
    current_values.insert(mix.clone(), scalar(STALE_SIDE));

    let auto = |id: &ValueCellId| AutoParam {
        id: id.clone(),
        param_type: dimensionless(),
        bounds: Some((LO, HI)),
        free: true,
    };
    let ge_lo = |e: CompiledExpr| CompiledExpr::binop(BinOp::Ge, e, lit(LO), Type::Bool);

    let problem = ResolutionProblem {
        auto_params: vec![auto(&a), auto(&b), auto(&c)],
        constraints: vec![
            (ConstraintNodeId::new("Part", 0), ge_lo(vref(&a))),
            (ConstraintNodeId::new("Part", 1), ge_lo(vref(&b))),
            (ConstraintNodeId::new("Part", 2), ge_lo(vref(&c))),
            // `side` reads {c} only, so this constrains the {c} component and
            // couples nothing new.
            (ConstraintNodeId::new("Part", 3), ge_lo(vref(&side))),
            // DELIBERATELY NOT CONSTRAINED: `mix`. It reads {a, c}, so under
            // task #5467's layer 2 (constraint refs now follow
            // `dependent_cells`) a constraint reading `mix` is a GENUINE a↔c
            // coupling and correctly collapses this fixture to ONE component —
            // destroying the two-component premise both tests below rest on,
            // and with it the per-component filter they exist to guard.
            //
            // The old `ge_lo(vref(&mix))` at ConstraintNodeId("Part", 4) was
            // not enforcing anything: pre-α its ref set was `{mix}`, which
            // intersected the auto params in NOTHING, so
            // `decompose_into_components` SKIPPED it outright and no solver
            // ever saw it. Removing it therefore drops zero coverage — it
            // removes a constraint that was silently inert, which is exactly
            // the α bug this branch fixes, sitting inside this fixture.
            //
            // `mix` REMAINS a dependent cell, which is the role it was added
            // for: it is the only cell whose auto set is neither owned by nor
            // disjoint from a component, so `is_subset` stays distinguishable
            // from `!is_disjoint`. A straddling cell and a constraint reading
            // it are simply incompatible with a two-component premise once
            // decomposition is transitive.
        ],
        current_values,
        objective: Some(ObjectiveSet::single(ObjectiveSense::Minimize, vref(&total))),
        functions: Arc::from(Vec::new()),
        // Stored order is load-bearing: `mid` BEFORE `total`, which reads it.
        dependent_cells: vec![
            // mid = A_COEFF*a — reads {a}.
            (
                mid.clone(),
                CompiledExpr::binop(BinOp::Mul, lit(A_COEFF), vref(&a), dimensionless()),
            ),
            // total = mid + B_COEFF*b — reads {a, b}, and `a` ONLY through `mid`.
            (
                total.clone(),
                CompiledExpr::binop(
                    BinOp::Add,
                    vref(&mid),
                    CompiledExpr::binop(BinOp::Mul, lit(B_COEFF), vref(&b), dimensionless()),
                    dimensionless(),
                ),
            ),
            // side = SIDE_COEFF*c — reads {c}.
            (
                side.clone(),
                CompiledExpr::binop(BinOp::Mul, lit(SIDE_COEFF), vref(&c), dimensionless()),
            ),
            // mix = mid + side — reads {a, c}: STRADDLES both components, so it
            // is a subset of NEITHER while overlapping BOTH.
            (
                mix.clone(),
                CompiledExpr::binop(BinOp::Add, vref(&mid), vref(&side), dimensionless()),
            ),
        ],
    };

    TwoComponentFixture {
        problem,
        a,
        b,
        c,
        mid,
        total,
        side,
        mix,
    }
}

/// Capture every sub-problem `SolverRegistry` hands its domain solver.
///
/// `solve()` runs with `want_optimality = false`, so EVERY component — the
/// objective-bearing one included — takes the plain `solver.solve()` arm and
/// the spy sees them all.
fn capture_subproblems(problem: &ResolutionProblem) -> Vec<ResolutionProblem> {
    let spy = reify_test_support::MultiCallSpyConstraintSolver::new(vec![SolveResult::Solved {
        values: std::collections::HashMap::new(),
        unique: true,
    }]);
    // Taken BEFORE the spy is boxed into the registry.
    let captured = spy.captured_problems();
    let registry = reify_constraints::SolverRegistry::new(Box::new(spy));
    let _ = ConstraintSolver::solve(&registry, problem);
    let guard = captured.lock().unwrap();
    guard.clone()
}

/// The ids of a sub-problem's autos, as a set — the ONLY safe way to identify a
/// captured component. `decompose_into_components` iterates its component map,
/// a `HashMap`, so capture ORDER is nondeterministic and an index-based
/// assertion would be intermittently red.
fn auto_id_set(p: &ResolutionProblem) -> std::collections::HashSet<ValueCellId> {
    p.auto_params.iter().map(|ap| ap.id.clone()).collect()
}

fn dependent_cell_ids(p: &ResolutionProblem) -> Vec<ValueCellId> {
    p.dependent_cells.iter().map(|(id, _)| id.clone()).collect()
}

/// Is `filtered` a subsequence of `original` (same relative order, gaps allowed)?
fn is_subsequence(filtered: &[ValueCellId], original: &[ValueCellId]) -> bool {
    let mut rest = original.iter();
    filtered.iter().all(|f| rest.any(|o| o == f))
}

/// Each component's sub-problem must carry ONLY the dependent cells whose
/// transitively-read autos it OWNS.
///
/// RED before the filter: both sub-problems receive ALL cells. Folding `total`
/// in the `{c}` component reads unbound `a`/`b` and writes `Undef`; folding
/// `side` in the `{a, b}` component reads unbound `c` and does the same. It is
/// also the O(#components × |dependent_cells| × NM_iterations × multistart_K)
/// cost the per-trial fold pays re-evaluating cells the component cannot move —
/// the efficiency half of this task's finding.
#[test]
fn each_component_subproblem_receives_only_its_own_dependent_cells() {
    let fx = two_component_dependent_cell_problem();
    let captured = capture_subproblems(&fx.problem);

    assert_eq!(
        captured.len(),
        2,
        "the fixture must decompose into exactly 2 components even AFTER the \
         obj_refs expansion ({{a, b}} joined by the objective's read of \
         `total`; {{c}} on its own); got {} sub-problem(s) with autos {:?}",
        captured.len(),
        captured.iter().map(auto_id_set).collect::<Vec<_>>()
    );

    let original = dependent_cell_ids(&fx.problem);
    let find = |want: &[&ValueCellId]| -> &ResolutionProblem {
        let want: std::collections::HashSet<ValueCellId> =
            want.iter().map(|id| (*id).clone()).collect();
        captured
            .iter()
            .find(|p| auto_id_set(p) == want)
            .unwrap_or_else(|| {
                panic!(
                    "no captured sub-problem owns exactly the autos {want:?}; \
                     captured auto sets were {:?}",
                    captured.iter().map(auto_id_set).collect::<Vec<_>>()
                )
            })
    };

    for (autos, want_cells, why) in [
        (
            vec![&fx.a, &fx.b],
            // Stored order — `mid` precedes `total`, which reads it.
            vec![fx.mid.clone(), fx.total.clone()],
            "`side = SIDE_COEFF*c` reads `c`, which this component does not own — \
             folding it here reads an unbound `c` and writes `Undef`. \
             `mix = mid + side` reads `{a, c}`: it OVERLAPS this component's \
             autos without being a SUBSET of them, so it must be dropped here \
             too — a filter testing mere intersection instead of containment \
             would keep it and fold the unowned `c`",
        ),
        (
            vec![&fx.c],
            vec![fx.side.clone()],
            "`mid = A_COEFF*a` and `total = mid + B_COEFF*b` read `a` and `b`, \
             which this component does not own — folding either here reads \
             unbound autos and writes `Undef`. `mix` reads `{a, c}` and so \
             overlaps without being contained, and must be dropped here as well",
        ),
    ] {
        let sub = find(&autos);
        let got = dependent_cell_ids(sub);
        // Compared as an ORDERED Vec, not a set: the retained cells must come
        // back in stored order, so a filter that rebuilt the list from a set or
        // a `HashMap` iteration fails here rather than passing on membership.
        assert_eq!(
            got, want_cells,
            "the sub-problem owning autos {:?} must carry exactly the dependent \
             cells {want_cells:?}, IN THAT ORDER; got {got:?}. Pre-fix BOTH \
             sub-problems receive ALL cells, because `solve_inner` builds each \
             from `..problem.clone()` and never filters the field. {why}.",
            autos.iter().map(|i| (*i).clone()).collect::<Vec<_>>()
        );
        assert!(
            !got.contains(&fx.mix),
            "`mix` transitively reads `{{a, c}}`, which NO component owns in \
             full, so it must be dropped from EVERY sub-problem — it is the \
             only cell in this fixture that distinguishes the `is_subset` \
             predicate from mere overlap (`!is_disjoint` / `intersects`). Got \
             {got:?} for the component owning {:?}.",
            autos.iter().map(|i| (*i).clone()).collect::<Vec<_>>()
        );
        assert!(
            is_subsequence(&got, &original),
            "the filtered list {got:?} must be a SUBSEQUENCE of the stored \
             order {original:?} — filtering must preserve the topological order \
             `build_dependent_cells` produced (PRD §6.3: the stored order is the \
             single authority, consumed here and never re-derived). A reorder \
             means the filter rebuilt the list from a set or a `HashMap` \
             iteration instead of retaining in place."
        );
    }
}

/// The single-component shape of [`lexicographic_joint_drive_problem`], with a
/// single-term objective so the whole cluster reaches ONE `solver.solve()` call.
///
/// The ONE constraint `a + b >= A_PLUS_B_FLOOR` joins both autos on the
/// constraint graph alone, so the component is single regardless of the
/// `obj_refs` expansion.
fn single_component_joint_drive_problem() -> (ResolutionProblem, ValueCellId, ValueCellId) {
    const A_PLUS_B_FLOOR: f64 = 4.0;

    let a = ValueCellId::new("Part", "a");
    let b = ValueCellId::new("Part", "b");
    let mid = ValueCellId::new("Part", "mid");
    let total = ValueCellId::new("Part", "total");

    let mut current_values = ValueMap::new();
    current_values.insert(mid.clone(), scalar(STALE_TOTAL));
    current_values.insert(total.clone(), scalar(STALE_TOTAL));

    let auto = |id: &ValueCellId| AutoParam {
        id: id.clone(),
        param_type: dimensionless(),
        bounds: Some((LO, HI)),
        free: true,
    };

    let problem = ResolutionProblem {
        auto_params: vec![auto(&a), auto(&b)],
        constraints: vec![(
            ConstraintNodeId::new("Part", 0),
            CompiledExpr::binop(
                BinOp::Ge,
                CompiledExpr::binop(BinOp::Add, vref(&a), vref(&b), dimensionless()),
                lit(A_PLUS_B_FLOOR),
                Type::Bool,
            ),
        )],
        current_values,
        objective: Some(ObjectiveSet::single(ObjectiveSense::Minimize, vref(&total))),
        functions: Arc::from(Vec::new()),
        // TWO cells in a known stored order, `mid` before the `total` that
        // reads it. A single-cell list would make the "FULL list in the SAME
        // order" assertion below unfalsifiable — any ordering of a 1-element
        // Vec is correct — so the identity claim needs at least two.
        dependent_cells: vec![
            (
                mid.clone(),
                CompiledExpr::binop(BinOp::Mul, lit(A_COEFF), vref(&a), dimensionless()),
            ),
            (
                total.clone(),
                CompiledExpr::binop(BinOp::Add, vref(&mid), vref(&b), dimensionless()),
            ),
        ],
    };

    (problem, mid, total)
}

/// I1 non-regression — the filter must be the IDENTITY on the single-component
/// path.
///
/// This is the guard against an OVER-aggressive filter. Every pre-existing
/// single-component solve stays byte-identical only because every cell's
/// transitive auto-read set is trivially a subset of the one component's autos
/// (PRD §6.2 / I1). A shortfall here means the subset predicate is measuring
/// the wrong thing — e.g. testing against the autos a component's CONSTRAINTS
/// mention rather than the autos it OWNS, or demanding set equality rather than
/// containment.
#[test]
fn single_component_subproblem_still_receives_the_full_dependent_cells_list() {
    let (problem, mid, total) = single_component_joint_drive_problem();
    let captured = capture_subproblems(&problem);

    assert_eq!(
        captured.len(),
        1,
        "`a + b >= FLOOR` joins both autos on the constraint graph alone, so \
         there is exactly ONE component; got {} sub-problem(s) with autos {:?}",
        captured.len(),
        captured.iter().map(auto_id_set).collect::<Vec<_>>()
    );

    let sub = &captured[0];
    assert_eq!(
        auto_id_set(sub),
        std::collections::HashSet::from([
            ValueCellId::new("Part", "a"),
            ValueCellId::new("Part", "b"),
        ]),
        "the single component must own BOTH autos"
    );
    assert_eq!(
        dependent_cell_ids(sub),
        dependent_cell_ids(&problem),
        "the filter is REQUIRED to be the identity whenever every auto lives in \
         one component: `mid` reads `a` and `total` reads `a` (through `mid`) \
         and `b`, all owned here, so every transitive auto-read set is \
         trivially a subset. That identity is the only thing keeping every \
         pre-existing single-component solve byte-identical (PRD §6.2 / I1) — \
         dropping a cell here silently un-folds an objective the pre-#5720 code \
         folded correctly. Compared as an ORDERED Vec, so a filter that rebuilt \
         the list from a set or a `HashMap` iteration fails here too. Expected \
         the FULL list in the SAME order, {:?}; got {:?}.",
        dependent_cell_ids(&problem),
        dependent_cell_ids(sub)
    );
    assert_eq!(
        dependent_cell_ids(sub),
        vec![mid, total],
        "fixture integrity: the list must hold at least TWO cells in a known \
         order, or the identity-and-order claim above is vacuous"
    );
}

/// The objective must attach to the component owning the autos it reaches ONLY
/// through a dependent cell.
///
/// Pins the expansion → union → `objective_component` chain that the hardcoded
/// `0` fallthrough in `SolverRegistry::solve_inner` used to short-circuit. Pre-expansion,
/// `obj_refs` held no auto ids at all — the objective is a bare read of `total`
/// — so no component matched, the lookup fell through to component `0` of a
/// NONDETERMINISTIC `HashMap` iteration, and the objective could land on `{c}`,
/// leaving `a` and `b` solved feasibility-only against stale seeds. A silently
/// suboptimal answer on exactly the production path β exists to enable.
///
/// Identified by auto SET, never by capture index, for the same reason.
#[test]
fn objective_reaching_an_auto_only_through_a_dependent_cell_attaches_to_that_autos_component() {
    let fx = two_component_dependent_cell_problem();
    let captured = capture_subproblems(&fx.problem);

    let with_objective: Vec<&ResolutionProblem> = captured
        .iter()
        .filter(|p| p.objective.is_some())
        .collect();

    assert_eq!(
        with_objective.len(),
        1,
        "exactly one component may carry the objective; got {} of {} \
         sub-problem(s) (auto sets: {:?})",
        with_objective.len(),
        captured.len(),
        captured.iter().map(auto_id_set).collect::<Vec<_>>()
    );

    assert_eq!(
        auto_id_set(with_objective[0]),
        std::collections::HashSet::from([fx.a.clone(), fx.b.clone()]),
        "the objective reads `total = A_COEFF*a + B_COEFF*b` and NO auto \
         directly, so it must attach to the component owning {{a, b}} — the \
         autos it reaches through that cell. It landed on the component owning \
         {:?} instead. Without the `obj_refs` expansion, \
         `decompose_into_components` unions nothing, `a` and `b` never share a \
         component with each other, and `objective_component` falls through to \
         the hardcoded `0` — handing the objective to whichever component \
         `decompose_into_components`' component-map iteration happened to \
         yield first, and \
         leaving the unattached autos solved feasibility-only against their \
         stale seeds.",
        auto_id_set(with_objective[0])
    );

    // The objective-bearing component must also still be able to SCORE: every
    // cell the objective transitively reads has to survive the step-4 filter
    // there, or `build_scoring_values` folds an incomplete map.
    assert!(
        dependent_cell_ids(with_objective[0]).contains(&fx.total),
        "the objective-bearing component must retain `total` — the cell the \
         objective actually reads. This is the invariant that makes the \
         per-component filter SAFE: the expansion guarantees every auto the \
         objective transitively drives lands in ONE component, so every cell it \
         reads passes the subset test there. Got {:?}.",
        dependent_cell_ids(with_objective[0])
    );
    assert!(
        !dependent_cell_ids(with_objective[0]).contains(&fx.side),
        "…and must NOT retain `side`, which reads the auto `c` it does not own; \
         got {:?}",
        dependent_cell_ids(with_objective[0])
    );
    // `c`'s component is the one that must never see the objective.
    assert_ne!(
        auto_id_set(with_objective[0]),
        std::collections::HashSet::from([fx.c.clone()]),
        "the objective must never attach to the component owning only `c`, \
         which the objective does not reach at all"
    );
}
