use std::io::{BufRead, BufReader, Read as _, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

/// Global mutex that serializes LSP protocol tests.
///
/// Each test spawns a long-running `reify lsp` child process with a tokio
/// runtime. Running two such processes concurrently inside the same test
/// binary — especially during a full `cargo test -p reify-cli` run with many
/// parallel test binaries — can starve one process's runtime and cause the
/// 10-second `wait_for_response` timeout to fire. Holding this lock for the
/// lifetime of each test ensures at most one LSP process is active at a time
/// from this binary.
static LSP_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Acquire the global LSP test serialization lock.
///
/// Uses `unwrap_or_else(|e| e.into_inner())` instead of `unwrap()` so that a
/// poisoned mutex (from a prior test that panicked while holding the lock —
/// see esc-1672-40) does not cascade into a `PoisonError` panic in subsequent
/// tests. The lock guards `()` (unit type), so there is no inconsistent state
/// to worry about; silent recovery is strictly better than propagating the
/// poison. This pattern is used at 14+ other sites in the codebase
/// (priority_promotion.rs, concurrent.rs, concurrent_eval.rs, diff.rs, mocks.rs).
fn acquire_lsp_test_lock() -> std::sync::MutexGuard<'static, ()> {
    LSP_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

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

/// Read all JSON-RPC messages from stdout in a background thread.
/// Returns a receiver that collects all messages.
/// This prevents the server from blocking on stdout when it sends notifications.
fn spawn_reader(stdout: std::process::ChildStdout) -> mpsc::Receiver<serde_json::Value> {
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
                && tx.send(json).is_err()
            {
                return;
            }
        }
    });
    rx
}

/// Verify that `acquire_lsp_test_lock()` recovers from a poisoned mutex rather
/// than propagating the `PoisonError` as a panic.
///
/// Regression test for esc-1672-40: a timed-out LSP test that held the lock
/// poisoned the mutex, causing all subsequent LSP tests to fail with an opaque
/// `PoisonError` cascade. With `.lock().unwrap()` the second acquisition below
/// panics; with `.lock().unwrap_or_else(|e| e.into_inner())` it succeeds.
///
/// ## Why a local mirror mutex?
///
/// This test cannot poison `LSP_TEST_LOCK` directly without causing intermittent
/// timeouts in the other LSP tests (esc-1685-81).  When `LSP_TEST_LOCK` is poisoned
/// and multiple test threads race to recover it, OS scheduling non-determinism
/// occasionally starves the second LSP child process long enough to hit the
/// 30-second `wait_for_response` timeout.  The fix is to:
///   1. Hold `LSP_TEST_LOCK` for the whole test so this function is fully
///      serialised with the other LSP tests (no concurrent LSP process running).
///   2. Test the poison-recovery idiom on `POISON_TEST_LOCK` — a static
///      `OnceLock<Mutex<()>>` with exactly the same structure — without ever
///      polluting the global LSP lock.
///
/// The idiom under test (`unwrap_or_else(|e| e.into_inner())`) is identical;
/// only the mutex instance differs.
#[test]
fn acquire_lsp_test_lock_recovers_from_poisoned_mutex() {
    // Hold the global LSP lock for the duration to prevent this test from
    // running concurrently with the LSP process tests.
    let _global_lock = acquire_lsp_test_lock();

    // Local mirror: same OnceLock<Mutex<()>> structure as LSP_TEST_LOCK.
    static POISON_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    // Spawn a thread that acquires the mirror lock and panics, poisoning it.
    let handle = thread::spawn(|| {
        let _guard = POISON_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap(); // .unwrap() here is intentional: we *want* it to poison
        panic!("intentional poison to simulate a test crash");
    });

    // Confirm the thread panicked while holding the lock.
    assert!(
        handle.join().is_err(),
        "spawned thread should have panicked while holding the lock"
    );

    // Acquiring the now-poisoned mirror lock must not panic.
    // With .lock().unwrap() this line panics (PoisonError); with
    // .lock().unwrap_or_else(|e| e.into_inner()) it succeeds.
    // This is the exact idiom used inside acquire_lsp_test_lock().
    let _guard = POISON_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
}

/// Wait until we receive a response with the given id from the message stream.
///
/// Uses a 30-second timeout to accommodate CPU saturation when many test
/// binaries run in parallel (e.g., during `cargo test --workspace`).  Under
/// heavy load the spawned tokio runtime may not be scheduled for several
/// seconds before it can process the `initialize` request; 30 s gives ample
/// headroom without making genuinely failing tests unreasonably slow.
fn wait_for_response(rx: &mpsc::Receiver<serde_json::Value>, id: u64) -> serde_json::Value {
    let timeout = std::time::Duration::from_secs(30);
    loop {
        match rx.recv_timeout(timeout) {
            Ok(msg) => {
                if msg.get("id").and_then(|v| v.as_u64()) == Some(id) {
                    return msg;
                }
                // Otherwise it's a notification (e.g. publishDiagnostics), skip it
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!("timed out after 30s waiting for response with id={id}")
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!(
                    "reader thread disconnected (LSP process may have crashed) \
                     while waiting for response with id={id}"
                )
            }
        }
    }
}

#[test]
fn lsp_full_interactive_loop_through_binary() {
    let _lock = acquire_lsp_test_lock();
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
    // Verify textDocumentSync capability is present (canonical assertion migrated
    // from lsp_initialize_returns_capabilities, which was removed because it ran as
    // a second subprocess test and was intermittently flaky under CPU load; all
    // protocol coverage now lives in this single reliable test).
    let capabilities = &init_response["result"]["capabilities"];
    assert!(
        !capabilities["textDocumentSync"].is_null(),
        "initialize response should include textDocumentSync capability, got: {}",
        serde_json::to_string_pretty(&init_response).unwrap()
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
    param width: Length = 80mm
    param height: Length = 100mm
    param thickness: Length = 5mm
    param fillet_radius: Length = 3mm
    param hole_diameter: Length = 6mm

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
        "param thickness: Length = 5mm",
        "param thickness: Length = 1mm",
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
