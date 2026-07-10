//! Eval regression/sync-drift guard for `fallback` over a `Result<T, E>`
//! subject — task result-fallback Layer-B δ #4038 (PRD
//! docs/prds/v0_6/result-and-fallback.md §4.3/§5 D6/§8.B task B-δ).
//!
//! Mirrors `result_combinator_eval_tests.rs`'s helpers and structure. Unlike
//! that file's `unwrap_or`/`is_ok`/`is_err`/`or_else`/`ok_or` tests (task γ
//! #4037, written RED against a genuinely missing eval arm), `fallback`'s
//! eval-side dispatch was ALREADY live before this task: it shares the
//! arity-2 extract-or-default arm with `unwrap_or`/`or_default` in
//! `option_recovery::is_combinator`/`eval_combinator`, and that arm's helper
//! `eval_extract_or_default` already dispatches
//! `Value::Enum{type_name:"Result",variant:"Ok"/"Err"}` (added by task γ
//! #4037 alongside `unwrap_or`). This file is a deliberate REGRESSION LOCK
//! closing the coverage gap left by the 4037 sync-drift check (which omits
//! `fallback`), not a RED-first test: it is expected GREEN as soon as it is
//! written, on the reused arm.
//!
//! The discriminating signal is the Ok-subject case: `eval_extract_or_default`
//! returns the *unboxed inner Ok value*, never the supplied default — so a
//! passing `fallback(Ok{value:5mm}, 0mm) == 5mm` (not `0mm`) proves
//! `is_combinator("fallback", 2)` + the Result arm of
//! `eval_extract_or_default` are both live. If `"fallback"` were ever
//! dropped from `is_combinator`, the call would fall through to the
//! placeholder `.ri` body `{ dflt }`, which always returns the default
//! (ignoring `Ok`) — silently wrong, and this test would catch it.

use reify_core::{DimensionVector, Type};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{CompiledExpr, Value, ValueMap};

// ── helpers (mirrors result_combinator_eval_tests.rs) ────────────────────────

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

fn expr_0mm() -> CompiledExpr {
    CompiledExpr::literal(val_0mm(), Type::length())
}

/// `Result<Length, String>` — the type used throughout this file's fixtures.
fn result_len_str_type() -> Type {
    Type::Applied {
        name: "Result".to_string(),
        args: vec![Type::length(), Type::String],
    }
}

fn val_ok(inner: Value) -> Value {
    Value::Enum {
        type_name: "Result".to_string(),
        variant: "Ok".to_string(),
        payload: vec![("value".to_string(), inner)],
    }
}

fn val_err(msg: &str) -> Value {
    Value::Enum {
        type_name: "Result".to_string(),
        variant: "Err".to_string(),
        payload: vec![("error".to_string(), Value::String(msg.to_string()))],
    }
}

fn expr_ok_5mm() -> CompiledExpr {
    CompiledExpr::literal(val_ok(val_5mm()), result_len_str_type())
}

fn expr_err_x() -> CompiledExpr {
    CompiledExpr::literal(val_err("x"), result_len_str_type())
}

/// Literal Undef with `Result<Length, String>` type — represents the
/// undef-of-Result state.
fn expr_undef_result() -> CompiledExpr {
    CompiledExpr::literal(Value::Undef, result_len_str_type())
}

fn expr_some_5mm() -> CompiledExpr {
    CompiledExpr::option_some(
        CompiledExpr::literal(val_5mm(), Type::length()),
        Type::Option(Box::new(Type::length())),
    )
}

fn eval_simple(expr: &CompiledExpr) -> Value {
    eval_expr(expr, &EvalContext::simple(&ValueMap::new()))
}

// ── fallback over a Result subject ───────────────────────────────────────────

/// [CORE SIGNAL / recognition] `fallback(Ok{value:5mm}, 0mm) == 5mm` — the
/// unboxed inner value, NOT the default. Proves `is_combinator("fallback",
/// 2)` + `eval_extract_or_default`'s Result arm are live (the placeholder
/// `.ri` body would instead always return `dflt` = `0mm`).
#[test]
fn fallback_ok_subject_returns_inner() {
    let call = CompiledExpr::user_function_call(
        "fallback".to_string(),
        vec![expr_ok_5mm(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        val_5mm(),
        "fallback(Ok{{value:5mm}}, 0mm) must return the inner value 5mm — proves the call \
         routes through eval_combinator/eval_extract_or_default, not the placeholder .ri body"
    );
}

/// `fallback(Err{error:"x"}, 0mm) == 0mm` — recover to the default on the
/// `Err` tag (PRD D6).
#[test]
fn fallback_err_subject_returns_default() {
    let call = CompiledExpr::user_function_call(
        "fallback".to_string(),
        vec![expr_err_x(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        val_0mm(),
        "fallback(Err{{error:\"x\"}}, 0mm) must return the default 0mm"
    );
}

/// `fallback(undef, 0mm) == Value::Undef` — undef-subject passthrough
/// (INV-2 / Kleene three-valued propagation).
#[test]
fn fallback_undef_subject_returns_undef() {
    let call = CompiledExpr::user_function_call(
        "fallback".to_string(),
        vec![expr_undef_result(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "fallback(undef, 0mm) must propagate Undef — undef subject passthrough (INV-2)"
    );
}

// ── REGRESSION GUARD: Option subject still evaluates correctly ─────────────

/// [REGRESSION GUARD] `fallback(some(5mm), 0mm) == 5mm` — the pre-existing
/// Option-subject route must keep working now that `fallback` also
/// dispatches `Value::Enum{"Result",..}` subjects.
#[test]
fn fallback_option_subject_regression_guard() {
    let call = CompiledExpr::user_function_call(
        "fallback".to_string(),
        vec![expr_some_5mm(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        val_5mm(),
        "fallback(some(5mm), 0mm) must still return 5mm — Option-subject regression guard"
    );
}
