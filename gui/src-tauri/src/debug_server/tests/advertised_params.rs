//! Task 6752: every param the wait, status, view and image tools read is
//! advertised in `tool_defs()` under the name its parser reads.
//!
//! A handler reading a name its schema does not advertise passes every other
//! guard, since both halves compile and the parity tests compare tool NAMES
//! only. So each test here reads the property names FROM the advertised schema
//! and feeds them through the parser the handler calls, the way
//! `write_tools::reify_write_tool_params_match_their_advertised_schemas` does
//! for the write tools.
//!
//! A child of `debug_server::tests` because `tool_defs()` is private.

use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::debug_server::tool_defs;
use crate::engine_activity::SettleRequest;
use crate::engine_state_view::EngineStateView;
use crate::screenshot_save::take_save_path;

/// `tool`'s advertised input-schema properties.
fn advertised_properties(tool: &str) -> Map<String, Value> {
    let def = tool_defs()
        .into_iter()
        .find(|d| d.name == tool)
        .unwrap_or_else(|| panic!("{tool} must be advertised in tool_defs()"));
    def.input_schema["properties"]
        .as_object()
        .cloned()
        .unwrap_or_else(|| panic!("{tool}'s schema must declare properties"))
}

/// The advertised name `name` of `tool`, asserted present with JSON type `ty`.
fn advertised_name(props: &Map<String, Value>, tool: &str, name: &str, ty: &str) -> String {
    let (key, schema) = props
        .get_key_value(name)
        .unwrap_or_else(|| panic!("{tool} must advertise '{name}'; it advertises {props:?}"));
    assert_eq!(
        schema["type"].as_str(),
        Some(ty),
        "{tool}.{name} must be advertised as {ty}"
    );
    key.clone()
}

fn params(pairs: Vec<(String, Value)>) -> Value {
    Value::Object(pairs.into_iter().collect())
}

/// Every advertised name must be one of `read`, so the schema offers nothing
/// the parser ignores.
fn assert_only(props: &Map<String, Value>, tool: &str, read: &[&str]) {
    let unread: Vec<&String> = props
        .keys()
        .filter(|k| !read.contains(&k.as_str()))
        .collect();
    assert!(
        unread.is_empty(),
        "{tool} advertises {unread:?}, which its parser never reads"
    );
}

#[test]
fn engine_state_advertises_the_view_params_engine_state_view_reads() {
    let props = advertised_properties("engine_state");
    let summary_only = advertised_name(&props, "engine_state", "summary_only", "boolean");
    let fields = advertised_name(&props, "engine_state", "fields", "array");
    assert_only(&props, "engine_state", &["summary_only", "fields"]);

    assert_eq!(
        EngineStateView::from_params(&params(vec![(summary_only, json!(true))])),
        Ok(EngineStateView::Summary)
    );
    assert_eq!(
        EngineStateView::from_params(&params(vec![(fields, json!(["values"]))])),
        Ok(EngineStateView::Fields(vec!["values".to_string()]))
    );
}

#[test]
fn every_image_tool_advertises_the_save_path_take_save_path_reads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("advertised-params.png");
    for tool in ["screenshot", "screenshot_window", "element_screenshot"] {
        let props = advertised_properties(tool);
        let save_path = advertised_name(&props, tool, "save_path", "string");

        let (taken, _forwarded) =
            take_save_path(params(vec![(save_path, json!(target.to_string_lossy()))]))
                .unwrap_or_else(|e| panic!("{tool}: an absolute save_path must be accepted: {e}"));

        assert_eq!(taken.as_deref(), Some(target.as_path()), "{tool}");
    }
}

#[test]
fn wait_for_idle_advertises_the_params_settle_request_reads() {
    let props = advertised_properties("wait_for_idle");
    let timeout_ms = advertised_name(&props, "wait_for_idle", "timeout_ms", "integer");
    let since_generation = advertised_name(&props, "wait_for_idle", "since_generation", "integer");
    assert_only(&props, "wait_for_idle", &["timeout_ms", "since_generation"]);

    let request = SettleRequest::from_params(&params(vec![
        (timeout_ms, json!(1234)),
        (since_generation, json!(7)),
    ]))
    .expect("advertised params must be accepted");

    assert_eq!(request.timeout, Duration::from_millis(1234));
    assert_eq!(request.since_generation, Some(7));
}

#[test]
fn engine_status_is_advertised_with_no_params() {
    let def = tool_defs()
        .into_iter()
        .find(|d| d.name == "engine_status")
        .expect("engine_status must be advertised in tool_defs()");

    assert_eq!(def.input_schema["properties"], json!({}));
    assert!(
        def.input_schema["required"]
            .as_array()
            .is_none_or(|required| required.is_empty()),
        "engine_status must require nothing"
    );
}
