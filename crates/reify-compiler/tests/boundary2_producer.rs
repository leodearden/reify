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

    // Build a module with: let bad = thickness + mass_val
    // where thickness is Scalar(Length) and mass_val is Scalar(Mass)
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
                MemberDecl::Param(ParamDecl {
                    name: "mass_val".into(),
                    type_expr: None,
                    default: Some(Expr {
                        kind: ExprKind::QuantityLiteral {
                            value: 2.0,
                            unit: "kg".into(),
                        },
                        span: SourceSpan::new(20, 23),
                    }),
                    span: SourceSpan::new(15, 23),
                    content_hash: ContentHash::of_str("param mass_val = 2kg"),
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
                                kind: ExprKind::Ident("mass_val".into()),
                                span: SourceSpan::new(42, 50),
                            }),
                        },
                        span: SourceSpan::new(30, 50),
                    },
                    span: SourceSpan::new(25, 50),
                    content_hash: ContentHash::of_str("let bad = thickness + mass_val"),
                }),
            ],
            span: SourceSpan::new(0, 60),
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
