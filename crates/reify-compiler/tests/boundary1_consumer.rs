//! Boundary 1 (syntax → compiler) — Consumer-side tests.
//!
//! These tests verify that the compiler can accept well-formed ParsedModules
//! from the parser and produce correct output (or appropriate diagnostics).

use reify_test_support::*;

/// Accept well-formed ParsedModule → produce compiled output.
#[test]
#[ignore = "requires compiler implementation"]
fn accept_well_formed_parsed_module() {
    let parsed = bracket_parsed_module();
    let compiled = reify_compiler::compile(&parsed);
    assert!(compiled.diagnostics.is_empty(), "no diagnostics for valid input");
    assert_eq!(compiled.templates.len(), 1);
    assert_eq!(compiled.templates[0].name, "Bracket");
}

/// Reject ParsedModule with unresolved type names → diagnostics, not panic.
#[test]
#[ignore = "requires compiler implementation"]
fn reject_unresolved_type_names() {
    use reify_syntax::*;
    use reify_types::*;

    let module = ParsedModule {
        path: ModulePath::single("bad"),
        declarations: vec![Declaration::Structure(StructureDef {
            name: "Bad".into(),
            members: vec![MemberDecl::Param(ParamDecl {
                name: "x".into(),
                type_expr: Some(TypeExpr {
                    name: "NonexistentType".into(),
                    span: SourceSpan::new(0, 15),
                }),
                default: None,
                span: SourceSpan::new(0, 30),
                content_hash: ContentHash::of_str("param x: NonexistentType"),
            })],
            span: SourceSpan::new(0, 50),
            content_hash: ContentHash::of_str("structure Bad"),
        })],
        errors: vec![],
        content_hash: ContentHash::of_str("bad module"),
    };

    let compiled = reify_compiler::compile(&module);
    assert!(!compiled.diagnostics.is_empty(), "should have diagnostics for unresolved type");
}

/// Handle ParsedModule with parse errors → process valid declarations.
#[test]
#[ignore = "requires compiler implementation"]
fn handle_parse_errors_gracefully() {
    use reify_syntax::*;
    use reify_types::*;

    let module = ParsedModule {
        path: ModulePath::single("partial"),
        declarations: vec![Declaration::Structure(StructureDef {
            name: "Partial".into(),
            members: vec![MemberDecl::Param(ParamDecl {
                name: "width".into(),
                type_expr: Some(TypeExpr {
                    name: "Scalar".into(),
                    span: SourceSpan::new(0, 6),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 80.0,
                        unit: "mm".into(),
                    },
                    span: SourceSpan::new(9, 13),
                }),
                span: SourceSpan::new(0, 13),
                content_hash: ContentHash::of_str("param width: Scalar = 80mm"),
            })],
            span: SourceSpan::new(0, 50),
            content_hash: ContentHash::of_str("structure Partial"),
        })],
        errors: vec![ParseError {
            message: "unexpected token".into(),
            span: SourceSpan::new(40, 45),
        }],
        content_hash: ContentHash::of_str("partial module"),
    };

    // Should not panic; should produce output for valid parts
    let compiled = reify_compiler::compile(&module);
    assert_eq!(compiled.templates.len(), 1);
}
