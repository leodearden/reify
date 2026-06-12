//! Dispatch tests for the `fn_field` intercepting-builtin (task 4220 β,
//! PRD docs/prds/v0_6/std-fields-api.md §5.2).
//!
//! Boundary tests covered:
//!   B1  — `sample(fn_field(|p| 2.0 * p), 3.0)` evaluates to `Value::Real(6.0)`
//!   (B10 typing invariant: the compiled FunctionCall node carries
//!    `result_type = Field<Real,Real>` which the arm reads for domain/codomain)
//!
//! Model: `field_op_dispatch_tests.rs` — same direct-Value construction +
//! `eval_expr(&expr, &EvalContext::simple(&ValueMap::new()))` pattern.
//!
//! Both tests are RED before step-2 (the impl arm lands):
//!   - `fn_field(lambda)` falls through to `reify_stdlib::eval_builtin` (no
//!     binding) → `Value::Undef`
//!   - `sample(Undef, 3.0)` → strict Undef propagation → `Value::Undef`

use reify_core::{ContentHash, Type, ValueCellId};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{
    BinOp, CompiledExpr, CompiledExprKind, FieldSourceKind, ResolvedFunction, Value, ValueMap,
};

// ── shared helpers ──────────────────────────────────────────────────────────

/// Build a `Value::Lambda` for `|p| 2.0 * p` (Real → Real).
///
/// Used as the fn_field argument: `fn_field(|p| 2.0 * p)`.
fn make_double_lambda(scope: &str) -> Value {
    let p_id = ValueCellId::new(scope, "p");
    Value::Lambda {
        params: vec![("p".to_string(), p_id.clone())],
        body: Box::new(CompiledExpr::binop(
            BinOp::Mul,
            CompiledExpr::value_ref(p_id, Type::dimensionless_scalar()),
            CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar()),
            Type::dimensionless_scalar(),
        )),
        captures: ValueMap::new(),
    }
}

/// Build a `fn_field(lambda)` FunctionCall `CompiledExpr` whose
/// `result_type = Field<Real, Real>` (α's `field_op_result_type` stamps this).
///
/// The lambda arg is wrapped in a `CompiledExpr::literal` with
/// `Type::Function { params: [Real], return_type: Real }` — the type the
/// compiler assigns to a `|p: Real| … : Real` lambda literal.
fn make_fn_field_call(lambda: Value) -> CompiledExpr {
    let hash = ContentHash::of(b"fn_field_dispatch_test");
    let lambda_type = Type::Function {
        params: vec![Type::dimensionless_scalar()],
        return_type: Box::new(Type::dimensionless_scalar()),
    };
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: "fn_field".to_string(),
                qualified_name: "std::fn_field".to_string(),
            },
            args: vec![CompiledExpr::literal(lambda, lambda_type)],
        },
        result_type: Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(Type::dimensionless_scalar()),
        },
        content_hash: hash,
    }
}

/// Build a `sample(field_expr, at)` FunctionCall `CompiledExpr` where the field
/// argument is itself a `CompiledExpr` (allowing the fn_field call to be nested
/// directly rather than pre-evaluated to a literal).
fn make_sample_of_expr(field_expr: CompiledExpr, at: Value) -> CompiledExpr {
    let hash = ContentHash::of(b"sample_of_fn_field_test");
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: "sample".to_string(),
                qualified_name: "std::sample".to_string(),
            },
            args: vec![
                field_expr,
                CompiledExpr::literal(at, Type::dimensionless_scalar()),
            ],
        },
        result_type: Type::dimensionless_scalar(),
        content_hash: hash,
    }
}

// ── tests ───────────────────────────────────────────────────────────────────

/// `fn_field(|p| 2.0 * p)` must evaluate to a `Value::Field` whose source is
/// `FieldSourceKind::Analytical`, lambda slot contains the original
/// `Value::Lambda`, and domain/codomain are both `Type::dimensionless_scalar()` (read from
/// the node's `result_type = Field<Real,Real>` stamped by α).
///
/// **RED before step-2**: no fn_field arm → falls through to
/// `reify_stdlib::eval_builtin` (no binding) → `Value::Undef`.
///
/// **GREEN after step-2**: the `"fn_field"` arm constructs and returns the
/// `Value::Field { source: Analytical, lambda: Arc(Value::Lambda{..}), .. }`.
#[test]
fn fn_field_evaluates_to_analytical_field() {
    let lambda = make_double_lambda("$fn_field_dispatch_test_a.S");
    let fn_field_expr = make_fn_field_call(lambda);

    let result = eval_expr(&fn_field_expr, &EvalContext::simple(&ValueMap::new()));

    assert!(
        matches!(
            &result,
            Value::Field {
                source: FieldSourceKind::Analytical,
                domain_type,
                codomain_type,
                lambda,
            } if *domain_type == Type::dimensionless_scalar()
              && *codomain_type == Type::dimensionless_scalar()
              && matches!(lambda.as_ref(), Value::Lambda { .. })
        ),
        "fn_field(|p| 2.0 * p) must yield Value::Field {{ source: Analytical, \
         domain_type: Real, codomain_type: Real, lambda: Value::Lambda{{..}} }}; got {:?}",
        result
    );
}

/// `sample(fn_field(|p| 2.0 * p), 3.0)` must evaluate to `Value::Real(6.0)`.
///
/// This is the B1 boundary test: the fn_field arm constructs a
/// `Value::Field { source: Analytical, lambda: |p| 2.0 * p }`, which the
/// existing `sample_field_at` → `apply_lambda_with_point_unpacking` path
/// then samples at `3.0`, yielding `2.0 * 3.0 = 6.0`.
///
/// The fn_field call is nested directly as the first arg of `sample` so that
/// eval_expr evaluates it inline (not pre-computed as a literal).
///
/// **RED before step-2**: `fn_field(...)` → `Undef` → strict Undef
/// propagation in `sample` → `Undef`, not `Real(6.0)`.
///
/// **GREEN after step-2**: `fn_field(...)` → `Value::Field{..}` →
/// `sample(field, 3.0)` → `Value::Real(6.0)`.
#[test]
fn sample_fn_field_evaluates_to_real_b1() {
    let lambda = make_double_lambda("$fn_field_dispatch_test_b.S");
    let fn_field_expr = make_fn_field_call(lambda);
    let sample_expr = make_sample_of_expr(fn_field_expr, Value::Real(3.0));

    let result = eval_expr(&sample_expr, &EvalContext::simple(&ValueMap::new()));

    assert_eq!(
        result,
        Value::Real(6.0),
        "sample(fn_field(|p| 2.0 * p), 3.0) must be 6.0 (B1), got {:?}",
        result
    );
}

/// `fn_field(3.0)` — arg is a non-lambda `Value::Real` — must fall through to
/// `reify_stdlib::eval_builtin` (no `fn_field` binding there) and return
/// `Value::Undef`.
///
/// This pins the documented strict fall-through guarantee: when the match guard
/// `matches!(&evaluated_args[0], Value::Lambda { .. })` is not satisfied, the
/// arm is skipped and the call degrades gracefully to `Undef` rather than
/// panicking or constructing a malformed `Value::Field`.
///
/// Note: `field_op_result_type` in the compiler returns `None` for non-Function
/// args, so a well-typed tree never reaches this path.  This test covers the
/// runtime contract for a mistyped or hand-constructed tree.
#[test]
fn fn_field_non_lambda_arg_falls_through_to_undef() {
    let hash = reify_core::ContentHash::of(b"fn_field_non_lambda_arg_test");
    // Build `fn_field(3.0)` — arg is Value::Real(3.0), not a lambda.
    let expr = CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: "fn_field".to_string(),
                qualified_name: "std::fn_field".to_string(),
            },
            args: vec![CompiledExpr::literal(Value::Real(3.0), Type::dimensionless_scalar())],
        },
        result_type: Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(Type::dimensionless_scalar()),
        },
        content_hash: hash,
    };

    let result = eval_expr(&expr, &EvalContext::simple(&ValueMap::new()));

    assert_eq!(
        result,
        Value::Undef,
        "fn_field(non-lambda) must fall through to Undef, got {:?}",
        result
    );
}
