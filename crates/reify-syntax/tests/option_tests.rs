//! Parser tests for some(expr) and none expressions.
//! These tests verify the existing parser handles some/none via existing
//! function_call and identifier rules — no grammar changes needed.

use reify_ast::*;

/// Helper: parse source and return the first structure's members and errors.
fn parse_members(source: &str) -> (Vec<MemberDecl>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("option_test"));
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

/// step-1a: `some(42)` parses as FunctionCall { name: "some", args: [NumberLiteral(42)] }
#[test]
fn parse_some_integer_literal() {
    let source = r#"
structure S {
    let x = some(42)
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    assert_eq!(let_decl.name, "x");

    match &let_decl.value.kind {
        ExprKind::FunctionCall { name, args } => {
            assert_eq!(name, "some");
            assert_eq!(args.len(), 1);
            assert!(
                matches!(&args[0].kind, ExprKind::NumberLiteral { value: v, .. } if *v == 42.0),
                "expected NumberLiteral(42), got {:?}",
                args[0].kind
            );
        }
        other => panic!("expected FunctionCall, got {:?}", other),
    }
}

/// step-1b: `some(x + 1)` parses as FunctionCall with BinOp arg.
#[test]
fn parse_some_binop_arg() {
    let source = r#"
structure S {
    param x: Real
    let y = some(x + 1)
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    // The let is the second member
    let let_decl = match &members[1] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    assert_eq!(let_decl.name, "y");

    match &let_decl.value.kind {
        ExprKind::FunctionCall { name, args } => {
            assert_eq!(name, "some");
            assert_eq!(args.len(), 1);
            assert!(
                matches!(&args[0].kind, ExprKind::BinOp { op, .. } if op == "+"),
                "expected BinOp(+), got {:?}",
                args[0].kind
            );
        }
        other => panic!("expected FunctionCall, got {:?}", other),
    }
}

/// step-1c: `some(some(x))` nests correctly — FunctionCall wrapping FunctionCall.
#[test]
fn parse_some_nested() {
    let source = r#"
structure S {
    param x: Real
    let y = some(some(x))
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[1] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    match &let_decl.value.kind {
        ExprKind::FunctionCall { name, args } => {
            assert_eq!(name, "some");
            assert_eq!(args.len(), 1);
            // Inner call also FunctionCall { name: "some", ... }
            match &args[0].kind {
                ExprKind::FunctionCall {
                    name: inner_name,
                    args: inner_args,
                } => {
                    assert_eq!(inner_name, "some");
                    assert_eq!(inner_args.len(), 1);
                    assert!(
                        matches!(&inner_args[0].kind, ExprKind::Ident(n) if n == "x"),
                        "expected Ident(x), got {:?}",
                        inner_args[0].kind
                    );
                }
                other => panic!("expected inner FunctionCall, got {:?}", other),
            }
        }
        other => panic!("expected outer FunctionCall, got {:?}", other),
    }
}

/// step-1d: `none` as a let value parses as Ident("none").
#[test]
fn parse_none_as_let_value() {
    let source = r#"
structure S {
    let x = none
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    assert_eq!(let_decl.name, "x");

    assert!(
        matches!(&let_decl.value.kind, ExprKind::Ident(n) if n == "none"),
        "expected Ident(none), got {:?}",
        let_decl.value.kind
    );
}

/// step-1e: `none` as a param default parses as Ident("none").
#[test]
fn parse_none_as_param_default() {
    let source = r#"
structure S {
    param x: Real = none
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let param_decl = match &members[0] {
        MemberDecl::Param(p) => p,
        other => panic!("expected Param, got {:?}", other),
    };
    assert_eq!(param_decl.name, "x");

    let default = param_decl.default.as_ref().expect("should have a default");
    assert!(
        matches!(&default.kind, ExprKind::Ident(n) if n == "none"),
        "expected Ident(none), got {:?}",
        default.kind
    );
}

/// step-1f: some/none in conditional branches parse correctly.
#[test]
fn parse_some_none_in_conditional() {
    let source = r#"
structure S {
    param flag: Bool
    let val = if flag then some(1) else none
}
"#;
    let (members, errors) = parse_members(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);

    let let_decl = match &members[1] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };

    match &let_decl.value.kind {
        ExprKind::Conditional {
            condition: _,
            then_branch,
            else_branch,
        } => {
            // then: some(1)
            assert!(
                matches!(&then_branch.kind, ExprKind::FunctionCall { name, .. } if name == "some"),
                "expected some() in then branch, got {:?}",
                then_branch.kind
            );
            // else: none
            assert!(
                matches!(&else_branch.kind, ExprKind::Ident(n) if n == "none"),
                "expected none in else branch, got {:?}",
                else_branch.kind
            );
        }
        other => panic!("expected Conditional expression, got {:?}", other),
    }
}
