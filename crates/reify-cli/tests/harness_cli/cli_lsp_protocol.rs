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
///
/// Also joins `stderr_reader` (the background thread draining the child's
/// stderr pipe — see the load-bearing ordering comment where it is spawned)
/// and returns its captured text alongside the exit status. `reify lsp`'s
/// stderr is the only channel it uses to report startup/runtime failures, so
/// discarding it would make any future failure of this test a black box. On
/// the timeout path the child is killed and then reaped: kill() only sends
/// the signal, and reaping is what closes the stderr pipe's write end so the
/// reader thread's `read_to_end()` returns instead of blocking forever.
fn wait_for_exit(
    child: &mut Child,
    timeout_secs: u64,
    stderr_reader: thread::JoinHandle<Vec<u8>>,
) -> (ExitStatus, String) {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait().expect("try_wait failed") {
            Some(status) => {
                let stderr = drain(stderr_reader, "stderr");
                return (status, stderr);
            }
            None => {
                if Instant::now() >= deadline {
                    child.kill().ok();
                    child.wait().ok();
                    let stderr = drain(stderr_reader, "stderr");
                    panic!(
                        "child process did not exit within {timeout_secs}s\n\
                         --- child stderr ---\n{stderr}\n--- end child stderr ---"
                    );
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// Join a pipe-reader thread and render its bytes as lossy UTF-8 for a panic
/// message. Lossy rather than strict: this is diagnostic output, so a child
/// that emitted a partial multi-byte sequence before dying must still be
/// readable. Mirrors the same spawn-readers-before-write/wait, drain-lossily
/// pattern being applied to `mcp_roundtrip` in mcp_integration.rs under
/// #5389 — not a shared helper (that file has no `drain(reader, label)` of
/// its own at present; each file reimplements the pattern locally).
fn drain(reader: thread::JoinHandle<Vec<u8>>, label: &str) -> String {
    let bytes = reader
        .join()
        .unwrap_or_else(|_| panic!("{label} reader thread panicked"));
    String::from_utf8_lossy(&bytes).into_owned()
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

/// Full interactive LSP session through the real `reify lsp` binary, driven
/// over stdio with real JSON-RPC framing.
///
/// Beyond protocol coverage (initialize capabilities, didOpen, didChange
/// with violating/valid sources, shutdown/exit), this test pins the
/// stderr-drain fix for the harness's own subprocess handling: phase 4b
/// below sends a `textDocument/didChange` for a URI that was never opened,
/// with a deliberately huge (160 KiB) path. `DocumentStore::update`
/// (crates/reify-lsp/src/document.rs) returns `false` for any unknown URI,
/// which makes `did_change`'s handler (crates/reify-lsp/src/server.rs:226)
/// `eprintln!` the URI verbatim — one ~160 KiB write to the child's stderr
/// pipe, entirely client-controlled.
///
/// Measured A/B on this binary (target/debug/reify):
///   - undrained (pre-fix) shape: stderr piped but never taken/drained —
///     child does not exit within 20s; the main thread parks in
///     `wchan=pipe_write` (same signature as the stdout hang task 5389
///     root-caused).
///   - drained (post-fix) shape: reader thread spawned before the
///     write/wait phase — child exits `rc=0` in 0.05s with 163_895 bytes of
///     stderr captured.
///
/// The assertions below on the returned `stderr` are therefore load-bearing,
/// not diagnostic: they prove the drain actually ran to completion under
/// real backpressure, not just that the happy path (small/no stderr) works.
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
    let mut stderr_pipe = child.stderr.take().expect("stderr");

    // Spawn the stdout AND stderr reader threads immediately after spawn(),
    // before any stdin writes. This ordering is load-bearing (mirrors the
    // shared spawn-readers-before-write/wait pattern being applied to
    // mcp_roundtrip in mcp_integration.rs under #5389), not stylistic: a
    // child that writes enough stderr to fill its pipe buffer blocks in
    // write() and never gets around to reading stdin or exiting, so
    // draining stderr only after the write/wait phase reintroduces the
    // same deadlock class on the other pipe. The margin is far thinner
    // than the nominal 64 KiB pipe capacity suggests — once
    // fs.pipe-user-pages-soft (16384 pages) is exceeded the kernel shrinks
    // each NEW pipe to a single page, and a 24-way-parallel workspace
    // nextest run does this routinely (F_GETPIPE_SZ measured at 8192 bytes
    // mid-run). This ordering claim is enforced by tests, not just
    // documented: phase 4b below (in this test) pins the drain itself
    // under real backpressure, and
    // `wait_for_exit_timeout_branch_drains_and_reports_stderr` pins the
    // kill-then-reap-then-join ordering on the timeout path.
    let rx = spawn_reader(stdout);
    let stderr_reader = thread::spawn(move || {
        let mut buf = Vec::new();
        stderr_pipe.read_to_end(&mut buf).ok();
        buf
    });

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

    // 4b) didChange for a never-opened URI with a deliberately huge path.
    // DocumentStore::update returns false for any URI that was never opened
    // via didOpen, so did_change's handler (server.rs:226) eprintln!s the
    // URI verbatim — a ~160 KiB write to the child's stderr pipe, applying
    // the backpressure that pins the drain fix (see rustdoc above).
    let huge_uri = format!("file:///tmp/{}.ri", "a".repeat(160 * 1024));
    let did_change_unknown_uri = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {
                "uri": huge_uri,
                "version": 4
            },
            "contentChanges": [{ "text": valid_source }]
        }
    });
    send_jsonrpc(&mut stdin, &did_change_unknown_uri.to_string());

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

    // 30s deadlock/flakiness backstop for contended CI (mirrors
    // wait_for_response's CPU-saturation rationale above), not a
    // shutdown-speed assertion: a genuine hang still exceeds this bound
    // and fails, so widening it loses no discrimination.
    let (status, stderr) = wait_for_exit(&mut child, 30, stderr_reader);
    assert!(
        status.success(),
        "reify lsp should exit cleanly after full interactive loop (stderr: {stderr})"
    );

    // Non-vacuity guard: proves phase 4b's huge-URI didChange really did put
    // the child's stderr pipe under backpressure. Without this, a future
    // reify-lsp change that stops logging unknown-URI didChange calls would
    // leave this test silently pinning nothing while still passing green.
    // A failure here means the chatty-stderr trigger has moved and this
    // regression guard needs re-pointing — NOT that the drain itself broke.
    assert!(
        stderr.len() >= 128 * 1024,
        "expected >=128KiB of captured stderr from phase 4b's huge-URI didChange \
         (measured 163_895 bytes when this guard was written), got {} bytes. This means \
         the chatty-stderr trigger has moved (reify-lsp's unknown-URI didChange path no \
         longer logs ~160KiB to stderr) and this regression guard needs re-pointing — it \
         does NOT mean the stderr drain is broken. Captured stderr: {stderr}",
        stderr.len()
    );
    // Proves the captured bytes came from the intended production path
    // (server.rs's unknown-URI didChange handler) rather than incidental
    // noise. A failure here likewise means the trigger moved, not that the
    // drain broke.
    assert!(
        stderr.contains("didChange for unknown URI"),
        "expected captured stderr to contain the unknown-URI diagnostic emitted by \
         reify-lsp's did_change handler (server.rs:226). This means the chatty-stderr \
         trigger has moved and this regression guard needs re-pointing — it does NOT mean \
         the stderr drain is broken. Captured stderr: {stderr}"
    );
}

/// Pins `wait_for_exit`'s timeout branch (kill → reap → join → interpolate),
/// which has ZERO coverage from `lsp_full_interactive_loop_through_binary`
/// above: `reify lsp` always exits cleanly, so nothing ever drives the
/// deadline-exceeded path. If that path regressed, the whole harness would
/// HANG rather than fail — the worst failure mode for this file — so it is
/// pinned here with a cheap non-LSP stub instead of relying on incidental
/// coverage.
///
/// The stub is `/bin/sh -c "printf 'REIFY_6161_TIMEOUT_MARKER\n' >&2; exec
/// sleep 30"`: it writes a recognisable marker to stderr and then blocks
/// well past the 1s deadline given to `wait_for_exit`. `exec` preserves the
/// pid, so the pid `wait_for_exit` kills is exactly the process holding the
/// stderr write end, which is what guarantees the reader thread reaches
/// EOF instead of blocking forever.
///
/// `#[should_panic(expected = ...)]` matching the marker pins three things
/// at once: the timeout branch panics rather than looping forever, the
/// reader join returned rather than hanging (proving kill-then-reap ran
/// *before* the join, not after), and the child's stderr was drained and
/// interpolated into the panic message.
///
/// Measured premise (direct experiment, not assumed): the stub does not
/// exit before a 1s deadline; kill() then wait() then joining the reader
/// returns exactly "REIFY_6161_TIMEOUT_MARKER\n" in 1.01s.
///
/// Precedent: crates/reify-fdm/tests/slice.rs:326-358 and :371-400 already
/// drive `/bin/sh -c '...; exec sleep 30'` stub children from Rust tests
/// under `#[cfg(unix)]`.
///
/// Known hazard: if the timeout branch ever regressed to joining BEFORE
/// killing, this test would HANG instead of failing, surfacing as a
/// nextest slow-timeout rather than an assertion failure. That is an
/// accepted, documented trade-off for exercising the real branch rather
/// than a mock of it.
///
/// Does not take `acquire_lsp_test_lock()`: this stub is not an LSP
/// process, needs no tokio runtime, and taking the lock would serialise a
/// 1s test behind the 30s LSP test for no benefit.
#[cfg(unix)]
#[test]
#[should_panic(expected = "REIFY_6161_TIMEOUT_MARKER")]
fn wait_for_exit_timeout_branch_drains_and_reports_stderr() {
    let mut child = Command::new("/bin/sh")
        .args([
            "-c",
            "printf 'REIFY_6161_TIMEOUT_MARKER\n' >&2; exec sleep 30",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn /bin/sh stub");

    let mut stderr_pipe = child.stderr.take().expect("stderr");
    let stderr_reader = thread::spawn(move || {
        let mut buf = Vec::new();
        stderr_pipe.read_to_end(&mut buf).ok();
        buf
    });

    wait_for_exit(&mut child, 1, stderr_reader);
}
