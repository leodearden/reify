//! Guard (where) clause tests.
//!
//! Tests for per-declaration postfix guards (`param x : Length where cond`)
//! and block-level guards (`where cond { ...members... } else { ...members... }`).

use reify_syntax::*;

/// Helper: parse source and return the first structure's members.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("guard_test"));
    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    (structure.members.clone(), module.errors.clone())
}

/// Parse `param x : Length where needs_cooling` — expects ParamDecl with WhereClause.
#[test]
fn param_with_where_clause() {
    let source = r#"structure S {
    param x : Scalar where needs_cooling
}"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    assert_eq!(members.len(), 1);

    let param = match &members[0] {
        MemberDecl::Param(p) => p,
        other => panic!("expected Param, got {:?}", other),
    };
    assert_eq!(param.name, "x");

    let wc = param.where_clause.as_ref().expect("expected where_clause");
    match &wc.condition.kind {
        ExprKind::Ident(name) => assert_eq!(name, "needs_cooling"),
        other => panic!("expected Ident('needs_cooling'), got {:?}", other),
    }
}

/// Backward compatibility: bracket source with no guards parses with where_clause=None.
#[test]
fn bracket_source_no_guards() {
    let source = reify_test_support::bracket_source();
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("bracket"));
    assert!(module.errors.is_empty(), "parse errors: {:?}", module.errors);

    let structure = match &module.declarations[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };

    for (i, member) in structure.members.iter().enumerate() {
        match member {
            MemberDecl::Param(p) => {
                assert!(
                    p.where_clause.is_none(),
                    "param {} ({}) should have no where_clause",
                    i,
                    p.name
                );
            }
            MemberDecl::Let(l) => {
                assert!(
                    l.where_clause.is_none(),
                    "let {} ({}) should have no where_clause",
                    i,
                    l.name
                );
            }
            MemberDecl::Constraint(c) => {
                assert!(
                    c.where_clause.is_none(),
                    "constraint {} should have no where_clause",
                    i,
                );
            }
            MemberDecl::Sub(s) => {
                assert!(
                    s.where_clause.is_none(),
                    "sub {} ({}) should have no where_clause",
                    i,
                    s.name
                );
            }
            MemberDecl::Minimize(m) => {
                assert!(
                    m.where_clause.is_none(),
                    "minimize {} should have no where_clause",
                    i,
                );
            }
            MemberDecl::Maximize(m) => {
                assert!(
                    m.where_clause.is_none(),
                    "maximize {} should have no where_clause",
                    i,
                );
            }
            _ => {}
        }
    }
}
