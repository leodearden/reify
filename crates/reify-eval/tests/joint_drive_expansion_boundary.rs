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
use reify_constraints::DimensionalSolver;
use reify_core::{
    ContentHash, DiagnosticCode, DimensionVector, ModulePath, Severity, Type, ValueCellId,
};
use reify_eval::{Engine, EvalResult};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, CompiledFnBody, CompiledFunction, ObjectiveSense,
    ObjectiveSet, Value, ValueMap,
};
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, SpyConstraintSolver, TopologyTemplateBuilder,
    binop, collect_errors, compile_source_with_stdlib, gt, literal, mm,
    parse_and_compile_with_stdlib, value_ref, value_ref_typed,
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

// ---------------------------------------------------------------------------
// [JOINT-DRIVE β] (task #5189 step-9) — BT-5, the user-observable LEAF, and
// BT-6, "an illegal cycle still errors" (rejection OBSERVED, not assumed).
//
// PRD: docs/prds/v0_6/whole-model-joint-drive-seam.md §7 (BT-5, BT-6).
// ---------------------------------------------------------------------------

/// The SHIPPED joint-drive example, read from disk rather than copied into a
/// fixture string — so this test degrades loudly if the published example drifts
/// away from the behaviour its own header comment advertises.
///
/// Idiom: `crates/reify-eval/tests/continuous_cost_min_example_e2e.rs`.
const JOINT_DRIVE_EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/whole_model_joint_drive.ri"
);

/// Compile + eval `src` through the REAL solver.
///
/// `Engine::new(..).with_solver(Box::new(DimensionalSolver))` is load-bearing:
/// the plain test engine wires NO solver, so every auto would stay `Undef` and
/// both halves of BT-5 would compare `Undef` to `Undef`.
fn eval_ri_with_real_solver(src: &str, what: &str) -> EvalResult {
    let compiled = compile_source_with_stdlib(src);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "the {what} source must compile without errors; got {errors:#?}",
    );

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));
    let result = engine.eval(&compiled);

    // Zero Severity::Error. Deliberately NOT pinned: Info/Warning. A
    // `RobustnessFloorApplied` Info is EXPECTED on the Money-objective path, and
    // a `W_SCOPE_COUPLING` warning may accompany the merged cluster.
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "the {what} eval must emit no Severity::Error diagnostics; got \
         {eval_errors:#?}",
    );

    result
}

/// The SI magnitude of a resolved `Scalar` cell, or a panic naming what was
/// actually there (an unresolved auto surfaces as `Undef`, which is exactly the
/// failure BT-5 must report legibly rather than as a silent `0.0`).
fn scalar_si(result: &EvalResult, id: &ValueCellId, what: &str) -> f64 {
    match result.values.get(id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!("expected a resolved Scalar for {id:?} in the {what} eval; got {other:?}"),
    }
}

/// `Some(si)` iff `id` resolved to a `Scalar` — used to pick BT-5's cost cell
/// without asserting on which one materialises.
fn scalar_si_opt(result: &EvalResult, id: &ValueCellId) -> Option<f64> {
    match result.values.get(id) {
        Some(Value::Scalar { si_value, .. }) => Some(*si_value),
        _ => None,
    }
}

/// Derive the FROZEN-CASCADE variant from the shipped source by removing the
/// parent's inlined `minimize` line.
///
/// DERIVED, never transcribed: the baseline must be the *same model* minus the
/// objective, so it cannot drift from the example the merged half evaluates. A
/// recorded constant would silently rot the moment the solver's seeding, bounds
/// defaults, or synthesised centrality objective changed — converting a live
/// comparative signal into a stale pin.
///
/// With no `minimize`, δ forms no cluster, so `Rivet` resolves ALONE bottom-up:
/// that IS the frozen cascade, by definition rather than by reconstruction. The
/// child still carries live inequality constraints, so `build_centrality_objective`
/// synthesises a Chebyshev-centre objective and the freeze lands at the BOX
/// CENTRE.
///
/// Only the real statement is stripped: every `minimize` mention in the header
/// comment is behind a `//`, so `trim_start().starts_with("minimize ")` cannot
/// match it. The exactly-one assertion below is the guard on that claim.
fn strip_inlined_minimize(src: &str) -> String {
    let kept: Vec<&str> = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("minimize "))
        .collect();
    assert_eq!(
        kept.len(),
        src.lines().count() - 1,
        "exactly ONE `minimize` statement must be stripped to derive the \
         frozen-cascade baseline — if the shipped example gained or lost one, \
         BT-5's baseline is no longer 'the same model minus the objective' and \
         the comparison is void",
    );
    kept.join("\n")
}

/// BT-5 — THE LEAF. The parent's cost objective reaches the CHILD's auto in one
/// merged solve, and the whole assembly lands strictly cheaper than the bottom-up
/// freeze-as-you-go cascade would have pinned it.
///
/// Both assertions are COMPARATIVE (strict inequality), never an absolute
/// converged value or a tuned tolerance — the house norm for `.ri`-layer tests
/// (precise-argmax assertions belong at the `reify-constraints` layer with
/// explicitly bounded autos; cf. `continuous_cost_min_example_e2e.rs`'s
/// "why no upper-bound assertion" note).
///
/// # Achievability — DERIVED, not guessed
///
/// * FROZEN: `Rivet` resolves alone with NO user objective but live inequality
///   constraints, so the synthesised Chebyshev-centre objective pins it at the
///   BOX CENTRE — half the box width above the box's lower bound.
/// * MERGED: the child is resolved INSIDE the parent's problem, governed by the
///   parent's Money objective `cost(self.descendants)` = `line_cost` =
///   `unit_cost * quantity_produced`. No centrality objective is synthesised
///   (`solver.rs` only synthesises one when `problem.objective.is_none()`), so
///   the freeze-at-the-centre outcome is GONE and the cost-minimising direction
///   governs instead.
/// * `line_cost` is strictly increasing in `quantity_produced`, so a strictly
///   lower resolved auto implies a strictly lower cost.
///
/// # What this test does NOT claim — the eval-layer convergence boundary
///
/// It does NOT assert the merged auto lands ON the cost argmin. A LINEAR Money
/// objective's argmin sits exactly on a constraint boundary, where the penalty
/// method's stationary point is offset INSIDE the penalty by
/// `objective_gradient / (2 * PENALTY_WEIGHT)` ≈ 2.5e-7 — vastly larger than
/// `FEASIBILITY_THRESHOLD` (1e-12) — so the converged point reads as infeasible
/// and `solve_core` returns its `initially_feasible` fallback: the seed
/// `extract_initial_point` supplies. That is the SAME documented eval-layer
/// behaviour `examples/continuous_cost_min.ri`'s header records ("returns the
/// initially-feasible SEED ... rather than a unique convergent point", PRD §9 Q2
/// "no fix required"), and it is exactly why the house norm puts precise-argmin
/// assertions at the `reify-constraints` layer with explicitly bounded autos and
/// keeps `.ri`-layer tests on ordering / off-boundary claims.
///
/// # Why it is still a live signal, not a vacuous one
///
/// Each link of the chain fails this test if it regresses:
/// * δ's cluster stops forming ⇒ the child resolves alone ⇒ merged == frozen ⇒
///   (i) fails on the strict inequality.
/// * α's `dependent_cells` arrives empty, or β's per-trial fold / write-back
///   stops running ⇒ the objective's instance-path `ValueRef` is never written
///   ⇒ `eval_objective_set` is `None` ⇒ `NoProgress` ⇒ the zero-`Error`
///   precondition in [`eval_ri_with_real_solver`] fails.
/// * the instance-path ALIAS entry regresses ⇒ (iii) fails.
///
/// RED until the whole chain works end to end: δ forms the cluster → α populates
/// `dependent_cells` → β folds them per trial → α writes them back.
#[test]
fn bt5_parent_objective_drives_child_auto_strictly_below_the_frozen_cascade() {
    let merged_src = std::fs::read_to_string(JOINT_DRIVE_EXAMPLE_PATH)
        .unwrap_or_else(|e| panic!("could not read {JOINT_DRIVE_EXAMPLE_PATH}: {e}"));
    let frozen_src = strip_inlined_minimize(&merged_src);

    let merged = eval_ri_with_real_solver(&merged_src, "merged (inlined `minimize` present)");
    let frozen = eval_ri_with_real_solver(&frozen_src, "frozen cascade (`minimize` removed)");

    // ---- (i) the child auto resolves STRICTLY DIFFERENT from its freeze. ----

    // STRUCTURE-KEYED, not instance-path. The solver's auto_params, and hence
    // its solved write-back, are keyed by the DECLARING TEMPLATE
    // (`ValueCellId::new(&structure.name, &param.name)`); the instance-path
    // spelling `RivetedPanel.rivets.quantity_produced` is never written for a
    // solver-resolved auto in EITHER half (a separate, pre-existing
    // sub-elaboration gap — see the example header's "reading the result" note).
    let auto_id = ValueCellId::new("Rivet", "quantity_produced");
    let merged_q = scalar_si(&merged, &auto_id, "merged");
    let frozen_q = scalar_si(&frozen, &auto_id, "frozen-cascade");

    // Asserted with a DIRECTION, which is strictly stronger than `!=` and still
    // purely comparative: cost-min drives the auto DOWN toward the box's lower
    // bound, while the frozen cascade parks it at the box centre above.
    assert!(
        merged_q < frozen_q,
        "BT-5(i): the merged solve must drive the child's auto STRICTLY BELOW \
         the value the bottom-up frozen cascade pins it at (cost-min argmin at \
         the box's lower bound vs. the synthesised Chebyshev centre). Got \
         merged={merged_q} vs frozen={frozen_q} (difference {}). Equal values \
         mean the parent's objective never reached the child's auto at all.",
        frozen_q - merged_q,
    );

    // ---- (ii) merged whole-assembly cost STRICTLY LESS than the baseline. ----

    // Prefer the parent's `let total_cost` aggregate; fall back to the child's
    // derived `Costed.line_cost`. With ONE depth-1 Costed descendant the two are
    // the same number (`cost(self.descendants)` == `[rivets.line_cost].sum`), so
    // either is a faithful whole-assembly cost — but BOTH sides must be read
    // from the SAME cell or the comparison is not apples-to-apples.
    let total_cost = ValueCellId::new("RivetedPanel", "total_cost");
    let line_cost = ValueCellId::new("Rivet", "line_cost");
    let (cost_id, which) = if scalar_si_opt(&merged, &total_cost).is_some()
        && scalar_si_opt(&frozen, &total_cost).is_some()
    {
        (total_cost, "the parent's `let total_cost` aggregate")
    } else {
        (
            line_cost,
            "the child's derived `Costed.line_cost` (the parent aggregate did \
             not materialise as a Scalar post-solve)",
        )
    };

    let merged_cost = scalar_si(&merged, &cost_id, "merged");
    let frozen_cost = scalar_si(&frozen, &cost_id, "frozen-cascade");
    assert!(
        merged_cost < frozen_cost,
        "BT-5(ii): the merged whole-assembly cost (read from {which}) must be \
         STRICTLY LESS than the bottom-up frozen-cascade baseline — that gap IS \
         the user-observable joint-drive signal. Got merged={merged_cost} vs \
         frozen={frozen_cost} (saving {}).",
        frozen_cost - merged_cost,
    );

    // ---- (iii) the objective's OWN spelling of the cost cell is live. ----
    //
    // The expanded objective term reads the INSTANCE-PATH id
    // `RivetedPanel.rivets.line_cost` (`apply_cost_aggregation` composes it from
    // `enumerate_descendants`' prefixes), while the only declaration — and hence
    // the only foldable `default_expr` — is the STRUCTURE-KEYED `Rivet.line_cost`.
    // `build_dependent_cells` bridges the two namespaces by emitting an alias
    // entry; without it the objective reads an id nothing writes, evaluates to
    // `Undef`, and the merged solve reports `NoProgress`.
    //
    // Derived, not pinned: assert the alias AGREES with its structure-keyed
    // source, and that both agree with `unit_cost * quantity_produced` read from
    // the same eval — the `Costed` closed form. A stale (unfolded) cell fails
    // this even when (i) and (ii) would pass.
    let aliased_cost = scalar_si(
        &merged,
        &ValueCellId::new("RivetedPanel.rivets", "line_cost"),
        "merged",
    );
    let merged_line_cost = scalar_si(&merged, &ValueCellId::new("Rivet", "line_cost"), "merged");
    assert_eq!(
        aliased_cost, merged_line_cost,
        "BT-5(iii): the INSTANCE-PATH cost cell the expanded objective actually \
         reads must hold the same freshly-folded value as its STRUCTURE-KEYED \
         source — that alias entry is what makes the objective non-`Undef` \
         inside the merged solve",
    );
    let merged_unit_cost = scalar_si(&merged, &ValueCellId::new("Rivet", "unit_cost"), "merged");
    assert_eq!(
        merged_line_cost,
        merged_unit_cost * merged_q,
        "BT-5(iii): the derived `Costed.line_cost` must be REFOLDED against the \
         solved auto (`unit_cost * quantity_produced` = {merged_unit_cost} * \
         {merged_q}), not left at whatever it held before the solve",
    );
}

/// BT-6(a) — an INTRA-TEMPLATE let cycle in a model that ALSO carries an auto
/// and a `minimize` still errors. β's per-trial SCC-unroll must not have
/// silently admitted a cycle that has nothing to do with the solver feedback
/// edge.
///
/// `detect_let_cycle` reports it as an un-coded `Severity::Error` reading
/// "circular let-binding dependency in template {name}: [members]" (assertion
/// shape: `crates/reify-eval/tests/unified_dag_cycle_contract.rs`). The auto and
/// the `minimize` are the load-bearing part of the fixture — a bare cyclic
/// module never reaches the solver-problem builders at all, so it would not test
/// β's territory.
#[test]
fn bt6a_intra_template_let_cycle_with_an_auto_and_an_objective_still_errors() {
    let source = "\
structure CyclicLetWithAuto {
    param t : Length = auto(free)
    constraint t > 2mm

    let a = b + 1.0
    let b = a + 1.0

    minimize t / 1mm
}
";

    let compiled = compile_source_with_stdlib(source);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "a mutual let cycle is an EVAL-time property — the compiler accepts it; \
         got compile errors {errors:#?}",
    );

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));
    let result = engine.eval(&compiled);

    assert!(
        result.diagnostics.iter().any(|d| d.severity == Severity::Error
            && d.message.contains("circular let-binding dependency")),
        "the intra-template `a → b → a` let cycle must STILL surface its \
         existing error through `engine.eval()` even though the model carries an \
         auto and a `minimize` — β's per-trial SCC-unroll admits only the solver \
         feedback edge, never a data cycle; got {:#?}",
        result.diagnostics,
    );
}

/// A `MergedSolve` cluster `{A, B}` carrying an INADMISSIBLE cross-scope value
/// cycle: `A.total = A.line_cost + B.total` and `B.total = B.line_cost + A.total`.
///
/// Modelled on `two_cycle_cross_scope_coupled_module` above; the ONE difference
/// is that `B.total` closes the cross-scope read back onto `A.total`, so the
/// two aggregate cells depend on each other WITHOUT passing through a solver
/// auto. Every other edge is identical, which is what makes the pair a clean A/B
/// against BT-4's admissible fixture.
///
/// All four Let cells transitively read a cluster auto and feed the spanning
/// objective, so all four enter `build_dependent_cells`' stage-(e) node set; the
/// two `line_cost` cells are acyclic and must survive, the two `total` cells are
/// the cycle and must be dropped AND reported.
///
/// Invisible to every pre-existing emitter: `detect_let_cycle` and
/// `build_combined_param_let_graph` are per-template, and within A alone
/// `line_cost → total` is a plain chain (likewise within B). Only the
/// cross-template builder can see the cycle.
fn cross_scope_value_cycle_cluster_module() -> CompiledModule {
    let a = TopologyTemplateBuilder::new("A")
        .auto_param("A", "k", Type::length())
        .param(
            "A",
            "unit",
            Type::dimensionless_scalar(),
            Some(literal(Value::Real(2.0))),
        )
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
        // CROSS-SCOPE read of B.total — one half of the inadmissible cycle.
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
        // 2-cycle edge: guarantees the {A, B} MergedSolve cluster.
        .constraint("A", 0, None, gt(value_ref("B", "m"), literal(mm(0.0))))
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
        // CROSS-SCOPE read back onto A.total — CLOSES the cycle. This is the
        // ONLY divergence from BT-4's admissible `two_cycle_cross_scope_coupled_module`.
        .let_binding(
            "B",
            "total",
            Type::length(),
            binop(
                BinOp::Add,
                value_ref("B", "line_cost"),
                value_ref("A", "total"),
            ),
        )
        .constraint("B", 0, None, gt(value_ref("A", "k"), literal(mm(0.0))))
        .build();

    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(a)
        .template(b)
        .build()
}

/// BT-6(b) — the OBSERVABLE TWIN of step-7's unit-level assertion: a cross-scope
/// value cycle inside a would-be cluster surfaces the guardrail's
/// `DiagnosticCode::EvalCycle` error all the way out through `Engine::eval`'s
/// `EvalResult.diagnostics` — i.e. it reaches the user through `reify eval`, not
/// just the private `build_dependent_cells` helper.
///
/// A `SpyConstraintSolver` (this file's established idiom) keeps the run
/// deterministic; the solver choice is immaterial to what is being asserted,
/// because the guardrail fires while the solver PROBLEM is being built, strictly
/// upstream of any solve.
///
/// RED before step-8: `topological_sort` is Kahn's — it returns a `Vec`, cannot
/// error, and silently omits cycle members — and this cross-template cycle is
/// out of reach of all three per-template emitters, so it is swallowed with ZERO
/// user-visible signal.
#[test]
fn bt6b_cross_scope_value_cycle_surfaces_eval_cycle_through_engine_eval() {
    let module = cross_scope_value_cycle_cluster_module();

    let mut solved = HashMap::new();
    solved.insert(ValueCellId::new("A", "k"), mm(3.0));
    solved.insert(ValueCellId::new("B", "m"), mm(7.0));

    let spy = SpyConstraintSolver::new_solved(solved);
    let captured = spy.captured_problem();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let result = engine.eval(&module);

    // ---- the ERROR reaches EvalResult.diagnostics, coded EvalCycle. ----

    let cycle_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.code == Some(DiagnosticCode::EvalCycle))
        .collect();
    assert_eq!(
        cycle_errors.len(),
        1,
        "the inadmissible cross-scope value cycle must surface EXACTLY ONE \
         Severity::Error coded DiagnosticCode::EvalCycle through \
         `Engine::eval` — the guardrail has to reach the user, not just the \
         private builder; got {:#?}",
        result.diagnostics,
    );

    // FULLY-QUALIFIED ids: this detector is cross-template, so the bare member
    // name `detect_let_cycle` uses would be ambiguous (`A.total` vs `B.total`).
    let msg = &cycle_errors[0].message;
    for want in ["A.total", "B.total"] {
        assert!(
            msg.contains(want),
            "the cycle diagnostic must name the cyclic cell by its \
             FULLY-QUALIFIED id `{want}`; got {msg:?}",
        );
    }

    // ---- the dropped cycle members never reach the fold. ----

    let problem = captured
        .lock()
        .unwrap()
        .clone()
        .expect("the two-cycle {A,B} SCC must dispatch ONE merged solve");
    let ids: Vec<ValueCellId> = problem
        .dependent_cells
        .iter()
        .map(|(id, _)| id.clone())
        .collect();

    for dropped in [ValueCellId::new("A", "total"), ValueCellId::new("B", "total")] {
        assert!(
            !ids.contains(&dropped),
            "the cycle member {dropped:?} must be ABSENT from dependent_cells — \
             β's per-trial fold and α's post-solve write-back must never see a \
             cycle member; got {ids:?}",
        );
    }
    for kept in [
        ValueCellId::new("A", "line_cost"),
        ValueCellId::new("B", "line_cost"),
    ] {
        assert!(
            ids.contains(&kept),
            "the ACYCLIC remainder {kept:?} must survive the guardrail — it \
             rejects the cycle, it does not discard the whole coupled set; got \
             {ids:?}",
        );
    }
}

// ---------------------------------------------------------------------------
// [JOINT-DRIVE β] (task #5189 step-14) — BT-12 (PRD §7; renumbered from a
// §7-collision by task #5764), per-sub PARAMETER OVERRIDES.
//
// PRD: docs/prds/v0_6/whole-model-joint-drive-seam.md §12.
// ---------------------------------------------------------------------------

/// The joint-drive model with a per-sub PARAMETER OVERRIDE on the child.
///
/// Deliberately an INLINE source rather than an edit to the shipped
/// `examples/whole_model_joint_drive.ri`: changing that file would perturb
/// BT-5's hand-derived arithmetic and re-trigger the three example
/// auto-enrolling gates (`examples_smoke`, the determinism walk, the
/// no-bare-`Scalar` corpus check) for no benefit.
///
/// `unit_cost: 0.90USD` against a template default of `0.50USD` — an 80% gap, so
/// an override-honouring fold and a template-default fold are unmistakably
/// different numbers at any non-zero solved quantity.
const OVERRIDE_SRC: &str = r#"
module joint_drive_sub_override

structure def Rivet : Costed {
    param supplier          : String = "Acme Fastener"
    param part_number       : String = "R-4210"
    param unit_cost         : Money  = 0.50USD
    param lead_time         : Time   = 24h

    param quantity_produced : Real   = auto(free)
    constraint quantity_produced >= 0.0
    constraint quantity_produced <= 100.0
}

structure RivetedPanel {
    sub rivets = Rivet(unit_cost: 0.90USD)

    minimize cost(self.descendants)

    let total_cost : Money = cost(self.descendants)
}
"#;

/// BT-12(b) — the per-sub override must reach the folded instance-path cost
/// cell that the expanded objective actually reads.
///
/// The DSL supports per-sub parameter overrides (`examples/auto_binding_sites.ri`,
/// `examples/bom_lifecycle.ri`), but stage (g) of `build_dependent_cells` emits
/// the instance-path ALIAS entry as a VERBATIM clone of the template-keyed
/// `default_expr`. Its inner reads therefore stay template-keyed, so
/// `RivetedPanel.rivets.line_cost` folds to `Rivet.unit_cost × q` — the TEMPLATE
/// DEFAULT — with two silent consequences: the per-trial fold makes the solver
/// minimise the wrong objective, and `materialize_dependent_cells` writes that
/// same wrong number back as `Determined`, clobbering the correctly-elaborated
/// per-instance value.
///
/// Asserted as a RELATIONSHIP (`line_cost == unit_cost_override × solved_q`),
/// never a hand-computed absolute: the auto is solver-determined, so a literal
/// would re-break on any solver retuning while the invariant would not.
#[test]
fn bt11_per_sub_parameter_override_reaches_the_folded_instance_path_cost() {
    let result = eval_ri_with_real_solver(OVERRIDE_SRC, "per-sub override");

    let scoped_unit_cost = ValueCellId::new("RivetedPanel.rivets", "unit_cost");
    let template_unit_cost = ValueCellId::new("Rivet", "unit_cost");

    let override_cost = scalar_si(&result, &scoped_unit_cost, "per-sub override");
    let default_cost = scalar_si(&result, &template_unit_cost, "per-sub override");

    // Fixture integrity — if elaboration collapsed the two spellings onto one
    // value there would be nothing to distinguish, and every assertion below
    // would pass vacuously.
    assert_ne!(
        override_cost, default_cost,
        "fixture integrity: the instance-path `RivetedPanel.rivets.unit_cost` \
         must carry the OVERRIDE (0.90USD) while the template-keyed \
         `Rivet.unit_cost` keeps its default (0.50USD); both read {override_cost}",
    );

    let solved_q = scalar_si(
        &result,
        &ValueCellId::new("Rivet", "quantity_produced"),
        "per-sub override",
    );
    // A zero auto would make the two candidate products equal and the
    // discriminating assertion vacuous.
    assert!(
        solved_q != 0.0,
        "fixture integrity: the solved auto must be non-zero for the \
         override-vs-default products to differ; got {solved_q}",
    );

    let aliased = scalar_si(&result, &ValueCellId::new("RivetedPanel.rivets", "line_cost"), "per-sub override");

    assert_eq!(
        aliased,
        override_cost * solved_q,
        "BT-12: the instance-path cost cell the expanded objective reads must \
         fold against the PER-SUB OVERRIDE ({override_cost} × {solved_q}), not \
         the template default ({default_cost} × {solved_q} = {}). A \
         template-default value here means stage (g) emitted the alias as a \
         verbatim clone of the template-keyed `default_expr`, so the override \
         was silently ignored at every trial point AND clobbered in the \
         post-solve write-back.",
        default_cost * solved_q,
    );
}

// ---------------------------------------------------------------------------
// [M-WHOLE ε] (task #5017) — BT3/BT4 CI integration gate: the SHIPPED
// `examples/whole_model_cost_min.ri`, a genuinely WHOLE-model cluster of TWO
// coupled children under ONE parent cost objective.
//
// PRD: docs/prds/v0_6/whole-model-objective-coupling.md §1.1/§6 (BT3, BT4);
// docs/prds/v0_6/whole-model-joint-drive-seam.md.
//
// NAMING TRAP: this file already has `bt3_objective_and_constraint_position_
// cost_is_expanded`, which is the *joint-drive-seam PRD's* (#5188/#5189) own
// "BT-3" (an objective/constraint-position `cost()` expansion check) — a
// DIFFERENT boundary test from M-WHOLE's BT3 (the cross-scope solved-auto
// surface read). Every M-WHOLE test below is prefixed `mwhole_` so the two
// unrelated BT-numbering schemes never collide in a reader's mind.
// ---------------------------------------------------------------------------

/// The SHIPPED M-WHOLE ε example, read from disk rather than copied into a
/// fixture string — so this test degrades loudly if the published example
/// drifts away from the behaviour its own header comment advertises.
///
/// Idiom: this file's own `JOINT_DRIVE_EXAMPLE_PATH`;
/// `crates/reify-eval/tests/continuous_cost_min_example_e2e.rs`.
const WHOLE_MODEL_COST_MIN_EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/whole_model_cost_min.ri"
);

/// Shared preamble for every `mwhole_*` test below: read the shipped M-WHOLE ε
/// example from disk, derive its frozen-cascade counterpart via
/// `strip_inlined_minimize`, and evaluate BOTH halves through the REAL
/// `DimensionalSolver`. All three `mwhole_*` tests need exactly this pair, so
/// centralising it keeps the read+strip+eval mechanics — and the "run the
/// next impl step" panic message — a single source of truth instead of three
/// literal copies.
fn mwhole_halves() -> (EvalResult, EvalResult) {
    let merged_src =
        std::fs::read_to_string(WHOLE_MODEL_COST_MIN_EXAMPLE_PATH).unwrap_or_else(|e| {
            panic!(
                "Could not read {WHOLE_MODEL_COST_MIN_EXAMPLE_PATH}: {e} — run the next impl \
                 step to create the example file",
            )
        });
    let frozen_src = strip_inlined_minimize(&merged_src);

    let merged = eval_ri_with_real_solver(&merged_src, "merged (inlined `minimize` present)");
    let frozen = eval_ri_with_real_solver(&frozen_src, "frozen cascade (`minimize` removed)");
    (merged, frozen)
}

/// BT4(i) — the joint-drive signal generalised to TWO coupled children under
/// ONE parent objective (§6 BT1's shape). Unlike this file's
/// `bt5_parent_objective_drives_child_auto_strictly_below_the_frozen_cascade`
/// (a single-child, single-auto cluster), this fixture couples
/// `{CostAssembly, Plate, Spacer}` into ONE cluster of dimension 2 — the
/// signal BT-5 cannot show: a whole assembly, not one child.
///
/// Both assertions below are COMPARATIVE (strict inequality), never an
/// absolute converged value or a tuned tolerance — the house norm for
/// `.ri`-layer tests (see `bt5_...`'s doc comment for the full penalty-method
/// / seed-fallback rationale this fixture inherits unchanged).
///
/// # Achievability — DERIVED, not guessed
///
/// * FROZEN (baseline derived in-test by `strip_inlined_minimize`, never
///   transcribed): with the `minimize` stripped, NO cluster forms — each
///   child resolves ALONE, bottom-up, with live inequality constraints, so
///   `build_centrality_objective` synthesises a Chebyshev-centre objective
///   per child and each auto freezes at its own BOX CENTRE.
/// * MERGED: `compute_clusters`' union-find merges `{CostAssembly, Plate,
///   Spacer}` into ONE cluster (each child contributes an objective-read →
///   auto-owner edge), dimension 2 — far below `WHOLE_MODEL_CLUSTER_DIM_CAP`
///   (12). No centrality objective is synthesised (only synthesised when
///   `problem.objective.is_none()`), so cost-minimisation governs both autos
///   toward their lower brackets instead.
/// * `line_cost = unit_cost * quantity_produced` is strictly increasing in
///   each auto, so cost-minimisation drives both children's autos strictly
///   below their frozen box centres.
///
/// # Structure-keyed, not instance-path
///
/// The solver's `auto_params`, and hence its solved write-back, are keyed by
/// the DECLARING TEMPLATE (`ValueCellId::new(&structure.name, &param.name)`);
/// the instance-path spelling (e.g. `CostAssembly.plate.quantity_produced`)
/// is never written for a solver-resolved auto in EITHER half — the same gap
/// `bt5_...` documents at :1014-1019.
///
/// RED until `examples/whole_model_cost_min.ri` exists and both children's
/// boxes/unit_costs are tuned to produce a live, non-degenerate gap.
#[test]
fn mwhole_bt4_parent_objective_jointly_drives_both_child_autos_below_the_frozen_cascade() {
    let (merged, frozen) = mwhole_halves();

    // Both children, structure-keyed. A `for` loop (rather than two copies of
    // the same block) keeps the assertion — and its failure message — a
    // single source of truth for both children.
    for structure in ["Plate", "Spacer"] {
        let auto_id = ValueCellId::new(structure, "quantity_produced");
        let merged_q = scalar_si(&merged, &auto_id, "merged");
        let frozen_q = scalar_si(&frozen, &auto_id, "frozen-cascade");

        assert!(
            merged_q < frozen_q,
            "BT4(i) [{structure}]: the merged solve must drive this child's \
             auto STRICTLY BELOW the value the bottom-up frozen cascade pins \
             it at (cost-min argmin toward the box's lower bound vs. the \
             synthesised Chebyshev centre). Got merged={merged_q} vs \
             frozen={frozen_q} (difference {}). Equal values mean the \
             parent's objective never reached this child's auto at all.",
            frozen_q - merged_q,
        );
    }
}

/// BT4(ii) — the merged WHOLE-ASSEMBLY cost (summed across BOTH children) is
/// STRICTLY LESS than the frozen-cascade baseline. Reading BOTH children's
/// `line_cost` cells (not just one) is what makes this an assertion about the
/// WHOLE assembly rather than a single child.
///
/// Reads the STRUCTURE-KEYED `line_cost` cells — the same cells the sibling
/// test reads the accompanying auto from — and sums them for BOTH halves.
/// Reading the SAME cells on both sides keeps the comparison apples-to-apples,
/// per `bt5_...`'s (ii) note.
///
/// # Achievability — DERIVED, not guessed
///
/// `line_cost = unit_cost * quantity_produced` is strictly increasing in each
/// auto. The sibling BT4(i) test establishes each child's merged auto sits
/// strictly below its own frozen-cascade freeze, so each child's merged
/// `line_cost` is strictly below its frozen `line_cost`; the sum of two
/// strictly-smaller non-negative numbers is strictly smaller than the sum of
/// their frozen counterparts.
///
/// # What this value-level assertion does NOT discriminate
///
/// This SUM comparison, by itself, does not distinguish ONE merged cluster
/// spanning both children from a hypothetical regression to TWO independent
/// single-child clusters that each still carried a copy of the parent's
/// objective: because the merged figures are the solver's `initially_feasible`
/// SEED rather than a converged argmin (see `bt5_...`'s "eval-layer
/// convergence boundary" note), either cluster shape would drive both autos to
/// the same 0.01 seed and satisfy this assertion — and the sibling BT4(i) —
/// identically. A mis-expanded or wrong-sense objective would likewise still
/// suppress the synthesised centrality objective and still land both autos at
/// 0.01.
///
/// That structural claim — a parent plus TWO children sharing ONE spanning
/// objective union into EXACTLY ONE cluster, never two — is proven at the
/// unit level, independently of this `.ri`-level gate, by
/// `aggregate_objective_forms_single_spanning_cluster` (α-BT1,
/// `crates/reify-eval/src/resolve_order.rs`), which asserts
/// `ro.clusters.len() == 1` and `cluster.scopes == [Parent, ChildA, ChildB]`
/// for exactly this acyclic aggregate-objective shape. This test's job is
/// narrower and complementary: confirm the WHOLE CHAIN — cluster formation,
/// per-trial fold, the REAL `DimensionalSolver`'s convergence direction, and
/// write-back — produces the right end-to-end SUMMED numeric behaviour on the
/// shipped example, not re-derive the cluster-shape discrimination the unit
/// test already owns.
///
/// RED until the example's boxes/unit_costs produce a live, positive gap in
/// the SUMMED total (not merely per-child).
#[test]
fn mwhole_bt4_merged_whole_assembly_cost_is_strictly_below_the_frozen_baseline() {
    let (merged, frozen) = mwhole_halves();

    let whole_assembly_cost = |result: &EvalResult, what: &str| -> f64 {
        let plate = scalar_si(result, &ValueCellId::new("Plate", "line_cost"), what);
        let spacer = scalar_si(result, &ValueCellId::new("Spacer", "line_cost"), what);
        plate + spacer
    };

    let merged_total = whole_assembly_cost(&merged, "merged");
    let frozen_total = whole_assembly_cost(&frozen, "frozen-cascade");

    assert!(
        merged_total < frozen_total,
        "BT4(ii): the merged WHOLE-ASSEMBLY cost (Plate.line_cost + \
         Spacer.line_cost) must be STRICTLY LESS than the bottom-up \
         frozen-cascade baseline — that gap IS the user-observable \
         whole-model joint-drive signal, and it must span BOTH children, not \
         just one. Got merged={merged_total} vs frozen={frozen_total} \
         (saving {}).",
        frozen_total - merged_total,
    );
}

/// BT3 core — the cross-scope SURFACE-SPELLING read (`self.plate.line_cost`,
/// as a design author would write inside `CostAssembly`) surfaces the
/// CO-SOLVED value — not a frozen one, and not `Undef`.
///
/// A `.ri` read `self.plate.line_cost` written inside `CostAssembly` compiles
/// to the INSTANCE-PATH id `ValueCellId::new("CostAssembly.plate",
/// "line_cost")` — the compiler mints `format!("{}.{}", scope.entity_name,
/// sub_name)` (`crates/reify-compiler/src/expr.rs:843`).
///
/// PRD: docs/prds/v0_6/whole-model-objective-coupling.md §1.1/§6 (BT3) — "a
/// scope that reads another scope's solved `auto` cell surfaces the
/// co-solved value under `reify eval`" (the F-inherit ζ #4826 deferral this
/// PRD homes).
///
/// # Achievability — DIRECT, not assumed
///
/// Identical mechanism and identical fixture precondition as the
/// green-on-main `bt5_...`(iii), which asserts the MERGED-side alias for
/// `RivetedPanel.rivets.line_cost`; the fixture satisfies the
/// `build_single_instance_alias_paths` precondition (two DISTINCT
/// structures, one non-collection instance path each).
///
/// (a)+(d) together are BT3's substance: a scope reading another scope's
/// solved cell surfaces the co-solved value — not a frozen one, and not
/// `Undef`. (b)/(c) are derived-identity sanity checks: both sides read from
/// the SAME eval, so they pin bit-identity of a freshly-folded product, not a
/// convergence claim.
///
/// # Where the cross-scope read lives — the objective, not a `let`
///
/// `CostAssembly`'s `.ri`-level read of its children's `line_cost` is the
/// INLINED `minimize cost(self.descendants)`, which expands to
/// `[CostAssembly.plate.line_cost, CostAssembly.spacer.line_cost].sum` — that
/// expansion IS the cross-scope surface read BT3 names, and it is what mints
/// the instance-path ids this test reads.
///
/// Do NOT "strengthen" this fixture by adding a parent-level consumer binding
/// such as `let plate_cost_observed : Money = self.plate.line_cost`. Confirmed
/// by direct `EvalResult` inspection (esc-5017-7): such a binding is
/// permanently `Undef` in BOTH halves, because `build_dependent_cells` seeds
/// its walk FROM objective/constraint terms and sweeps only their own UPSTREAM
/// dependencies — a purely downstream consumer let is never swept in, and
/// `evaluate_let_bindings` has already run before `materialize_dependent_cells`
/// writes the solved values. Making that shape work is an engine change owned
/// by β/α, tracked separately (#5835); it is not a fixture-authoring gap.
#[test]
fn mwhole_bt3_cross_scope_surface_read_surfaces_the_co_solved_value() {
    let (merged, frozen) = mwhole_halves();

    for (instance_path, structure) in [
        ("CostAssembly.plate", "Plate"),
        ("CostAssembly.spacer", "Spacer"),
    ] {
        let alias_id = ValueCellId::new(instance_path, "line_cost");

        // (a) resolves to a Scalar in the merged eval -- not Undef.
        let merged_aliased = scalar_si(&merged, &alias_id, "merged");

        // (b) equals its structure-keyed source in the SAME eval -- proving
        // the alias is freshly refolded, not stale.
        let merged_structure_keyed = scalar_si(
            &merged,
            &ValueCellId::new(structure, "line_cost"),
            "merged",
        );
        assert_eq!(
            merged_aliased, merged_structure_keyed,
            "BT3 [{structure}] (b): the instance-path cell `{instance_path}.line_cost` \
             must hold the same freshly-folded value as its structure-keyed source \
             `{structure}.line_cost` in the merged eval",
        );

        // (c) equals unit_cost * quantity_produced read from the SAME eval --
        // the Costed closed form, so the value is derived, not pinned. A
        // RELATIVE-epsilon comparison, not `assert_eq!`: exact f64 bit-equality
        // here only holds because today's fold happens to multiply the same
        // two SI magnitudes in the same order — a unit-conversion scaling,
        // reassociation, or FMA would break bit-identity with no behavioural
        // regression. (BT3(b) above, which compares two cells that must hold
        // the SAME folded value, is legitimately exact.)
        let merged_unit_cost = scalar_si(
            &merged,
            &ValueCellId::new(structure, "unit_cost"),
            "merged",
        );
        let merged_q = scalar_si(
            &merged,
            &ValueCellId::new(structure, "quantity_produced"),
            "merged",
        );
        let expected_aliased = merged_unit_cost * merged_q;
        let tolerance = 1e-12 * expected_aliased.abs().max(1.0);
        let diff = (merged_aliased - expected_aliased).abs();
        assert!(
            diff <= tolerance,
            "BT3 [{structure}] (c): the instance-path cell must be the Costed \
             closed form unit_cost * quantity_produced ({merged_unit_cost} * \
             {merged_q} = {expected_aliased}), refolded against the solved \
             auto — not left at whatever it held before the solve. Got \
             {merged_aliased} (difference {diff} exceeds tolerance {tolerance}).",
        );

        // (d) is STRICTLY BELOW the value the frozen cascade freezes this
        // child's cost at -- proving the surface read carries the CO-SOLVED
        // value, not the bottom-up freeze.
        //
        // The frozen side is read STRUCTURE-KEYED (`{structure}.line_cost`),
        // NOT through `alias_id`, and that asymmetry is load-bearing rather
        // than a convenience. The instance-path cell `{instance_path}.line_cost`
        // exists ONLY as a `build_dependent_cells` alias entry, and that entry
        // is seeded from the objective/constraint terms of a merged cluster
        // (`build_dependent_cells(seed_exprs: &[&CompiledExpr], ..)`,
        // engine_eval.rs:1611). `strip_inlined_minimize` removes the sole
        // `minimize`, so the frozen half forms no cluster, mints no alias, and
        // `CostAssembly.plate.line_cost` is Undef there BY CONSTRUCTION -- for
        // any fixture whose parent owns no `auto` of its own, which BT3/BT4's
        // pure-cost-aggregator parent must not (a parent auto would make
        // `build_solver_problem` take a different path entirely). Reading
        // `alias_id` on the frozen side is therefore not a stricter assertion,
        // it is an unsatisfiable one. The green-on-main precedent this test
        // cites, `bt5_..`(iii), reads the alias in the MERGED eval only, for
        // exactly this reason.
        //
        // `{structure}.line_cost` IS the frozen cascade's own spelling of this
        // child's cost (25.00 USD / 45.00 USD -- the box-centre freeze the
        // header derives), so comparing against it discharges BT3's "not a
        // frozen [value]" clause against the real frozen number rather than
        // against an absent cell. "Not Undef" is discharged by (a), which
        // panics via `scalar_si` on anything but a Scalar.
        //
        // STRICT INEQUALITY, not `assert_ne!`: the house comparative norm, and
        // it pins the cost-minimising DIRECTION -- a merged value that merely
        // differed (e.g. drifted upward) would satisfy `!=` while contradicting
        // the whole point of the objective.
        let frozen_structure_keyed = scalar_si(
            &frozen,
            &ValueCellId::new(structure, "line_cost"),
            "frozen-cascade",
        );
        assert!(
            merged_aliased < frozen_structure_keyed,
            "BT3 [{structure}] (d): the cross-scope surface read \
             `{instance_path}.line_cost` must surface the CO-SOLVED value — \
             STRICTLY BELOW the `{structure}.line_cost` the frozen cascade \
             freezes this child at. An equal-or-greater value would mean the \
             surface read carries the bottom-up freeze, not the co-solved \
             value. Got merged={merged_aliased} vs frozen={frozen_structure_keyed}",
        );
    }
}
