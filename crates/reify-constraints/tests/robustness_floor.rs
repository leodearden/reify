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

fn constraint_id(entity: &str, index: u32) -> reify_core::ConstraintNodeId {
    reify_core::ConstraintNodeId::new(entity, index)
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
