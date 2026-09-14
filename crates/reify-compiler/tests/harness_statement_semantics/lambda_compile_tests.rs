//! Lambda compilation tests.

use reify_core::ValueCellId;

/// step-7: Compile `|x| x * 2` in a let binding — produces CompiledExprKind::Lambda
/// with 1 param, no captures, and correct body.
#[test]
fn compile_lambda_single_param_no_captures() {
    let source = r#"
structure S {
    let f = |x| x * 2
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_lambda"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
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
        reify_ir::CompiledExprKind::Lambda {
            params,
            param_ids,
            body,
            captures,
        } => {
            assert_eq!(params.len(), 1, "expected 1 param");
            assert_eq!(params[0].0, "x");
            assert!(params[0].1.is_none(), "untyped param");
            assert_eq!(param_ids.len(), 1);
            assert!(
                param_ids[0].entity.starts_with("$lambda"),
                "param entity should start with $lambda"
            );
            assert_eq!(param_ids[0].member, "x");
            assert!(captures.is_empty(), "no captures for simple lambda");
            match &body.kind {
                reify_ir::CompiledExprKind::BinOp { op, .. } => {
                    assert_eq!(*op, reify_ir::BinOp::Mul);
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
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_capture"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
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
        reify_ir::CompiledExprKind::Lambda { captures, .. } => {
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
/// type is recorded as Some(Type::dimensionless_scalar()) in the compiled output.
#[test]
fn compile_lambda_typed_param() {
    let source = r#"
structure S {
    let f = |x: Real| x + 1.0
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_typed"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
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
        reify_ir::CompiledExprKind::Lambda { params, .. } => {
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].0, "x");
            assert_eq!(
                params[0].1,
                Some(reify_core::Type::dimensionless_scalar()),
                "param type should be Real"
            );
        }
        other => panic!("expected Lambda, got {:?}", other),
    }

    match &f_expr.result_type {
        reify_core::Type::Function {
            params,
            return_type,
        } => {
            assert_eq!(params, &[reify_core::Type::dimensionless_scalar()]);
            assert_eq!(**return_type, reify_core::Type::dimensionless_scalar());
        }
        other => panic!("expected Function type, got {:?}", other),
    }
}

/// Param shadowing: compile `let x = 5; let f = |x| x * 2`, assert captures
/// does NOT contain outer x.
#[test]
fn compile_lambda_param_shadows_outer() {
    let source = r#"
structure S {
    param x: Real = 5
    let f = |x| x * 2
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_shadow"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let template = &compiled.templates[0];
    let f_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "f")
        .expect("should have 'f'");

    let f_expr = f_cell.default_expr.as_ref().expect("let should have expr");
    match &f_expr.kind {
        reify_ir::CompiledExprKind::Lambda { captures, .. } => {
            assert!(
                captures.is_empty(),
                "lambda param 'x' should shadow outer 'x', so no captures. Got: {:?}",
                captures
            );
        }
        other => panic!("expected Lambda, got {:?}", other),
    }
}

/// Verify param ID entity starts with `$lambda`.
#[test]
fn compile_lambda_synthetic_entity_name() {
    let source = r#"
structure MyStruct {
    let f = |a, b| a + b
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_synth"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    let f_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "f")
        .expect("should have 'f'");

    let f_expr = f_cell.default_expr.as_ref().expect("let should have expr");
    match &f_expr.kind {
        reify_ir::CompiledExprKind::Lambda { param_ids, .. } => {
            assert_eq!(param_ids.len(), 2);
            for id in param_ids {
                assert!(
                    id.entity.starts_with("$lambda"),
                    "param entity should start with $lambda, got: {}",
                    id.entity
                );
            }
            assert_eq!(param_ids[0].member, "a");
            assert_eq!(param_ids[1].member, "b");
        }
        other => panic!("expected Lambda, got {:?}", other),
    }
}
