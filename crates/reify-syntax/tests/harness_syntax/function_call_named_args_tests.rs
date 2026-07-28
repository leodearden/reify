//! Integration tests for function_call lowering with named arguments.
//!
//! Verifies that named arguments in function calls are PRESERVED as
//! `arg_names: Vec<Option<String>>` in `ExprKind::FunctionCall`, length-matched
//! to `args`.  `None` entries encode positional (unlabelled) arguments.

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
fn function_call_single_named_arg_preserves_name() {
    // foo(a: 1.0) -> arg_names == [Some("a")]
    let expr = first_let_value(r#"structure S { let x = foo(a: 1.0) }"#);
    match expr.kind {
        ExprKind::FunctionCall { name, args, arg_names } => {
            assert_eq!(name, "foo");
            assert_eq!(args.len(), 1, "expected 1 arg, got {:?}", args);
            assert_eq!(
                arg_names,
                vec![Some("a".to_string())],
                "expected arg_names == [Some(\"a\")]"
            );
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
fn function_call_multiple_named_args_preserves_names() {
    // foo(a: 1.0, b: 2.0) -> arg_names == [Some("a"), Some("b")]
    let expr = first_let_value(r#"structure S { let x = foo(a: 1.0, b: 2.0) }"#);
    match expr.kind {
        ExprKind::FunctionCall { name, args, arg_names } => {
            assert_eq!(name, "foo");
            assert_eq!(args.len(), 2, "expected 2 args, got {:?}", args);
            assert_eq!(
                arg_names,
                vec![Some("a".to_string()), Some("b".to_string())],
                "expected arg_names == [Some(\"a\"), Some(\"b\")]"
            );
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
fn function_call_all_positional_args_all_none() {
    // foo(1.0, 2.0) -> arg_names == [None, None]
    let expr = first_let_value(r#"structure S { let x = foo(1.0, 2.0) }"#);
    match expr.kind {
        ExprKind::FunctionCall { name, args, arg_names } => {
            assert_eq!(name, "foo");
            assert_eq!(args.len(), 2, "expected 2 args, got {:?}", args);
            assert_eq!(
                arg_names,
                vec![None, None],
                "expected arg_names == [None, None]"
            );
        }
        other => panic!("expected FunctionCall, got {:?}", other),
    }
}

#[test]
fn function_call_mixed_positional_and_named_args() {
    // foo(1.0, b: 2.0) -> arg_names == [None, Some("b")]
    let expr = first_let_value(r#"structure S { let x = foo(1.0, b: 2.0) }"#);
    match expr.kind {
        ExprKind::FunctionCall { name, args, arg_names } => {
            assert_eq!(name, "foo");
            assert_eq!(args.len(), 2, "expected 2 args, got {:?}", args);
            assert_eq!(
                arg_names,
                vec![None, Some("b".to_string())],
                "expected arg_names == [None, Some(\"b\")]"
            );
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
    // Host(m: Steel(density: 1000.0))
    // Outer: arg_names == [Some("m")]
    // Inner: arg_names == [Some("density")]
    let expr = first_let_value(r#"structure S { let x = Host(m: Steel(density: 1000.0)) }"#);
    let (outer_name, outer_args, outer_arg_names) = match expr.kind {
        ExprKind::FunctionCall { name, args, arg_names } => (name, args, arg_names),
        other => panic!("expected outer FunctionCall, got {:?}", other),
    };
    assert_eq!(outer_name, "Host");
    assert_eq!(outer_args.len(), 1, "expected 1 arg in Host(...), got {:?}", outer_args);
    assert_eq!(
        outer_arg_names,
        vec![Some("m".to_string())],
        "expected outer arg_names == [Some(\"m\")]"
    );

    let (inner_name, inner_args, inner_arg_names) = match &outer_args[0].kind {
        ExprKind::FunctionCall { name, args, arg_names } => {
            (name.clone(), args.clone(), arg_names.clone())
        }
        other => panic!("expected inner FunctionCall (Steel), got {:?}", other),
    };
    assert_eq!(inner_name, "Steel");
    assert_eq!(inner_args.len(), 1, "expected 1 arg in Steel(...), got {:?}", inner_args);
    assert_eq!(
        inner_arg_names,
        vec![Some("density".to_string())],
        "expected inner arg_names == [Some(\"density\")]"
    );
    assert!(
        matches!(inner_args[0].kind, ExprKind::NumberLiteral { value: v, .. } if (v - 1000.0).abs() < 1e-10),
        "expected NumberLiteral(1000.0), got {:?}",
        inner_args[0].kind
    );
}
