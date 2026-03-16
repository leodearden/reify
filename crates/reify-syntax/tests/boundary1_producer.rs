//! Boundary 1 (syntax → compiler) — Producer-side tests.
//!
//! These tests verify that the parser produces well-formed ParsedModule structures
//! that the compiler can consume. Until the Tree-sitter parser is implemented,
//! tests use the hand-built fixture from reify-test-support.

use reify_syntax::*;
use reify_test_support::*;

/// Parse bracket → verify structure (1 StructureDef, 5 params, 3 constraints, 2 lets).
#[test]
fn bracket_structure() {
    let module = bracket_parsed_module();
    assert_eq!(module.declarations.len(), 1);

    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!("expected Structure"),
    };

    assert_eq!(structure.name, "Bracket");

    let params: Vec<_> = structure
        .members
        .iter()
        .filter(|m| matches!(m, MemberDecl::Param(_)))
        .collect();
    let lets: Vec<_> = structure
        .members
        .iter()
        .filter(|m| matches!(m, MemberDecl::Let(_)))
        .collect();
    let constraints: Vec<_> = structure
        .members
        .iter()
        .filter(|m| matches!(m, MemberDecl::Constraint(_)))
        .collect();

    assert_eq!(params.len(), 5, "expected 5 params");
    assert_eq!(constraints.len(), 3, "expected 3 constraints");
    assert_eq!(lets.len(), 2, "expected 2 lets (volume + body)");
}

/// Error recovery: malformed input still produces partial declarations + ParseErrors.
#[test]
fn error_recovery_partial_parse() {
    let source = r#"structure Broken {
    param width: Scalar = 80mm
    param !!!invalid!!!
    param height: Scalar = 100mm
}"#;
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("broken"));
    // Should have parse errors but also recovered declarations
    assert!(!module.errors.is_empty());
    assert!(!module.declarations.is_empty());
}

/// Content hash determinism: same source → same hashes.
#[test]
fn content_hash_determinism() {
    let m1 = bracket_parsed_module();
    let m2 = bracket_parsed_module();
    assert_eq!(m1.content_hash, m2.content_hash);
}

/// Content hash sensitivity: changed default → changed hash.
#[test]
fn content_hash_sensitivity() {
    let module = bracket_parsed_module();
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!("expected Structure"),
    };

    // Different params should have different content hashes
    let param_hashes: Vec<_> = structure
        .members
        .iter()
        .filter_map(|m| match m {
            MemberDecl::Param(p) => Some(p.content_hash),
            _ => None,
        })
        .collect();

    // All param hashes should be unique
    for (i, h1) in param_hashes.iter().enumerate() {
        for (j, h2) in param_hashes.iter().enumerate() {
            if i != j {
                assert_ne!(h1, h2, "params {} and {} have same hash", i, j);
            }
        }
    }
}

/// Quantity literal parsing: `80mm` → QuantityLiteral { value: 80.0, unit: "mm" }.
#[test]
fn quantity_literal_parsing() {
    let module = bracket_parsed_module();
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!("expected Structure"),
    };

    // First param: width with default 80mm
    let width = match &structure.members[0] {
        MemberDecl::Param(p) => p,
        _ => panic!("expected Param"),
    };

    assert_eq!(width.name, "width");
    match &width.default {
        Some(expr) => match &expr.kind {
            ExprKind::QuantityLiteral { value, unit } => {
                assert!((value - 80.0).abs() < f64::EPSILON);
                assert_eq!(unit, "mm");
            }
            other => panic!("expected QuantityLiteral, got {:?}", other),
        },
        None => panic!("expected default value"),
    }
}

/// Operator precedence in the AST: multiplication before addition in volume computation.
#[test]
fn operator_precedence_in_ast() {
    let module = bracket_parsed_module();
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!("expected Structure"),
    };

    // Find the volume let binding
    let volume = structure
        .members
        .iter()
        .find_map(|m| match m {
            MemberDecl::Let(l) if l.name == "volume" => Some(l),
            _ => None,
        })
        .expect("volume let not found");

    // volume = width * height * thickness
    // Should be left-associative: (width * height) * thickness
    match &volume.value.kind {
        ExprKind::BinOp { op, left, .. } => {
            assert_eq!(op, "*");
            match &left.kind {
                ExprKind::BinOp { op: inner_op, .. } => {
                    assert_eq!(inner_op, "*");
                }
                other => panic!("expected inner BinOp, got {:?}", other),
            }
        }
        other => panic!("expected BinOp for volume, got {:?}", other),
    }
}

/// The bracket source text should match the fixture.
#[test]
fn bracket_source_round_trip() {
    let source = bracket_source();
    assert!(source.contains("structure Bracket"));
    assert!(source.contains("param width: Scalar = 80mm"));
    assert!(source.contains("constraint thickness > 2mm"));
    assert!(source.contains("let volume = width * height * thickness"));
    assert!(source.contains("let body = box(width, height, thickness)"));
}

/// Parse bracket → all members carry non-empty spans.
#[test]
fn all_spans_valid() {
    let source = bracket_source();
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("bracket"));
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        _ => panic!("expected Structure"),
    };

    for member in &structure.members {
        let span = match member {
            MemberDecl::Param(p) => p.span,
            MemberDecl::Let(l) => l.span,
            MemberDecl::Constraint(c) => c.span,
            MemberDecl::Sub(s) => s.span,
        };
        assert!(span.start < span.end, "span should be non-empty");
    }
}
