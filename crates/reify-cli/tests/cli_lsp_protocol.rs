use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// Send a JSON-RPC message with Content-Length header framing.
fn send_jsonrpc(stdin: &mut impl Write, body: &str) {
    let msg = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    stdin.write_all(msg.as_bytes()).expect("write to stdin");
    stdin.flush().expect("flush stdin");
}

/// Read a JSON-RPC response from stdout (Content-Length framing).
/// Returns the parsed JSON body.
fn read_jsonrpc(reader: &mut BufReader<impl std::io::Read>) -> serde_json::Value {
    // Read headers until blank line
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).expect("read header line");
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(val) = line.strip_prefix("Content-Length: ") {
            content_length = val.parse().expect("parse content length");
        }
    }

    assert!(content_length > 0, "expected Content-Length header");

    // Read exactly content_length bytes
    let mut body = vec![0u8; content_length];
    std::io::Read::read_exact(reader, &mut body).expect("read body");
    serde_json::from_slice(&body).expect("parse JSON body")
}

#[test]
fn lsp_initialize_returns_capabilities() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["lsp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn reify lsp");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    // Send initialize request
    let init_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "capabilities": {},
            "rootUri": null
        }
    });
    send_jsonrpc(&mut stdin, &init_request.to_string());

    // Read initialize response
    let response = read_jsonrpc(&mut reader);

    // Verify capabilities include textDocumentSync
    let capabilities = &response["result"]["capabilities"];
    assert!(
        !capabilities["textDocumentSync"].is_null(),
        "initialize response should include textDocumentSync capability, got: {}",
        serde_json::to_string_pretty(&response).unwrap()
    );

    // Send initialized notification
    let initialized = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    });
    send_jsonrpc(&mut stdin, &initialized.to_string());

    // Send shutdown request
    let shutdown = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown",
        "params": null
    });
    send_jsonrpc(&mut stdin, &shutdown.to_string());

    // Read shutdown response
    let _shutdown_response = read_jsonrpc(&mut reader);

    // Send exit notification
    let exit = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null
    });
    send_jsonrpc(&mut stdin, &exit.to_string());

    // Drop stdin to signal EOF, then wait for exit
    drop(stdin);

    let status = child.wait().expect("wait for child");
    assert!(
        status.success(),
        "reify lsp should exit cleanly after shutdown+exit"
    );
}
