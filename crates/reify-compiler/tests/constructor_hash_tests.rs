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

use reify_test_support::compile_source;
use reify_types::{CompiledExpr, CompiledExprKind};

// ── shared helper ────────────────────────────────────────────────────────────

/// Retrieve the compiled `default_expr` of a let binding by name.
fn get_let_expr<'a>(module: &'a reify_compiler::CompiledModule, name: &str) -> &'a CompiledExpr {
    let template = module
        .templates
        .first()
        .expect("expected at least one template in module");
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == name)
        .unwrap_or_else(|| panic!("no value cell named '{name}'"));
    cell.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("value cell '{name}' has no default expr"))
}

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

    let CompiledExprKind::UserFunctionCall { function_name, args } = &compiled.kind else {
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
        panic!(
            "expected v to compile to Match, got {:?}",
            compiled.kind
        );
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
