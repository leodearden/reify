//! Meta access expression evaluation tests — `meta.key` resolves from EvalContext.

use std::collections::HashMap;

use reify_expr::{EvalContext, eval_expr};
use reify_ir::{CompiledExpr, Value, ValueMap};

// ── step-13: MetaAccess returns string value ────────────────────────────────

/// Evaluating `meta.description` with a meta map containing the key should
/// return `Value::String("A bracket")`.
#[test]
fn eval_meta_access_returns_string_value() {
    let expr = CompiledExpr::meta_access("Bracket".into(), "description".into());
    let values = ValueMap::new();

    let mut meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut bracket_meta = HashMap::new();
    bracket_meta.insert("description".to_string(), "A bracket".to_string());
    meta_map.insert("Bracket".to_string(), bracket_meta);

    let ctx = EvalContext::simple(&values).with_meta(&meta_map);
    let result = eval_expr(&expr, &ctx);
    assert_eq!(result, Value::String("A bracket".to_string()));
}

/// Evaluating `meta.description` with a proper meta map must NOT return Undef.
#[test]
fn eval_meta_access_is_not_undef() {
    let expr = CompiledExpr::meta_access("Bracket".into(), "description".into());
    let values = ValueMap::new();

    let mut meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut bracket_meta = HashMap::new();
    bracket_meta.insert("description".to_string(), "A bracket".to_string());
    meta_map.insert("Bracket".to_string(), bracket_meta);

    let ctx = EvalContext::simple(&values).with_meta(&meta_map);
    let result = eval_expr(&expr, &ctx);
    assert_ne!(result, Value::Undef);
}

// ── step-15: Multiple keys in one entity ────────────────────────────────────

/// Evaluating MetaAccess for each key of a multi-key entity should return the
/// correct `Value::String` for each key.
#[test]
fn eval_meta_access_multiple_keys() {
    let values = ValueMap::new();

    let mut widget_meta = HashMap::new();
    widget_meta.insert("name".to_string(), "Gear".to_string());
    widget_meta.insert("version".to_string(), "2.0".to_string());
    widget_meta.insert("material".to_string(), "steel".to_string());

    let mut meta_map: HashMap<String, HashMap<String, String>> = HashMap::new();
    meta_map.insert("Widget".to_string(), widget_meta);

    let ctx = EvalContext::simple(&values).with_meta(&meta_map);

    let expr_name = CompiledExpr::meta_access("Widget".into(), "name".into());
    assert_eq!(
        eval_expr(&expr_name, &ctx),
        Value::String("Gear".to_string())
    );

    let expr_version = CompiledExpr::meta_access("Widget".into(), "version".into());
    assert_eq!(
        eval_expr(&expr_version, &ctx),
        Value::String("2.0".to_string())
    );

    let expr_material = CompiledExpr::meta_access("Widget".into(), "material".into());
    assert_eq!(
        eval_expr(&expr_material, &ctx),
        Value::String("steel".to_string())
    );
}

// ── step-17: No meta context panics (silent-defaults convention) ────────────

/// Evaluating MetaAccess without meta context should panic — not silently
/// return Undef. Enforces the 'silent defaults should be noisy' convention.
#[test]
#[should_panic(expected = "MetaAccess evaluation requires meta context")]
fn eval_meta_access_no_context_panics() {
    let expr = CompiledExpr::meta_access("Bracket".into(), "description".into());
    let values = ValueMap::new();
    let ctx = EvalContext::simple(&values); // meta defaults to None
    let _ = eval_expr(&expr, &ctx);
}
