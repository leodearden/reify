use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Helper: spawn `reify mcp-server` with the given args, send JSON-RPC lines,
/// close stdin, and read all stdout. Returns parsed JSON lines.
/// Times out after 10 seconds to prevent CI deadlocks.
fn mcp_roundtrip(args: &[&str], requests: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_reify"))
        .arg("mcp-server")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn reify mcp-server");

    // Write all requests to stdin, then close it
    {
        let stdin = child.stdin.as_mut().expect("failed to open stdin");
        for req in requests {
            let line = format!("{}\n", req);
            stdin
                .write_all(line.as_bytes())
                .expect("failed to write to stdin");
        }
    }
    // Drop stdin by closing it
    drop(child.stdin.take());

    // Wait with timeout to prevent CI deadlocks.
    // 30s headroom: task 4503 raised the OCCT nextest max-threads cap 4->24
    // (commit 9ea8cdd4b8), so under peak concurrent load the child's stdlib
    // compile (parse_with_stdlib for bracket.ri) can exceed the old 10s ceiling
    // even though the MCP roundtrip itself is fast. This guards deadlocks, not
    // performance, so a wider ceiling is safe.
    let timeout = Duration::from_secs(30);
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => {
                if start.elapsed() > timeout {
                    child.kill().ok();
                    panic!("mcp_roundtrip: child process timed out after {timeout:?}");
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("mcp_roundtrip: error waiting for child: {e}"),
        }
    }

    let output = child
        .wait_with_output()
        .expect("failed to collect output from reify mcp-server");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse each non-empty line as JSON
    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str(l).unwrap_or_else(|e| panic!("bad JSON line: {e}\nline: {l}"))
        })
        .collect()
}

#[test]
fn mcp_server_tools_list_includes_core_tools() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bracket.ri");

    let requests = vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    ];

    let responses = mcp_roundtrip(&[fixture], &requests);

    assert!(
        responses.len() >= 2,
        "expected at least 2 responses, got {}",
        responses.len()
    );

    // Second response should be tools/list
    let tools_response = &responses[1];
    let tools = tools_response["result"]["tools"]
        .as_array()
        .expect("tools/list should have result.tools array");

    let tool_names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().unwrap_or("?"))
        .collect();

    // Minimum count to catch catastrophic regressions
    assert!(
        tools.len() >= 16,
        "expected at least 16 tools, got {}: {:?}",
        tools.len(),
        tool_names
    );

    // Core tools exercised by other tests in this file
    let core_tools = [
        "reify_get_source",
        "reify_get_parameters",
        "reify_set_parameter",
        "reify_update_source",
        "reify_get_constraints",
        "reify_language_reference",
    ];
    for core in &core_tools {
        assert!(
            tool_names.contains(core),
            "core tool '{}' missing from tools/list, got: {:?}",
            core,
            tool_names
        );
    }
}

#[test]
fn mcp_server_language_reference_returns_content() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bracket.ri");

    let requests = vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "reify_language_reference",
                "arguments": {"topic": "geometry"}
            }
        }),
    ];

    let responses = mcp_roundtrip(&[fixture], &requests);
    assert!(responses.len() >= 2, "expected at least 2 responses");

    let call_response = &responses[1];
    assert_eq!(
        call_response["result"]["isError"],
        Value::Bool(false),
        "language_reference should not return error: {:?}",
        call_response
    );

    let content = call_response["result"]["content"]
        .as_array()
        .expect("should have content array");
    assert!(!content.is_empty(), "content should not be empty");

    let text = content[0]["text"]
        .as_str()
        .expect("content[0].text should be a string");
    // Geometry-related keywords from the language reference
    let has_geometry_keyword = text.contains("box")
        || text.contains("cylinder")
        || text.contains("geometry")
        || text.contains("Geometry");
    assert!(
        has_geometry_keyword,
        "language reference for 'geometry' should contain geometry keywords, got: {}",
        &text[..text.len().min(200)]
    );
}

#[test]
fn mcp_server_get_parameters_returns_bracket_params() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bracket.ri");

    let requests = vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "reify_get_parameters",
                "arguments": {}
            }
        }),
    ];

    let responses = mcp_roundtrip(&[fixture], &requests);
    assert!(responses.len() >= 2, "expected at least 2 responses");

    let call_response = &responses[1];
    assert_eq!(
        call_response["result"]["isError"],
        Value::Bool(false),
        "get_parameters should not return error: {:?}",
        call_response
    );

    let content = call_response["result"]["content"]
        .as_array()
        .expect("should have content array");
    let text = content[0]["text"]
        .as_str()
        .expect("content[0].text should be a string");
    let params: Vec<serde_json::Value> =
        serde_json::from_str(text).expect("content should be JSON array of parameters");

    // bracket.ri has 5 params: width, height, thickness, fillet_radius, hole_diameter
    let expected_names = [
        "width",
        "height",
        "thickness",
        "fillet_radius",
        "hole_diameter",
    ];
    let param_names: Vec<&str> = params
        .iter()
        .filter(|p| p["kind"].as_str() == Some("Param"))
        .map(|p| p["name"].as_str().unwrap_or("?"))
        .collect();

    for expected in &expected_names {
        assert!(
            param_names.contains(expected),
            "expected parameter '{}' in params, got: {:?}",
            expected,
            param_names
        );
    }

    // Verify each has a cell_id containing "Bracket."
    for param in &params {
        if expected_names.contains(&param["name"].as_str().unwrap_or("")) {
            let cell_id = param["cell_id"].as_str().unwrap_or("");
            assert!(
                cell_id.contains("Bracket."),
                "cell_id should contain 'Bracket.', got: {cell_id}"
            );
            let value = param["value"].as_str().unwrap_or("");
            assert!(
                !value.is_empty(),
                "parameter {} should have a non-empty value",
                param["name"]
            );
        }
    }
}

#[test]
fn mcp_server_set_parameter_changes_value() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bracket.ri");

    let requests = vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        // Set width to 0.1 m (100mm in SI)
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "reify_set_parameter",
                "arguments": {"cell_id": "Bracket.width", "value": "0.1"}
            }
        }),
        // Get parameters to verify the change
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "reify_get_parameters",
                "arguments": {}
            }
        }),
    ];

    let responses = mcp_roundtrip(&[fixture], &requests);
    assert!(responses.len() >= 3, "expected at least 3 responses");

    // set_parameter response
    let set_response = &responses[1];
    assert_eq!(
        set_response["result"]["isError"],
        Value::Bool(false),
        "set_parameter should not return error: {:?}",
        set_response
    );

    let set_content = set_response["result"]["content"]
        .as_array()
        .expect("should have content array");
    let set_text = set_content[0]["text"]
        .as_str()
        .expect("content[0].text should be string");
    let set_result: serde_json::Value =
        serde_json::from_str(set_text).expect("set_parameter result should be JSON");
    assert_eq!(
        set_result["success"], true,
        "set_parameter should return success=true: {:?}",
        set_result
    );
    // Verify set_parameter response itself reports the correct new value
    assert_eq!(
        set_result["new_value"].as_str().unwrap_or(""),
        "0.1 m",
        "set_parameter response new_value should be '0.1 m', got: {:?}",
        set_result["new_value"]
    );

    // get_parameters response — verify width changed
    let get_response = &responses[2];
    let get_content = get_response["result"]["content"]
        .as_array()
        .expect("should have content array");
    let get_text = get_content[0]["text"].as_str().expect("should be string");
    let params: Vec<serde_json::Value> =
        serde_json::from_str(get_text).expect("should be JSON array");

    let width_param = params
        .iter()
        .find(|p| p["name"].as_str() == Some("width"))
        .expect("should have width parameter");

    let width_value = width_param["value"].as_str().unwrap_or("");
    // Original was 80mm (0.08 m SI), new should be 0.1 m (100mm) after setting to 0.1
    assert_eq!(
        width_value, "0.1 m",
        "width should be 0.1 m (100mm) after setting to 0.1"
    );
}

#[test]
fn mcp_server_update_source_invalid_preserves_state() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bracket.ri");
    // Compute the canonical path the server will use as the file key
    let abs_fixture = std::fs::canonicalize(fixture)
        .expect("fixture should exist")
        .to_string_lossy()
        .to_string();

    // Read original bracket.ri content for comparison
    let original_content = std::fs::read_to_string(fixture).expect("fixture should be readable");

    let requests = vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        // Send invalid source (missing enum name triggers parse error) via update_source
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "reify_update_source",
                "arguments": {
                    "file_path": abs_fixture,
                    "content": "enum { }"
                }
            }
        }),
        // Get source — should still return the original bracket.ri content
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "reify_get_source",
                "arguments": {}
            }
        }),
        // Get parameters — should still return the original bracket.ri params
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "reify_get_parameters",
                "arguments": {}
            }
        }),
    ];

    let responses = mcp_roundtrip(&[fixture], &requests);
    assert!(responses.len() >= 4, "expected at least 4 responses");

    // get_source should return the ORIGINAL content, not the broken "enum { }"
    // This is the key assertion: if update_source mutates files before validation,
    // get_source will return the broken content instead of the original.
    let source_response = &responses[2];
    assert_eq!(
        source_response["result"]["isError"],
        Value::Bool(false),
        "get_source should not return error after failed update: {:?}",
        source_response
    );

    let source_content = source_response["result"]["content"]
        .as_array()
        .expect("should have content array");
    let source_text = source_content[0]["text"]
        .as_str()
        .expect("content[0].text should be string");
    // Parse get_source result (it returns JSON with content and file_path)
    let source_result: serde_json::Value =
        serde_json::from_str(source_text).expect("get_source result should be JSON");
    let returned_content = source_result["content"]
        .as_str()
        .expect("should have content field");
    assert_eq!(
        returned_content.trim(),
        original_content.trim(),
        "get_source should return original bracket.ri content after failed update_source, \
         not the broken content"
    );

    // get_parameters should still return original bracket.ri parameters
    let get_response = &responses[3];
    assert_eq!(
        get_response["result"]["isError"],
        Value::Bool(false),
        "get_parameters should not return error after failed update: {:?}",
        get_response
    );

    let get_content = get_response["result"]["content"]
        .as_array()
        .expect("should have content array");
    let get_text = get_content[0]["text"].as_str().expect("should be string");
    let params: Vec<serde_json::Value> =
        serde_json::from_str(get_text).expect("should be JSON array of parameters");

    // All 5 original params should be present
    let expected_names = [
        "width",
        "height",
        "thickness",
        "fillet_radius",
        "hole_diameter",
    ];
    let param_names: Vec<&str> = params
        .iter()
        .filter(|p| p["kind"].as_str() == Some("Param"))
        .map(|p| p["name"].as_str().unwrap_or("?"))
        .collect();

    for expected in &expected_names {
        assert!(
            param_names.contains(expected),
            "after failed update_source, parameter '{}' should still be present, got: {:?}",
            expected,
            param_names
        );
    }

    // Width should still be the original value (0.08 m)
    let width_param = params
        .iter()
        .find(|p| p["name"].as_str() == Some("width"))
        .expect("should have width parameter");
    let width_value = width_param["value"].as_str().unwrap_or("");
    assert_eq!(
        width_value, "0.08 m",
        "width should still be original 0.08 m after failed update_source"
    );
}

#[test]
fn mcp_server_set_parameter_reports_new_value_accurately() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bracket.ri");

    let requests = vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        // Set width to 0.2 m (200mm in SI)
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "reify_set_parameter",
                "arguments": {"cell_id": "Bracket.width", "value": "0.2"}
            }
        }),
    ];

    let responses = mcp_roundtrip(&[fixture], &requests);
    assert!(responses.len() >= 2, "expected at least 2 responses");

    // set_parameter response should report success
    let set_response = &responses[1];
    assert_eq!(
        set_response["result"]["isError"],
        Value::Bool(false),
        "set_parameter should not return error: {:?}",
        set_response
    );

    let set_content = set_response["result"]["content"]
        .as_array()
        .expect("should have content array");
    let set_text = set_content[0]["text"]
        .as_str()
        .expect("content[0].text should be string");
    let set_result: serde_json::Value =
        serde_json::from_str(set_text).expect("set_parameter result should be JSON");

    assert_eq!(
        set_result["success"], true,
        "set_parameter should return success=true: {:?}",
        set_result
    );

    // Verify the new_value field accurately reports the post-edit value.
    // This validates that edit_param result is used to confirm the value was applied,
    // rather than blindly reporting success.
    assert_eq!(
        set_result["new_value"].as_str().unwrap_or(""),
        "0.2 m",
        "set_parameter new_value should be '0.2 m' after setting width to 0.2, got: {:?}",
        set_result["new_value"]
    );
}

#[test]
fn mcp_server_set_parameter_constraint_verified_after_change() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bracket.ri");

    let requests = vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        // Set width to 0.01 m (10mm in SI). This violates: thickness < width/4
        // because thickness=5mm and width/4=2.5mm, so 5mm < 2.5mm is false.
        // Constraints are soft, so set_parameter still returns success=true.
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "reify_set_parameter",
                "arguments": {"cell_id": "Bracket.width", "value": "0.01"}
            }
        }),
        // Get parameters to verify the new width is applied
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "reify_get_parameters",
                "arguments": {}
            }
        }),
        // Get constraints to verify the constraint evaluator still runs at correct SI scale
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "reify_get_constraints",
                "arguments": {}
            }
        }),
    ];

    let responses = mcp_roundtrip(&[fixture], &requests);
    assert!(responses.len() >= 4, "expected at least 4 responses");

    // set_parameter should return success=true (constraints are soft, not blocking)
    let set_response = &responses[1];
    assert_eq!(
        set_response["result"]["isError"],
        Value::Bool(false),
        "set_parameter should not return error even with constraint violation: {:?}",
        set_response
    );
    let set_content = set_response["result"]["content"]
        .as_array()
        .expect("should have content array");
    let set_text = set_content[0]["text"]
        .as_str()
        .expect("content[0].text should be string");
    let set_result: serde_json::Value =
        serde_json::from_str(set_text).expect("set_parameter result should be JSON");
    assert_eq!(
        set_result["success"], true,
        "set_parameter should return success=true even with constraint violation: {:?}",
        set_result
    );

    // get_parameters should show width = "0.01 m"
    let get_response = &responses[2];
    let get_content = get_response["result"]["content"]
        .as_array()
        .expect("should have content array");
    let get_text = get_content[0]["text"].as_str().expect("should be string");
    let params: Vec<serde_json::Value> =
        serde_json::from_str(get_text).expect("should be JSON array of parameters");
    let width_param = params
        .iter()
        .find(|p| p["name"].as_str() == Some("width"))
        .expect("should have width parameter");
    assert_eq!(
        width_param["value"].as_str().unwrap_or(""),
        "0.01 m",
        "width should be 0.01 m after setting to 0.01, got: {:?}",
        width_param["value"]
    );

    // get_constraints should return all 3 bracket constraints (confirming constraint evaluator runs)
    let constraints_response = &responses[3];
    assert_eq!(
        constraints_response["result"]["isError"],
        Value::Bool(false),
        "get_constraints should not return error: {:?}",
        constraints_response
    );
    let constraints_content = constraints_response["result"]["content"]
        .as_array()
        .expect("should have content array");
    let constraints_text = constraints_content[0]["text"]
        .as_str()
        .expect("content[0].text should be string");
    let constraints: Vec<serde_json::Value> =
        serde_json::from_str(constraints_text).expect("constraints result should be JSON array");
    assert_eq!(
        constraints.len(),
        3,
        "bracket.ri has 3 constraints, got {}: {:?}",
        constraints.len(),
        constraints
    );
}

#[test]
fn mcp_server_set_parameter_error_preserves_state() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bracket.ri");

    let requests = vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        // Attempt to set a non-existent parameter — should return an error
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "reify_set_parameter",
                "arguments": {"cell_id": "Bracket.nonexistent", "value": "1.0"}
            }
        }),
        // Get parameters to verify state was NOT corrupted by the failed operation
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "reify_get_parameters",
                "arguments": {}
            }
        }),
    ];

    let responses = mcp_roundtrip(&[fixture], &requests);
    assert!(responses.len() >= 3, "expected at least 3 responses");

    // set_parameter with non-existent cell_id should indicate an error
    let set_response = &responses[1];
    let is_error = set_response["result"]["isError"] == true;
    let has_error_field = !set_response["error"].is_null();
    assert!(
        is_error || has_error_field,
        "set_parameter with non-existent cell_id should return an error, got: {:?}",
        set_response
    );

    // get_parameters should still return all 5 original parameters with original values
    let get_response = &responses[2];
    assert_eq!(
        get_response["result"]["isError"],
        Value::Bool(false),
        "get_parameters should not return error after failed set_parameter: {:?}",
        get_response
    );

    let get_content = get_response["result"]["content"]
        .as_array()
        .expect("should have content array");
    let get_text = get_content[0]["text"].as_str().expect("should be string");
    let params: Vec<serde_json::Value> =
        serde_json::from_str(get_text).expect("should be JSON array of parameters");

    // All 5 original params should be present with original values
    let expected = [
        ("width", "0.08 m"),
        ("height", "0.1 m"),
        ("thickness", "0.005 m"),
        ("fillet_radius", "0.003 m"),
        ("hole_diameter", "0.006 m"),
    ];
    for (name, original_value) in &expected {
        let param = params
            .iter()
            .find(|p| p["name"].as_str() == Some(name))
            .unwrap_or_else(|| {
                panic!(
                    "parameter '{}' should be present after failed set_parameter",
                    name
                )
            });
        assert_eq!(
            param["value"].as_str().unwrap_or(""),
            *original_value,
            "parameter '{}' should retain original value '{}' after failed set_parameter, got: {:?}",
            name,
            original_value,
            param["value"]
        );
    }
}

#[test]
fn mcp_server_get_parameters_distinguishes_auto_free_kind() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/auto_kinds.ri");

    let requests = vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "reify_get_parameters",
                "arguments": {}
            }
        }),
    ];

    let responses = mcp_roundtrip(&[fixture], &requests);
    assert!(responses.len() >= 2, "expected at least 2 responses");

    let call_response = &responses[1];
    assert_eq!(
        call_response["result"]["isError"],
        Value::Bool(false),
        "get_parameters should not return error: {:?}",
        call_response
    );

    let content = call_response["result"]["content"]
        .as_array()
        .expect("should have content array");
    let text = content[0]["text"]
        .as_str()
        .expect("content[0].text should be a string");
    let params: Vec<serde_json::Value> =
        serde_json::from_str(text).expect("content should be JSON array of parameters");
    assert_eq!(params.len(), 3, "expected exactly 3 parameters");

    // Locate the three params by name
    let width_param = params
        .iter()
        .find(|p| p["name"].as_str() == Some("width"))
        .expect("should have 'width' parameter");
    let tolerance_param = params
        .iter()
        .find(|p| p["name"].as_str() == Some("tolerance"))
        .expect("should have 'tolerance' parameter");
    let offset_param = params
        .iter()
        .find(|p| p["name"].as_str() == Some("offset"))
        .expect("should have 'offset' parameter");

    // width is a regular param
    assert_eq!(
        width_param["kind"].as_str().unwrap_or(""),
        "Param",
        "width should have kind 'Param', got: {:?}",
        width_param["kind"]
    );

    // tolerance is `auto` → kind should be "Auto"
    assert_eq!(
        tolerance_param["kind"].as_str().unwrap_or(""),
        "Auto",
        "tolerance (auto) should have kind 'Auto', got: {:?}",
        tolerance_param["kind"]
    );

    // offset is `auto(free)` → kind should be "Auto(free)"
    assert_eq!(
        offset_param["kind"].as_str().unwrap_or(""),
        "Auto(free)",
        "offset (auto(free)) should have kind 'Auto(free)', got: {:?}",
        offset_param["kind"]
    );
}

/// MCP-protocol-level regression guard for the unified get_source_location semantics.
///
/// Sends two tools/call requests through the full JSON-RPC pipeline:
/// 1. `{"entity_path": "Bracket"}` — plain template name
/// 2. `{"entity_path": "Bracket.width"}` — full cell ID
///
/// Both must return a non-error SourceLocationInfo JSON with:
/// - `file_path` containing "bracket.ri"
/// - `line >= 1`, `column >= 1`, `end_line >= line`, `end_column >= 1`
///
/// Additionally, the two responses must have identical `(line, column, end_line,
/// end_column)` tuples — since "Bracket" proxies to the first value cell (width).
///
/// This test passes against pre-step-6 CLI code (the old CLI already accepted
/// both forms) and serves as the regression guard ensuring step-6's refactor
/// to the shared helper preserves identical behaviour.
#[test]
fn mcp_server_get_source_location_accepts_template_name_and_cell_id() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bracket.ri");

    let requests = vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        // (1) plain template name
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "reify_get_source_location",
                "arguments": {"entity_path": "Bracket"}
            }
        }),
        // (2) full cell ID
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "reify_get_source_location",
                "arguments": {"entity_path": "Bracket.width"}
            }
        }),
    ];

    let responses = mcp_roundtrip(&[fixture], &requests);
    assert!(
        responses.len() >= 3,
        "expected at least 3 responses, got {}",
        responses.len()
    );

    // Helper: parse the SourceLocationInfo from a tools/call response.
    let parse_loc = |resp: &Value, label: &str| -> Value {
        assert_eq!(
            resp["result"]["isError"],
            Value::Bool(false),
            "{label}: expected isError=false, got {:?}",
            resp
        );
        let content = resp["result"]["content"]
            .as_array()
            .unwrap_or_else(|| panic!("{label}: expected content array"));
        let text = content[0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{label}: expected content[0].text string"));
        serde_json::from_str(text).unwrap_or_else(|e| {
            panic!("{label}: content[0].text is not valid JSON: {e}\nraw: {text}")
        })
    };

    let loc_name = parse_loc(&responses[1], "entity_path='Bracket'");
    let loc_width = parse_loc(&responses[2], "entity_path='Bracket.width'");

    // Both must resolve to a real location.
    for (loc, label) in [(&loc_name, "Bracket"), (&loc_width, "Bracket.width")] {
        let file_path = loc["file_path"].as_str().unwrap_or("");
        assert!(
            file_path.contains("bracket.ri"),
            "{label}: file_path must contain 'bracket.ri', got: {file_path}"
        );
        let line = loc["line"].as_u64().unwrap_or(0);
        assert!(line >= 1, "{label}: line must be >= 1, got {line}");
        let column = loc["column"].as_u64().unwrap_or(0);
        assert!(column >= 1, "{label}: column must be >= 1, got {column}");
        let end_line = loc["end_line"].as_u64().unwrap_or(0);
        assert!(
            end_line >= line,
            "{label}: end_line ({end_line}) must be >= line ({line})"
        );
        let end_column = loc["end_column"].as_u64().unwrap_or(0);
        assert!(
            end_column >= 1,
            "{label}: end_column must be >= 1, got {end_column}"
        );
    }

    // Template-name proxy must return identical span to the first cell (width).
    assert_eq!(
        (
            loc_name["line"].as_u64(),
            loc_name["column"].as_u64(),
            loc_name["end_line"].as_u64(),
            loc_name["end_column"].as_u64(),
        ),
        (
            loc_width["line"].as_u64(),
            loc_width["column"].as_u64(),
            loc_width["end_line"].as_u64(),
            loc_width["end_column"].as_u64(),
        ),
        "template-name 'Bracket' must proxy to the first value cell (width): \
         spans must be identical.\nBracket={loc_name:?}\nBracket.width={loc_width:?}"
    );
}
