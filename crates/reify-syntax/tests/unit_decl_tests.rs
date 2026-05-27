//! Unit declaration tests.
//!
//! Tests for `unit ident : type_expr (= expr)? (offset expr)?` declarations.

use reify_syntax::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("unit_decl_test"));
    (module.declarations, module.errors)
}

// ── Step 1: simple unit (no conversion, no offset) ───────────────

#[test]
fn parse_simple_unit() {
    let source = "unit meter : Length";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1, "expected 1 declaration, got {:?}", decls);

    match &decls[0] {
        Declaration::Unit(u) => {
            assert_eq!(u.name, "meter");
            assert_eq!(u.dimension_type.to_string(), "Length");
            assert!(
                u.conversion.is_none(),
                "expected no conversion, got {:?}",
                u.conversion
            );
            assert!(u.offset.is_none(), "expected no offset, got {:?}", u.offset);
        }
        other => panic!("expected Declaration::Unit, got {:?}", other),
    }
}

// ── Step 3: derived unit (with conversion factor) ────────────────

#[test]
fn parse_derived_unit() {
    let source = "unit mm : Length = 0.001";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let u = match &decls[0] {
        Declaration::Unit(u) => u,
        other => panic!("expected Declaration::Unit, got {:?}", other),
    };

    assert_eq!(u.name, "mm");
    assert_eq!(u.dimension_type.to_string(), "Length");
    assert!(u.offset.is_none(), "expected no offset");

    match &u.conversion {
        Some(expr) => match &expr.kind {
            ExprKind::NumberLiteral { value: v, .. } => {
                assert!((v - 0.001).abs() < 1e-9, "expected 0.001, got {}", v);
            }
            other => panic!("expected NumberLiteral(0.001), got {:?}", other),
        },
        None => panic!("expected Some conversion, got None"),
    }
}

// ── Step 5: offset unit (with both conversion and offset) ─────────

#[test]
fn parse_offset_unit() {
    let source = "unit degC : Temperature = 1 offset 273.15";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let u = match &decls[0] {
        Declaration::Unit(u) => u,
        other => panic!("expected Declaration::Unit, got {:?}", other),
    };

    assert_eq!(u.name, "degC");
    assert_eq!(u.dimension_type.to_string(), "Temperature");

    match &u.conversion {
        Some(expr) => match &expr.kind {
            ExprKind::NumberLiteral { value: v, .. } => {
                assert!((v - 1.0).abs() < 1e-9, "expected 1.0, got {}", v);
            }
            other => panic!("expected NumberLiteral(1.0), got {:?}", other),
        },
        None => panic!("expected Some conversion"),
    }

    match &u.offset {
        Some(expr) => match &expr.kind {
            ExprKind::NumberLiteral { value: v, .. } => {
                assert!((v - 273.15).abs() < 1e-9, "expected 273.15, got {}", v);
            }
            other => panic!("expected NumberLiteral(273.15), got {:?}", other),
        },
        None => panic!("expected Some offset"),
    }
}

// ── Step 7: pub unit declaration ──────────────────────────────────

#[test]
fn parse_pub_unit() {
    let source = "pub unit mm : Length = 0.001";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let u = match &decls[0] {
        Declaration::Unit(u) => u,
        other => panic!("expected Declaration::Unit, got {:?}", other),
    };

    assert!(u.is_pub, "expected is_pub == true");
    assert_eq!(u.name, "mm");
}

// ── Step 9: complex conversion expression ────────────────────────

#[test]
fn parse_unit_complex_conversion() {
    let source = "unit inch : Length = 25.4 * 0.001";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let u = match &decls[0] {
        Declaration::Unit(u) => u,
        other => panic!("expected Declaration::Unit, got {:?}", other),
    };

    assert_eq!(u.name, "inch");

    match &u.conversion {
        Some(expr) => match &expr.kind {
            ExprKind::BinOp { op, left, right } => {
                assert_eq!(op, "*");
                match &left.kind {
                    ExprKind::NumberLiteral { value: v, .. } => {
                        assert!((v - 25.4).abs() < 1e-9, "expected left=25.4, got {}", v);
                    }
                    other => panic!("expected NumberLiteral(25.4) on left, got {:?}", other),
                }
                match &right.kind {
                    ExprKind::NumberLiteral { value: v, .. } => {
                        assert!((v - 0.001).abs() < 1e-9, "expected right=0.001, got {}", v);
                    }
                    other => panic!("expected NumberLiteral(0.001) on right, got {:?}", other),
                }
            }
            other => panic!("expected BinOp(*), got {:?}", other),
        },
        None => panic!("expected Some conversion"),
    }
}

// ── Step 11: quantity literal in conversion expression ────────────

#[test]
fn parse_unit_quantity_literal_conversion() {
    let source = "unit thou : Length = 0.0254mm";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let u = match &decls[0] {
        Declaration::Unit(u) => u,
        other => panic!("expected Declaration::Unit, got {:?}", other),
    };

    assert_eq!(u.name, "thou");

    match &u.conversion {
        Some(expr) => match &expr.kind {
            ExprKind::QuantityLiteral { value, unit } => {
                assert!(
                    (value - 0.0254).abs() < 1e-9,
                    "expected value=0.0254, got {}",
                    value
                );
                assert_eq!(unit, "mm");
            }
            other => panic!("expected QuantityLiteral, got {:?}", other),
        },
        None => panic!("expected Some conversion"),
    }
}

// ── Step 13: offset only (no conversion) ─────────────────────────

#[test]
fn parse_unit_offset_only() {
    let source = "unit kelvin : Temperature offset 273.15";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let u = match &decls[0] {
        Declaration::Unit(u) => u,
        other => panic!("expected Declaration::Unit, got {:?}", other),
    };

    assert_eq!(u.name, "kelvin");
    assert!(
        u.conversion.is_none(),
        "expected no conversion, got {:?}",
        u.conversion
    );

    match &u.offset {
        Some(expr) => match &expr.kind {
            ExprKind::NumberLiteral { value: v, .. } => {
                assert!((v - 273.15).abs() < 1e-9, "expected 273.15, got {}", v);
            }
            other => panic!("expected NumberLiteral(273.15), got {:?}", other),
        },
        None => panic!("expected Some offset"),
    }
}

// ── Step 15: parameterized dimension type ─────────────────────────

#[test]
fn parse_unit_parameterized_dimension_type() {
    let source = "unit newton : Derived<Force> = 1";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1);

    let u = match &decls[0] {
        Declaration::Unit(u) => u,
        other => panic!("expected Declaration::Unit, got {:?}", other),
    };

    assert_eq!(u.name, "newton");
    let (dim_name, dim_args) = match &u.dimension_type.kind {
        TypeExprKind::Named { name, type_args } => (name.as_str(), type_args.as_slice()),
        other => panic!("expected Named dimension type, got {:?}", other),
    };
    assert_eq!(dim_name, "Derived");
    assert_eq!(dim_args.len(), 1, "expected 1 type arg");
    assert_eq!(dim_args[0].to_string(), "Force");
}

// ── Step 17: multiple unit declarations ───────────────────────────

#[test]
fn parse_multiple_unit_declarations() {
    let source = "unit meter : Length\nunit mm : Length = 0.001\nunit inch : Length = 25.4 * 0.001";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 3, "expected 3 declarations");

    let names: Vec<&str> = decls
        .iter()
        .map(|d| match d {
            Declaration::Unit(u) => u.name.as_str(),
            other => panic!("expected Declaration::Unit, got {:?}", other),
        })
        .collect();

    assert_eq!(names, vec!["meter", "mm", "inch"]);
}

// ── Step 19: unit among other declaration types ───────────────────

#[test]
fn parse_unit_among_other_declarations() {
    let source = "\
structure Bolt {}
unit mm : Length = 0.001
enum Direction { In, Out }
";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 3, "expected 3 declarations");

    match &decls[0] {
        Declaration::Structure(s) => assert_eq!(s.name, "Bolt"),
        other => panic!("expected Declaration::Structure, got {:?}", other),
    }
    match &decls[1] {
        Declaration::Unit(u) => assert_eq!(u.name, "mm"),
        other => panic!("expected Declaration::Unit, got {:?}", other),
    }
    match &decls[2] {
        Declaration::Enum(e) => assert_eq!(e.name, "Direction"),
        other => panic!("expected Declaration::Enum, got {:?}", other),
    }
}
