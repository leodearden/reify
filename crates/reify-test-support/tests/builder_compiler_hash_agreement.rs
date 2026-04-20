/// Cross-crate agreement tests: content_hash values produced by the
/// reify-test-support expression builders must match those produced by the
/// reify-compiler for equivalent expressions.
///
/// These tests compile a tiny source snippet with the real compiler, extract
/// the `result_expr.content_hash`, then build the equivalent expression via
/// the test-support builders and assert the hashes are equal.  Any tag-byte
/// or string divergence between the two code paths will be caught immediately.
use reify_test_support::builders::{conditional_expr, fn_call, literal, user_fn_call};
use reify_types::{CompiledExprKind, ModulePath, Type, Value};

/// Conditional: `fn t() -> Int { if true then 1 else 2 }`
///
/// The compiler produces a `Conditional` with content-hash tag `[5]`.
/// The test-support `conditional_expr` builder must use the same tag.
#[test]
fn conditional_expr_content_hash_matches_compiler() {
    let source = "fn t() -> Int { if true then 1 else 2 }";
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    let compiled = reify_compiler::compile(&parsed);

    assert!(
        compiled.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        compiled.diagnostics
    );
    assert_eq!(compiled.functions.len(), 1);

    let compiler_expr = &compiled.functions[0].body.result_expr;
    assert!(
        matches!(compiler_expr.kind, CompiledExprKind::Conditional { .. }),
        "expected Conditional kind, got {:?}",
        compiler_expr.kind
    );

    // Build equivalent via test-support builders
    let expected = conditional_expr(
        literal(Value::Bool(true)),
        literal(Value::Int(1)),
        literal(Value::Int(2)),
    );

    assert_eq!(
        compiler_expr.content_hash, expected.content_hash,
        "content_hash mismatch for Conditional: compiler produced {:?}, builder produced {:?}",
        compiler_expr.content_hash, expected.content_hash
    );
}

/// FunctionCall: `fn t() -> Real { sin(0.5) }`
///
/// Without stdlib loaded, the compiler produces a `FunctionCall` with
/// `qualified_name = "std::sin"` and content-hash tag `[4]` combined with
/// `ContentHash::of_str("std::sin")`.  The test-support `fn_call` builder
/// must use the same tag and hash the qualified name.
#[test]
fn fn_call_content_hash_matches_compiler() {
    let source = "fn t() -> Real { sin(0.5) }";
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    let compiled = reify_compiler::compile(&parsed);

    assert!(
        compiled.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        compiled.diagnostics
    );
    assert_eq!(compiled.functions.len(), 1);

    let compiler_expr = &compiled.functions[0].body.result_expr;
    match &compiler_expr.kind {
        CompiledExprKind::FunctionCall { function, args } => {
            assert_eq!(function.qualified_name, "std::sin");
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected FunctionCall kind, got {:?}", other),
    }

    // Build equivalent via test-support builders
    let expected = fn_call(
        "sin",
        "std::sin",
        vec![literal(Value::Real(0.5))],
        Type::Real,
    );

    assert_eq!(
        compiler_expr.content_hash, expected.content_hash,
        "content_hash mismatch for FunctionCall: compiler produced {:?}, builder produced {:?}",
        compiler_expr.content_hash, expected.content_hash
    );
}

/// UserFunctionCall: `fn add1(x: Int) -> Int { x + 1 }\nfn t() -> Int { add1(2) }`
///
/// Regression-armor test: the `user_fn_call` builder already uses the correct
/// tag `[6]`.  This test locks in the contract to catch any future drift.
#[test]
fn user_fn_call_content_hash_matches_compiler() {
    let source = "fn add1(x: Int) -> Int { x + 1 }\nfn t() -> Int { add1(2) }";
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    let compiled = reify_compiler::compile(&parsed);

    assert!(
        compiled.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        compiled.diagnostics
    );
    assert_eq!(compiled.functions.len(), 2);

    let compiler_expr = &compiled.functions[1].body.result_expr;
    match &compiler_expr.kind {
        CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } => {
            assert_eq!(function_name, "add1");
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected UserFunctionCall kind, got {:?}", other),
    }

    // Build equivalent via test-support builders
    let expected = user_fn_call("add1", vec![literal(Value::Int(2))], Type::Int);

    assert_eq!(
        compiler_expr.content_hash, expected.content_hash,
        "content_hash mismatch for UserFunctionCall: compiler produced {:?}, builder produced {:?}",
        compiler_expr.content_hash, expected.content_hash
    );
}
