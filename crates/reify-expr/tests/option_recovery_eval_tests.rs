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
//! ## Stdlib-compiled drift guards (task 5410 / PRD
//! docs/prds/v0_6/placeholder-type-eradication-ratchet.md §3 item 10, §8 task
//! ζ, BT10)
//!
//! Tests that evaluate under `EvalContext::simple` have an EMPTY function
//! table, so they pin the intercept's behaviour but say nothing about which
//! path a *real compiled program* takes.  The `e2e_*_with_stdlib` tests close
//! that half of the gap: they compile the real stdlib and evaluate under
//! `EvalContext::new(&values, &module.functions)`, so the compiler-emitted
//! `UserFunctionCall` is exercised on the same path a real program takes.
//!
//! WHAT THEY ACTUALLY GUARD, MEASURED: that THE INTERCEPT FIRES.  They do NOT
//! observe the `.ri` placeholder body at all — `module.functions` is
//! user-source-only, so with the intercept removed every such call falls
//! through to `eval_user_function_call`, matches nothing, and returns
//! `Value::Undef`.  See the section banner above `e2e_or_default_some_with_stdlib`
//! for the full mechanism, the measured evidence, and the scope residue
//! (follow-up ticket tkt_0RRQDAY187DZW2V1Q5N5P9679F).

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
// CANONICAL MECHANISM NOTE for every `e2e_*_with_stdlib` test in this file.
//
// These compile the REAL stdlib via `compile_source_with_stdlib` and evaluate
// against `module.functions`, so the compiler-emitted `UserFunctionCall` runs
// on the same path a real program takes.
//
// WHAT THEY GUARD, MEASURED — *not* what the `.ri` placeholder body returns.
// `module.functions` is USER-SOURCE-ONLY: the compiler merges the prelude /
// stdlib `.ri` functions into `resolution_functions` for compile-time overload
// dispatch, but stores only user-source functions in `CompiledModule` (see
// reify-compiler `src/compile_builder/functions_phase.rs:100-105`). So in THIS
// harness the stdlib bodies are never registered. With the intercept removed
// each call falls through to `eval_user_function_call`, finds no matching
// entry, and returns `Value::Undef` (reify-expr `src/lib.rs:1625`).
//
// MEASURED (task 5410 step-11; the three gates in reify-expr's
// `UserFunctionCall` arm — `option_recovery::is_combinator`, `map_or`/3 and
// `map_err`/2 — locally set to `if false && …`): every guard listed below
// fails with `left: Undef`.  Never a placeholder value.  The one exception is
// `e2e_get_or_undef_map_with_stdlib`, which stays GREEN because its expected
// value is *itself* `Undef`; it is documented there as a propagation test, not
// a drift guard.
//
// SCOPE, stated honestly: these guard "THE INTERCEPT FIRES", which is the
// drift mode this harness can observe. They do NOT reproduce the PRD's
// silent-WRONG-VALUE mode (intercept gone → placeholder body silently returns
// `dflt` / `true` / the subject), because that needs a prelude-backed function
// table. Deferred to follow-up ticket tkt_0RRQDAY187DZW2V1Q5N5P9679F
// (prelude-backed eval harness via reify-eval `merge_functions`, or a new
// public reify-compiler prelude accessor).
//
// The `.ri` placeholder values quoted in the per-test doc comments are TRUE
// ABOUT THE `.ri` SOURCE — they are the PRD linkage and the reason each
// fixture was chosen — but this harness does not observe them.
//
// These are deliberate REGRESSION LOCKS, not RED-first tests: the intercept is
// already live, so they are GREEN the moment they are written.

/// End-to-end: `or_default(some(5mm), 0mm)` compiled with the real stdlib must
/// evaluate to 5mm — the unboxed inner value.
///
/// MEASURED under intercept removal: fails with `left: Undef`. The call falls
/// through to `eval_user_function_call`, whose `module.functions` table holds
/// no `or_default` body (user-source-only — see the section banner above).
/// (`option_recovery.ri` ships
/// `pub fn or_default<T>(o: Option<T>, dflt: T) -> T { dflt }`, a
/// typecheck-only placeholder that would return `0mm`; that body is never
/// registered in this harness, so the guard observes the fallthrough, not the
/// placeholder.)
///
/// FIXTURE RATIONALE (against the `.ri` source, for the prelude-backed harness
/// of ticket tkt_0RRQDAY187DZW2V1Q5N5P9679F): the `some` subject is chosen so
/// the fixture would discriminate there too — a `none` subject makes `0mm` the
/// correct answer, agreeing with the placeholder's `dflt`.
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
    let expr = cell_expr_stdlib(&module, "v");
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
/// MEASURED under intercept removal: fails with `left: Undef`. The call falls
/// through to `eval_user_function_call`, whose `module.functions` table holds
/// no `is_some` body (user-source-only — see the section banner above
/// `e2e_or_default_some_with_stdlib`). (`option_recovery.ri` ships
/// `pub fn is_some<T>(o: Option<T>) -> Bool { true }`, a typecheck-only
/// placeholder that hardcodes `true`; that body is never registered in this
/// harness, so the guard observes the fallthrough, not the placeholder.)
///
/// FIXTURE RATIONALE (against the `.ri` source, for the prelude-backed harness
/// of ticket tkt_0RRQDAY187DZW2V1Q5N5P9679F): the `none` subject is chosen so
/// the fixture would discriminate there too — a `some` subject coincides with
/// the placeholder's hardcoded `true`.
///
/// Note a bare `none` DOES type-infer here — `is_some` has a single type
/// parameter and no competing constraint — even though the same bare `none`
/// fails to infer in the two-argument `or_else`/`map_or` calls, where a second
/// argument pins a conflicting `T`.
#[test]
fn e2e_is_some_none_with_stdlib() {
    let module =
        reify_test_support::compile_source_with_stdlib("structure S { let v = is_some(none) }");
    let expr = cell_expr_stdlib(&module, "v");
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
/// MEASURED under intercept removal: fails with `left: Undef`. The call falls
/// through to `eval_user_function_call`, whose `module.functions` table holds
/// no `is_none` body (user-source-only — see the section banner above
/// `e2e_or_default_some_with_stdlib`). (`option_recovery.ri` ships
/// `pub fn is_none<T>(o: Option<T>) -> Bool { false }`, a typecheck-only
/// placeholder that hardcodes `false`; that body is never registered in this
/// harness, so the guard observes the fallthrough, not the placeholder.)
///
/// FIXTURE RATIONALE (against the `.ri` source, for the prelude-backed harness
/// of ticket tkt_0RRQDAY187DZW2V1Q5N5P9679F): shares the `none` subject with
/// `e2e_is_some_none_with_stdlib` above, because one subject discriminates BOTH
/// predicates there — the two placeholder constants are the exact inverses of
/// the correct answers for a `none`, whereas a `some` subject would coincide
/// with both.
#[test]
fn e2e_is_none_none_with_stdlib() {
    let module =
        reify_test_support::compile_source_with_stdlib("structure S { let v = is_none(none) }");
    let expr = cell_expr_stdlib(&module, "v");
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
/// `left: Undef`. It is a real, working drift guard.
///
/// Mechanism: `module.functions` is user-source-only, so the `.ri` placeholder
/// body is never registered here; with the intercept removed the call falls
/// through to `eval_user_function_call`, matches nothing, and returns
/// `Value::Undef` — which differs from the expected `0mm`. The old note
/// reasoned about `option_recovery.ri`'s `{ dflt }` body returning `0mm` and
/// so agreeing with the expected value, but this harness never runs that body.
/// See the section banner above `e2e_or_default_some_with_stdlib` for the full
/// mechanism and the scope residue (ticket tkt_0RRQDAY187DZW2V1Q5N5P9679F).
///
/// This is the drift guard for `get_or`'s ABSENT-KEY path (task 5410 / PRD ζ /
/// BT10). Pinned here to prove the compiler-emitted UserFunctionCall
/// function_name+arity reaches the intercept.
#[test]
fn e2e_get_or_absent_key_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        r#"structure S { let v = get_or(map{"k" => 1mm}, "absent", 0mm) }"#,
    );
    let expr = cell_expr_stdlib(&module, "v");
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
/// under intercept removal: BOTH tests fail, each with `left: Undef`, because
/// `module.functions` is user-source-only and the fallthrough to
/// `eval_user_function_call` matches nothing. Both are real drift guards; this
/// present-key case additionally pins the map-lookup RESULT (1mm), which the
/// absent-key case cannot.
///
/// The `.ri` reasoning behind the old claim remains true of the `.ri` SOURCE —
/// `option_recovery.ri`'s `{ dflt }` body would return `0mm`, agreeing with the
/// absent-key expectation — it is just not what this harness observes. See the
/// section banner above `e2e_or_default_some_with_stdlib`.
#[test]
fn e2e_get_or_present_key_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        r#"structure S { let v = get_or(map{"k" => 1mm}, "k", 0mm) }"#,
    );
    let expr = cell_expr_stdlib(&module, "v");
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
/// NOT A DRIFT GUARD — read this before citing it as one.
///
/// MEASURED (task 5410 step-11, three intercept gates disabled): this test
/// stays GREEN. It is the only `e2e_*_with_stdlib` test in this file that does.
/// Reason: `module.functions` is user-source-only, so with the intercept
/// removed the call falls through to `eval_user_function_call`, matches
/// nothing, and returns `Value::Undef` — which is EXACTLY this test's expected
/// value. The fallthrough result and the intercept result coincide, so the
/// assertion cannot tell them apart. Every OTHER guard here expects a non-Undef
/// value and therefore does bite. See the section banner above
/// `e2e_or_default_some_with_stdlib`.
///
/// WHERE THE ABSENT-PATH DRIFT COVERAGE ACTUALLY LIVES:
/// `e2e_get_or_absent_key_with_stdlib` (above), which MEASURABLY fails with
/// `left: Undef` under intercept removal. Its own doc comment used to call it
/// "coincidentally correct"; task 5410 corrected that — the claim was reasoning
/// about the `.ri` placeholder body, which this harness never registers.
///
/// Kept anyway because the INV-2 propagation property is worth pinning at a
/// compiled call site: it is the only stdlib-compiled test showing that an
/// undef map SUBJECT is not silently conflated with a key miss (contrast
/// `get_or_undef_key_returns_undef` / `get_or_absent_key_returns_default`
/// below, which prove the same distinction under `EvalContext::simple`).
///
/// MEASURED NEGATIVE, recorded so it is not re-attempted: the undef-KEY form
/// `get_or(map{"k" => 1mm}, undef, 0mm)` does NOT compile —
/// `E_FALLBACK_TYPE: conflicting type arguments for type parameter 'K' ...
/// String vs <error>`.
#[test]
fn e2e_get_or_undef_map_with_stdlib() {
    let module = reify_test_support::compile_source_with_stdlib(
        r#"structure S { let v = get_or(undef, "k", 0mm) }"#,
    );
    let expr = cell_expr_stdlib(&module, "v");
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    assert_eq!(
        reify_expr::eval_expr(expr, &ctx),
        Value::Undef,
        "e2e: get_or(undef, \"k\", 0mm) compiled via stdlib must propagate Undef (INV-2) — \
         propagation test, NOT a drift guard: it stays green under intercept removal"
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
/// `UserFunctionCall` arm, which is NOT in `option_recovery::is_combinator`.
/// With it disabled the call falls through to `eval_user_function_call`, whose
/// `module.functions` table holds no `map_or` body (user-source-only — see the
/// section banner above `e2e_or_default_some_with_stdlib`).
/// (`option_recovery.ri` ships
/// `pub fn map_or<T, U>(o: Option<T>, dflt: U, f: (T) -> U) -> U { dflt }`, a
/// typecheck-only placeholder that would return `0mm` with `f` never applied;
/// that body is never registered in this harness.)
///
/// The NON-IDENTITY lambda is load-bearing IN THIS HARNESS: the 5mm→10mm
/// doubling is the only evidence here that the compiled lambda argument really
/// reaches `apply_lambda`, rather than the intercept merely matching name and
/// arity.
///
/// FIXTURE RATIONALE (against the `.ri` source, for the prelude-backed harness
/// of ticket tkt_0RRQDAY187DZW2V1Q5N5P9679F): the `some` subject is chosen so
/// the fixture would discriminate there too — a `none` subject makes `dflt` the
/// correct answer, agreeing with the placeholder.
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
    let expr = cell_expr_stdlib(&module, "v");
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
