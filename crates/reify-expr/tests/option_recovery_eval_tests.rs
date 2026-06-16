//! Option recovery combinator evaluation tests — task β of PRD
//! docs/prds/v0_6/result-and-fallback.md §8 Phase 2.
//!
//! Tests fire the UserFunctionCall intercept by name + arity using
//! `CompiledExpr::user_function_call` with `EvalContext::simple` (no
//! function bodies needed — the intercept runs before body evaluation).
//!
//! Each combinator gets its own section.  RED tests are labelled with the
//! placeholder behaviour that makes them fail today.  End-to-end cases using
//! `compile_source_with_stdlib` appear in steps 1 and 9.

use reify_core::{DimensionVector, Type};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{CompiledExpr, Value, ValueMap};

// ── helpers ───────────────────────────────────────────────────────────────────

fn val_5mm() -> Value {
    Value::Scalar {
        si_value: 0.005,
        dimension: DimensionVector::LENGTH,
    }
}

fn val_0mm() -> Value {
    Value::Scalar {
        si_value: 0.0,
        dimension: DimensionVector::LENGTH,
    }
}

fn expr_5mm() -> CompiledExpr {
    CompiledExpr::literal(val_5mm(), Type::length())
}

fn expr_0mm() -> CompiledExpr {
    CompiledExpr::literal(val_0mm(), Type::length())
}

fn expr_some_5mm() -> CompiledExpr {
    CompiledExpr::option_some(expr_5mm(), Type::Option(Box::new(Type::length())))
}

fn expr_none_length() -> CompiledExpr {
    CompiledExpr::option_none(Type::Option(Box::new(Type::length())))
}

/// Literal Undef with Option<Length> type — represents the undef-of-Option state.
fn expr_undef_option_length() -> CompiledExpr {
    CompiledExpr::literal(Value::Undef, Type::Option(Box::new(Type::length())))
}

/// Literal Undef with Length type — represents an undef default argument.
fn expr_undef_length() -> CompiledExpr {
    CompiledExpr::literal(Value::Undef, Type::length())
}

fn eval_simple(expr: &CompiledExpr) -> Value {
    eval_expr(expr, &EvalContext::simple(&ValueMap::new()))
}

/// Locate the `default_expr` of a named value cell in the first template.
fn cell_expr_stdlib<'a>(
    module: &'a reify_compiler::CompiledModule,
    member: &str,
) -> &'a reify_ir::CompiledExpr {
    let template = &module.templates[0];
    template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("value cell '{member}' not found"))
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("value cell '{member}' has no default_expr"))
}

// ── step-1: unwrap_or ─────────────────────────────────────────────────────────

/// unwrap_or(some(5mm), 0mm) == 5mm
///
/// RED today: EvalContext::simple has no functions → function not found →
/// Undef.  After step-2 impl the intercept returns *inner (5mm).
#[test]
fn unwrap_or_some_returns_inner() {
    let call = CompiledExpr::user_function_call(
        "unwrap_or".to_string(),
        vec![expr_some_5mm(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        val_5mm(),
        "unwrap_or(some(5mm), 0mm) must return the inner value 5mm"
    );
}

/// unwrap_or(none, 0mm) == 0mm
///
/// RED today: EvalContext::simple has no functions → function not found →
/// Undef.  After step-2 impl the intercept returns args[1] (0mm).
#[test]
fn unwrap_or_none_returns_default() {
    let call = CompiledExpr::user_function_call(
        "unwrap_or".to_string(),
        vec![expr_none_length(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        val_0mm(),
        "unwrap_or(none, 0mm) must return the default 0mm"
    );
}

/// unwrap_or(undef, 0mm) == Value::Undef  (INV-2 subject passthrough)
///
/// Recovery is driven by the SUBJECT tag.  When the subject is undef (existence
/// undecided), the combinator must propagate Undef regardless of the default.
/// GREEN today coincidentally: the any-arg-undef shortcircuit in
/// eval_user_function_call fires and returns Undef.  Pinned here to ensure the
/// impl preserves this.
#[test]
fn unwrap_or_undef_subject_returns_undef() {
    let call = CompiledExpr::user_function_call(
        "unwrap_or".to_string(),
        vec![expr_undef_option_length(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "unwrap_or(undef, 0mm) must propagate Undef — undef subject passthrough (INV-2)"
    );
}

/// unwrap_or(some(5mm), undef) == 5mm  (SUBJECT-tag-driven, not strict-all-args-undef)
///
/// CRITICAL: recovery is driven by the SUBJECT tag, not by strict all-args
/// undef.  some(x) yields x regardless of whether the default is undef.
///
/// RED today: the any-arg-undef shortcircuit fires (dflt=undef → shortcircuit)
/// returning Undef instead of 5mm.  After step-2 impl the intercept checks only
/// the subject and returns *inner when it is some(x).
#[test]
fn unwrap_or_some_with_undef_default_returns_inner() {
    let call = CompiledExpr::user_function_call(
        "unwrap_or".to_string(),
        vec![expr_some_5mm(), expr_undef_length()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        val_5mm(),
        "unwrap_or(some(5mm), undef) must return 5mm — some wins, default is unused (SUBJECT-tag-driven)"
    );
}

/// End-to-end: `unwrap_or(some(5mm), 0mm)` compiled with the stdlib must
/// evaluate to 5mm.
///
/// RED today: the placeholder body `{ dflt }` returns 0mm.  After step-2 impl
/// the UserFunctionCall intercept fires before the body and returns 5mm.
#[test]
fn e2e_unwrap_or_some_5mm_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        "structure S { let v = unwrap_or(some(5mm), 0mm) }",
    );
    let expr = cell_expr_stdlib(&module, "v");
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let result = reify_expr::eval_expr(expr, &ctx);
    assert_eq!(
        result,
        val_5mm(),
        "e2e: unwrap_or(some(5mm), 0mm) compiled via stdlib must evaluate to 5mm"
    );
}
