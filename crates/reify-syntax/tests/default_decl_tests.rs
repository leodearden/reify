//! CST→AST lowering tests for `default TypeName = expr` declarations.
//!
//! Task 4496, step-3 (TDD RED): references `Declaration::Default`, `DefaultDecl`,
//! and `PurposeDef.defaults`, which do not yet exist in reify-ast. The test
//! therefore fails to compile — the idiomatic RED for an API-surface addition in
//! this codebase (cf. reify-ast/tests/api_surface_decl.rs).
//!
//! Models on crates/reify-syntax/tests/unit_decl_tests.rs.

use reify_ast::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(
        source,
        reify_core::ModulePath::single("default_decl_test"),
    );
    (module.declarations, module.errors)
}

// ── Top-level `default Material = steel` ─────────────────────────────────

/// Parse a top-level `default Material = steel` and assert it lowers to
/// `Declaration::Default(DefaultDecl)` with the correct type_expr and value.
///
/// RED until step-4 adds `DefaultDecl` / `Declaration::Default` / lowering.
#[test]
fn parse_top_level_default_declaration() {
    let source = "default Material = steel";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1, "expected 1 declaration, got {:?}", decls);

    let d = match &decls[0] {
        Declaration::Default(d) => d,
        other => panic!("expected Declaration::Default, got {:?}", other),
    };

    // The `type` field must lower to a TypeExprKind::Named "Material".
    match &d.type_expr.kind {
        TypeExprKind::Named { name, type_args } => {
            assert_eq!(name, "Material", "expected type name 'Material', got '{name}'");
            assert!(
                type_args.is_empty(),
                "expected no type args, got {:?}",
                type_args
            );
        }
        other => panic!("expected TypeExprKind::Named for type_expr, got {:?}", other),
    }

    // The `value` field must lower to ExprKind::Ident("steel").
    match &d.value.kind {
        ExprKind::Ident(name) => {
            assert_eq!(name, "steel", "expected value 'steel', got '{name}'");
        }
        other => panic!("expected ExprKind::Ident(\"steel\") for value, got {:?}", other),
    }
}

// ── Purpose-nested `purpose Exploration() { default Material = steel }` ──

/// Parse a purpose-nested `default Material = steel` and assert it appears in
/// `PurposeDef.defaults` (NOT in `PurposeDef.members`) with the correct shape.
///
/// RED until step-4 adds `PurposeDef.defaults` field.
#[test]
fn parse_purpose_nested_default_declaration() {
    let source = "purpose Exploration() { default Material = steel }";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1, "expected 1 declaration, got {:?}", decls);

    let p = match &decls[0] {
        Declaration::Purpose(p) => p,
        other => panic!("expected Declaration::Purpose, got {:?}", other),
    };

    // The default must be in PurposeDef.defaults, not in PurposeDef.members.
    assert_eq!(
        p.defaults.len(),
        1,
        "expected 1 default in PurposeDef.defaults, got {:?}",
        p.defaults
    );
    assert!(
        p.members.is_empty(),
        "PurposeDef.members must be empty when body has only a default declaration; \
         got {:?}",
        p.members
    );

    let d = &p.defaults[0];

    // type_expr must be TypeExprKind::Named "Material".
    match &d.type_expr.kind {
        TypeExprKind::Named { name, type_args } => {
            assert_eq!(name, "Material", "expected type name 'Material', got '{name}'");
            assert!(type_args.is_empty(), "expected no type args");
        }
        other => panic!("expected TypeExprKind::Named for type_expr, got {:?}", other),
    }

    // value must be ExprKind::Ident("steel").
    match &d.value.kind {
        ExprKind::Ident(name) => {
            assert_eq!(name, "steel", "expected value 'steel', got '{name}'");
        }
        other => panic!("expected ExprKind::Ident(\"steel\") for value, got {:?}", other),
    }
}

// ── Parameterized type: `default Container<Material> = steel` ─────────────────

/// Parse `default Container<Material> = steel` and assert the type_expr lowers to
/// a parameterized TypeExprKind::Named with the correct name and type argument.
///
/// Exercises the `parameterized_type` branch of `lower_type_expr_node` inside
/// `lower_default_decl` — a non-trivial type path not covered by the bare-identifier
/// tests above.
#[test]
fn parse_top_level_default_with_parameterized_type() {
    let source = "default Container<Material> = steel";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1, "expected 1 declaration, got {:?}", decls);

    let d = match &decls[0] {
        Declaration::Default(d) => d,
        other => panic!("expected Declaration::Default, got {:?}", other),
    };

    // type_expr: Named "Container" with one type arg "Material"
    match &d.type_expr.kind {
        TypeExprKind::Named { name, type_args } => {
            assert_eq!(name, "Container", "expected type name 'Container', got '{name}'");
            assert_eq!(type_args.len(), 1, "expected 1 type arg, got {:?}", type_args);
            match &type_args[0].kind {
                TypeExprKind::Named { name: arg_name, type_args: inner } => {
                    assert_eq!(arg_name, "Material", "expected type arg 'Material', got '{arg_name}'");
                    assert!(inner.is_empty(), "expected no nested type args, got {:?}", inner);
                }
                other => panic!("expected Named type arg, got {:?}", other),
            }
        }
        other => panic!("expected TypeExprKind::Named for type_expr, got {:?}", other),
    }

    match &d.value.kind {
        ExprKind::Ident(name) => assert_eq!(name, "steel"),
        other => panic!("expected ExprKind::Ident(\"steel\"), got {:?}", other),
    }
}

// ── Compound value expression: `default Material = 1 + 2` ─────────────────────

/// Parse `default Material = 1 + 2` and assert the value lowers to a BinOp
/// expression.
///
/// Exercises the `_expression`-wide `lower_expr` path inside `lower_default_decl`
/// beyond the trivial single-identifier case — a non-trivial value path not
/// covered by the bare-identifier tests above.
#[test]
fn parse_top_level_default_with_compound_value() {
    let source = "default Material = 1 + 2";
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(decls.len(), 1, "expected 1 declaration, got {:?}", decls);

    let d = match &decls[0] {
        Declaration::Default(d) => d,
        other => panic!("expected Declaration::Default, got {:?}", other),
    };

    // type_expr: plain identifier "Material"
    match &d.type_expr.kind {
        TypeExprKind::Named { name, type_args } => {
            assert_eq!(name, "Material");
            assert!(type_args.is_empty());
        }
        other => panic!("expected TypeExprKind::Named for type_expr, got {:?}", other),
    }

    // value: BinOp(1 + 2)
    match &d.value.kind {
        ExprKind::BinOp { op, left, right } => {
            assert_eq!(op, "+", "expected '+' operator, got '{op}'");
            match &left.kind {
                ExprKind::NumberLiteral { value, .. } => {
                    assert!(
                        (*value - 1.0_f64).abs() < f64::EPSILON,
                        "expected left operand 1, got {value}"
                    );
                }
                other => panic!("expected NumberLiteral for left operand, got {:?}", other),
            }
            match &right.kind {
                ExprKind::NumberLiteral { value, .. } => {
                    assert!(
                        (*value - 2.0_f64).abs() < f64::EPSILON,
                        "expected right operand 2, got {value}"
                    );
                }
                other => panic!("expected NumberLiteral for right operand, got {:?}", other),
            }
        }
        other => panic!("expected ExprKind::BinOp for compound value, got {:?}", other),
    }
}

// ── Annotation on default declaration emits a diagnostic ──────────────────────

/// When an annotation immediately precedes a `default` declaration, a ParseError
/// must be emitted (defaults are not annotatable in v1). The default declaration
/// is still lowered so compilation can proceed without cascading errors.
///
/// Pins the behavior introduced by amendment 1 (robustness_silent_drop): the
/// annotation is no longer silently discarded.
#[test]
fn annotation_before_default_emits_parse_error() {
    let source = "@deprecated\ndefault Material = steel";
    let (decls, errors) = parse_decls(source);

    // A ParseError must be emitted describing the unsupported annotation.
    assert!(
        !errors.is_empty(),
        "expected at least one ParseError for the annotation on a default; got none"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("not annotatable") || e.message.contains("not supported")),
        "expected an error mentioning annotation is not annotatable/supported; got: {:?}",
        errors
    );

    // Despite the error, the default declaration is still produced.
    assert_eq!(
        decls.len(),
        1,
        "expected the default decl to still be lowered despite the annotation error; got {:?}",
        decls
    );
    assert!(
        matches!(decls[0], Declaration::Default(_)),
        "expected Declaration::Default, got {:?}",
        decls[0]
    );
}
