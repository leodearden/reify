//! Integration tests verifying that the `CompiledExpr` constructors produce
//! the same content hash as the compiler's inline emission path.
//!
//! Background: `reify-types` owns `CompiledExpr::user_function_call` and
//! `CompiledExpr::match_expr` constructors that encode the hash formula.
//! `reify-compiler/src/expr.rs` has inline counterparts at the emission sites
//! for those variants.  These tests assert the two paths agree, so a
//! divergence (e.g. a tag byte or combine-order change in one place) fails a
//! test rather than silently producing inconsistent hashes.
//!
//! Method: compile Reify source that emits the variant, extract the compiled
//! node, then *rebuild* an equivalent node via the public constructor using the
//! exact same sub-expressions (by cloning them out of the compiled result).
//! Because both sides hash the same sub-expression trees, any difference in the
//! formula (tag byte, combine order, missing field) will surface here.

use reify_test_support::{compile_source, get_let_expr};
use reify_ir::{CompiledExpr, CompiledExprKind};

// ── UserFunctionCall ─────────────────────────────────────────────────────────

/// Verify that `CompiledExpr::user_function_call` produces the same content
/// hash as the compiler's inline emission in `reify-compiler/src/expr.rs`.
///
/// The test compiles a minimal user-function call `f(5)`, extracts the
/// compiled `UserFunctionCall` node, then rebuilds an equivalent node via the
/// public constructor using the compiler-produced `function_name` and `args`.
/// Hash equality means the tag byte and combine formula are identical in both
/// paths.
#[test]
fn constructor_hash_matches_compiler_for_user_function_call() {
    let source = r#"
fn f(x: Int) -> Int { x }
structure S {
    let v = f(5)
}
"#;
    let module = compile_source(source);
    let compiled = get_let_expr(&module, "v");

    let CompiledExprKind::UserFunctionCall {
        function_name,
        args,
    } = &compiled.kind
    else {
        panic!(
            "expected v to compile to UserFunctionCall, got {:?}",
            compiled.kind
        );
    };

    // Rebuild via the public constructor with the exact same sub-expressions.
    // If the hash formula in the constructor matches the compiler's inline
    // formula, the hashes must be equal.
    let reconstructed = CompiledExpr::user_function_call(
        function_name.clone(),
        args.clone(),
        compiled.result_type.clone(),
    );
    assert_eq!(
        compiled.content_hash, reconstructed.content_hash,
        "CompiledExpr::user_function_call constructor hash must match \
         the compiler's inline emission hash for the same inputs — \
         tag byte or combine order may have diverged"
    );
}

/// Verify the `user_function_call` constructor's per-arg combine order agrees
/// with the compiler's inline emission when the call has ≥2 arguments.
///
/// A single-arg call hashes `tag • name • arg0`, which is indistinguishable from
/// `tag • name • arg0 • <nothing>`.  With two or more args the sequence matters:
/// a refactor that folds args in reverse, or drops the second arg, would be
/// caught only here and not by the single-arg case above.
#[test]
fn constructor_hash_matches_compiler_for_multi_arg_user_function_call() {
    let source = r#"
fn f(x: Int, y: Int) -> Int { x + y }
structure S {
    let v = f(3, 5)
}
"#;
    let module = compile_source(source);
    let compiled = get_let_expr(&module, "v");

    let CompiledExprKind::UserFunctionCall {
        function_name,
        args,
    } = &compiled.kind
    else {
        panic!(
            "expected v to compile to UserFunctionCall, got {:?}",
            compiled.kind
        );
    };
    assert!(args.len() >= 2, "expected ≥2 args, got {}", args.len());

    let reconstructed = CompiledExpr::user_function_call(
        function_name.clone(),
        args.clone(),
        compiled.result_type.clone(),
    );
    assert_eq!(
        compiled.content_hash, reconstructed.content_hash,
        "CompiledExpr::user_function_call constructor hash must match \
         the compiler's inline emission hash for the same inputs — \
         per-arg combine order may have diverged"
    );
}

// ── Match ────────────────────────────────────────────────────────────────────

/// Verify that `CompiledExpr::match_expr` produces the same content hash as
/// the compiler's inline emission in `reify-compiler/src/expr.rs`.
///
/// The test compiles a two-arm match expression, extracts the compiled `Match`
/// node, then rebuilds via the public constructor using the same discriminant
/// and arms.  Hash equality confirms the tag byte and per-arm combine order
/// (pattern strings then body) are consistent between constructor and compiler.
#[test]
fn constructor_hash_matches_compiler_for_match_expr() {
    let source = r#"
enum Color { Red, Blue }
structure S {
    let c = Color.Red
    let v = match c { Red => 1, Blue => 2 }
}
"#;
    let module = compile_source(source);
    let compiled = get_let_expr(&module, "v");

    let CompiledExprKind::Match { discriminant, arms } = &compiled.kind else {
        panic!("expected v to compile to Match, got {:?}", compiled.kind);
    };

    // Rebuild via the public constructor with the exact same sub-expressions.
    let reconstructed = CompiledExpr::match_expr(
        // discriminant is Box<CompiledExpr>; deref-clone to get CompiledExpr.
        (**discriminant).clone(),
        arms.clone(),
        compiled.result_type.clone(),
    );
    assert_eq!(
        compiled.content_hash, reconstructed.content_hash,
        "CompiledExpr::match_expr constructor hash must match \
         the compiler's inline emission hash for the same inputs — \
         tag byte or combine order may have diverged"
    );
}

/// Verify the `match_expr` constructor agrees with the compiler when arms have
/// multiple patterns (`Red | Blue => ...`) and the match also contains a
/// wildcard arm.
///
/// The single-pattern two-arm case above fixes arm-order and tag byte, but
/// leaves the per-pattern combine loop inside a single arm unverified.  A
/// refactor that, say, combines only `patterns[0]` per arm, or folds patterns
/// in reverse, would pass the single-pattern test and fail here.
#[test]
fn constructor_hash_matches_compiler_for_multi_pattern_match() {
    let source = r#"
enum Color { Red, Blue, Green }
structure S {
    let c = Color.Red
    let v = match c { Red | Blue => 1, _ => 2 }
}
"#;
    let module = compile_source(source);
    let compiled = get_let_expr(&module, "v");

    let CompiledExprKind::Match { discriminant, arms } = &compiled.kind else {
        panic!("expected v to compile to Match, got {:?}", compiled.kind);
    };
    assert!(arms.len() >= 2, "expected ≥2 arms, got {}", arms.len());
    assert!(
        arms.iter().any(|a| a.patterns.len() >= 2),
        "expected at least one arm with ≥2 patterns, got {:?}",
        arms.iter().map(|a| &a.patterns).collect::<Vec<_>>()
    );

    let reconstructed = CompiledExpr::match_expr(
        (**discriminant).clone(),
        arms.clone(),
        compiled.result_type.clone(),
    );
    assert_eq!(
        compiled.content_hash, reconstructed.content_hash,
        "CompiledExpr::match_expr constructor hash must match the \
         compiler's inline emission hash for multi-pattern arms — \
         per-pattern combine order within an arm may have diverged"
    );
}
