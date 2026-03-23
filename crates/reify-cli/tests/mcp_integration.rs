use std::io::Write;
use std::process::{Command, Stdio};

/// Helper: spawn `reify mcp-server` with the given args, send JSON-RPC lines,
/// close stdin, and read all stdout. Returns parsed JSON lines.
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
            stdin.write_all(line.as_bytes()).expect("failed to write to stdin");
        }
    }
    // Drop stdin by closing it
    drop(child.stdin.take());

    // Wait with timeout
    let output = child
        .wait_with_output()
        .expect("failed to wait for reify mcp-server");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse each non-empty line as JSON
    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("bad JSON line: {e}\nline: {l}")))
        .collect()
}

#[test]
fn mcp_server_tools_list_returns_16_tools() {
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

    assert_eq!(
        tools.len(),
        16,
        "expected 16 tools, got {}: {:?}",
        tools.len(),
        tools.iter().map(|t| t["name"].as_str().unwrap_or("?")).collect::<Vec<_>>()
    );
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
    assert_ne!(
        call_response["result"]["isError"],
        true,
        "language_reference should not return error: {:?}",
        call_response
    );

    let content = call_response["result"]["content"]
        .as_array()
        .expect("should have content array");
    assert!(!content.is_empty(), "content should not be empty");

    let text = content[0]["text"].as_str().expect("content[0].text should be a string");
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
    assert_ne!(
        call_response["result"]["isError"],
        true,
        "get_parameters should not return error: {:?}",
        call_response
    );

    let content = call_response["result"]["content"]
        .as_array()
        .expect("should have content array");
    let text = content[0]["text"].as_str().expect("content[0].text should be a string");
    let params: Vec<serde_json::Value> =
        serde_json::from_str(text).expect("content should be JSON array of parameters");

    // bracket.ri has 5 params: width, height, thickness, fillet_radius, hole_diameter
    let expected_names = ["width", "height", "thickness", "fillet_radius", "hole_diameter"];
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
        // Set width to 100 (100mm = 0.1 in SI)
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "reify_set_parameter",
                "arguments": {"cell_id": "Bracket.width", "value": "100"}
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
    assert_ne!(
        set_response["result"]["isError"],
        true,
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
        "100 m",
        "set_parameter response new_value should be '100 m', got: {:?}",
        set_result["new_value"]
    );

    // get_parameters response — verify width changed
    let get_response = &responses[2];
    let get_content = get_response["result"]["content"]
        .as_array()
        .expect("should have content array");
    let get_text = get_content[0]["text"].as_str().expect("should be string");
    let params: Vec<serde_json::Value> = serde_json::from_str(get_text).expect("should be JSON array");

    let width_param = params
        .iter()
        .find(|p| p["name"].as_str() == Some("width"))
        .expect("should have width parameter");

    let width_value = width_param["value"].as_str().unwrap_or("");
    // Original was 80mm (0.08 m SI), new should be 100 m after setting to 100
    assert_eq!(
        width_value, "100 m",
        "width should be 100 m after setting to 100"
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
    assert_ne!(
        source_response["result"]["isError"],
        true,
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
    assert_ne!(
        get_response["result"]["isError"],
        true,
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
    let expected_names = ["width", "height", "thickness", "fillet_radius", "hole_diameter"];
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
        // Set width to 200
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "reify_set_parameter",
                "arguments": {"cell_id": "Bracket.width", "value": "200"}
            }
        }),
    ];

    let responses = mcp_roundtrip(&[fixture], &requests);
    assert!(responses.len() >= 2, "expected at least 2 responses");

    // set_parameter response should report success
    let set_response = &responses[1];
    assert_ne!(
        set_response["result"]["isError"],
        true,
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
        "200 m",
        "set_parameter new_value should be '200 m' after setting width to 200, got: {:?}",
        set_result["new_value"]
    );
}
