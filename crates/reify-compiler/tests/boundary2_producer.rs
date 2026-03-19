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

fn assert_no_unresolved(expr: &reify_types::CompiledExpr) {
    use reify_types::CompiledExprKind;
    match &expr.kind {
        CompiledExprKind::Literal(_) => {}
        CompiledExprKind::ValueRef(_) => {} // Resolved — good
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
            reify_types::Type::Bool,
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
        reify_types::ContentHash(0),
        "module content hash should be non-zero"
    );
    for template in &module.templates {
        assert_ne!(
            template.content_hash,
            reify_types::ContentHash(0),
            "template content hash should be non-zero"
        );
    }
}

/// Type error detection: adding length to mass should fail.
#[test]
fn type_error_dimension_mismatch() {
    use reify_syntax::*;
    use reify_types::*;

    // Build a module with: let bad = thickness + 2kg
    // thickness is Scalar(Length) via type_expr, 2kg is Scalar(Mass) literal
    let module = ParsedModule {
        path: ModulePath::single("dim_mismatch"),
        declarations: vec![Declaration::Structure(StructureDef {
            name: "Bad".into(),
            members: vec![
                MemberDecl::Param(ParamDecl {
                    name: "thickness".into(),
                    type_expr: Some(TypeExpr {
                        name: "Scalar".into(),
                        span: SourceSpan::new(0, 6),
                    }),
                    default: Some(Expr {
                        kind: ExprKind::QuantityLiteral {
                            value: 5.0,
                            unit: "mm".into(),
                        },
                        span: SourceSpan::new(9, 12),
                    }),
                    span: SourceSpan::new(0, 12),
                    content_hash: ContentHash::of_str("param thickness: Scalar = 5mm"),
                }),
                MemberDecl::Let(LetDecl {
                    name: "bad".into(),
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
                                    unit: "kg".into(),
                                },
                                span: SourceSpan::new(42, 45),
                            }),
                        },
                        span: SourceSpan::new(30, 45),
                    },
                    span: SourceSpan::new(25, 45),
                    content_hash: ContentHash::of_str("let bad = thickness + 2kg"),
                }),
            ],
            span: SourceSpan::new(0, 55),
            content_hash: ContentHash::of_str("structure Bad"),
        })],
        errors: vec![],
        content_hash: ContentHash::of_str("dim_mismatch module"),
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
    use reify_syntax::*;
    use reify_types::*;

    // Build a module with: constraint width * height
    // This produces Scalar[m^2], not Bool
    let module = ParsedModule {
        path: ModulePath::single("non_bool_constraint"),
        declarations: vec![Declaration::Structure(StructureDef {
            name: "Bad".into(),
            members: vec![
                MemberDecl::Param(ParamDecl {
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
                }),
                MemberDecl::Param(ParamDecl {
                    name: "height".into(),
                    type_expr: Some(TypeExpr {
                        name: "Scalar".into(),
                        span: SourceSpan::new(20, 26),
                    }),
                    default: Some(Expr {
                        kind: ExprKind::QuantityLiteral {
                            value: 100.0,
                            unit: "mm".into(),
                        },
                        span: SourceSpan::new(29, 34),
                    }),
                    span: SourceSpan::new(18, 34),
                    content_hash: ContentHash::of_str("param height: Scalar = 100mm"),
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
                    span: SourceSpan::new(39, 64),
                    content_hash: ContentHash::of_str("constraint width * height"),
                }),
            ],
            span: SourceSpan::new(0, 70),
            content_hash: ContentHash::of_str("structure Bad non_bool"),
        })],
        errors: vec![],
        content_hash: ContentHash::of_str("non_bool_constraint module"),
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
    param x: Scalar = auto
    param y: Scalar = 5mm
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    assert!(compiled.diagnostics.is_empty(), "compile diagnostics: {:?}", compiled.diagnostics);

    let template = &compiled.templates[0];
    assert_eq!(template.value_cells.len(), 2);

    // x should be Auto with no default_expr
    let x = &template.value_cells[0];
    assert_eq!(x.id, reify_types::ValueCellId::new("S", "x"));
    assert_eq!(x.kind, ValueCellKind::Auto);
    assert!(x.default_expr.is_none(), "auto param should have no default_expr");

    // y should be Param with a default_expr
    let y = &template.value_cells[1];
    assert_eq!(y.id, reify_types::ValueCellId::new("S", "y"));
    assert_eq!(y.kind, ValueCellKind::Param);
    assert!(y.default_expr.is_some(), "normal param should have default_expr");
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
    use reify_syntax::*;
    use reify_types::*;

    let module = ParsedModule {
        path: ModulePath::single("mul_div_dims"),
        declarations: vec![Declaration::Structure(StructureDef {
            name: "Good".into(),
            members: vec![
                MemberDecl::Param(ParamDecl {
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
                }),
                MemberDecl::Param(ParamDecl {
                    name: "height".into(),
                    type_expr: Some(TypeExpr {
                        name: "Scalar".into(),
                        span: SourceSpan::new(20, 26),
                    }),
                    default: Some(Expr {
                        kind: ExprKind::QuantityLiteral {
                            value: 100.0,
                            unit: "mm".into(),
                        },
                        span: SourceSpan::new(29, 34),
                    }),
                    span: SourceSpan::new(18, 34),
                    content_hash: ContentHash::of_str("param height: Scalar = 100mm"),
                }),
                // let area = width * height (Length * Length → Area)
                MemberDecl::Let(LetDecl {
                    name: "area".into(),
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
                    span: SourceSpan::new(39, 64),
                    content_hash: ContentHash::of_str("let area = width * height"),
                }),
                // let ratio = width / height (Length / Length → dimensionless Real)
                MemberDecl::Let(LetDecl {
                    name: "ratio".into(),
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
                    span: SourceSpan::new(70, 94),
                    content_hash: ContentHash::of_str("let ratio = width / height"),
                }),
            ],
            span: SourceSpan::new(0, 100),
            content_hash: ContentHash::of_str("structure Good mul_div"),
        })],
        errors: vec![],
        content_hash: ContentHash::of_str("mul_div_dims module"),
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
    let source = r#"import "std/math"

structure S {
    param w: Scalar = 80mm
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_import"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    assert_eq!(
        compiled.imports.len(),
        1,
        "expected 1 import, got {}",
        compiled.imports.len()
    );
    assert_eq!(compiled.imports[0].path, "std/math");
}

/// Import diagnostic: should produce exactly one warning mentioning the import path.
/// Compilation should still succeed (structures after import compile correctly).
#[test]
fn import_produces_warning_diagnostic() {
    let source = r#"import "fasteners/bolt"

structure S {
    param w: Scalar = 80mm
    param h: Scalar = 100mm
    constraint w > 0mm
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_import_diag"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

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
        reify_types::Severity::Warning,
        "import diagnostic should be Warning, not Error"
    );
    assert!(
        diag.message.contains("import") && diag.message.contains("fasteners/bolt"),
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
    param d: Scalar = 6mm
    sub mount_hole = Hole(diameter: 6mm)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_sub"));
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
    let source = r#"structure S {
    param t: Scalar = 5mm
    sub rib = Rib(height: t * 0.8)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_sub_ref"));
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
        .filter(|d| d.severity == reify_types::Severity::Error)
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
    fn contains_value_ref(expr: &reify_types::CompiledExpr, member: &str) -> bool {
        use reify_types::CompiledExprKind;
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
    param w: Scalar = 80mm
    let half_w = abs(w / 2)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_stdlib_e2e"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

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
    let mut values = reify_types::ValueMap::new();
    let w_id = reify_types::ValueCellId::new("S", "w");
    // 80mm = 0.08m
    values.insert(w_id, reify_types::Value::length(0.08));

    // Evaluate the let expression — should produce a defined value, NOT Undef
    let result = reify_expr::eval_expr(half_w_expr, &values);
    assert!(
        !result.is_undef(),
        "half_w = abs(w / 2) should produce a defined value, got Undef"
    );

    // abs(0.08 / 2) = abs(0.04) = 0.04
    let v = result.as_f64().unwrap();
    assert!(
        (v - 0.04).abs() < 1e-10,
        "expected ~0.04, got {}",
        v
    );
}

/// Comprehensive: import + sub-structure + stdlib function in one module.
#[test]
fn comprehensive_all_three_features() {
    let source = r#"import "std/math"

structure Bracket {
    param w: Scalar = 80mm
    param h: Scalar = 100mm
    let diag = sqrt(w * w + h * h)
    sub base = Base(width: w)
    constraint diag > 0mm
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_comprehensive"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    // Imports: should have 1 entry
    assert_eq!(compiled.imports.len(), 1);
    assert_eq!(compiled.imports[0].path, "std/math");

    // Only the import warning diagnostic (no errors)
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no error diagnostics, got: {:?}", errors);

    let warnings: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Warning)
        .collect();
    assert_eq!(warnings.len(), 1, "expected 1 warning (import), got: {:?}", warnings);
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
    let diag_expr = diag_cell.default_expr.as_ref().expect("let should have expr");

    let mut values = reify_types::ValueMap::new();
    values.insert(
        reify_types::ValueCellId::new("Bracket", "w"),
        reify_types::Value::length(0.08),
    );
    values.insert(
        reify_types::ValueCellId::new("Bracket", "h"),
        reify_types::Value::length(0.1),
    );

    let result = reify_expr::eval_expr(diag_expr, &values);
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
    let path = reify_types::ModulePath::single("bracket");

    let source_a = reify_test_support::bracket_source_with_width("80mm");
    let parsed_a = reify_syntax::parse(&source_a, path.clone());
    assert!(parsed_a.errors.is_empty(), "parse errors: {:?}", parsed_a.errors);
    let compiled_a = reify_compiler::compile(&parsed_a);

    let source_b = reify_test_support::bracket_source_with_width("120mm");
    let parsed_b = reify_syntax::parse(&source_b, path.clone());
    assert!(parsed_b.errors.is_empty(), "parse errors: {:?}", parsed_b.errors);
    let compiled_b = reify_compiler::compile(&parsed_b);

    assert_ne!(
        compiled_a.content_hash, compiled_b.content_hash,
        "different source content should produce different module content_hashes"
    );
}

/// Scalar + Int is a type error: adding dimensioned and dimensionless values.
#[test]
fn scalar_plus_int_type_error() {
    use reify_syntax::*;
    use reify_types::*;

    let module = ParsedModule {
        path: ModulePath::single("scalar_plus_int"),
        declarations: vec![Declaration::Structure(StructureDef {
            name: "Bad".into(),
            members: vec![
                MemberDecl::Param(ParamDecl {
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
                }),
                // let bad = width + 5
                MemberDecl::Let(LetDecl {
                    name: "bad".into(),
                    type_expr: None,
                    value: Expr {
                        kind: ExprKind::BinOp {
                            op: "+".into(),
                            left: Box::new(Expr {
                                kind: ExprKind::Ident("width".into()),
                                span: SourceSpan::new(30, 35),
                            }),
                            right: Box::new(Expr {
                                kind: ExprKind::NumberLiteral(5.0),
                                span: SourceSpan::new(38, 39),
                            }),
                        },
                        span: SourceSpan::new(30, 39),
                    },
                    span: SourceSpan::new(20, 39),
                    content_hash: ContentHash::of_str("let bad = width + 5"),
                }),
            ],
            span: SourceSpan::new(0, 45),
            content_hash: ContentHash::of_str("structure Bad scalar_plus_int"),
        })],
        errors: vec![],
        content_hash: ContentHash::of_str("scalar_plus_int module"),
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
