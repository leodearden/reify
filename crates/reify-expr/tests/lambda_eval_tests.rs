//! Lambda evaluation tests.

use reify_expr::eval_expr;
use reify_types::{BinOp, CompiledExpr, Type, Value, ValueCellId, ValueMap};

/// step-13: Evaluate a lambda expression `|x| x * 2` — verify it produces
/// Value::Lambda with the correct params and empty captures.
#[test]
fn eval_lambda_simple_no_captures() {
    // Build: |x| x * 2
    // Lambda params: [("x", None)]
    // Body: BinOp(Mul, ValueRef($lambda.x), Literal(2))
    // Captures: []
    let x_id = ValueCellId::new("$lambda", "x");
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Real,
    );
    let lambda_expr = CompiledExpr::lambda(
        vec![("x".to_string(), None)],
        body,
        vec![], // no captures
        Type::Function {
            params: vec![Type::Real],
            return_type: Box::new(Type::Real),
        },
    );

    let values = ValueMap::new();
    let result = eval_expr(&lambda_expr, &values);

    match &result {
        Value::Lambda {
            params,
            body: _,
            captures,
        } => {
            assert_eq!(params, &["x".to_string()]);
            assert!(captures.is_empty(), "no captures expected");
        }
        other => panic!("expected Value::Lambda, got {:?}", other),
    }
}

/// step-15: Evaluate a lambda with captures — `factor=3` in ValueMap,
/// eval `|x| x * factor`. Verify the resulting Value::Lambda captures the
/// factor value from the ValueMap.
#[test]
fn eval_lambda_with_captures() {
    // Build: |x| x * factor
    // factor is captured from the outer scope
    let x_id = ValueCellId::new("$lambda", "x");
    let factor_id = ValueCellId::new("S", "factor");

    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(factor_id.clone(), Type::Int),
        Type::Real,
    );
    let lambda_expr = CompiledExpr::lambda(
        vec![("x".to_string(), None)],
        body,
        vec![factor_id.clone()], // captures factor
        Type::Function {
            params: vec![Type::Real],
            return_type: Box::new(Type::Real),
        },
    );

    // Set up outer scope with factor = 3
    let mut values = ValueMap::new();
    values.insert(factor_id.clone(), Value::Int(3));

    let result = eval_expr(&lambda_expr, &values);

    match &result {
        Value::Lambda {
            params,
            body: _,
            captures,
        } => {
            assert_eq!(params, &["x".to_string()]);
            assert_eq!(captures.len(), 1, "should capture factor");
            assert_eq!(
                captures.get(&factor_id),
                Some(&Value::Int(3)),
                "captured factor should be Int(3)"
            );
        }
        other => panic!("expected Value::Lambda, got {:?}", other),
    }
}

/// step-17: Evaluate a lambda with Undef capture — one captured variable is
/// Undef. Verify the lambda is still created but the capture contains Undef.
#[test]
fn eval_lambda_with_undef_capture() {
    let x_id = ValueCellId::new("$lambda", "x");
    let missing_id = ValueCellId::new("S", "missing");

    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(missing_id.clone(), Type::Real),
        Type::Real,
    );
    let lambda_expr = CompiledExpr::lambda(
        vec![("x".to_string(), None)],
        body,
        vec![missing_id.clone()], // captures a variable not in ValueMap
        Type::Function {
            params: vec![Type::Real],
            return_type: Box::new(Type::Real),
        },
    );

    // ValueMap does NOT contain 'missing' — so capture should be Undef
    let values = ValueMap::new();
    let result = eval_expr(&lambda_expr, &values);

    match &result {
        Value::Lambda {
            params,
            body: _,
            captures,
        } => {
            assert_eq!(params, &["x".to_string()]);
            assert_eq!(captures.len(), 1);
            assert_eq!(
                captures.get(&missing_id),
                Some(&Value::Undef),
                "missing captured variable should be Undef"
            );
        }
        other => panic!("expected Value::Lambda, got {:?}", other),
    }
}

/// step-19: Apply a Value::Lambda — `(|x| x * 2)` applied to `[Int(5)]`
/// should return `Int(10)`.
#[test]
fn apply_lambda_simple() {
    use reify_expr::apply_lambda;

    // Build a Value::Lambda for |x| x * 2
    let x_id = ValueCellId::new("$lambda", "x");
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Real,
    );

    let lambda = Value::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(body),
        captures: ValueMap::new(),
    };

    let result = apply_lambda(&lambda, &[Value::Int(5)]);
    assert_eq!(result, Value::Int(10));
}

/// step-21: Apply a lambda with captures — `factor=3`, lambda `|x| x * factor`,
/// apply to `[Int(5)]` returns `Int(15)`.
#[test]
fn apply_lambda_with_captures() {
    use reify_expr::apply_lambda;

    let x_id = ValueCellId::new("$lambda", "x");
    let factor_id = ValueCellId::new("S", "factor");

    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(factor_id.clone(), Type::Int),
        Type::Real,
    );

    let mut captures = ValueMap::new();
    captures.insert(factor_id.clone(), Value::Int(3));

    let lambda = Value::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(body),
        captures,
    };

    let result = apply_lambda(&lambda, &[Value::Int(5)]);
    assert_eq!(result, Value::Int(15));
}

/// step-23: Apply a lambda with wrong arity (2-param lambda applied with 1 arg)
/// returns Undef. Also test 0-param lambda application.
#[test]
fn apply_lambda_arity_mismatch_returns_undef() {
    use reify_expr::apply_lambda;

    let x_id = ValueCellId::new("$lambda", "x");
    let y_id = ValueCellId::new("$lambda", "y");

    // 2-param lambda: |x, y| x + y
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::value_ref(y_id.clone(), Type::Real),
        Type::Real,
    );

    let lambda = Value::Lambda {
        params: vec!["x".to_string(), "y".to_string()],
        body: Box::new(body),
        captures: ValueMap::new(),
    };

    // Wrong arity: 1 arg for 2-param lambda
    let result = apply_lambda(&lambda, &[Value::Int(5)]);
    assert!(result.is_undef(), "arity mismatch should return Undef");

    // Wrong arity: 3 args for 2-param lambda
    let result = apply_lambda(&lambda, &[Value::Int(1), Value::Int(2), Value::Int(3)]);
    assert!(result.is_undef(), "too many args should return Undef");
}

#[test]
fn apply_lambda_zero_params() {
    use reify_expr::apply_lambda;

    // 0-param lambda: || true
    let body = CompiledExpr::literal(Value::Bool(true), Type::Bool);
    let lambda = Value::Lambda {
        params: vec![],
        body: Box::new(body),
        captures: ValueMap::new(),
    };

    // Apply with 0 args
    let result = apply_lambda(&lambda, &[]);
    assert_eq!(result, Value::Bool(true));

    // Apply with args to 0-param lambda — arity mismatch
    let result = apply_lambda(&lambda, &[Value::Int(1)]);
    assert!(result.is_undef(), "0-param lambda with args should return Undef");
}

/// step-25: Value::Lambda content_hash is deterministic and distinct from other variants.
/// Two identical lambdas produce the same hash; different lambdas produce different hashes.
#[test]
fn lambda_content_hash_deterministic_and_distinct() {
    let body1 = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(ValueCellId::new("$lambda", "x"), Type::Real),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Real,
    );
    let body2 = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(ValueCellId::new("$lambda", "x"), Type::Real),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Real,
    );
    let body3 = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(ValueCellId::new("$lambda", "x"), Type::Real),
        CompiledExpr::literal(Value::Int(1), Type::Int),
        Type::Real,
    );

    let lambda1 = Value::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(body1),
        captures: ValueMap::new(),
    };
    let lambda2 = Value::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(body2),
        captures: ValueMap::new(),
    };
    let lambda3 = Value::Lambda {
        params: vec!["x".to_string()],
        body: Box::new(body3),
        captures: ValueMap::new(),
    };

    // Same lambdas produce same hash
    assert_eq!(
        lambda1.content_hash(),
        lambda2.content_hash(),
        "identical lambdas should have same hash"
    );

    // Different lambdas produce different hash
    assert_ne!(
        lambda1.content_hash(),
        lambda3.content_hash(),
        "different lambdas should have different hash"
    );

    // Lambda hash differs from other Value variants
    assert_ne!(lambda1.content_hash(), Value::Undef.content_hash());
    assert_ne!(lambda1.content_hash(), Value::Int(0).content_hash());
    assert_ne!(lambda1.content_hash(), Value::Bool(false).content_hash());

    // Different param names produce different hash
    let lambda_y = Value::Lambda {
        params: vec!["y".to_string()],
        body: Box::new(CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::value_ref(ValueCellId::new("$lambda", "x"), Type::Real),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            Type::Real,
        )),
        captures: ValueMap::new(),
    };
    assert_ne!(
        lambda1.content_hash(),
        lambda_y.content_hash(),
        "different param names should produce different hash"
    );
}

/// step-27: Integration test — full pipeline parse → compile → eval for a structure
/// with a lambda that captures a value from the same structure.
#[test]
fn integration_parse_compile_eval_lambda() {
    use reify_expr::apply_lambda;

    let source = r#"
structure S {
    let factor: Real = 3.0
    let f = |x| x * factor
}
"#;
    // Parse
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_integration"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let template = &compiled.templates[0];

    // Find value cells
    let factor_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "factor")
        .expect("should have 'factor'");
    let f_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "f")
        .expect("should have 'f'");

    // Set up ValueMap with factor = 3.0
    let mut values = ValueMap::new();
    let factor_expr = factor_cell
        .default_expr
        .as_ref()
        .expect("factor should have expr");
    let factor_val = eval_expr(factor_expr, &values);
    values.insert(factor_cell.id.clone(), factor_val);

    // Evaluate the lambda expression
    let f_expr = f_cell.default_expr.as_ref().expect("f should have expr");
    let f_val = eval_expr(f_expr, &values);

    // Verify we got a lambda
    match &f_val {
        Value::Lambda { params, .. } => {
            assert_eq!(params, &["x".to_string()]);
        }
        other => panic!("expected Value::Lambda, got {:?}", other),
    }

    // Apply the lambda: f(5.0) should return 15.0
    let result = apply_lambda(&f_val, &[Value::Real(5.0)]);
    match result {
        Value::Real(v) => assert!(
            (v - 15.0).abs() < 1e-12,
            "expected 15.0, got {}",
            v
        ),
        other => panic!("expected Real(15.0), got {:?}", other),
    }
}
