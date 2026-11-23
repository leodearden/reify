//! A structure ctor evaluates its members in dependency order (task 10098).
//!
//! A ctor's uncovered `defaults` and its `lets` may read one another in either
//! direction, as template scope and `sub` already allow. Each member must see
//! every sibling it reads, whatever their stored order, or the read silently
//! evaluates to `Undef`.

use reify_core::{Type, ValueCellId};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{BinOp, CompiledExpr, StructureInstanceData, Value, ValueMap};

const TYPE: &str = "B";

fn lit(n: i64) -> CompiledExpr {
    CompiledExpr::literal(Value::Int(n), Type::Int)
}

fn vref(member: &str) -> CompiledExpr {
    CompiledExpr::value_ref(ValueCellId::new(TYPE, member), Type::Int)
}

fn add(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Add, left, right, Type::Int)
}

fn mul(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Mul, left, right, Type::Int)
}

fn named(members: Vec<(&str, CompiledExpr)>) -> Vec<(String, CompiledExpr)> {
    members
        .into_iter()
        .map(|(name, expr)| (name.to_string(), expr))
        .collect()
}

fn ctor(
    ordered: Vec<(&str, CompiledExpr)>,
    defaults: Vec<(&str, CompiledExpr)>,
    lets: Vec<(&str, CompiledExpr)>,
) -> CompiledExpr {
    CompiledExpr::structure_instance_ctor(
        reify_ir::StructureTypeId(0),
        TYPE.to_string(),
        1,
        named(ordered),
        named(defaults),
        named(lets),
        None,
        Type::StructureRef(TYPE.to_string()),
    )
}

fn construct(expr: &CompiledExpr) -> StructureInstanceData {
    let outer = ValueMap::new();
    match eval_expr(expr, &EvalContext::simple(&outer)) {
        Value::StructureInstance(data) => *data,
        other => panic!("ctor must build a StructureInstance, got {other:?}"),
    }
}

fn field<'d>(data: &'d StructureInstanceData, member: &str) -> &'d Value {
    data.fields
        .get(member)
        .unwrap_or_else(|| panic!("instance has no `{member}` slot: {:?}", data.fields))
}

#[test]
fn default_reading_a_let_sees_the_let() {
    let data = construct(&ctor(vec![], vec![("n", vref("k"))], vec![("k", lit(3))]));
    assert_eq!(
        field(&data, "k"),
        &Value::Int(3),
        "the let itself must evaluate"
    );
    assert_eq!(
        field(&data, "n"),
        &Value::Int(3),
        "a default reading a sibling let must see it, not evaluate to Undef \
         because defaults run before lets"
    );
}

#[test]
fn default_reading_a_later_default_sees_it() {
    let data = construct(&ctor(
        vec![],
        vec![("a", add(vref("b"), lit(1))), ("b", lit(2))],
        vec![],
    ));
    assert_eq!(
        field(&data, "a"),
        &Value::Int(3),
        "a default reading a later-declared default must see it, not evaluate \
         to Undef because defaults run in declaration order"
    );
}

#[test]
fn let_reading_a_later_let_sees_it() {
    let data = construct(&ctor(
        vec![],
        vec![],
        vec![("a", add(vref("b"), lit(1))), ("b", lit(3))],
    ));
    assert_eq!(
        field(&data, "a"),
        &Value::Int(4),
        "a let reading a later-declared let must see it, not evaluate to \
         Undef because lets run in declaration order"
    );
}

#[test]
fn chain_across_defaults_and_lets_resolves_transitively() {
    let data = construct(&ctor(
        vec![],
        vec![("p", vref("l")), ("q", lit(5))],
        vec![("l", mul(vref("q"), lit(2)))],
    ));
    assert_eq!(
        field(&data, "l"),
        &Value::Int(10),
        "the let must see the default it reads"
    );
    assert_eq!(
        field(&data, "p"),
        &Value::Int(10),
        "a default must see a let that itself reads a later default, not \
         evaluate to Undef because defaults and lets run as separate passes"
    );
}

#[test]
fn arg_supplied_member_is_in_scope_for_a_let() {
    let data = construct(&ctor(
        vec![("n", lit(10))],
        vec![],
        vec![("m", mul(vref("n"), lit(2)))],
    ));
    assert_eq!(
        field(&data, "m"),
        &Value::Int(20),
        "a let must see an arg-supplied param"
    );
}

#[test]
fn cyclic_members_still_construct_the_instance() {
    let data = construct(&ctor(
        vec![],
        vec![("a", vref("b"))],
        vec![("b", vref("a"))],
    ));
    assert!(
        data.fields.get("a").is_some() && data.fields.get("b").is_some(),
        "a cycle between members must still build both slots: {:?}",
        data.fields
    );
}
