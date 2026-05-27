//! Quantifier expression parsing tests.

use reify_syntax::*;

/// Helper: parse source and return the first structure's members and errors.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("quantifier_test"));
    let structure = match &module
        .declarations
        .iter()
        .find(|d| matches!(d, Declaration::Structure(_)))
    {
        Some(Declaration::Structure(s)) => s.clone(),
        other => panic!("expected Structure, got {:?}", other),
    };
    (structure.members.clone(), module.errors.clone())
}

/// step-1: Parse `forall x in list: x > 0` -> Quantifier(ForAll)
#[test]
fn parse_forall_expression() {
    let source = r#"
structure S {
    let items = [1, 2, 3]
    constraint forall x in items: x > 0
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let constraint = match &members[1] {
        MemberDecl::Constraint(c) => c,
        other => panic!("expected Constraint, got {:?}", other),
    };

    match &constraint.expr.kind {
        ExprKind::Quantifier {
            kind,
            variable,
            collection,
            predicate,
        } => {
            assert_eq!(*kind, QuantifierKind::ForAll);
            assert_eq!(variable, "x");
            assert!(matches!(&collection.kind, ExprKind::Ident(n) if n == "items"));
            match &predicate.kind {
                ExprKind::BinOp { op, left, right } => {
                    assert_eq!(op, ">");
                    assert!(matches!(&left.kind, ExprKind::Ident(n) if n == "x"));
                    assert!(
                        matches!(&right.kind, ExprKind::NumberLiteral { value: v, .. } if *v == 0.0)
                    );
                }
                other => panic!("expected BinOp(>), got {:?}", other),
            }
        }
        other => panic!("expected Quantifier, got {:?}", other),
    }
}

/// step-1: Parse `exists x in set: x == target` -> Quantifier(Exists)
#[test]
fn parse_exists_expression() {
    let source = r#"
structure S {
    let target = 5
    let items = [1, 2, 3, 5]
    let found = exists x in items: x == target
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[2] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    assert_eq!(let_decl.name, "found");

    match &let_decl.value.kind {
        ExprKind::Quantifier {
            kind,
            variable,
            collection,
            predicate,
        } => {
            assert_eq!(*kind, QuantifierKind::Exists);
            assert_eq!(variable, "x");
            assert!(matches!(&collection.kind, ExprKind::Ident(n) if n == "items"));
            match &predicate.kind {
                ExprKind::BinOp { op, .. } => {
                    assert_eq!(op, "==");
                }
                other => panic!("expected BinOp(==), got {:?}", other),
            }
        }
        other => panic!("expected Quantifier, got {:?}", other),
    }
}
