//! Solver-level tests for the `cost_robustness_tradeoff` normalised two-anchor
//! blend (task #4791 γ).
//!
//! PRD `docs/prds/v0_6/continuous-cost-minimisation.md` §2.4/§8.1: the
//! `minimize cost_robustness_tradeoff(<money-expr>, λ)` special form REPLACES
//! the α robustness floor (#4789) with a normalised convex blend of two anchor
//! solves — a pure-cost anchor and a pure-robustness (Chebyshev-centre) anchor.
//! This file verifies the §8.1 structural invariants over a 1-D cost-monotonic
//! problem with two-sided inequalities (`1mm < t < 4mm`):
//!
//!   - λ=1 ⇒ pure-cost, floor-free minimisation → the TRUE constraint boundary
//!     (1mm), not the α-floor-held standoff.
//!   - λ=0 ⇒ identical argmax to [`build_centrality_objective`]'s Chebyshev
//!     centre (2.5mm) — the blend at λ=0 is a positive-affine transform of
//!     `min_slack`, so both share the exact same argmax.
//!   - λ=0.5 ⇒ strictly between the λ=1 and λ=0 anchors (monotone betweenness,
//!     valid on this 1-D cost-monotonic problem).
//!
//! Step-05 tests (RED until step-06 impl lands): today the `cost_robustness_lambda`
//! marker on `ObjectiveSet` is ignored by the solver — `cost_robustness_tradeoff`
//! solves as an ordinary Money-dimensioned `Minimize` objective for every λ, so
//! the α floor (or its seed-fallback path) applies uniformly and all three λ
//! values converge on the same point, failing every assertion below.

use reify_constraints::{DimensionalSolver, build_centrality_objective};
use reify_core::{DimensionVector, Type, ValueCellId};
use reify_ir::{
    AutoParam, BinOp, CompiledExpr, ConstraintSolver, ObjectiveSet, ResolutionProblem,
    SolveResult, Value, ValueMap,
};

/// Absolute tolerance (metres) for anchor-convergence checks below. The
/// *targets* being compared are computed structurally (an independent
/// centrality solve; a strict `<` betweenness check) per PRD §8.1 — this
/// constant is only the unavoidable float-equality epsilon for the position
/// comparisons, not a guessed target value. Tight enough to clearly separate
/// "true boundary" (λ=1) from the α-robustness-floor standoff (≥ 20 µm for
/// this problem's 1mm lower bound; `REL_MARGIN = 0.02` in solver.rs), while
/// comfortably above Nelder-Mead's actual numerical precision on this smooth,
/// single-parameter, cost-monotonic problem.
const ANCHOR_TOL_M: f64 = 1e-5;

// ── helpers (mirrors crates/reify-constraints/tests/robustness_floor.rs) ──

/// Returns `5 USD × (x / 1mm)` — Money-dimensioned, monotonically increasing
/// in `x`, so minimizing it pushes toward the smallest feasible `x`.
fn money_expr_x_per_mm(x_id: &ValueCellId) -> CompiledExpr {
    let money_dim = DimensionVector::MONEY;
    let length_dim = DimensionVector::LENGTH;
    let dimensionless = DimensionVector::DIMENSIONLESS;

    let five_usd = CompiledExpr::literal(
        Value::Scalar {
            si_value: 5.0,
            dimension: money_dim,
        },
        Type::Scalar { dimension: money_dim },
    );
    let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::Scalar { dimension: length_dim });
    let one_mm = CompiledExpr::literal(
        Value::Scalar {
            si_value: 0.001,
            dimension: length_dim,
        },
        Type::Scalar { dimension: length_dim },
    );
    let x_per_mm = CompiledExpr::binop(
        BinOp::Div,
        x_ref,
        one_mm,
        Type::Scalar { dimension: dimensionless },
    );
    CompiledExpr::binop(
        BinOp::Mul,
        five_usd,
        x_per_mm,
        Type::Scalar { dimension: money_dim },
    )
}

/// Builds `x_id > bound_si_m` as a `CompiledExpr`.
fn gt_expr(x_id: &ValueCellId, bound_si_m: f64) -> CompiledExpr {
    let length_dim = DimensionVector::LENGTH;
    let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::Scalar { dimension: length_dim });
    let bound = CompiledExpr::literal(
        Value::Scalar {
            si_value: bound_si_m,
            dimension: length_dim,
        },
        Type::Scalar { dimension: length_dim },
    );
    CompiledExpr::binop(BinOp::Gt, x_ref, bound, Type::Bool)
}

/// Builds `x_id < bound_si_m` as a `CompiledExpr`.
fn lt_expr(x_id: &ValueCellId, bound_si_m: f64) -> CompiledExpr {
    let length_dim = DimensionVector::LENGTH;
    let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::Scalar { dimension: length_dim });
    let bound = CompiledExpr::literal(
        Value::Scalar {
            si_value: bound_si_m,
            dimension: length_dim,
        },
        Type::Scalar { dimension: length_dim },
    );
    CompiledExpr::binop(BinOp::Lt, x_ref, bound, Type::Bool)
}

fn constraint_id(entity: &str, index: u32) -> reify_core::ConstraintNodeId {
    reify_core::ConstraintNodeId::new(entity, index)
}

/// Shared two-sided-inequality problem skeleton: `1mm < t < 4mm` (Chebyshev
/// centre = 2.5mm), with `AutoParam` bounds `[1mm, 5mm]` deliberately wider /
/// asymmetric relative to the constraint interval. The bounds-midpoint seed
/// (3mm) therefore coincides with NEITHER the λ=1 target (1mm) NOR the λ=0
/// target (2.5mm) — a passing assertion can only come from the solver
/// actually reaching the target, never from a seed/target coincidence.
fn base_problem(t_id: &ValueCellId, objective: Option<ObjectiveSet>) -> ResolutionProblem {
    ResolutionProblem {
        auto_params: vec![AutoParam {
            id: t_id.clone(),
            param_type: Type::Scalar { dimension: DimensionVector::LENGTH },
            bounds: Some((0.001, 0.005)),
            free: true,
        }],
        constraints: vec![
            (constraint_id("CostRobustnessTradeoff", 0), gt_expr(t_id, 0.001)),
            (constraint_id("CostRobustnessTradeoff", 1), lt_expr(t_id, 0.004)),
        ],
        current_values: ValueMap::new(),
        objective,
        functions: vec![].into(),
    }
}

/// Solves `problem` and extracts `t_id`'s resolved SI value, panicking on any
/// non-`Solved` result (Infeasible/NoProgress are not expected for this
/// well-posed 1-D problem at any point in this file's lifecycle).
fn solve_t(problem: &ResolutionProblem, t_id: &ValueCellId) -> f64 {
    match DimensionalSolver.solve(problem) {
        SolveResult::Solved { values, .. } => values
            .get(t_id)
            .and_then(|v| v.as_f64())
            .expect("solved value for t missing or non-numeric"),
        other => panic!("expected Solved, got {:?}", other),
    }
}

/// λ=1 ≡ pure-cost, floor-free minimisation → the TRUE constraint boundary
/// (1mm), not the α-robustness-floor-held standoff (task #4789).
///
/// RED today: the `cost_robustness_lambda` marker is ignored, so this solves
/// as a plain Money-dimensioned `Minimize` objective — the α floor applies
/// (or its seed-fallback path returns the unmoved 3mm seed); either way `t`
/// lands nowhere near the true 1mm boundary.
#[test]
fn lambda_one_reaches_true_boundary_floor_free() {
    let t_id = ValueCellId::new("CostRobustnessTradeoff", "t");
    let objective = ObjectiveSet::cost_robustness_tradeoff(money_expr_x_per_mm(&t_id), 1.0);
    let problem = base_problem(&t_id, Some(objective));

    let t_si = solve_t(&problem, &t_id);

    assert!(
        (t_si - 0.001).abs() < ANCHOR_TOL_M,
        "λ=1 should reach the TRUE constraint boundary (1mm, floor-free), got t = {:.6e} m",
        t_si,
    );
}

/// λ=0 ≡ [`build_centrality_objective`]'s argmax (Chebyshev centre of
/// `[1mm, 4mm]` = 2.5mm): the blend at λ=0 is a positive-affine transform of
/// `min_slack`, so it shares the exact same argmax as the plain centrality
/// objective — independent of anchor-solve tolerance (PRD §8.1).
///
/// RED today: the marker is ignored, so t(λ=0) solves the SAME Money-minimize
/// (with floor) as λ=1, nowhere near the independently-computed 2.5mm centre.
#[test]
fn lambda_zero_matches_centrality_reference() {
    let t_id = ValueCellId::new("CostRobustnessTradeoff", "t");
    let objective = ObjectiveSet::cost_robustness_tradeoff(money_expr_x_per_mm(&t_id), 0.0);
    let problem = base_problem(&t_id, Some(objective));

    let t_lambda0 = solve_t(&problem, &t_id);

    // Independent reference: solve the SAME auto param / constraints with the
    // plain Maximize(min_slack) centrality objective — no tradeoff marker at all.
    let centrality_obj = build_centrality_objective(&problem.auto_params, &problem.constraints)
        .expect(
        "two-sided inequalities on a Scalar auto param must synthesise a centrality objective",
    );
    let mut centrality_problem = problem.clone();
    centrality_problem.objective = Some(centrality_obj);
    let t_centrality = solve_t(&centrality_problem, &t_id);

    assert!(
        (t_lambda0 - t_centrality).abs() < ANCHOR_TOL_M,
        "λ=0 should match the independent centrality-objective solve (blend \
         argmax at λ=0 ≡ Chebyshev centre); t(λ=0) = {:.6e} m, centrality \
         reference = {:.6e} m",
        t_lambda0,
        t_centrality,
    );
}

/// λ=0.5 lies strictly between the λ=1 (pure-cost) and λ=0 (pure-robustness)
/// anchors — monotone betweenness on this 1-D cost-monotonic problem (PRD §8.1).
///
/// RED today: with the marker ignored, all three λ values solve the identical
/// Money-minimize-with-floor problem, so t(λ=1) == t(λ=0.5) == t(λ=0) and the
/// strict betweenness fails.
#[test]
fn lambda_half_strictly_between_anchors() {
    let t_id = ValueCellId::new("CostRobustnessTradeoff", "t");

    let obj_1 = ObjectiveSet::cost_robustness_tradeoff(money_expr_x_per_mm(&t_id), 1.0);
    let t_lambda1 = solve_t(&base_problem(&t_id, Some(obj_1)), &t_id);

    let obj_half = ObjectiveSet::cost_robustness_tradeoff(money_expr_x_per_mm(&t_id), 0.5);
    let t_lambda_half = solve_t(&base_problem(&t_id, Some(obj_half)), &t_id);

    let obj_0 = ObjectiveSet::cost_robustness_tradeoff(money_expr_x_per_mm(&t_id), 0.0);
    let t_lambda0 = solve_t(&base_problem(&t_id, Some(obj_0)), &t_id);

    assert!(
        t_lambda1 < t_lambda_half && t_lambda_half < t_lambda0,
        "λ=0.5 should lie strictly between the λ=1 and λ=0 anchors: \
         t(λ=1)={:.6e}, t(λ=0.5)={:.6e}, t(λ=0)={:.6e}",
        t_lambda1,
        t_lambda_half,
        t_lambda0,
    );
}
