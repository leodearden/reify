//! Solver-level tests for the robustness floor (task #4789).
//!
//! Verifies that `DimensionalSolver` synthesises a margin floor on each
//! inequality slack when the objective is Money-dimensioned, parking auto
//! values OFF the constraint boundary instead of on it.
//!
//! Step-1 tests (RED until step-2 impl lands):
//!   - `money_objective_floor_holds_value_off_boundary`
//!   - `non_money_objective_unchanged`
//!
//! Step-3 tests (RED until step-4 impl lands):
//!   - `floor_infeasible_emits_distinct_diagnostic`
//!   - `non_money_infeasible_keeps_constraint_unsatisfiable`
//!
//! Task #5618 section (constraint-derived bounds) at the bottom of the file:
//! a Money-objective auto bracketed away from 0 must not report a false
//! `RobustnessFloorInfeasible`, and a genuinely floor-empty bracket must still
//! report a real one.
//!
//! Task #5618 step-7 sub-section (RED until step-8 impl lands) pins solution
//! QUALITY and the two remaining seed consumers:
//!   - `bracketed_money_auto_resolves_at_floored_argmin`
//!   - `bracketed_money_strict_auto_survives_uniqueness_resolve`
//!   - `bracketed_money_multistart_cluster_ranks_a_feasible_candidate`
//!
//! Task #5618 step-9 sub-section (RED until step-10 impl lands) pins DIAGNOSTIC
//! HONESTY for the shapes steps 2-8 cannot rescue:
//!   - `margin_only_infeasibility_names_the_margin_not_an_empty_region`
//!   - `genuinely_unsatisfiable_constraints_keep_the_region_empty_wording`

use reify_constraints::DimensionalSolver;
use reify_core::{DiagnosticCode, DimensionVector, Type, ValueCellId};
use reify_ir::{
    AutoParam, BinOp, CompiledExpr, ConstraintSolver, ObjectiveSense,
    ObjectiveSet, ResolutionProblem, SolveResult, Value, ValueMap,
};

// ── helper: build a Money-dimensioned expression = `unit_cost_per_mm * x` ──
//
// Returns the expression `5 USD × (x / 1mm)`, which has result_type Scalar<MONEY>.
// This is Minimized, making it monotonically increasing in x — so the optimal
// unconstrained point is x=0, and the constraint `x > 1mm` forces the boundary.
fn money_expr_x_per_mm(x_id: &ValueCellId) -> CompiledExpr {
    let money_dim = DimensionVector::MONEY;
    let length_dim = DimensionVector::LENGTH;
    let dimensionless = DimensionVector::DIMENSIONLESS;

    // 5 USD literal
    let five_usd = CompiledExpr::literal(
        Value::Scalar {
            si_value: 5.0,
            dimension: money_dim,
        },
        Type::Scalar {
            dimension: money_dim,
        },
    );

    // x reference (Length)
    let x_ref = CompiledExpr::value_ref(
        x_id.clone(),
        Type::Scalar {
            dimension: length_dim,
        },
    );

    // 1mm literal = 0.001 m
    let one_mm = CompiledExpr::literal(
        Value::Scalar {
            si_value: 0.001,
            dimension: length_dim,
        },
        Type::Scalar {
            dimension: length_dim,
        },
    );

    // x / 1mm  → dimensionless
    let x_per_mm = CompiledExpr::binop(
        BinOp::Div,
        x_ref,
        one_mm,
        Type::Scalar {
            dimension: dimensionless,
        },
    );

    // 5 USD × (x / 1mm) → Money
    CompiledExpr::binop(
        BinOp::Mul,
        five_usd,
        x_per_mm,
        Type::Scalar { dimension: money_dim },
    )
}

// ── helper: build `x > bound_si_m` as a CompiledExpr ──
fn gt_expr(x_id: &ValueCellId, bound_si_m: f64) -> CompiledExpr {
    let length_dim = DimensionVector::LENGTH;
    let x_ref = CompiledExpr::value_ref(
        x_id.clone(),
        Type::Scalar {
            dimension: length_dim,
        },
    );
    let bound = CompiledExpr::literal(
        Value::Scalar {
            si_value: bound_si_m,
            dimension: length_dim,
        },
        Type::Scalar {
            dimension: length_dim,
        },
    );
    CompiledExpr::binop(BinOp::Gt, x_ref, bound, Type::Bool)
}

// ── helper: build `x < bound_si_m` as a CompiledExpr ──
fn lt_expr(x_id: &ValueCellId, bound_si_m: f64) -> CompiledExpr {
    let length_dim = DimensionVector::LENGTH;
    let x_ref = CompiledExpr::value_ref(
        x_id.clone(),
        Type::Scalar {
            dimension: length_dim,
        },
    );
    let bound = CompiledExpr::literal(
        Value::Scalar {
            si_value: bound_si_m,
            dimension: length_dim,
        },
        Type::Scalar {
            dimension: length_dim,
        },
    );
    CompiledExpr::binop(BinOp::Lt, x_ref, bound, Type::Bool)
}

fn length_auto_param(id: ValueCellId) -> AutoParam {
    AutoParam {
        id,
        param_type: Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        bounds: None,
        free: false,
    }
}

/// `length_auto_param` with `free: true` — skips the uniqueness re-solve, so a test
/// can pin floor/bounds behaviour without also pinning determinism.
fn length_auto_param_free(id: ValueCellId) -> AutoParam {
    AutoParam {
        free: true,
        ..length_auto_param(id)
    }
}

fn constraint_id(entity: &str, index: u32) -> reify_core::ConstraintNodeId {
    reify_core::ConstraintNodeId::new(entity, index)
}

// ── helper: `x OP bound_si_m` for any comparison op (Length) ──
//
// Sibling of `gt_expr` / `lt_expr` above, for the non-strict `>=` / `<=` shapes
// the task #5618 tests need.
fn length_cmp(op: BinOp, x_id: &ValueCellId, bound_si_m: f64) -> CompiledExpr {
    let length_dim = DimensionVector::LENGTH;
    CompiledExpr::binop(
        op,
        CompiledExpr::value_ref(x_id.clone(), Type::Scalar { dimension: length_dim }),
        CompiledExpr::literal(
            Value::Scalar { si_value: bound_si_m, dimension: length_dim },
            Type::Scalar { dimension: length_dim },
        ),
        Type::Bool,
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Dimensionless (`Real`) siblings of the Length helpers above — task #5618.
//
// `money_expr_x_per_mm` and `length_auto_param` are Length-specific; the headline
// #5618 defect was reproduced on a *dimensionless* auto, whose `default_bounds_for`
// box is `(-1e6, 1e6)` rather than Length's `(1µm, 10m)`.
// ─────────────────────────────────────────────────────────────────────────────

fn real_auto_param(id: ValueCellId, free: bool) -> AutoParam {
    AutoParam {
        id,
        param_type: Type::Scalar { dimension: DimensionVector::DIMENSIONLESS },
        bounds: None,
        free,
    }
}

// ── helper: `q OP bound` for any comparison op (dimensionless) ──
fn real_cmp(op: BinOp, q_id: &ValueCellId, bound: f64) -> CompiledExpr {
    let dimensionless = DimensionVector::DIMENSIONLESS;
    CompiledExpr::binop(
        op,
        CompiledExpr::value_ref(q_id.clone(), Type::Scalar { dimension: dimensionless }),
        CompiledExpr::literal(
            Value::Scalar { si_value: bound, dimension: dimensionless },
            Type::Scalar { dimension: dimensionless },
        ),
        Type::Bool,
    )
}

// ── helper: Money-dimensioned `1 USD × q` for a dimensionless `q` ──
//
// Monotonically increasing in `q`, so under `Minimize` the constrained optimum is
// the lower bound of the FLOORED feasible window.
fn money_times_real(q_id: &ValueCellId) -> CompiledExpr {
    let money_dim = DimensionVector::MONEY;
    let dimensionless = DimensionVector::DIMENSIONLESS;
    CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(
            Value::Scalar { si_value: 1.0, dimension: money_dim },
            Type::Scalar { dimension: money_dim },
        ),
        CompiledExpr::value_ref(q_id.clone(), Type::Scalar { dimension: dimensionless }),
        Type::Scalar { dimension: money_dim },
    )
}

// ── helper: Money-dimensioned `1 USD × (q0 + q1)` over two dimensionless autos ──
//
// Monotonically increasing in BOTH params, so under `Minimize` the constrained
// optimum is each param's own floored lower bound. Used by the multistart case
// (`solve_ranked` is only multistart-eligible at dim >= 2 with an objective).
fn money_times_real_sum(q_ids: &[ValueCellId]) -> CompiledExpr {
    let money_dim = DimensionVector::MONEY;
    let dimensionless = DimensionVector::DIMENSIONLESS;
    let sum = q_ids
        .iter()
        .map(|id| {
            CompiledExpr::value_ref(id.clone(), Type::Scalar { dimension: dimensionless })
        })
        .reduce(|acc, next| {
            CompiledExpr::binop(
                BinOp::Add,
                acc,
                next,
                Type::Scalar { dimension: dimensionless },
            )
        })
        .expect("money_times_real_sum needs at least one param");
    CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::literal(
            Value::Scalar { si_value: 1.0, dimension: money_dim },
            Type::Scalar { dimension: money_dim },
        ),
        sum,
        Type::Scalar { dimension: money_dim },
    )
}

/// Builds the #5618 headline problem: one auto param bracketed away from 0 by an
/// inequality pair, under a Money `Minimize` objective, with NO current value and
/// NO explicit `AutoParam.bounds` (the production shape).
fn bracketed_money_problem(
    auto_params: Vec<AutoParam>,
    constraints: Vec<CompiledExpr>,
    objective: CompiledExpr,
) -> ResolutionProblem {
    ResolutionProblem {
        dependent_cells: Vec::new(),
        auto_params,
        constraints: constraints
            .into_iter()
            .enumerate()
            .map(|(i, e)| (constraint_id("Bracketed", i as u32), e))
            .collect(),
        current_values: ValueMap::new(),
        objective: Some(ObjectiveSet::single(ObjectiveSense::Minimize, objective)),
        functions: vec![].into(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// step-1 tests (RED until step-2)
// ─────────────────────────────────────────────────────────────────────────────

/// When a Money-dimensioned Minimize objective is present, the solver adds
/// a robustness floor: `slack(x > 1mm) = x - 1mm ≥ margin` where
/// `margin = REL_MARGIN * 1mm = 0.02 * 0.001 = 0.00002 m = 20 µm`.
///
/// ## Mechanism (penalty-based fallback path)
///
/// The floor constraint is `Ge(x − 1mm, 0.02mm)` (x ≥ 1.02mm).  The
/// penalty-based Nelder-Mead optimiser is dominated by the Money objective
/// (`5 USD × x/1mm`) over the floor penalty — at x = 1mm the money saving
/// over x = 1.02mm is 0.10 USD while the floor penalty is only
/// `PENALTY_WEIGHT × (0.02mm)² ≈ 4×10⁻⁴` — so it converges toward x ≈ 1mm.
/// That final solution violates the floor (`residual ≈ 0.02mm >> FEASIBILITY_THRESHOLD`).
///
/// Because the **seed** (midpoint of `[1mm, 1.5mm]` = 1.25mm) IS initially
/// feasible under the floor (1.25mm − 1mm = 0.25mm ≥ 0.02mm), the fallback
/// path triggers: `initially_feasible = true`, optimizer drifts infeasible
/// (`final_max_residual > FEASIBILITY_THRESHOLD`), solver falls back to the seed
/// (1.25mm).
///
/// The **diagnostic invariant** is that:
/// - Without floor: optimizer parks at x = 1mm (Gt residual ≈ 0 ≤ FEASIBILITY_THRESHOLD),
///   `x > 0.001` fails.
/// - With floor: floor makes x = 1mm infeasible; fallback returns seed 1.25mm, well
///   within `(0.001, 0.00130]`, `x > 0.001` passes.
///
/// ## Upper-bound choice
///
/// `x < 0.00130` (1.3mm): covers the seed fallback value (1.25mm = 0.00125m) with a
/// 0.05mm margin, while being substantially tighter than the old 1.5mm ceiling.
/// A genuine floor-convergence test (requiring the optimizer to find 1.02mm exactly)
/// is not achievable with this penalty weight and money coefficient combination
/// (money savings dominate the floor penalty at this scale); the floor-convergence
/// property is separately verified in the eval-level test via the initially-infeasible
/// floor diagnostic.
///
/// Uses `free: true` to bypass the uniqueness check (floor behaviour is the
/// concern, not determinism). Explicit bounds `[1mm, 1.5mm]` place the seed
/// at 1.25mm (initially feasible under the floor).
#[test]
fn money_objective_floor_holds_value_off_boundary() {
    let x_id = ValueCellId::new("CostMinFloor", "x");

    // x > 1mm
    let constraint = gt_expr(&x_id, 0.001);

    // Money objective: minimize 5 USD × (x / 1mm), monotone ↑ in x
    let objective = ObjectiveSet::single(
        ObjectiveSense::Minimize,
        money_expr_x_per_mm(&x_id),
    );

    let problem = ResolutionProblem {
        dependent_cells: Vec::new(),
        auto_params: vec![AutoParam {
            id: x_id.clone(),
            param_type: Type::Scalar { dimension: DimensionVector::LENGTH },
            // Bounds [1mm, 1.5mm]: seed = midpoint = 1.25mm (initially feasible under floor).
            // Without floor: optimizer converges to x=1mm (feasible, Gt residual≈0) → on boundary.
            // With floor:    x=1mm infeasible (floor residual=0.02mm) → fallback to seed 1.25mm.
            bounds: Some((0.001, 0.0015)),
            free: true,
        }],
        constraints: vec![(constraint_id("CostMinFloor", 0), constraint)],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = DimensionalSolver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            let x_si = values.get(&x_id).unwrap().as_f64().unwrap();
            // Must be strictly OFF the 1mm boundary (> 0.001).
            // Without floor: optimizer parks at x=1mm (fails this assertion).
            // With floor: fallback to seed 1.25mm (passes this assertion).
            assert!(
                x_si > 0.001,
                "expected x > 1mm (boundary), got x = {:.6e} m",
                x_si
            );
            // Must be near the floor region (< 1.3mm = 0.00130m), not at an arbitrary
            // far-from-boundary value.  1.25mm (seed fallback) < 1.30mm ✓.
            // This is tighter than the explicit bounds ceiling (1.5mm) and excludes
            // seeds that would accidentally pass without the floor having any effect.
            assert!(
                x_si < 0.00130,
                "expected x near floor region (< 1.3mm), got x = {:.6e} m; \
                 seed-fallback value should be 1.25mm (midpoint of [1mm, 1.5mm])",
                x_si
            );
        }
        other => panic!(
            "expected Solved with floor-held value, got {:?}",
            other
        ),
    }
}

/// Non-Money objective: no floor synthesised.
///
/// Two-param problem: minimize 0.7*a - 0.3*b with a<50mm, b>1mm.
/// Optimizer chases the minimum (small a, large b).
/// Without a floor (non-Money objective), the solution parks AT the constraint
/// boundaries: a ≈ 0mm (unconstrained lower end) and b ≈ default.
///
/// Key assertion: NO floor is synthesised — the solution is unchanged from today
/// (invariant ii). We check that a is near zero (not forced off boundary by a
/// floor from the b>1mm constraint) and that the result is Solved.
#[test]
fn non_money_objective_unchanged() {
    let a_id = ValueCellId::new("NonMoneyObjTest", "a");
    let b_id = ValueCellId::new("NonMoneyObjTest", "b");
    let length_dim = DimensionVector::LENGTH;

    // a reference
    let a_ref = CompiledExpr::value_ref(a_id.clone(), Type::Scalar { dimension: length_dim });
    // b reference
    let b_ref = CompiledExpr::value_ref(b_id.clone(), Type::Scalar { dimension: length_dim });

    // constraint: a < 50mm
    let a_bound = CompiledExpr::literal(
        Value::Scalar { si_value: 0.050, dimension: length_dim },
        Type::Scalar { dimension: length_dim },
    );
    let a_lt = CompiledExpr::binop(BinOp::Lt, a_ref.clone(), a_bound, Type::Bool);

    // constraint: b > 1mm
    let b_bound = CompiledExpr::literal(
        Value::Scalar { si_value: 0.001, dimension: length_dim },
        Type::Scalar { dimension: length_dim },
    );
    let b_gt = CompiledExpr::binop(BinOp::Gt, b_ref.clone(), b_bound, Type::Bool);

    // Length objective: minimize 0.7*a - 0.3*b  (NOT Money)
    let point7 = CompiledExpr::literal(
        Value::Scalar { si_value: 0.7, dimension: DimensionVector::DIMENSIONLESS },
        Type::Scalar { dimension: DimensionVector::DIMENSIONLESS },
    );
    let point3 = CompiledExpr::literal(
        Value::Scalar { si_value: 0.3, dimension: DimensionVector::DIMENSIONLESS },
        Type::Scalar { dimension: DimensionVector::DIMENSIONLESS },
    );
    let term_a = CompiledExpr::binop(BinOp::Mul, point7, a_ref, Type::Scalar { dimension: length_dim });
    let term_b = CompiledExpr::binop(BinOp::Mul, point3, b_ref, Type::Scalar { dimension: length_dim });
    let obj_expr = CompiledExpr::binop(BinOp::Sub, term_a, term_b, Type::Scalar { dimension: length_dim });
    let objective = ObjectiveSet::single(ObjectiveSense::Minimize, obj_expr);

    let problem = ResolutionProblem {
        dependent_cells: Vec::new(),
        auto_params: vec![
            AutoParam { id: a_id.clone(), param_type: Type::Scalar { dimension: length_dim }, bounds: Some((0.0, 0.1)), free: false },
            AutoParam { id: b_id.clone(), param_type: Type::Scalar { dimension: length_dim }, bounds: Some((0.0, 0.1)), free: false },
        ],
        constraints: vec![
            (constraint_id("NonMoneyObjTest", 0), a_lt),
            (constraint_id("NonMoneyObjTest", 1), b_gt),
        ],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = DimensionalSolver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            let a_si = values.get(&a_id).unwrap().as_f64().unwrap();
            let b_si = values.get(&b_id).unwrap().as_f64().unwrap();
            // a should be close to zero (optimizer pushes small), well below 50mm
            assert!(
                a_si < 0.005,
                "non-money: expected a near 0, got a = {:.4e} m (unexpected floor?)",
                a_si
            );
            // b should be pushed by the optimizer toward larger b (−0.3*b term in objective)
            // but must be above 1mm
            assert!(
                b_si > 0.001,
                "non-money: b must stay above 1mm constraint, got b = {:.4e} m",
                b_si
            );
            // b should be pushed significantly above its lower bound (1mm) by the
            // optimizer (−0.3*b term drives b toward its upper bound 100mm).
            // Threshold 0.010 (10mm) is deliberately loose: it is 10× the lower
            // bound (1mm) and well below the expected optimizer corner (~90mm), so
            // it confirms "no spurious floor effect on b" without coupling the test
            // to Nelder-Mead convergence precision.  A non-Money-objective floor
            // would have to be ≥ 2% of the 1mm bound = 0.02mm ≪ 10mm to matter,
            // so b > 10mm suffices to prove no non-Money floor was synthesised.
            assert!(
                b_si > 0.010,
                "non-money: expected b pushed well above 1mm (no floor on b from length objective), \
                 got b = {:.4e} m",
                b_si
            );
        }
        other => panic!("expected Solved, got {:?}", other),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// step-3 tests (RED until step-4)
// ─────────────────────────────────────────────────────────────────────────────

/// Tight opposing box: x > 10mm AND x < 10.3mm (gap = 0.3mm).
/// With floor:
///   m_lower = 0.02 × 10mm  = 0.2mm → x must be ≥ 10.2mm
///   m_upper = 0.02 × 10.3mm = 0.206mm → x must be ≤ 10.094mm
/// 10.2mm > 10.094mm → floored region is empty → Infeasible.
/// Un-floored box [10mm, 10.3mm] is itself feasible.
///
/// The infeasible diagnostic must carry code `RobustnessFloorInfeasible`,
/// NOT `ConstraintUnsatisfiable`.
#[test]
fn floor_infeasible_emits_distinct_diagnostic() {
    let x_id = ValueCellId::new("FloorInfeasible", "x");

    // x > 10mm AND x < 10.3mm
    let gt = gt_expr(&x_id, 0.010);
    let lt = lt_expr(&x_id, 0.0103);

    // Money objective (required to activate the floor)
    let objective = ObjectiveSet::single(
        ObjectiveSense::Minimize,
        money_expr_x_per_mm(&x_id),
    );

    let problem = ResolutionProblem {
        dependent_cells: Vec::new(),
        auto_params: vec![length_auto_param(x_id.clone())],
        constraints: vec![
            (constraint_id("FloorInfeasible", 0), gt),
            (constraint_id("FloorInfeasible", 1), lt),
        ],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = DimensionalSolver.solve(&problem);
    match result {
        SolveResult::Infeasible { diagnostics } => {
            let found_floor_code = diagnostics.iter().any(|d| {
                d.code == Some(DiagnosticCode::RobustnessFloorInfeasible)
            });
            assert!(
                found_floor_code,
                "expected RobustnessFloorInfeasible diagnostic, got: {:?}",
                diagnostics
            );
        }
        other => panic!(
            "expected Infeasible (floor makes box infeasible), got {:?}",
            other
        ),
    }
}

/// Control: a genuinely infeasible non-Money problem (x > 5mm AND x < 1mm)
/// must still emit `ConstraintUnsatisfiable`, NOT `RobustnessFloorInfeasible`.
#[test]
fn non_money_infeasible_keeps_constraint_unsatisfiable() {
    let x_id = ValueCellId::new("NonMoneyInfeasible", "x");

    // x > 5mm AND x < 1mm (inherently infeasible, no floor)
    let gt = gt_expr(&x_id, 0.005);
    let lt = lt_expr(&x_id, 0.001);

    // No objective (no floor should be synthesised)
    let problem = ResolutionProblem {
        dependent_cells: Vec::new(),
        auto_params: vec![length_auto_param(x_id.clone())],
        constraints: vec![
            (constraint_id("NonMoneyInfeasible", 0), gt),
            (constraint_id("NonMoneyInfeasible", 1), lt),
        ],
        current_values: ValueMap::new(),
        objective: None,
        functions: vec![].into(),
    };

    let result = DimensionalSolver.solve(&problem);
    match result {
        SolveResult::Infeasible { diagnostics } => {
            let has_unsatisfiable = diagnostics.iter().any(|d| {
                d.code == Some(DiagnosticCode::ConstraintUnsatisfiable)
            });
            assert!(
                has_unsatisfiable,
                "expected ConstraintUnsatisfiable for non-money infeasible, got: {:?}",
                diagnostics
            );
            let has_floor_code = diagnostics.iter().any(|d| {
                d.code == Some(DiagnosticCode::RobustnessFloorInfeasible)
            });
            assert!(
                !has_floor_code,
                "must NOT emit RobustnessFloorInfeasible for non-money problem, got: {:?}",
                diagnostics
            );
        }
        other => panic!("expected Infeasible, got {:?}", other),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// task #5618 — a Money-objective auto bracketed away from 0 must not report a
// false RobustnessFloorInfeasible.
//
// Mechanism of the defect: `AutoParam.bounds` is always `None` in production, so
// `effective_bounds` degraded to `default_bounds_for` = `(-1e6, 1e6)` for a
// dimensionless Real.  `extract_initial_point` then fell all the way through to the
// fixed `0.01`, which is OUTSIDE the window `synthesise_floor_constraints` carves
// out of `q ∈ [1, 100]` (`q - 1 ≥ 0.02` ∧ `100 - q ≥ 2%·q_seed`), and `build_simplex`
// stepped by `(1e6 − −1e6)·0.1 = 2e5` off the same useless box.  Nelder-Mead
// reflected to ~−2e5 and contracted back toward 0.01 from the wrong side, never
// entering the feasible region.
//
// Steps 5-6 fix the false Infeasible; the exact-argmin assertion is in step-7.
// ─────────────────────────────────────────────────────────────────────────────

/// HEADLINE: a dimensionless `auto(free)` bracketed by `q >= 1 ∧ q <= 100` under a
/// Money `Minimize` objective must SOLVE, landing inside the floored window.
///
/// Before task #5618 this returned `Infeasible` / `RobustnessFloorInfeasible`
/// ("the floored feasible region is empty") even though `[1, 100]` is 99 units wide.
#[test]
fn bracketed_money_auto_solves_inside_floored_window() {
    let q_id = ValueCellId::new("Bracketed", "q");

    let problem = bracketed_money_problem(
        vec![real_auto_param(q_id.clone(), true)],
        vec![
            real_cmp(BinOp::Ge, &q_id, 1.0),
            real_cmp(BinOp::Le, &q_id, 100.0),
        ],
        money_times_real(&q_id),
    );

    match DimensionalSolver.solve(&problem) {
        SolveResult::Solved { values, .. } => {
            let q = values.get(&q_id).unwrap().as_f64().unwrap();
            // Floored window: lower slack `q − 1 ≥ 0.02·1 = 0.02` → q ≥ 1.02.
            // The upper floor is looser than 98.0 for every reachable seed.
            assert!(
                (1.02..=98.0).contains(&q),
                "expected q inside the floored window [1.02, 98.0], got q = {q}"
            );
        }
        other => panic!(
            "expected Solved for a Money objective over q ∈ [1, 100] — a 99-unit-wide \
             box whose floored window [1.02, ~99] is plainly non-empty; got {:?}",
            other
        ),
    }
}

/// Control: the `q >= 0.0` variant, which already solved before task #5618 (the
/// fixed `0.01` seed happens to sit inside its floored window), must keep solving.
#[test]
fn bracketed_money_auto_from_zero_still_solves() {
    let q_id = ValueCellId::new("Bracketed", "q");

    let problem = bracketed_money_problem(
        vec![real_auto_param(q_id.clone(), true)],
        vec![
            real_cmp(BinOp::Ge, &q_id, 0.0),
            real_cmp(BinOp::Le, &q_id, 100.0),
        ],
        money_times_real(&q_id),
    );

    match DimensionalSolver.solve(&problem) {
        SolveResult::Solved { values, .. } => {
            let q = values.get(&q_id).unwrap().as_f64().unwrap();
            // Floor margin degenerates to ABS_FLOOR_SI = 1e-9 when the bound is 0.
            assert!(
                q >= 1e-9,
                "expected q at or above the degenerate floor (ABS_FLOOR_SI = 1e-9), got {q}"
            );
            assert!(q <= 100.0, "expected q inside the upper bound, got {q}");
        }
        other => panic!("expected Solved for the q >= 0.0 control, got {:?}", other),
    }
}

/// No false negative: a bracket whose FLOORED window is genuinely empty must still
/// report `Infeasible` with `RobustnessFloorInfeasible`.
///
/// `q ∈ [99, 100]` is only 1 unit wide, but the 2% margins are ~1.98 and ~1.99 —
/// the floored bounds invert (lo ≈ 100.98 > hi ≈ 98.01), so `resolve_bounds`'
/// empty-box guard discards the derived box wholesale and behaviour is unchanged.
#[test]
fn floor_empty_bracket_still_infeasible() {
    let q_id = ValueCellId::new("Bracketed", "q");

    let problem = bracketed_money_problem(
        vec![real_auto_param(q_id.clone(), true)],
        vec![
            real_cmp(BinOp::Ge, &q_id, 99.0),
            real_cmp(BinOp::Le, &q_id, 100.0),
        ],
        money_times_real(&q_id),
    );

    match DimensionalSolver.solve(&problem) {
        SolveResult::Infeasible { diagnostics } => {
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.code == Some(DiagnosticCode::RobustnessFloorInfeasible)),
                "expected RobustnessFloorInfeasible for a genuinely floor-empty bracket, \
                 got: {:?}",
                diagnostics
            );
        }
        other => panic!(
            "expected Infeasible for q ∈ [99, 100] (2% margins ~1.98 + ~1.99 do not fit \
             in a 1-unit box); the fix must not manufacture a false Solved; got {:?}",
            other
        ),
    }
}

/// The fix is not dimension-specific: the same shape on a Length auto
/// (`x >= 50mm ∧ x <= 200mm`) must also solve inside its floored window.
///
/// Length's `default_bounds_for` box is `(1µm, 10m)`, so the pre-#5618 `0.01` seed
/// was inside the DEFAULT box yet still outside this bracket's floored window
/// (x ≥ 51mm) — a different numeric path to the same false Infeasible.
#[test]
fn bracketed_money_length_auto_solves_inside_floored_window() {
    let x_id = ValueCellId::new("Bracketed", "x");

    let problem = bracketed_money_problem(
        vec![length_auto_param_free(x_id.clone())],
        vec![
            length_cmp(BinOp::Ge, &x_id, 0.050),
            length_cmp(BinOp::Le, &x_id, 0.200),
        ],
        money_expr_x_per_mm(&x_id),
    );

    match DimensionalSolver.solve(&problem) {
        SolveResult::Solved { values, .. } => {
            let x = values.get(&x_id).unwrap().as_f64().unwrap();
            // Lower slack `x − 50mm ≥ 0.02·50mm = 1mm` → x ≥ 51mm.
            assert!(
                (0.051..=0.1975).contains(&x),
                "expected x inside the floored window [51mm, 197.5mm], got x = {x} m"
            );
        }
        other => panic!(
            "expected Solved for a Money objective over x ∈ [50mm, 200mm]; got {:?}",
            other
        ),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// task #5618 step-7 — solution QUALITY, and the two remaining seed consumers
// (`build_perturbation_anchors` and `multistart_points`), which step-6 left on
// the useless `effective_bounds` default box.
//
// Steps 5-6 removed the false `Infeasible`.  These three pin that the answer is
// also GOOD: the exact argmin rather than the seed, a strict auto that survives
// the uniqueness re-solve rather than trading a false `RobustnessFloorInfeasible`
// for a false `ConstraintNonUnique`, and a dim>=2 cluster that reaches
// `solve_ranked`'s ranked arm rather than its `scored.is_empty()` fallback.
// ─────────────────────────────────────────────────────────────────────────────

/// ARGMIN SHARPNESS: the Money objective `1 USD × q` is strictly increasing in `q`,
/// so over `q ∈ [1, 100]` the constrained optimum is the FLOORED lower bound,
/// `q = 1.02` — not the derived seed.
///
/// This is the assertion that distinguishes the full fix from a seed-only fix.
/// The architect's runtime probe measured exactly two outcomes by injecting
/// candidate boxes into `AutoParam.bounds`: the raw constraint box `(1, 100)` gives
/// `Solved` at **q = 50.5** (its own midpoint seed, returned via the drift
/// fallback — 50× off the argmin), while the floored box `(1.02, 98)` gives
/// `Solved` at **q = 1.02**, the exact argmin.  A test that only asserted
/// `1.02 ≤ q ≤ 98` (as `bracketed_money_auto_solves_inside_floored_window` does)
/// accepts both; this one accepts only the second.
#[test]
fn bracketed_money_auto_resolves_at_floored_argmin() {
    let q_id = ValueCellId::new("Bracketed", "q");

    let problem = bracketed_money_problem(
        vec![real_auto_param(q_id.clone(), true)],
        vec![
            real_cmp(BinOp::Ge, &q_id, 1.0),
            real_cmp(BinOp::Le, &q_id, 100.0),
        ],
        money_times_real(&q_id),
    );

    match DimensionalSolver.solve(&problem) {
        SolveResult::Solved { values, .. } => {
            let q = values.get(&q_id).unwrap().as_f64().unwrap();
            assert!(
                (q - 1.02).abs() < 1e-3,
                "expected q at the floored argmin 1.02 (the constrained minimum of an \
                 increasing Money objective), got q = {q}; a value near the seed (e.g. \
                 50.5) means the derived box reached the SEED but not the CLAMP"
            );
        }
        other => panic!("expected Solved at the floored argmin, got {:?}", other),
    }
}

/// STRICT-AUTO UNIQUENESS: the headline problem with `free: false` must still
/// `Solved`, not degrade into `ConstraintNonUnique`.
///
/// A strict auto triggers `verify_uniqueness`, which re-solves from
/// `build_perturbation_anchors`' reflected anchor.  That anchor is
/// `lo + 0.9·(hi − lo)` of the param's box — on the un-derived dimensionless
/// default box `(-1e6, 1e6)` that is ~8×10⁵, nowhere near `[1, 100]`, so the
/// re-solve cannot reconverge and the disagreement is reported as
/// `ConstraintNonUnique`: one false error traded for another, leaving the
/// task's headline model still broken for every non-`free` auto.
#[test]
fn bracketed_money_strict_auto_survives_uniqueness_resolve() {
    let q_id = ValueCellId::new("Bracketed", "q");

    let problem = bracketed_money_problem(
        vec![real_auto_param(q_id.clone(), false)],
        vec![
            real_cmp(BinOp::Ge, &q_id, 1.0),
            real_cmp(BinOp::Le, &q_id, 100.0),
        ],
        money_times_real(&q_id),
    );

    match DimensionalSolver.solve(&problem) {
        SolveResult::Solved { values, .. } => {
            let q = values.get(&q_id).unwrap().as_f64().unwrap();
            assert!(
                (1.02..=98.0).contains(&q),
                "expected the strict auto inside the floored window [1.02, 98.0], got {q}"
            );
        }
        SolveResult::Infeasible { diagnostics } => {
            assert!(
                !diagnostics
                    .iter()
                    .any(|d| d.code == Some(DiagnosticCode::ConstraintNonUnique)),
                "the uniqueness re-solve must perturb from inside the DERIVED box, not \
                 from ~0.9·(2e6) off the dimensionless default box; got: {:?}",
                diagnostics
            );
            panic!("expected Solved for a strict bracketed auto, got Infeasible");
        }
        other => panic!("expected Solved for a strict bracketed auto, got {:?}", other),
    }
}

/// MULTISTART: a two-auto cluster, each param bracketed away from 0 by its own
/// `>=`/`<=` pair under one shared Money objective, is `multistart_eligible`
/// (dim >= 2 + objective + no `cost_robustness_lambda`) and so goes through
/// `solve_ranked`'s best-of-K loop instead of the single-start path.
///
/// `multistart_points` builds K = 2·(dim+1) = 6 starts: start #0 is
/// `extract_initial_point` (already derived, step-4), but starts #1..5 are the
/// all-midpoint point and the per-axis low/high corners, taken from the param's
/// box.  On the un-derived dimensionless default box those corners are ±10⁶ — none
/// of them can converge — so the loop's `scored` list would be carried entirely by
/// start #0, and any regression there drops straight through to the
/// `scored.is_empty()` Infeasible fallback.
#[test]
fn bracketed_money_multistart_cluster_ranks_a_feasible_candidate() {
    let q0_id = ValueCellId::new("Bracketed", "q0");
    let q1_id = ValueCellId::new("Bracketed", "q1");

    let problem = bracketed_money_problem(
        vec![
            real_auto_param(q0_id.clone(), true),
            real_auto_param(q1_id.clone(), true),
        ],
        vec![
            real_cmp(BinOp::Ge, &q0_id, 1.0),
            real_cmp(BinOp::Le, &q0_id, 100.0),
            real_cmp(BinOp::Ge, &q1_id, 2.0),
            real_cmp(BinOp::Le, &q1_id, 200.0),
        ],
        money_times_real_sum(&[q0_id.clone(), q1_id.clone()]),
    );

    match DimensionalSolver.solve_ranked(&problem) {
        reify_ir::RankedSolveResult::Ranked { candidates, .. } => {
            let best = candidates.first().expect("I2: candidates is non-empty");
            let q0 = best.values.get(&q0_id).unwrap().as_f64().unwrap();
            let q1 = best.values.get(&q1_id).unwrap().as_f64().unwrap();
            // Floored windows: q0 ≥ 1.02 (2% of 1), q1 ≥ 2.04 (2% of 2).
            assert!(
                (1.02..=98.0).contains(&q0),
                "expected q0 inside its floored window [1.02, 98.0], got {q0}"
            );
            assert!(
                (2.04..=196.0).contains(&q1),
                "expected q1 inside its floored window [2.04, 196.0], got {q1}"
            );
        }
        other => panic!(
            "expected a Ranked result for a 2-auto bracketed Money cluster — every start \
             must be able to reach the feasible box; got {:?}",
            other
        ),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// task #5618 step-9 — DIAGNOSTIC HONESTY (RED until step-10).
//
// Steps 2-8 stop the solver from *manufacturing* a floor-infeasible answer for a
// bracketed Money auto.  They cannot rescue every shape: a bracket genuinely too
// tight for the 2% margin is still `Infeasible`, and rightly so.  What must change
// is what the solver SAYS about it.
//
// Today the `floor_applied` arm asserts, unconditionally, that "the floored
// feasible region is empty".  For the tight-bracket shape that sentence actively
// misleads — the user's OWN constraint box is non-empty and the returned point sits
// inside it; only the synthesised robustness margin does not fit.  The original
// report's sharpest complaint was exactly this.
//
// The two cases must stay DISTINGUISHABLE: "your constraints do not admit a
// solution" and "your constraints do, but my margin does not" are different user
// actions.  The `DiagnosticCode` stays `RobustnessFloorInfeasible` in both — the
// eval-layer `RobustnessFloorApplied` suppression path keys off the code, and a new
// code would ripple into `reify-core/src/diagnostics.rs` for no user-visible gain.
// ─────────────────────────────────────────────────────────────────────────────

/// Extract the single `RobustnessFloorInfeasible` message from an `Infeasible` solve,
/// panicking with the full diagnostic list on any other shape.
fn floor_infeasible_message(problem: &ResolutionProblem) -> String {
    match DimensionalSolver.solve(problem) {
        SolveResult::Infeasible { diagnostics } => diagnostics
            .iter()
            .find(|d| d.code == Some(DiagnosticCode::RobustnessFloorInfeasible))
            .unwrap_or_else(|| {
                panic!("expected a RobustnessFloorInfeasible diagnostic, got: {diagnostics:?}")
            })
            .message
            .clone(),
        other => panic!("expected Infeasible, got {other:?}"),
    }
}

/// MARGIN-ONLY infeasibility must NOT claim the feasible region is empty.
///
/// `q ∈ [99, 100]` is a 1-unit-wide, perfectly satisfiable box.  Only the synthesised
/// margins (2% of 99 = 1.98 below, 2% of 100 = 2.0 above) invert it: q ≥ 100.98 ∧
/// q ≤ 98.  The returned point still satisfies the user's ORIGINAL constraints, so
/// the diagnostic must say so and name the robustness margin — not the constraints —
/// as what could not be met.
///
/// WHY THIS FIXTURE and not the `x > 10mm ∧ x < 10.3mm` Length bracket that already
/// sits in this file (`floor_infeasible_emits_distinct_diagnostic`), which reads like
/// the more obvious choice: measured, that shape does NOT satisfy its own original
/// constraints at the returned point.  Its objective `5 USD × (x / 1mm)` has gradient
/// 5000 per metre against `PENALTY_WEIGHT = 1e6`, so the penalty minimiser sits
/// ~1.25e-3 m BELOW the floored lower bound — i.e. outside the user's box — and
/// step-10's residual check correctly refuses to claim otherwise.  Here the objective
/// `1 USD × q` has gradient 1, the shift is ~2.5e-7, and the returned point stays
/// inside [99, 100].
///
/// That gap is real but is NOT this task's headline: making the honest branch reachable
/// for a steep objective means changing WHICH point a floor-infeasible solve reports,
/// which is solver semantics.  Filed as task #5714 (see also the step-10 note at the
/// emit site in `solver.rs`).  Do not "simplify" this test onto the Length fixture.
#[test]
fn margin_only_infeasibility_names_the_margin_not_an_empty_region() {
    let q_id = ValueCellId::new("Bracketed", "q");

    let problem = bracketed_money_problem(
        vec![real_auto_param(q_id.clone(), true)],
        vec![
            real_cmp(BinOp::Ge, &q_id, 99.0),
            real_cmp(BinOp::Le, &q_id, 100.0),
        ],
        money_times_real(&q_id),
    );

    let message = floor_infeasible_message(&problem);

    assert!(
        !message.contains("feasible region is empty"),
        "the user's box [99, 100] is 1 unit wide and the returned point is inside \
         it — the diagnostic must not claim the region is empty; got: {message}"
    );
    assert!(
        message.contains("original constraints"),
        "the diagnostic must say the ORIGINAL constraints are satisfied at the returned \
         point; got: {message}"
    );
    assert!(
        message.contains("robustness margin"),
        "the diagnostic must name the synthesised robustness margin as what cannot be \
         met; got: {message}"
    );
    assert!(
        message.contains("2%"),
        "the diagnostic must report the REL_MARGIN (2%) the user has to relax; \
         got: {message}"
    );
    assert!(
        message.contains("cost_robustness_tradeoff"),
        "the diagnostic must keep the cost_robustness_tradeoff override hint (PRD \
         §2.4/§9); got: {message}"
    );
}

/// CONTROL: constraints that are themselves unsatisfiable keep the region-empty
/// wording, so the two situations stay distinguishable.
///
/// `x >= 50mm ∧ x <= 10mm` admits no point at all, with or without the margin.
/// A Money objective still activates the floor (so this lands in the same
/// `floor_applied` arm as the test above), but here "the region is empty" is the
/// literal truth and must survive.
#[test]
fn genuinely_unsatisfiable_constraints_keep_the_region_empty_wording() {
    let x_id = ValueCellId::new("ReallyEmpty", "x");

    let problem = ResolutionProblem {
        dependent_cells: Vec::new(),
        auto_params: vec![length_auto_param(x_id.clone())],
        constraints: vec![
            (
                constraint_id("ReallyEmpty", 0),
                length_cmp(BinOp::Ge, &x_id, 0.050),
            ),
            (
                constraint_id("ReallyEmpty", 1),
                length_cmp(BinOp::Le, &x_id, 0.010),
            ),
        ],
        current_values: ValueMap::new(),
        objective: Some(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            money_expr_x_per_mm(&x_id),
        )),
        functions: vec![].into(),
    };

    let message = floor_infeasible_message(&problem);

    assert!(
        message.contains("the floored feasible region is empty"),
        "x >= 50mm ∧ x <= 10mm is unsatisfiable on its own terms — the region-empty \
         wording must survive so it stays distinguishable from a margin-only failure; \
         got: {message}"
    );
    assert!(
        !message.contains("original constraints ARE"),
        "must not claim the original constraints are satisfied when they are not; \
         got: {message}"
    );
}
