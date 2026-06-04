//! Integration tests for the `__interp_render` builtin.
//!
//! Drives `eval_expr` over hand-built `FunctionCall("__interp_render", [literal])`
//! nodes to pin the render rules from the string-interpolation PRD §3:
//!
//! - Scalar (engineering unit): `format_display_pair` joined `"{value} {unit}"`
//!   when unit is non-empty (5 mm — NOT Display's "0.005 m")
//! - Bare string: `format_display` unquoted (x — NOT Display's `"\"x\""`)
//! - Every other non-Undef variant: `format_display` verbatim
//! - Undef: literal `"undef"` (NOT `format_display`'s `"undefined"`) and
//!   NOT `Value::Undef` (the undef hole must NOT poison the interpolated string)
//!
//! Using the public `eval_expr` path is essential — it is the only path that
//! pins the Undef-not-short-circuited wiring that task γ depends on.

#![allow(clippy::mutable_key_type)]

use reify_core::{ContentHash, DimensionVector, Type};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{CompiledExpr, CompiledExprKind, ResolvedFunction, Value, ValueMap};

// ── Helper ────────────────────────────────────────────────────────────────────

/// Build a `__interp_render(value)` call expression and evaluate it,
/// mirroring the `make_function_call` helper in `worst_case_dispatch_tests.rs`.
fn render(value: Value) -> Value {
    let name = "__interp_render";
    let hash = ContentHash::of(name.as_bytes());
    let call = CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: name.to_string(),
                qualified_name: format!("std::{name}"),
            },
            args: vec![CompiledExpr::literal(value, Type::Real)],
        },
        result_type: Type::String,
        content_hash: hash,
    };
    let values = ValueMap::new();
    eval_expr(&call, &EvalContext::simple(&values))
}

// ── Cycle 1: non-Undef render shape ──────────────────────────────────────────

/// Engineering-unit scalar: format_display_pair joined with a space.
/// 5 mm (si_value = 0.005, dimension = LENGTH) must render as "5 mm",
/// NOT Display's "0.005 m".
#[test]
fn render_scalar_length_5mm_returns_engineering_unit_string() {
    let scalar = Value::Scalar {
        si_value: 0.005,
        dimension: DimensionVector::LENGTH,
    };
    assert_eq!(
        render(scalar),
        Value::String("5 mm".to_string()),
        "__interp_render(5mm Scalar) must render as \"5 mm\" via format_display_pair, not Display"
    );
}

/// Int: format_display verbatim.
#[test]
fn render_int_returns_decimal_string() {
    assert_eq!(render(Value::Int(2)), Value::String("2".to_string()));
}

/// Bool: format_display verbatim.
#[test]
fn render_bool_true_returns_string_true() {
    assert_eq!(render(Value::Bool(true)), Value::String("true".to_string()));
}

/// Bare string: format_display returns the raw string contents (unquoted).
/// Must NOT use Display's quoted form `"\"x\""`.
#[test]
fn render_string_returns_unquoted_contents() {
    assert_eq!(
        render(Value::String("x".to_string())),
        Value::String("x".to_string()),
        "__interp_render(String(\"x\")) must return \"x\" unquoted, not Display's \"\\\"x\\\"\""
    );
}

/// Option(Some(Scalar 5mm)): format_display_pair recursion yields "5 mm".
#[test]
fn render_option_some_scalar_5mm_returns_engineering_unit_string() {
    let inner = Value::Scalar {
        si_value: 0.005,
        dimension: DimensionVector::LENGTH,
    };
    let opt = Value::Option(Some(Box::new(inner)));
    assert_eq!(render(opt), Value::String("5 mm".to_string()));
}

/// List([Int(1), Int(2)]): every-other-variant arm → format_display.
#[test]
fn render_list_of_ints_returns_format_display_string() {
    let list = Value::List(vec![Value::Int(1), Value::Int(2)]);
    assert_eq!(render(list), Value::String("[1, 2]".to_string()));
}

// ── Cycle 2: Undef determinacy pin ───────────────────────────────────────────

/// Undef must render as the literal string "undef" (NOT "undefined") and must
/// NOT return Value::Undef.
///
/// This pins BOTH failure modes:
///   (a) the result must be Value::String("undef") — the Undef hole does NOT
///       poison the interpolated string (PRD §6.3 determinacy decision);
///   (b) the text must be "undef" (the language keyword), not "undefined"
///       (what format_display would produce).
#[test]
fn render_undef_returns_string_undef_not_poison() {
    let result = render(Value::Undef);
    assert_eq!(
        result,
        Value::String("undef".to_string()),
        "__interp_render(Undef) must return String(\"undef\"), not Value::Undef or \"undefined\""
    );
}

/// Option(None): not a Scalar, so the pair arm does not apply; falls through
/// to format_display — must not crash and must return a String.
#[test]
fn render_option_none_returns_string_none() {
    let result = render(Value::Option(None));
    // format_display for Option(None) — confirm it yields a String, not Undef.
    assert!(
        matches!(result, Value::String(_)),
        "__interp_render(Option(None)) must return a String, got {result:?}"
    );
}
