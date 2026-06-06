//! Compiler-behaviour tests for task 3968: `InterpolatedString` lowering
//! (render-then-concat fold, type `Type::String`).
//!
//! ## Coverage
//!
//! (a) `arith_hole_folds_and_evals_to_string` — `"x={1+1}"` must compile to a
//!     `BinOp { op: Add, right: FunctionCall("std::__interp_render", …) }` with
//!     `result_type == Type::String`, emit NO "not yet" diagnostic, and evaluate
//!     hermetically to `Value::String("x=2")`.
//!
//! (b) `length_hole_typechecks_to_string_no_mix_error` — `"a{t}b"` with
//!     `t : Length = 5mm` must resolve to `Type::String` with NO
//!     Severity::Error diagnostics (specifically no "incompatible types",
//!     "dimension", or "not yet" messages).
//!
//! (c) `plain_string_fast_path_untouched` — `"plain"` (no holes) must still
//!     compile to a `Literal(Value::String("plain"))` untouched by task γ.
//!
//! ## Step mapping
//!
//! These tests are RED against the step-1 stub (which emits a poison diagnostic
//! containing "string interpolation is not yet supported (task γ)") and GREEN
//! after step-2 replaces the stub with the real fold.
//!
//! User-observable signal:
//!   `cargo test -p reify-compiler --test string_interp_lowering_tests`

use reify_core::{Severity, Type};
use reify_ir::{BinOp, CompiledExprKind, Value, ValueMap};
use reify_expr::{eval_expr, EvalContext};
use reify_test_support::{compile_source, get_let_expr};

/// `"x={1+1}"` must lower to a render-then-concat fold: a `BinOp::Add` whose
/// right operand is a `FunctionCall` to `std::__interp_render`, the result
/// type must be `Type::String`, there must be no "not yet" diagnostic, and
/// hermetic eval must produce `Value::String("x=2")`.
///
/// RED against the step-1 stub (stub emits "not yet" poison, returns
/// `Literal(Value::Undef, Type::Error)`).
/// GREEN after step-2 real lowering.
#[test]
fn arith_hole_folds_and_evals_to_string() {
    let source = r#"
structure S {
    let label = "x={1+1}"
}
"#;
    let module = compile_source(source);

    // No "not yet" placeholder diagnostic — stub must be replaced.
    let has_not_yet = module
        .diagnostics
        .iter()
        .any(|d| d.message.contains("not yet"));
    assert!(
        !has_not_yet,
        "expected no 'not yet' placeholder diagnostic for interpolated string (stub not replaced?), got: {:?}",
        module.diagnostics,
    );

    let expr = get_let_expr(&module, "label");

    // Result type must be Type::String.
    assert_eq!(
        expr.result_type,
        Type::String,
        "expected result_type Type::String for interpolated string, got {:?}",
        expr.result_type,
    );

    // The outer expression must be a BinOp::Add (the concat fold).
    let CompiledExprKind::BinOp { op, left, right } = &expr.kind else {
        panic!(
            "expected CompiledExprKind::BinOp for interpolated string fold, got {:?}",
            expr.kind
        );
    };
    assert_eq!(
        *op,
        BinOp::Add,
        "expected BinOp::Add for concat fold of interpolated string",
    );

    // The left operand must be a bare Literal("x="), confirming that literal
    // StringParts bypass the render wrapper (only Hole parts are wrapped).
    let CompiledExprKind::Literal(left_val) = &left.kind else {
        panic!(
            "expected left operand to be CompiledExprKind::Literal (literal string part \
             must NOT be wrapped in __interp_render), got {:?}",
            left.kind
        );
    };
    assert_eq!(
        left_val,
        &Value::String("x=".into()),
        "expected left operand Literal(Value::String(\"x=\")), got {:?}",
        left_val,
    );

    // The right operand must be a FunctionCall to std::__interp_render.
    // (pins render-then-concat, NOT raw `+` over the raw hole value)
    let CompiledExprKind::FunctionCall { function, .. } = &right.kind else {
        panic!(
            "expected right operand to be CompiledExprKind::FunctionCall (for __interp_render), got {:?}",
            right.kind
        );
    };
    assert_eq!(
        function.qualified_name,
        "std::__interp_render",
        "expected right operand function qualified_name == \"std::__interp_render\", got {:?}",
        function.qualified_name,
    );

    // Hermetic eval: constant-hole case must return Value::String("x=2").
    let result = eval_expr(expr, &EvalContext::simple(&ValueMap::new()));
    assert_eq!(
        result,
        Value::String("x=2".into()),
        "expected eval to produce Value::String(\"x=2\") for \"x={{1+1}}\", got {:?}",
        result,
    );
}

/// `"a{t}b"` with `t : Length = 5mm` must compile to result_type `Type::String`
/// with no Severity::Error diagnostics — specifically no "incompatible types",
/// "dimension mismatch", or "not yet supported" messages.
///
/// Pins: mixing a Length hole into a string interpolation must NOT produce a
/// type error.
///
/// RED against the step-1 stub (stub emits "not yet" Severity::Error).
/// GREEN after step-2 real lowering.
#[test]
fn length_hole_typechecks_to_string_no_mix_error() {
    let source = r#"
structure S {
    param t : Length = 5mm
    let label = "a{t}b"
}
"#;
    let module = compile_source(source);

    let expr = get_let_expr(&module, "label");

    // Result type must be Type::String.
    assert_eq!(
        expr.result_type,
        Type::String,
        "expected result_type Type::String for \"a{{t}}b\" with t:Length, got {:?}",
        expr.result_type,
    );

    // No Severity::Error diagnostics — specifically none about "incompatible types",
    // "dimension", or "not yet supported".
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Severity::Error diagnostics for \"a{{t}}b\" (got: {:?})",
        errors,
    );
}

/// A hole-only string `"{1+1}"` must lower to a bare `FunctionCall` to
/// `std::__interp_render` — NOT a `BinOp::Add` with an empty-string left seed.
///
/// Pins the no-seed-fold invariant from the code comment at expr.rs line ~3688:
/// when there is exactly one part (a single `Hole`), `iter.next()` returns that
/// part as `acc` and the fold body never executes.  The result is `render(1+1)`
/// directly, with no spurious `"" +` prefix.
///
/// GREEN after step-2 real lowering.
#[test]
fn hole_only_lowers_to_render_not_binop_add() {
    let source = r#"
structure S {
    let v = "{1+1}"
}
"#;
    let module = compile_source(source);

    // No "not yet" placeholder diagnostic.
    let has_not_yet = module
        .diagnostics
        .iter()
        .any(|d| d.message.contains("not yet"));
    assert!(
        !has_not_yet,
        "expected no 'not yet' placeholder diagnostic for hole-only interpolated string, got: {:?}",
        module.diagnostics,
    );

    let expr = get_let_expr(&module, "v");

    // Result type must be Type::String.
    assert_eq!(
        expr.result_type,
        Type::String,
        "expected result_type Type::String for hole-only string, got {:?}",
        expr.result_type,
    );

    // The outer expression must be a FunctionCall to std::__interp_render —
    // NOT a BinOp::Add with an empty-string left operand (no-seed fold guarantee).
    let CompiledExprKind::FunctionCall { function, .. } = &expr.kind else {
        panic!(
            "expected CompiledExprKind::FunctionCall (render only, no BinOp) for hole-only \
             string \"{{1+1}}\", got {:?} — did the fold accidentally concat with an empty string?",
            expr.kind
        );
    };
    assert_eq!(
        function.qualified_name,
        "std::__interp_render",
        "expected function qualified_name == \"std::__interp_render\" for hole-only string, got {:?}",
        function.qualified_name,
    );
}

/// `"plain"` (no holes) must still compile to `Literal(Value::String("plain"))`
/// with `result_type == Type::String`. Task γ must not disturb the no-hole fast
/// path (which stays as `ExprKind::StringLiteral`, not `InterpolatedString`).
///
/// GREEN both before and after step-2 — this guards that task γ leaves the
/// StringLiteral path untouched.
#[test]
fn plain_string_fast_path_untouched() {
    let source = r#"
structure S {
    let v = "plain"
}
"#;
    let module = compile_source(source);

    let expr = get_let_expr(&module, "v");

    // Must be a Literal(Value::String("plain")).
    let CompiledExprKind::Literal(val) = &expr.kind else {
        panic!(
            "expected CompiledExprKind::Literal for plain string, got {:?}",
            expr.kind
        );
    };
    assert_eq!(
        val,
        &Value::String("plain".into()),
        "expected Literal(Value::String(\"plain\")) for plain string, got {:?}",
        val,
    );

    // Result type must be Type::String.
    assert_eq!(
        expr.result_type,
        Type::String,
        "expected result_type Type::String for plain string, got {:?}",
        expr.result_type,
    );
}
