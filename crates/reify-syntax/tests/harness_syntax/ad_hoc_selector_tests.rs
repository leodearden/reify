//! Ad-hoc port selector parsing integration tests.
//!
//! Tests for `expr @ ident(args)` selector expressions.

use reify_ast::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("adhoc_test"));
    (module.declarations, module.errors)
}

// ── Step 6: member access binds tighter than @ ────────────────────────────────

#[test]
fn parse_ad_hoc_selector_chained_member_access() {
    let (decls, errors) = parse_decls(r#"structure S { let x = sub_part.mount @ face("top") }"#);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = match &structure.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    match &let_decl.value.kind {
        ExprKind::AdHocSelector {
            base,
            selector,
            args,
        } => {
            assert_eq!(selector, "face");
            assert_eq!(args.len(), 1);
            assert!(matches!(&args[0].kind, ExprKind::StringLiteral(s) if s == "top"));
            // base must be a MemberAccess, not just an Ident — verifies member access
            // binds tighter than @
            match &base.kind {
                ExprKind::MemberAccess { object, member } => {
                    assert!(matches!(&object.kind, ExprKind::Ident(n) if n == "sub_part"));
                    assert_eq!(member, "mount");
                }
                other => panic!("expected MemberAccess as base, got {:?}", other),
            }
        }
        other => panic!("expected AdHocSelector, got {:?}", other),
    }
}

// ── Step 7: ad-hoc selector in connect port refs ──────────────────────────────

#[test]
fn parse_ad_hoc_selector_in_connect() {
    let (decls, errors) = parse_decls(
        r#"structure S {
            port a : out T
            port b : in T
            connect a @ face("top") -> b @ face("bottom")
        }"#,
    );
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    // 2 ports + 1 connect = 3 members
    assert_eq!(structure.members.len(), 3);

    let connect = match &structure.members[2] {
        MemberDecl::Connect(c) => c,
        other => panic!("expected Connect, got {:?}", other),
    };

    // Check left port ref: a @ face("top")
    match &connect.left.expr.kind {
        ExprKind::AdHocSelector {
            base,
            selector,
            args,
        } => {
            assert!(matches!(&base.kind, ExprKind::Ident(n) if n == "a"));
            assert_eq!(selector, "face");
            assert_eq!(args.len(), 1);
            assert!(matches!(&args[0].kind, ExprKind::StringLiteral(s) if s == "top"));
        }
        other => panic!("expected AdHocSelector for left, got {:?}", other),
    }

    // Check right port ref: b @ face("bottom")
    match &connect.right.expr.kind {
        ExprKind::AdHocSelector {
            base,
            selector,
            args,
        } => {
            assert!(matches!(&base.kind, ExprKind::Ident(n) if n == "b"));
            assert_eq!(selector, "face");
            assert_eq!(args.len(), 1);
            assert!(matches!(&args[0].kind, ExprKind::StringLiteral(s) if s == "bottom"));
        }
        other => panic!("expected AdHocSelector for right, got {:?}", other),
    }
}

// ── Step 8: @ binds tighter than [] ──────────────────────────────────────────

#[test]
fn parse_ad_hoc_selector_precedence_over_index() {
    let (decls, errors) = parse_decls(r#"structure S { let x = port @ face("top")[0] }"#);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = match &structure.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    // Outer expression must be IndexAccess ([] binds looser than @)
    match &let_decl.value.kind {
        ExprKind::IndexAccess { object, index } => {
            assert!(
                matches!(&index.kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 0.0).abs() < f64::EPSILON)
            );
            // The indexed object must be AdHocSelector
            match &object.kind {
                ExprKind::AdHocSelector {
                    base,
                    selector,
                    args,
                } => {
                    assert!(matches!(&base.kind, ExprKind::Ident(n) if n == "port"));
                    assert_eq!(selector, "face");
                    assert_eq!(args.len(), 1);
                    assert!(matches!(&args[0].kind, ExprKind::StringLiteral(s) if s == "top"));
                }
                other => panic!("expected AdHocSelector inside IndexAccess, got {:?}", other),
            }
        }
        other => panic!("expected IndexAccess as outer expression, got {:?}", other),
    }
}

// ── Step 9: complex expressions as selector args ──────────────────────────────

#[test]
fn parse_ad_hoc_selector_with_expr_args() {
    let (decls, errors) = parse_decls("structure S { let x = port @ edge(width * 2) }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    let let_decl = match &structure.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    match &let_decl.value.kind {
        ExprKind::AdHocSelector {
            base,
            selector,
            args,
        } => {
            assert!(matches!(&base.kind, ExprKind::Ident(n) if n == "port"));
            assert_eq!(selector, "edge");
            assert_eq!(args.len(), 1);
            // The argument must be a BinOp expression
            match &args[0].kind {
                ExprKind::BinOp { op, left, right } => {
                    assert_eq!(op, "*");
                    assert!(matches!(&left.kind, ExprKind::Ident(n) if n == "width"));
                    assert!(
                        matches!(&right.kind, ExprKind::NumberLiteral { value: v, .. } if (*v - 2.0).abs() < f64::EPSILON)
                    );
                }
                other => panic!("expected BinOp as arg, got {:?}", other),
            }
        }
        other => panic!("expected AdHocSelector, got {:?}", other),
    }
}
