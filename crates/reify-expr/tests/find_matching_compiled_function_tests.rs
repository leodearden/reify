//! Direct unit tests for `reify_expr::find_matching_compiled_function`.
//!
//! The helper is a small, pure first-match-wins overload-resolution function
//! shared by two call sites:
//!   - `eval_user_function_call` in `reify-expr/src/lib.rs`
//!   - the `@optimized` → `ComputeNode` lowering site in `reify-eval/src/engine_eval.rs`
//!
//! These tests exercise the five distinct behaviour axes of the matcher in
//! isolation, independent of the end-to-end eval paths covered by
//! `lambda_eval_tests.rs` and `compute_dispatch_registry.rs`.

use reify_expr::find_matching_compiled_function;
use reify_core::{ContentHash, Type};
use reify_ir::{CompiledExpr, CompiledFnBody, CompiledFunction, Value};

/// Build a minimal `CompiledFunction` with the given name and a single
/// parameter of the given type. The body is a constant `Int(0)` literal —
/// irrelevant for matching purposes.
fn make_fn(name: &str, param_type: Type) -> CompiledFunction {
    let params = vec![("x".to_string(), param_type)];
    CompiledFunction {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        param_defaults: CompiledFunction::no_defaults_for(&params),
        params,
        return_type: Type::Int,
        body: CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::literal(Value::Int(0), Type::Int),
        },
        content_hash: ContentHash::of(name.as_bytes()),
        annotations: vec![],
        optimized_target: None,
    }
}

/// Build a minimal `CompiledFunction` with the given name and NO parameters.
fn make_fn_nullary(name: &str) -> CompiledFunction {
    let params: Vec<(String, Type)> = vec![];
    CompiledFunction {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        param_defaults: CompiledFunction::no_defaults_for(&params),
        params,
        return_type: Type::Int,
        body: CompiledFnBody {
            let_bindings: vec![],
            result_expr: CompiledExpr::literal(Value::Int(0), Type::Int),
        },
        content_hash: ContentHash::of(name.as_bytes()),
        annotations: vec![],
        optimized_target: None,
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Test: name mismatch → None
// ────────────────────────────────────────────────────────────────────────────

/// When no function in the slice has the requested name, the helper returns
/// `None` even if arity and types would otherwise match.
#[test]
fn name_mismatch_returns_none() {
    let fns = vec![make_fn("foo", Type::Int)];
    // arg has the right type and arity, but name "bar" doesn't exist
    let args = [CompiledExpr::literal(Value::Int(0), Type::Int)];
    let result = find_matching_compiled_function(&fns, "bar", &args);
    assert!(result.is_none(), "expected None for name mismatch");
}

// ────────────────────────────────────────────────────────────────────────────
// Test: arity mismatch → None
// ────────────────────────────────────────────────────────────────────────────

/// When the function exists by name but takes a different number of parameters,
/// the helper returns `None`.
#[test]
fn arity_mismatch_returns_none() {
    // function takes 1 param; we pass 0 args
    let fns = vec![make_fn("foo", Type::Int)];
    let args: [CompiledExpr; 0] = [];
    let result = find_matching_compiled_function(&fns, "foo", &args);
    assert!(result.is_none(), "expected None for arity mismatch (0 args vs 1 param)");

    // function takes 0 params; we pass 1 arg
    let fns_nullary = vec![make_fn_nullary("bar")];
    let args_one = [CompiledExpr::literal(Value::Int(0), Type::Int)];
    let result2 = find_matching_compiled_function(&fns_nullary, "bar", &args_one);
    assert!(result2.is_none(), "expected None for arity mismatch (1 arg vs 0 params)");
}

// ────────────────────────────────────────────────────────────────────────────
// Test: per-param type mismatch → None
// ────────────────────────────────────────────────────────────────────────────

/// When name and arity match but the parameter type differs from the arg's
/// `result_type`, the helper returns `None`.
///
/// Concretely: function expects `Type::Int`, arg carries `Type::Real`.
#[test]
fn param_type_mismatch_returns_none() {
    // function param is Type::Int
    let fns = vec![make_fn("foo", Type::Int)];
    // arg result_type is Type::Real  → should not match
    let args = [CompiledExpr::literal(Value::Real(1.0), Type::Real)];
    let result = find_matching_compiled_function(&fns, "foo", &args);
    assert!(result.is_none(), "expected None for per-param type mismatch (Int param vs Real arg)");
}

// ────────────────────────────────────────────────────────────────────────────
// Test: exact match → Some
// ────────────────────────────────────────────────────────────────────────────

/// When name, arity, and all per-param types match exactly, the helper returns
/// `Some` pointing to the matching `CompiledFunction`.
#[test]
fn exact_match_returns_some() {
    let f = make_fn("foo", Type::Int);
    let fns = vec![f.clone()];
    let args = [CompiledExpr::literal(Value::Int(42), Type::Int)];
    let result = find_matching_compiled_function(&fns, "foo", &args);
    assert!(result.is_some(), "expected Some for exact match");
    assert_eq!(result.unwrap().name, "foo");
}

// ────────────────────────────────────────────────────────────────────────────
// Test: first-match-wins ordering
// ────────────────────────────────────────────────────────────────────────────

/// When multiple functions in the slice all satisfy name + arity + type
/// constraints, the helper returns the *first* one in iteration order.
///
/// This test uses two functions with the same name and compatible signature
/// but different `content_hash` values so we can identify which one was
/// returned.
#[test]
fn first_match_wins() {
    // Two functions, both named "foo", both taking a single Int param.
    let mut first = make_fn("foo", Type::Int);
    first.content_hash = ContentHash::of(b"first");

    let mut second = make_fn("foo", Type::Int);
    second.content_hash = ContentHash::of(b"second");

    let fns = vec![first.clone(), second];

    let args = [CompiledExpr::literal(Value::Int(0), Type::Int)];
    let result = find_matching_compiled_function(&fns, "foo", &args);

    assert!(result.is_some(), "expected Some");
    assert_eq!(
        result.unwrap().content_hash,
        first.content_hash,
        "expected the first matching function to be returned"
    );
}
