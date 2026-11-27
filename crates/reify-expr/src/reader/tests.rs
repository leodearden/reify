use std::sync::Arc;

use reify_ast::QuantifierKind;
use reify_core::{Diagnostic, FIELD_ENTITY_PREFIX, Type, ValueCellId};
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, CompiledFunction, CompiledMatchArm, CompiledPattern,
    DeterminacyState, FieldSourceKind, PersistentMap, Value, ValueMap,
};
use reify_test_support::builders::{list_expr, std_call};

use super::CellReader;
use crate::{BranchRecord, EvalContext, Seeds, apply_lambda, eval_dual, eval_expr};

fn cell(member: &str) -> ValueCellId {
    ValueCellId::new("Probe", member)
}

fn store() -> ValueMap {
    let mut store = ValueMap::new();
    store.insert(cell("x"), Value::Int(5));
    store.insert(cell("y"), Value::Int(6));
    store
}

fn entries(map: &ValueMap) -> Vec<(ValueCellId, Value)> {
    let mut entries: Vec<_> = map.iter().map(|(id, v)| (id.clone(), v.clone())).collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

#[test]
fn a_value_map_reader_reads_a_stored_cell_and_undef_for_any_other() {
    let store = store();
    let reader: &dyn CellReader = &store;
    assert_eq!(reader.read(&cell("x")), Value::Int(5));
    assert_eq!(reader.read(&cell("absent")), Value::Undef);
}

#[test]
fn a_value_map_reader_is_its_own_map() {
    let store = store();
    let reader: &dyn CellReader = &store;
    assert_eq!(entries(reader.as_value_map()), entries(&store));
}

// ── Every store read goes through the reader ───────────────────────────────
//
// Each test evaluates under a context whose `values` is EMPTY and whose reader
// holds the store. A read that bypassed the reader would see nothing, so the
// result would be Undef rather than the value the store-backed context gives.

fn int(n: i64) -> CompiledExpr {
    CompiledExpr::literal(Value::Int(n), Type::Int)
}

fn read(id: ValueCellId) -> CompiledExpr {
    CompiledExpr::value_ref(id, Type::Int)
}

fn binop(op: BinOp, left: CompiledExpr, right: CompiledExpr, ty: Type) -> CompiledExpr {
    CompiledExpr::binop(op, left, right, ty)
}

fn binder() -> ValueCellId {
    ValueCellId::new("$matcharm0.Wrap", "v")
}

/// `match Wrap.Some { v: 7 } { Some { v } => body }`.
fn in_match_arm(body: CompiledExpr) -> CompiledExpr {
    let discriminant = Value::Enum {
        type_name: "Wrap".to_string(),
        variant: "Some".to_string(),
        payload: vec![("v".to_string(), Value::Int(7))],
    };
    let arm = CompiledMatchArm {
        patterns: vec![CompiledPattern::VariantBind {
            name: "Some".to_string(),
            binders: vec![("v".to_string(), binder())],
        }],
        body,
    };
    CompiledExpr::match_expr(
        CompiledExpr::literal(discriminant, Type::Enum("Wrap".to_string())),
        vec![arm],
        Type::Int,
    )
}

/// `exists e in [4, 5]: e == Probe::x`.
fn exists_element_equal_to_x() -> CompiledExpr {
    let element = ValueCellId::new("$quant", "e");
    CompiledExpr::quantifier(
        QuantifierKind::Exists,
        "e".to_string(),
        element.clone(),
        list_expr(vec![int(4), int(5)]),
        binop(BinOp::Eq, read(element), read(cell("x")), Type::Bool),
    )
}

/// The composed-field cell `base`, holding the field `|p| p * p * p`.
fn cube_field() -> (ValueCellId, Value) {
    let p = ValueCellId::new("$lambda_base", "p");
    let ty = Type::dimensionless_scalar();
    let p_ref = || CompiledExpr::value_ref(p.clone(), ty.clone());
    let body = binop(
        BinOp::Mul,
        p_ref(),
        binop(BinOp::Mul, p_ref(), p_ref(), ty.clone()),
        ty.clone(),
    );
    let lambda = Value::Lambda {
        params: vec![("p".to_string(), p.clone())],
        body: Box::new(body),
        captures: ValueMap::new(),
    };
    let field = Value::Field {
        domain_type: ty.clone(),
        codomain_type: ty,
        source: FieldSourceKind::Analytical,
        lambda: Arc::new(lambda),
    };
    (ValueCellId::new(FIELD_ENTITY_PREFIX, "base"), field)
}

/// A store of dimensionless reals: seeded `x = 2`, unseeded `c = 3`, and the
/// composed field `base`.
fn real_store() -> ValueMap {
    let mut store = ValueMap::new();
    store.insert(cell("x"), Value::Real(2.0));
    store.insert(cell("c"), Value::Real(3.0));
    let (base, field) = cube_field();
    store.insert(base, field);
    store
}

fn real_read(member: &str) -> CompiledExpr {
    CompiledExpr::value_ref(cell(member), Type::dimensionless_scalar())
}

fn call_base(arg: CompiledExpr) -> CompiledExpr {
    std_call("base", vec![arg], Type::dimensionless_scalar())
}

/// Evaluate `expr` once under `store` as `values`, and once under an empty
/// `values` with `store` only in the reader; both must give the same non-Undef
/// value.
fn assert_reads_through_the_reader(expr: &CompiledExpr, store: &ValueMap) {
    let empty = ValueMap::new();
    let through_reader = eval_expr(expr, &EvalContext::simple(&empty).with_reader(store));
    let over_values = eval_expr(expr, &EvalContext::simple(store));
    assert_ne!(through_reader, Value::Undef, "a read bypassed the reader");
    assert_eq!(through_reader, over_values);
}

#[test]
fn a_top_level_value_ref_reads_through_the_reader() {
    assert_reads_through_the_reader(&read(cell("x")), &store());
}

#[test]
fn a_store_read_in_a_match_arm_reads_through_the_reader() {
    let body = binop(BinOp::Add, read(binder()), read(cell("x")), Type::Int);
    assert_reads_through_the_reader(&in_match_arm(body), &store());
}

#[test]
fn a_store_read_in_a_quantifier_predicate_reads_through_the_reader() {
    assert_reads_through_the_reader(&exists_element_equal_to_x(), &store());
}

#[test]
fn a_store_read_in_a_quantifier_predicate_with_determinacy_reads_through_the_reader() {
    let store = store();
    let empty = ValueMap::new();
    let determinacy: PersistentMap<ValueCellId, (Value, DeterminacyState)> = PersistentMap::new();
    let ctx = EvalContext::simple(&empty)
        .with_reader(&store)
        .with_determinacy(&determinacy);
    assert_eq!(
        eval_expr(&exists_element_equal_to_x(), &ctx),
        Value::Bool(true)
    );
}

#[test]
fn a_lambda_capture_reads_through_the_reader_and_its_body_sees_the_capture() {
    let store = store();
    let empty = ValueMap::new();
    let ctx = EvalContext::simple(&empty).with_reader(&store);
    let lambda = eval_expr(
        &CompiledExpr::lambda(vec![], vec![], read(cell("x")), vec![cell("x")], Type::Int),
        &ctx,
    );
    assert_eq!(apply_lambda(&lambda, &[], &ctx), Value::Int(5));
}

#[test]
fn the_composed_field_probe_reads_through_the_reader() {
    assert_reads_through_the_reader(&call_base(real_read("x")), &real_store());
}

#[test]
fn a_reflective_cell_list_quantifier_reads_through_the_reader() {
    let element = ValueCellId::new("$quant0.S", "p");
    let cells = CompiledExpr::reflective_cell_list(
        vec![read(cell("x")), read(cell("y"))],
        Type::List(Box::new(Type::Int)),
    );
    let positive = binop(BinOp::Gt, read(element.clone()), int(0), Type::Bool);
    let forall = CompiledExpr::quantifier(
        QuantifierKind::ForAll,
        "p".to_string(),
        element,
        cells,
        positive,
    );
    assert_reads_through_the_reader(&forall, &store());
}

/// `eval_dual` of `expr` with `x` seeded, under an empty `values` whose reader
/// holds [`real_store`]: its primal must equal plain evaluation over the store,
/// and its tangent must be `expected`.
fn assert_dual_reads_through_the_reader(expr: &CompiledExpr, expected: f64) {
    let store = real_store();
    let empty = ValueMap::new();
    let seed_cells = [cell("x")];
    let seeds = Seeds::new(&seed_cells);
    let ctx = EvalContext::simple(&empty).with_reader(&store);
    let dual = eval_dual(expr, &ctx, &seeds, &mut BranchRecord::new());
    let plain = eval_expr(expr, &EvalContext::simple(&store));
    assert_ne!(plain, Value::Undef);
    assert_eq!(
        dual.value, plain,
        "the primal read the store through the reader"
    );
    assert_eq!(dual.tangent.materialize(1), Some(vec![expected]));
}

#[test]
fn eval_dual_reads_seeded_and_unseeded_cells_through_the_reader() {
    let expr = binop(
        BinOp::Mul,
        real_read("x"),
        real_read("c"),
        Type::dimensionless_scalar(),
    );
    assert_dual_reads_through_the_reader(&expr, 3.0);
}

#[test]
fn eval_dual_reads_the_composed_field_probe_through_the_reader() {
    // d/dx x³ at x = 2 is 12.
    assert_dual_reads_through_the_reader(&call_base(real_read("x")), 12.0);
}

/// Stand-in for the selector hook: answers `probe_store` with the value the
/// scope it was handed holds for `Probe::x`.
fn store_probe(expr: &CompiledExpr, values: &ValueMap, _: &mut Vec<Diagnostic>) -> Option<Value> {
    let CompiledExprKind::FunctionCall { function, .. } = &expr.kind else {
        return None;
    };
    (function.name == "probe_store")
        .then(|| values.get(&cell("x")).cloned().unwrap_or(Value::Undef))
}

fn eval_store_probe_through_the_reader(expr: &CompiledExpr) -> Value {
    let store = store();
    let empty = ValueMap::new();
    let functions: [CompiledFunction; 0] = [];
    let ctx = EvalContext::new(&empty, &functions)
        .with_reader(&store)
        .with_symbolic_selector_ctor(store_probe);
    eval_expr(expr, &ctx)
}

#[test]
fn the_selector_hook_sees_a_store_cell_only_the_reader_holds() {
    let probe = std_call("probe_store", vec![], Type::Int);
    assert_eq!(eval_store_probe_through_the_reader(&probe), Value::Int(5));
    assert_eq!(
        eval_store_probe_through_the_reader(&in_match_arm(probe)),
        Value::Int(5),
        "inside a match arm"
    );
}
