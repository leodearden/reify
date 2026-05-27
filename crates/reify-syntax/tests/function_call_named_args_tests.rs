//! Integration tests for function_call lowering with named arguments.
//!
//! Verifies that named arguments in function calls are stripped of their
//! labels and lowered as positional args in `ExprKind::FunctionCall`.

use reify_ast::*;

/// Helper: parse source and return declarations and errors.
fn parse_decls(source: &str) -> (Vec<Declaration>, Vec<ParseError>) {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("fn_call_test"));
    (module.declarations, module.errors)
}

/// Helper: extract the `value` Expr from the first Let in the first Structure.
fn first_let_value(source: &str) -> Expr {
    let (decls, errors) = parse_decls(source);
    assert!(errors.is_empty(), "parse errors: {:?}", errors);
    let structure = match &decls[0] {
        Declaration::Structure(s) => s,
        other => panic!("expected Structure, got {:?}", other),
    };
    let let_decl = match &structure.members[0] {
        MemberDecl::Let(l) => l,
        other => panic!("expected Let, got {:?}", other),
    };
    let_decl.value.clone()
}

#[test]
fn function_call_single_named_arg_strips_name() {
    // foo(a: 1.0) should lower to FunctionCall { name: "foo", args: [1.0] }
    let expr = first_let_value(r#"structure S { let x = foo(a: 1.0) }"#);
    match expr.kind {
        ExprKind::FunctionCall { name, args } => {
            assert_eq!(name, "foo");
            assert_eq!(args.len(), 1, "expected 1 positional arg, got {:?}", args);
            assert!(
                matches!(args[0].kind, ExprKind::NumberLiteral { value: v, .. } if (v - 1.0).abs() < 1e-10),
                "expected NumberLiteral(1.0), got {:?}",
                args[0].kind
            );
        }
        other => panic!("expected FunctionCall, got {:?}", other),
    }
}

#[test]
fn function_call_multiple_named_args_strips_names() {
    // foo(a: 1.0, b: 2.0) should lower to FunctionCall { name: "foo", args: [1.0, 2.0] }
    let expr = first_let_value(r#"structure S { let x = foo(a: 1.0, b: 2.0) }"#);
    match expr.kind {
        ExprKind::FunctionCall { name, args } => {
            assert_eq!(name, "foo");
            assert_eq!(args.len(), 2, "expected 2 positional args, got {:?}", args);
            assert!(
                matches!(args[0].kind, ExprKind::NumberLiteral { value: v, .. } if (v - 1.0).abs() < 1e-10),
                "expected args[0] = NumberLiteral(1.0)"
            );
            assert!(
                matches!(args[1].kind, ExprKind::NumberLiteral { value: v, .. } if (v - 2.0).abs() < 1e-10),
                "expected args[1] = NumberLiteral(2.0)"
            );
        }
        other => panic!("expected FunctionCall, got {:?}", other),
    }
}

#[test]
fn function_call_nested_named_args_acceptance_shape() {
    // Host(m: Steel(density: 1000.0)) — the acceptance-criterion example.
    // Outer: FunctionCall { name: "Host", args: [inner] }
    // Inner: FunctionCall { name: "Steel", args: [1000.0] }
    let expr = first_let_value(r#"structure S { let x = Host(m: Steel(density: 1000.0)) }"#);
    let (outer_name, outer_args) = match expr.kind {
        ExprKind::FunctionCall { name, args } => (name, args),
        other => panic!("expected outer FunctionCall, got {:?}", other),
    };
    assert_eq!(outer_name, "Host");
    assert_eq!(
        outer_args.len(),
        1,
        "expected 1 arg in Host(...), got {:?}",
        outer_args
    );

    let (inner_name, inner_args) = match &outer_args[0].kind {
        ExprKind::FunctionCall { name, args } => (name.clone(), args.clone()),
        other => panic!("expected inner FunctionCall (Steel), got {:?}", other),
    };
    assert_eq!(inner_name, "Steel");
    assert_eq!(
        inner_args.len(),
        1,
        "expected 1 arg in Steel(...), got {:?}",
        inner_args
    );
    assert!(
        matches!(inner_args[0].kind, ExprKind::NumberLiteral { value: v, .. } if (v - 1000.0).abs() < 1e-10),
        "expected NumberLiteral(1000.0), got {:?}",
        inner_args[0].kind
    );
}

#[test]
fn function_call_mixed_positional_and_named_args() {
    // foo(1.0, b: 2.0) — mixed positional + named, args: [1.0, 2.0]
    let expr = first_let_value(r#"structure S { let x = foo(1.0, b: 2.0) }"#);
    match expr.kind {
        ExprKind::FunctionCall { name, args } => {
            assert_eq!(name, "foo");
            assert_eq!(args.len(), 2, "expected 2 positional args, got {:?}", args);
            assert!(
                matches!(args[0].kind, ExprKind::NumberLiteral { value: v, .. } if (v - 1.0).abs() < 1e-10),
                "expected args[0] = NumberLiteral(1.0)"
            );
            assert!(
                matches!(args[1].kind, ExprKind::NumberLiteral { value: v, .. } if (v - 2.0).abs() < 1e-10),
                "expected args[1] = NumberLiteral(2.0)"
            );
        }
        other => panic!("expected FunctionCall, got {:?}", other),
    }
}
