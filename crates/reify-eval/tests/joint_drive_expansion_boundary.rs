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
use reify_core::{ContentHash, DimensionVector, ModulePath, Type, ValueCellId};
use reify_eval::Engine;
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, CompiledFnBody, CompiledFunction, ObjectiveSense,
    ObjectiveSet, Value, ValueMap,
};
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
///
/// * auto `k` (the solved trial variable);
/// * param `unit` (a dimensionless constant coefficient; reads NO auto);
/// * Let `line_cost = unit * k` (reads the auto `k`);
/// * Let `total = line_cost` (reads `line_cost`, transitively the auto);
/// * a self-constraint `k > 0` (guarantees the auto-bearing scope dispatches a
///   single-scope solve so the problem is captured); and
/// * objective `minimize total`.
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

// ---------------------------------------------------------------------------
// step-5: @optimized / ComputeNode exclusion from dependent_cells.
// ---------------------------------------------------------------------------

/// A `CompiledFunction` named `name` with one `length` param, carrying
/// `optimized_target = Some(target)` — the static marker
/// `find_matching_compiled_function(..).optimized_target` reads to identify an
/// `@optimized` call (the compute-dispatch-bypass precedent at
/// `engine_eval.rs:8204`, mirrored from `invariants.rs`'s
/// `zero_arg_optimized_function`). Its body is an inert `length(0)` literal;
/// the test never depends on the call's runtime value (an unregistered target
/// inlines/falls back, it does not abort eval).
fn optimized_length_fn(name: &str, target: &str) -> CompiledFunction {
    CompiledFunction {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        params: vec![("x".to_string(), Type::length())],
        param_defaults: vec![None],
        return_type: Type::length(),
        body: CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::literal(Value::length(0.0), Type::length()),
        },
        content_hash: ContentHash::of_str(name),
        annotations: vec![],
        optimized_target: Some(target.to_string()),
        type_params: vec![],
    }
}

/// A `UserFunctionCall(function_name, [arg])` `CompiledExpr` with a `length`
/// result type — the default_expr shape of an `@optimized` coupled cell. The
/// single `arg` (a `ValueRef` to the auto) is what makes the cell transitively
/// read the auto: `extract_value_deps` walks `UserFunctionCall` args.
fn user_call(function_name: &str, arg: CompiledExpr) -> CompiledExpr {
    CompiledExpr {
        kind: CompiledExprKind::UserFunctionCall {
            function_name: function_name.to_string(),
            args: vec![arg],
        },
        result_type: Type::length(),
        content_hash: ContentHash::of_str("opt-cost-call"),
    }
}

/// A single scope `S` extending `single_scope_coupled_let_module` with an
/// `@optimized` coupled cell:
///
/// * auto `k`;
/// * param `unit` (constant coefficient; reads NO auto);
/// * Let `line_cost = unit * k` — a plain non-`@optimized` coupled cell;
/// * Let `opt_cost = opt_fn(k)` — an `@optimized` `UserFunctionCall`
///   (`opt_fn.optimized_target = Some(..)`) that transitively reads the auto
///   `k` (through its arg) and feeds the objective (through `total`);
/// * Let `total = line_cost + opt_cost` — a plain non-`@optimized` coupled
///   cell and the objective seed;
/// * self-constraint `k > 0` (guarantees a single-scope solve is dispatched
///   and its problem captured); and
/// * objective `minimize total`.
///
/// After the step-6 membership filter, `dependent_cells` must be EXACTLY the
/// two non-`@optimized` coupled cells `{line_cost, total}` — the `@optimized`
/// `opt_cost` is excluded (it must stay frozen: re-running it through plain
/// `eval_expr` in the post-solve write-back would bypass the compute-dispatch
/// registry). The `opt_fn` `CompiledFunction` is registered on the module so
/// `find_matching_compiled_function` resolves the call to its `optimized_target`.
fn single_scope_optimized_coupled_module() -> CompiledModule {
    let s = TopologyTemplateBuilder::new("S")
        .auto_param("S", "k", Type::length())
        .param(
            "S",
            "unit",
            Type::dimensionless_scalar(),
            Some(literal(Value::Real(2.0))),
        )
        // line_cost = unit * k — non-@optimized coupled cell (must REMAIN).
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
        // opt_cost = opt_fn(k) — @optimized UserFunctionCall (must be EXCLUDED).
        .let_binding(
            "S",
            "opt_cost",
            Type::length(),
            user_call("opt_fn", value_ref("S", "k")),
        )
        // total = line_cost + opt_cost — non-@optimized; opt_cost feeds the
        // objective THROUGH total (must REMAIN, and IS the objective seed).
        .let_binding(
            "S",
            "total",
            Type::length(),
            binop(
                BinOp::Add,
                value_ref("S", "line_cost"),
                value_ref("S", "opt_cost"),
            ),
        )
        .constraint("S", 0, None, gt(value_ref("S", "k"), literal(mm(0.0))))
        .objective(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            value_ref("S", "total"),
        ))
        .build();

    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(s)
        .function(optimized_length_fn("opt_fn", "test::opt_cost"))
        .build()
}

/// step-5: the captured `ResolutionProblem.dependent_cells` must EXCLUDE the
/// `@optimized` `UserFunctionCall` cell `opt_cost` (it must stay frozen), while
/// the plain non-`@optimized` coupled cells `line_cost` and `total` remain
/// present and topologically ordered.
///
/// RED today: the step-4 membership filter keeps ANY non-auto cell that (a)
/// feeds the objective and (b) transitively reads an auto — including
/// `opt_cost`, whose `@optimized` status is not yet consulted. The exclusion
/// lands in step-6.
#[test]
fn dependent_cells_excludes_optimized_userfunctioncall_cell() {
    let module = single_scope_optimized_coupled_module();

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
    let opt_cost = ValueCellId::new("S", "opt_cost");
    let total = ValueCellId::new("S", "total");

    let ids: Vec<ValueCellId> = problem
        .dependent_cells
        .iter()
        .map(|(id, _)| id.clone())
        .collect();

    // (a) core exclusion: the @optimized cell must be ABSENT. Re-running it
    // through plain eval_expr in the write-back would bypass the
    // compute-dispatch registry and clobber its dispatched result.
    assert!(
        !ids.contains(&opt_cost),
        "dependent_cells must EXCLUDE the @optimized UserFunctionCall cell \
         `opt_cost` (its optimized_target is Some ⇒ it must stay frozen, not \
         be re-folded through plain eval_expr); got {ids:?}",
    );

    // (b) non-@optimized coupled cells remain present.
    assert!(
        ids.contains(&line_cost),
        "dependent_cells must still contain the non-@optimized coupled cell \
         `line_cost`; got {ids:?}",
    );
    assert!(
        ids.contains(&total),
        "dependent_cells must still contain the non-@optimized coupled cell \
         `total` (the objective seed); got {ids:?}",
    );

    // (c) exactly the two non-@optimized coupled cells survive.
    assert_eq!(
        ids.len(),
        2,
        "dependent_cells must be EXACTLY {{line_cost, total}} — the @optimized \
         `opt_cost` excluded; got {ids:?}",
    );

    // (d) topological order preserved: line_cost precedes total.
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
        "dependent_cells must stay topologically ordered: `line_cost` before \
         `total` (total reads line_cost); got {ids:?}",
    );
}

// ---------------------------------------------------------------------------
// BT-4 (step-7): dependent_cells is the SINGLE cross-scope authority the
// post-solve write-back materializes coupled cells in.
// ---------------------------------------------------------------------------

/// A two-cycle `MergedSolve` cluster `{A, B}` (A reads `B.m`, B reads `A.k` ⇒
/// irreducible SCC ⇒ guaranteed `MergedSolve`, per `merged_cluster_solve.rs`'s
/// `two_cycle_cluster_module` idiom) with a CROSS-SCOPE Let coupling:
///
/// * `A` (idx0): auto `k`; param `unit = 2`; Let `line_cost = unit * k`;
///   Let `total = line_cost + B.total` — reads the OTHER (later-indexed) scope's
///   Let cell; a 2-cycle constraint `B.m > 0`; and the spanning objective
///   `minimize A.total + B.total`.
/// * `B` (idx1): auto `m`; param `unit = 2`; Let `line_cost = unit * m`;
///   Let `total = line_cost`; a 2-cycle constraint `A.k > 0`.
///
/// The cross-scope Let read is what makes the authoritative order OBSERVABLE:
/// `cluster.scopes` is ascending source index `[A, B]`, so the merged
/// write-back's per-member `evaluate_let_bindings` loop processes `A` BEFORE
/// `B`. `A.total`'s expr reads `B.total`, which — during A's per-member pass —
/// is still the STALE value from B's pre-solve main pass (`m` undetermined ⇒
/// `Undef`). Only re-materializing the coupled cells in the cross-scope
/// authoritative order (`B.total` before `A.total`) yields the correct
/// `A.total`. The four coupled Let cells all transitively read a cluster auto
/// and feed the spanning objective, so they are exactly `dependent_cells`; the
/// two `unit` params read no auto and are excluded.
fn two_cycle_cross_scope_coupled_module() -> CompiledModule {
    let a = TopologyTemplateBuilder::new("A")
        .auto_param("A", "k", Type::length())
        .param(
            "A",
            "unit",
            Type::dimensionless_scalar(),
            Some(literal(Value::Real(2.0))),
        )
        // line_cost_A = unit_A * k  (own-scope; reads auto k).
        .let_binding(
            "A",
            "line_cost",
            Type::length(),
            binop(
                BinOp::Mul,
                value_ref_typed("A", "unit", Type::dimensionless_scalar()),
                value_ref("A", "k"),
            ),
        )
        // total_A = line_cost_A + B.total  — CROSS-SCOPE read of the LATER
        // scope's Let cell: the write-back order is what determines whether
        // B.total is fresh or stale when this is materialized.
        .let_binding(
            "A",
            "total",
            Type::length(),
            binop(
                BinOp::Add,
                value_ref("A", "line_cost"),
                value_ref("B", "total"),
            ),
        )
        // 2-cycle edge: A reads B.m (guarantees the {A,B} MergedSolve cluster).
        .constraint("A", 0, None, gt(value_ref("B", "m"), literal(mm(0.0))))
        // Spanning objective reading BOTH scopes' totals.
        .objective(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            binop(
                BinOp::Add,
                value_ref("A", "total"),
                value_ref("B", "total"),
            ),
        ))
        .build();

    let b = TopologyTemplateBuilder::new("B")
        .auto_param("B", "m", Type::length())
        .param(
            "B",
            "unit",
            Type::dimensionless_scalar(),
            Some(literal(Value::Real(2.0))),
        )
        // line_cost_B = unit_B * m  (own-scope; reads auto m).
        .let_binding(
            "B",
            "line_cost",
            Type::length(),
            binop(
                BinOp::Mul,
                value_ref_typed("B", "unit", Type::dimensionless_scalar()),
                value_ref("B", "m"),
            ),
        )
        // total_B = line_cost_B  (own-scope).
        .let_binding("B", "total", Type::length(), value_ref("B", "line_cost"))
        // 2-cycle edge: B reads A.k → cycle → guaranteed MergedSolve.
        .constraint("B", 0, None, gt(value_ref("A", "k"), literal(mm(0.0))))
        .build();

    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(a)
        .template(b)
        .build()
}

/// BT-4: the captured merged `problem.dependent_cells` must be (i) a valid
/// CROSS-SCOPE topological order over the four coupled Let cells spanning both
/// scopes, and (ii) the SINGLE authority the post-solve write-back materializes
/// those cells in — verified by comparing the `EvalResult`'s final coupled-cell
/// values to a reference fold that seeds the spy-returned autos + `unit`
/// constants and evaluates `problem.dependent_cells` in stored order.
///
/// RED today: the merged write-back re-evaluates each cluster member's Let cone
/// in per-member `detect_let_cycle` order (A before B, `cluster.scopes` order),
/// so `A.total` reads a STALE `B.total` and lands as `Undef` — diverging from
/// the cross-scope-ordered reference fold (which yields `A.total = 6mm + 14mm =
/// 20mm`). `dependent_cells` is not yet the write-back's authority.
#[test]
fn bt4_dependent_cells_is_cross_scope_topo_order_and_writeback_authority() {
    let module = two_cycle_cross_scope_coupled_module();

    let a_k = ValueCellId::new("A", "k");
    let b_m = ValueCellId::new("B", "m");
    let mut solved = HashMap::new();
    solved.insert(a_k.clone(), mm(3.0));
    solved.insert(b_m.clone(), mm(7.0));

    let spy = SpyConstraintSolver::new_solved(solved);
    let captured = spy.captured_problem();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let result = engine.eval(&module);

    let problem = captured.lock().unwrap().clone().expect(
        "the two-cycle {A,B} SCC must dispatch ONE merged solve, capturing its \
         problem",
    );

    let line_cost_a = ValueCellId::new("A", "line_cost");
    let total_a = ValueCellId::new("A", "total");
    let line_cost_b = ValueCellId::new("B", "line_cost");
    let total_b = ValueCellId::new("B", "total");

    let ids: Vec<ValueCellId> = problem
        .dependent_cells
        .iter()
        .map(|(id, _)| id.clone())
        .collect();

    // ---- (i) valid CROSS-SCOPE topological order spanning both scopes. ----

    // membership: exactly the four coupled Let cells, autos excluded.
    for want in [&line_cost_a, &total_a, &line_cost_b, &total_b] {
        assert!(
            ids.contains(want),
            "dependent_cells must contain the coupled Let cell {want:?} (reads a \
             cluster auto and feeds the spanning objective); got {ids:?}",
        );
    }
    assert!(
        !ids.contains(&a_k) && !ids.contains(&b_m),
        "dependent_cells must EXCLUDE the cluster autos {a_k:?}/{b_m:?}; got {ids:?}",
    );
    assert_eq!(
        ids.len(),
        4,
        "dependent_cells must be EXACTLY the four coupled Let cells \
         {{A.line_cost, A.total, B.line_cost, B.total}} (the `unit` params read \
         no auto ⇒ excluded); got {ids:?}",
    );

    // spanning both scopes.
    assert!(
        ids.iter().any(|id| id.entity == "A") && ids.iter().any(|id| id.entity == "B"),
        "dependent_cells must span BOTH cluster scopes A and B; got {ids:?}",
    );

    // ordering: each cell after all its in-set deps, INCLUDING the cross-scope
    // edge B.total → A.total.
    let pos = |id: &ValueCellId| -> usize {
        ids.iter()
            .position(|x| x == id)
            .unwrap_or_else(|| panic!("dependent_cells missing {id:?}; got {ids:?}"))
    };
    assert!(
        pos(&line_cost_a) < pos(&total_a),
        "within-scope order: A.line_cost must precede A.total (A.total reads \
         A.line_cost); got {ids:?}",
    );
    assert!(
        pos(&line_cost_b) < pos(&total_b),
        "within-scope order: B.line_cost must precede B.total (B.total reads \
         B.line_cost); got {ids:?}",
    );
    assert!(
        pos(&total_b) < pos(&total_a),
        "CROSS-SCOPE order: B.total must precede A.total (A.total reads B.total \
         across scopes) — this is the edge a per-member detect_let_cycle order \
         cannot honour; got {ids:?}",
    );

    // ---- (ii) EvalResult coupled-cell values == reference fold of the SAME
    // dependent_cells list, in stored order (single authority). ----

    // Reference fold: seed the spy-returned autos + the `unit` param constants
    // (the only reads the coupled exprs make that are NOT themselves in
    // dependent_cells), then evaluate problem.dependent_cells IN STORED ORDER
    // via the same evaluator the write-back uses. If the write-back consumed a
    // DIFFERENT order (or a different list), the materialized A.total diverges.
    let functions: &[CompiledFunction] = &problem.functions;
    let mut fold_values = ValueMap::new();
    fold_values.insert(a_k.clone(), mm(3.0));
    fold_values.insert(b_m.clone(), mm(7.0));
    fold_values.insert(ValueCellId::new("A", "unit"), Value::Real(2.0));
    fold_values.insert(ValueCellId::new("B", "unit"), Value::Real(2.0));
    for (id, expr) in &problem.dependent_cells {
        let v = eval_expr(expr, &EvalContext::new(&fold_values, functions));
        fold_values.insert(id.clone(), v);
    }

    for id in [&line_cost_a, &total_a, &line_cost_b, &total_b] {
        let expected = fold_values.get_or_undef(id);
        let actual = result.values.get(id).cloned().unwrap_or(Value::Undef);
        assert_eq!(
            actual, expected,
            "coupled cell {id:?}: the post-solve write-back must materialize it \
             in the SAME authoritative cross-scope order as \
             problem.dependent_cells (single authority). Expected the \
             dependent_cells fold value {expected:?}, got {actual:?}. RED today: \
             the write-back re-evaluates each cluster member's Let cone in \
             per-member detect_let_cycle order, so A.total reads a STALE B.total \
             (B is processed after A) instead of the cross-scope-ordered value.",
        );
    }
}
