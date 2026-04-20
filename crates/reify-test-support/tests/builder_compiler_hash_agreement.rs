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
    // Extract qualified_name from the compiled output so this test is about
    // hash-algorithm agreement rather than resolver output format.  A separate
    // assertion still pins the expected qualified_name so resolver changes are
    // caught with a clear message rather than a silent hash-mismatch.
    let qualified_name = match &compiler_expr.kind {
        CompiledExprKind::FunctionCall { function, args } => {
            assert_eq!(
                function.qualified_name, "std::sin",
                "resolver produced unexpected qualified name"
            );
            assert_eq!(args.len(), 1);
            function.qualified_name.clone()
        }
        other => panic!("expected FunctionCall kind, got {:?}", other),
    };

    // Build equivalent via test-support builders, using the extracted
    // qualified_name so any future resolver rename only trips the assertion
    // above, not the hash-agreement assertion below.
    let expected = fn_call(
        "sin",
        &qualified_name,
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

/// Multi-arg FunctionCall: `fn t() -> Real { atan2(0.0, 1.0) }`
///
/// Verifies that argument hashes are combined in order — a two-arg call with
/// distinct arg values (0.0 ≠ 1.0) detects any argument-ordering regression
/// that the single-arg `sin` test cannot catch.  The qualified_name is
/// extracted from the compiled output so the test is about hash-algorithm
/// agreement rather than resolver format.
#[test]
fn fn_call_multi_arg_content_hash_matches_compiler() {
    // Use non-whole literals (0.5, 1.5) so the compiler represents them as
    // Value::Real rather than Value::Int (whole numbers like 0.0 and 1.0 are
    // folded to Int by the compiler's number-literal path).
    let source = "fn t() -> Real { atan2(0.5, 1.5) }";
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    let compiled = reify_compiler::compile(&parsed);

    assert!(
        compiled.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        compiled.diagnostics
    );
    assert_eq!(compiled.functions.len(), 1);

    let compiler_expr = &compiled.functions[0].body.result_expr;
    let qualified_name = match &compiler_expr.kind {
        CompiledExprKind::FunctionCall { function, args } => {
            assert_eq!(args.len(), 2);
            function.qualified_name.clone()
        }
        other => panic!("expected FunctionCall kind, got {:?}", other),
    };

    // Build equivalent via test-support builders using the extracted
    // qualified_name; arg order Real(0.5) before Real(1.5) must be preserved.
    let expected = fn_call(
        "atan2",
        &qualified_name,
        vec![literal(Value::Real(0.5)), literal(Value::Real(1.5))],
        Type::Real,
    );

    assert_eq!(
        compiler_expr.content_hash, expected.content_hash,
        "content_hash mismatch for multi-arg FunctionCall: compiler produced {:?}, builder produced {:?}",
        compiler_expr.content_hash, expected.content_hash
    );
}

/// Nested Conditional: `fn t() -> Int { if true then (if false then 1 else 2) else 3 }`
///
/// Verifies recursive hash composition — the outer Conditional's hash must
/// incorporate the inner Conditional's hash as the `then_branch` component.
/// This catches any regression where nesting breaks hash accumulation.
#[test]
fn nested_conditional_content_hash_matches_compiler() {
    let source = "fn t() -> Int { if true then (if false then 1 else 2) else 3 }";
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
        "expected outer Conditional kind, got {:?}",
        compiler_expr.kind
    );

    // Build the nested equivalent: the inner conditional is the then_branch of
    // the outer, so its hash feeds into the outer hash at the then_branch slot.
    let expected = conditional_expr(
        literal(Value::Bool(true)),
        conditional_expr(
            literal(Value::Bool(false)),
            literal(Value::Int(1)),
            literal(Value::Int(2)),
        ),
        literal(Value::Int(3)),
    );

    assert_eq!(
        compiler_expr.content_hash, expected.content_hash,
        "content_hash mismatch for nested Conditional: compiler produced {:?}, builder produced {:?}",
        compiler_expr.content_hash, expected.content_hash
    );
}
