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
    assert!(
        compiled.diagnostics.is_empty(),
        "no diagnostics for valid input"
    );
    assert_eq!(compiled.templates.len(), 1);
    assert_eq!(compiled.templates[0].name, "Bracket");
}

/// Reject ParsedModule with unresolved type names → diagnostics, not panic.
#[test]
fn reject_unresolved_type_names() {
    use reify_ast::*;
    use reify_core::*;

    let module = ParsedModule {
        path: ModulePath::single("bad"),
        declarations: vec![Declaration::Structure(StructureDef {
            name: "Bad".into(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![MemberDecl::Param(ParamDecl {
                name: "x".into(),
                doc: None,
                type_expr: Some(TypeExpr {
                    kind: TypeExprKind::Named {
                        name: "NonexistentType".into(),
                        type_args: vec![],
                    },
                    span: SourceSpan::new(0, 15),
                }),
                default: None,
                where_clause: None,
                annotations: Vec::new(),
                span: SourceSpan::new(0, 30),
                content_hash: ContentHash::of_str("param x: NonexistentType"),
            })],
            span: SourceSpan::new(0, 50),
            content_hash: ContentHash::of_str("structure Bad"),
            pragmas: vec![],
            annotations: vec![],
        })],
        errors: vec![],
        content_hash: ContentHash::of_str("bad module"),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&module);
    assert!(
        !compiled.diagnostics.is_empty(),
        "should have diagnostics for unresolved type"
    );
}

/// Compiled constraints should propagate spans from parsed ConstraintDecls.
#[test]
fn compiled_constraint_spans_match_parsed_spans() {
    use reify_ast::MemberDecl;
    use reify_core::SourceSpan;

    let source = bracket_source();
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("bracket"));
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
        reify_ast::Declaration::Structure(s) => s
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

/// Compiled param ValueCellDecls should propagate spans from parsed ParamDecls.
#[test]
fn compiled_param_spans_match_parsed_spans() {
    use reify_ast::MemberDecl;
    use reify_core::SourceSpan;

    let source = bracket_source();
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("bracket"));
    let compiled = reify_compiler::compile(&parsed);

    assert_eq!(compiled.templates.len(), 1);
    let template = &compiled.templates[0];

    // Get compiled param ValueCellDecls (5 params: width, height, thickness, fillet_radius, hole_diameter)
    let compiled_params: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.kind == reify_compiler::ValueCellKind::Param)
        .collect();
    assert_eq!(compiled_params.len(), 5, "expected 5 param cells");

    // Each param span must be non-zero (not the hardcoded (0,0) default)
    for (i, param) in compiled_params.iter().enumerate() {
        assert_ne!(
            param.span,
            SourceSpan::new(0, 0),
            "param {} span should not be (0,0) — must propagate from ParamDecl",
            i
        );
    }

    // Extract parsed param spans for comparison
    let parsed_param_spans: Vec<SourceSpan> = match &parsed.declarations[0] {
        reify_ast::Declaration::Structure(s) => s
            .members
            .iter()
            .filter_map(|m| match m {
                MemberDecl::Param(p) => Some(p.span),
                _ => None,
            })
            .collect(),
        _ => panic!("expected Structure"),
    };
    assert_eq!(parsed_param_spans.len(), 5);

    // Compiled param spans must match parsed ParamDecl spans
    for (i, param) in compiled_params.iter().enumerate() {
        assert_eq!(
            param.span, parsed_param_spans[i],
            "param {} span should match parsed ParamDecl.span",
            i
        );
    }
}

/// Compiled let ValueCellDecls should propagate spans from parsed LetDecls.
#[test]
fn compiled_let_spans_match_parsed_spans() {
    use reify_ast::MemberDecl;
    use reify_core::SourceSpan;

    let source = bracket_source();
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("bracket"));
    let compiled = reify_compiler::compile(&parsed);

    assert_eq!(compiled.templates.len(), 1);
    let template = &compiled.templates[0];

    // Get compiled let ValueCellDecls (1 let: volume; 'body' is skipped as geometry-producing)
    let compiled_lets: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.kind == reify_compiler::ValueCellKind::Let)
        .collect();
    assert_eq!(compiled_lets.len(), 1, "expected 1 let cell (volume)");

    // The let span must be non-zero (not the hardcoded (0,0) default)
    assert_ne!(
        compiled_lets[0].span,
        SourceSpan::new(0, 0),
        "let 'volume' span should not be (0,0) — must propagate from LetDecl"
    );

    // Extract parsed let spans for non-geometry lets
    let parsed_let_spans: Vec<SourceSpan> = match &parsed.declarations[0] {
        reify_ast::Declaration::Structure(s) => s
            .members
            .iter()
            .filter_map(|m| match m {
                MemberDecl::Let(l) if l.name == "volume" => Some(l.span),
                _ => None,
            })
            .collect(),
        _ => panic!("expected Structure"),
    };
    assert_eq!(parsed_let_spans.len(), 1);

    // Compiled let span must match parsed LetDecl span
    assert_eq!(
        compiled_lets[0].span, parsed_let_spans[0],
        "let 'volume' span should match parsed LetDecl.span"
    );
}

/// Handle ParsedModule with parse errors → process valid declarations.
#[test]
fn handle_parse_errors_gracefully() {
    use reify_ast::*;
    use reify_core::*;

    let module = ParsedModule {
        path: ModulePath::single("partial"),
        declarations: vec![Declaration::Structure(StructureDef {
            name: "Partial".into(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![MemberDecl::Param(ParamDecl {
                name: "width".into(),
                doc: None,
                type_expr: Some(TypeExpr {
                    kind: TypeExprKind::Named {
                        name: "Scalar".into(),
                        type_args: vec![],
                    },
                    span: SourceSpan::new(0, 6),
                }),
                default: Some(Expr {
                    kind: ExprKind::QuantityLiteral {
                        value: 80.0,
                        unit: UnitExpr::Unit("mm".to_string()),
                    },
                    span: SourceSpan::new(9, 13),
                }),
                where_clause: None,
                annotations: Vec::new(),
                span: SourceSpan::new(0, 13),
                content_hash: ContentHash::of_str("param width: Scalar = 80mm"),
            })],
            span: SourceSpan::new(0, 50),
            content_hash: ContentHash::of_str("structure Partial"),
            pragmas: vec![],
            annotations: vec![],
        })],
        errors: vec![ParseError {
            message: "unexpected token".into(),
            span: SourceSpan::new(40, 45),
        }],
        content_hash: ContentHash::of_str("partial module"),
        pragmas: vec![],
        declared_module_path: None,
    };

    // Should not panic; should produce output for valid parts
    let compiled = reify_compiler::compile(&module);
    assert_eq!(compiled.templates.len(), 1);
}

/// Step 21: check_trait_conformance emits a diagnostic for unresolved type names
/// instead of silently falling back to Type::Real.
///
/// Constructs a module with:
/// - trait T requiring `param x : Real`
/// - structure S : T with `param x : NonexistentEnumType`
///
/// Before the fix (step 22), check_trait_conformance silently uses Type::Real for
/// the unresolved type, so assertion #3 (conformance-path message format) fails.
/// After the fix, a "unresolved type in conformance check: NonexistentEnumType"
/// diagnostic is emitted.
#[test]
fn reject_unresolved_type_in_trait_conformance() {
    use reify_ast::*;
    use reify_core::*;

    let module = ParsedModule {
        path: ModulePath::single("bad_conformance"),
        declarations: vec![
            // trait T { param x : Real }
            Declaration::Trait(TraitDecl {
                name: "T".into(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                refinements: vec![],
                members: vec![MemberDecl::Param(ParamDecl {
                    name: "x".into(),
                    doc: None,
                    type_expr: Some(TypeExpr {
                        kind: TypeExprKind::Named {
                            name: "Real".into(),
                            type_args: vec![],
                        },
                        span: SourceSpan::new(20, 24),
                    }),
                    default: None,
                    where_clause: None,
                    annotations: Vec::new(),
                    span: SourceSpan::new(16, 24),
                    content_hash: ContentHash::of_str("param x: Real"),
                })],
                span: SourceSpan::new(0, 30),
                content_hash: ContentHash::of_str("trait T"),
                pragmas: vec![],
                annotations: vec![],
            }),
            // structure def S : T { param x : NonexistentEnumType }
            Declaration::Structure(StructureDef {
                name: "S".into(),
                doc: None,
                is_pub: false,
                type_params: vec![],
                trait_bounds: vec![TraitBoundRef {
                    name: "T".into(),
                    type_args: vec![],
                    span: SourceSpan::new(50, 51),
                }],
                members: vec![MemberDecl::Param(ParamDecl {
                    name: "x".into(),
                    doc: None,
                    type_expr: Some(TypeExpr {
                        kind: TypeExprKind::Named {
                            name: "NonexistentEnumType".into(),
                            type_args: vec![],
                        },
                        span: SourceSpan::new(70, 89),
                    }),
                    default: None,
                    where_clause: None,
                    annotations: Vec::new(),
                    span: SourceSpan::new(66, 89),
                    content_hash: ContentHash::of_str("param x: NonexistentEnumType"),
                })],
                span: SourceSpan::new(45, 95),
                content_hash: ContentHash::of_str("structure S : T"),
                pragmas: vec![],
                annotations: vec![],
            }),
        ],
        errors: vec![],
        content_hash: ContentHash::of_str("bad conformance module"),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&module);

    // Assertion 1: diagnostics are non-empty
    assert!(
        !compiled.diagnostics.is_empty(),
        "should have diagnostics for unresolved type in conformance"
    );

    // Assertion 2: at least one diagnostic mentions both "unresolved type" and "NonexistentEnumType"
    let mentions_unresolved_and_name = compiled.diagnostics.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("unresolved type") && d.message.contains("NonexistentEnumType")
    });
    assert!(
        mentions_unresolved_and_name,
        "expected a diagnostic containing 'unresolved type' and 'NonexistentEnumType', got: {:?}",
        compiled
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // Assertion 3: specifically the conformance-path diagnostic must be present.
    // This assertion fails BEFORE the fix because check_trait_conformance
    // silently falls back to Type::Real without emitting a diagnostic.
    let has_conformance_diagnostic = compiled.diagnostics.iter().any(|d| {
        d.message.contains("unresolved type in conformance check")
            && d.message.contains("NonexistentEnumType")
    });
    assert!(
        has_conformance_diagnostic,
        "expected conformance-path diagnostic 'unresolved type in conformance check: NonexistentEnumType', \
         got diagnostics: {:?}",
        compiled
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}
