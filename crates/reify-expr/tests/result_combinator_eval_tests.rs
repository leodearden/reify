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
//!
//! The `e2e_*_with_stdlib` section at the end of this file instead compiles the
//! real stdlib and evaluates under a prelude-backed function table built by
//! `reify_test_support::prelude_backed_functions` — tasks 5410 and 5593, PRD
//! docs/prds/v0_6/placeholder-type-eradication-ratchet.md §8 task ζ / BT10.
//! What those tests do and do not guard is explained ONCE, in the CANONICAL
//! MECHANISM NOTE above `e2e_or_default_some_with_stdlib` in
//! `option_recovery_eval_tests.rs`.

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

// The `e2e_*_with_stdlib` section at the end of this file locates a compiled
// cell's `default_expr` with `reify_test_support::get_let_expr`, the shared
// helper — no private copy lives here.

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

// ── task 5410 (PRD ζ / BT10): stdlib-compiled intercept-fires drift guards ───
//
// MECHANISM: see the CANONICAL MECHANISM NOTE above
// `e2e_or_default_some_with_stdlib` in `option_recovery_eval_tests.rs` — the
// single copy for all three combinator eval-test files. In short: these guard
// that THE INTERCEPT BEATS THE PLACEHOLDER, measured. They evaluate under
// `reify_test_support::prelude_backed_functions(&module)` (task 5593), so the
// stdlib `.ri` bodies are registered and genuinely compete.
//
// MEASURED (task 5593, three intercept gates disabled): all seven guards below
// fail with the `.ri` placeholder's value, never `Undef` — `0mm` from
// `{ dflt }` (unwrap_or, and see the overload note below), `true`/`false` from
// `{ true }`/`{ false }` (is_ok, is_err), the `Err` subject from `{ r }`
// (or_else, map_err), and a naked `String("e")` from `{ err }` (both ok_or
// guards). Per-guard table: prelude_backed_harness_tests.rs.
//
// OVERLOAD NOTE, measured and benign: these fixtures' `arg[0].result_type` is
// `Enum("Result")`, which can never exact-equal
// `Applied{name:"Result", args:[T,E]}`, so pass 1 of
// `find_matching_compiled_function` misses BOTH candidates and pass 2's
// wildcard takes the first in table order — the `Option<T>` overload, since
// `std.option_recovery` loads before `std.result`. Observable value is
// unaffected because each overload pair returns the same positional argument
// (`{ dflt }` / `{ r }` vs `{ dflt }` / `{ o }`). Pre-existing reify-expr
// resolution behaviour, out of scope here; recorded so it is not relied on
// silently.
//
// Deliberate REGRESSION LOCKS, not RED-first tests — the same framing
// `result_fallback_eval_tests.rs` already documents for itself.

/// End-to-end: `unwrap_or(Ok { value: 5mm }, 0mm)` compiled with the real
/// stdlib must evaluate to 5mm — the unboxed inner `Ok` payload.
///
/// MEASURED under intercept removal: fails with `left: Undef` (see the section
/// banner above).
///
/// FIXTURE RATIONALE — `result.ri` ships the typecheck-only
/// `pub fn unwrap_or<T, E>(r: Result<T, E>, dflt: T) -> T { dflt }`, so under
/// #5593's prelude-backed harness the `Ok` subject is what makes this
/// discriminate: an `Err` subject makes `0mm` the correct answer, agreeing with
/// the placeholder's `dflt`.
#[test]
fn e2e_result_unwrap_or_ok_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        "structure S { let v = unwrap_or(Ok { value: 5mm }, 0mm) }",
    );
    let expr = reify_test_support::get_let_expr(&module, "v");
    let values = ValueMap::new();
    let functions = reify_test_support::prelude_backed_functions(&module);
    let ctx = EvalContext::new(&values, &functions);
    assert_eq!(
        eval_expr(expr, &ctx),
        val_5mm(),
        "e2e: unwrap_or(Ok{{value:5mm}}, 0mm) compiled via stdlib must evaluate to 5mm — \
         if the intercept stops firing this falls through to eval_user_function_call \
         and yields Undef"
    );
}

/// End-to-end: `or_else(Err { error: "e" }, Ok { value: 7mm })` compiled with
/// the real stdlib must evaluate to the ALTERNATIVE, `Ok{value:7mm}`.
///
/// MEASURED under intercept removal: fails with `left: Undef` (see the section
/// banner above).
///
/// FIXTURE RATIONALE — `result.ri` ships the typecheck-only
/// `pub fn or_else<T, E>(r: Result<T, E>, alt: Result<T, E>) -> Result<T, E> { r }`
/// (returns the SUBJECT), so under #5593's prelude-backed harness the `Err`
/// subject is MANDATORY. The Ok-subject form
/// `or_else(Ok{value:5mm}, Ok{value:7mm})` compiles and returns `Ok{value:5mm}`,
/// but the placeholder returns that same subject, so the two would agree. Only
/// an `Err` subject makes them diverge.
#[test]
fn e2e_result_or_else_err_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        r#"structure S { let v = or_else(Err { error: "e" }, Ok { value: 7mm }) }"#,
    );
    let expr = reify_test_support::get_let_expr(&module, "v");
    let values = ValueMap::new();
    let functions = reify_test_support::prelude_backed_functions(&module);
    let ctx = EvalContext::new(&values, &functions);
    let val_7mm = Value::Scalar {
        si_value: 0.007,
        dimension: DimensionVector::LENGTH,
    };
    assert_eq!(
        eval_expr(expr, &ctx),
        val_ok(val_7mm),
        "e2e: or_else(Err{{error:\"e\"}}, Ok{{value:7mm}}) compiled via stdlib must evaluate to \
         the alternative Ok{{value:7mm}} — if the intercept stops firing this falls through \
         to eval_user_function_call and yields Undef"
    );
}

/// End-to-end: `is_ok(Err { error: "e" })` compiled with the real stdlib must
/// evaluate to `false`.
///
/// MEASURED under intercept removal: fails with `left: Undef` (see the section
/// banner above).
///
/// FIXTURE RATIONALE — `result.ri` ships the typecheck-only
/// `pub fn is_ok<T, E>(r: Result<T, E>) -> Bool { true }`, so under #5593's
/// prelude-backed harness the `Err` subject is what makes this discriminate: an
/// `Ok` subject coincides with the placeholder's hardcoded `true`.
#[test]
fn e2e_result_is_ok_err_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        r#"structure S { let v = is_ok(Err { error: "e" }) }"#,
    );
    let expr = reify_test_support::get_let_expr(&module, "v");
    let values = ValueMap::new();
    let functions = reify_test_support::prelude_backed_functions(&module);
    let ctx = EvalContext::new(&values, &functions);
    assert_eq!(
        eval_expr(expr, &ctx),
        Value::Bool(false),
        "e2e: is_ok(Err{{error:\"e\"}}) compiled via stdlib must evaluate to false — \
         if the intercept stops firing this falls through to eval_user_function_call \
         and yields Undef"
    );
}

/// End-to-end: `is_err(Err { error: "e" })` compiled with the real stdlib must
/// evaluate to `true`.
///
/// MEASURED under intercept removal: fails with `left: Undef` (see the section
/// banner above).
///
/// FIXTURE RATIONALE — `result.ri` ships the typecheck-only
/// `pub fn is_err<T, E>(r: Result<T, E>) -> Bool { false }`. Shares the
/// `Err { error: "e" }` fixture with `e2e_result_is_ok_err_with_stdlib` above,
/// because under #5593's prelude-backed harness one subject discriminates BOTH
/// predicates: the two placeholder constants are the exact inverses of the
/// correct answers for an `Err`, whereas an `Ok` subject would coincide with
/// both.
#[test]
fn e2e_result_is_err_err_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        r#"structure S { let v = is_err(Err { error: "e" }) }"#,
    );
    let expr = reify_test_support::get_let_expr(&module, "v");
    let values = ValueMap::new();
    let functions = reify_test_support::prelude_backed_functions(&module);
    let ctx = EvalContext::new(&values, &functions);
    assert_eq!(
        eval_expr(expr, &ctx),
        Value::Bool(true),
        "e2e: is_err(Err{{error:\"e\"}}) compiled via stdlib must evaluate to true — \
         if the intercept stops firing this falls through to eval_user_function_call \
         and yields Undef"
    );
}

/// End-to-end: `ok_or(some(5mm), "e")` compiled with the real stdlib must
/// evaluate to `Ok{value:5mm}` — the Option→Result bridge's some-path.
///
/// MEASURED under intercept removal: fails with `left: Undef` (see the section
/// banner above). `result.ri` ships the typecheck-only
/// `pub fn ok_or<T, E>(o: Option<T>, err: E) -> Result<T, E> { err }`, which
/// returns the bare `Value::String("e")` and never constructs a `Result` enum
/// at all — that is what #5593's prelude-backed harness would observe here.
///
/// Beyond guarding the intercept, the some-path pins the Ok-WRAPPING direction
/// of the bridge: the intercept must not merely unbox `o`, it must re-wrap the
/// payload as `Result::Ok{value:..}`.
#[test]
fn e2e_ok_or_some_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        r#"structure S { let v = ok_or(some(5mm), "e") }"#,
    );
    let expr = reify_test_support::get_let_expr(&module, "v");
    let values = ValueMap::new();
    let functions = reify_test_support::prelude_backed_functions(&module);
    let ctx = EvalContext::new(&values, &functions);
    assert_eq!(
        eval_expr(expr, &ctx),
        val_ok(val_5mm()),
        "e2e: ok_or(some(5mm), \"e\") compiled via stdlib must evaluate to Ok{{value:5mm}} — \
         if the intercept stops firing this falls through to eval_user_function_call \
         and yields Undef"
    );
}

/// End-to-end: `ok_or(none, "e")` compiled with the real stdlib must evaluate
/// to `Err{error:"e"}` — the Option→Result bridge's none-path.
///
/// MEASURED under intercept removal: fails with `left: Undef` — same mechanism
/// as `e2e_ok_or_some_with_stdlib` above (see the section banner).
///
/// FIXTURE RATIONALE — under #5593's prelude-backed harness the none-path is
/// NOT redundant even though its error payload is "the same string" the
/// placeholder returns, because the two differ in SHAPE —
/// `Enum{Result::Err{error:String("e")}}` from the intercept vs a naked
/// `String("e")` from `{ err }`.
///
/// Note: a bare `none` here infers a DIMENSIONLESS `T`, which is harmless
/// because the assertion inspects only the `Err` payload.
#[test]
fn e2e_ok_or_none_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        r#"structure S { let v = ok_or(none, "e") }"#,
    );
    let expr = reify_test_support::get_let_expr(&module, "v");
    let values = ValueMap::new();
    let functions = reify_test_support::prelude_backed_functions(&module);
    let ctx = EvalContext::new(&values, &functions);
    assert_eq!(
        eval_expr(expr, &ctx),
        val_err("e"),
        "e2e: ok_or(none, \"e\") compiled via stdlib must evaluate to Err{{error:\"e\"}} — \
         if the intercept stops firing this falls through to eval_user_function_call \
         and yields Undef"
    );
}

/// End-to-end: `map_err(Err { error: 3mm }, |e: Length| e * 2)` compiled with
/// the real stdlib must evaluate to `Err{error:6mm}` — the lambda APPLIED to
/// the error payload.
///
/// This is the first stdlib-compiled guard for the CTX-AWARE `map_err/2`
/// intercept, which lives in its own branch of reify-expr's `UserFunctionCall`
/// arm rather than in `option_recovery::is_combinator` (that gate stays pure,
/// INV-1, and cannot apply a lambda).
///
/// MEASURED under intercept removal: fails with `left: Undef`. This one covers
/// the THIRD gate — the bare `map_err`/2 branch (see the section banner above).
/// `result.ri` ships the typecheck-only
/// `pub fn map_err<T, E, F>(r: Result<T, E>, f: (E) -> F) -> Result<T, F> { r }`,
/// which returns the undoubled `Err{error:3mm}` with `f` never applied — that
/// is what #5593's prelude-backed harness would observe here.
///
/// Unique value of this guard, and it holds IN THIS HARNESS: it is the only
/// Result-side test in task 5410 that proves the compiled LAMBDA argument
/// reaches `apply_lambda` end-to-end. Every other Result guard would still pass
/// if the intercept merely matched name+arity and ignored its function
/// argument; here the 3mm→6mm doubling can only happen if the real compiled
/// arrow-typed arg is actually invoked. (`e2e_map_or_some_with_stdlib` in
/// option_recovery_eval_tests.rs is the Option-side counterpart.)
///
/// Asserted on the VALUE, not on `result_type`: for an inline `Err`
/// construction the compiled `result_type` is
/// `Applied{"Result", [TypeParam("T"), Scalar[m]]}` — `T` stays erased because
/// nothing constrains it. That erasure is correct for the resolver and is
/// already covered by the compiler-side resolution tests; re-asserting it here
/// would be lockstep duplication and brittle to unrelated inference work.
#[test]
fn e2e_map_err_err_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        "structure S { let v = map_err(Err { error: 3mm }, |e: Length| e * 2) }",
    );
    let expr = reify_test_support::get_let_expr(&module, "v");
    let values = ValueMap::new();
    let functions = reify_test_support::prelude_backed_functions(&module);
    let ctx = EvalContext::new(&values, &functions);
    let val_6mm = Value::Scalar {
        si_value: 0.006,
        dimension: DimensionVector::LENGTH,
    };
    assert_eq!(
        eval_expr(expr, &ctx),
        Value::Enum {
            type_name: "Result".to_string(),
            variant: "Err".to_string(),
            payload: vec![("error".to_string(), val_6mm)],
        },
        "e2e: map_err(Err{{error:3mm}}, |e| e * 2) compiled via stdlib must evaluate to \
         Err{{error:6mm}} — if the map_err/2 gate stops firing this falls through to \
         eval_user_function_call and yields Undef"
    );
}
