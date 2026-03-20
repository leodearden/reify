//! Lambda expression parsing tests.

use reify_syntax::*;

/// Helper: parse source and return the first structure's members and errors.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_types::ModulePath::single("lambda_test"));
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

/// step-1: Parse `|x| x * 2` as a lambda expression.
#[test]
fn parse_lambda_single_untyped_param() {
    let source = r#"
structure S {
    let f = |x| x * 2
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    assert_eq!(let_decl.name, "f");

    match &let_decl.value.kind {
        ExprKind::Lambda { params, body } => {
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].name, "x");
            assert!(params[0].type_expr.is_none());
            // Body: x * 2
            match &body.kind {
                ExprKind::BinOp { op, left, right } => {
                    assert_eq!(op, "*");
                    assert!(matches!(&left.kind, ExprKind::Ident(n) if n == "x"));
                    assert!(matches!(&right.kind, ExprKind::NumberLiteral(v) if *v == 2.0));
                }
                other => panic!("expected BinOp(*), got {:?}", other),
            }
        }
        other => panic!("expected Lambda, got {:?}", other),
    }
}
