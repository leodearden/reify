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
use reify_core::{DiagnosticCode, DimensionVector, Type, ValueCellId};
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
        dependent_cells: Vec::new(),
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

// ── γ + STRICT auto (task #5711 amendment 2) ──────────────────────────────
//
// COVERAGE GAP, verified before writing these: γ + a STRICT auto had ZERO
// coverage anywhere in the workspace. `examples/cost_robustness_tradeoff.ri`,
// `examples/continuous_cost_min.ri`,
// `crates/reify-eval/tests/cost_robustness_tradeoff_example_e2e.rs` and this
// file's own `base_problem` ALL set `free: true`, which skips
// `finalise_uniqueness` — and therefore `verify_uniqueness` — entirely; a grep
// over `crates/reify-eval` finds no eval-level γ + strict-auto test either. The
// only tracked γ + strict-auto artifact is
// `tests/prd-gate/fixtures/cost_robustness_tradeoff_form.ri`, and nothing
// asserts on its eval output (it is pinned only in verify.sh's grammar-corpus
// list and as a historical pre-γ capability-manifest row). The two tests below
// close that gap at the solver level, for BOTH interval shapes.

/// `base_problem`'s shape with the two differences that matter for
/// `verify_uniqueness`: `free: false` (so `finalise_uniqueness` actually runs
/// the uniqueness check) and `bounds: None` (the PRODUCTION shape — no `.ri`
/// surface ever sets `AutoParam.bounds`; `engine_eval.rs` always emits `None`,
/// which is exactly why the pre-#5711 `effective_bounds` anchor sat at 9m for a
/// mm-scale part).
///
/// `upper` supplies the optional `t < upper` constraint: `Some(0.004)` gives the
/// two-sided `1mm < t < 4mm` bracket, `None` the one-sided `t > 1mm` shape.
fn strict_problem(t_id: &ValueCellId, lambda: f64, upper: Option<f64>) -> ResolutionProblem {
    let mut constraints = vec![(constraint_id("CostRobustnessTradeoff", 0), gt_expr(t_id, 0.001))];
    if let Some(hi) = upper {
        constraints.push((constraint_id("CostRobustnessTradeoff", 1), lt_expr(t_id, hi)));
    }
    ResolutionProblem {
        dependent_cells: Vec::new(),
        auto_params: vec![AutoParam {
            id: t_id.clone(),
            param_type: Type::Scalar { dimension: DimensionVector::LENGTH },
            bounds: None,
            free: false,
        }],
        constraints,
        current_values: ValueMap::new(),
        objective: Some(ObjectiveSet::cost_robustness_tradeoff(
            money_expr_x_per_mm(t_id),
            lambda,
        )),
        functions: vec![].into(),
    }
}

/// A STRICT auto bracketed on BOTH sides by the user's own constraints
/// (`1mm < t < 4mm`) must solve `unique: true` under γ, for EVERY λ.
///
/// Asserted across λ ∈ {0.0, 0.5, 1.0} rather than one value: MEASURED, all
/// three regress identically, and λ=1 regressing is what identifies the
/// mechanism. At λ=1 the blend is a positive-affine transform of cost alone, so
/// "λ<1 pulls the blend off the min-cost point" cannot explain it; the real
/// cause is that `solve_cost_robustness_tradeoff` is SEED-DEPENDENT by
/// construction (all three of its solves share one deterministic seed for
/// reproducibility, never seed-invariance, and a floor-free cost-minimise whose
/// optimum sits infinitesimally past the boundary hits
/// `solve_core_with_sd_tolerance`'s drift-fallback and returns THE SEED). A
/// perturbation-based uniqueness check therefore compares f(seed_A) against
/// f(seed_B) for a seed-dependent f — structurally inapplicable on this path.
///
/// RED today: all three λ return `Infeasible` carrying
/// `ConstraintNonUnique` ("strict auto parameter resolution is not uniquely
/// determined"). On main all three return `Solved { unique: true }` with
/// t = 2.5mm.
///
/// The assertion pins the Solved/unique/in-bracket CONTRACT rather than 2.5mm
/// exactly: the precise point is a blend/seed artifact, not a PRD invariant.
#[test]
fn gamma_strict_auto_two_sided_bracket_is_solved() {
    let t_id = ValueCellId::new("CostRobustnessTradeoff", "t");

    for lambda in [0.0_f64, 0.5, 1.0] {
        let problem = strict_problem(&t_id, lambda, Some(0.004));
        match DimensionalSolver.solve(&problem) {
            SolveResult::Solved { values, unique } => {
                assert!(
                    unique,
                    "λ={lambda}: a both-sides-bracketed strict auto is well-determined under \
                     §11.6 test (2) — the blend's argmin over the user's own interval — so it \
                     must report unique: true"
                );
                let t_si = values
                    .get(&t_id)
                    .and_then(|v| v.as_f64())
                    .expect("solved value for t missing or non-numeric");
                assert!(
                    (0.001..=0.004).contains(&t_si),
                    "λ={lambda}: t must land inside the 1mm..4mm bracket, got {t_si:.6e} m"
                );
            }
            SolveResult::Infeasible { diagnostics } => {
                panic!(
                    "λ={lambda}: expected Solved for a both-sides-bracketed strict auto under γ \
                     (main returns Solved{{unique:true, t=2.5mm}}); the perturbation re-solve is \
                     structurally inapplicable to a seed-dependent dispatch and must not \
                     manufacture a non-uniqueness verdict. diagnostics: {diagnostics:?}"
                );
            }
            other => panic!("λ={lambda}: expected Solved, got {other:?}"),
        }
    }
}

/// The SAME γ + strict shape with the upper bound REMOVED (`t > 1mm` only,
/// mirroring `tests/prd-gate/fixtures/cost_robustness_tradeoff_form.ri`) must
/// KEEP reporting `ConstraintNonUnique`.
///
/// This test is GREEN today and must STAY green — its already-green status is
/// DELIBERATE, not accidental. It is the guard that stops a future maintainer
/// "simplifying" `strict_autos_constraint_bracketed` into a blanket
/// `return true` for γ: that was MEASURED on the prd-gate fixture above to
/// convert an existing loud `error: strict auto parameter resolution is not
/// uniquely determined` into a silent `thickness = 10 m` — 10 m being
/// `default_bounds_for(Length)`'s ceiling, i.e. a value pinned by a
/// SOLVER-INTERNAL default the user never authored, which is precisely the
/// non-determinedness §11.6 exists to catch.
#[test]
fn gamma_strict_auto_one_sided_stays_non_unique() {
    let t_id = ValueCellId::new("CostRobustnessTradeoff", "t");
    let problem = strict_problem(&t_id, 0.5, None);

    match DimensionalSolver.solve(&problem) {
        SolveResult::Infeasible { diagnostics } => {
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.code == Some(DiagnosticCode::ConstraintNonUnique)),
                "a one-sided strict auto's upper side comes from default_bounds_for, not the \
                 user's model, so the resolved value is default-bounds-determined and must \
                 stay ConstraintNonUnique; got: {diagnostics:?}"
            );
        }
        other => panic!(
            "expected Infeasible{{ConstraintNonUnique}} for a ONE-SIDED strict auto under γ \
             (byte-identical to main); got {other:?}. If this is Solved, the bracketed \
             predicate has been weakened into a blanket γ abstention."
        ),
    }
}

// ── γ + a bound the DERIVATION cannot read (task #5711, esc-5711-3) ───────
//
// `strict_autos_constraint_bracketed` reads its evidence out of
// `derive_param_intervals`, which recognises only three syntactic shapes
// (`p OP c`, `p - k OP c`, `k - p OP c`) on `Ge`/`Gt`/`Le`/`Lt` with a
// CONSTANT, auto-free far operand. Every other legitimate way to bound a
// param — `Eq` (skipped outright), a coefficient (`2*t > 3mm`), a nonlinear
// form, or a COUPLED bound naming another auto (`y < 5mm - x`) — derives to
// `None`, and a `None` read as "the user did not bound this side" turns a
// perfectly bounded γ model into a user-facing `error: strict auto parameter
// resolution is not uniquely determined`.
//
// A derivation BLIND SPOT must never masquerade as positive evidence of
// under-determinedness. The three fixtures below pin the ABSTAIN rule: when a
// strict param's missing side is attributable to a constraint the derivation
// could not read, `verify_uniqueness` falls back to "cannot prove non-unique"
// (returns `true`) instead of manufacturing a `ConstraintNonUnique`. `false`
// stays reserved for params the derivation POSITIVELY confirms are
// constraint-unbounded on a side — which is exactly
// `gamma_strict_auto_one_sided_stays_non_unique` above, still green.

/// Builds `x_id == bound_si_m` as a `CompiledExpr`.
fn eq_expr(x_id: &ValueCellId, bound_si_m: f64) -> CompiledExpr {
    let length_dim = DimensionVector::LENGTH;
    let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::Scalar { dimension: length_dim });
    let bound = CompiledExpr::literal(
        Value::Scalar {
            si_value: bound_si_m,
            dimension: length_dim,
        },
        Type::Scalar { dimension: length_dim },
    );
    CompiledExpr::binop(BinOp::Eq, x_ref, bound, Type::Bool)
}

/// Builds `k * x_id > bound_si_m` — a COEFFICIENT form, outside the three
/// shapes `derive_from_side` recognises.
fn scaled_gt_expr(x_id: &ValueCellId, k: f64, bound_si_m: f64) -> CompiledExpr {
    let length_dim = DimensionVector::LENGTH;
    let dimensionless = DimensionVector::DIMENSIONLESS;
    let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::Scalar { dimension: length_dim });
    let k_lit = CompiledExpr::literal(
        Value::Scalar {
            si_value: k,
            dimension: dimensionless,
        },
        Type::Scalar { dimension: dimensionless },
    );
    let scaled = CompiledExpr::binop(
        BinOp::Mul,
        k_lit,
        x_ref,
        Type::Scalar { dimension: length_dim },
    );
    let bound = CompiledExpr::literal(
        Value::Scalar {
            si_value: bound_si_m,
            dimension: length_dim,
        },
        Type::Scalar { dimension: length_dim },
    );
    CompiledExpr::binop(BinOp::Gt, scaled, bound, Type::Bool)
}

/// Builds `y_id < total_si_m - x_id` — a COUPLED bound: `y`'s upper side is
/// supplied by the user's model, but names another auto, so
/// `constant_operand_value` rejects the far operand and no bound is derived
/// for EITHER param.
fn coupled_lt_expr(y_id: &ValueCellId, total_si_m: f64, x_id: &ValueCellId) -> CompiledExpr {
    let length_dim = DimensionVector::LENGTH;
    let y_ref = CompiledExpr::value_ref(y_id.clone(), Type::Scalar { dimension: length_dim });
    let x_ref = CompiledExpr::value_ref(x_id.clone(), Type::Scalar { dimension: length_dim });
    let total = CompiledExpr::literal(
        Value::Scalar {
            si_value: total_si_m,
            dimension: length_dim,
        },
        Type::Scalar { dimension: length_dim },
    );
    let rhs = CompiledExpr::binop(
        BinOp::Sub,
        total,
        x_ref,
        Type::Scalar { dimension: length_dim },
    );
    CompiledExpr::binop(BinOp::Lt, y_ref, rhs, Type::Bool)
}

/// A STRICT γ problem over `auto_ids`, with caller-supplied constraints.
fn strict_problem_with(
    auto_ids: &[ValueCellId],
    cost_id: &ValueCellId,
    lambda: f64,
    constraints: Vec<CompiledExpr>,
) -> ResolutionProblem {
    ResolutionProblem {
        dependent_cells: Vec::new(),
        auto_params: auto_ids
            .iter()
            .map(|id| AutoParam {
                id: id.clone(),
                param_type: Type::Scalar { dimension: DimensionVector::LENGTH },
                bounds: None,
                free: false,
            })
            .collect(),
        constraints: constraints
            .into_iter()
            .enumerate()
            .map(|(i, e)| (constraint_id("CostRobustnessTradeoff", i as u32), e))
            .collect(),
        current_values: ValueMap::new(),
        objective: Some(ObjectiveSet::cost_robustness_tradeoff(
            money_expr_x_per_mm(cost_id),
            lambda,
        )),
        functions: vec![].into(),
    }
}

/// Asserts the solve did NOT report `ConstraintNonUnique`.
fn assert_not_non_unique(problem: &ResolutionProblem, what: &str) {
    if let SolveResult::Infeasible { diagnostics } = DimensionalSolver.solve(problem) {
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.code == Some(DiagnosticCode::ConstraintNonUnique)),
            "{what}: the derivation could not READ this bound, which is not evidence the user \
             failed to write one — the γ predicate must abstain (cannot prove non-unique) \
             rather than report ConstraintNonUnique. diagnostics: {diagnostics:?}"
        );
    }
}

/// `constraint t == 2mm` — the canonical DSL way to determine a strict auto
/// (`examples/auto_binding_sites.ri`) — is skipped outright by
/// `derive_from_expr`'s op rule, so BOTH derived sides are `None`. That must
/// abstain, not error.
#[test]
fn gamma_strict_auto_eq_determined_is_not_non_unique() {
    let t_id = ValueCellId::new("CostRobustnessTradeoff", "t");
    let problem = strict_problem_with(
        std::slice::from_ref(&t_id),
        &t_id,
        0.5,
        vec![eq_expr(&t_id, 0.002)],
    );
    assert_not_non_unique(&problem, "Eq-determined strict auto");
}

/// A COEFFICIENT lower bound (`2*t > 3mm`, i.e. `t > 1.5mm`) paired with a
/// readable upper bound (`t < 4mm`). The interval is bounded on both sides by
/// the user's own model; only the derivation cannot see the lower one.
#[test]
fn gamma_strict_auto_coefficient_bound_is_not_non_unique() {
    let t_id = ValueCellId::new("CostRobustnessTradeoff", "t");
    let problem = strict_problem_with(
        std::slice::from_ref(&t_id),
        &t_id,
        0.5,
        vec![scaled_gt_expr(&t_id, 2.0, 0.003), lt_expr(&t_id, 0.004)],
    );
    assert_not_non_unique(&problem, "coefficient-bounded strict auto");
}

/// The reviewer's measured case [B]: `1mm < x < 4mm ∧ y > 1mm ∧ y < 5mm - x`.
/// The region is bounded and well-determined (x > 1mm ⇒ y < 4mm), and the
/// plain-`Minimize` path accepts the identical constraints — but `y`'s upper
/// bound names another auto, so `derive_from_side` yields `None` for it.
#[test]
fn gamma_strict_autos_coupled_bound_is_not_non_unique() {
    let x_id = ValueCellId::new("CostRobustnessTradeoff", "x");
    let y_id = ValueCellId::new("CostRobustnessTradeoff", "y");
    let problem = strict_problem_with(
        &[x_id.clone(), y_id.clone()],
        &x_id,
        0.5,
        vec![
            gt_expr(&x_id, 0.001),
            lt_expr(&x_id, 0.004),
            gt_expr(&y_id, 0.001),
            coupled_lt_expr(&y_id, 0.005, &x_id),
        ],
    );
    assert_not_non_unique(&problem, "coupled multi-param bound");
}

/// The KNOWN, ACCEPTED gap in `strict_autos_constraint_bracketed`: a γ blend
/// that is FLAT with respect to a bracketed strict auto still reports
/// `unique: true`.
///
/// `u` is bracketed by the user's own model (`1mm < u < 4mm`) but appears in NO
/// cost term — the money expression references `t` only — and at λ=1 the blend
/// is a positive-affine transform of cost alone, so the objective is genuinely
/// constant in `u` over its whole interval. Its argmin is therefore a SET, not
/// a point: §11.6 test (2) is not satisfied for `u`, yet the γ predicate — which
/// answers that test from the CONSTRAINTS alone, never evaluating the objective
/// — reports well-determined.
///
/// The non-γ path gives the OPPOSITE verdict for the analogous shape
/// (`solver.rs`'s `flat_objective_over_inequality_bracket_reports_non_unique`,
/// via `classify_uniqueness`'s tie arm). That divergence is accepted, not
/// overlooked — see `strict_autos_constraint_bracketed`'s "Known, ACCEPTED gap"
/// section for the reasoning (the widening is monotone: γ reported
/// `ConstraintNonUnique` for EVERY strict auto before #5711 amendment 2, so no
/// previously-`Solved` model changes verdict).
///
/// This test exists to PIN that as measured behaviour rather than leave it
/// inferred. It is a characterisation test: if a future change teaches the γ
/// path to consult the objective, this assertion is the one that must be
/// re-decided deliberately — flipping it is a §11.6 policy change for γ, not an
/// incidental regression. `t`'s resolved value is asserted too, so a flip that
/// merely broke the solve is distinguishable from a deliberate policy change.
#[test]
fn gamma_flat_blend_over_bracket_is_accepted_as_unique() {
    let t_id = ValueCellId::new("CostRobustnessTradeoff", "t");
    let u_id = ValueCellId::new("CostRobustnessTradeoff", "u");
    let problem = strict_problem_with(
        &[t_id.clone(), u_id.clone()],
        &t_id, // cost = 5 USD × (t / 1mm) — `u` appears nowhere in it
        1.0,   // λ=1 ⇒ pure cost ⇒ the blend is exactly flat in `u`
        vec![
            gt_expr(&t_id, 0.001),
            lt_expr(&t_id, 0.004),
            gt_expr(&u_id, 0.001),
            lt_expr(&u_id, 0.004),
        ],
    );

    match DimensionalSolver.solve(&problem) {
        SolveResult::Solved { values, unique } => {
            assert!(
                unique,
                "ACCEPTED GAP: the γ predicate decides §11.6 test (2) from constraint \
                 bracketing alone, so a blend that is flat in `u` still reports unique. \
                 If this flipped deliberately, update `strict_autos_constraint_bracketed`'s \
                 \"Known, ACCEPTED gap\" section rather than just this assertion"
            );
            let u_si = values
                .get(&u_id)
                .and_then(|v| v.as_f64())
                .expect("solved value for u missing or non-numeric");
            assert!(
                (0.001..=0.004).contains(&u_si),
                "u must still land inside its own 1mm..4mm bracket, got {u_si:.6e} m"
            );
        }
        other => panic!(
            "expected Solved for a both-sides-bracketed pair under γ (the flat-in-`u` blend \
             is the accepted gap, not an error path); got {other:?}"
        ),
    }
}

/// The ACCEPTED RISK of the esc-5711-3 abstention, pinned in the direction
/// that actually LOSES safety — the half `gamma_flat_blend_over_bracket_is_
/// accepted_as_unique` does not reach (review suggestion 2).
///
/// `params_in_underivable_constraints` is deliberately general: a strict auto
/// mentioned by ANY constraint the derivation cannot read abstains, including
/// one whose missing side really is `default_bounds_for`'s. The unit tests
/// pin the set-building half and `strict_autos_constraint_bracketed_abstains_
/// for_underivable_param` pins the predicate, but nothing pinned the COMPOSED
/// verdict for a model that is genuinely unbounded on a side AND carries one
/// unreadable conjunct. This is that model.
///
/// The two arms are the SAME model up to one extra constraint, so the contrast
/// is the whole point:
///
/// - CONTROL (`t > 1mm` alone) — the shape of
///   `gamma_strict_auto_one_sided_stays_non_unique` and of
///   `tests/prd-gate/fixtures/cost_robustness_tradeoff_form.ri`: t's upper side
///   comes from `default_bounds_for`, so every λ errors `ConstraintNonUnique`.
/// - ABSTENTION DOOR (`t > 1mm ∧ 2*t > 1mm`) — the added conjunct is REDUNDANT
///   (it restates `t > 0.5mm`, already implied) and changes the feasible region
///   not at all, but it is a COEFFICIENT form, so the derivation cannot read
///   it, `t` lands in the abstention set, and the missing upper side stops
///   counting as evidence. Every λ now reports `Solved { unique: true }`.
///
/// MEASURED at the same commit as this test, and this is the safety loss:
/// λ=0 resolves `t = 10.0 m` — literally `default_bounds_for(Length)`'s ceiling,
/// a value pinned by a solver-internal default the user never authored, for a
/// mm-scale part. λ=0.5 and λ=1 resolve `t = 1.1 mm` (cost pulls to the lower
/// bound). That is the SAME regression class `gamma_strict_auto_one_sided_
/// stays_non_unique` exists to block — a loud error becoming a silent 10 m —
/// reached through the abstention door rather than through a blanket
/// `return true`.
///
/// It is accepted rather than fixed because the alternative direction of error
/// is worse: reading a blind spot as evidence was measured to REJECT valid,
/// bounded models (the three `..._is_not_non_unique` fixtures above). Narrowing
/// it means teaching `derive_from_expr` the missing shapes — coefficient forms
/// first, which would close this exact fixture — not tightening the abstention
/// test. Tracked as task #6465 (γ quality: seed-invariance, or a precise
/// diagnostic for default-bounds-determined γ models). Do not re-file.
///
/// A CHARACTERISATION test: it asserts today's behaviour, not desired
/// behaviour. If a future change makes this error again, that is progress —
/// re-decide it deliberately here and in `params_in_underivable_constraints`'
/// "ACCEPTED CONSEQUENCE" note, rather than discovering it as a surprise.
#[test]
fn gamma_one_sided_plus_unreadable_conjunct_abstains_to_solved() {
    let t_id = ValueCellId::new("CostRobustnessTradeoff", "t");

    for lambda in [0.0_f64, 0.5, 1.0] {
        // CONTROL: `t > 1mm` alone — no abstention evidence, so the missing
        // upper side is read as default-bounds-determined.
        let control = strict_problem_with(
            std::slice::from_ref(&t_id),
            &t_id,
            lambda,
            vec![gt_expr(&t_id, 0.001)],
        );
        match DimensionalSolver.solve(&control) {
            SolveResult::Infeasible { diagnostics } => assert!(
                diagnostics
                    .iter()
                    .any(|d| d.code == Some(DiagnosticCode::ConstraintNonUnique)),
                "λ={lambda}: control must stay ConstraintNonUnique; got {diagnostics:?}"
            ),
            other => panic!(
                "λ={lambda}: the control arm is the same shape as \
                 `gamma_strict_auto_one_sided_stays_non_unique` and must error; got {other:?}"
            ),
        }

        // ABSTENTION DOOR: `2*t > 1mm` adds NO feasible-region information
        // (it restates `t > 0.5mm`), only unreadability.
        let with_blind_spot = strict_problem_with(
            std::slice::from_ref(&t_id),
            &t_id,
            lambda,
            vec![gt_expr(&t_id, 0.001), scaled_gt_expr(&t_id, 2.0, 0.001)],
        );
        match DimensionalSolver.solve(&with_blind_spot) {
            SolveResult::Solved { values, unique } => {
                assert!(
                    unique,
                    "λ={lambda}: ACCEPTED RISK — one unreadable conjunct mentioning `t` makes \
                     its genuinely-unbounded upper side abstain, so a model that errors \
                     without that conjunct reports unique instead"
                );
                let t_si = values
                    .get(&t_id)
                    .and_then(|v| v.as_f64())
                    .expect("solved value for t missing or non-numeric");
                if lambda == 0.0 {
                    // Pinned as a MAGNITUDE, not the exact 10.0 measured, so a
                    // blend/seed retune cannot red this on a technicality — the
                    // fact being pinned is "orders of magnitude outside the
                    // user's mm design scale", not the precise artifact value.
                    assert!(
                        t_si >= 1.0,
                        "λ=0: the accepted loss is that `t` resolves to the \
                         `default_bounds_for(Length)` ceiling (measured: 10.0 m) rather than \
                         erroring. Got {t_si:.6e} m — if this is now mm-scale the abstention \
                         no longer reaches the default box and this test needs re-deciding"
                    );
                } else {
                    assert!(
                        t_si > 0.001,
                        "λ={lambda}: cost pulls `t` to its lower bound (measured: 1.1mm); it \
                         must still satisfy `t > 1mm`, got {t_si:.6e} m"
                    );
                }
            }
            other => panic!(
                "λ={lambda}: expected Solved via the abstention door (measured: \
                 unique=true, t=10.0 m at λ=0 and t=1.1mm otherwise); got {other:?}"
            ),
        }
    }
}
