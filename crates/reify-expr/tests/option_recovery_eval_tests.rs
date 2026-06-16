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

// ── step-1: end-to-end via compile_source_with_stdlib ────────────────────────

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
/// RED today: placeholder body returns dflt regardless → 0mm coincidentally
/// correct for the absent-key case, but the present-key case above (step-9
/// primary signal) is RED.  Pinned here to prove the compiler-emitted
/// UserFunctionCall function_name+arity reaches the intercept.
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
