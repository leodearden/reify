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
    AutoParam, BinOp, CompiledExpr, CompiledFunction, ConstraintSolver, ObjectiveSense,
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
/// `margin = REL_MARGIN * 1mm = 0.02 * 0.001 = 0.00002 m`.
///
/// Result: x ≥ 1.02mm, strictly off the boundary.
/// Without floor: x parks ON the boundary (1mm) or fails with strict `>`.
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
        auto_params: vec![length_auto_param(x_id.clone())],
        constraints: vec![(constraint_id("CostMinFloor", 0), constraint)],
        current_values: ValueMap::new(),
        objective: Some(objective),
        functions: vec![].into(),
    };

    let result = DimensionalSolver.solve(&problem);
    match result {
        SolveResult::Solved { values, .. } => {
            let x_si = values.get(&x_id).unwrap().as_f64().unwrap();
            // Must be strictly OFF the 1mm boundary (> 0.001)
            assert!(
                x_si > 0.001,
                "expected x > 1mm (boundary), got x = {:.4e} m",
                x_si
            );
            // Must be close to the floor (within 0.5mm of boundary), not at seed (10mm)
            assert!(
                x_si < 0.0015,
                "expected x near floor (≈1.02mm), got x = {:.4e} m (too far from boundary)",
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
            // b should be notably above 1mm because optimizer maximizes it
            assert!(
                b_si > 0.046,
                "non-money: expected b pushed large (no floor on b from length objective), got b = {:.4e} m",
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
