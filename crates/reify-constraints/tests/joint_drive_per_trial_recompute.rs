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
    AutoParam, BinOp, CompiledExpr, ConstraintSolver, ObjectiveSense, ObjectiveSet,
    RankedSolveResult, ResolutionProblem, SolveResult, Value, ValueMap,
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
// BT-10 — `cost_robustness_tradeoff` is a THIRD, unconverted scoring site
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

/// BT-10 (a), PRIMARY — a cost-dominant λ and a robustness-dominant λ must
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

/// BT-10 (b), SECONDARY — pin the DIRECTION of the blend, so a future
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
