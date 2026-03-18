//! Boundary 1 (syntax → compiler) — Consumer-side tests.
//!
//! These tests verify that the compiler can accept well-formed ParsedModules
//! from the parser and produce correct output (or appropriate diagnostics).

use reify_test_support::*;

/// Accept well-formed ParsedModule → produce compiled output.
#[test]
fn accept_well_formed_parsed_module() {
    let parsed = bracket_parsed_module();
    let compiled = reify_compiler::compile(&parsed);
    assert!(compiled.diagnostics.is_empty(), "no diagnostics for valid input");
    assert_eq!(compiled.templates.len(), 1);
    assert_eq!(compiled.templates[0].name, "Bracket");
}

/// Reject ParsedModule with unresolved type names → diagnostics, not panic.
#[test]
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

/// Compiled constraints should propagate spans from parsed ConstraintDecls.
#[test]
fn compiled_constraint_spans_match_parsed_spans() {
    use reify_syntax::MemberDecl;
    use reify_types::SourceSpan;

    let source = bracket_source();
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("bracket"));
    let compiled = reify_compiler::compile(&parsed);

    assert_eq!(compiled.templates.len(), 1);
    let template = &compiled.templates[0];
    assert_eq!(template.constraints.len(), 3);

    // Each constraint span must be non-zero (not the hardcoded (0,0) default)
    for (i, constraint) in template.constraints.iter().enumerate() {
        assert_ne!(
            constraint.span,
            SourceSpan::new(0, 0),
            "constraint {} span should not be (0,0) — must propagate from ConstraintDecl",
            i
        );
    }

    // Extract parsed constraint spans for comparison
    let parsed_constraint_spans: Vec<SourceSpan> = match &parsed.declarations[0] {
        reify_syntax::Declaration::Structure(s) => s
            .members
            .iter()
            .filter_map(|m| match m {
                MemberDecl::Constraint(c) => Some(c.span),
                _ => None,
            })
            .collect(),
        _ => panic!("expected Structure"),
    };
    assert_eq!(parsed_constraint_spans.len(), 3);

    // Compiled constraint spans must match parsed ConstraintDecl spans
    for (i, constraint) in template.constraints.iter().enumerate() {
        assert_eq!(
            constraint.span, parsed_constraint_spans[i],
            "constraint {} span should match parsed ConstraintDecl.span",
            i
        );
    }
}

/// Handle ParsedModule with parse errors → process valid declarations.
#[test]
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
