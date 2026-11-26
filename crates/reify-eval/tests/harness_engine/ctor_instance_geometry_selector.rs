//! A let-constructed structure instance resolves a selector over its own
//! geometry `let` (task #10352).
//!
//! The ctor evaluator gives each template geometry let a symbolic handle in
//! the instance's member scope. That handle's `upstream_values_hash` comes
//! from reify-expr's `realization_upstream_values_hash`, while template and
//! `sub` scope mint theirs through the engine's own fold. The tests here pin
//! the two folds equal and the three scopes in agreement.

use reify_core::ValueCellId;
use reify_eval::EvalResult;
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

use crate::ctor_instance_support::{assert_no_error_diagnostics, cell, ctor_field, instance_field};

fn eval_with_stdlib(source: &str) -> EvalResult {
    make_simple_engine().eval(&parse_and_compile_with_stdlib(source))
}

#[track_caller]
fn assert_selector(value: &Value, what: &str) {
    assert!(
        matches!(value, Value::Selector(_)),
        "{what} must resolve to a Selector, got {value:?}"
    );
}

#[track_caller]
fn assert_no_field(result: &EvalResult, host: &str, holder: &str, member: &str) {
    match cell(result, host, holder) {
        Value::StructureInstance(data) => assert!(
            data.fields.get(member).is_none(),
            "{host}.{holder} must carry no `{member}` field (the geometry stays \
             in member scope, uniform-member-access D5): {:?}",
            data.fields
        ),
        other => panic!("{host}.{holder} must hold a StructureInstance, got {other:?}"),
    }
}

/// The engine's template-scope `Beam.body` handle hashes its upstream values
/// exactly as reify-expr's fold does over the same realization's op args.
/// The engine's fold is crate-private, so this public-seam comparison is what
/// keeps the two from diverging.
#[test]
fn reify_expr_fold_matches_engine_template_handle_hash() {
    let compiled = parse_and_compile_with_stdlib(
        "structure def Beam {
            param len : Length = 1000mm
            let body = box(len, 100mm, 100mm)
        }",
    );
    let result = make_simple_engine().eval(&compiled);

    let Some(Value::GeometryHandle {
        realization_ref,
        upstream_values_hash,
        ..
    }) = result.values.get(&ValueCellId::new("Beam", "body"))
    else {
        panic!(
            "Beam.body must hold a GeometryHandle, got {:?}",
            result.values.get(&ValueCellId::new("Beam", "body"))
        );
    };

    let beam = compiled
        .templates
        .iter()
        .find(|t| t.name == "Beam")
        .expect("Beam template");
    let body = beam
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some("body"))
        .expect("Beam's `body` realization");
    assert_eq!(*realization_ref, body.id);

    let ctx = reify_expr::EvalContext::new(&result.values, &compiled.functions);
    let folded = reify_expr::realization_upstream_values_hash(
        body.operations.iter().flat_map(|op| op.args()),
        |e| reify_expr::eval_expr(e, &ctx),
    );
    assert_eq!(
        *upstream_values_hash, folded,
        "reify-expr's realization_upstream_values_hash must stay byte-equal to \
         the engine's compute_realization_upstream_values_hash_from_ops"
    );
}

const SELECTOR_LET_SRC: &str = r#"
structure def Beam {
    param len : Length = 1000mm
    let body = box(len, 100mm, 100mm)
    let tip = vertex(body, "tip")
    let ld = PointLoad(point: tip)
}

structure def NestedAssembly {
    let primary = Beam()
    let wide = Beam(len: 2000mm)
    sub secondary = Beam(len: 2000mm)
}
"#;

/// A let-constructed instance's selector over its own geometry let agrees with
/// the template's, and so does a selector-fed sibling's field.
#[test]
fn let_instance_selector_over_geometry_let_agrees_with_template() {
    let result = eval_with_stdlib(SELECTOR_LET_SRC);
    assert_no_error_diagnostics(&result, "the selector-let fixture");

    let template_tip = cell(&result, "Beam", "tip");
    assert_selector(&template_tip, "template Beam.tip");
    let primary_tip = ctor_field(&result, "NestedAssembly", "primary", "tip");
    assert_selector(&primary_tip, "NestedAssembly.primary.tip");
    assert_eq!(primary_tip, template_tip);

    let template_point = instance_field(&cell(&result, "Beam", "ld"), "Beam.ld", "point");
    let primary_point = instance_field(
        &ctor_field(&result, "NestedAssembly", "primary", "ld"),
        "NestedAssembly.primary.ld",
        "point",
    );
    assert_selector(&primary_point, "NestedAssembly.primary.ld.point");
    assert_eq!(primary_point, template_point);
}

/// The selector comes from the instance's own bindings: a let instance with a
/// supplied `len` agrees with a `sub` given the same `len`, not the template.
#[test]
fn let_instance_selector_follows_its_own_bindings() {
    let result = eval_with_stdlib(SELECTOR_LET_SRC);
    assert_no_error_diagnostics(&result, "the selector-let fixture");

    let wide_tip = ctor_field(&result, "NestedAssembly", "wide", "tip");
    let sub_tip = cell(&result, "NestedAssembly.secondary", "tip");
    assert_selector(&wide_tip, "NestedAssembly.wide.tip");
    assert_eq!(wide_tip, sub_tip);
    assert_ne!(wide_tip, cell(&result, "Beam", "tip"));
}

#[test]
fn let_instance_carries_no_geometry_field() {
    let result = eval_with_stdlib(SELECTOR_LET_SRC);
    assert_no_field(&result, "NestedAssembly", "primary", "body");
    assert_no_field(&result, "NestedAssembly", "wide", "body");
}

const SELECTOR_DEFAULT_SRC: &str = r#"
structure def Beam {
    param load : Load = PointLoad(point: vertex(body, "tip"))
    let body = box(1000mm, 100mm, 100mm)
}

structure def NestedAssembly {
    let primary = Beam()
    sub secondary = Beam()
}
"#;

/// A param default whose nested selector reads a geometry let resolves in a
/// let-constructed instance's own member scope, agreeing with the template and
/// a `sub`, and the caller template gains no synthetic selector cell.
#[test]
fn let_instance_selector_in_param_default_agrees_with_template_and_sub() {
    let result = eval_with_stdlib(SELECTOR_DEFAULT_SRC);
    assert_no_error_diagnostics(&result, "the selector-default fixture");

    let template_point = instance_field(&cell(&result, "Beam", "load"), "Beam.load", "point");
    assert_selector(&template_point, "Beam.load.point");
    let primary_point = instance_field(
        &ctor_field(&result, "NestedAssembly", "primary", "load"),
        "NestedAssembly.primary.load",
        "point",
    );
    assert_selector(&primary_point, "NestedAssembly.primary.load.point");
    assert_eq!(primary_point, template_point);
    let sub_point = instance_field(
        &cell(&result, "NestedAssembly.secondary", "load"),
        "NestedAssembly.secondary.load",
        "point",
    );
    assert_eq!(sub_point, template_point);

    let caller_sel_cells: Vec<_> = result
        .values
        .iter()
        .map(|(id, _)| id)
        .filter(|id| id.entity == "NestedAssembly" && id.member.starts_with("__sel"))
        .collect();
    assert!(
        caller_sel_cells.is_empty(),
        "no selector cell may be minted in the caller template: {caller_sel_cells:?}"
    );
}
