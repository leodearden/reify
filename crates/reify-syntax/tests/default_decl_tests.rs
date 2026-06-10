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
