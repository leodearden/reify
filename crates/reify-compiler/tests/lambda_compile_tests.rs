//! Lambda compilation tests.

use reify_types::ValueCellId;

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

/// step-9: Compile lambda with capture — `let factor = 3; |x| x * factor`
/// Verify that the lambda's captures vec contains the ValueCellId for 'factor'.
#[test]
fn compile_lambda_with_capture() {
    let source = r#"
structure S {
    let factor = 3
    let f = |x| x * factor
}
"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_capture"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let template = &compiled.templates[0];
    let f_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "f")
        .expect("should have 'f' value cell");

    let f_expr = f_cell.default_expr.as_ref().expect("let should have expr");
    match &f_expr.kind {
        reify_types::CompiledExprKind::Lambda { captures, .. } => {
            let factor_id = ValueCellId::new("S", "factor");
            assert!(
                captures.contains(&factor_id),
                "captures should contain 'factor', got: {:?}",
                captures
            );
        }
        other => panic!("expected Lambda, got {:?}", other),
    }
}

/// step-11: Compile lambda with typed param `|x: Real| x + 1.0` — verify param
/// type is recorded as Some(Type::Real) in the compiled output.
#[test]
fn compile_lambda_typed_param() {
    let source = r#"
structure S {
    let f = |x: Real| x + 1.0
}
"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_typed"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let template = &compiled.templates[0];
    let f_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "f")
        .expect("should have 'f' value cell");

    let f_expr = f_cell.default_expr.as_ref().expect("let should have expr");
    match &f_expr.kind {
        reify_types::CompiledExprKind::Lambda { params, .. } => {
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].0, "x");
            assert_eq!(
                params[0].1,
                Some(reify_types::Type::Real),
                "param type should be Real"
            );
        }
        other => panic!("expected Lambda, got {:?}", other),
    }

    // Also verify the result_type is a Function type
    match &f_expr.result_type {
        reify_types::Type::Function {
            params,
            return_type,
        } => {
            assert_eq!(params, &[reify_types::Type::Real]);
            assert_eq!(**return_type, reify_types::Type::Real);
        }
        other => panic!("expected Function type, got {:?}", other),
    }
}
