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
