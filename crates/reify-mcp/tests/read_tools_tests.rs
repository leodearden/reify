use reify_mcp::context::MockToolContext;
use reify_mcp::registry::ToolRegistry;
use reify_mcp::tools::register_all_tools;
use reify_mcp::types::{
    ConstraintInfo, DiagnosticInfo, EvalStatusInfo, OpenFileInfo, ParameterInfo, SelectionInfo,
    SourceContent, SourceLocationInfo,
};

fn setup_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    register_all_tools(&mut registry);
    registry
}

// === reify_get_source ===

#[test]
fn get_source_returns_content_and_file_path() {
    let registry = setup_registry();
    let ctx = MockToolContext {
        source: SourceContent {
            content: "param x = 10mm".to_string(),
            file_path: "main.ri".to_string(),
        },
        ..Default::default()
    };

    let result = registry
        .call_tool(
            "reify_get_source",
            serde_json::json!({"file_path": "main.ri"}),
            &ctx,
        )
        .expect("should succeed");

    assert_eq!(result["content"], "param x = 10mm");
    assert_eq!(result["file_path"], "main.ri");
}

#[test]
fn get_source_without_file_path_returns_active_file() {
    let registry = setup_registry();
    let ctx = MockToolContext {
        source: SourceContent {
            content: "param y = 20mm".to_string(),
            file_path: "active.ri".to_string(),
        },
        ..Default::default()
    };

    let result = registry
        .call_tool("reify_get_source", serde_json::json!({}), &ctx)
        .expect("should succeed");

    assert_eq!(result["content"], "param y = 20mm");
    assert_eq!(result["file_path"], "active.ri");
}

// === reify_get_open_files ===

#[test]
fn get_open_files_returns_file_list() {
    let registry = setup_registry();
    let ctx = MockToolContext {
        open_files: vec![
            OpenFileInfo {
                path: "main.ri".to_string(),
                language: "reify".to_string(),
                dirty: true,
            },
            OpenFileInfo {
                path: "lib.ri".to_string(),
                language: "reify".to_string(),
                dirty: false,
            },
        ],
        ..Default::default()
    };

    let result = registry
        .call_tool("reify_get_open_files", serde_json::json!({}), &ctx)
        .expect("should succeed");

    let files = result.as_array().expect("should be array");
    assert_eq!(files.len(), 2);
    assert_eq!(files[0]["path"], "main.ri");
    assert_eq!(files[0]["dirty"], true);
    assert_eq!(files[1]["path"], "lib.ri");
    assert_eq!(files[1]["dirty"], false);
}

#[test]
fn get_open_files_returns_empty_array_when_none() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry
        .call_tool("reify_get_open_files", serde_json::json!({}), &ctx)
        .expect("should succeed");

    let files = result.as_array().expect("should be array");
    assert_eq!(files.len(), 0);
}

// === reify_get_diagnostics ===

fn make_diagnostic(file_path: &str, severity: &str, message: &str) -> DiagnosticInfo {
    DiagnosticInfo {
        file_path: file_path.to_string(),
        line: 1,
        column: 1,
        end_line: 1,
        end_column: 10,
        severity: severity.to_string(),
        message: message.to_string(),
        code: None,
        has_location: true,
    }
}

fn diagnostics_ctx() -> MockToolContext {
    MockToolContext {
        diagnostics: vec![
            make_diagnostic("file_a.ri", "error", "err1"),
            make_diagnostic("file_a.ri", "error", "err2"),
            make_diagnostic("file_a.ri", "warning", "warn1"),
            make_diagnostic("file_b.ri", "error", "err3"),
        ],
        ..Default::default()
    }
}

#[test]
fn get_diagnostics_returns_all_with_no_filter() {
    let registry = setup_registry();
    let ctx = diagnostics_ctx();

    let result = registry
        .call_tool("reify_get_diagnostics", serde_json::json!({}), &ctx)
        .expect("should succeed");

    assert_eq!(result.as_array().unwrap().len(), 4);
}

#[test]
fn get_diagnostics_filters_by_file_path() {
    let registry = setup_registry();
    let ctx = diagnostics_ctx();

    let result = registry
        .call_tool(
            "reify_get_diagnostics",
            serde_json::json!({"file_path": "file_a.ri"}),
            &ctx,
        )
        .expect("should succeed");

    let diags = result.as_array().unwrap();
    assert_eq!(diags.len(), 3);
    for d in diags {
        assert_eq!(d["file_path"], "file_a.ri");
    }
}

#[test]
fn get_diagnostics_filters_by_severity() {
    let registry = setup_registry();
    let ctx = diagnostics_ctx();

    let result = registry
        .call_tool(
            "reify_get_diagnostics",
            serde_json::json!({"severity": "error"}),
            &ctx,
        )
        .expect("should succeed");

    let diags = result.as_array().unwrap();
    assert_eq!(diags.len(), 3);
    for d in diags {
        assert_eq!(d["severity"], "error");
    }
}

#[test]
fn get_diagnostics_filters_by_file_path_and_severity() {
    let registry = setup_registry();
    let ctx = diagnostics_ctx();

    let result = registry
        .call_tool(
            "reify_get_diagnostics",
            serde_json::json!({"file_path": "file_a.ri", "severity": "error"}),
            &ctx,
        )
        .expect("should succeed");

    let diags = result.as_array().unwrap();
    assert_eq!(diags.len(), 2);
    for d in diags {
        assert_eq!(d["file_path"], "file_a.ri");
        assert_eq!(d["severity"], "error");
    }
}

// === reify_get_parameters ===

fn parameters_ctx() -> MockToolContext {
    MockToolContext {
        parameters: vec![
            ParameterInfo {
                cell_id: "c1".to_string(),
                name: "width".to_string(),
                value: "10".to_string(),
                unit: "mm".to_string(),
                kind: "real".to_string(),
                entity_path: "sketch1/width".to_string(),
                determinacy: "determined".to_string(),
                reason: None,
            },
            ParameterInfo {
                cell_id: "c2".to_string(),
                name: "height".to_string(),
                value: "20".to_string(),
                unit: "mm".to_string(),
                kind: "real".to_string(),
                entity_path: "sketch1/height".to_string(),
                determinacy: "determined".to_string(),
                reason: None,
            },
            ParameterInfo {
                cell_id: "c3".to_string(),
                name: "depth".to_string(),
                value: "5".to_string(),
                unit: "mm".to_string(),
                kind: "real".to_string(),
                entity_path: "sketch2/depth".to_string(),
                determinacy: "determined".to_string(),
                reason: None,
            },
        ],
        ..Default::default()
    }
}

#[test]
fn get_parameters_returns_all_with_no_filter() {
    let registry = setup_registry();
    let ctx = parameters_ctx();

    let result = registry
        .call_tool("reify_get_parameters", serde_json::json!({}), &ctx)
        .expect("should succeed");

    assert_eq!(result.as_array().unwrap().len(), 3);
}

#[test]
fn get_parameters_filters_by_entity_path_prefix() {
    let registry = setup_registry();
    let ctx = parameters_ctx();

    let result = registry
        .call_tool(
            "reify_get_parameters",
            serde_json::json!({"entity_path": "sketch1"}),
            &ctx,
        )
        .expect("should succeed");

    let params = result.as_array().unwrap();
    assert_eq!(params.len(), 2);
    assert_eq!(params[0]["name"], "width");
    assert_eq!(params[1]["name"], "height");
}

#[test]
fn get_parameters_returns_empty_when_prefix_matches_nothing() {
    let registry = setup_registry();
    let ctx = parameters_ctx();

    let result = registry
        .call_tool(
            "reify_get_parameters",
            serde_json::json!({"entity_path": "nonexistent"}),
            &ctx,
        )
        .expect("should succeed");

    assert_eq!(result.as_array().unwrap().len(), 0);
}

// === reify_get_constraints ===

fn constraints_ctx() -> MockToolContext {
    MockToolContext {
        constraints: vec![
            ConstraintInfo {
                node_id: "n1".to_string(),
                expression: "x > 0".to_string(),
                status: "satisfied".to_string(),
                label: Some("positive".to_string()),
                parameter_ids: vec!["c1".to_string()],
            },
            ConstraintInfo {
                node_id: "n2".to_string(),
                expression: "y < 100".to_string(),
                status: "satisfied".to_string(),
                label: None,
                parameter_ids: vec!["c2".to_string()],
            },
            ConstraintInfo {
                node_id: "n3".to_string(),
                expression: "x + y = 50".to_string(),
                status: "violated".to_string(),
                label: Some("sum".to_string()),
                parameter_ids: vec!["c1".to_string(), "c2".to_string()],
            },
        ],
        ..Default::default()
    }
}

#[test]
fn get_constraints_returns_all_with_no_filter() {
    let registry = setup_registry();
    let ctx = constraints_ctx();

    let result = registry
        .call_tool("reify_get_constraints", serde_json::json!({}), &ctx)
        .expect("should succeed");

    assert_eq!(result.as_array().unwrap().len(), 3);
}

#[test]
fn get_constraints_filters_by_status_satisfied() {
    let registry = setup_registry();
    let ctx = constraints_ctx();

    let result = registry
        .call_tool(
            "reify_get_constraints",
            serde_json::json!({"status": "satisfied"}),
            &ctx,
        )
        .expect("should succeed");

    let cons = result.as_array().unwrap();
    assert_eq!(cons.len(), 2);
    for c in cons {
        assert_eq!(c["status"], "satisfied");
    }
}

#[test]
fn get_constraints_filters_by_status_violated() {
    let registry = setup_registry();
    let ctx = constraints_ctx();

    let result = registry
        .call_tool(
            "reify_get_constraints",
            serde_json::json!({"status": "violated"}),
            &ctx,
        )
        .expect("should succeed");

    let cons = result.as_array().unwrap();
    assert_eq!(cons.len(), 1);
    assert_eq!(cons[0]["status"], "violated");
}

#[test]
fn get_constraints_returns_empty_when_status_matches_nothing() {
    let registry = setup_registry();
    let ctx = constraints_ctx();

    let result = registry
        .call_tool(
            "reify_get_constraints",
            serde_json::json!({"status": "indeterminate"}),
            &ctx,
        )
        .expect("should succeed");

    assert_eq!(result.as_array().unwrap().len(), 0);
}

// === reify_get_eval_status ===

#[test]
fn get_eval_status_returns_phase_progress_dirty_count() {
    let registry = setup_registry();
    let ctx = MockToolContext {
        eval_status: EvalStatusInfo {
            phase: "evaluating".to_string(),
            progress: Some(0.5),
            dirty_count: 3,
        },
        ..Default::default()
    };

    let result = registry
        .call_tool("reify_get_eval_status", serde_json::json!({}), &ctx)
        .expect("should succeed");

    assert_eq!(result["phase"], "evaluating");
    assert_eq!(result["progress"], 0.5);
    assert_eq!(result["dirty_count"], 3);
}

// === reify_get_selection ===

#[test]
fn get_selection_returns_selected_and_hovered() {
    let registry = setup_registry();
    let ctx = MockToolContext {
        selection: SelectionInfo {
            selected_entity: Some("bracket/body".to_string()),
            hovered_entity: Some("bracket/fillet1".to_string()),
            selected_entities: vec![],
        },
        ..Default::default()
    };

    let result = registry
        .call_tool("reify_get_selection", serde_json::json!({}), &ctx)
        .expect("should succeed");

    assert_eq!(result["selected_entity"], "bracket/body");
    assert_eq!(result["hovered_entity"], "bracket/fillet1");
}

#[test]
fn get_selection_returns_nulls_when_nothing_selected() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry
        .call_tool("reify_get_selection", serde_json::json!({}), &ctx)
        .expect("should succeed");

    assert!(result["selected_entity"].is_null());
    assert!(result["hovered_entity"].is_null());
    // selected_entities defaults to an empty JSON array
    assert_eq!(result["selected_entities"], serde_json::json!([]));
}

#[test]
fn get_selection_returns_selected_entities_list() {
    let registry = setup_registry();
    let ctx = MockToolContext {
        selection: SelectionInfo {
            selected_entity: Some("a".to_string()),
            hovered_entity: None,
            selected_entities: vec!["a".to_string(), "b".to_string()],
        },
        ..Default::default()
    };

    let result = registry
        .call_tool("reify_get_selection", serde_json::json!({}), &ctx)
        .expect("should succeed");

    assert_eq!(result["selected_entity"], "a");
    assert_eq!(result["selected_entities"], serde_json::json!(["a", "b"]));
}

// === reify_get_source_location ===

#[test]
fn get_source_location_returns_location_for_existing_entity() {
    let registry = setup_registry();
    let mut locations = std::collections::HashMap::new();
    locations.insert(
        "bracket/body".to_string(),
        SourceLocationInfo {
            file_path: "main.ri".to_string(),
            line: 5,
            column: 3,
            end_line: 20,
            end_column: 1,
        },
    );
    let ctx = MockToolContext {
        source_locations: locations,
        ..Default::default()
    };

    let result = registry
        .call_tool(
            "reify_get_source_location",
            serde_json::json!({"entity_path": "bracket/body"}),
            &ctx,
        )
        .expect("should succeed");

    assert_eq!(result["file_path"], "main.ri");
    assert_eq!(result["line"], 5);
    assert_eq!(result["column"], 3);
    assert_eq!(result["end_line"], 20);
    assert_eq!(result["end_column"], 1);
}

#[test]
fn get_source_location_returns_null_for_nonexistent_entity() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry
        .call_tool(
            "reify_get_source_location",
            serde_json::json!({"entity_path": "nonexistent/entity"}),
            &ctx,
        )
        .expect("should succeed");

    assert!(result.is_null());
}

#[test]
fn get_source_location_returns_error_when_entity_path_missing() {
    let registry = setup_registry();
    let ctx = MockToolContext::default();

    let result = registry.call_tool("reify_get_source_location", serde_json::json!({}), &ctx);

    assert!(result.is_err());
    match result {
        Err(reify_mcp::types::ToolError::InvalidParams(_)) => {} // expected
        other => panic!("expected InvalidParams, got: {other:?}"),
    }
}
