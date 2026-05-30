//! Lowering-equivalence tests for the expression-body `fn` sugar (task 3919).
//!
//! Spec §18 #10: `fn f(x: T) -> T = expr` is pure syntactic sugar for the block
//! form `fn f(x: T) -> T { expr }` with no let bindings.
//!
//! These tests assert that both forms produce an **identical** `FnBody` structure
//! (modulo span):
//!   - `let_bindings` is empty in both cases
//!   - `result_expr` is structurally equal (a `BinOp { "*", Ident("x"), NumberLiteral(2) }`)
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

// ── Expression form ───────────────────────────────────────────────────────────

/// The expression body `fn double(x: Int) -> Int = x * 2` lowers to a `FnBody`
/// with `let_bindings` empty.
#[test]
fn expr_body_has_empty_let_bindings() {
    let body = parse_fn_body("fn double(x: Int) -> Int = x * 2");
    assert!(
        body.let_bindings.is_empty(),
        "expression-body fn must lower to FnBody with no let bindings; \
         got {} let binding(s)",
        body.let_bindings.len(),
    );
}

/// The expression body `fn double(x: Int) -> Int = x * 2` lowers to a `FnBody`
/// whose `result_expr` is `BinOp { op: "*", left: Ident("x"), right: NumberLiteral(2) }`.
#[test]
fn expr_body_result_expr_is_binop_mul() {
    let body = parse_fn_body("fn double(x: Int) -> Int = x * 2");
    match &body.result_expr.kind {
        ExprKind::BinOp { op, left, right } => {
            assert_eq!(op, "*", "result_expr op must be \"*\"");
            assert!(
                matches!(&left.kind, ExprKind::Ident(name) if name == "x"),
                "result_expr left must be Ident(\"x\"); got {:?}",
                left.kind,
            );
            assert!(
                matches!(&right.kind, ExprKind::NumberLiteral { value, is_real: false } if (*value - 2.0).abs() < f64::EPSILON),
                "result_expr right must be NumberLiteral {{ value: 2.0, is_real: false }}; got {:?}",
                right.kind,
            );
        }
        other => panic!(
            "result_expr must be BinOp for `x * 2`; got {:?}",
            other
        ),
    }
}

// ── Block form (regression guard) ─────────────────────────────────────────────

/// The block body `fn double(x: Int) -> Int { x * 2 }` lowers to a `FnBody`
/// with `let_bindings` empty.
#[test]
fn block_body_has_empty_let_bindings() {
    let body = parse_fn_body("fn double(x: Int) -> Int { x * 2 }");
    assert!(
        body.let_bindings.is_empty(),
        "block-body fn (no let bindings) must lower to FnBody with no let bindings; \
         got {} let binding(s)",
        body.let_bindings.len(),
    );
}

/// The block body `fn double(x: Int) -> Int { x * 2 }` lowers to a `FnBody`
/// whose `result_expr` is `BinOp { op: "*", left: Ident("x"), right: NumberLiteral(2) }`.
#[test]
fn block_body_result_expr_is_binop_mul() {
    let body = parse_fn_body("fn double(x: Int) -> Int { x * 2 }");
    match &body.result_expr.kind {
        ExprKind::BinOp { op, left, right } => {
            assert_eq!(op, "*", "result_expr op must be \"*\"");
            assert!(
                matches!(&left.kind, ExprKind::Ident(name) if name == "x"),
                "result_expr left must be Ident(\"x\"); got {:?}",
                left.kind,
            );
            assert!(
                matches!(&right.kind, ExprKind::NumberLiteral { value, is_real: false } if (*value - 2.0).abs() < f64::EPSILON),
                "result_expr right must be NumberLiteral {{ value: 2.0, is_real: false }}; got {:?}",
                right.kind,
            );
        }
        other => panic!(
            "result_expr must be BinOp for `x * 2`; got {:?}",
            other
        ),
    }
}

// ── Equivalence (modulo span) ─────────────────────────────────────────────────

/// Both forms lower to a `FnBody` with `let_bindings` of equal length (both empty).
#[test]
fn expr_and_block_let_bindings_lengths_equal() {
    let expr_body = parse_fn_body("fn double(x: Int) -> Int = x * 2");
    let block_body = parse_fn_body("fn double(x: Int) -> Int { x * 2 }");
    assert_eq!(
        expr_body.let_bindings.len(),
        block_body.let_bindings.len(),
        "expression and block forms must produce equal let_bindings length",
    );
}

/// Both forms produce a `result_expr` with the same `ExprKind` discriminant
/// (`BinOp`), the same operator (`*`), and structurally identical operands.
///
/// Span fields differ between the two forms (different source positions) so we
/// compare structural shape only, not the span.
#[test]
fn expr_and_block_result_expr_shapes_equal() {
    let expr_body = parse_fn_body("fn double(x: Int) -> Int = x * 2");
    let block_body = parse_fn_body("fn double(x: Int) -> Int { x * 2 }");

    // Both must be BinOp "*"
    let (expr_op, expr_left, expr_right) = match &expr_body.result_expr.kind {
        ExprKind::BinOp { op, left, right } => (op.clone(), left.as_ref(), right.as_ref()),
        other => panic!("expression body: expected BinOp, got {:?}", other),
    };
    let (block_op, block_left, block_right) = match &block_body.result_expr.kind {
        ExprKind::BinOp { op, left, right } => (op.clone(), left.as_ref(), right.as_ref()),
        other => panic!("block body: expected BinOp, got {:?}", other),
    };

    assert_eq!(expr_op, block_op, "operator must be equal");

    // Left operands: both Ident("x")
    let expr_left_name = match &expr_left.kind {
        ExprKind::Ident(n) => n.clone(),
        other => panic!("expression body left: expected Ident, got {:?}", other),
    };
    let block_left_name = match &block_left.kind {
        ExprKind::Ident(n) => n.clone(),
        other => panic!("block body left: expected Ident, got {:?}", other),
    };
    assert_eq!(expr_left_name, block_left_name, "left operand names must be equal");

    // Right operands: both NumberLiteral { value: 2.0, is_real: false }
    let (expr_right_val, expr_right_is_real) = match &expr_right.kind {
        ExprKind::NumberLiteral { value, is_real } => (*value, *is_real),
        other => panic!("expression body right: expected NumberLiteral, got {:?}", other),
    };
    let (block_right_val, block_right_is_real) = match &block_right.kind {
        ExprKind::NumberLiteral { value, is_real } => (*value, *is_real),
        other => panic!("block body right: expected NumberLiteral, got {:?}", other),
    };
    assert!(
        (expr_right_val - block_right_val).abs() < f64::EPSILON,
        "right operand values must be equal: {} vs {}",
        expr_right_val,
        block_right_val,
    );
    assert_eq!(
        expr_right_is_real, block_right_is_real,
        "right operand is_real flags must be equal",
    );
}
