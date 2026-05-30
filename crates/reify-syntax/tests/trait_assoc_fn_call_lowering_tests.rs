//! Trait associated-fn call-expr lowering round-trip tests.
//!
//! Step 3 (RED): these tests fail to compile until step-4 adds:
//! - `ExprKind::TraitStaticCall { trait_name, method, args }`
//! - `ExprKind::TraitMethodCall { object, trait_name, method, args }`

use reify_ast::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("call_lowering_test"));
    (module.declarations, module.errors)
}

/// Helper: find the first `MemberDecl::Let` in the first structure declaration.
fn first_let_in_structure(decls: &[Declaration]) -> &LetDecl {
    match &decls[0] {
        Declaration::Structure(s) => {
            for m in &s.members {
                if let MemberDecl::Let(l) = m {
                    return l;
                }
            }
            panic!("no Let member found in structure")
        }
        other => panic!("expected Structure, got {:?}", other),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Test 1: Trait::method() → TraitStaticCall
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn trait_static_call_lowers_to_trait_static_call() {
    let (decls, errors) = parse_decls("structure def A { let s = C::make() }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = first_let_in_structure(&decls);
    match &let_decl.value.kind {
        ExprKind::TraitStaticCall { trait_name, method, args } => {
            assert_eq!(trait_name, "C");
            assert_eq!(method, "make");
            assert!(args.is_empty(), "expected no args, got {:?}", args);
        }
        other => panic!("expected TraitStaticCall, got {:?}", other),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Test 2: obj.(Trait::method)() → TraitMethodCall
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn instance_trait_call_lowers_to_trait_method_call() {
    let (decls, errors) =
        parse_decls("structure def A { sub pin : P  let w = pin.(C::area)() }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = first_let_in_structure(&decls);
    match &let_decl.value.kind {
        ExprKind::TraitMethodCall { object, trait_name, method, args } => {
            assert!(
                matches!(&object.kind, ExprKind::Ident(n) if n == "pin"),
                "expected Ident(pin), got {:?}",
                object.kind
            );
            assert_eq!(trait_name, "C");
            assert_eq!(method, "area");
            assert!(args.is_empty(), "expected no args, got {:?}", args);
        }
        other => panic!("expected TraitMethodCall, got {:?}", other),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Test 3: args preserved in TraitStaticCall
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn trait_static_call_preserves_args() {
    let (decls, errors) = parse_decls("structure def A { let d = D::f(5mm, x) }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = first_let_in_structure(&decls);
    match &let_decl.value.kind {
        ExprKind::TraitStaticCall { trait_name, method, args } => {
            assert_eq!(trait_name, "D");
            assert_eq!(method, "f");
            assert_eq!(args.len(), 2, "expected 2 args, got {:?}", args);
        }
        other => panic!("expected TraitStaticCall, got {:?}", other),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Test 4 (regression): bare C::make (no ()) stays QualifiedAccess
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn bare_qualified_access_not_a_call_stays_qualified_access() {
    let (decls, errors) = parse_decls("structure def A { let a = C::make }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = first_let_in_structure(&decls);
    match &let_decl.value.kind {
        ExprKind::QualifiedAccess { qualifier, member } => {
            assert!(
                matches!(&qualifier.kind, ExprKind::Ident(n) if n == "C"),
                "expected qualifier Ident(C), got {:?}",
                qualifier.kind
            );
            assert_eq!(member, "make");
        }
        other => panic!("expected QualifiedAccess (no trailing ()), got {:?}", other),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Test 5 (regression): bare pin.(C::area) (no ()) stays InstanceQualifiedAccess
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn bare_instance_qualified_access_not_a_call_stays_instance_qualified_access() {
    let (decls, errors) = parse_decls("structure def A { let b = pin.(C::area) }");
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = first_let_in_structure(&decls);
    match &let_decl.value.kind {
        ExprKind::InstanceQualifiedAccess { object, qualified } => {
            assert!(
                matches!(&object.kind, ExprKind::Ident(n) if n == "pin"),
                "expected object Ident(pin), got {:?}",
                object.kind
            );
            match &qualified.kind {
                ExprKind::QualifiedAccess { qualifier, member } => {
                    assert!(
                        matches!(&qualifier.kind, ExprKind::Ident(n) if n == "C"),
                        "expected qualifier Ident(C), got {:?}",
                        qualifier.kind
                    );
                    assert_eq!(member, "area");
                }
                other => panic!("expected inner QualifiedAccess, got {:?}", other),
            }
        }
        other => panic!(
            "expected InstanceQualifiedAccess (no trailing ()), got {:?}",
            other
        ),
    }
}
