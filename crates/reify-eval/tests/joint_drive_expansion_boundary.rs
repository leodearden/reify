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

use std::collections::HashMap;

use reify_compiler::CompiledModule;
use reify_core::{DimensionVector, ModulePath, Type, ValueCellId};
use reify_eval::Engine;
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, ObjectiveSense, ObjectiveSet, Value};
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, SpyConstraintSolver, TopologyTemplateBuilder,
    binop, gt, literal, mm, parse_and_compile_with_stdlib, value_ref, value_ref_typed,
};

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

// ---------------------------------------------------------------------------
// step-3: dependent_cells population + membership + topological order.
// ---------------------------------------------------------------------------

/// A single uncoupled scope `S` with:
///   * auto  `k`                     (the solved trial variable),
///   * param `unit`                  (a dimensionless constant coefficient;
///                                     reads NO auto),
///   * Let   `line_cost = unit * k`  (reads the auto `k`),
///   * Let   `total = line_cost`     (reads `line_cost`, transitively the auto),
///   * a self-constraint `k > 0`     (guarantees the auto-bearing scope
///                                     dispatches a single-scope solve so the
///                                     problem is captured), and
///   * objective `minimize total`.
///
/// `S` has no cross-scope reads, so it is solved on the single-scope
/// `build_solver_problem` path. Its `dependent_cells` must therefore be
/// exactly the coupled Let cells `{line_cost, total}` — the non-auto,
/// non-`@optimized` cells that (a) transitively feed the objective AND (b)
/// transitively read the auto `k` — in a topological order where `line_cost`
/// precedes `total`. The auto `k` (membership excludes autos) and the param
/// `unit` (reads no auto) must both be absent.
fn single_scope_coupled_let_module() -> CompiledModule {
    let s = TopologyTemplateBuilder::new("S")
        .auto_param("S", "k", Type::length())
        .param(
            "S",
            "unit",
            Type::dimensionless_scalar(),
            Some(literal(Value::Real(2.0))),
        )
        // line_cost = unit * k  (dimensionless * length = length)
        .let_binding(
            "S",
            "line_cost",
            Type::length(),
            binop(
                BinOp::Mul,
                value_ref_typed("S", "unit", Type::dimensionless_scalar()),
                value_ref("S", "k"),
            ),
        )
        // total = line_cost
        .let_binding("S", "total", Type::length(), value_ref("S", "line_cost"))
        // A self-constraint reading the auto guarantees a single-scope solve is
        // dispatched (and captured); it introduces no new coupled cells.
        .constraint("S", 0, None, gt(value_ref("S", "k"), literal(mm(0.0))))
        .objective(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            value_ref("S", "total"),
        ))
        .build();

    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(s)
        .build()
}

/// step-3: the captured `ResolutionProblem.dependent_cells` must hold exactly
/// the coupled Let cells `{line_cost, total}`, exclude the auto `k` and the
/// non-auto-reading param `unit`, and be topologically ordered
/// (`line_cost` before `total`, since `total`'s default_expr reads
/// `line_cost`).
///
/// RED today: `dependent_cells` is `Vec::new()` (empty from pre-1); the
/// authoritative cross-scope-order helper that populates it lands in step-4.
#[test]
fn dependent_cells_holds_coupled_lets_excludes_auto_and_is_topologically_ordered() {
    let module = single_scope_coupled_let_module();

    let k = ValueCellId::new("S", "k");
    let mut solved = HashMap::new();
    solved.insert(k.clone(), mm(3.0));

    let spy = SpyConstraintSolver::new_solved(solved);
    let captured = spy.captured_problem();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let _result = engine.eval(&module);

    let problem = captured.lock().unwrap().clone().expect(
        "S (the auto-bearing scope) must dispatch a single-scope solve, \
         capturing its problem",
    );

    let line_cost = ValueCellId::new("S", "line_cost");
    let total = ValueCellId::new("S", "total");
    let unit = ValueCellId::new("S", "unit");

    let ids: Vec<ValueCellId> = problem
        .dependent_cells
        .iter()
        .map(|(id, _)| id.clone())
        .collect();

    // (a) membership: exactly the two coupled Let cells.
    assert!(
        ids.contains(&line_cost),
        "dependent_cells must contain the coupled Let cell `line_cost` \
         (reads the auto `k` and feeds the objective); got {ids:?}",
    );
    assert!(
        ids.contains(&total),
        "dependent_cells must contain the coupled Let cell `total` \
         (transitively reads the auto `k` and IS the objective seed); got {ids:?}",
    );
    assert!(
        !ids.contains(&k),
        "dependent_cells must EXCLUDE the auto `k` (membership excludes \
         auto_params); got {ids:?}",
    );
    assert!(
        !ids.contains(&unit),
        "dependent_cells must EXCLUDE the param `unit` — it feeds the \
         objective but reads NO auto, so it fails membership rule (b); got {ids:?}",
    );
    assert_eq!(
        ids.len(),
        2,
        "dependent_cells must be EXACTLY the two coupled Let cells \
         {{line_cost, total}}; got {ids:?}",
    );

    // (b) topological order: line_cost precedes total (total reads line_cost).
    let pos_line_cost = ids
        .iter()
        .position(|id| id == &line_cost)
        .expect("line_cost present (asserted above)");
    let pos_total = ids
        .iter()
        .position(|id| id == &total)
        .expect("total present (asserted above)");
    assert!(
        pos_line_cost < pos_total,
        "dependent_cells must be topologically ordered: `line_cost` must \
         precede `total` because `total`'s default_expr reads `line_cost`; \
         got order {ids:?}",
    );
}
