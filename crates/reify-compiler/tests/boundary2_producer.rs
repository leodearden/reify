//! Boundary 2 (compiler → eval) — Producer-side tests.
//!
//! These tests verify that the compiler produces well-formed CompiledModules
//! that the evaluator can consume.

use reify_compiler::*;
use reify_test_support::*;

/// Compile bracket → verify TopologyTemplate structure.
#[test]
fn bracket_topology_structure() {
    let module = bracket_compiled_module();
    assert_eq!(module.templates.len(), 1);

    let template = &module.templates[0];
    assert_eq!(template.name, "Bracket");

    // 5 params + 1 let (volume) = 6 value cells
    assert_eq!(template.value_cells.len(), 6);

    let params: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.kind == ValueCellKind::Param)
        .collect();
    let lets: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.kind == ValueCellKind::Let)
        .collect();

    assert_eq!(params.len(), 5, "expected 5 param cells");
    assert_eq!(lets.len(), 1, "expected 1 let cell (volume)");

    // 3 constraints
    assert_eq!(template.constraints.len(), 3);
}

/// All CompiledExpr identifiers are ValueRef (never unresolved).
#[test]
fn all_identifiers_resolved() {
    let module = bracket_compiled_module();
    let template = &module.templates[0];

    // Check all constraint expressions
    for constraint in &template.constraints {
        assert_no_unresolved(&constraint.expr);
    }

    // Check all let expressions
    for vc in &template.value_cells {
        if let Some(expr) = &vc.default_expr {
            assert_no_unresolved(expr);
        }
    }
}

fn assert_no_unresolved(expr: &reify_ir::CompiledExpr) {
    use reify_ir::CompiledExprKind;
    match &expr.kind {
        CompiledExprKind::Literal(_) => {}
        CompiledExprKind::ValueRef(_) => {} // Resolved — good
        // CrossSubGeometryRef is a leaf (consumed by entity.rs before eval); treat as resolved.
        CompiledExprKind::CrossSubGeometryRef(_) => {}
        CompiledExprKind::BinOp { left, right, .. } => {
            assert_no_unresolved(left);
            assert_no_unresolved(right);
        }
        CompiledExprKind::UnOp { operand, .. } => {
            assert_no_unresolved(operand);
        }
        CompiledExprKind::FunctionCall { args, .. } => {
            for arg in args {
                assert_no_unresolved(arg);
            }
        }
        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            assert_no_unresolved(condition);
            assert_no_unresolved(then_branch);
            assert_no_unresolved(else_branch);
        }
        CompiledExprKind::Match { discriminant, arms } => {
            assert_no_unresolved(discriminant);
            for arm in arms {
                assert_no_unresolved(&arm.body);
            }
        }
        CompiledExprKind::UserFunctionCall { args, .. } => {
            for arg in args {
                assert_no_unresolved(arg);
            }
        }
        CompiledExprKind::Lambda { body, .. } => {
            assert_no_unresolved(body);
        }
        CompiledExprKind::ListLiteral(elements) => {
            for elem in elements {
                assert_no_unresolved(elem);
            }
        }
        CompiledExprKind::ReflectiveCellList(elements) => {
            for elem in elements {
                assert_no_unresolved(elem);
            }
        }
        CompiledExprKind::SetLiteral(elements) => {
            for elem in elements {
                assert_no_unresolved(elem);
            }
        }
        CompiledExprKind::MapLiteral(entries) => {
            for (key, val) in entries {
                assert_no_unresolved(key);
                assert_no_unresolved(val);
            }
        }
        CompiledExprKind::IndexAccess { object, index } => {
            assert_no_unresolved(object);
            assert_no_unresolved(index);
        }
        CompiledExprKind::MethodCall { object, args, .. } => {
            assert_no_unresolved(object);
            for arg in args {
                assert_no_unresolved(arg);
            }
        }
        CompiledExprKind::Quantifier {
            collection,
            predicate,
            ..
        } => {
            assert_no_unresolved(collection);
            assert_no_unresolved(predicate);
        }
        CompiledExprKind::OptionSome(inner) => {
            assert_no_unresolved(inner);
        }
        CompiledExprKind::OptionNone => {}
        CompiledExprKind::MetaAccess { .. } => {}
        CompiledExprKind::DeterminacyPredicate { .. } => {}
        CompiledExprKind::RangeConstructor { lower, upper, .. } => {
            if let Some(lo) = lower {
                assert_no_unresolved(lo);
            }
            if let Some(hi) = upper {
                assert_no_unresolved(hi);
            }
        }
        CompiledExprKind::AdHocSelector { base, args, .. } => {
            assert_no_unresolved(base);
            for arg in args {
                assert_no_unresolved(arg);
            }
        }
        // Reflective-aggregation placeholder (task-2289): leaf, expanded by
        // activate_purpose at runtime. Treated as resolved here.
        CompiledExprKind::PurposeReflectiveAggregation { .. } => {}
        // task 3540 (SIR-α): exhaustiveness-forced adapter arm for the new
        // shared-enum variant (step-16). Recurse into supplied args + captured
        // defaults so unresolved identifiers nested inside a structure ctor's
        // argument expressions are still asserted against.
        CompiledExprKind::StructureInstanceCtor {
            ordered_args,
            defaults,
            ..
        } => {
            for (_, arg) in ordered_args {
                assert_no_unresolved(arg);
            }
            for (_, def) in defaults {
                assert_no_unresolved(def);
            }
        }
        // task 4118 (γ): recurse into the wrapped selector.
        CompiledExprKind::ResolveSelector { selector } => {
            assert_no_unresolved(selector);
        }
    }
}

/// Type checking: constraint expr → Bool result type.
#[test]
fn constraint_result_types_are_bool() {
    let module = bracket_compiled_module();
    let template = &module.templates[0];

    for constraint in &template.constraints {
        assert_eq!(
            constraint.expr.result_type,
            reify_core::Type::Bool,
            "constraint {} should have Bool result type",
            constraint.id
        );
    }
}

/// Content hash is non-zero for all templates.
#[test]
fn content_hashes_present() {
    let module = bracket_compiled_module();
    assert_ne!(
        module.content_hash,
        reify_core::ContentHash(0),
        "module content hash should be non-zero"
    );
    for template in &module.templates {
        assert_ne!(
            template.content_hash,
            reify_core::ContentHash(0),
            "template content hash should be non-zero"
        );
    }
}

/// Type error detection: adding length to mass should fail.
#[test]
fn type_error_dimension_mismatch() {
    use reify_ast::*;
    use reify_core::*;

    // Build a module with: let bad = thickness + 2kg
    // thickness is Scalar(Length) via type_expr, 2kg is Scalar(Mass) literal
    let module = ParsedModule {
        path: ModulePath::single("dim_mismatch"),
        declarations: vec![Declaration::Structure(StructureDef {
            name: "Bad".into(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![
                MemberDecl::Param(ParamDecl {
                    name: "thickness".into(),
                    doc: None,
                    is_priv: false,
                    type_expr: Some(TypeExpr {
                        kind: TypeExprKind::Named {
                            name: "Length".into(),
                            type_args: vec![],
                        },
                        span: SourceSpan::new(0, 6),
                    }),
                    default: Some(Expr {
                        kind: ExprKind::QuantityLiteral {
                            value: 5.0,
                            unit: UnitExpr::Unit("mm".to_string()),
                        },
                        span: SourceSpan::new(9, 12),
                    }),
                    where_clause: None,
                    annotations: Vec::new(),
                    span: SourceSpan::new(0, 12),
                    content_hash: ContentHash::of_str("param thickness: Length = 5mm"),
                }),
                MemberDecl::Let(LetDecl {
                    name: "bad".into(),
                    doc: None,
                    is_pub: false,
                    is_aux: false,
                    type_expr: None,
                    value: Expr {
                        kind: ExprKind::BinOp {
                            op: "+".into(),
                            left: Box::new(Expr {
                                kind: ExprKind::Ident("thickness".into()),
                                span: SourceSpan::new(30, 39),
                            }),
                            right: Box::new(Expr {
                                kind: ExprKind::QuantityLiteral {
                                    value: 2.0,
                                    unit: UnitExpr::Unit("kg".to_string()),
                                },
                                span: SourceSpan::new(42, 45),
                            }),
                        },
                        span: SourceSpan::new(30, 45),
                    },
                    where_clause: None,
                    annotations: Vec::new(),
                    span: SourceSpan::new(25, 45),
                    content_hash: ContentHash::of_str("let bad = thickness + 2kg"),
                }),
            ],
            span: SourceSpan::new(0, 55),
            content_hash: ContentHash::of_str("structure Bad"),
            pragmas: vec![],
            annotations: vec![],
        })],
        errors: vec![],
        content_hash: ContentHash::of_str("dim_mismatch module"),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&module);
    assert!(
        !compiled.diagnostics.is_empty(),
        "should have diagnostics for dimension mismatch (Length + Mass)"
    );
    assert!(
        compiled.diagnostics.iter().any(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("dimension") || msg.contains("mismatch")
        }),
        "diagnostics should mention dimension mismatch, got: {:?}",
        compiled.diagnostics
    );
}

/// Constraint expression with non-Bool result type should produce a warning.
#[test]
fn constraint_non_bool_produces_warning() {
    use reify_ast::*;
    use reify_core::*;

    // Build a module with: constraint width * height
    // This produces Scalar[m^2], not Bool
    let module = ParsedModule {
        path: ModulePath::single("non_bool_constraint"),
        declarations: vec![Declaration::Structure(StructureDef {
            name: "Bad".into(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![
                MemberDecl::Param(ParamDecl {
                    name: "width".into(),
                    doc: None,
                    is_priv: false,
                    type_expr: Some(TypeExpr {
                        kind: TypeExprKind::Named {
                            name: "Length".into(),
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
                    content_hash: ContentHash::of_str("param width: Length = 80mm"),
                }),
                MemberDecl::Param(ParamDecl {
                    name: "height".into(),
                    doc: None,
                    is_priv: false,
                    type_expr: Some(TypeExpr {
                        kind: TypeExprKind::Named {
                            name: "Length".into(),
                            type_args: vec![],
                        },
                        span: SourceSpan::new(20, 26),
                    }),
                    default: Some(Expr {
                        kind: ExprKind::QuantityLiteral {
                            value: 100.0,
                            unit: UnitExpr::Unit("mm".to_string()),
                        },
                        span: SourceSpan::new(29, 34),
                    }),
                    where_clause: None,
                    annotations: Vec::new(),
                    span: SourceSpan::new(18, 34),
                    content_hash: ContentHash::of_str("param height: Length = 100mm"),
                }),
                MemberDecl::Constraint(ConstraintDecl {
                    label: None,
                    expr: Expr {
                        kind: ExprKind::BinOp {
                            op: "*".into(),
                            left: Box::new(Expr {
                                kind: ExprKind::Ident("width".into()),
                                span: SourceSpan::new(50, 55),
                            }),
                            right: Box::new(Expr {
                                kind: ExprKind::Ident("height".into()),
                                span: SourceSpan::new(58, 64),
                            }),
                        },
                        span: SourceSpan::new(50, 64),
                    },
                    where_clause: None,
                    span: SourceSpan::new(39, 64),
                    content_hash: ContentHash::of_str("constraint width * height"),
                }),
            ],
            span: SourceSpan::new(0, 70),
            content_hash: ContentHash::of_str("structure Bad non_bool"),
            pragmas: vec![],
            annotations: vec![],
        })],
        errors: vec![],
        content_hash: ContentHash::of_str("non_bool_constraint module"),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&module);
    assert!(
        !compiled.diagnostics.is_empty(),
        "should have diagnostics for non-Bool constraint expression"
    );
    assert!(
        compiled.diagnostics.iter().any(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("bool") || msg.contains("constraint")
        }),
        "diagnostics should mention Bool or constraint type issue, got: {:?}",
        compiled.diagnostics
    );
}

/// Compile auto param → ValueCellKind::Auto, default_expr: None.
#[test]
fn compile_auto_param() {
    let source = r#"structure S {
    param x: Length = auto
    param y: Length = 5mm
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    assert!(
        compiled.diagnostics.is_empty(),
        "compile diagnostics: {:?}",
        compiled.diagnostics
    );

    let template = &compiled.templates[0];
    assert_eq!(template.value_cells.len(), 2);

    // x should be Auto { free: false } with no default_expr
    let x = &template.value_cells[0];
    assert_eq!(x.id, reify_core::ValueCellId::new("S", "x"));
    assert_eq!(
        x.kind,
        ValueCellKind::Auto { free: false },
        "bare auto should compile to Auto {{ free: false }}"
    );
    assert!(
        x.default_expr.is_none(),
        "auto param should have no default_expr"
    );

    // y should be Param with a default_expr
    let y = &template.value_cells[1];
    assert_eq!(y.id, reify_core::ValueCellId::new("S", "y"));
    assert_eq!(y.kind, ValueCellKind::Param);
    assert!(
        y.default_expr.is_some(),
        "normal param should have default_expr"
    );
}

/// Compile `auto(free)` param → ValueCellKind::Auto { free: true }, default_expr: None.
#[test]
fn compile_auto_free_param() {
    let source = r#"structure S {
    param x: Length = auto(free)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    assert!(
        compiled.diagnostics.is_empty(),
        "compile diagnostics: {:?}",
        compiled.diagnostics
    );

    let template = &compiled.templates[0];
    assert_eq!(template.value_cells.len(), 1);

    let x = &template.value_cells[0];
    assert_eq!(x.id, reify_core::ValueCellId::new("S", "x"));
    assert_eq!(
        x.kind,
        ValueCellKind::Auto { free: true },
        "auto(free) should compile to Auto {{ free: true }}"
    );
    assert!(
        x.default_expr.is_none(),
        "auto(free) param should have no default_expr"
    );
}

/// Compile structure with both bare `auto` and `auto(free)` params.
#[test]
fn compile_mixed_auto_and_auto_free() {
    let source = r#"structure S {
    param a: Length = auto
    param b: Length = auto(free)
    param c: Length = 3mm
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    assert!(
        compiled.diagnostics.is_empty(),
        "compile diagnostics: {:?}",
        compiled.diagnostics
    );

    let template = &compiled.templates[0];
    assert_eq!(template.value_cells.len(), 3);

    let a = &template.value_cells[0];
    assert_eq!(a.id, reify_core::ValueCellId::new("S", "a"));
    assert_eq!(
        a.kind,
        ValueCellKind::Auto { free: false },
        "bare auto should compile to Auto {{ free: false }}"
    );

    let b = &template.value_cells[1];
    assert_eq!(b.id, reify_core::ValueCellId::new("S", "b"));
    assert_eq!(
        b.kind,
        ValueCellKind::Auto { free: true },
        "auto(free) should compile to Auto {{ free: true }}"
    );

    let c = &template.value_cells[2];
    assert_eq!(c.id, reify_core::ValueCellId::new("S", "c"));
    assert_eq!(c.kind, ValueCellKind::Param);
}

/// Port param with `auto(free)` default → port member ValueCellDecl has kind Auto { free: true }.
#[test]
fn compile_auto_free_in_port_param() {
    let source = r#"
trait MyPort {
    param foo : Length
}

structure def S {
    port mount : MyPort {
        param foo : Length = auto(free)
    }
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    // Filter to non-error diagnostics only (unknown port type warnings are ok)
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name.as_str().contains('S'))
        .expect("expected template S");

    // Port 'mount' should have exactly 1 member
    assert_eq!(template.ports.len(), 1, "expected 1 port");
    let port = &template.ports[0];
    assert_eq!(port.name, "mount");
    assert_eq!(port.members.len(), 1, "expected 1 port member");

    let foo = &port.members[0];
    assert_eq!(foo.id, reify_core::ValueCellId::new("S", "mount.foo"));
    assert_eq!(
        foo.kind,
        ValueCellKind::Auto { free: true },
        "port auto(free) param should compile to Auto {{ free: true }}"
    );
    assert!(
        foo.default_expr.is_none(),
        "auto(free) port param should have no default_expr"
    );
}

/// Guarded param with `auto(free)` default → guarded member ValueCellDecl has kind Auto { free: true }.
///
/// Exercises the guards.rs compile_guarded_members path.
#[test]
fn compile_auto_free_in_guarded_param() {
    let source = r#"
structure S {
    param active : Bool = true
    where active {
        param x : Length = auto(free)
    }
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let template = &compiled.templates[0];

    // x is guarded — should NOT appear in top-level value_cells
    assert!(
        !template.value_cells.iter().any(|vc| vc.id.member == "x"),
        "guarded param x should not be in top-level value_cells"
    );

    // Should have exactly 1 guarded group
    assert_eq!(template.guarded_groups.len(), 1, "expected 1 guarded group");
    let group = &template.guarded_groups[0];

    // Group should have x as its sole member
    assert_eq!(group.members.len(), 1, "expected 1 member in guarded group");
    let x = &group.members[0];
    assert!(
        x.id.member.contains("x"),
        "expected member 'x', got '{}'",
        x.id.member
    );
    assert_eq!(
        x.kind,
        ValueCellKind::Auto { free: true },
        "auto(free) guarded param should compile to Auto {{ free: true }}"
    );
    assert!(
        x.default_expr.is_none(),
        "auto(free) guarded param should have no default_expr"
    );
}

/// Auto param ValueCellDecl span should be non-zero and match parsed ParamDecl span.
#[test]
fn compiled_auto_param_span_not_zero() {
    use reify_core::SourceSpan;

    let source = r#"structure S {
    param x: Length = auto
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_auto_span"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    assert!(
        compiled.diagnostics.is_empty(),
        "compile diagnostics: {:?}",
        compiled.diagnostics
    );

    let template = &compiled.templates[0];
    let x = &template.value_cells[0];
    assert!(x.kind.is_auto());

    // Auto param span must not be (0,0)
    assert_ne!(
        x.span,
        SourceSpan::new(0, 0),
        "auto param span should not be (0,0) — must propagate from ParamDecl"
    );

    // Extract parsed param span for comparison
    let parsed_span = match &parsed.declarations[0] {
        reify_ast::Declaration::Structure(s) => match &s.members[0] {
            reify_ast::MemberDecl::Param(p) => p.span,
            _ => panic!("expected Param"),
        },
        _ => panic!("expected Structure"),
    };

    assert_eq!(
        x.span, parsed_span,
        "auto param span should match parsed ParamDecl.span"
    );
}

/// Regression: bracket fixture compiles with zero diagnostics.
/// The dimension and constraint checks must not false-positive on valid expressions.
#[test]
fn bracket_compiles_with_zero_diagnostics() {
    let parsed = bracket_parsed_module();
    let compiled = reify_compiler::compile(&parsed);
    assert!(
        compiled.diagnostics.is_empty(),
        "bracket should compile cleanly, but got diagnostics: {:?}",
        compiled.diagnostics
    );
}

/// Mul/Div with different Scalar dimensions should compile cleanly (no diagnostics).
/// Length * Length → Area, Length / Length → dimensionless Real.
#[test]
fn mul_div_different_dimensions_no_diagnostic() {
    use reify_ast::*;
    use reify_core::*;

    let module = ParsedModule {
        path: ModulePath::single("mul_div_dims"),
        declarations: vec![Declaration::Structure(StructureDef {
            name: "Good".into(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![
                MemberDecl::Param(ParamDecl {
                    name: "width".into(),
                    doc: None,
                    is_priv: false,
                    type_expr: Some(TypeExpr {
                        kind: TypeExprKind::Named {
                            name: "Length".into(),
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
                    content_hash: ContentHash::of_str("param width: Length = 80mm"),
                }),
                MemberDecl::Param(ParamDecl {
                    name: "height".into(),
                    doc: None,
                    is_priv: false,
                    type_expr: Some(TypeExpr {
                        kind: TypeExprKind::Named {
                            name: "Length".into(),
                            type_args: vec![],
                        },
                        span: SourceSpan::new(20, 26),
                    }),
                    default: Some(Expr {
                        kind: ExprKind::QuantityLiteral {
                            value: 100.0,
                            unit: UnitExpr::Unit("mm".to_string()),
                        },
                        span: SourceSpan::new(29, 34),
                    }),
                    where_clause: None,
                    annotations: Vec::new(),
                    span: SourceSpan::new(18, 34),
                    content_hash: ContentHash::of_str("param height: Length = 100mm"),
                }),
                // let area = width * height (Length * Length → Area)
                MemberDecl::Let(LetDecl {
                    name: "area".into(),
                    doc: None,
                    is_pub: false,
                    is_aux: false,
                    type_expr: None,
                    value: Expr {
                        kind: ExprKind::BinOp {
                            op: "*".into(),
                            left: Box::new(Expr {
                                kind: ExprKind::Ident("width".into()),
                                span: SourceSpan::new(50, 55),
                            }),
                            right: Box::new(Expr {
                                kind: ExprKind::Ident("height".into()),
                                span: SourceSpan::new(58, 64),
                            }),
                        },
                        span: SourceSpan::new(50, 64),
                    },
                    where_clause: None,
                    annotations: Vec::new(),
                    span: SourceSpan::new(39, 64),
                    content_hash: ContentHash::of_str("let area = width * height"),
                }),
                // let ratio = width / height (Length / Length → dimensionless Real)
                MemberDecl::Let(LetDecl {
                    name: "ratio".into(),
                    doc: None,
                    is_pub: false,
                    is_aux: false,
                    type_expr: None,
                    value: Expr {
                        kind: ExprKind::BinOp {
                            op: "/".into(),
                            left: Box::new(Expr {
                                kind: ExprKind::Ident("width".into()),
                                span: SourceSpan::new(80, 85),
                            }),
                            right: Box::new(Expr {
                                kind: ExprKind::Ident("height".into()),
                                span: SourceSpan::new(88, 94),
                            }),
                        },
                        span: SourceSpan::new(80, 94),
                    },
                    where_clause: None,
                    annotations: Vec::new(),
                    span: SourceSpan::new(70, 94),
                    content_hash: ContentHash::of_str("let ratio = width / height"),
                }),
            ],
            span: SourceSpan::new(0, 100),
            content_hash: ContentHash::of_str("structure Good mul_div"),
            pragmas: vec![],
            annotations: vec![],
        })],
        errors: vec![],
        content_hash: ContentHash::of_str("mul_div_dims module"),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&module);
    assert!(
        compiled.diagnostics.is_empty(),
        "Mul/Div with different Scalar dimensions should produce no diagnostics, got: {:?}",
        compiled.diagnostics
    );
}

/// Import declarations should be compiled into CompiledModule.imports, not silently dropped.
#[test]
fn import_compiles_into_module_imports() {
    let source = r#"import std.math

structure S {
    param w: Length = 80mm
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_import"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    assert_eq!(
        compiled.imports.len(),
        1,
        "expected 1 import, got {}",
        compiled.imports.len()
    );
    assert_eq!(compiled.imports[0].path, "std.math");
}

/// Import diagnostic: should produce exactly one warning mentioning the import path.
/// Compilation should still succeed (structures after import compile correctly).
#[test]
fn import_produces_warning_diagnostic() {
    let source = r#"import fasteners.bolt

structure S {
    param w: Length = 80mm
    param h: Length = 100mm
    constraint w > 0mm
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_import_diag"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // Should have exactly one diagnostic (the import warning)
    assert_eq!(
        compiled.diagnostics.len(),
        1,
        "expected 1 diagnostic, got {:?}",
        compiled.diagnostics
    );

    let diag = &compiled.diagnostics[0];
    assert_eq!(
        diag.severity,
        reify_core::Severity::Warning,
        "import diagnostic should be Warning, not Error"
    );
    assert!(
        diag.message.contains("import") && diag.message.contains("fasteners.bolt"),
        "diagnostic should mention import and path, got: {}",
        diag.message
    );

    // Structure after import should still compile correctly
    assert_eq!(compiled.templates.len(), 1);
    let template = &compiled.templates[0];
    assert_eq!(template.name, "S");
    assert_eq!(template.value_cells.len(), 2);
    assert_eq!(template.constraints.len(), 1);
}

/// Sub-structure declarations should be compiled into TopologyTemplate.sub_components.
#[test]
fn sub_compiles_into_template_sub_components() {
    let source = r#"structure Parent {
    param d: Length = 6mm
    sub mount_hole = Hole(diameter: 6mm)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_sub"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];

    assert_eq!(
        template.sub_components.len(),
        1,
        "expected 1 sub_component, got {}",
        template.sub_components.len()
    );

    let sub = &template.sub_components[0];
    assert_eq!(sub.name, "mount_hole");
    assert_eq!(sub.structure_name, "Hole");
    assert_eq!(sub.args.len(), 1, "expected 1 arg");
    assert_eq!(sub.args[0].0, "diameter");
}

/// Sub-structure args can reference parent params — expressions are compiled with
/// the parent's scope for name resolution.
#[test]
fn sub_args_reference_parent_params() {
    // Rib is defined in the same module so the compile-time existence check
    // (task 4528) accepts the sub declaration without emitting an error.
    let source = r#"structure S {
    param t: Length = 5mm
    sub rib = Rib(height: t * 0.8)
}
structure Rib {
    param height: Length = 1mm
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_sub_ref"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // No error diagnostics expected
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    let template = &compiled.templates[0];
    assert_eq!(template.sub_components.len(), 1);

    let sub = &template.sub_components[0];
    assert_eq!(sub.args[0].0, "height");

    // The arg expression should contain a ValueRef to 't' (resolved identifier)
    fn contains_value_ref(expr: &reify_ir::CompiledExpr, member: &str) -> bool {
        use reify_ir::CompiledExprKind;
        match &expr.kind {
            CompiledExprKind::ValueRef(id) => id.member == member,
            CompiledExprKind::BinOp { left, right, .. } => {
                contains_value_ref(left, member) || contains_value_ref(right, member)
            }
            CompiledExprKind::UnOp { operand, .. } => contains_value_ref(operand, member),
            CompiledExprKind::FunctionCall { args, .. } => {
                args.iter().any(|a| contains_value_ref(a, member))
            }
            _ => false,
        }
    }

    assert!(
        contains_value_ref(&sub.args[0].1, "t"),
        "sub arg expression should contain ValueRef to 't'"
    );
}

/// E2E: parse source with stdlib function in let binding, compile, eval_expr.
/// Validates the full pipeline: parse → compile (FunctionCall) → eval → stdlib dispatch.
#[test]
fn e2e_stdlib_function_in_let_binding() {
    let source = r#"structure S {
    param w: Length = 80mm
    let half_w = abs(w / 2)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_stdlib_e2e"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];

    // Find the 'half_w' let binding
    let half_w = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "half_w")
        .expect("should have half_w value cell");
    assert_eq!(half_w.kind, ValueCellKind::Let);
    let half_w_expr = half_w.default_expr.as_ref().expect("let should have expr");

    // Build a ValueMap with the param default value
    let mut values = reify_ir::ValueMap::new();
    let w_id = reify_core::ValueCellId::new("S", "w");
    // 80mm = 0.08m
    values.insert(w_id, reify_ir::Value::length(0.08));

    // Evaluate the let expression — should produce a defined value, NOT Undef
    let result = reify_expr::eval_expr(half_w_expr, &reify_expr::EvalContext::simple(&values));
    assert!(
        !result.is_undef(),
        "half_w = abs(w / 2) should produce a defined value, got Undef"
    );

    // abs(0.08 / 2) = abs(0.04) = 0.04
    let v = result.as_f64().unwrap();
    assert!((v - 0.04).abs() < 1e-10, "expected ~0.04, got {}", v);
}

/// Comprehensive: import + sub-structure + stdlib function in one module.
#[test]
fn comprehensive_all_three_features() {
    // Base is defined in the same module so the compile-time existence check
    // (task 4528) accepts the sub declaration without emitting an error.
    let source = r#"import std.math

structure Bracket {
    param w: Length = 80mm
    param h: Length = 100mm
    let diag = sqrt(w * w + h * h)
    sub base = Base(width: w)
    constraint diag > 0mm
}
structure Base {
    param width: Length = 1mm
}"#;
    let parsed = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("test_comprehensive"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // Imports: should have 1 entry
    assert_eq!(compiled.imports.len(), 1);
    assert_eq!(compiled.imports[0].path, "std.math");

    // Only the import warning diagnostic (no errors)
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    let warnings: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Warning)
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "expected 1 warning (import), got: {:?}",
        warnings
    );
    assert!(warnings[0].message.contains("import"));

    // Template structure
    let template = &compiled.templates[0];
    assert_eq!(template.name, "Bracket");

    // Sub-components: should have 1 entry
    assert_eq!(template.sub_components.len(), 1);
    assert_eq!(template.sub_components[0].name, "base");
    assert_eq!(template.sub_components[0].structure_name, "Base");

    // Eval: 'diag' let binding should produce non-Undef
    let diag_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "diag")
        .expect("should have diag value cell");
    let diag_expr = diag_cell
        .default_expr
        .as_ref()
        .expect("let should have expr");

    let mut values = reify_ir::ValueMap::new();
    values.insert(
        reify_core::ValueCellId::new("Bracket", "w"),
        reify_ir::Value::length(0.08),
    );
    values.insert(
        reify_core::ValueCellId::new("Bracket", "h"),
        reify_ir::Value::length(0.1),
    );

    let result = reify_expr::eval_expr(diag_expr, &reify_expr::EvalContext::simple(&values));
    assert!(
        !result.is_undef(),
        "diag = sqrt(w*w + h*h) should produce non-Undef, got Undef"
    );

    // sqrt(0.08^2 + 0.1^2) = sqrt(0.0064 + 0.01) = sqrt(0.0164) ≈ 0.12806
    let v = result.as_f64().unwrap();
    assert!(
        (v - 0.0164_f64.sqrt()).abs() < 1e-10,
        "expected ~0.128, got {}",
        v
    );
}

/// Two different source texts with the same module path should produce different
/// CompiledModule content_hashes — content changes must be reflected in the hash.
#[test]
fn different_content_same_path_different_hash() {
    let path = reify_core::ModulePath::single("bracket");

    let source_a = reify_test_support::bracket_source_with_width("80mm");
    let parsed_a = reify_syntax::parse(&source_a, path.clone());
    assert!(
        parsed_a.errors.is_empty(),
        "parse errors: {:?}",
        parsed_a.errors
    );
    let compiled_a = reify_compiler::compile(&parsed_a);

    let source_b = reify_test_support::bracket_source_with_width("120mm");
    let parsed_b = reify_syntax::parse(&source_b, path.clone());
    assert!(
        parsed_b.errors.is_empty(),
        "parse errors: {:?}",
        parsed_b.errors
    );
    let compiled_b = reify_compiler::compile(&parsed_b);

    assert_ne!(
        compiled_a.content_hash, compiled_b.content_hash,
        "different source content should produce different module content_hashes"
    );
}

/// Same source compiled twice should produce identical content_hashes (determinism).
#[test]
fn same_content_same_hash_deterministic() {
    let path = reify_core::ModulePath::single("bracket");
    let source = reify_test_support::bracket_source();

    let parsed_1 = reify_syntax::parse(source, path.clone());
    let compiled_1 = reify_compiler::compile(&parsed_1);

    let parsed_2 = reify_syntax::parse(source, path.clone());
    let compiled_2 = reify_compiler::compile(&parsed_2);

    assert_eq!(
        compiled_1.content_hash, compiled_2.content_hash,
        "same source compiled twice should produce identical module content_hashes"
    );

    // Also check template-level hashes
    assert_eq!(
        compiled_1.templates[0].content_hash, compiled_2.templates[0].content_hash,
        "same source compiled twice should produce identical template content_hashes"
    );
}

/// Changing a param default value should change the content_hash.
#[test]
fn param_default_change_changes_hash() {
    let path = reify_core::ModulePath::single("test_thickness");

    let source_a = r#"structure S {
    param thickness: Length = 5mm
}"#;
    let source_b = r#"structure S {
    param thickness: Length = 10mm
}"#;

    let parsed_a = reify_syntax::parse(source_a, path.clone());
    assert!(
        parsed_a.errors.is_empty(),
        "parse errors: {:?}",
        parsed_a.errors
    );
    let compiled_a = reify_compiler::compile(&parsed_a);

    let parsed_b = reify_syntax::parse(source_b, path.clone());
    assert!(
        parsed_b.errors.is_empty(),
        "parse errors: {:?}",
        parsed_b.errors
    );
    let compiled_b = reify_compiler::compile(&parsed_b);

    assert_ne!(
        compiled_a.content_hash, compiled_b.content_hash,
        "changing a param default value should change the module content_hash"
    );

    assert_ne!(
        compiled_a.templates[0].content_hash, compiled_b.templates[0].content_hash,
        "changing a param default value should change the template content_hash"
    );
}

/// Adding/removing a constraint should change the content_hash.
#[test]
fn add_constraint_changes_hash() {
    let path = reify_core::ModulePath::single("test_constraints");

    let source_2 = r#"structure S {
    param w: Length = 80mm
    param h: Length = 100mm
    constraint w > 0mm
    constraint h > 0mm
}"#;
    let source_3 = r#"structure S {
    param w: Length = 80mm
    param h: Length = 100mm
    constraint w > 0mm
    constraint h > 0mm
    constraint w > h
}"#;

    let parsed_2 = reify_syntax::parse(source_2, path.clone());
    assert!(
        parsed_2.errors.is_empty(),
        "parse errors: {:?}",
        parsed_2.errors
    );
    let compiled_2 = reify_compiler::compile(&parsed_2);

    let parsed_3 = reify_syntax::parse(source_3, path.clone());
    assert!(
        parsed_3.errors.is_empty(),
        "parse errors: {:?}",
        parsed_3.errors
    );
    let compiled_3 = reify_compiler::compile(&parsed_3);

    assert_ne!(
        compiled_2.content_hash, compiled_3.content_hash,
        "adding a constraint should change the module content_hash"
    );

    assert_ne!(
        compiled_2.templates[0].content_hash, compiled_3.templates[0].content_hash,
        "adding a constraint should change the template content_hash"
    );
}

/// Compile minimize → TopologyTemplate.objective is Some(Minimize(...)).
#[test]
fn compile_minimize_objective() {
    let source = r#"structure S {
    param x: Length = auto
    constraint x > 2mm
    minimize x
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_min_obj"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    assert!(
        compiled.diagnostics.is_empty(),
        "compile diagnostics: {:?}",
        compiled.diagnostics
    );

    let template = &compiled.templates[0];

    // Should have an objective
    let objective = template
        .objective
        .as_ref()
        .expect("template should have an objective");

    assert_eq!(objective.combination, reify_ir::ObjectiveCombination::WeightedSum);
    assert_eq!(objective.terms.len(), 1, "expected 1 term");
    let term = &objective.terms[0];
    assert_eq!(term.sense, reify_ir::ObjectiveSense::Minimize, "expected Minimize");
    // The expression should reference ValueCellId("S", "x")
    match &term.expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => {
            assert_eq!(id, &reify_core::ValueCellId::new("S", "x"));
        }
        other => panic!("expected ValueRef, got {:?}", other),
    }
}

/// Compile maximize → TopologyTemplate.objective is Some(Maximize(...)).
#[test]
fn compile_maximize_objective() {
    let source = r#"structure S {
    param w: Length = 80mm
    param h: Length = 100mm
    let volume = w * h
    maximize volume
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_max_obj"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    assert!(
        compiled.diagnostics.is_empty(),
        "compile diagnostics: {:?}",
        compiled.diagnostics
    );

    let template = &compiled.templates[0];

    let objective = template
        .objective
        .as_ref()
        .expect("template should have an objective");

    assert_eq!(objective.combination, reify_ir::ObjectiveCombination::WeightedSum);
    assert_eq!(objective.terms.len(), 1, "expected 1 term");
    let term = &objective.terms[0];
    assert_eq!(term.sense, reify_ir::ObjectiveSense::Maximize, "expected Maximize");
    // The expression should reference ValueCellId("S", "volume")
    match &term.expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => {
            assert_eq!(id, &reify_core::ValueCellId::new("S", "volume"));
        }
        other => panic!("expected ValueRef, got {:?}", other),
    }
}

/// Backward compatibility: bracket source (no minimize/maximize) → objective is None.
#[test]
fn no_objective_when_absent() {
    let parsed = bracket_parsed_module();
    let compiled = reify_compiler::compile(&parsed);
    assert!(
        compiled.diagnostics.is_empty(),
        "bracket should compile cleanly: {:?}",
        compiled.diagnostics
    );

    let template = &compiled.templates[0];
    assert!(
        template.objective.is_none(),
        "bracket has no minimize/maximize, objective should be None"
    );

    // Verify existing structure is unaffected
    assert_eq!(template.value_cells.len(), 6);
    assert_eq!(template.constraints.len(), 3);
}

/// E2E: parse source with auto params, constraints, lets, and minimize → compile → verify.
#[test]
fn e2e_minimize_round_trip() {
    let source = r#"structure Bracket {
    param width: Length = 80mm
    param height: Length = 100mm
    param thickness: Length = auto
    let volume = width * height * thickness
    constraint thickness > 2mm
    constraint thickness < width
    minimize thickness
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_e2e_min"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // (a) no error diagnostics
    assert!(
        compiled.diagnostics.is_empty(),
        "should have no diagnostics: {:?}",
        compiled.diagnostics
    );

    let template = &compiled.templates[0];

    // (b) correct value_cells count: 3 params + 1 let = 4
    assert_eq!(template.value_cells.len(), 4, "expected 4 value cells");

    // (c) correct constraints count
    assert_eq!(template.constraints.len(), 2, "expected 2 constraints");

    // (d) objective is Some(Minimize(...))
    let objective = template
        .objective
        .as_ref()
        .expect("template should have an objective");
    assert_eq!(objective.combination, reify_ir::ObjectiveCombination::WeightedSum);
    assert_eq!(objective.terms.len(), 1, "expected 1 term");
    let term = &objective.terms[0];
    assert_eq!(term.sense, reify_ir::ObjectiveSense::Minimize, "expected Minimize");
    // (e) compiled expression references ValueCellId for thickness
    match &term.expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => {
            assert_eq!(id, &reify_core::ValueCellId::new("Bracket", "thickness"));
        }
        other => panic!("expected ValueRef to thickness, got {:?}", other),
    }

    // (f) existing constraints and value cells are unaffected
    let auto_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.kind.is_auto())
        .collect();
    assert_eq!(auto_cells.len(), 1, "expected 1 auto param");
    assert_eq!(
        auto_cells[0].id,
        reify_core::ValueCellId::new("Bracket", "thickness")
    );

    let param_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.kind == ValueCellKind::Param)
        .collect();
    assert_eq!(param_cells.len(), 2, "expected 2 regular params");

    let let_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.kind == ValueCellKind::Let)
        .collect();
    assert_eq!(let_cells.len(), 1, "expected 1 let binding");
}

/// Enum declarations should be compiled into CompiledModule.enum_defs.
#[test]
fn compile_enum_populates_registry() {
    let source = r#"enum Direction { In, Out, Bidi }
structure S { param x: Length = 5mm }"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_enum_reg"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    assert_eq!(
        compiled.enum_defs.len(),
        1,
        "expected 1 enum_def, got {}",
        compiled.enum_defs.len()
    );
    assert_eq!(compiled.enum_defs[0].name, "Direction");
    assert_eq!(compiled.enum_defs[0].variants, vec!["In", "Out", "Bidi"]);
}

/// Enum access expression should compile to a literal Value::Enum.
#[test]
fn compile_enum_access_to_literal() {
    let source = r#"enum Direction { In, Out, Bidi }
structure S { let d = Direction.In }"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_enum_access"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // No error diagnostics expected
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    let template = &compiled.templates[0];
    let d_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "d")
        .expect("should have 'd' value cell");

    let d_expr = d_cell.default_expr.as_ref().expect("let should have expr");

    match &d_expr.kind {
        reify_ir::CompiledExprKind::Literal(reify_ir::Value::Enum { type_name, variant }) => {
            assert_eq!(type_name, "Direction");
            assert_eq!(variant, "In");
        }
        other => panic!("expected Literal(Enum), got {:?}", other),
    }

    assert_eq!(
        d_expr.result_type,
        reify_core::Type::Enum("Direction".into())
    );
}

/// Unknown enum type produces diagnostic (parser sees MemberAccess, not EnumAccess).
#[test]
fn compile_unknown_enum_produces_diagnostic() {
    let source = r#"structure S { let d = UnknownEnum.Variant }"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_unknown_enum"));
    let compiled = reify_compiler::compile(&parsed);
    assert!(
        !compiled.diagnostics.is_empty(),
        "expected diagnostics for unknown enum access"
    );
}

/// E2E: enum equality evaluates through full pipeline.
#[test]
fn e2e_enum_equality_eval() {
    let source = r#"enum Direction { In, Out, Bidi }
structure S { constraint Direction.In == Direction.In }"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_enum_e2e"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // No error diagnostics
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    let template = &compiled.templates[0];
    assert_eq!(template.constraints.len(), 1);

    let constraint_expr = &template.constraints[0].expr;
    let result = reify_expr::eval_expr(
        constraint_expr,
        &reify_expr::EvalContext::simple(&reify_ir::ValueMap::new()),
    );
    match result {
        reify_ir::Value::Bool(true) => {}
        other => panic!("expected Bool(true), got {:?}", other),
    }
}

// ── User-defined function tests ────────────────────────────

/// Compile a simple function definition.
#[test]
fn compile_simple_function() {
    let source = "fn double(x: Real) -> Real { x + x }";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_fn"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    assert!(
        compiled.diagnostics.is_empty(),
        "compile diagnostics: {:?}",
        compiled.diagnostics
    );

    assert_eq!(compiled.functions.len(), 1, "expected 1 function");
    let f = &compiled.functions[0];
    assert_eq!(f.name, "double");
    assert!(!f.is_pub);
    assert_eq!(f.params, vec![("x".to_string(), reify_core::Type::dimensionless_scalar())]);
    assert_eq!(f.return_type, reify_core::Type::dimensionless_scalar());
    assert!(f.body.let_bindings.is_empty());
    assert_eq!(f.body.result_expr.result_type, reify_core::Type::dimensionless_scalar());
}

/// Compile a function with let bindings in body.
#[test]
fn compile_function_with_let_bindings() {
    let source = "fn f(x: Real) -> Real { let y = x + x; let z = y * y; z + 1 }";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_fn_lets"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    assert!(
        compiled.diagnostics.is_empty(),
        "compile diagnostics: {:?}",
        compiled.diagnostics
    );

    let f = &compiled.functions[0];
    assert_eq!(f.body.let_bindings.len(), 2);
    assert_eq!(f.body.let_bindings[0].0, "y");
    assert_eq!(f.body.let_bindings[1].0, "z");
    // result_expr should compile without unresolved name errors
    assert_eq!(f.body.result_expr.result_type, reify_core::Type::dimensionless_scalar());
}

/// Two overloaded functions with the same name but different param types.
#[test]
fn compile_overloaded_functions() {
    let source = "fn convert(x: Real) -> Real { x }\nfn convert(x: Int) -> Int { x }";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_fn_overload"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    assert!(
        compiled.diagnostics.is_empty(),
        "compile diagnostics: {:?}",
        compiled.diagnostics
    );

    assert_eq!(compiled.functions.len(), 2);
    assert_eq!(compiled.functions[0].name, "convert");
    assert_eq!(compiled.functions[1].name, "convert");
    assert_eq!(
        compiled.functions[0].params,
        vec![("x".to_string(), reify_core::Type::dimensionless_scalar())]
    );
    assert_eq!(
        compiled.functions[1].params,
        vec![("x".to_string(), reify_core::Type::Int)]
    );
}

/// Two functions with identical name AND param types should produce a diagnostic.
#[test]
fn compile_duplicate_function_signature_error() {
    let source = "fn f(x: Real) -> Real { x }\nfn f(x: Real) -> Int { x }";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_fn_dup"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    assert!(
        !compiled.diagnostics.is_empty(),
        "expected diagnostics for duplicate function signature"
    );
    assert!(
        compiled.diagnostics.iter().any(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("duplicate") || msg.contains("conflict")
        }),
        "diagnostics should mention duplicate, got: {:?}",
        compiled.diagnostics
    );
}

/// CompiledConstraint domain field: compiler sets it to None by default.
/// Construction with Some(domain) is also valid.
#[test]
fn compiled_constraint_domain_field() {
    // Verify compiler-produced constraints have domain: None
    let module = bracket_compiled_module();
    let template = &module.templates[0];
    for constraint in &template.constraints {
        assert!(
            constraint.domain.is_none(),
            "compiler should set domain to None by default, but constraint {} has {:?}",
            constraint.id,
            constraint.domain
        );
    }

    // Verify manual construction with Some(domain) works
    use reify_core::{ConstraintNodeId, SourceSpan};
    use reify_ir::ConstraintDomain;
    let manual = reify_compiler::CompiledConstraint {
        id: ConstraintNodeId::new("Test", 0),
        label: Some("test".to_string()),
        expr: reify_ir::CompiledExpr::literal(
            reify_ir::Value::Bool(true),
            reify_core::Type::Bool,
        ),
        span: SourceSpan::new(0, 0),
        domain: Some(ConstraintDomain::Dimensional),
        optimized_target: None,
        arg_bindings: Vec::new(),
    };
    assert_eq!(manual.domain, Some(ConstraintDomain::Dimensional));

    // Verify None construction backward compatibility
    let compat = reify_compiler::CompiledConstraint {
        id: ConstraintNodeId::new("Test", 1),
        label: None,
        expr: reify_ir::CompiledExpr::literal(
            reify_ir::Value::Bool(true),
            reify_core::Type::Bool,
        ),
        span: SourceSpan::new(0, 0),
        domain: None,
        optimized_target: None,
        arg_bindings: Vec::new(),
    };
    assert!(compat.domain.is_none());
}

/// Scalar + Int is a type error: adding dimensioned and dimensionless values.
#[test]
fn scalar_plus_int_type_error() {
    use reify_ast::*;
    use reify_core::*;

    let module = ParsedModule {
        path: ModulePath::single("scalar_plus_int"),
        declarations: vec![Declaration::Structure(StructureDef {
            name: "Bad".into(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![
                MemberDecl::Param(ParamDecl {
                    name: "width".into(),
                    doc: None,
                    is_priv: false,
                    type_expr: Some(TypeExpr {
                        kind: TypeExprKind::Named {
                            name: "Length".into(),
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
                    content_hash: ContentHash::of_str("param width: Length = 80mm"),
                }),
                // let bad = width + 5
                MemberDecl::Let(LetDecl {
                    name: "bad".into(),
                    doc: None,
                    is_pub: false,
                    is_aux: false,
                    type_expr: None,
                    value: Expr {
                        kind: ExprKind::BinOp {
                            op: "+".into(),
                            left: Box::new(Expr {
                                kind: ExprKind::Ident("width".into()),
                                span: SourceSpan::new(30, 35),
                            }),
                            right: Box::new(Expr {
                                kind: ExprKind::NumberLiteral {
                                    value: 5.0,
                                    is_real: false,
                                },
                                span: SourceSpan::new(38, 39),
                            }),
                        },
                        span: SourceSpan::new(30, 39),
                    },
                    where_clause: None,
                    annotations: Vec::new(),
                    span: SourceSpan::new(20, 39),
                    content_hash: ContentHash::of_str("let bad = width + 5"),
                }),
            ],
            span: SourceSpan::new(0, 45),
            content_hash: ContentHash::of_str("structure Bad scalar_plus_int"),
            pragmas: vec![],
            annotations: vec![],
        })],
        errors: vec![],
        content_hash: ContentHash::of_str("scalar_plus_int module"),
        pragmas: vec![],
        declared_module_path: None,
    };

    let compiled = reify_compiler::compile(&module);
    assert!(
        !compiled.diagnostics.is_empty(),
        "should have diagnostics for Scalar + Int (dimensioned + dimensionless)"
    );
    assert!(
        compiled.diagnostics.iter().any(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("dimension") || msg.contains("incompatible") || msg.contains("mismatch")
        }),
        "diagnostics should mention type incompatibility, got: {:?}",
        compiled.diagnostics
    );
}

/// Forward reference: structure declared BEFORE the enum it references.
/// Order-independent declarations require all enums to be available regardless of source order.
#[test]
fn compile_enum_forward_reference_order_independent() {
    let source = r#"structure S { let d = Direction.In }
enum Direction { In, Out, Bidi }"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_enum_fwd"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // No error diagnostics expected — enum should be resolved despite forward reference
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics for forward enum ref, got: {:?}",
        errors
    );

    let template = &compiled.templates[0];
    let d_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "d")
        .expect("should have 'd' value cell");

    let d_expr = d_cell.default_expr.as_ref().expect("let should have expr");

    match &d_expr.kind {
        reify_ir::CompiledExprKind::Literal(reify_ir::Value::Enum { type_name, variant }) => {
            assert_eq!(type_name, "Direction");
            assert_eq!(variant, "In");
        }
        other => panic!("expected Literal(Enum), got {:?}", other),
    }

    assert_eq!(
        d_expr.result_type,
        reify_core::Type::Enum("Direction".into())
    );
}

/// Multiple enums and structures interleaved in various orders.
/// Validates two-pass compilation handles forward and backward references.
#[test]
fn compile_enum_forward_reference_multiple_enums() {
    let source = r#"structure A { let x = Color.Red }
enum Direction { In, Out }
structure B {
    let y = Direction.In
    let z = Color.Red
}
enum Color { Red, Green, Blue }"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_enum_multi"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    // No error diagnostics expected
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    // Should have 2 enum_defs
    assert_eq!(compiled.enum_defs.len(), 2, "expected 2 enum_defs");

    // Template A: x → Value::Enum(Color, Red)
    let template_a = &compiled.templates[0];
    let x_cell = template_a
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("should have 'x' value cell");
    let x_expr = x_cell.default_expr.as_ref().expect("let should have expr");
    match &x_expr.kind {
        reify_ir::CompiledExprKind::Literal(reify_ir::Value::Enum { type_name, variant }) => {
            assert_eq!(type_name, "Color");
            assert_eq!(variant, "Red");
        }
        other => panic!("A.x: expected Literal(Enum(Color, Red)), got {:?}", other),
    }

    // Template B: y → Value::Enum(Direction, In), z → Value::Enum(Color, Red)
    let template_b = &compiled.templates[1];
    let y_cell = template_b
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "y")
        .expect("should have 'y' value cell");
    let y_expr = y_cell.default_expr.as_ref().expect("let should have expr");
    match &y_expr.kind {
        reify_ir::CompiledExprKind::Literal(reify_ir::Value::Enum { type_name, variant }) => {
            assert_eq!(type_name, "Direction");
            assert_eq!(variant, "In");
        }
        other => panic!(
            "B.y: expected Literal(Enum(Direction, In)), got {:?}",
            other
        ),
    }

    let z_cell = template_b
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "z")
        .expect("should have 'z' value cell");
    let z_expr = z_cell.default_expr.as_ref().expect("let should have expr");
    match &z_expr.kind {
        reify_ir::CompiledExprKind::Literal(reify_ir::Value::Enum { type_name, variant }) => {
            assert_eq!(type_name, "Color");
            assert_eq!(variant, "Red");
        }
        other => panic!("B.z: expected Literal(Enum(Color, Red)), got {:?}", other),
    }
}

/// Calling a user-defined function from a structure should compile to UserFunctionCall.
#[test]
fn compile_user_function_call() {
    let source = "fn double(x: Real) -> Real { x + x }\nstructure S { let v = double(1.5) }";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_fn_call"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    assert!(
        compiled.diagnostics.is_empty(),
        "compile diagnostics: {:?}",
        compiled.diagnostics
    );

    let template = &compiled.templates[0];
    let v_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "v")
        .expect("should have 'v' value cell");
    let v_expr = v_cell.default_expr.as_ref().expect("let should have expr");

    // Should be UserFunctionCall, not stdlib FunctionCall
    match &v_expr.kind {
        reify_ir::CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } => {
            assert_eq!(function_name, "double");
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected UserFunctionCall, got {:?}", other),
    }
    assert_eq!(v_expr.result_type, reify_core::Type::dimensionless_scalar());
}

/// Overload resolution should pick the correct overload based on argument types.
#[test]
fn compile_overload_resolution_picks_correct() {
    let source = "fn process(x: Real) -> Real { x + 1 }\nfn process(x: Int) -> Int { x }\nstructure S { let a = process(3.14) }";
    let parsed = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("test_fn_overload_resolve"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    assert!(
        compiled.diagnostics.is_empty(),
        "compile diagnostics: {:?}",
        compiled.diagnostics
    );

    let template = &compiled.templates[0];
    let a_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "a")
        .expect("should have 'a' value cell");
    let a_expr = a_cell.default_expr.as_ref().expect("let should have expr");

    match &a_expr.kind {
        reify_ir::CompiledExprKind::UserFunctionCall { function_name, .. } => {
            assert_eq!(function_name, "process");
        }
        other => panic!("expected UserFunctionCall, got {:?}", other),
    }
    // 3.14 is Real, so it matches the Real overload
    assert_eq!(a_expr.result_type, reify_core::Type::dimensionless_scalar());
}

/// Forward reference: structure calls a function declared AFTER it.
#[test]
fn compile_function_forward_reference() {
    let source = "structure S { let v = double(1.5) }\nfn double(x: Real) -> Real { x + x }";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_fn_fwd"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    assert!(
        compiled.diagnostics.is_empty(),
        "compile diagnostics: {:?}",
        compiled.diagnostics
    );

    let template = &compiled.templates[0];
    let v_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "v")
        .expect("should have 'v' value cell");
    let v_expr = v_cell.default_expr.as_ref().expect("let should have expr");

    match &v_expr.kind {
        reify_ir::CompiledExprKind::UserFunctionCall { function_name, .. } => {
            assert_eq!(function_name, "double");
        }
        other => panic!("expected UserFunctionCall, got {:?}", other),
    }
}

/// Fn body calling another user-defined function should produce UserFunctionCall.
#[test]
fn compile_fn_body_calls_other_user_fn() {
    let source =
        "fn double(x: Real) -> Real { x + x }\nfn quadruple(x: Real) -> Real { double(double(x)) }";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    let compiled = reify_compiler::compile(&parsed);

    assert!(
        compiled.diagnostics.is_empty(),
        "no diagnostics expected: {:?}",
        compiled.diagnostics
    );
    assert_eq!(compiled.functions.len(), 2);
    assert_eq!(compiled.functions[0].name, "double");
    assert_eq!(compiled.functions[1].name, "quadruple");

    // quadruple's result_expr should be UserFunctionCall { function_name: "double", .. }
    let q_body = &compiled.functions[1].body;
    assert!(q_body.let_bindings.is_empty());

    // Outer call: double(double(x))
    match &q_body.result_expr.kind {
        reify_ir::CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } => {
            assert_eq!(function_name, "double", "outer call should be double");
            assert_eq!(args.len(), 1);
            assert_eq!(q_body.result_expr.result_type, reify_core::Type::dimensionless_scalar());

            // Inner call: double(x)
            match &args[0].kind {
                reify_ir::CompiledExprKind::UserFunctionCall {
                    function_name: inner_name,
                    args: inner_args,
                } => {
                    assert_eq!(inner_name, "double", "inner call should be double");
                    assert_eq!(inner_args.len(), 1);
                    assert_eq!(args[0].result_type, reify_core::Type::dimensionless_scalar());
                }
                other => panic!("expected inner UserFunctionCall, got {:?}", other),
            }
        }
        other => panic!("expected outer UserFunctionCall, got {:?}", other),
    }
}

/// Fn body let bindings can call other user-defined functions.
#[test]
fn compile_fn_body_calls_user_fn_in_let_binding() {
    let source = "fn double(x: Real) -> Real { x + x }\nfn calc(x: Real) -> Real { let y = double(x); y + 1 }";
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    let compiled = reify_compiler::compile(&parsed);

    assert!(
        compiled.diagnostics.is_empty(),
        "no diagnostics expected: {:?}",
        compiled.diagnostics
    );
    assert_eq!(compiled.functions.len(), 2);
    assert_eq!(compiled.functions[1].name, "calc");

    let calc_body = &compiled.functions[1].body;
    assert_eq!(calc_body.let_bindings.len(), 1);

    // let y = double(x); — should be UserFunctionCall
    match &calc_body.let_bindings[0].1.kind {
        reify_ir::CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } => {
            assert_eq!(function_name, "double");
            assert_eq!(args.len(), 1);
            assert_eq!(
                calc_body.let_bindings[0].1.result_type,
                reify_core::Type::dimensionless_scalar()
            );
        }
        other => panic!("expected UserFunctionCall in let binding, got {:?}", other),
    }

    // result expr: y + 1 — should be BinOp with result_type Real
    assert_eq!(calc_body.result_expr.result_type, reify_core::Type::dimensionless_scalar());
    match &calc_body.result_expr.kind {
        reify_ir::CompiledExprKind::BinOp { op, .. } => {
            assert_eq!(*op, reify_ir::BinOp::Add);
        }
        other => panic!("expected BinOp(+) for result expr, got {:?}", other),
    }
}

/// E2E regression: bracket source (no fn declarations) compiles unchanged.
#[test]
fn e2e_function_with_structure_unchanged() {
    let parsed = bracket_parsed_module();
    let compiled = reify_compiler::compile(&parsed);

    assert!(
        compiled.diagnostics.is_empty(),
        "bracket should compile cleanly: {:?}",
        compiled.diagnostics
    );

    // Bracket has no function declarations
    assert!(
        compiled.functions.is_empty(),
        "bracket has no fn declarations, functions should be empty"
    );

    // Existing structure should be unaffected
    let template = &compiled.templates[0];
    assert_eq!(template.value_cells.len(), 6, "expected 6 value cells");
    assert_eq!(template.constraints.len(), 3, "expected 3 constraints");
    assert_eq!(template.realizations.len(), 1, "expected 1 realization");
}
