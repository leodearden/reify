//! Result recovery combinator evaluation tests — task result-fallback Layer-B
//! γ #4037 (PRD docs/prds/v0_6/result-and-fallback.md §4.3/§8.B).
//!
//! Mirrors `option_recovery_eval_tests.rs`: tests fire the `UserFunctionCall`
//! intercept by name + arity using `CompiledExpr::user_function_call` with
//! `EvalContext::simple` (no function bodies needed — the intercept runs
//! before body evaluation). `Result` has no dedicated `CompiledExpr`
//! constructor the way `Option` does (`option_some`/`option_none`) — its
//! subjects are built directly as `Value::Enum { type_name: "Result",
//! variant: "Ok"/"Err", payload }` literals via `CompiledExpr::literal`.
//!
//! `unwrap_or` / `is_ok` / `is_err` / `or_else` / `ok_or` are the five PURE
//! combinators (dispatched by `option_recovery::is_combinator` +
//! `eval_combinator`, subject-tag-driven, no `EvalContext` needed — the
//! Result arms share the existing `unwrap_or`/`or_else` match statements by
//! subject tag). `map_err` is ctx-aware (must apply its lambda argument) and
//! is covered separately by the step-5/6 tests appended to this file later.

use reify_core::{DimensionVector, Type, ValueCellId};
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

fn expr_string(s: &str) -> CompiledExpr {
    CompiledExpr::literal(Value::String(s.to_string()), Type::String)
}

fn expr_some_5mm() -> CompiledExpr {
    CompiledExpr::option_some(expr_5mm(), Type::Option(Box::new(Type::length())))
}

fn expr_none_length() -> CompiledExpr {
    CompiledExpr::option_none(Type::Option(Box::new(Type::length())))
}

/// Literal Undef with Option<Length> type — represents the undef-of-Option
/// state (used only by the ok_or undef-subject test).
fn expr_undef_option_length() -> CompiledExpr {
    CompiledExpr::literal(Value::Undef, Type::Option(Box::new(Type::length())))
}

fn eval_simple(expr: &CompiledExpr) -> Value {
    eval_expr(expr, &EvalContext::simple(&ValueMap::new()))
}

// ── unwrap_or over a Result subject ──────────────────────────────────────────

/// unwrap_or(Ok{value:5mm}, 0mm) == 5mm
///
/// RED today: `eval_extract_or_default` has no `Value::Enum{type_name:
/// "Result",..}` arm, so the Ok subject falls through the `_ =>
/// Value::Undef` degrade arm.
#[test]
fn unwrap_or_ok_subject_returns_inner() {
    let call = CompiledExpr::user_function_call(
        "unwrap_or".to_string(),
        vec![expr_ok_5mm(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        val_5mm(),
        "unwrap_or(Ok{{value:5mm}}, 0mm) must return the inner value 5mm"
    );
}

/// unwrap_or(Err{error:"x"}, 0mm) == 0mm
///
/// RED today: same as above — the Err subject falls through to Undef.
#[test]
fn unwrap_or_err_subject_returns_default() {
    let call = CompiledExpr::user_function_call(
        "unwrap_or".to_string(),
        vec![expr_err_x(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        val_0mm(),
        "unwrap_or(Err{{error:\"x\"}}, 0mm) must return the default 0mm"
    );
}

/// unwrap_or(undef, 0mm) == Value::Undef (PRD D2 / INV-2 subject passthrough)
///
/// GREEN today (coincidentally): the any-arg-undef shortcircuit in
/// eval_user_function_call fires. Pinned here so the real intercept
/// preserves it.
#[test]
fn unwrap_or_undef_subject_returns_undef() {
    let call = CompiledExpr::user_function_call(
        "unwrap_or".to_string(),
        vec![expr_undef_result(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "unwrap_or(undef, 0mm) must propagate Undef — undef subject passthrough (D2)"
    );
}

// ── is_ok / is_err over a Result subject ─────────────────────────────────────

/// is_ok(Ok{value:5mm}) == Bool(true)
///
/// RED today: `is_ok` is absent from `is_combinator` entirely → falls
/// through to `eval_user_function_call` → function not found (simple ctx) →
/// Undef.
#[test]
fn is_ok_ok_subject_returns_true() {
    let call =
        CompiledExpr::user_function_call("is_ok".to_string(), vec![expr_ok_5mm()], Type::Bool);
    assert_eq!(
        eval_simple(&call),
        Value::Bool(true),
        "is_ok(Ok{{value:5mm}}) must return Bool(true)"
    );
}

/// is_ok(Err{error:"x"}) == Bool(false)
///
/// RED today: same as above.
#[test]
fn is_ok_err_subject_returns_false() {
    let call =
        CompiledExpr::user_function_call("is_ok".to_string(), vec![expr_err_x()], Type::Bool);
    assert_eq!(
        eval_simple(&call),
        Value::Bool(false),
        "is_ok(Err{{error:\"x\"}}) must return Bool(false)"
    );
}

/// is_ok(undef) == Value::Undef
///
/// GREEN today (coincidentally): any-arg-undef shortcircuit fires.
#[test]
fn is_ok_undef_subject_returns_undef() {
    let call = CompiledExpr::user_function_call(
        "is_ok".to_string(),
        vec![expr_undef_result()],
        Type::Bool,
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "is_ok(undef) must return Undef (Kleene three-valued)"
    );
}

/// is_err(Ok{value:5mm}) == Bool(false)
///
/// RED today: `is_err` is absent from `is_combinator` entirely.
#[test]
fn is_err_ok_subject_returns_false() {
    let call =
        CompiledExpr::user_function_call("is_err".to_string(), vec![expr_ok_5mm()], Type::Bool);
    assert_eq!(
        eval_simple(&call),
        Value::Bool(false),
        "is_err(Ok{{value:5mm}}) must return Bool(false)"
    );
}

/// is_err(Err{error:"x"}) == Bool(true)
///
/// RED today: same as above.
#[test]
fn is_err_err_subject_returns_true() {
    let call =
        CompiledExpr::user_function_call("is_err".to_string(), vec![expr_err_x()], Type::Bool);
    assert_eq!(
        eval_simple(&call),
        Value::Bool(true),
        "is_err(Err{{error:\"x\"}}) must return Bool(true)"
    );
}

/// is_err(undef) == Value::Undef
///
/// GREEN today (coincidentally): any-arg-undef shortcircuit fires.
#[test]
fn is_err_undef_subject_returns_undef() {
    let call = CompiledExpr::user_function_call(
        "is_err".to_string(),
        vec![expr_undef_result()],
        Type::Bool,
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "is_err(undef) must return Undef (Kleene three-valued)"
    );
}

// ── or_else over a Result subject ────────────────────────────────────────────

/// or_else(Err{error:"x"}, alt) == alt
///
/// RED today: `eval_or_else` has no `Value::Enum{type_name:"Result",..}` arm,
/// so the Err subject falls through the `_ => Value::Undef` degrade arm.
#[test]
fn or_else_err_subject_returns_alt() {
    let call = CompiledExpr::user_function_call(
        "or_else".to_string(),
        vec![expr_err_x(), expr_ok_5mm()],
        result_len_str_type(),
    );
    assert_eq!(
        eval_simple(&call),
        val_ok(val_5mm()),
        "or_else(Err{{error:\"x\"}}, alt) must return alt"
    );
}

/// or_else(Ok{value:5mm}, alt) == Ok{value:5mm} (subject unchanged, alt unused)
///
/// RED today: same as above.
#[test]
fn or_else_ok_subject_returns_subject_unchanged() {
    let call = CompiledExpr::user_function_call(
        "or_else".to_string(),
        vec![expr_ok_5mm(), expr_err_x()],
        result_len_str_type(),
    );
    assert_eq!(
        eval_simple(&call),
        val_ok(val_5mm()),
        "or_else(Ok{{value:5mm}}, alt) must return the subject unchanged"
    );
}

/// or_else(undef, alt) == Value::Undef (INV-2 subject passthrough)
///
/// GREEN today (coincidentally): any-arg-undef shortcircuit fires.
#[test]
fn or_else_undef_subject_returns_undef() {
    let call = CompiledExpr::user_function_call(
        "or_else".to_string(),
        vec![expr_undef_result(), expr_err_x()],
        result_len_str_type(),
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "or_else(undef, alt) must propagate Undef"
    );
}

// ── ok_or: the Option→Result bridge ──────────────────────────────────────────

/// ok_or(some(5mm), "e") == Ok{value:5mm}
///
/// RED today: `ok_or` is absent from `is_combinator` entirely.
#[test]
fn ok_or_some_subject_returns_ok() {
    let call = CompiledExpr::user_function_call(
        "ok_or".to_string(),
        vec![expr_some_5mm(), expr_string("e")],
        result_len_str_type(),
    );
    assert_eq!(
        eval_simple(&call),
        val_ok(val_5mm()),
        "ok_or(some(5mm), \"e\") must return Ok{{value:5mm}}"
    );
}

/// ok_or(none, "e") == Err{error:"e"}
///
/// RED today: same as above.
#[test]
fn ok_or_none_subject_returns_err() {
    let call = CompiledExpr::user_function_call(
        "ok_or".to_string(),
        vec![expr_none_length(), expr_string("e")],
        result_len_str_type(),
    );
    assert_eq!(
        eval_simple(&call),
        val_err("e"),
        "ok_or(none, \"e\") must return Err{{error:\"e\"}}"
    );
}

/// ok_or(undef, "e") == Value::Undef
///
/// GREEN today (coincidentally): any-arg-undef shortcircuit fires.
#[test]
fn ok_or_undef_subject_returns_undef() {
    let call = CompiledExpr::user_function_call(
        "ok_or".to_string(),
        vec![expr_undef_option_length(), expr_string("e")],
        result_len_str_type(),
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "ok_or(undef, \"e\") must propagate Undef"
    );
}

// ── REGRESSION GUARD: Option subject still resolves correctly ───────────────

/// unwrap_or(some(5mm), 0mm) == 5mm — the pre-existing Option overload must
/// keep working now that `unwrap_or` also dispatches
/// `Value::Enum{"Result",..}` subjects.
#[test]
fn unwrap_or_option_subject_regression_guard() {
    let call = CompiledExpr::user_function_call(
        "unwrap_or".to_string(),
        vec![expr_some_5mm(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        val_5mm(),
        "unwrap_or(some(5mm), 0mm) must still return 5mm — Option-subject regression guard"
    );
}

// ── sync-drift check ─────────────────────────────────────────────────────────

/// Every Result-subject route declared in `result.ri` (`unwrap_or`, `is_ok`,
/// `is_err`, `or_else`, `ok_or`) is recognised by `is_combinator` and routes
/// through `eval_combinator` rather than falling through to the placeholder
/// `.ri` body.
///
/// `map_err` is intentionally excluded here — it is ctx-aware (step 5/6) and
/// is never in `is_combinator` (mirroring `map_or`'s exclusion in the Option
/// sync-drift check).
#[test]
fn sync_drift_check_result_combinators_recognized() {
    // unwrap_or(Ok{value:5mm}, 0mm) -> 5mm (inner, not dflt)
    {
        let call = CompiledExpr::user_function_call(
            "unwrap_or".to_string(),
            vec![expr_ok_5mm(), expr_0mm()],
            Type::length(),
        );
        assert_eq!(
            eval_simple(&call),
            val_5mm(),
            "unwrap_or(Ok{{..}}, dflt) must return the inner value — gate out of sync with result.ri"
        );
    }

    // is_ok(Ok{value:5mm}) -> true
    {
        let call =
            CompiledExpr::user_function_call("is_ok".to_string(), vec![expr_ok_5mm()], Type::Bool);
        assert_eq!(
            eval_simple(&call),
            Value::Bool(true),
            "is_ok(Ok{{..}}) must return true — gate out of sync with result.ri"
        );
    }

    // is_err(Err{error:"x"}) -> true
    {
        let call =
            CompiledExpr::user_function_call("is_err".to_string(), vec![expr_err_x()], Type::Bool);
        assert_eq!(
            eval_simple(&call),
            Value::Bool(true),
            "is_err(Err{{..}}) must return true — gate out of sync with result.ri"
        );
    }

    // or_else(Err{error:"x"}, Ok{value:5mm}) -> Ok{value:5mm} (alt)
    {
        let call = CompiledExpr::user_function_call(
            "or_else".to_string(),
            vec![expr_err_x(), expr_ok_5mm()],
            result_len_str_type(),
        );
        assert_eq!(
            eval_simple(&call),
            val_ok(val_5mm()),
            "or_else(Err{{..}}, alt) must return alt — gate out of sync with result.ri"
        );
    }

    // ok_or(some(5mm), "e") -> Ok{value:5mm}
    {
        let call = CompiledExpr::user_function_call(
            "ok_or".to_string(),
            vec![expr_some_5mm(), expr_string("e")],
            result_len_str_type(),
        );
        assert_eq!(
            eval_simple(&call),
            val_ok(val_5mm()),
            "ok_or(some(5mm), \"e\") must return Ok{{value:5mm}} — gate out of sync with result.ri"
        );
    }
}

// ── map_err (ctx-aware arrow-type intercept, steps 5-6) ──────────────────────
//
// map_err(r, f): subject=Ok{value:x} -> Ok{value:x} unchanged (f NOT applied);
// subject=Err{error:e} -> Err{error:f(e)} (f APPLIED to the error payload);
// subject=undef -> Undef (Kleene INV-2).
//
// Unlike the five pure combinators above (eval_combinator / is_combinator),
// map_err must APPLY its function argument `f` and therefore needs the
// EvalContext (for apply_lambda) — mirrors map_or (task 4595) exactly. It is
// handled by a dedicated ctx-aware branch in reify-expr/src/lib.rs's
// UserFunctionCall arm, NOT by is_combinator (which stays pure, INV-1).
//
// RED today: no map_err intercept exists, so the call falls through to
// eval_user_function_call. `EvalContext::simple` has no functions registered,
// so `find_matching_compiled_function` returns None and the call degrades to
// Value::Undef regardless of subject — matching the Err/undef expectation by
// coincidence but NOT the Ok expectation (RED signal) or the actually-mapped
// Err payload (RED signal).

/// Build the lambda CompiledExpr `|e: String| true` (String -> Bool), no
/// captures. A constant-returning lambda is enough to prove the intercept
/// applies `f` at all: the payload's Value tag changes from `String` to
/// `Bool` only if `f` actually ran. Mirrors the `|e:String| true` lambda used
/// by the step-1 compiler resolution test (d), which proves F resolves to
/// Bool independent of T, E.
fn expr_lambda_const_true_from_string() -> CompiledExpr {
    let e_id = ValueCellId::new("$lambda_map_err.S", "e");
    let body = CompiledExpr::literal(Value::Bool(true), Type::Bool);
    CompiledExpr::lambda(
        vec![("e".to_string(), None)],
        vec![e_id],
        body,
        vec![],
        Type::Function {
            params: vec![Type::String],
            return_type: Box::new(Type::Bool),
        },
    )
}

/// `Result<Length, Bool>` — the type produced by mapping `Result<Length,
/// String>` through `map_err`'s `(String) -> Bool` lambda.
fn result_len_bool_type() -> Type {
    Type::Applied {
        name: "Result".to_string(),
        args: vec![Type::length(), Type::Bool],
    }
}

fn val_err_bool(b: bool) -> Value {
    Value::Enum {
        type_name: "Result".to_string(),
        variant: "Err".to_string(),
        payload: vec![("error".to_string(), Value::Bool(b))],
    }
}

/// map_err(Err{error:"x"}, f) == Err{error: f("x")} == Err{error: true}
///
/// The discriminating case: `f` must be APPLIED to the error payload.
///
/// RED today: falls through to eval_user_function_call -> function not found
/// (simple ctx) -> Undef, not Err{error:true}.
#[test]
fn map_err_err_subject_applies_lambda_to_error_payload() {
    let call = CompiledExpr::user_function_call(
        "map_err".to_string(),
        vec![expr_err_x(), expr_lambda_const_true_from_string()],
        result_len_bool_type(),
    );
    assert_eq!(
        eval_simple(&call),
        val_err_bool(true),
        "map_err(Err{{error:\"x\"}}, f) must apply f to the error payload -> Err{{error:true}}"
    );
}

/// map_err(Ok{value:5mm}, f) == Ok{value:5mm} unchanged (f NOT applied)
///
/// RED today: falls through to eval_user_function_call -> function not found
/// (simple ctx) -> Undef, not the Ok subject unchanged.
#[test]
fn map_err_ok_subject_returns_subject_unchanged() {
    let call = CompiledExpr::user_function_call(
        "map_err".to_string(),
        vec![expr_ok_5mm(), expr_lambda_const_true_from_string()],
        result_len_bool_type(),
    );
    assert_eq!(
        eval_simple(&call),
        val_ok(val_5mm()),
        "map_err(Ok{{value:5mm}}, f) must return the Ok subject unchanged — f NOT applied"
    );
}

/// map_err(undef, f) == Value::Undef (INV-2 subject passthrough)
///
/// GREEN today (coincidentally): eval_user_function_call's own Undef handling
/// degrades to Undef when the function is not found, same as the Err/Ok
/// cases above — but here it happens to match the correct contract.
#[test]
fn map_err_undef_subject_returns_undef() {
    let call = CompiledExpr::user_function_call(
        "map_err".to_string(),
        vec![expr_undef_result(), expr_lambda_const_true_from_string()],
        result_len_bool_type(),
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "map_err(undef, f) must propagate Undef — undef subject passthrough (INV-2)"
    );
}
