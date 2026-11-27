//! Which cells an expression sees from inside a local binder scope.
//!
//! Two kinds of scope exist. A `match` arm's binders and a quantifier's bound
//! variable EXTEND the caller's scope, so the caller's store cells stay
//! visible beside them. A user function's body, a lambda's body and a
//! structure ctor's member expressions REPLACE it, so a store cell the caller
//! holds reads `Undef` there unless it was bound (a param, a `let`, a capture,
//! a sibling member).
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

fn eval_in_f(function: CompiledFunction, arg: i64) -> Value {
    let values = store();
    let functions = [function];
    eval_expr(&call_f(arg), &EvalContext::new(&values, &functions))
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

fn construct_b(
    defaults: Vec<(&str, CompiledExpr)>,
    lets: Vec<(&str, CompiledExpr)>,
) -> StructureInstanceData {
    let named = |members: Vec<(&str, CompiledExpr)>| {
        members
            .into_iter()
            .map(|(name, expr)| (name.to_string(), expr))
            .collect()
    };
    let ctor = CompiledExpr::structure_instance_ctor(
        reify_ir::StructureTypeId(0),
        STRUCT.to_string(),
        1,
        vec![],
        named(defaults),
        named(lets),
        Vec::new(),
        None,
        Type::StructureRef(STRUCT.to_string()),
    );
    let mut values = ValueMap::new();
    values.insert(ValueCellId::new(STRUCT, "ghost"), Value::Int(STORE_VALUE));
    match eval_expr(&ctor, &EvalContext::simple(&values)) {
        Value::StructureInstance(data) => *data,
        other => panic!("ctor must build a StructureInstance, got {other:?}"),
    }
}

#[test]
fn a_ctor_member_does_not_see_a_caller_cell_named_like_a_non_member() {
    let data = construct_b(vec![("n", read(ValueCellId::new(STRUCT, "ghost")))], vec![]);
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

/// `exists e in [4, 5]: e == Probe::x` is true only if the predicate reads both
/// the bound element and the store cell.
fn exists_element_equal_to_store_cell() -> CompiledExpr {
    let element = ValueCellId::new("$quant", "e");
    CompiledExpr::quantifier(
        QuantifierKind::Exists,
        "e".to_string(),
        element.clone(),
        list_expr(vec![int(4), int(5)]),
        equals(read(element), read(store_id())),
    )
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
