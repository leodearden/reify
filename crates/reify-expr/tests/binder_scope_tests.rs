//! Which cells an expression sees from inside a local binder scope.
//!
//! Two kinds of scope exist. A `match` arm's binders and a quantifier's bound
//! variable EXTEND the caller's scope, so the caller's store cells stay
//! visible beside them. A user function's body, a lambda's body and a
//! structure ctor's member expressions REPLACE it, so a store cell the caller
//! holds reads `Undef` there unless it was bound (a param, a `let`, a capture,
//! a sibling member).
//!
//! A replacing scope opened inside an extending one (a function called in a
//! match arm, a lambda applied in a quantifier predicate) replaces that one
//! too: it sees neither the arm's binder or the bound element nor the store.
//!
//! These tests pin that visibility through the public evaluator only, so any
//! rework of how scopes are built has to keep it.

use reify_ast::QuantifierKind;
use reify_core::{Diagnostic, Type, ValueCellId};
use reify_expr::{EvalContext, apply_lambda, eval_expr};
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, CompiledFunction, CompiledMatchArm, CompiledPattern,
    DeterminacyState, PersistentMap, StructureInstanceData, Value, ValueMap,
};
use reify_test_support::builders::{compiled_function, list_expr, std_call, user_fn_call};

const STORE_VALUE: i64 = 5;
const BINDER_VALUE: i64 = 7;

fn store_id() -> ValueCellId {
    ValueCellId::new("Probe", "x")
}

fn binder_id() -> ValueCellId {
    ValueCellId::new("$matcharm0.Wrap", "v")
}

fn param_id() -> ValueCellId {
    ValueCellId::new("f", "a")
}

/// A caller scope holding one store cell, `Probe::x = 5`.
fn store() -> ValueMap {
    let mut values = ValueMap::new();
    values.insert(store_id(), Value::Int(STORE_VALUE));
    values
}

fn int(n: i64) -> CompiledExpr {
    CompiledExpr::literal(Value::Int(n), Type::Int)
}

fn read(id: ValueCellId) -> CompiledExpr {
    CompiledExpr::value_ref(id, Type::Int)
}

fn add(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Add, left, right, Type::Int)
}

fn equals(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Eq, left, right, Type::Bool)
}

/// `match Wrap.Some { v: 7 } { Some { v } => body }`, binding `v` to `binder`.
fn match_binding(binder: ValueCellId, body: CompiledExpr) -> CompiledExpr {
    let discriminant = Value::Enum {
        type_name: "Wrap".to_string(),
        variant: "Some".to_string(),
        payload: vec![("v".to_string(), Value::Int(BINDER_VALUE))],
    };
    let arm = CompiledMatchArm {
        patterns: vec![CompiledPattern::VariantBind {
            name: "Some".to_string(),
            binders: vec![("v".to_string(), binder)],
        }],
        body,
    };
    CompiledExpr::match_expr(
        CompiledExpr::literal(discriminant, Type::Enum("Wrap".to_string())),
        vec![arm],
        Type::Int,
    )
}

/// `fn f(a: Int) = body`, so `a` is the cell [`param_id`].
fn function_f(lets: Vec<(&str, CompiledExpr)>, body: CompiledExpr) -> CompiledFunction {
    let lets = lets
        .into_iter()
        .map(|(name, expr)| (name.to_string(), expr))
        .collect();
    compiled_function("f", &[("a", Type::Int)], lets, body)
}

fn call_f(arg: i64) -> CompiledExpr {
    user_fn_call("f", vec![int(arg)], Type::Int)
}

fn eval_with_f(function: CompiledFunction, expr: &CompiledExpr) -> Value {
    let values = store();
    let functions = [function];
    eval_expr(expr, &EvalContext::new(&values, &functions))
}

fn eval_in_f(function: CompiledFunction, arg: i64) -> Value {
    eval_with_f(function, &call_f(arg))
}

// ── (a) a user function body sees its params and lets, never the caller's cells

#[test]
fn a_function_body_does_not_see_a_store_cell_of_its_caller() {
    assert_eq!(
        eval_in_f(function_f(vec![], read(store_id())), 1),
        Value::Undef
    );
}

#[test]
fn a_function_body_sees_its_own_param() {
    assert_eq!(
        eval_in_f(function_f(vec![], read(param_id())), 3),
        Value::Int(3)
    );
}

#[test]
fn a_function_body_sees_an_earlier_let() {
    let function = function_f(
        vec![("b", add(read(param_id()), int(10)))],
        read(ValueCellId::new("f", "b")),
    );
    assert_eq!(eval_in_f(function, 3), Value::Int(13));
}

// ── (b) a lambda body sees its captures and params, never the applier's cells

fn lambda_reading_store(captures: ValueMap) -> Value {
    Value::Lambda {
        params: vec![],
        body: Box::new(read(store_id())),
        captures,
    }
}

#[test]
fn a_lambda_body_does_not_see_an_uncaptured_store_cell_of_its_applier() {
    let values = store();
    let lambda = lambda_reading_store(ValueMap::new());
    assert_eq!(
        apply_lambda(&lambda, &[], &EvalContext::simple(&values)),
        Value::Undef
    );
}

#[test]
fn a_lambda_body_reads_a_captured_cell_from_its_captures() {
    let values = store();
    let mut captures = ValueMap::new();
    captures.insert(store_id(), Value::Int(9));
    let lambda = lambda_reading_store(captures);
    assert_eq!(
        apply_lambda(&lambda, &[], &EvalContext::simple(&values)),
        Value::Int(9),
        "the captured value, not the applier's"
    );
}

// ── (c) a ctor member sees its siblings, never the caller's cells

const STRUCT: &str = "B";

fn ctor_b(defaults: Vec<(&str, CompiledExpr)>, lets: Vec<(&str, CompiledExpr)>) -> CompiledExpr {
    let named = |members: Vec<(&str, CompiledExpr)>| {
        members
            .into_iter()
            .map(|(name, expr)| (name.to_string(), expr))
            .collect()
    };
    CompiledExpr::structure_instance_ctor(
        reify_ir::StructureTypeId(0),
        STRUCT.to_string(),
        1,
        vec![],
        named(defaults),
        named(lets),
        Vec::new(),
        None,
        Type::StructureRef(STRUCT.to_string()),
    )
}

fn ghost_id() -> ValueCellId {
    ValueCellId::new(STRUCT, "ghost")
}

/// Evaluate `expr`, which must build a `B`, over a store holding `B::ghost = 5`.
fn construct(expr: &CompiledExpr) -> StructureInstanceData {
    let mut values = ValueMap::new();
    values.insert(ghost_id(), Value::Int(STORE_VALUE));
    match eval_expr(expr, &EvalContext::simple(&values)) {
        Value::StructureInstance(data) => *data,
        other => panic!("ctor must build a StructureInstance, got {other:?}"),
    }
}

fn construct_b(
    defaults: Vec<(&str, CompiledExpr)>,
    lets: Vec<(&str, CompiledExpr)>,
) -> StructureInstanceData {
    construct(&ctor_b(defaults, lets))
}

#[test]
fn a_ctor_member_does_not_see_a_caller_cell_named_like_a_non_member() {
    let data = construct_b(vec![("n", read(ghost_id()))], vec![]);
    assert_eq!(data.fields.get("n"), Some(&Value::Undef));
}

#[test]
fn a_ctor_member_sees_a_sibling_member() {
    let data = construct_b(
        vec![("n", read(ValueCellId::new(STRUCT, "k")))],
        vec![("k", int(3))],
    );
    assert_eq!(data.fields.get("n"), Some(&Value::Int(3)));
}

// ── (d) a match arm sees its binders AND the caller's cells

#[test]
fn a_match_arm_sees_its_binder_and_a_store_cell_of_its_caller() {
    let values = store();
    let expr = match_binding(binder_id(), add(read(binder_id()), read(store_id())));
    assert_eq!(
        eval_expr(&expr, &EvalContext::simple(&values)),
        Value::Int(BINDER_VALUE + STORE_VALUE)
    );
}

#[test]
fn a_match_binder_shadows_a_store_cell_with_the_same_id() {
    let values = store();
    let expr = match_binding(store_id(), read(store_id()));
    assert_eq!(
        eval_expr(&expr, &EvalContext::simple(&values)),
        Value::Int(BINDER_VALUE)
    );
}

// ── (e) a value-iteration quantifier sees its element AND the caller's cells

fn element_id() -> ValueCellId {
    ValueCellId::new("$quant", "e")
}

/// `exists e in [4, 5]: e == right`, with `e` the cell [`element_id`].
fn exists_element_equal_to(right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::quantifier(
        QuantifierKind::Exists,
        "e".to_string(),
        element_id(),
        list_expr(vec![int(4), int(5)]),
        equals(read(element_id()), right),
    )
}

/// `exists e in [4, 5]: e == Probe::x` is true only if the predicate reads both
/// the bound element and the store cell.
fn exists_element_equal_to_store_cell() -> CompiledExpr {
    exists_element_equal_to(read(store_id()))
}

#[test]
fn a_quantifier_predicate_sees_its_element_and_a_store_cell_of_its_caller() {
    let values = store();
    assert_eq!(
        eval_expr(
            &exists_element_equal_to_store_cell(),
            &EvalContext::simple(&values)
        ),
        Value::Bool(true)
    );
}

#[test]
fn a_quantifier_predicate_with_a_determinacy_snapshot_sees_its_element_and_a_store_cell() {
    let values = store();
    let determinacy: PersistentMap<ValueCellId, (Value, DeterminacyState)> = PersistentMap::new();
    let ctx = EvalContext::simple(&values).with_determinacy(&determinacy);
    assert_eq!(
        eval_expr(&exists_element_equal_to_store_cell(), &ctx),
        Value::Bool(true)
    );
}

// ── (f) an open scope nested in a sealed one extends the sealed one only

#[test]
fn a_match_arm_in_a_function_body_sees_its_binder_and_the_param() {
    let function = function_f(
        vec![],
        match_binding(binder_id(), add(read(binder_id()), read(param_id()))),
    );
    assert_eq!(eval_in_f(function, 3), Value::Int(BINDER_VALUE + 3));
}

#[test]
fn a_match_arm_in_a_function_body_does_not_see_a_store_cell_of_the_caller() {
    let function = function_f(vec![], match_binding(binder_id(), read(store_id())));
    assert_eq!(eval_in_f(function, 3), Value::Undef);
}

// ── (g) the selector hook is handed the current scope

/// Stand-in for the kernel-free selector hook: answers the calls `probe_store`,
/// `probe_binder` and `probe_param` with the value the scope it was handed
/// holds for that cell, so a test can tell which scope the hook saw.
fn scope_probe(expr: &CompiledExpr, values: &ValueMap, _: &mut Vec<Diagnostic>) -> Option<Value> {
    let CompiledExprKind::FunctionCall { function, .. } = &expr.kind else {
        return None;
    };
    let id = match function.name.as_str() {
        "probe_store" => store_id(),
        "probe_binder" => binder_id(),
        "probe_param" => param_id(),
        _ => return None,
    };
    Some(values.get(&id).cloned().unwrap_or(Value::Undef))
}

/// A call no builtin knows, so it evaluates to Undef and the hook is asked.
fn probe(name: &str) -> CompiledExpr {
    std_call(name, vec![], Type::Int)
}

fn eval_with_hook(expr: &CompiledExpr, functions: &[CompiledFunction]) -> Value {
    let values = store();
    let ctx = EvalContext::new(&values, functions).with_symbolic_selector_ctor(scope_probe);
    eval_expr(expr, &ctx)
}

#[test]
fn the_hook_at_top_level_sees_the_store() {
    assert_eq!(
        eval_with_hook(&probe("probe_store"), &[]),
        Value::Int(STORE_VALUE)
    );
}

#[test]
fn the_hook_in_a_match_arm_sees_the_store_and_the_binder() {
    assert_eq!(
        eval_with_hook(&match_binding(binder_id(), probe("probe_store")), &[]),
        Value::Int(STORE_VALUE)
    );
    assert_eq!(
        eval_with_hook(&match_binding(binder_id(), probe("probe_binder")), &[]),
        Value::Int(BINDER_VALUE)
    );
}

#[test]
fn the_hook_in_a_function_body_sees_the_param_and_not_the_store() {
    let store_probe = [function_f(vec![], probe("probe_store"))];
    assert_eq!(eval_with_hook(&call_f(3), &store_probe), Value::Undef);
    let param_probe = [function_f(vec![], probe("probe_param"))];
    assert_eq!(eval_with_hook(&call_f(3), &param_probe), Value::Int(3));
}

// ── (h) a sealed scope nested in an open one sees neither the open one nor the store

#[test]
fn a_function_called_in_a_match_arm_sees_neither_the_arm_binder_nor_a_store_cell() {
    let in_arm = match_binding(binder_id(), call_f(3));
    assert_eq!(
        eval_with_f(function_f(vec![], read(binder_id())), &in_arm),
        Value::Undef,
        "the arm binder"
    );
    assert_eq!(
        eval_with_f(function_f(vec![], read(store_id())), &in_arm),
        Value::Undef,
        "the store cell"
    );
}

#[test]
fn a_function_called_in_a_match_arm_is_handed_the_arm_binder_as_an_argument() {
    let in_arm = match_binding(
        binder_id(),
        user_fn_call("f", vec![read(binder_id())], Type::Int),
    );
    assert_eq!(
        eval_with_f(function_f(vec![], read(param_id())), &in_arm),
        Value::Int(BINDER_VALUE)
    );
}

#[test]
fn a_ctor_built_in_a_match_arm_sees_neither_the_arm_binder_nor_a_store_cell() {
    let ctor = ctor_b(
        vec![("n", read(binder_id())), ("g", read(ghost_id()))],
        vec![],
    );
    let data = construct(&match_binding(binder_id(), ctor));
    assert_eq!(data.fields.get("n"), Some(&Value::Undef), "the arm binder");
    assert_eq!(data.fields.get("g"), Some(&Value::Undef), "the store cell");
}

/// `map_or(some(0), 0, |p| body)`, the lambda capturing `captures`: a call that
/// applies the lambda in the scope the call is evaluated in.
fn apply_lambda_reading(body: CompiledExpr, captures: Vec<ValueCellId>) -> CompiledExpr {
    let p = ValueCellId::new("$lambda0", "p");
    let lambda = CompiledExpr::lambda(
        vec![("p".to_string(), None)],
        vec![p],
        body,
        captures,
        Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::Int),
        },
    );
    let some_zero = CompiledExpr::option_some(int(0), Type::Option(Box::new(Type::Int)));
    user_fn_call("map_or", vec![some_zero, int(0), lambda], Type::Int)
}

fn eval_over_store(expr: &CompiledExpr) -> Value {
    let values = store();
    eval_expr(expr, &EvalContext::simple(&values))
}

#[test]
fn a_lambda_applied_in_a_quantifier_predicate_sees_neither_the_element_nor_a_store_cell() {
    let reads_element = exists_element_equal_to(apply_lambda_reading(read(element_id()), vec![]));
    assert_eq!(eval_over_store(&reads_element), Value::Undef, "the element");
    let reads_store = exists_element_equal_to(apply_lambda_reading(read(store_id()), vec![]));
    assert_eq!(
        eval_over_store(&reads_store),
        Value::Undef,
        "the store cell"
    );
}

#[test]
fn a_lambda_applied_in_a_quantifier_predicate_sees_the_element_it_captured() {
    let captured =
        exists_element_equal_to(apply_lambda_reading(read(element_id()), vec![element_id()]));
    assert_eq!(eval_over_store(&captured), Value::Bool(true));
}

#[test]
fn the_hook_in_a_function_called_in_a_match_arm_sees_the_param_only() {
    let in_arm = match_binding(binder_id(), call_f(3));
    let binder_probe = [function_f(vec![], probe("probe_binder"))];
    assert_eq!(
        eval_with_hook(&in_arm, &binder_probe),
        Value::Undef,
        "the arm binder"
    );
    let store_probe = [function_f(vec![], probe("probe_store"))];
    assert_eq!(
        eval_with_hook(&in_arm, &store_probe),
        Value::Undef,
        "the store cell"
    );
    let param_probe = [function_f(vec![], probe("probe_param"))];
    assert_eq!(eval_with_hook(&in_arm, &param_probe), Value::Int(3));
}
