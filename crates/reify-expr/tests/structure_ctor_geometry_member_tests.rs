//! A structure ctor puts its template's geometry lets into the constructed
//! instance's member scope as symbolic handles (task 10352).
//!
//! A sibling selector over the geometry let then resolves through the
//! kernel-free selector hook, while the handle itself never becomes an
//! instance field (docs/prds/v0_6/uniform-member-access.md D5).

use reify_core::hash::ContentHash;
use reify_core::identity::RealizationNodeId;
use reify_core::{Diagnostic, Type, ValueCellId};
use reify_expr::{EvalContext, eval_expr, realization_upstream_values_hash};
use reify_ir::{
    CompiledExpr, CompiledExprKind, CtorGeometryLet, ResolvedFunction, StructureInstanceData,
    Value, ValueMap,
};

const TYPE: &str = "B";

fn length(meters: f64) -> CompiledExpr {
    CompiledExpr::literal(Value::length(meters), Type::length())
}

fn vref(member: &str) -> CompiledExpr {
    CompiledExpr::value_ref(ValueCellId::new(TYPE, member), Type::length())
}

/// Stand-in for reify-eval's symbolic topology-selector hook: builds only calls
/// named `stub_sel`, naming the value its first `ValueRef` argument has in the
/// scope the hook was handed. Like a real `Value::Selector`, the result holds
/// no geometry handle.
fn stub_mint(expr: &CompiledExpr, values: &ValueMap, _: &mut Vec<Diagnostic>) -> Option<Value> {
    let CompiledExprKind::FunctionCall { function, args } = &expr.kind else {
        return None;
    };
    if function.name != "stub_sel" {
        return None;
    }
    match args.first().map(|a| &a.kind) {
        Some(CompiledExprKind::ValueRef(id)) => values.get(id).map(selected),
        _ => None,
    }
}

fn selected(target: &Value) -> Value {
    Value::String(format!("selector over {target:?}"))
}

fn stub_sel(arg: CompiledExpr) -> CompiledExpr {
    CompiledExpr {
        content_hash: ContentHash::of(b"stub_sel").combine(arg.content_hash),
        result_type: Type::Geometry,
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: "stub_sel".to_string(),
                qualified_name: "std::stub_sel".to_string(),
            },
            args: vec![arg],
        },
    }
}

fn body_reading(width: CompiledExpr) -> CtorGeometryLet {
    CtorGeometryLet {
        member: "body".to_string(),
        realization: RealizationNodeId::new(TYPE, 0),
        upstream_args: vec![("width".to_string(), width)],
    }
}

fn named(members: Vec<(&str, CompiledExpr)>) -> Vec<(String, CompiledExpr)> {
    members
        .into_iter()
        .map(|(name, expr)| (name.to_string(), expr))
        .collect()
}

fn ctor(
    ordered: Vec<(&str, CompiledExpr)>,
    lets: Vec<(&str, CompiledExpr)>,
    geometry_lets: Vec<CtorGeometryLet>,
) -> CompiledExpr {
    let defaults: Vec<(&str, CompiledExpr)> = if ordered.iter().any(|(n, _)| *n == "len") {
        vec![]
    } else {
        vec![("len", length(1.0))]
    };
    CompiledExpr::structure_instance_ctor(
        reify_ir::StructureTypeId(0),
        TYPE.to_string(),
        1,
        named(ordered),
        named(defaults),
        named(lets),
        geometry_lets,
        None,
        Type::StructureRef(TYPE.to_string()),
    )
}

fn construct(expr: &CompiledExpr) -> StructureInstanceData {
    let outer = ValueMap::new();
    let ctx = EvalContext::simple(&outer).with_symbolic_selector_ctor(stub_mint);
    match eval_expr(expr, &ctx) {
        Value::StructureInstance(data) => *data,
        other => panic!("ctor must build a StructureInstance, got {other:?}"),
    }
}

fn field<'d>(data: &'d StructureInstanceData, member: &str) -> &'d Value {
    data.fields
        .get(member)
        .unwrap_or_else(|| panic!("instance has no `{member}` slot: {:?}", data.fields))
}

/// The handle a symbolic `body` minted over `width` evaluated to `width_value`.
fn expected_body_handle(width_value: CompiledExpr) -> Value {
    let outer = ValueMap::new();
    let plain = EvalContext::simple(&outer);
    Value::GeometryHandle {
        realization_ref: RealizationNodeId::new(TYPE, 0),
        upstream_values_hash: realization_upstream_values_hash(
            &[("width".to_string(), width_value)],
            |e| eval_expr(e, &plain),
        ),
        kernel_handle: None,
    }
}

/// What `stub_sel(body)` yields when `body` minted over `width_value`.
fn expected_tip(width_value: CompiledExpr) -> Value {
    selected(&expected_body_handle(width_value))
}

fn tip_reading_body() -> Vec<(&'static str, CompiledExpr)> {
    vec![("tip", stub_sel(vref("body")))]
}

#[test]
fn selector_let_sees_the_symbolic_geometry_member() {
    let data = construct(&ctor(
        vec![],
        tip_reading_body(),
        vec![body_reading(vref("len"))],
    ));
    assert_eq!(
        field(&data, "tip"),
        &expected_tip(length(1.0)),
        "a selector let over a geometry let must see the instance's symbolic \
         handle, hashed over the instance's own `len`"
    );
}

#[test]
fn geometry_member_is_not_an_instance_field() {
    let data = construct(&ctor(
        vec![],
        tip_reading_body(),
        vec![body_reading(vref("len"))],
    ));
    assert!(
        data.fields.get("body").is_none(),
        "the geometry handle must stay in member scope, never a field (D5): {:?}",
        data.fields
    );
}

#[test]
fn supplied_param_changes_the_geometry_member_hash() {
    let defaulted = construct(&ctor(
        vec![],
        tip_reading_body(),
        vec![body_reading(vref("len"))],
    ));
    let supplied = construct(&ctor(
        vec![("len", length(2.0))],
        tip_reading_body(),
        vec![body_reading(vref("len"))],
    ));
    assert_eq!(field(&supplied, "tip"), &expected_tip(length(2.0)));
    assert_ne!(field(&supplied, "tip"), field(&defaulted, "tip"));
}

#[test]
fn geometry_member_reading_a_sibling_let_sees_it() {
    let mut lets = tip_reading_body();
    lets.push(("w", length(3.0)));
    let data = construct(&ctor(vec![], lets, vec![body_reading(vref("w"))]));
    assert_eq!(
        field(&data, "tip"),
        &expected_tip(length(3.0)),
        "a geometry member must follow the sibling let it reads in the ctor's \
         dependency order"
    );
}

/// A cell no ctor member provides: a `sub` member, which a ctor never carries.
fn absent() -> CompiledExpr {
    CompiledExpr::value_ref(ValueCellId::new("B.inner", "w"), Type::length())
}

#[track_caller]
fn assert_tip_is_undef(data: &StructureInstanceData, what: &str) {
    assert_eq!(
        field(data, "tip"),
        &Value::Undef,
        "{what}: a geometry member whose input member scope cannot resolve must \
         not be minted, so its sibling selector stays Undef"
    );
}

#[test]
fn geometry_member_over_an_unresolved_input_is_not_minted() {
    let data = construct(&ctor(
        vec![],
        tip_reading_body(),
        vec![body_reading(absent())],
    ));
    assert_tip_is_undef(&data, "a direct unresolved input");
    assert!(
        data.fields.get("body").is_none(),
        "the refused geometry member must not become a field either: {:?}",
        data.fields
    );
}

#[test]
fn geometry_member_over_a_sibling_let_that_is_unresolved_is_not_minted() {
    let mut lets = tip_reading_body();
    lets.push(("w2", absent()));
    let data = construct(&ctor(vec![], lets, vec![body_reading(vref("w2"))]));
    assert_tip_is_undef(&data, "an input unresolved through a sibling let");
}

#[test]
fn geometry_member_over_an_input_holding_a_nested_undef_is_not_minted() {
    let nested = CompiledExpr::list_literal(
        vec![absent(), length(1.0)],
        Type::List(Box::new(Type::length())),
    );
    let data = construct(&ctor(
        vec![],
        tip_reading_body(),
        vec![body_reading(nested)],
    ));
    assert_tip_is_undef(&data, "an input whose Undef sits inside a list literal");
}

#[test]
fn cross_sub_geometry_arg_is_skipped_so_the_member_still_mints() {
    let body = CtorGeometryLet {
        member: "body".to_string(),
        realization: RealizationNodeId::new(TYPE, 0),
        upstream_args: vec![
            ("width".to_string(), vref("len")),
            (
                "target".to_string(),
                CompiledExpr::cross_sub_geometry_ref(
                    ValueCellId::new("B.inner", "body"),
                    Type::Geometry,
                ),
            ),
        ],
    };
    let data = construct(&ctor(vec![], tip_reading_body(), vec![body]));
    assert_eq!(
        field(&data, "tip"),
        &expected_tip(length(1.0)),
        "the fold skips a CrossSubGeometryRef arg unevaluated, so it neither \
         refuses the mint nor changes the hash"
    );
}

#[test]
fn field_member_carrying_the_geometry_member_by_value_holds_no_handle() {
    let mut lets = tip_reading_body();
    lets.push(("held", vref("body")));
    lets.push((
        "listed",
        CompiledExpr::list_literal(vec![vref("body")], Type::List(Box::new(Type::Geometry))),
    ));
    let data = construct(&ctor(vec![], lets, vec![body_reading(vref("len"))]));
    assert_eq!(field(&data, "tip"), &expected_tip(length(1.0)));
    assert_eq!(
        field(&data, "held"),
        &Value::Undef,
        "a handle serves selectors only, never a field value"
    );
    assert_eq!(field(&data, "listed"), &Value::List(vec![Value::Undef]));
}
