//! Lambda evaluation tests.

use std::sync::Arc;

use reify_expr::{EvalContext, eval_expr};
use reify_core::{ContentHash, Type, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, FieldSourceKind, ResolvedFunction, Value, ValueMap};

/// Helper to build a Value::Lambda with (name, id) param pairs.
fn make_value_lambda(
    params: Vec<(&str, ValueCellId)>,
    body: CompiledExpr,
    captures: ValueMap,
) -> Value {
    Value::Lambda {
        params: params
            .into_iter()
            .map(|(n, id)| (n.to_string(), id))
            .collect(),
        body: Box::new(body),
        captures,
    }
}

/// step-13: Evaluate a lambda expression `|x| x * 2` — verify it produces
/// Value::Lambda with the correct params and empty captures.
#[test]
fn eval_lambda_simple_no_captures() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::dimensionless_scalar()),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::dimensionless_scalar(),
    );
    let lambda_expr = CompiledExpr::lambda(
        vec![("x".to_string(), None)],
        vec![x_id.clone()],
        body,
        vec![],
        Type::Function {
            params: vec![Type::dimensionless_scalar()],
            return_type: Box::new(Type::dimensionless_scalar()),
        },
    );

    let values = ValueMap::new();
    let result = eval_expr(&lambda_expr, &EvalContext::simple(&values));

    match &result {
        Value::Lambda {
            params,
            body: _,
            captures,
        } => {
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].0, "x");
            assert_eq!(params[0].1, x_id);
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
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let factor_id = ValueCellId::new("S", "factor");

    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::dimensionless_scalar()),
        CompiledExpr::value_ref(factor_id.clone(), Type::Int),
        Type::dimensionless_scalar(),
    );
    let lambda_expr = CompiledExpr::lambda(
        vec![("x".to_string(), None)],
        vec![x_id.clone()],
        body,
        vec![factor_id.clone()],
        Type::Function {
            params: vec![Type::dimensionless_scalar()],
            return_type: Box::new(Type::dimensionless_scalar()),
        },
    );

    let mut values = ValueMap::new();
    values.insert(factor_id.clone(), Value::Int(3));

    let result = eval_expr(&lambda_expr, &EvalContext::simple(&values));

    match &result {
        Value::Lambda {
            params,
            body: _,
            captures,
        } => {
            assert_eq!(params[0].0, "x");
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
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let missing_id = ValueCellId::new("S", "missing");

    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::dimensionless_scalar()),
        CompiledExpr::value_ref(missing_id.clone(), Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );
    let lambda_expr = CompiledExpr::lambda(
        vec![("x".to_string(), None)],
        vec![x_id.clone()],
        body,
        vec![missing_id.clone()],
        Type::Function {
            params: vec![Type::dimensionless_scalar()],
            return_type: Box::new(Type::dimensionless_scalar()),
        },
    );

    let values = ValueMap::new();
    let result = eval_expr(&lambda_expr, &EvalContext::simple(&values));

    match &result {
        Value::Lambda {
            params,
            body: _,
            captures,
        } => {
            assert_eq!(params[0].0, "x");
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

    let x_id = ValueCellId::new("$lambda0.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::dimensionless_scalar()),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::dimensionless_scalar(),
    );

    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let empty = ValueMap::new();
    let result = apply_lambda(&lambda, &[Value::Int(5)], &EvalContext::simple(&empty));
    assert_eq!(result, Value::Int(10));
}

/// step-21: Apply a lambda with captures — `factor=3`, lambda `|x| x * factor`,
/// apply to `[Int(5)]` returns `Int(15)`.
#[test]
fn apply_lambda_with_captures() {
    use reify_expr::apply_lambda;

    let x_id = ValueCellId::new("$lambda0.S", "x");
    let factor_id = ValueCellId::new("S", "factor");

    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::dimensionless_scalar()),
        CompiledExpr::value_ref(factor_id.clone(), Type::Int),
        Type::dimensionless_scalar(),
    );

    let mut captures = ValueMap::new();
    captures.insert(factor_id.clone(), Value::Int(3));

    let lambda = make_value_lambda(vec![("x", x_id)], body, captures);

    let empty = ValueMap::new();
    let result = apply_lambda(&lambda, &[Value::Int(5)], &EvalContext::simple(&empty));
    assert_eq!(result, Value::Int(15));
}

/// step-23: Apply a lambda with wrong arity (2-param lambda applied with 1 arg)
/// returns Undef. Also test 0-param lambda application.
#[test]
fn apply_lambda_arity_mismatch_returns_undef() {
    use reify_expr::apply_lambda;

    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda0.S", "y");

    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(x_id.clone(), Type::dimensionless_scalar()),
        CompiledExpr::value_ref(y_id.clone(), Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );

    let lambda = make_value_lambda(vec![("x", x_id), ("y", y_id)], body, ValueMap::new());

    let empty = ValueMap::new();
    let result = apply_lambda(&lambda, &[Value::Int(5)], &EvalContext::simple(&empty));
    assert!(result.is_undef(), "arity mismatch should return Undef");

    let result = apply_lambda(
        &lambda,
        &[Value::Int(1), Value::Int(2), Value::Int(3)],
        &EvalContext::simple(&empty),
    );
    assert!(result.is_undef(), "too many args should return Undef");
}

#[test]
fn apply_lambda_zero_params() {
    use reify_expr::apply_lambda;

    let body = CompiledExpr::literal(Value::Bool(true), Type::Bool);
    let lambda = make_value_lambda(vec![], body, ValueMap::new());

    let empty = ValueMap::new();
    let result = apply_lambda(&lambda, &[], &EvalContext::simple(&empty));
    assert_eq!(result, Value::Bool(true));

    let result = apply_lambda(&lambda, &[Value::Int(1)], &EvalContext::simple(&empty));
    assert!(
        result.is_undef(),
        "0-param lambda with args should return Undef"
    );
}

/// step-25: Value::Lambda content_hash is deterministic and distinct from other variants.
#[test]
fn lambda_content_hash_deterministic_and_distinct() {
    let x_id = ValueCellId::new("$lambda0.S", "x");

    let body1 = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::dimensionless_scalar()),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::dimensionless_scalar(),
    );
    let body2 = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::dimensionless_scalar()),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::dimensionless_scalar(),
    );
    let body3 = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(x_id.clone(), Type::dimensionless_scalar()),
        CompiledExpr::literal(Value::Int(1), Type::Int),
        Type::dimensionless_scalar(),
    );

    let lambda1 = make_value_lambda(vec![("x", x_id.clone())], body1, ValueMap::new());
    let lambda2 = make_value_lambda(vec![("x", x_id.clone())], body2, ValueMap::new());
    let lambda3 = make_value_lambda(vec![("x", x_id.clone())], body3, ValueMap::new());

    assert_eq!(
        lambda1.content_hash(),
        lambda2.content_hash(),
        "identical lambdas should have same hash"
    );
    assert_ne!(
        lambda1.content_hash(),
        lambda3.content_hash(),
        "different lambdas should have different hash"
    );
    assert_ne!(lambda1.content_hash(), Value::Undef.content_hash());
    assert_ne!(lambda1.content_hash(), Value::Int(0).content_hash());
    assert_ne!(lambda1.content_hash(), Value::Bool(false).content_hash());

    // Different param names produce different hash
    let y_id = ValueCellId::new("$lambda0.S", "y");
    let lambda_y = make_value_lambda(
        vec![("y", y_id)],
        CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::value_ref(x_id.clone(), Type::dimensionless_scalar()),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            Type::dimensionless_scalar(),
        ),
        ValueMap::new(),
    );
    assert_ne!(
        lambda1.content_hash(),
        lambda_y.content_hash(),
        "different param names should produce different hash"
    );
}

/// step-29: Two Value::Lambda instances with identical params and body but with
/// captures inserted in different orders should have equal content_hash.
#[test]
fn lambda_content_hash_invariant_capture_insertion_order() {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let cap_a_id = ValueCellId::new("S", "a_var");
    let cap_b_id = ValueCellId::new("S", "b_var");

    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(x_id.clone(), Type::dimensionless_scalar()),
        CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(cap_a_id.clone(), Type::Int),
            CompiledExpr::value_ref(cap_b_id.clone(), Type::Int),
            Type::Int,
        ),
        Type::dimensionless_scalar(),
    );

    let mut captures_a = ValueMap::new();
    captures_a.insert(cap_a_id.clone(), Value::Int(10));
    captures_a.insert(cap_b_id.clone(), Value::Int(20));

    let lambda_a = make_value_lambda(vec![("x", x_id.clone())], body.clone(), captures_a);

    let mut captures_b = ValueMap::new();
    captures_b.insert(cap_b_id.clone(), Value::Int(20));
    captures_b.insert(cap_a_id.clone(), Value::Int(10));

    let lambda_b = make_value_lambda(vec![("x", x_id)], body, captures_b);

    assert_eq!(
        lambda_a, lambda_b,
        "lambdas with same captures in different insertion order should be equal"
    );
    assert_eq!(
        lambda_a.content_hash(),
        lambda_b.content_hash(),
        "content_hash invariant violated: equal lambdas must have equal hashes"
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
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_integration"));
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
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let template = &compiled.templates[0];

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

    let mut values = ValueMap::new();
    let factor_expr = factor_cell
        .default_expr
        .as_ref()
        .expect("factor should have expr");
    let factor_val = eval_expr(factor_expr, &EvalContext::simple(&values));
    values.insert(factor_cell.id.clone(), factor_val);

    let f_expr = f_cell.default_expr.as_ref().expect("f should have expr");
    let f_val = eval_expr(f_expr, &EvalContext::simple(&values));

    match &f_val {
        Value::Lambda { params, .. } => {
            assert_eq!(params[0].0, "x");
        }
        other => panic!("expected Value::Lambda, got {:?}", other),
    }

    let empty = ValueMap::new();
    let result = apply_lambda(&f_val, &[Value::Real(5.0)], &EvalContext::simple(&empty));
    match result {
        Value::Real(v) => assert!((v - 15.0).abs() < 1e-12, "expected 15.0, got {}", v),
        other => panic!("expected Real(15.0), got {:?}", other),
    }
}

// --- Phase 5: New tests ---

// ─── step-1: apply_lambda with populated function registry ───

/// step-1: apply_lambda with a populated function registry — lambda body
/// is `UserFunctionCall(double, [x])`. With EvalContext::new containing
/// the double function, apply_lambda(&lambda, &[Int(5)], &ctx) returns Int(10).
#[test]
fn apply_lambda_with_user_function_registry() {
    use reify_expr::apply_lambda;
    use reify_core::ContentHash;
    use reify_ir::{CompiledExprKind, CompiledFnBody, CompiledFunction};

    // Define user function: double(x) = x * 2
    let params = vec![("x".to_string(), Type::Int)];
    let double_fn = CompiledFunction {
        name: "double".to_string(),
        doc: None,
        is_pub: false,
        param_defaults: CompiledFunction::no_defaults_for(&params),
        params,
        return_type: Type::Int,
        body: CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::value_ref(ValueCellId::new("double", "x"), Type::Int),
                CompiledExpr::literal(Value::Int(2), Type::Int),
                Type::Int,
            ),
        },
        content_hash: ContentHash::of(b"double_fn_step1"),
        annotations: vec![],
        optimized_target: None,
        type_params: vec![],
    };

    // Lambda body: double(x) via UserFunctionCall
    let x_id = ValueCellId::new("$lambda_uf.S", "x");
    let lambda_body = CompiledExpr {
        kind: CompiledExprKind::UserFunctionCall {
            function_name: "double".to_string(),
            args: vec![CompiledExpr::value_ref(x_id.clone(), Type::Int)],
        },
        result_type: Type::Int,
        content_hash: ContentHash::of(b"double_call_step1"),
    };

    let lambda = make_value_lambda(vec![("x", x_id)], lambda_body, ValueMap::new());

    let values = ValueMap::new();
    let functions = vec![double_fn];
    let ctx = EvalContext::new(&values, &functions);
    let result = apply_lambda(&lambda, &[Value::Int(5)], &ctx);
    assert_eq!(
        result,
        Value::Int(10),
        "apply_lambda should use the function registry from the passed EvalContext"
    );
}

// ─── step-9: apply_lambda with empty registry — UserFunctionCall returns Undef ───

/// step-9: apply_lambda where lambda body calls unknown_fn(x) via UserFunctionCall,
/// but EvalContext::simple (empty registry) is passed. Should return Undef.
#[test]
fn apply_lambda_user_fn_not_in_registry_returns_undef() {
    use reify_expr::apply_lambda;
    use reify_core::ContentHash;
    use reify_ir::CompiledExprKind;

    // Lambda body: unknown_fn(x) via UserFunctionCall — not in registry
    let x_id = ValueCellId::new("$lambda_uf.S", "x");
    let lambda_body = CompiledExpr {
        kind: CompiledExprKind::UserFunctionCall {
            function_name: "unknown_fn".to_string(),
            args: vec![CompiledExpr::value_ref(x_id.clone(), Type::Int)],
        },
        result_type: Type::Int,
        content_hash: ContentHash::of(b"unknown_call_step9"),
    };

    let lambda = make_value_lambda(vec![("x", x_id)], lambda_body, ValueMap::new());

    let empty = ValueMap::new();
    let result = apply_lambda(&lambda, &[Value::Int(5)], &EvalContext::simple(&empty));
    assert!(
        result.is_undef(),
        "calling an unknown user function should return Undef, got {:?}",
        result
    );
}

// ─── step-11: nested lambda calls user function ───

/// step-11: nested lambda calls user function — `|x| |y| double(x) + y`.
/// Apply outer with 3, apply inner with 4, expect 10 (double(3)=6, 6+4=10).
#[test]
fn nested_lambda_calls_user_function() {
    use reify_expr::apply_lambda;
    use reify_core::ContentHash;
    use reify_ir::{CompiledExprKind, CompiledFnBody, CompiledFunction};

    // Define user function: double(x) = x * 2
    let params = vec![("x".to_string(), Type::Int)];
    let double_fn = CompiledFunction {
        name: "double".to_string(),
        doc: None,
        is_pub: false,
        param_defaults: CompiledFunction::no_defaults_for(&params),
        params,
        return_type: Type::Int,
        body: CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::binop(
                BinOp::Mul,
                CompiledExpr::value_ref(ValueCellId::new("double", "x"), Type::Int),
                CompiledExpr::literal(Value::Int(2), Type::Int),
                Type::Int,
            ),
        },
        content_hash: ContentHash::of(b"double_fn_step11"),
        annotations: vec![],
        optimized_target: None,
        type_params: vec![],
    };

    let x_id = ValueCellId::new("$lambda_outer.S", "x");
    let y_id = ValueCellId::new("$lambda_inner.S", "y");

    // Inner lambda body: double(x) + y
    let double_x = CompiledExpr {
        kind: CompiledExprKind::UserFunctionCall {
            function_name: "double".to_string(),
            args: vec![CompiledExpr::value_ref(x_id.clone(), Type::Int)],
        },
        result_type: Type::Int,
        content_hash: ContentHash::of(b"double_call_step11"),
    };
    let inner_body = CompiledExpr::binop(
        BinOp::Add,
        double_x,
        CompiledExpr::value_ref(y_id.clone(), Type::Int),
        Type::Int,
    );

    // Inner lambda: |y| double(x) + y (captures x)
    let inner_lambda_expr = CompiledExpr::lambda(
        vec![("y".to_string(), None)],
        vec![y_id.clone()],
        inner_body,
        vec![x_id.clone()], // captures x
        Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::Int),
        },
    );

    // Outer lambda: |x| <inner_lambda>
    let outer_lambda_expr = CompiledExpr::lambda(
        vec![("x".to_string(), None)],
        vec![x_id.clone()],
        inner_lambda_expr,
        vec![],
        Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::Function {
                params: vec![Type::Int],
                return_type: Box::new(Type::Int),
            }),
        },
    );

    let values = ValueMap::new();
    let functions = vec![double_fn];
    let ctx = EvalContext::new(&values, &functions);

    // Eval outer lambda expression
    let outer_val = eval_expr(&outer_lambda_expr, &ctx);

    // Apply outer with x=3 → should yield Value::Lambda with x captured as 3
    let inner_val = apply_lambda(&outer_val, &[Value::Int(3)], &ctx);
    match &inner_val {
        Value::Lambda { captures, .. } => {
            assert_eq!(captures.get(&x_id), Some(&Value::Int(3)));
        }
        other => panic!(
            "expected inner Lambda after outer application, got {:?}",
            other
        ),
    }

    // Apply inner with y=4 → double(3) + 4 = 6 + 4 = 10
    let result = apply_lambda(&inner_val, &[Value::Int(4)], &ctx);
    assert_eq!(
        result,
        Value::Int(10),
        "nested lambda with user function should yield double(3)+4=10"
    );
}

/// Non-Lambda apply returns Undef.
#[test]
fn apply_non_lambda_returns_undef() {
    use reify_expr::apply_lambda;
    let empty = ValueMap::new();
    assert!(apply_lambda(&Value::Int(5), &[], &EvalContext::simple(&empty)).is_undef());
}

/// Nested lambda: eval and apply `|x| |y| x + y`.
#[test]
fn nested_lambda_eval_and_apply() {
    use reify_expr::apply_lambda;

    let x_id = ValueCellId::new("$lambda0.S", "x");
    let y_id = ValueCellId::new("$lambda1.S", "y");

    // Inner: |y| x + y  (x is captured)
    let inner_body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::value_ref(y_id.clone(), Type::Int),
        Type::Int,
    );
    let inner_lambda = CompiledExpr::lambda(
        vec![("y".to_string(), None)],
        vec![y_id.clone()],
        inner_body,
        vec![x_id.clone()], // captures x
        Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::Int),
        },
    );

    // Outer: |x| <inner_lambda>
    let outer_lambda = CompiledExpr::lambda(
        vec![("x".to_string(), None)],
        vec![x_id.clone()],
        inner_lambda,
        vec![], // no outer captures
        Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::Function {
                params: vec![Type::Int],
                return_type: Box::new(Type::Int),
            }),
        },
    );

    let values = ValueMap::new();
    let outer_val = eval_expr(&outer_lambda, &EvalContext::simple(&values));

    // Apply outer with x=3 → should yield Lambda with x captured as 3
    let empty = ValueMap::new();
    let inner_val = apply_lambda(&outer_val, &[Value::Int(3)], &EvalContext::simple(&empty));
    match &inner_val {
        Value::Lambda { captures, .. } => {
            assert_eq!(captures.get(&x_id), Some(&Value::Int(3)));
        }
        other => panic!("expected inner Lambda, got {:?}", other),
    }

    // Apply inner with y=4 → should return 7
    let result = apply_lambda(&inner_val, &[Value::Int(4)], &EvalContext::simple(&empty));
    assert_eq!(result, Value::Int(7));
}

/// apply_lambda must return Undef when recursion depth is at MAX_RECURSION_DEPTH,
/// preventing unbounded recursion through the sample→apply_lambda→eval path.
#[test]
fn apply_lambda_returns_undef_at_max_depth() {
    use reify_expr::apply_lambda;

    let x_id = ValueCellId::new("$lambda0.S", "x");
    // |x| x + 1
    let body = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(1), Type::Int),
        Type::Int,
    );
    let lambda = make_value_lambda(vec![("x", x_id)], body, ValueMap::new());

    let values = ValueMap::new();

    // At MAX depth: should return Undef (depth limit reached)
    let ctx_at_max = EvalContext::_test_at_depth(&values, 256);
    let result = apply_lambda(&lambda, &[Value::Int(5)], &ctx_at_max);
    assert_eq!(
        result,
        Value::Undef,
        "apply_lambda at MAX_RECURSION_DEPTH must return Undef"
    );

    // At MAX-1 depth: should still evaluate normally
    let ctx_below_max = EvalContext::_test_at_depth(&values, 255);
    let result = apply_lambda(&lambda, &[Value::Int(5)], &ctx_below_max);
    assert_eq!(
        result,
        Value::Int(6),
        "apply_lambda below MAX_RECURSION_DEPTH must evaluate"
    );
}

// ── Field operation diagnostic tests ─────────────────────────────────────

/// Helper to build a FunctionCall expression for stdlib functions.
fn make_function_call(name: &str, args: Vec<CompiledExpr>, result_type: Type) -> CompiledExpr {
    let hash = ContentHash::of(name.as_bytes());
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: name.to_string(),
                qualified_name: format!("std::{}", name),
            },
            args,
        },
        result_type,
        content_hash: hash,
    }
}

/// sample(Real, Real) returns Undef when the first arg is not a Field.
#[test]
fn sample_non_field_returns_undef() {
    let expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar()),
            CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar()),
        ],
        Type::dimensionless_scalar(),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "sample of non-Field must return Undef"
    );
}

/// sample(Field { lambda: Undef }, point) returns Undef when the lambda is not callable.
#[test]
fn sample_field_with_undef_lambda_returns_undef() {
    let field = Value::Field {
        domain_type: Type::dimensionless_scalar(),
        codomain_type: Type::dimensionless_scalar(),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::Undef),
    };
    let expr = make_function_call(
        "sample",
        vec![
            CompiledExpr::literal(
                field,
                Type::Field {
                    domain: Box::new(Type::dimensionless_scalar()),
                    codomain: Box::new(Type::dimensionless_scalar()),
                },
            ),
            CompiledExpr::literal(Value::Real(0.5), Type::dimensionless_scalar()),
        ],
        Type::dimensionless_scalar(),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "sample of Field with Undef lambda must return Undef"
    );
}

/// gradient(Real) returns Undef when the argument is not a Field.
#[test]
fn gradient_non_field_returns_undef() {
    let expr = make_function_call(
        "gradient",
        vec![CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar())],
        Type::dimensionless_scalar(),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Undef,
        "gradient of non-Field must return Undef"
    );
}
