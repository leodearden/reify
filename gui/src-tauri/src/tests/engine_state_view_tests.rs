//! Tests for [`crate::engine_state_view`].

use serde_json::{Value, json};

use crate::commands::engine_state_json;
use crate::engine_state_view::EngineStateView;
use crate::tests::make_test_engine;

/// A real `engine_state` payload, so the keys under test are the live ones.
fn full_payload() -> Value {
    let engine = make_test_engine();
    let mut session = engine.lock().expect("engine lock");
    let full = engine_state_json(&mut session).expect("engine_state_json succeeds");
    let files = full["files"].as_array().expect("files is an array");
    assert!(
        files
            .iter()
            .any(|file| !file["content"].as_str().unwrap_or_default().is_empty()),
        "the fixture must carry source content, or the summary's content-free check is vacuous"
    );
    full
}

fn keys(value: &Value) -> Vec<&String> {
    value.as_object().expect("an object").keys().collect()
}

fn view(params: Value) -> Result<EngineStateView, String> {
    EngineStateView::from_params(&params)
}

fn contains_key_anywhere(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(k, v)| k == key || contains_key_anywhere(v, key)),
        Value::Array(items) => items.iter().any(|v| contains_key_anywhere(v, key)),
        _ => false,
    }
}

#[test]
fn no_view_params_return_the_full_payload_unchanged() {
    let full = full_payload();

    let parsed = view(json!({})).expect("empty params are valid");
    assert_eq!(parsed, EngineStateView::Full);
    let applied = parsed.apply(full.clone()).expect("the full view applies");

    assert_eq!(
        serde_json::to_string(&applied).expect("serializes"),
        serde_json::to_string(&full).expect("serializes"),
    );
}

#[test]
fn summary_only_false_is_the_full_view() {
    assert_eq!(
        view(json!({"summary_only": false})),
        Ok(EngineStateView::Full)
    );
}

#[test]
fn fields_select_exactly_the_named_top_level_keys() {
    let full = full_payload();

    let applied = view(json!({"fields": ["values", "stale"]}))
        .expect("known fields")
        .apply(full.clone())
        .expect("known fields apply");

    assert_eq!(
        applied,
        json!({"values": full["values"], "stale": full["stale"]})
    );
}

#[test]
fn an_unknown_field_is_refused_naming_it_and_every_valid_key() {
    let full = full_payload();

    let refusal = view(json!({"fields": ["values", "nope"]}))
        .expect("the shape is valid; the names are checked against the payload")
        .apply(full.clone())
        .expect_err("an unknown field must be refused, never silently omitted");

    assert!(refusal.contains("nope"), "{refusal}");
    for key in keys(&full) {
        assert!(refusal.contains(key.as_str()), "missing {key}: {refusal}");
    }
}

#[test]
fn malformed_view_params_are_refused() {
    for (params, problem) in [
        (json!({"fields": []}), "empty"),
        (json!({"fields": "values"}), "array"),
        (json!({"fields": [1]}), "string"),
        (json!({"summary_only": "yes"}), "boolean"),
    ] {
        let refusal = view(params.clone()).expect_err("malformed params must be refused");
        assert!(
            refusal.contains(problem),
            "{params}: expected the refusal to mention {problem:?}, got {refusal}"
        );
    }
}

#[test]
fn summary_only_and_fields_are_mutually_exclusive() {
    assert_eq!(
        view(json!({"summary_only": true, "fields": ["values"]})),
        Err("summary_only and fields are mutually exclusive".to_string())
    );
}

#[test]
fn the_summary_counts_arrays_keeps_scalars_and_lists_files_without_content() {
    let full = full_payload();

    let parsed = view(json!({"summary_only": true})).expect("valid");
    assert_eq!(parsed, EngineStateView::Summary);
    let summary = parsed.apply(full.clone()).expect("the summary applies");

    let full = full.as_object().expect("an object");
    let counts = summary["counts"].as_object().expect("counts is an object");
    let mut arrays = 0;
    for (key, value) in full {
        match value {
            Value::Array(items) => {
                arrays += 1;
                assert_eq!(counts.get(key), Some(&json!(items.len())), "count of {key}");
            }
            scalar => assert_eq!(summary.get(key), Some(scalar), "{key} passes through"),
        }
    }
    assert_eq!(counts.len(), arrays, "one count per array-valued key");
    assert!(arrays >= 6, "the live payload has six arrays; got {arrays}");

    let listed: Vec<Value> = full["files"]
        .as_array()
        .expect("files")
        .iter()
        .map(|file| {
            let content = file["content"].as_str().expect("content");
            json!({
                "path": file["path"],
                "bytes": content.len(),
                "lines": content.lines().count(),
            })
        })
        .collect();
    assert_eq!(summary["files"], json!(listed));
    assert!(
        !contains_key_anywhere(&summary, "content"),
        "no source content may appear in the summary: {summary}"
    );
}
