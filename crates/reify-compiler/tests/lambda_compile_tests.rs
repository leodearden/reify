//! Lambda compilation tests.

/// step-7: Compile `|x| x * 2` in a let binding — produces CompiledExprKind::Lambda
/// with 1 param, no captures, and correct body.
#[test]
fn compile_lambda_single_param_no_captures() {
    let source = r#"
structure S {
    let f = |x| x * 2
}
"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_lambda"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    // No error diagnostics expected
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    let template = &compiled.templates[0];
    let f_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "f")
        .expect("should have 'f' value cell");

    let f_expr = f_cell.default_expr.as_ref().expect("let should have expr");
    match &f_expr.kind {
        reify_types::CompiledExprKind::Lambda {
            params,
            body,
            captures,
        } => {
            assert_eq!(params.len(), 1, "expected 1 param");
            assert_eq!(params[0].0, "x");
            assert!(params[0].1.is_none(), "untyped param");
            assert!(captures.is_empty(), "no captures for simple lambda");
            // Body should be a BinOp(Mul)
            match &body.kind {
                reify_types::CompiledExprKind::BinOp { op, .. } => {
                    assert_eq!(*op, reify_types::BinOp::Mul);
                }
                other => panic!("expected BinOp(Mul), got {:?}", other),
            }
        }
        other => panic!("expected Lambda, got {:?}", other),
    }
}
