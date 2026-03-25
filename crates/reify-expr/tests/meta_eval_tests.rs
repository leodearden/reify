//! Meta access expression evaluation tests — `meta.key` resolves from EvalContext.

use std::collections::HashMap;

use reify_expr::{eval_expr, EvalContext};
use reify_types::{CompiledExpr, Value, ValueMap};

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
