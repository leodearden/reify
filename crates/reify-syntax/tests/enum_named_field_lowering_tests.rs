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
