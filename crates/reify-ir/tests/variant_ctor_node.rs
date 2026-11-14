//! `CompiledExprKind::VariantCtor` — the runtime constructor for a
//! data-carrying enum variant whose payload is not all literals (task 7456).
//!
//! Driven only through the public `CompiledExpr` seams: the `variant_ctor`
//! constructor, `content_hash`, and the traversals every consumer relies on.

use std::collections::BTreeSet;

use reify_core::{Type, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, Value};

fn cell(member: &str) -> ValueCellId {
    ValueCellId::new("E", member)
}

fn scalar() -> Type {
    Type::dimensionless_scalar()
}

fn shape() -> Type {
    Type::Enum("Shape".to_string())
}

fn r(member: &str) -> CompiledExpr {
    CompiledExpr::value_ref(cell(member), scalar())
}

fn lit(v: f64) -> CompiledExpr {
    CompiledExpr::literal(Value::Real(v), scalar())
}

fn twice(e: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Mul, e, lit(2.0), scalar())
}

fn rect(payload: Vec<(&str, CompiledExpr)>) -> CompiledExpr {
    variant("Rect", payload)
}

fn variant(name: &str, payload: Vec<(&str, CompiledExpr)>) -> CompiledExpr {
    CompiledExpr::variant_ctor(
        "Shape".to_string(),
        name.to_string(),
        payload
            .into_iter()
            .map(|(field, e)| (field.to_string(), e))
            .collect(),
        shape(),
    )
}

/// `Rect { width: a, height: a * 2.0 }`.
fn rect_reading_a() -> CompiledExpr {
    rect(vec![("width", r("a")), ("height", twice(r("a")))])
}

fn payload_of(expr: &CompiledExpr) -> &[(String, CompiledExpr)] {
    match &expr.kind {
        CompiledExprKind::VariantCtor { payload, .. } => payload,
        other => panic!("expected VariantCtor, got {other:?}"),
    }
}

fn payload_refs(expr: &CompiledExpr) -> Vec<Vec<ValueCellId>> {
    payload_of(expr)
        .iter()
        .map(|(_, e)| e.collect_value_refs())
        .collect()
}

#[test]
fn variant_ctor_stores_type_variant_payload_and_result_type() {
    let expr = rect_reading_a();

    match &expr.kind {
        CompiledExprKind::VariantCtor {
            type_name,
            variant,
            payload,
        } => {
            assert_eq!(type_name, "Shape");
            assert_eq!(variant, "Rect");
            let names: Vec<&str> = payload.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(names, ["width", "height"]);
        }
        other => panic!("expected VariantCtor, got {other:?}"),
    }
    assert_eq!(expr.result_type, shape());
}

#[test]
fn variant_ctor_content_hash_is_structural() {
    let base = rect_reading_a().content_hash;

    assert_eq!(base, rect_reading_a().content_hash, "equal inputs hash equally");
    assert_ne!(
        base,
        rect(vec![("width", r("a")), ("height", twice(r("b")))]).content_hash,
        "a different payload expr must change the hash"
    );
    assert_ne!(
        base,
        variant("Square", vec![("width", r("a")), ("height", twice(r("a")))]).content_hash,
        "a different variant name must change the hash"
    );
    assert_ne!(
        base,
        rect(vec![("w", r("a")), ("height", twice(r("a")))]).content_hash,
        "a different field name must change the hash"
    );
}

#[test]
fn variant_ctor_hash_is_distinct_from_a_folded_literal_of_the_same_values() {
    let ctor = rect(vec![("width", lit(1.0)), ("height", lit(2.0))]);
    let folded = CompiledExpr::literal(
        Value::Enum {
            type_name: "Shape".to_string(),
            variant: "Rect".to_string(),
            payload: vec![
                ("width".to_string(), Value::Real(1.0)),
                ("height".to_string(), Value::Real(2.0)),
            ],
        },
        shape(),
    );

    assert_ne!(ctor.content_hash, folded.content_hash);
}

#[test]
fn walk_and_collect_value_refs_reach_every_payload_expr() {
    let expr = rect(vec![("width", r("a")), ("height", twice(r("b")))]);

    let mut walked_refs = BTreeSet::new();
    expr.walk(&mut |node| {
        if let CompiledExprKind::ValueRef(id) = &node.kind {
            walked_refs.insert(id.clone());
        }
    });
    let expected: BTreeSet<ValueCellId> = [cell("a"), cell("b")].into_iter().collect();
    assert_eq!(walked_refs, expected, "walk must visit every payload expr");

    let collected: BTreeSet<ValueCellId> = expr.collect_value_refs().into_iter().collect();
    assert_eq!(collected, expected);
}

#[test]
fn map_value_refs_rebuilds_variant_ctor_with_fresh_hash() {
    let original = rect_reading_a();
    let original_hash = original.content_hash;

    let remapped = original.map_value_refs(&mut |id| {
        if id == cell("a") { cell("a2") } else { id }
    });

    assert_eq!(payload_refs(&remapped), vec![vec![cell("a2")], vec![cell("a2")]]);
    let fresh = rect(vec![("width", r("a2")), ("height", twice(r("a2")))]);
    assert_eq!(remapped.content_hash, fresh.content_hash);
    assert_ne!(remapped.content_hash, original_hash);
}

#[test]
fn remap_entity_and_remap_cell_rewrite_payload_cells() {
    let mut by_entity = rect(vec![("width", r("a")), ("height", twice(r("b")))]);
    by_entity.remap_entity("E", "F");
    assert_eq!(
        payload_refs(&by_entity),
        vec![
            vec![ValueCellId::new("F", "a")],
            vec![ValueCellId::new("F", "b")],
        ]
    );

    let mut by_cell = rect(vec![("width", r("a")), ("height", twice(r("b")))]);
    by_cell.remap_cell(&cell("b"), &cell("c"));
    assert_eq!(payload_refs(&by_cell), vec![vec![cell("a")], vec![cell("c")]]);
}
