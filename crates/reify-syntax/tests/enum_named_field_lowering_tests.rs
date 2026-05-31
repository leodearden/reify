//! Lowering tests for named-field enum declarations (task 3936 α).
//!
//! Step-3 RED: the current grammar emits bare identifiers (or ERROR nodes)
//! for named-field variants, so the lowered EnumDecl.variants are all
//! VariantPayload::Unit (or missing entirely).  Step-4 rewrites lower_enum
//! to iterate `enum_variant` children and builds VariantPayload::Named for
//! field-carrying variants.
//!
//! Source: the shared fixture at tree-sitter-reify/test/fixtures/dce-2-nameddecl.ri.

use reify_ast::{Declaration, EnumVariantDecl, VariantPayload};
use reify_core::ModulePath;

const FIXTURE_SOURCE: &str = r#"
/// Named-field enum declaration fixture — α parse-signal.
/// Tests: bare variant + named-field variants in one enum body.
enum Shape {
    Point,
    Circle { radius: Length },
    Rect { width: Length, height: Length },
}
"#;

fn parse_shape_enum() -> reify_ast::EnumDecl {
    let module = reify_syntax::parse(FIXTURE_SOURCE, ModulePath::single("test_dce"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors; got: {:?}",
        module.errors
    );
    match module.declarations.into_iter().next() {
        Some(Declaration::Enum(e)) => e,
        other => panic!("expected Enum declaration, got: {:?}", other),
    }
}

// ── (a) Basic structure ──────────────────────────────────────────────────────

/// Three variants lowered in source order.
///
/// RED: the current grammar + lower_enum only produce bare identifiers
/// (Unit payloads), so Named variants are not yet in the output.
#[test]
fn named_field_enum_has_three_variants_in_order() {
    let e = parse_shape_enum();
    assert_eq!(e.name, "Shape");
    assert_eq!(
        e.variants.len(),
        3,
        "expected 3 variants (Point, Circle, Rect); got {:?}",
        e.variants.iter().map(|v| &v.name).collect::<Vec<_>>()
    );
    assert_eq!(e.variants[0].name, "Point");
    assert_eq!(e.variants[1].name, "Circle");
    assert_eq!(e.variants[2].name, "Rect");
}

// ── (b) Point — bare (Unit) payload ────────────────────────────────────────

/// `Point` lowers to VariantPayload::Unit.
#[test]
fn point_variant_lowers_to_unit_payload() {
    let e = parse_shape_enum();
    let point = &e.variants[0];
    assert_eq!(point.name, "Point");
    assert_eq!(
        point.payload,
        VariantPayload::Unit,
        "Point must have Unit payload"
    );
}

// ── (c) Circle — Named payload with one field ────────────────────────────────

/// `Circle { radius: Length }` lowers to VariantPayload::Named with one entry.
///
/// RED: the current lower_enum produces Unit for all variants.
#[test]
fn circle_variant_lowers_to_named_one_field() {
    let e = parse_shape_enum();
    let circle = &e.variants[1];
    assert_eq!(circle.name, "Circle");
    match &circle.payload {
        VariantPayload::Named(fields) => {
            assert_eq!(
                fields.len(),
                1,
                "Circle must have 1 named field; got {:?}",
                fields.iter().map(|(n, _)| n).collect::<Vec<_>>()
            );
            assert_eq!(fields[0].0, "radius", "first field must be 'radius'");
            // The type is a Named TypeExpr for "Length".
            match &fields[0].1.kind {
                reify_ast::TypeExprKind::Named { name, type_args } => {
                    assert_eq!(name, "Length");
                    assert!(type_args.is_empty());
                }
                other => panic!("expected Named TypeExpr for 'Length', got {:?}", other),
            }
        }
        other => panic!("Circle must have Named payload, got {:?}", other),
    }
}

// ── (d) Rect — Named payload with two fields ────────────────────────────────

/// `Rect { width: Length, height: Length }` lowers to VariantPayload::Named
/// with two entries in source order.
///
/// RED: the current lower_enum produces Unit for all variants.
#[test]
fn rect_variant_lowers_to_named_two_fields() {
    let e = parse_shape_enum();
    let rect = &e.variants[2];
    assert_eq!(rect.name, "Rect");
    match &rect.payload {
        VariantPayload::Named(fields) => {
            assert_eq!(
                fields.len(),
                2,
                "Rect must have 2 named fields; got {:?}",
                fields.iter().map(|(n, _)| n).collect::<Vec<_>>()
            );
            assert_eq!(fields[0].0, "width");
            assert_eq!(fields[1].0, "height");
        }
        other => panic!("Rect must have Named payload, got {:?}", other),
    }
}

// ── (e) Bare-enum baseline regression ───────────────────────────────────────

/// `enum Dir { In, Out }` still lowers correctly to two Unit variants.
/// This must remain green before and after the grammar change.
#[test]
fn bare_enum_baseline_still_lowers_correctly() {
    let module = reify_syntax::parse(
        "enum Dir { In, Out }",
        ModulePath::single("test_dce_baseline"),
    );
    assert!(module.errors.is_empty());
    match &module.declarations[0] {
        Declaration::Enum(e) => {
            let names: Vec<&str> = e.variants.iter().map(|v| v.name.as_str()).collect();
            assert_eq!(names, vec!["In", "Out"]);
            for v in &e.variants {
                assert_eq!(v.payload, VariantPayload::Unit, "{} must be Unit", v.name);
            }
        }
        other => panic!("expected Enum, got {:?}", other),
    }
}

// ── Step-7 RED: variant_construction brace-expression lowering ───────────────
//
// These tests assert that `variant_construction` CST nodes (grammar step-6)
// lower to ExprKind::VariantConstruct in the AST.  They fail (RED) until
// step-8 adds the `"variant_construction"` dispatch arm in ts_parser.rs.

const CONSTRUCTION_SOURCE: &str = r#"
structure def Widget {
    param outline : Shape = Rect { width: 20mm, height: 10mm }
}
"#;

fn parse_widget_outline_default() -> reify_ast::Expr {
    use reify_ast::{Declaration, MemberDecl, StructureDef};
    let module = reify_syntax::parse(CONSTRUCTION_SOURCE, ModulePath::single("test_dce_construct"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors; got: {:?}",
        module.errors
    );
    let structure = match module.declarations.into_iter().next() {
        Some(Declaration::Structure(s)) => s,
        other => panic!("expected Structure declaration, got: {:?}", other),
    };
    let param = match structure.members.into_iter().next() {
        Some(MemberDecl::Param(p)) => p,
        other => panic!("expected Param member, got: {:?}", other),
    };
    param
        .default
        .expect("param 'outline' must have a default value")
}

/// The param-default `Rect { width: 20mm, height: 10mm }` lowers to
/// `ExprKind::VariantConstruct { name: "Rect", fields: [("width", ...), ("height", ...)] }`.
///
/// RED: ts_parser has no dispatch for `variant_construction` CST nodes.
/// GREEN (step-8): `lower_variant_construction` is wired into the dispatch.
#[test]
fn variant_construction_lowers_to_expr_kind() {
    use reify_ast::ExprKind;

    let default_expr = parse_widget_outline_default();
    match &default_expr.kind {
        ExprKind::VariantConstruct { name, fields } => {
            assert_eq!(name, "Rect", "expected variant name 'Rect'");
            assert_eq!(
                fields.len(),
                2,
                "expected 2 fields (width, height); got {:?}",
                fields.iter().map(|(n, _)| n).collect::<Vec<_>>()
            );
            assert_eq!(fields[0].0, "width", "first field must be 'width'");
            assert_eq!(fields[1].0, "height", "second field must be 'height'");
        }
        other => panic!(
            "expected ExprKind::VariantConstruct, got {:?}",
            other
        ),
    }
}

/// The field values in `Rect { width: 20mm, height: 10mm }` lower to
/// `QuantityLiteral` nodes with the correct numeric values and units.
///
/// RED: no dispatch for variant_construction → no fields at all.
#[test]
fn variant_construction_field_values_lower_to_quantity_literals() {
    use reify_ast::{ExprKind, UnitExpr};

    let default_expr = parse_widget_outline_default();
    let (name, fields) = match default_expr.kind {
        ExprKind::VariantConstruct { name, fields } => (name, fields),
        other => panic!(
            "expected ExprKind::VariantConstruct, got {:?}",
            other
        ),
    };
    assert_eq!(name, "Rect");

    // width: 20mm
    match &fields[0].1.kind {
        ExprKind::QuantityLiteral { value, unit } => {
            assert!(
                (*value - 20.0).abs() < 1e-9,
                "width value must be 20.0, got {}",
                value
            );
            assert_eq!(
                unit,
                &UnitExpr::Unit("mm".to_string()),
                "width unit must be mm"
            );
        }
        other => panic!(
            "expected QuantityLiteral for 'width', got {:?}",
            other
        ),
    }

    // height: 10mm
    match &fields[1].1.kind {
        ExprKind::QuantityLiteral { value, unit } => {
            assert!(
                (*value - 10.0).abs() < 1e-9,
                "height value must be 10.0, got {}",
                value
            );
            assert_eq!(
                unit,
                &UnitExpr::Unit("mm".to_string()),
                "height unit must be mm"
            );
        }
        other => panic!(
            "expected QuantityLiteral for 'height', got {:?}",
            other
        ),
    }
}
