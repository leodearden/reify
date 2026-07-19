//! Boundary tests for [JOINT-DRIVE α] (task #5188): engine-side
//! objective/constraint-position `cost(self.descendants)` expansion,
//! `ResolutionProblem.dependent_cells` population + authoritative cross-scope
//! topological order.
//!
//! PRD: docs/prds/v0_6/whole-model-joint-drive-seam.md task α (§4/§5/§6.1/§6.3).
//!
//! ## Test harness
//!
//! Observed through the PUBLIC `Engine::eval` path via
//! `SpyConstraintSolver.captured_problems()` (idiom: `merged_cluster_solve.rs`),
//! so the tests assert on the exact `ResolutionProblem` handed to the solver
//! after elaboration + expansion without exposing the private
//! `build_solver_problem` / `build_merged_solver_problem` builders.
//!
//! BT-3 uses a real `parse_and_compile_with_stdlib` source fixture because it
//! needs the genuine compiler `cost(self.descendants)` `FunctionCall` shape,
//! the stdlib `Costed` trait, and descendants enumeration — only the real
//! compile path produces those.

use reify_core::{DimensionVector, ValueCellId};
use reify_eval::Engine;
use reify_ir::{CompiledExpr, CompiledExprKind, Value};
use reify_test_support::{MockConstraintChecker, SpyConstraintSolver, parse_and_compile_with_stdlib};

// ---------------------------------------------------------------------------
// Shared CompiledExpr structural walkers.
// ---------------------------------------------------------------------------

/// Collect `e` and every transitively-nested sub-expression into `out`.
///
/// Covers every compound `CompiledExprKind` a `cost(self.descendants)`
/// objective/constraint expression can contain before OR after the
/// `apply_cost_aggregation` rewrite (`BinOp` comparison, the `cost`
/// `FunctionCall`, the `self.descendants` `MethodCall` placeholder, and the
/// post-expansion `[ValueRef(line_cost)...].sum` `MethodCall`/`ListLiteral`).
fn collect<'a>(e: &'a CompiledExpr, out: &mut Vec<&'a CompiledExpr>) {
    out.push(e);
    match &e.kind {
        CompiledExprKind::BinOp { left, right, .. } => {
            collect(left, out);
            collect(right, out);
        }
        CompiledExprKind::UnOp { operand, .. } => collect(operand, out),
        CompiledExprKind::FunctionCall { args, .. } => {
            for a in args {
                collect(a, out);
            }
        }
        CompiledExprKind::UserFunctionCall { args, .. } => {
            for a in args {
                collect(a, out);
            }
        }
        CompiledExprKind::MethodCall { object, args, .. } => {
            collect(object, out);
            for a in args {
                collect(a, out);
            }
        }
        CompiledExprKind::ListLiteral(v) | CompiledExprKind::SetLiteral(v) => {
            for a in v {
                collect(a, out);
            }
        }
        CompiledExprKind::IndexAccess { object, index } => {
            collect(object, out);
            collect(index, out);
        }
        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            collect(condition, out);
            collect(then_branch, out);
            collect(else_branch, out);
        }
        CompiledExprKind::OptionSome(inner) => collect(inner, out),
        _ => {}
    }
}

fn all_nodes(e: &CompiledExpr) -> Vec<&CompiledExpr> {
    let mut out = Vec::new();
    collect(e, &mut out);
    out
}

/// True iff `e` contains a raw `cost(...)` builtin `FunctionCall` anywhere —
/// i.e. the objective/constraint expander did NOT run (the RED state).
fn has_cost_function_call(e: &CompiledExpr) -> bool {
    all_nodes(e).iter().any(|n| {
        matches!(&n.kind, CompiledExprKind::FunctionCall { function, .. } if function.name == "cost")
    })
}

/// True iff `e` still contains an unexpanded `self.descendants` structural-query
/// placeholder (`MethodCall { method == "descendants" }` on the `__self`
/// pseudo-cell, or any residual `ValueRef(_, "__self")`).
fn has_descendants_placeholder(e: &CompiledExpr) -> bool {
    all_nodes(e).iter().any(|n| match &n.kind {
        CompiledExprKind::MethodCall { method, .. } => method == "descendants",
        CompiledExprKind::ValueRef(id) => id.member == "__self",
        _ => false,
    })
}

/// True iff `e` contains a `.sum` `MethodCall` whose object subtree holds at
/// least one `ValueRef(ValueCellId { member: "line_cost", .. })` — the exact
/// shape `apply_cost_aggregation` rewrites `cost(<Costed list>)` into.
fn line_cost_ref_under_sum(e: &CompiledExpr) -> bool {
    all_nodes(e).iter().any(|n| match &n.kind {
        CompiledExprKind::MethodCall { object, method, .. } if method == "sum" => all_nodes(object)
            .iter()
            .any(|inner| matches!(&inner.kind, CompiledExprKind::ValueRef(id) if id.member == "line_cost")),
        _ => false,
    })
}

/// Assert `expr` has been through the objective/constraint-position expansion
/// pass: `cost(self.descendants)` is now `[ValueRef(line_cost)...].sum`, with
/// no residual `cost` builtin call and no `self.descendants` placeholder.
#[track_caller]
fn assert_cost_expanded(expr: &CompiledExpr, ctx: &str) {
    assert!(
        line_cost_ref_under_sum(expr),
        "{ctx}: expected a `.sum` MethodCall over ValueRef(_, \"line_cost\") \
         nodes after expansion; got expr kind {:?}",
        expr.kind,
    );
    assert!(
        !has_cost_function_call(expr),
        "{ctx}: expected NO raw `cost(...)` FunctionCall after expansion — the \
         builder must run apply_cost_aggregation before the expr enters the \
         ResolutionProblem; got expr kind {:?}",
        expr.kind,
    );
    assert!(
        !has_descendants_placeholder(expr),
        "{ctx}: expected NO residual `self.descendants` MethodCall placeholder \
         (or __self ValueRef) after expansion; got expr kind {:?}",
        expr.kind,
    );
}

// ---------------------------------------------------------------------------
// BT-3 (step-1): objective/constraint-position cost() expansion.
// ---------------------------------------------------------------------------

/// `CapScrew` / `MotorMount` are `Costed`-conforming subs (definitions reused
/// from `cost_subtree_aggregate_eval.rs`): each derives a `line_cost` from
/// `unit_cost * quantity_produced`.
const CAP_SCREW_DEF: &str = r#"
structure def CapScrew : Costed {
    param supplier          : String = "McMaster-Carr"
    param part_number       : String = "91251A190"
    param unit_cost         : Money  = 0.12USD
    param lead_time         : Time   = 24h
    param quantity_produced : Real   = 24.0
}
"#;

const MOTOR_MOUNT_DEF: &str = r#"
structure def MotorMount : Costed {
    param supplier          : String = "Misumi"
    param part_number       : String = "BNVAS25-30"
    param unit_cost         : Money  = 8.50USD
    param lead_time         : Time   = 72h
    param quantity_produced : Real   = 4.0
}
"#;

/// BT-3: a parent scope `Rig` with a `Money` auto (`budget`), a
/// `minimize cost(self.descendants)` objective, and a
/// `constraint budget > cost(self.descendants)` (the constraint reads the auto
/// so it survives `filter_constraints_reading_autos`), over two `Costed`
/// children.
///
/// After the α expansion, BOTH the captured objective term expr AND the
/// captured constraint expr must carry `ValueRef(_, "line_cost")` under a
/// `.sum` MethodCall, with NO raw `cost(...)` FunctionCall and NO residual
/// `self.descendants` placeholder.
///
/// RED today: `build_solver_problem` plain-clones the objective
/// (`objective.cloned()`) and filters UNEXPANDED `template.constraints`, so a
/// raw `cost` FunctionCall reaches the problem for both.
#[test]
fn bt3_objective_and_constraint_position_cost_is_expanded() {
    let source = format!(
        r#"
{CAP_SCREW_DEF}
{MOTOR_MOUNT_DEF}
structure Rig {{
    param budget : Money = auto(free)
    sub bolts = CapScrew()
    sub mounts = MotorMount()
    constraint budget > cost(self.descendants)
    minimize cost(self.descendants)
}}
"#
    );

    let compiled = parse_and_compile_with_stdlib(&source);

    let budget = ValueCellId::new("Rig", "budget");
    let mut solved = std::collections::HashMap::new();
    solved.insert(
        budget.clone(),
        Value::Scalar {
            si_value: 100.0,
            dimension: DimensionVector::MONEY,
        },
    );

    let spy = SpyConstraintSolver::new_solved(solved);
    let captured = spy.captured_problem();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let _result = engine.eval(&compiled);

    let problem = captured
        .lock()
        .unwrap()
        .clone()
        .expect("Rig (the only auto-bearing scope) must dispatch a solve, capturing its problem");

    // (a) objective term expr expanded.
    let objective = problem
        .objective
        .as_ref()
        .expect("Rig's `minimize cost(self.descendants)` must reach the problem as an objective");
    assert_eq!(
        objective.terms.len(),
        1,
        "the objective must carry Rig's single minimize term; got {} term(s)",
        objective.terms.len(),
    );
    assert_cost_expanded(&objective.terms[0].expr, "objective term expr");

    // (b) constraint expr expanded. Exactly one constraint reads the `budget`
    // auto (`budget > cost(self.descendants)`), so it survives filtering.
    assert_eq!(
        problem.constraints.len(),
        1,
        "exactly the `budget > cost(...)` constraint (which reads the auto) \
         must survive filter_constraints_reading_autos; got {} constraint(s)",
        problem.constraints.len(),
    );
    assert_cost_expanded(&problem.constraints[0].1, "constraint expr");
}
