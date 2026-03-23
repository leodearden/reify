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
    // Original was 80mm (0.08 SI), new should be 100mm (0.1 SI)
    // The value is set as raw number which will be interpreted in the cell's dimension
    assert!(
        width_value != "0.08 m",
        "width should have changed from original 0.08 m, got: {width_value}"
    );
}
