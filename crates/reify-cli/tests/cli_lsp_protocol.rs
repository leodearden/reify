use std::io::{BufRead, BufReader, Read as _, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Send a JSON-RPC message with Content-Length header framing.
fn send_jsonrpc(stdin: &mut impl Write, body: &str) {
    let msg = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    stdin.write_all(msg.as_bytes()).expect("write to stdin");
    stdin.flush().expect("flush stdin");
}

/// Wait for a child process to exit with a timeout.
/// Panics with a clear message if the deadline expires instead of hanging CI.
fn wait_for_exit(child: &mut Child, timeout_secs: u64) -> ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait().expect("try_wait failed") {
            Some(status) => return status,
            None => {
                assert!(
                    Instant::now() < deadline,
                    "child process did not exit within {timeout_secs}s"
                );
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
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

    // Use spawn_reader + wait_for_response to handle notifications and timeouts
    let rx = spawn_reader(stdout);

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

    // Read initialize response (filters by id, skips notifications, has timeout)
    let response = wait_for_response(&rx, 1);

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

    // Read shutdown response (filters by id=2, skips notifications, has timeout)
    let _shutdown_response = wait_for_response(&rx, 2);

    // Send exit notification
    let exit = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null
    });
    send_jsonrpc(&mut stdin, &exit.to_string());

    // Drop stdin to signal EOF, then wait for exit with timeout
    drop(stdin);

    let status = wait_for_exit(&mut child, 10);
    assert!(
        status.success(),
        "reify lsp should exit cleanly after shutdown+exit"
    );
}

/// Read all JSON-RPC messages from stdout in a background thread.
/// Returns a receiver that collects all messages.
/// This prevents the server from blocking on stdout when it sends notifications.
fn spawn_reader(
    stdout: std::process::ChildStdout,
) -> mpsc::Receiver<serde_json::Value> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            // Try to read Content-Length header
            let mut content_length: usize = 0;
            let mut found_header = false;
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => return, // EOF
                    Ok(_) => {}
                    Err(_) => return,
                }
                let trimmed = line.trim_end();
                if trimmed.is_empty() {
                    if found_header {
                        break;
                    }
                    continue;
                }
                if let Some(val) = trimmed.strip_prefix("Content-Length: ") {
                    content_length = val.parse().unwrap_or(0);
                    found_header = true;
                }
            }
            if content_length == 0 {
                continue;
            }
            let mut body = vec![0u8; content_length];
            if reader.read_exact(&mut body).is_err() {
                return;
            }
            if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&body)
                && tx.send(json).is_err() {
                    return;
                }
        }
    });
    rx
}

/// Wait until we receive a response with the given id from the message stream.
fn wait_for_response(rx: &mpsc::Receiver<serde_json::Value>, id: u64) -> serde_json::Value {
    let timeout = std::time::Duration::from_secs(10);
    loop {
        match rx.recv_timeout(timeout) {
            Ok(msg) => {
                if msg.get("id").and_then(|v| v.as_u64()) == Some(id) {
                    return msg;
                }
                // Otherwise it's a notification (e.g. publishDiagnostics), skip it
            }
            Err(_) => panic!("timed out waiting for response with id={id}"),
        }
    }
}

#[test]
fn lsp_full_interactive_loop_through_binary() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(["lsp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn reify lsp");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");

    // Spawn a background reader to consume all messages (responses + notifications)
    let rx = spawn_reader(stdout);

    // 1) Initialize
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
    let init_response = wait_for_response(&rx, 1);
    assert!(
        init_response.get("result").is_some(),
        "initialize should return a result"
    );

    // Send initialized notification
    let initialized = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    });
    send_jsonrpc(&mut stdin, &initialized.to_string());

    // 2) didOpen with valid bracket source
    let valid_source = r#"structure Bracket {
    param width: Scalar = 80mm
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm
    param fillet_radius: Scalar = 3mm
    param hole_diameter: Scalar = 6mm

    let volume = width * height * thickness

    constraint thickness > 2mm
    constraint thickness < width / 4
    constraint hole_diameter < thickness * 2

    let body = box(width, height, thickness)
}"#;

    let did_open = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": "file:///tmp/test_bracket.ri",
                "languageId": "reify",
                "version": 1,
                "text": valid_source
            }
        }
    });
    send_jsonrpc(&mut stdin, &did_open.to_string());

    // Small delay to let the server process the notification
    std::thread::sleep(std::time::Duration::from_millis(200));

    // 3) didChange with violating source (thickness=1mm violates thickness > 2mm)
    let violating_source = valid_source.replace(
        "param thickness: Scalar = 5mm",
        "param thickness: Scalar = 1mm",
    );
    let did_change_violating = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {
                "uri": "file:///tmp/test_bracket.ri",
                "version": 2
            },
            "contentChanges": [{ "text": violating_source }]
        }
    });
    send_jsonrpc(&mut stdin, &did_change_violating.to_string());

    std::thread::sleep(std::time::Duration::from_millis(200));

    // 4) didChange back to valid source
    let did_change_valid = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {
                "uri": "file:///tmp/test_bracket.ri",
                "version": 3
            },
            "contentChanges": [{ "text": valid_source }]
        }
    });
    send_jsonrpc(&mut stdin, &did_change_valid.to_string());

    std::thread::sleep(std::time::Duration::from_millis(200));

    // 5) Shutdown + exit
    let shutdown = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown",
        "params": null
    });
    send_jsonrpc(&mut stdin, &shutdown.to_string());
    let _shutdown_response = wait_for_response(&rx, 2);

    let exit = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null
    });
    send_jsonrpc(&mut stdin, &exit.to_string());

    drop(stdin);

    let status = wait_for_exit(&mut child, 10);
    assert!(
        status.success(),
        "reify lsp should exit cleanly after full interactive loop"
    );
}
