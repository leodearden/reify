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
//!
//! The `e2e_*_with_stdlib` tests instead compile the real stdlib and evaluate
//! under `EvalContext::new(&values, &module.functions)` — task 5410, PRD
//! docs/prds/v0_6/placeholder-type-eradication-ratchet.md §8 task ζ / BT10.
//! What they do and do not guard is explained ONCE, in the CANONICAL MECHANISM
//! NOTE above `e2e_or_default_some_with_stdlib` below; the sibling files
//! `result_combinator_eval_tests.rs` and `result_fallback_eval_tests.rs` point
//! at that note rather than restate it.

use reify_core::{DimensionVector, Type, ValueCellId};
use reify_expr::{EvalContext, eval_expr};
use reify_ir::{BinOp, CompiledExpr, Value, ValueMap};

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

/// 10mm — the result of doubling 5mm, used by the task-5410 `map_or` e2e guard.
fn val_10mm() -> Value {
    Value::Scalar {
        si_value: 0.01,
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

// The `e2e_*_with_stdlib` tests below locate a compiled cell's `default_expr`
// with `reify_test_support::get_let_expr`, the shared helper — this file used to
// carry a private `cell_expr_stdlib` copy of it.

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

// ── step-3: or_default and fallback aliases ───────────────────────────────────
//
// or_default and fallback have identical extract-or-default semantics to
// unwrap_or.  RED today: is_combinator does not yet handle these names so they
// fall through to eval_user_function_call → function not found → Undef.

/// or_default(some(5mm), 0mm) == 5mm
///
/// RED today: or_default not intercepted → Undef.
#[test]
fn or_default_some_returns_inner() {
    let call = CompiledExpr::user_function_call(
        "or_default".to_string(),
        vec![expr_some_5mm(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        val_5mm(),
        "or_default(some(5mm), 0mm) must return the inner value 5mm"
    );
}

/// or_default(none, 0mm) == 0mm
///
/// RED today: or_default not intercepted → Undef.
#[test]
fn or_default_none_returns_default() {
    let call = CompiledExpr::user_function_call(
        "or_default".to_string(),
        vec![expr_none_length(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        val_0mm(),
        "or_default(none, 0mm) must return the default 0mm"
    );
}

/// or_default(undef, 0mm) == Value::Undef  (INV-2)
///
/// GREEN today (coincidentally): any-arg-undef shortcircuit fires.
#[test]
fn or_default_undef_subject_returns_undef() {
    let call = CompiledExpr::user_function_call(
        "or_default".to_string(),
        vec![expr_undef_option_length(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "or_default(undef, 0mm) must propagate Undef"
    );
}

/// fallback(some(5mm), 0mm) == 5mm
///
/// RED today: fallback not intercepted → Undef.
#[test]
fn fallback_some_returns_inner() {
    let call = CompiledExpr::user_function_call(
        "fallback".to_string(),
        vec![expr_some_5mm(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        val_5mm(),
        "fallback(some(5mm), 0mm) must return the inner value 5mm"
    );
}

/// fallback(none, 0mm) == 0mm
///
/// RED today: fallback not intercepted → Undef.
#[test]
fn fallback_none_returns_default() {
    let call = CompiledExpr::user_function_call(
        "fallback".to_string(),
        vec![expr_none_length(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        val_0mm(),
        "fallback(none, 0mm) must return the default 0mm"
    );
}

/// fallback(undef, 0mm) == Value::Undef  (INV-2)
///
/// GREEN today (coincidentally): any-arg-undef shortcircuit fires.
#[test]
fn fallback_undef_subject_returns_undef() {
    let call = CompiledExpr::user_function_call(
        "fallback".to_string(),
        vec![expr_undef_option_length(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "fallback(undef, 0mm) must propagate Undef"
    );
}

// ── task 5410 (PRD ζ / BT10): stdlib-compiled intercept-fires drift guards ───
//
// CANONICAL MECHANISM NOTE — the single copy for all three combinator eval-test
// files. `result_combinator_eval_tests.rs` and `result_fallback_eval_tests.rs`
// point here. PRD: docs/prds/v0_6/placeholder-type-eradication-ratchet.md §3
// item 10, §8 task ζ, BT10.
//
// Every `e2e_*_with_stdlib` test compiles the REAL stdlib via
// `compile_source_with_stdlib` and evaluates against `module.functions`, so the
// compiler-emitted `UserFunctionCall` runs on the same path a real program
// takes.
//
// WHAT THEY GUARD, MEASURED — *not* what the `.ri` placeholder body returns.
// `module.functions` is USER-SOURCE-ONLY: reify-compiler routes prelude/stdlib
// `.ri` functions through `merge_prelude_functions` into `resolution_functions`
// for compile-time overload dispatch, but stores only user-source functions in
// `CompiledModule`. So in THIS harness the stdlib bodies are never registered;
// with the intercept removed each call falls through to reify-expr's
// `eval_user_function_call`, finds no matching entry, and returns
// `Value::Undef`.
//
// MEASURED (task 5410 step-11; the three gates in reify-expr's
// `UserFunctionCall` arm — `option_recovery::is_combinator`, `map_or`/3 and
// `map_err`/2 — locally set to `if false && …`): every `e2e_*_with_stdlib` test
// across the three files fails with `left: Undef`.  Never a placeholder value.
//
// SCOPE, stated honestly: these guard "THE INTERCEPT FIRES", the drift mode
// this harness can observe. They do NOT reproduce the PRD's silent-WRONG-VALUE
// mode (intercept gone → placeholder body silently returns `dflt` / `true` /
// the subject), which needs a prelude-backed function table. Deferred to #5593
// (originally filed as ticket tkt_0RRQDAY187DZW2V1Q5N5P9679F): a prelude-backed
// eval harness via reify-eval's `merge_functions`, or a new public
// reify-compiler prelude accessor.
//
// Consequently the `.ri` placeholder values quoted in the per-test "FIXTURE
// RATIONALE" notes are TRUE ABOUT THE `.ri` SOURCE — they are the PRD linkage
// and the reason each fixture was chosen — but this harness does not observe
// them.
//
// These are deliberate REGRESSION LOCKS, not RED-first tests: the intercept is
// already live, so they are GREEN the moment they are written.

/// End-to-end: `or_default(some(5mm), 0mm)` compiled with the real stdlib must
/// evaluate to 5mm — the unboxed inner value.
///
/// MEASURED under intercept removal: fails with `left: Undef` (see the
/// mechanism note above for why the fallthrough, not the placeholder, is what
/// this harness observes).
///
/// FIXTURE RATIONALE — `option_recovery.ri` ships the typecheck-only
/// `pub fn or_default<T>(o: Option<T>, dflt: T) -> T { dflt }`, so under
/// #5593's prelude-backed harness the `some` subject is what makes this
/// discriminate: a `none` subject makes `0mm` the correct answer, agreeing with
/// the placeholder's `dflt`.
///
/// NOT redundant with `e2e_unwrap_or_some_5mm_with_stdlib`. `or_default`,
/// `unwrap_or` and `fallback` share the `eval_extract_or_default` ARM, but each
/// is a SEPARATE NAME in the `option_recovery::is_combinator` gate. Dropping
/// just `"or_default"` from that gate would leave the `unwrap_or` e2e green
/// while routing every `or_default` call off the intercept — that name-level
/// independence is exactly what this guard covers.
#[test]
fn e2e_or_default_some_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        "structure S { let v = or_default(some(5mm), 0mm) }",
    );
    let expr = reify_test_support::get_let_expr(&module, "v");
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    assert_eq!(
        reify_expr::eval_expr(expr, &ctx),
        val_5mm(),
        "e2e: or_default(some(5mm), 0mm) compiled via stdlib must evaluate to 5mm — \
         if the intercept stops firing this falls through to eval_user_function_call \
         and yields Undef"
    );
}

// ── step-1: end-to-end via compile_source_with_stdlib ────────────────────────

/// End-to-end: `unwrap_or(some(5mm), 0mm)` compiled with the stdlib must
/// evaluate to 5mm.
///
/// Historical note from task β, when this was written RED: "the placeholder
/// body `{ dflt }` returns 0mm.  After step-2 impl the UserFunctionCall
/// intercept fires before the body and returns 5mm."
///
/// TASK 5410 CORRECTION — that mechanism is not what this harness observes.
/// `module.functions` is user-source-only, so the `.ri` placeholder body is
/// never registered here. MEASURED under intercept removal: this test fails
/// with `left: Undef`, from the `eval_user_function_call` fallthrough — not
/// with `0mm` from the placeholder. It is a real drift guard either way; only
/// the stated reason changes. See the section banner above
/// `e2e_or_default_some_with_stdlib`.
#[test]
fn e2e_unwrap_or_some_5mm_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        "structure S { let v = unwrap_or(some(5mm), 0mm) }",
    );
    let expr = reify_test_support::get_let_expr(&module, "v");
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let result = reify_expr::eval_expr(expr, &ctx);
    assert_eq!(
        result,
        val_5mm(),
        "e2e: unwrap_or(some(5mm), 0mm) compiled via stdlib must evaluate to 5mm"
    );
}

// ── step-5: or_else ───────────────────────────────────────────────────────────
//
// or_else(o, alt): subject=some(x)->return whole Value::Option(Some(x))
// unchanged; subject=none->return alt; subject=undef->Undef.
// Result type is Option<Length>.
//
// RED today: or_else not yet in is_combinator → falls through →
// eval_user_function_call → function not found (simple ctx) → Undef.

/// or_else(none, some(3mm)) == Value::Option(Some(3mm))
///
/// RED today: or_else not intercepted → Undef.
#[test]
fn or_else_none_returns_alt() {
    let three_mm = Value::Scalar {
        si_value: 0.003,
        dimension: DimensionVector::LENGTH,
    };
    let expr_some_3mm = CompiledExpr::option_some(
        CompiledExpr::literal(three_mm.clone(), Type::length()),
        Type::Option(Box::new(Type::length())),
    );
    let call = CompiledExpr::user_function_call(
        "or_else".to_string(),
        vec![expr_none_length(), expr_some_3mm],
        Type::Option(Box::new(Type::length())),
    );
    assert_eq!(
        eval_simple(&call),
        Value::Option(Some(Box::new(three_mm))),
        "or_else(none, some(3mm)) must return the alternative some(3mm)"
    );
}

/// or_else(some(5mm), some(3mm)) == Value::Option(Some(5mm))
///
/// Subject is some → return the subject Option unchanged (not the alternative).
///
/// RED today: or_else not intercepted → Undef.
#[test]
fn or_else_some_returns_subject() {
    let expr_some_3mm = CompiledExpr::option_some(
        CompiledExpr::literal(
            Value::Scalar { si_value: 0.003, dimension: DimensionVector::LENGTH },
            Type::length(),
        ),
        Type::Option(Box::new(Type::length())),
    );
    let call = CompiledExpr::user_function_call(
        "or_else".to_string(),
        vec![expr_some_5mm(), expr_some_3mm],
        Type::Option(Box::new(Type::length())),
    );
    assert_eq!(
        eval_simple(&call),
        Value::Option(Some(Box::new(val_5mm()))),
        "or_else(some(5mm), some(3mm)) must return subject some(5mm) unchanged"
    );
}

/// or_else(undef, some(3mm)) == Value::Undef  (INV-2 subject passthrough)
///
/// GREEN today (coincidentally): any-arg-undef shortcircuit fires.
#[test]
fn or_else_undef_subject_returns_undef() {
    let expr_some_3mm = CompiledExpr::option_some(
        CompiledExpr::literal(
            Value::Scalar { si_value: 0.003, dimension: DimensionVector::LENGTH },
            Type::length(),
        ),
        Type::Option(Box::new(Type::length())),
    );
    let call = CompiledExpr::user_function_call(
        "or_else".to_string(),
        vec![expr_undef_option_length(), expr_some_3mm],
        Type::Option(Box::new(Type::length())),
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "or_else(undef, some(3mm)) must propagate Undef (INV-2)"
    );
}

// ── step-7: is_some / is_none presence predicates ─────────────────────────────
//
// Kleene three-valued: some->true/false, none->false/true, undef->Undef.
// Result type is Type::Bool.
//
// RED today: is_some/is_none not yet in is_combinator → falls through →
// eval_user_function_call → function not found (simple ctx) → Undef.

/// is_some(some(5mm)) == Bool(true)
///
/// RED today: is_some not intercepted → Undef.
#[test]
fn is_some_some_returns_true() {
    let call = CompiledExpr::user_function_call(
        "is_some".to_string(),
        vec![expr_some_5mm()],
        Type::Bool,
    );
    assert_eq!(
        eval_simple(&call),
        Value::Bool(true),
        "is_some(some(5mm)) must return Bool(true)"
    );
}

/// is_some(none) == Bool(false)
///
/// RED today: is_some not intercepted → Undef.
#[test]
fn is_some_none_returns_false() {
    let call = CompiledExpr::user_function_call(
        "is_some".to_string(),
        vec![expr_none_length()],
        Type::Bool,
    );
    assert_eq!(
        eval_simple(&call),
        Value::Bool(false),
        "is_some(none) must return Bool(false)"
    );
}

/// is_some(undef) == Value::Undef  (INV-2 Kleene three-valued)
///
/// GREEN today (coincidentally): any-arg-undef shortcircuit fires.
#[test]
fn is_some_undef_returns_undef() {
    let call = CompiledExpr::user_function_call(
        "is_some".to_string(),
        vec![expr_undef_option_length()],
        Type::Bool,
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "is_some(undef) must return Undef (Kleene three-valued, INV-2)"
    );
}

/// is_none(some(5mm)) == Bool(false)
///
/// RED today: is_none not intercepted → Undef.
#[test]
fn is_none_some_returns_false() {
    let call = CompiledExpr::user_function_call(
        "is_none".to_string(),
        vec![expr_some_5mm()],
        Type::Bool,
    );
    assert_eq!(
        eval_simple(&call),
        Value::Bool(false),
        "is_none(some(5mm)) must return Bool(false)"
    );
}

/// is_none(none) == Bool(true)
///
/// RED today: is_none not intercepted → Undef.
#[test]
fn is_none_none_returns_true() {
    let call = CompiledExpr::user_function_call(
        "is_none".to_string(),
        vec![expr_none_length()],
        Type::Bool,
    );
    assert_eq!(
        eval_simple(&call),
        Value::Bool(true),
        "is_none(none) must return Bool(true)"
    );
}

/// is_none(undef) == Value::Undef  (INV-2 Kleene three-valued)
///
/// GREEN today (coincidentally): any-arg-undef shortcircuit fires.
#[test]
fn is_none_undef_returns_undef() {
    let call = CompiledExpr::user_function_call(
        "is_none".to_string(),
        vec![expr_undef_option_length()],
        Type::Bool,
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "is_none(undef) must return Undef (Kleene three-valued, INV-2)"
    );
}

// ── task 5410 (PRD ζ / BT10): stdlib-compiled is_some / is_none guards ───────

/// End-to-end: `is_some(none)` compiled with the real stdlib must evaluate to
/// `false`.
///
/// MEASURED under intercept removal: fails with `left: Undef` (mechanism note
/// above `e2e_or_default_some_with_stdlib`).
///
/// FIXTURE RATIONALE — `option_recovery.ri` ships the typecheck-only
/// `pub fn is_some<T>(o: Option<T>) -> Bool { true }`, so under #5593's
/// prelude-backed harness the `none` subject is what makes this discriminate:
/// a `some` subject coincides with the placeholder's hardcoded `true`.
///
/// Note a bare `none` DOES type-infer here — `is_some` has a single type
/// parameter and no competing constraint — even though the same bare `none`
/// fails to infer in the two-argument `or_else`/`map_or` calls, where a second
/// argument pins a conflicting `T`.
#[test]
fn e2e_is_some_none_with_stdlib() {
    let module =
        reify_test_support::compile_source_with_stdlib("structure S { let v = is_some(none) }");
    let expr = reify_test_support::get_let_expr(&module, "v");
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    assert_eq!(
        reify_expr::eval_expr(expr, &ctx),
        Value::Bool(false),
        "e2e: is_some(none) compiled via stdlib must evaluate to false — \
         if the intercept stops firing this falls through to eval_user_function_call \
         and yields Undef"
    );
}

/// End-to-end: `is_none(none)` compiled with the real stdlib must evaluate to
/// `true`.
///
/// MEASURED under intercept removal: fails with `left: Undef` (mechanism note
/// above `e2e_or_default_some_with_stdlib`).
///
/// FIXTURE RATIONALE — `option_recovery.ri` ships the typecheck-only
/// `pub fn is_none<T>(o: Option<T>) -> Bool { false }`. Shares the `none`
/// subject with `e2e_is_some_none_with_stdlib` above, because under #5593's
/// prelude-backed harness one subject discriminates BOTH predicates: the two
/// placeholder constants are the exact inverses of the correct answers for a
/// `none`, whereas a `some` subject would coincide with both.
#[test]
fn e2e_is_none_none_with_stdlib() {
    let module =
        reify_test_support::compile_source_with_stdlib("structure S { let v = is_none(none) }");
    let expr = reify_test_support::get_let_expr(&module, "v");
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    assert_eq!(
        reify_expr::eval_expr(expr, &ctx),
        Value::Bool(true),
        "e2e: is_none(none) compiled via stdlib must evaluate to true — \
         if the intercept stops firing this falls through to eval_user_function_call \
         and yields Undef"
    );
}

// ── step-9: get_or (Map<K,V> miss recovery) ───────────────────────────────────
//
// get_or(m, key, dflt): key present -> m[key]; key absent -> dflt (§9.2.6
// map-miss recovery); m=undef -> Undef.
// Result type is Type::length() (the map value type V).
//
// RED today: get_or not yet in is_combinator → falls through →
// eval_user_function_call → function not found (simple ctx) → Undef.

fn val_1mm() -> Value {
    Value::Scalar {
        si_value: 0.001,
        dimension: DimensionVector::LENGTH,
    }
}

/// Build a Map<String,Length> literal with one entry: "k" => 1mm.
fn expr_map_k_1mm() -> CompiledExpr {
    CompiledExpr::map_literal(
        vec![(
            CompiledExpr::literal(Value::String("k".to_string()), Type::String),
            CompiledExpr::literal(val_1mm(), Type::length()),
        )],
        Type::Map(Box::new(Type::String), Box::new(Type::length())),
    )
}

/// get_or(map{"k"=>1mm}, "k", 0mm) == 1mm  (present key)
///
/// RED today: get_or not intercepted → Undef.
#[test]
fn get_or_present_key_returns_value() {
    let call = CompiledExpr::user_function_call(
        "get_or".to_string(),
        vec![
            expr_map_k_1mm(),
            CompiledExpr::literal(Value::String("k".to_string()), Type::String),
            expr_0mm(),
        ],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        val_1mm(),
        "get_or(map{{k=>1mm}}, \"k\", 0mm) must return the map value 1mm"
    );
}

/// get_or(map{"k"=>1mm}, "absent", 0mm) == 0mm  (absent key recovers to dflt)
///
/// RED today: get_or not intercepted → Undef.
#[test]
fn get_or_absent_key_returns_default() {
    let call = CompiledExpr::user_function_call(
        "get_or".to_string(),
        vec![
            expr_map_k_1mm(),
            CompiledExpr::literal(Value::String("absent".to_string()), Type::String),
            expr_0mm(),
        ],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        val_0mm(),
        "get_or(map{{k=>1mm}}, \"absent\", 0mm) must return the default 0mm (§9.2.6 map-miss)"
    );
}

/// get_or(undef, "k", 0mm) == Value::Undef  (undef map subject passthrough)
///
/// GREEN today (coincidentally): any-arg-undef shortcircuit fires.
#[test]
fn get_or_undef_map_returns_undef() {
    let undef_map = CompiledExpr::literal(
        Value::Undef,
        Type::Map(Box::new(Type::String), Box::new(Type::length())),
    );
    let call = CompiledExpr::user_function_call(
        "get_or".to_string(),
        vec![
            undef_map,
            CompiledExpr::literal(Value::String("k".to_string()), Type::String),
            expr_0mm(),
        ],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "get_or(undef, \"k\", 0mm) must propagate Undef — undef map passthrough"
    );
}

/// End-to-end: `get_or(map{"k" => 1mm}, "absent", 0mm)` compiled with the
/// stdlib must evaluate to 0mm (absent key recovers to default).
///
/// CORRECTED BY TASK 5410 (this comment previously called the test
/// "coincidentally correct" and therefore a no-op — that premise was WRONG for
/// this harness). MEASURED under intercept removal: this test FAILS with
/// `left: Undef`. It is a real, working drift guard for `get_or`'s ABSENT-KEY
/// path (PRD ζ / BT10), proving the compiler-emitted `UserFunctionCall`
/// function_name+arity reaches the intercept.
///
/// The old note reasoned about `option_recovery.ri`'s `{ dflt }` body returning
/// `0mm` and so agreeing with the expected value — true of the `.ri` SOURCE,
/// but this harness never registers that body. See the mechanism note above
/// `e2e_or_default_some_with_stdlib` (and #5593 for the scope residue).
#[test]
fn e2e_get_or_absent_key_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        r#"structure S { let v = get_or(map{"k" => 1mm}, "absent", 0mm) }"#,
    );
    let expr = reify_test_support::get_let_expr(&module, "v");
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let result = reify_expr::eval_expr(expr, &ctx);
    assert_eq!(
        result,
        val_0mm(),
        "e2e: get_or(map{{k=>1mm}}, \"absent\", 0mm) compiled via stdlib must evaluate to 0mm"
    );
}

// ── step-9: end-to-end present key via compile_source_with_stdlib ─────────────

/// End-to-end: `get_or(map{"k" => 1mm}, "k", 0mm)` compiled with the stdlib
/// must evaluate to 1mm (present key → intercept fires → returns map value).
///
/// CORRECTED BY TASK 5410: this comment previously said the absent-key e2e
/// (`e2e_get_or_absent_key_with_stdlib`) was "coincidentally GREEN ... and does
/// NOT prove the intercept fires". That is FALSE for this harness. MEASURED
/// under intercept removal: BOTH tests fail, each with `left: Undef`. Both are
/// real drift guards; this present-key case additionally pins the map-lookup
/// RESULT (1mm), which the absent-key case cannot.
///
/// The `.ri` reasoning behind the old claim remains true of the `.ri` SOURCE —
/// `option_recovery.ri`'s `{ dflt }` body would return `0mm`, agreeing with the
/// absent-key expectation — it is just not what this harness observes. See the
/// mechanism note above `e2e_or_default_some_with_stdlib`.
#[test]
fn e2e_get_or_present_key_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        r#"structure S { let v = get_or(map{"k" => 1mm}, "k", 0mm) }"#,
    );
    let expr = reify_test_support::get_let_expr(&module, "v");
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let result = reify_expr::eval_expr(expr, &ctx);
    assert_eq!(
        result,
        val_1mm(),
        "e2e: get_or(map{{k=>1mm}}, \"k\", 0mm) compiled via stdlib must evaluate to 1mm (intercept fires)"
    );
}

// ── task 5410 (PRD ζ / BT10): stdlib-compiled get_or undef-PROPAGATION test ──

/// End-to-end: `get_or(undef, "k", 0mm)` compiled with the real stdlib
/// evaluates to `Undef` — the Kleene INV-2 undef-subject passthrough, NOT the
/// default. This pins undef PROPAGATION through a stdlib-compiled call site.
///
/// PAIRED WITH A LIVENESS WITNESS. `Value::Undef` is this evaluator's universal
/// degradation value, so an `assert_eq!(.., Undef)` on its own also passes when
/// nothing works: the compiler failing to resolve `get_or`, `get_let_expr`
/// returning some other degrading expression, the any-arg-undef short-circuit
/// in `eval_user_function_call` firing early, or the map subject not compiling
/// at all. The fixture therefore compiles a SECOND cell `w` in the same
/// `structure S` whose value is non-Undef (`1mm`) and asserts it FIRST. `w`
/// failing means the harness is dead; `w` passing and `v` failing means undef
/// propagation genuinely regressed. Without `w` this test pins no behaviour a
/// broken harness would not also satisfy.
///
/// MEASURED (task 5410 step-11, three intercept gates disabled): the `w`
/// assertion FAILS with `left: Undef` — so, unlike the single-cell version this
/// replaces, the test as a whole is RED under intercept removal. The `v`
/// assertion alone would stay GREEN, because the `eval_user_function_call`
/// fallthrough value and the correct INV-2 answer are both `Undef`.
///
/// The absent-KEY path's own drift coverage lives in
/// `e2e_get_or_absent_key_with_stdlib` (above), which also fails with
/// `left: Undef` under intercept removal.
///
/// The propagation property is worth pinning at a compiled call site even
/// though `get_or_undef_map_returns_undef` covers it under
/// `EvalContext::simple`: this is the only stdlib-compiled test showing that an
/// undef map SUBJECT is not silently conflated with a key miss — `v` and `w`
/// here are exactly that contrast, in one compiled module.
///
/// MEASURED NEGATIVE, recorded so it is not re-attempted: the undef-KEY form
/// `get_or(map{"k" => 1mm}, undef, 0mm)` does NOT compile —
/// `E_FALLBACK_TYPE: conflicting type arguments for type parameter 'K' ...
/// String vs <error>`.
#[test]
fn e2e_get_or_undef_map_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        r#"structure S { let v = get_or(undef, "k", 0mm)  let w = get_or(map{"k" => 1mm}, "k", 0mm) }"#,
    );
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);

    // Liveness witness first: a non-Undef expectation on the same compiled
    // module, so the Undef assertion below cannot pass on a dead harness.
    assert_eq!(
        reify_expr::eval_expr(reify_test_support::get_let_expr(&module, "w"), &ctx),
        val_1mm(),
        "harness liveness: get_or(map{{k=>1mm}}, \"k\", 0mm) in the same compiled module must \
         evaluate to 1mm — if THIS fails, the Undef assertion below proves nothing"
    );

    assert_eq!(
        reify_expr::eval_expr(reify_test_support::get_let_expr(&module, "v"), &ctx),
        Value::Undef,
        "e2e: get_or(undef, \"k\", 0mm) compiled via stdlib must propagate Undef (INV-2)"
    );
}

// ── get_or: undef key propagation ────────────────────────────────────────────

/// get_or(map{"k"=>1mm}, undef_key, 0mm) == Value::Undef
///
/// An undef key (failed key computation) must not be silently conflated with a
/// legitimate key miss (which recovers to dflt).  Mirrors the
/// `eval_index_access` behaviour in `lib.rs` — both return Undef when the
/// index/key is undef.
///
/// Without the guard: BTreeMap::get(&Undef) returns None → dflt (0mm).
/// With the guard: the undef-key short-circuit fires before the BTreeMap
/// lookup → Undef.
#[test]
fn get_or_undef_key_returns_undef() {
    let call = CompiledExpr::user_function_call(
        "get_or".to_string(),
        vec![
            expr_map_k_1mm(),
            CompiledExpr::literal(Value::Undef, Type::String),
            expr_0mm(),
        ],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "get_or(map, undef_key, dflt) must propagate Undef — undef key mirrors eval_index_access"
    );
}

// ── step-9: map_or (ctx-aware arrow-type intercept) ───────────────────────────
//
// map_or(o, dflt, f): subject=some(x) -> APPLY f to x (f(x)); subject=none -> dflt;
// subject=undef -> Undef (Kleene INV-2).
//
// Unlike the 7 pure combinators (eval_combinator / is_combinator), map_or must
// APPLY its function argument `f` and therefore needs the EvalContext (for
// apply_lambda).  It is handled by a dedicated ctx-aware branch in
// reify-expr/src/lib.rs's UserFunctionCall arm — NOT by is_combinator (which
// stays pure, INV-1).
//
// RED today: no map_or intercept exists, so the call falls through to
// eval_user_function_call (no functions in EvalContext::simple) → function not
// found → Undef for the some and none cases (the discriminating signals).

/// Build the lambda CompiledExpr `|x| x * 2` (Int -> Int), no captures.
fn expr_lambda_double() -> CompiledExpr {
    let x_id = ValueCellId::new("$lambda0.S", "x");
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Int,
    );
    CompiledExpr::lambda(
        vec![("x".to_string(), None)],
        vec![x_id],
        body,
        vec![],
        Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::Int),
        },
    )
}

fn expr_some_int(n: i64) -> CompiledExpr {
    CompiledExpr::option_some(
        CompiledExpr::literal(Value::Int(n), Type::Int),
        Type::Option(Box::new(Type::Int)),
    )
}

fn expr_none_int() -> CompiledExpr {
    CompiledExpr::option_none(Type::Option(Box::new(Type::Int)))
}

fn expr_int(n: i64) -> CompiledExpr {
    CompiledExpr::literal(Value::Int(n), Type::Int)
}

/// map_or(some(5), 99, |x| x * 2) == Int(10)  (lambda APPLIED to the inner value)
///
/// The discriminating case: the placeholder `.ri` body `{ dflt }` returns 99
/// (or Undef under EvalContext::simple), so only a real ctx-aware intercept that
/// applies `f` to the unwrapped Some value yields f(5)=10.
///
/// RED today: map_or has no intercept → falls through → function not found
/// (simple ctx) → Undef.  After step-10 the intercept applies the lambda → 10.
#[test]
fn map_or_some_applies_lambda_to_inner() {
    let call = CompiledExpr::user_function_call(
        "map_or".to_string(),
        vec![expr_some_int(5), expr_int(99), expr_lambda_double()],
        Type::Int,
    );
    assert_eq!(
        eval_simple(&call),
        Value::Int(10),
        "map_or(some(5), 99, |x| x*2) must APPLY the lambda to the inner value → f(5)=10"
    );
}

/// map_or(none, 99, |x| x * 2) == Int(99)  (default; lambda NOT applied)
///
/// RED today: map_or not intercepted → function not found (simple ctx) → Undef.
#[test]
fn map_or_none_returns_default() {
    let call = CompiledExpr::user_function_call(
        "map_or".to_string(),
        vec![expr_none_int(), expr_int(99), expr_lambda_double()],
        Type::Int,
    );
    assert_eq!(
        eval_simple(&call),
        Value::Int(99),
        "map_or(none, 99, f) must return the default 99 (f not applied)"
    );
}

/// map_or(undef, 99, |x| x * 2) == Value::Undef  (Kleene INV-2 subject passthrough)
///
/// GREEN today (coincidentally): the any-arg-undef shortcircuit in
/// eval_user_function_call fires on the undef subject and returns Undef.  Pinned
/// here so the step-10 intercept preserves undef-subject passthrough.
#[test]
fn map_or_undef_subject_returns_undef() {
    let undef_opt = CompiledExpr::literal(Value::Undef, Type::Option(Box::new(Type::Int)));
    let call = CompiledExpr::user_function_call(
        "map_or".to_string(),
        vec![undef_opt, expr_int(99), expr_lambda_double()],
        Type::Int,
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "map_or(undef, 99, f) must propagate Undef — undef subject passthrough (Kleene INV-2)"
    );
}

/// map_or(5, 99, |x| x * 2) == Value::Undef — non-Option subject degrades gracefully.
#[test]
fn map_or_non_option_subject_degrades_to_undef() {
    let call = CompiledExpr::user_function_call(
        "map_or".to_string(),
        vec![expr_int(5), expr_int(99), expr_lambda_double()],
        Type::Int,
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "map_or with non-Option subject must degrade to Undef (graceful type-error)"
    );
}

// ── task 5410 (PRD ζ / BT10): stdlib-compiled map_or drift guard ─────────────

/// End-to-end: `map_or(some(5mm), 0mm, |x: Length| x * 2)` compiled with the
/// real stdlib must evaluate to 10mm — the lambda APPLIED to the inner value.
///
/// MEASURED under intercept removal: fails with `left: Undef`. This one covers
/// the SECOND gate — the bare `map_or`/3 branch in reify-expr's
/// `UserFunctionCall` arm, which is NOT in `option_recovery::is_combinator`
/// (mechanism note above `e2e_or_default_some_with_stdlib`).
///
/// The NON-IDENTITY lambda is load-bearing IN THIS HARNESS: the 5mm→10mm
/// doubling is the only evidence here that the compiled lambda argument really
/// reaches `apply_lambda`, rather than the intercept merely matching name and
/// arity.
///
/// FIXTURE RATIONALE — `option_recovery.ri` ships the typecheck-only
/// `pub fn map_or<T, U>(o: Option<T>, dflt: U, f: (T) -> U) -> U { dflt }`
/// (returns `dflt`, `f` never applied), so under #5593's prelude-backed harness
/// the `some` subject is what makes this discriminate: a `none` subject makes
/// `dflt` the correct answer, agreeing with the placeholder.
///
/// Measured negative: the none-path form `map_or(none, 7mm, |x: Length| x * 2)`
/// does NOT compile — `conflicting type arguments for 'T': Real vs Scalar[m]`,
/// because a bare `none` infers `T = Real`. The some-path is therefore the only
/// inline stdlib-compiled form available for this combinator.
#[test]
fn e2e_map_or_some_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        "structure S { let v = map_or(some(5mm), 0mm, |x: Length| x * 2) }",
    );
    let expr = reify_test_support::get_let_expr(&module, "v");
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    assert_eq!(
        reify_expr::eval_expr(expr, &ctx),
        val_10mm(),
        "e2e: map_or(some(5mm), 0mm, |x| x * 2) compiled via stdlib must evaluate to 10mm — \
         if the map_or/3 gate stops firing this falls through to eval_user_function_call \
         and yields Undef"
    );
}

// ── type-error degradation: `_` arms ─────────────────────────────────────────
//
// Each combinator's `_` arm degrades gracefully to Value::Undef when the
// subject carries the wrong tag (Option-family combinators expect Value::Option;
// get_or expects Value::Map).  These arms prevent panics or undefined behaviour
// when a type error reaches the runtime.

/// unwrap_or(5mm, 0mm) == Value::Undef — non-Option subject degrades gracefully.
#[test]
fn unwrap_or_non_option_subject_degrades_to_undef() {
    let call = CompiledExpr::user_function_call(
        "unwrap_or".to_string(),
        vec![expr_5mm(), expr_0mm()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "unwrap_or with non-Option subject must degrade to Undef (graceful type-error)"
    );
}

/// or_else(5mm, none) == Value::Undef — non-Option subject degrades gracefully.
#[test]
fn or_else_non_option_subject_degrades_to_undef() {
    let call = CompiledExpr::user_function_call(
        "or_else".to_string(),
        vec![expr_5mm(), expr_none_length()],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "or_else with non-Option subject must degrade to Undef (graceful type-error)"
    );
}

/// is_some(5mm) == Value::Undef — non-Option subject degrades gracefully.
#[test]
fn is_some_non_option_subject_degrades_to_undef() {
    let call = CompiledExpr::user_function_call(
        "is_some".to_string(),
        vec![expr_5mm()],
        Type::Bool,
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "is_some with non-Option subject must degrade to Undef (graceful type-error)"
    );
}

/// is_none(5mm) == Value::Undef — non-Option subject degrades gracefully.
#[test]
fn is_none_non_option_subject_degrades_to_undef() {
    let call = CompiledExpr::user_function_call(
        "is_none".to_string(),
        vec![expr_5mm()],
        Type::Bool,
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "is_none with non-Option subject must degrade to Undef (graceful type-error)"
    );
}

/// get_or(5mm, "k", 0mm) == Value::Undef — non-Map subject degrades gracefully.
#[test]
fn get_or_non_map_subject_degrades_to_undef() {
    let call = CompiledExpr::user_function_call(
        "get_or".to_string(),
        vec![
            expr_5mm(),
            CompiledExpr::literal(Value::String("k".to_string()), Type::String),
            expr_0mm(),
        ],
        Type::length(),
    );
    assert_eq!(
        eval_simple(&call),
        Value::Undef,
        "get_or with non-Map subject must degrade to Undef (graceful type-error)"
    );
}

// ── sync-drift check ──────────────────────────────────────────────────────────
//
// Asserts that each combinator declared in option_recovery.ri is recognised by
// the is_combinator gate and routes to the correct eval_combinator logic.
//
// If a new combinator is added to option_recovery.ri without a matching entry
// in is_combinator, the intercept won't fire and the placeholder .ri body runs
// instead.  This consolidated test catches that drift — any Undef result for
// the chosen inputs proves the gate is not firing.
//
// Cross-reference:
//   crates/reify-compiler/stdlib/option_recovery.ri  — canonical pub fn arities
//   crates/reify-compiler/src/expr.rs FALLBACK_COMBINATORS — type-checker subset

/// Every combinator declared in option_recovery.ri is recognised by
/// `is_combinator` at its declared arity and routes to the correct eval logic.
///
/// Inputs are chosen to produce a known non-Undef output when the intercept
/// fires.  If the intercept does NOT fire, `eval_user_function_call` is called
/// (no functions in the simple EvalContext) → Undef, failing the assertion.
#[test]
fn sync_drift_check_all_combinators_recognized() {
    // extract-or-default family (arity 2): some(5mm) → 5mm (inner, not dflt)
    for &name in ["unwrap_or", "or_default", "fallback"].iter() {
        let call = CompiledExpr::user_function_call(
            name.to_string(),
            vec![expr_some_5mm(), expr_0mm()],
            Type::length(),
        );
        assert_eq!(
            eval_simple(&call),
            val_5mm(),
            "{name}(some(5mm), 0mm) must return 5mm — is_combinator gate out of sync with option_recovery.ri"
        );
    }

    // or_else (arity 2): some(5mm) → Value::Option(Some(5mm)) (subject unchanged)
    {
        let call = CompiledExpr::user_function_call(
            "or_else".to_string(),
            vec![expr_some_5mm(), expr_none_length()],
            Type::Option(Box::new(Type::length())),
        );
        assert_eq!(
            eval_simple(&call),
            Value::Option(Some(Box::new(val_5mm()))),
            "or_else(some(5mm), none) must return some(5mm) — gate out of sync"
        );
    }

    // is_some (arity 1): some(5mm) → Bool(true)
    {
        let call = CompiledExpr::user_function_call(
            "is_some".to_string(),
            vec![expr_some_5mm()],
            Type::Bool,
        );
        assert_eq!(
            eval_simple(&call),
            Value::Bool(true),
            "is_some(some(5mm)) must return Bool(true) — gate out of sync"
        );
    }

    // is_none (arity 1): none → Bool(true)
    {
        let call = CompiledExpr::user_function_call(
            "is_none".to_string(),
            vec![expr_none_length()],
            Type::Bool,
        );
        assert_eq!(
            eval_simple(&call),
            Value::Bool(true),
            "is_none(none) must return Bool(true) — gate out of sync"
        );
    }

    // get_or (arity 3): map{"k"=>1mm}, "k", 0mm → 1mm (present key)
    {
        let call = CompiledExpr::user_function_call(
            "get_or".to_string(),
            vec![
                expr_map_k_1mm(),
                CompiledExpr::literal(Value::String("k".to_string()), Type::String),
                expr_0mm(),
            ],
            Type::length(),
        );
        assert_eq!(
            eval_simple(&call),
            val_1mm(),
            "get_or(map{{k=>1mm}}, \"k\", 0mm) must return 1mm — gate out of sync"
        );
    }

    // map_or (arity 3, ctx-aware): some(5), 99, |x| x*2 → 10 (lambda applied).
    //
    // map_or is declared in option_recovery.ri but, unlike the 7 pure
    // combinators above, is intentionally NOT in is_combinator (it must apply
    // its function argument, which needs the EvalContext).  Its drift signal is
    // therefore the dedicated ctx-aware branch in lib.rs's UserFunctionCall arm.
    // If that branch is missing/out of sync, the call falls through to
    // eval_user_function_call → Undef ≠ 10, failing here.
    {
        let call = CompiledExpr::user_function_call(
            "map_or".to_string(),
            vec![expr_some_int(5), expr_int(99), expr_lambda_double()],
            Type::Int,
        );
        assert_eq!(
            eval_simple(&call),
            Value::Int(10),
            "map_or(some(5), 99, |x| x*2) must return 10 — ctx-aware map_or route out of sync with option_recovery.ri"
        );
    }
}
