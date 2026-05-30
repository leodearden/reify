//! Lowering-equivalence tests for the expression-body `fn` sugar (task 3919).
//!
//! Spec §18 #10: `fn f(x: T) -> T = expr` is pure syntactic sugar for the block
//! form `fn f(x: T) -> T { expr }` with no let bindings.
//!
//! The expression form is a pure desugar: `lower_fn_body` is unchanged because both
//! grammar arms share the `result` field name, so the generic lowering collects
//! zero `fn_let_binding` children and reads the same `result` field in both arms.
//!
//! User-observable signal:
//!   `cargo test -p reify-syntax --test fn_body_expr_parser_tests`

use reify_ast::*;

/// Parse a top-level `fn` declaration and extract its `FnBody`.
///
/// Panics if parsing fails or the source is not a single `Function` declaration.
fn parse_fn_body(source: &str) -> FnBody {
    let module = reify_syntax::parse(source, reify_core::ModulePath::single("fn_body_test"));
    assert!(
        module.errors.is_empty(),
        "expected no parse errors for `{source}`; got: {:?}",
        module.errors,
    );
    match module.declarations.as_slice() {
        [Declaration::Function(f)] => f.body.clone(),
        other => panic!(
            "expected exactly one Function declaration; got: {:?}",
            other
        ),
    }
}

/// Destructure a `result_expr` and assert it is `BinOp { "*", Ident("x"), NumberLiteral(2.0) }`.
///
/// Extracted to avoid copy-pasting the four-level match across multiple tests.
fn assert_result_is_binop_mul_x_2(expr: &Expr, ctx: &str) {
    match &expr.kind {
        ExprKind::BinOp { op, left, right } => {
            assert_eq!(op, "*", "[{ctx}] result_expr op must be \"*\"");
            assert!(
                matches!(&left.kind, ExprKind::Ident(name) if name == "x"),
                "[{ctx}] result_expr left must be Ident(\"x\"); got {:?}",
                left.kind,
            );
            assert!(
                matches!(&right.kind, ExprKind::NumberLiteral { value, is_real: false }
                    if (*value - 2.0).abs() < f64::EPSILON),
                "[{ctx}] result_expr right must be NumberLiteral {{ value: 2.0, is_real: false }}; \
                 got {:?}",
                right.kind,
            );
        }
        other => panic!(
            "[{ctx}] result_expr must be BinOp for `x * 2`; got {:?}",
            other
        ),
    }
}

// ── Equivalence (modulo span) ─────────────────────────────────────────────────

/// Both fn_body forms lower to an equivalent `FnBody`:
///   - `let_bindings` is empty in both cases
///   - `result_expr` has the same shape (`BinOp { "*", Ident("x"), NumberLiteral(2.0) }`)
///
/// This is the key desugar claim: the expression arm shares the `result` field
/// with the block arm so `lower_fn_body` requires no branching.
/// Span fields differ between the two forms and are intentionally not compared.
#[test]
fn both_forms_produce_equivalent_fn_body() {
    let expr_body = parse_fn_body("fn double(x: Int) -> Int = x * 2");
    let block_body = parse_fn_body("fn double(x: Int) -> Int { x * 2 }");

    assert_eq!(
        expr_body.let_bindings.len(),
        block_body.let_bindings.len(),
        "expression and block forms must produce equal let_bindings length",
    );
    assert!(
        expr_body.let_bindings.is_empty(),
        "both forms must produce empty let_bindings when there are no `let` bindings; \
         expr form has {}",
        expr_body.let_bindings.len(),
    );

    assert_result_is_binop_mul_x_2(&expr_body.result_expr, "expression form");
    assert_result_is_binop_mul_x_2(&block_body.result_expr, "block form");
}

// ── Non-empty let_bindings (positive coverage for the collection branch) ──────

/// A block body with one `let` binding lowers to a `FnBody` with exactly one
/// entry in `let_bindings`, confirming that `lower_fn_body`'s `fn_let_binding`
/// collection loop is exercised and not silently dropped.
///
/// This anchors the desugar claim: the expression form produces zero bindings
/// because the grammar emits zero `fn_let_binding` children — not because the
/// collector is broken and always returns empty.
#[test]
fn block_body_with_let_binding_lowers_correctly() {
    let body = parse_fn_body("fn f(x: Int) -> Int { let y = x; y }");

    assert_eq!(
        body.let_bindings.len(),
        1,
        "fn body with one let binding must lower to exactly one LetDecl; got {}",
        body.let_bindings.len(),
    );

    let binding = &body.let_bindings[0];
    assert_eq!(
        binding.name, "y",
        "let binding name must be \"y\"; got {:?}",
        binding.name,
    );
    assert!(
        matches!(&binding.value.kind, ExprKind::Ident(name) if name == "x"),
        "let binding value must be Ident(\"x\"); got {:?}",
        binding.value.kind,
    );

    // result_expr is the final `y` identifier
    assert!(
        matches!(&body.result_expr.kind, ExprKind::Ident(name) if name == "y"),
        "result_expr must be Ident(\"y\"); got {:?}",
        body.result_expr.kind,
    );
}
