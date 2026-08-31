use std::io::{self, BufRead, BufReader, Read as _, Write};
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
/// poison. This pattern is used at several other sites in the codebase,
/// e.g. `priority_promotion.rs` and `mocks.rs`. It was also used by
/// `reify-runtime/src/concurrent.rs` and
/// `reify-runtime/src/concurrent_eval.rs`, both deleted in c1b8dba3f7
/// (task 5065, task ο step-2); neither file exists any more.
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

/// Kills and reaps the wrapped child on drop (including mid-unwind, so this
/// still runs when a test panics). `std::process::Child` itself has no such
/// `Drop` impl — dropping the handle alone leaves the OS process running,
/// which matters most for a child parked in pipe-write backpressure: no
/// other cleanup (e.g. closing `stdin`) unblocks a `write()` to a full
/// stdout/stderr pipe, so without this guard a panic mid-test leaks the
/// child for the rest of its natural life (or the whole 30s `sleep` on the
/// timeout-branch stub below).
///
/// Killing/reaping an already-exited child a second time — the common case,
/// since `wait_for_exit` itself kills+reaps on its own timeout path, and a
/// clean exit is already reaped by the time `try_wait` observes it — is a
/// harmless no-op: the OS reports ESRCH/ECHILD, which `.ok()` discards.
///
/// `Deref`/`DerefMut` forward to the wrapped `Child` so call sites read as
/// ordinary `Child` usage (`child.stdin.take()`, `wait_for_exit(&mut child,
/// ..)`) rather than threading `.0` through every access.
struct KillOnDrop(Child);
impl std::ops::Deref for KillOnDrop {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}
impl std::ops::DerefMut for KillOnDrop {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

/// Budget for each post-deadline cleanup step in `wait_for_exit`'s timeout
/// branch (reaping the killed child, then joining the stderr reader). Both
/// steps run *after* the deadline has already expired, so neither may be
/// unbounded: see `wait_for_exit`'s doc comment.
const CLEANUP_BUDGET: Duration = Duration::from_secs(5);

/// A background thread draining a child's pipe to completion (see
/// `spawn_pipe_reader`), yielding the captured bytes and, if the read
/// itself failed partway through, the `io::Error` that stopped it. Named so
/// this shape — spelled out in full it is a six-site repeat across this
/// file (`spawn_pipe_reader`, `drain`, `drain_bounded`, `wait_for_exit`,
/// `DrainedLspSession`, `spawn_sh_stub`) — is written once (task #6162).
type PipeReader = thread::JoinHandle<(Vec<u8>, Option<io::Error>)>;

/// Wait for a child process to exit with a timeout.
/// Panics with a clear message if the deadline expires instead of hanging CI.
///
/// Also joins `stderr_reader` (the background thread draining the child's
/// stderr pipe — see `spawn_pipe_reader`'s doc comment for the load-bearing
/// ordering it must be spawned under) and returns its captured text
/// alongside the exit status. `reify lsp`'s stderr is the only channel it
/// uses to report startup/runtime failures, so discarding it would make any
/// future failure of this test a black box.
///
/// On the timeout path the child is killed, then reaped, then the reader is
/// joined — in that order. `kill()` only queues the signal asynchronously;
/// the process's stderr write end is not closed by the kernel until it has
/// actually terminated, so reaping first is what lets the join see EOF
/// instead of racing the child's death.
///
/// Both of those post-deadline steps are bounded by `CLEANUP_BUDGET`, so
/// this guard cannot itself become the hang it exists to prevent. Two ways
/// it otherwise could: a child wedged in uninterruptible sleep never reaps,
/// and — more realistically — a child that forked a grandchild inheriting
/// its stderr leaves the pipe's write end open forever, so `read_to_end` in
/// the reader thread never reaches EOF no matter how dead the direct child
/// is. Neither applies to today's callers (`reify lsp` does not fork; the
/// stub child `exec`s), which is why this is belt-and-braces rather than a
/// live bug fix. On expiry the panic still fires, carrying a placeholder in
/// place of the stderr it could not collect.
fn wait_for_exit(
    child: &mut Child,
    timeout_secs: u64,
    stderr_reader: PipeReader,
) -> (ExitStatus, String) {
    match wait_for_exit_no_stderr(child, timeout_secs) {
        Some(status) => (status, drain(stderr_reader, "stderr")),
        None => {
            child.kill().ok();
            reap_bounded(child, CLEANUP_BUDGET);
            let stderr = drain_bounded(stderr_reader, "stderr", CLEANUP_BUDGET);
            panic!(
                "child process did not exit within {timeout_secs}s\n\
                 --- child stderr ---\n{}\n--- end child stderr ---",
                elide(&stderr)
            );
        }
    }
}

/// Polls `try_wait` until the child exits or `timeout_secs` elapses,
/// returning `None` on timeout instead of panicking or touching stderr.
/// This is the poll/deadline/50ms-sleep timing policy shared by
/// `wait_for_exit` above (which layers stderr draining and a kill+panic on
/// top on `None`) and
/// `lsp_survives_huge_unknown_uri_didchange_with_undrained_stderr` (which
/// must NOT drain stderr — see that test's doc comment, and `wait_for_exit`'s
/// own doc comment for why draining it would defeat the test). Extracted so
/// the timing policy lives in exactly one place instead of two copies that
/// could silently drift apart (task #6162).
fn wait_for_exit_no_stderr(child: &mut Child, timeout_secs: u64) -> Option<ExitStatus> {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if let Some(status) = child.try_wait().expect("try_wait failed") {
            return Some(status);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Poll `try_wait` until the (already-killed) child is reaped or `budget`
/// expires, instead of blocking in `wait()` forever. Returning without
/// having reaped is not an error here: the caller is on its way to
/// panicking, and `KillOnDrop` retries the reap as the stack unwinds.
fn reap_bounded(child: &mut Child, budget: Duration) {
    let deadline = Instant::now() + budget;
    loop {
        match child.try_wait() {
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            // Reaped, errored (ECHILD — already reaped), or out of budget.
            _ => return,
        }
    }
}

/// `drain` with a bound on the join: hands the reader to a helper thread and
/// waits at most `budget` for its text, falling back to a placeholder.
///
/// The helper thread is deliberately detached rather than joined — if the
/// reader is wedged on a pipe whose write end outlived the child, joining it
/// is exactly the unbounded wait being avoided. It leaks for the remainder
/// of the test binary's life, which is bounded and only reachable on a path
/// that is already panicking.
fn drain_bounded(reader: PipeReader, label: &'static str, budget: Duration) -> String {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || tx.send(drain(reader, label)).ok());
    rx.recv_timeout(budget).unwrap_or_else(|_| {
        format!("<{label} drain did not complete within {budget:?}; capture unavailable>")
    })
}

/// Spawn a background thread that reads a child's pipe (stdout/stderr) to
/// completion, returning the captured bytes and, if the read itself failed
/// partway through (e.g. EIO/EBADF), the `io::Error` that stopped it.
///
/// Must be spawned before any write to the child's stdin and before any
/// `wait()`/`try_wait()` on the child. This ordering is load-bearing, not
/// stylistic: a child that writes enough to a pipe to fill its buffer
/// blocks in `write()` and never gets around to reading stdin or exiting,
/// so draining a pipe only after the write/wait phase reintroduces the same
/// deadlock class on whichever pipe is drained late. The margin is far
/// thinner than the nominal 64 KiB pipe capacity suggests: once
/// `fs.pipe-user-pages-soft` (16384 pages) is exceeded, the kernel shrinks
/// each NEW pipe to a single page, and a 24-way-parallel workspace nextest
/// run does this routinely (`F_GETPIPE_SZ` measured at 8192 bytes mid-run).
/// A regression in this ordering surfaces as a hang, not a failed assertion
/// — `stderr_drain_survives_backpressure_from_a_chatty_stub_child` and
/// `wait_for_exit_timeout_branch_drains_and_reports_stderr` are what at
/// least make that hang reachable by two named tests (see their doc
/// comments), rather than proof against it.
///
/// `crates/reify-cli/tests/harness_cli/mcp_integration.rs` is in the same
/// exposure class; see #5389. (Deliberately a bare pointer: describing that
/// file's current internals here would go stale the moment #5389 lands, and
/// nothing gates it.) This file reimplements the pattern locally rather than
/// sharing a helper.
fn spawn_pipe_reader(mut pipe: impl io::Read + Send + 'static) -> PipeReader {
    thread::spawn(move || {
        let mut buf = Vec::new();
        let err = pipe.read_to_end(&mut buf).err();
        (buf, err)
    })
}

/// Join a pipe-reader thread and render its bytes as lossy UTF-8. Lossy
/// rather than strict: this is diagnostic output, so a child that emitted a
/// partial multi-byte sequence before dying must still be readable.
///
/// Never panics on the read itself. If the underlying read failed partway
/// through (e.g. EIO/EBADF — distinct from the child simply writing
/// little/nothing), the error is folded into the returned text inline as a
/// trailing `[<label> read failed before EOF: <err>]` marker. Every caller
/// here already interpolates the captured text into whatever message it
/// fails with, so the marker reaches the reader of that failure without a
/// separate out-of-band flag — and because it is appended last, `elide`'s
/// tail window preserves it even for a 256 KiB capture (see
/// `stderr_drain_survives_backpressure_from_a_chatty_stub_child`). Pinned
/// by `drain_folds_a_mid_read_failure_into_the_returned_text`.
///
/// (`reader.join()` failing — the thread itself panicking, as opposed to
/// the read it performed returning an `io::Error` — is a distinct, harder
/// failure and still hard-panics here: it means `spawn_pipe_reader`'s own
/// closure broke, not that the child said something unexpected.)
fn drain(reader: PipeReader, label: &str) -> String {
    let (bytes, err) = reader
        .join()
        .unwrap_or_else(|_| panic!("{label} reader thread panicked"));
    let text = String::from_utf8_lossy(&bytes).into_owned();
    match err {
        Some(e) => format!("{text}\n[{label} read failed before EOF: {e}]"),
        None => text,
    }
}

/// Drives `drain`'s `Some(e)` arm — the one path a real child pipe will
/// essentially never take, and which therefore had no coverage at all while
/// it was still plumbed out through a `bool` return.
///
/// Pins both halves of the fold: bytes read before the failure survive, and
/// the error is appended as a trailing marker (trailing specifically so
/// `elide`'s tail window keeps it for a capture too large to print whole).
#[test]
fn drain_folds_a_mid_read_failure_into_the_returned_text() {
    /// Yields `remaining` across as many `read` calls as it takes, then
    /// fails — the shape of a pipe that hits EIO/EBADF partway through.
    struct FailsAfterBytes {
        remaining: &'static [u8],
    }
    impl io::Read for FailsAfterBytes {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.remaining.is_empty() {
                return Err(io::Error::other("simulated EIO"));
            }
            let n = buf.len().min(self.remaining.len());
            buf[..n].copy_from_slice(&self.remaining[..n]);
            self.remaining = &self.remaining[n..];
            Ok(n)
        }
    }

    let text = drain(
        spawn_pipe_reader(FailsAfterBytes {
            remaining: b"partial capture",
        }),
        "stderr",
    );

    assert!(
        text.starts_with("partial capture"),
        "bytes read before the failure must survive the fold, got {text:?}"
    );
    assert!(
        text.ends_with("[stderr read failed before EOF: simulated EIO]"),
        "the io::Error must be appended as a trailing marker, got {text:?}"
    );
}

/// Render a possibly-huge diagnostic string for inclusion in a panic/assert
/// message: the first and last 512 bytes plus the total length, instead of
/// the whole thing. `stderr_drain_survives_backpressure_from_a_chatty_stub_child`
/// deliberately captures ~256 KiB of stderr; interpolating it whole into
/// every failure message would bury genuinely useful signal (e.g. an
/// unrelated `status.success()` failure) under a repeated 256 KiB dump.
fn elide(s: &str) -> String {
    const HEAD_TAIL: usize = 512;
    if s.len() <= HEAD_TAIL * 2 {
        return s.to_string();
    }
    // Slice on char boundaries so multi-byte UTF-8 (from the lossy decode
    // in `drain`) is never split mid-codepoint.
    let mut head_end = HEAD_TAIL.min(s.len());
    while !s.is_char_boundary(head_end) {
        head_end -= 1;
    }
    let mut tail_start = s.len().saturating_sub(HEAD_TAIL);
    while !s.is_char_boundary(tail_start) {
        tail_start += 1;
    }
    format!(
        "{} bytes total (showing first {} and last {} bytes):\n{:?}\n...\n{:?}",
        s.len(),
        head_end,
        s.len() - tail_start,
        &s[..head_end],
        &s[tail_start..]
    )
}

/// Unit-pins `elide`, which is otherwise fed only ASCII by its real call
/// sites (256 KiB of spaces from the backpressure stub, a 25-byte marker
/// from the timeout stub) and so never executes its two `is_char_boundary`
/// walk loops in an end-to-end run. Those loops are the only non-trivial
/// thing in the function, and they run one decrementing and one
/// incrementing — an inverted `+=`/`-=` would ship green and only surface
/// later as a `byte index is not a char boundary` panic *inside* some
/// other test's failure message, i.e. exactly when someone is already
/// debugging something else.
#[test]
fn elide_passes_short_input_through_and_walks_to_char_boundaries() {
    // (a) At or below the 2 * HEAD_TAIL threshold: verbatim, no header.
    assert_eq!(elide(""), "");
    assert_eq!(elide("hello"), "hello");
    let at_threshold = "a".repeat(1024);
    assert_eq!(elide(&at_threshold), at_threshold);

    // (b) One byte over: elided, reporting the true total and a full
    // 512-byte head/tail (all-ASCII, so neither walk loop moves).
    let over = "a".repeat(1025);
    let rendered = elide(&over);
    assert_eq!(
        rendered.lines().next(),
        Some("1025 bytes total (showing first 512 and last 512 bytes):"),
        "full rendering: {rendered:?}"
    );

    // (c) All 3-byte codepoints, so both cut points land mid-codepoint:
    // 512 % 3 == 2 and (len - 512) % 3 == 1. The head walk must shrink
    // 512 -> 510 and the tail walk must grow len-512 -> len-510, and
    // neither slice may panic. U+FFFD is not arbitrary: it is what
    // `String::from_utf8_lossy` in `drain` emits for a child that died
    // mid-sequence, which is the input shape that motivated the loops.
    let lossy = "\u{FFFD}".repeat(500);
    assert_eq!(lossy.len(), 1500, "500 * 3-byte codepoints");
    assert!(
        !lossy.is_char_boundary(512) && !lossy.is_char_boundary(1500 - 512),
        "premise: both cut points must land mid-codepoint for this case to bite"
    );
    let rendered = elide(&lossy);
    assert_eq!(
        rendered.lines().next(),
        Some("1500 bytes total (showing first 510 and last 510 bytes):"),
        "full rendering: {rendered:?}"
    );
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

/// Wait for a notification with the given `method` whose `params.uri` and
/// `params.version` equal `uri`/`version`, skipping any other messages
/// (responses, or notifications for other documents/versions) received in
/// between. Returns the matched notification so the caller can assert on
/// its `params` (e.g. `diagnostics`).
///
/// Used as a deterministic barrier in place of a fixed-duration sleep:
/// observing the notification the server published *for the exact
/// uri+version just sent* proves it finished handling that message, rather
/// than hoping a wall-clock delay was long enough under CPU load. (What
/// reify-lsp does internally between receiving the message and publishing
/// is deliberately not restated here — that is production control flow, and
/// the phase-4b assertion in `lsp_full_interactive_loop_through_binary` is
/// the actual proof.)
///
/// `version` is required, not optional, for correctness: the mpsc channel
/// is a FIFO of every notification the server has already published, so
/// for a `uri` that was published before, a `method`+`uri`-only match can
/// return a *stale* already-queued notification and provide no barrier at
/// all.
///
/// Same 30s timeout and CPU-saturation rationale as `wait_for_response`
/// above.
fn wait_for_notification(
    rx: &mpsc::Receiver<serde_json::Value>,
    method: &str,
    uri: &str,
    version: i64,
) -> serde_json::Value {
    let timeout = std::time::Duration::from_secs(30);
    loop {
        match rx.recv_timeout(timeout) {
            Ok(msg) => {
                if msg.get("method").and_then(|v| v.as_str()) == Some(method)
                    && msg["params"]["uri"].as_str() == Some(uri)
                    && msg["params"]["version"].as_i64() == Some(version)
                {
                    return msg;
                }
                // Otherwise it's a response, or a notification for a
                // different document/version; keep waiting.
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!(
                    "timed out after 30s waiting for {method} v{version} notification for uri={uri}"
                )
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!(
                    "reader thread disconnected (LSP process may have crashed) \
                     while waiting for {method} v{version} notification for uri={uri}"
                )
            }
        }
    }
}

/// Extract the ERROR-severity entries (LSP `severity == 1`, i.e.
/// `DiagnosticSeverity::ERROR`; see the numeric mapping already asserted by
/// `crates/reify-lsp/tests/in_process_bridge.rs`) from a
/// `textDocument/publishDiagnostics` notification returned by
/// `wait_for_notification`. Returns owned clones (diagnostics are tiny) so
/// callers don't have to reason about borrows against the notification.
fn error_diagnostics(notification: &serde_json::Value) -> Vec<serde_json::Value> {
    notification["params"]["diagnostics"]
        .as_array()
        .expect("publishDiagnostics params.diagnostics should be a JSON array")
        .iter()
        .filter(|d| d.get("severity").and_then(|s| s.as_i64()) == Some(1))
        .cloned()
        .collect()
}

/// Result of `spawn_lsp_drained`: a running `reify lsp` child that has
/// already completed the `initialize`/`initialized` handshake, with stderr
/// being drained in the background and ready for `wait_for_exit`.
struct DrainedLspSession {
    child: KillOnDrop,
    stdin: std::process::ChildStdin,
    rx: mpsc::Receiver<serde_json::Value>,
    /// The raw `initialize` response, for callers that assert on its shape
    /// (e.g. capabilities) beyond the generic `result.is_some()` check
    /// `spawn_lsp_and_initialize` already performs.
    init_response: serde_json::Value,
    /// The background thread already draining stderr (spawned before any
    /// stdin write — see `spawn_pipe_reader`'s doc comment), ready to be
    /// handed to `wait_for_exit`.
    stderr_reader: PipeReader,
}

/// Result of `spawn_lsp_undrained`: a running `reify lsp` child that has
/// already completed the `initialize`/`initialized` handshake, with the
/// raw, deliberately undrained stderr pipe.
struct UndrainedLspSession {
    child: KillOnDrop,
    stdin: std::process::ChildStdin,
    rx: mpsc::Receiver<serde_json::Value>,
    /// The caller must keep this alive for as long as backpressure needs to
    /// be sustained — dropping it early gives the child EPIPE instead of
    /// backpressure.
    stderr_pipe: std::process::ChildStderr,
}

/// Spawns `reify lsp`, drains stdout, and drives the `initialize` /
/// `initialized` handshake — the setup shared by every test in this file
/// that talks to the real binary (task #6162: previously duplicated near-
/// verbatim between `lsp_full_interactive_loop_through_binary` and
/// `lsp_survives_huge_unknown_uri_didchange_with_undrained_stderr`).
///
/// stdout is always drained via `spawn_reader`, regardless of what the
/// caller does with stderr, so a blocked stdout pipe can never be mistaken
/// for a stderr backpressure scenario a caller is deliberately inducing.
///
/// Private core shared by `spawn_lsp_drained`/`spawn_lsp_undrained`: those
/// are the two functions callers should actually use. `take_stderr` decides
/// the stderr discipline and is handed the raw `ChildStderr` before any
/// stdin write — load-bearing ordering, see `spawn_pipe_reader`'s doc
/// comment — with whatever it returns threaded back out as `T`. Generic
/// over `T` rather than an enum-tagged result: the caller picks the
/// discipline by which closure it passes (`spawn_pipe_reader` vs. the
/// identity closure), so the stderr shape a call site gets back is decided
/// by the type of `T` it asked for, with no `unreachable!()` re-assertion
/// needed at either wrapper below (task #6162).
fn spawn_lsp_and_initialize<T>(
    take_stderr: impl FnOnce(std::process::ChildStderr) -> T,
) -> (
    KillOnDrop,
    std::process::ChildStdin,
    mpsc::Receiver<serde_json::Value>,
    serde_json::Value,
    T,
) {
    let mut child = KillOnDrop(
        Command::new(env!("CARGO_BIN_EXE_reify"))
            .args(["lsp"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn reify lsp"),
    );

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let stderr_pipe = child.stderr.take().expect("stderr");

    let rx = spawn_reader(stdout);
    // Runs before any stdin write below — load-bearing ordering, see
    // `spawn_pipe_reader`'s doc comment.
    let stderr = take_stderr(stderr_pipe);

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

    let initialized = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    });
    send_jsonrpc(&mut stdin, &initialized.to_string());

    (child, stdin, rx, init_response, stderr)
}

/// Spawns `reify lsp` with stderr drained the same way stdout is drained —
/// use this when the test needs the child to run to completion without
/// stderr backpressure. Pairs with `wait_for_exit`, which expects exactly
/// this `stderr_reader` shape. See `spawn_lsp_and_initialize`'s doc
/// comment for the shared spawn + handshake sequence.
fn spawn_lsp_drained() -> DrainedLspSession {
    let (child, stdin, rx, init_response, stderr_reader) =
        spawn_lsp_and_initialize(spawn_pipe_reader);
    DrainedLspSession {
        child,
        stdin,
        rx,
        init_response,
        stderr_reader,
    }
}

/// Spawns `reify lsp` with stderr taken into a named handle and
/// deliberately never read — use this when the test needs to put stderr
/// under deliberate, sustained backpressure. Dropping the returned
/// `stderr_pipe` early gives the child EPIPE instead of backpressure. See
/// `spawn_lsp_and_initialize`'s doc comment for the shared spawn +
/// handshake sequence.
fn spawn_lsp_undrained() -> UndrainedLspSession {
    let (child, stdin, rx, _init_response, stderr_pipe) = spawn_lsp_and_initialize(|pipe| pipe);
    UndrainedLspSession {
        child,
        stdin,
        rx,
        stderr_pipe,
    }
}

/// Builds task #6162's trigger: a `textDocument/didChange` for a
/// never-opened URI with a deliberately huge (160 KiB) path, so
/// `DocumentStore::update` returns `false` and `did_change`'s unknown-URI
/// `eprintln!` fires (see `crates/reify-lsp/src/server.rs`). Returns the URI
/// alongside the JSON-RPC body so callers can both send the notification and
/// match `wait_for_notification`/`wait_for_response` against the same
/// string.
///
/// Single definition shared by `lsp_full_interactive_loop_through_binary`'s
/// phase 4b and
/// `lsp_survives_huge_unknown_uri_didchange_with_undrained_stderr` — the two
/// tests are two halves of one regression guard (bounded logging, and no
/// wedge) for the *same* trigger, so a future change to the URI's size or
/// shape cannot update one copy without the other (task #6162).
fn huge_unknown_uri_did_change(version: i64) -> (String, serde_json::Value) {
    let uri = format!("file:///tmp/{}.ri", "a".repeat(160 * 1024));
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {
                "uri": uri,
                "version": version
            },
            "contentChanges": [{ "text": reify_test_support::bracket_source() }]
        }
    });
    (uri, body)
}

/// Full interactive LSP session through the real `reify lsp` binary, driven
/// over stdio with real JSON-RPC framing.
///
/// Beyond protocol coverage (initialize capabilities, didOpen, didChange
/// with violating/valid sources, shutdown/exit), phase 4b below sends a
/// `textDocument/didChange` for a URI that was never opened, with a
/// deliberately huge (160 KiB) path. `DocumentStore::update`
/// (crates/reify-lsp/src/document.rs) returns `false` for any unknown URI,
/// which makes `did_change`'s unknown-URI `eprintln!`
/// (crates/reify-lsp/src/server.rs) fire. Since task #6162 landed, that
/// line is bounded by `truncate_for_log` before it is written — the
/// captured stderr is small (well under 16 KiB), not the URI echoed
/// verbatim — and the bounded-length assertions at the end of this function
/// are this test's end-to-end regression guard for that fix, through the
/// real binary.
///
/// Historical/pre-fix measured A/B on this binary (target/debug/reify),
/// kept here because it is what motivates `spawn_pipe_reader` being spawned
/// before the write/wait phase (see its doc comment): with stderr piped but
/// never taken/drained, the *unbounded* unknown-URI log line left the child
/// unable to exit, with the main thread parked in `wchan=pipe_write` (the
/// same signature as the stdout hang #5389 root-caused); with the reader
/// thread spawned first it exited `rc=0` promptly with the full stderr
/// captured. Now that the logged line is bounded, phase 4b's own run no
/// longer puts the pipe under real backpressure — that demonstration has
/// moved to `stderr_drain_survives_backpressure_from_a_chatty_stub_child`,
/// which drives a deterministic 256 KiB stub instead of depending on
/// reify-lsp's logging staying unbounded, so a future logging change can no
/// longer make the drain-under-backpressure guard vacuous.
///
/// Every phase (didOpen and each didChange, including 4b) synchronizes with
/// `wait_for_notification`, blocking on that phase's own `publishDiagnostics`
/// notification rather than a fixed sleep: tower-lsp dispatches
/// requests/notifications with a concurrency level > 1, so a wall-clock
/// delay is not a reliable proxy for "the server has processed this
/// specific message" under the CPU-saturation conditions this file already
/// designs around (see `wait_for_response`'s doc comment). Phases 2-4 also
/// assert ERROR diagnostics are absent/present/absent across the
/// valid → violating → valid sequence, so this test would fail if the LSP
/// stopped wiring `did_open`/`did_change` to the diagnostics engine, not
/// just if it stopped draining stderr. That is deliberately the *wiring*
/// only — the diagnostic semantics are owned in-process by
/// `reify-lsp`'s `diagnostics` tests, and both payloads come from the
/// `reify_test_support` fixtures those tests use, so the two cannot drift
/// apart.
///
/// Phase 4b keeps the `stderr.contains("didChange for unknown URI")`
/// assertion as a non-vacuity anchor: proof the unknown-URI diagnostic
/// still fires at all, so the bounded-length assertions beside it cannot
/// pass simply because the log line vanished entirely.
#[test]
fn lsp_full_interactive_loop_through_binary() {
    let _lock = acquire_lsp_test_lock();
    // See `spawn_lsp_and_initialize`'s doc comment for the spawn +
    // handshake sequence, including why the reader-thread ordering is
    // load-bearing. Wrapped in KillOnDrop (see its doc comment above) so
    // every panic site below — wait_for_notification and the stderr
    // assertions — kills and reaps this child instead of leaving it running
    // (e.g. parked in `pipe_write` backpressure) for the test process to
    // clean up on exit.
    //
    // `stderr_drain_survives_backpressure_from_a_chatty_stub_child` and
    // `wait_for_exit_timeout_branch_drains_and_reports_stderr` are what make
    // a regression in the reader-thread ordering reachable by name, even
    // though (per `spawn_pipe_reader`'s doc comment) the failure mode either
    // would hit is a hang, not a clean assertion failure.
    let DrainedLspSession {
        mut child,
        mut stdin,
        rx,
        init_response,
        stderr_reader,
    } = spawn_lsp_drained();

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

    // 2) didOpen with valid bracket source.
    //
    // Both payloads come from `reify_test_support`, not a local literal, so
    // this e2e test and the in-process diagnostics tests provably drive the
    // *same* source. The literal that used to live here had already drifted
    // from the fixture (`structure Bracket` vs the fixture's `structure def
    // Bracket`) with nothing to catch it, and the phase assertions below
    // are the first thing to depend on it being semantically equivalent.
    let valid_source = reify_test_support::bracket_source();

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

    // Deterministic barrier (see rustdoc above) in place of a fixed sleep:
    // block for this didOpen's own publishDiagnostics (version 1). Valid
    // source should produce no ERROR diagnostics (mirrors
    // diagnostics::stateful_diagnostics_three_phase_lifecycle's phase 1).
    let diag_open = wait_for_notification(
        &rx,
        "textDocument/publishDiagnostics",
        "file:///tmp/test_bracket.ri",
        1,
    );
    assert!(
        error_diagnostics(&diag_open).is_empty(),
        "phase 2 (didOpen, valid source): expected no ERROR diagnostics, got {:#?}",
        diag_open["params"]["diagnostics"]
    );

    // 3) didChange with violating source (the fixture sets thickness=1mm,
    // violating the `thickness > 2mm` constraint the valid source declares).
    let violating_source = reify_test_support::bracket_source_violating();
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

    // Deterministic barrier: block for this didChange's own
    // publishDiagnostics (version 2). Asserting only that the violating
    // fixture reaches the diagnostics engine at all — i.e. the didChange ->
    // publishDiagnostics *wiring*, which only an out-of-process test can
    // cover. What the resulting diagnostic says is owned by
    // reify-lsp's in-process
    // `diagnostics::stateful_violating_source_always_produces_constraint_violation`,
    // and re-deriving that message predicate here would just duplicate it.
    let diag_violating = wait_for_notification(
        &rx,
        "textDocument/publishDiagnostics",
        "file:///tmp/test_bracket.ri",
        2,
    );
    assert!(
        !error_diagnostics(&diag_violating).is_empty(),
        "phase 3 (didChange, violating source): expected at least one ERROR diagnostic, got {:#?}",
        diag_violating["params"]["diagnostics"]
    );

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

    // Deterministic barrier: block for this didChange's own
    // publishDiagnostics (version 3). Back to valid source, so ERROR
    // diagnostics should clear (mirrors
    // diagnostics::stateful_diagnostics_three_phase_lifecycle's phase 3).
    let diag_valid_again = wait_for_notification(
        &rx,
        "textDocument/publishDiagnostics",
        "file:///tmp/test_bracket.ri",
        3,
    );
    assert!(
        error_diagnostics(&diag_valid_again).is_empty(),
        "phase 4 (didChange, back to valid source): expected violations to clear, got {:#?}",
        diag_valid_again["params"]["diagnostics"]
    );

    // 4b) didChange for a never-opened URI with a deliberately huge path.
    // DocumentStore::update returns false for any URI that was never opened
    // via didOpen, so did_change's unknown-URI eprintln! (server.rs) fires.
    // This is the end-to-end regression guard for task #6162's fix (see
    // rustdoc above): the resulting stderr is asserted bounded below,
    // instead of the pipe-backpressure role this phase used to serve
    // (that has moved to stderr_drain_survives_backpressure_from_a_chatty_stub_child).
    let (huge_uri, did_change_unknown_uri) = huge_unknown_uri_did_change(4);
    send_jsonrpc(&mut stdin, &did_change_unknown_uri.to_string());

    // Deterministic barrier (see rustdoc above): block until the server
    // publishes diagnostics for the huge URI (version 4), proving
    // did_change's handler — including the unknown-URI eprintln! — has
    // already run, instead of hoping a fixed sleep was long enough. NOTE:
    // in the undrained (pre-fix) regression scenario this eprintln! blocks
    // forever on pipe backpressure, so did_change never reaches
    // publish_diagnostics and this call times out after its own 30s rather
    // than reaching `wait_for_exit`'s timeout below — still a failing
    // test, just via a different panic site. `child` is wrapped in
    // `KillOnDrop` (see above), so even in that scenario the still-blocked
    // `reify lsp` process is killed and reaped as this function's stack
    // unwinds, rather than left for the test process to clean up on exit.
    wait_for_notification(&rx, "textDocument/publishDiagnostics", &huge_uri, 4);

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
    // Elided once and reused in every message below. Phase 4b's captured
    // stderr is now small (bounded by task #6162's fix), so `elide` here is
    // just a cheap uniform renderer rather than a defence against a huge
    // blob — but reusing it keeps every failure message in this function
    // formatted the same way, including if `status.success()` fails for an
    // unrelated reason and stderr happens to be large again.
    let stderr_summary = elide(&stderr);
    assert!(
        status.success(),
        "reify lsp should exit cleanly after full interactive loop (stderr: {stderr_summary})"
    );

    // Proves the captured bytes came from the intended production path
    // (server.rs's unknown-URI didChange handler) rather than incidental
    // noise. A failure here likewise means the trigger moved, not that the
    // drain broke.
    assert!(
        stderr.contains("didChange for unknown URI"),
        "expected captured stderr to contain the unknown-URI diagnostic emitted by \
         reify-lsp's did_change handler (server.rs). This means the chatty-stderr \
         trigger has moved and this regression guard needs re-pointing — it does NOT mean \
         the stderr drain is broken. Captured stderr: {stderr_summary}"
    );

    // Task #6162 regression guard: a 160 KiB client-supplied URI must not
    // become 160 KiB of stderr. Derived basis: the truncated log line is at
    // most 39 bytes of prefix + 1024 bytes of (256-char) URI + ~35 bytes of
    // marker, i.e. < 1.15 KiB — 16 KiB leaves >14x headroom over that bound
    // while sitting far below the measured pre-fix value of 163_895 bytes.
    // A failure here means the truncation regressed (or never happened),
    // NOT that the drain broke.
    assert!(
        stderr.len() < 16 * 1024,
        "expected <16KiB of captured stderr from phase 4b's huge-URI didChange (derived \
         worst case < 1.15KiB; measured 163_895 bytes pre-fix), got {} bytes. This means the \
         did_change unknown-URI log line is not being truncated — NOT that the stderr drain \
         is broken. Captured stderr: {stderr_summary}",
        stderr.len()
    );
    // Proves the bounded-length assertion above isn't vacuously satisfied by
    // the log line disappearing entirely — the elision marker must actually
    // have fired.
    assert!(
        stderr.contains("[truncated,"),
        "expected captured stderr to contain truncate_for_log's elision marker \
         (\"[truncated, N bytes total]\"), proving the huge URI was actually truncated rather \
         than the log line vanishing. Captured stderr: {stderr_summary}"
    );
}

/// Reproduces task #6162's reported bug at the layer it was measured: a
/// client that sends a huge unknown-URI `didChange` and never drains the
/// server's stderr must not wedge the server process.
///
/// Unlike `lsp_full_interactive_loop_through_binary`, this test never spawns
/// a reader for the child's stderr pipe. `child.stderr` is taken into the
/// named `stderr_pipe` binding and deliberately never read. Holding the
/// read end alive (rather than dropping it) is load-bearing: dropping it
/// would give the child EPIPE/SIGPIPE on its next stderr write instead of
/// pipe-full backpressure — a different failure mode that would make this
/// test silently vacuous. The explicit `drop(stderr_pipe)` at the end
/// documents that its lifetime must span every assertion above it, so a
/// future refactor cannot accidentally shorten it by accident.
///
/// stdout IS drained via `spawn_reader` (the same helper
/// `lsp_full_interactive_loop_through_binary` uses): a blocked stdout pipe
/// is a different wedge, and leaving it undrained would mean this test
/// proves nothing about stderr specifically.
///
/// Measured evidence (task #6162): with stderr piped but never drained, the
/// pre-fix binary does not exit within 20s, and the main thread's
/// `/proc/<pid>/task/<tid>/wchan` reads `pipe_write`. With stderr drained
/// (the happy path `lsp_full_interactive_loop_through_binary` exercises) it
/// exits `rc=0` in 0.05s having written 163_895 bytes. The primary RED
/// tripwire below is `wait_for_notification` for the huge-URI `didChange`:
/// pre-fix, `did_change` blocks forever inside the unknown-URI `eprintln!`'s
/// `pipe_write` call and never reaches `publish_diagnostics`, so that call
/// panics on its own 30s timeout. Expect this test to cost ~30s while RED —
/// that is the notification timeout firing as designed, not a hang.
///
/// Deliberately does NOT assert the child's exit CODE and does NOT wait for
/// the `shutdown` response: tower-lsp dispatches requests/notifications
/// with concurrency > 1 (see `wait_for_response`'s doc comment), so the
/// ordering of `shutdown`'s response relative to `exit` is not guaranteed
/// under CPU saturation, and asserting on it would add flake without adding
/// signal. The property the bug violates is "the process never exits at
/// all", so process exit is the only assertion that needs to carry weight
/// here — the `wait_for_notification` barrier above already proves the
/// handler ran to completion. It does, however, assert the process did not
/// die from a *signal* (`status.code().is_some()`): unlike a specific exit
/// code or response ordering, signal death is unambiguous and independent
/// of tower-lsp's dispatch ordering, so checking it costs no extra flake.
///
/// Does NOT call `wait_for_exit`: that helper drains stderr via its
/// `stderr_reader` argument, which is exactly the one thing this test must
/// not do.
///
/// Not gated `#[cfg(unix)]`, unlike this file's two `/bin/sh`-stub tests:
/// this test drives `CARGO_BIN_EXE_reify` through the same portable
/// `Command`/`Stdio` surface as `lsp_full_interactive_loop_through_binary`
/// and uses no unix-only API, so gating it would silently drop the task's
/// primary end-to-end regression test on non-unix targets for no reason.
#[test]
fn lsp_survives_huge_unknown_uri_didchange_with_undrained_stderr() {
    let _lock = acquire_lsp_test_lock();
    // See `spawn_lsp_and_initialize`'s doc comment for the spawn +
    // handshake sequence.
    let UndrainedLspSession {
        mut child,
        mut stdin,
        rx,
        stderr_pipe,
    } = spawn_lsp_undrained();
    // Deliberately NEVER read — see doc comment above.

    // Task #6162's trigger (see `huge_unknown_uri_did_change`'s doc
    // comment): a never-opened URI with a deliberately huge (160 KiB) path,
    // so DocumentStore::update returns false and did_change's unknown-URI
    // eprintln! fires (see server.rs).
    let (huge_uri, did_change_unknown_uri) = huge_unknown_uri_did_change(1);
    send_jsonrpc(&mut stdin, &did_change_unknown_uri.to_string());

    // Primary RED tripwire (see doc comment above): pre-fix, did_change
    // blocks forever in pipe_write inside the unknown-URI eprintln! and
    // never reaches publish_diagnostics, so this panics on its own 30s
    // timeout rather than observing the notification.
    wait_for_notification(&rx, "textDocument/publishDiagnostics", &huge_uri, 1);

    let shutdown = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown",
        "params": null
    });
    send_jsonrpc(&mut stdin, &shutdown.to_string());

    let exit = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null
    });
    send_jsonrpc(&mut stdin, &exit.to_string());

    drop(stdin);

    // Deliberately NOT wait_for_exit (it drains stderr — see doc comment
    // above), and deliberately not asserting the exit code or waiting for
    // the shutdown response (see doc comment above): only that the process
    // exits at all (without dying from a signal), which is exactly the
    // property the bug violates. Reuses `wait_for_exit`'s own poll/deadline
    // policy via `wait_for_exit_no_stderr` rather than a second inline copy
    // of it (task #6162).
    let status = wait_for_exit_no_stderr(&mut child, 30).unwrap_or_else(|| {
        panic!(
            "reify lsp did not exit within 30s after a huge unknown-URI didChange with stderr \
             piped but never drained — task #6162's wedge: did_change blocks in pipe_write \
             while holding the state write lock, so the process can never exit"
        )
    });
    // The exit CODE stays deliberately unasserted (see doc comment above),
    // but abnormal termination is unambiguous and ordering-independent: a
    // child killed by SIGSEGV/SIGABRT (e.g. a panic-abort in did_change
    // after publishDiagnostics already fired) would satisfy a bare "it
    // exited" check while still being a real regression.
    assert!(
        status.code().is_some(),
        "reify lsp died from a signal ({status:?}) rather than exiting normally after a huge \
         unknown-URI didChange with stderr piped but never drained"
    );

    // Documents that stderr_pipe must outlive every assertion above: a
    // future refactor cannot accidentally drop (and thus close) the read
    // end early, which would turn this test's backpressure into EPIPE and
    // make it silently vacuous.
    drop(stderr_pipe);
}

/// Spawns `/bin/sh -c script` with stdin/stdout null and stderr piped,
/// wraps it in `KillOnDrop`, and spawns its stderr pipe reader — the
/// boilerplate shared by every `/bin/sh`-stub test in this file (task
/// #6162). The reader is spawned here, before the caller can possibly wait
/// on the child, so the load-bearing ordering `spawn_pipe_reader`'s doc
/// comment describes lives in exactly one place instead of being
/// duplicated per call site.
#[cfg(unix)]
fn spawn_sh_stub(script: &str) -> (KillOnDrop, PipeReader) {
    let child = Command::new("/bin/sh")
        .args(["-c", script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn /bin/sh stub");
    let mut guard = KillOnDrop(child);
    let stderr_pipe = guard.stderr.take().expect("stderr");
    let stderr_reader = spawn_pipe_reader(stderr_pipe);
    (guard, stderr_reader)
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
/// well past the deadline given to `wait_for_exit`. `exec` preserves the
/// pid, so the pid `wait_for_exit` kills is exactly the process holding the
/// stderr write end, which is what guarantees the reader thread reaches
/// EOF instead of blocking forever.
///
/// That deadline is 5s, not the ~1s this test's own runtime needs. The
/// `should_panic` match below requires the marker to have *already reached
/// the stderr pipe* by the time the deadline expires, so the deadline is
/// doing double duty as a start-up budget for `/bin/sh` — and a 1s budget
/// contradicts the CPU-saturation rationale the rest of this file is
/// designed around (see `wait_for_response`'s 30s). Under a 24-way-parallel
/// nextest run, a fork/exec that has not been scheduled far enough to run
/// `printf` within ~1s would be killed with an empty pipe, and
/// `should_panic(expected = ...)` would report a *failure* that reads as
/// "the drain broke" when the child was merely slow to start. 5s keeps that
/// margin while staying far below the stub's 30s sleep, so the branch under
/// test is still the timeout branch.
///
/// `#[should_panic(expected = ...)]` matching the marker pins three things
/// at once: the timeout branch panics rather than looping forever, the
/// reader join returned rather than hanging (proving kill-then-reap ran
/// *before* the join, not after), and the child's stderr was drained and
/// interpolated into the panic message.
///
/// Measured premise (direct experiment, not assumed): the stub does not
/// exit on its own before the deadline; kill → reap → join then returns
/// exactly "REIFY_6161_TIMEOUT_MARKER\n", with the whole test costing
/// marginally more than the deadline itself.
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
/// process, needs no tokio runtime, and taking the lock would serialise
/// this short test behind the 30s LSP test for no benefit.
///
/// The stub child is wrapped in the module-level `KillOnDrop` (see its doc
/// comment) so that IF the timeout branch instead regressed to returning
/// normally without ever killing the child (a different, milder regression
/// than the join-before-kill hazard above: this one does not hang, it just
/// fails to enforce the timeout), the test still fails loudly via
/// `#[should_panic]`'s "did not panic" — and the orphaned `sleep 30` plus
/// its blocked reader thread are still cleaned up promptly instead of
/// leaking for the rest of the 30s sleep on every run.
#[cfg(unix)]
#[test]
#[should_panic(expected = "REIFY_6161_TIMEOUT_MARKER")]
fn wait_for_exit_timeout_branch_drains_and_reports_stderr() {
    let (mut guard, stderr_reader) =
        spawn_sh_stub("printf 'REIFY_6161_TIMEOUT_MARKER\n' >&2; exec sleep 30");

    wait_for_exit(&mut guard, 5, stderr_reader);
}

/// Owns the "the drain works under REAL pipe backpressure" guard that used
/// to live in `lsp_full_interactive_loop_through_binary`'s phase 4b (task
/// #6162). That property belongs to `spawn_pipe_reader`/`wait_for_exit`'s
/// ordering, not to reify-lsp's logging, so it is pinned here against a
/// trigger this file fully controls rather than a production log line
/// reify-lsp is free to change — including a fix that truncates that line,
/// which is exactly what task #6162 does. Re-pointing this guard at a stub
/// also means it can never again be silently re-coupled to a production
/// code path and go vacuous the way task 6161's original version did (its
/// `>= 128 KiB` bound depended on an incidental coincidence between the
/// logged URI's length and the pipe's capacity).
///
/// The stub — pure POSIX shell builtins, no external commands — writes a
/// recognisable marker FIRST, so it survives `elide`'s head window even
/// though the marker is nowhere near the 512-byte threshold, then exactly
/// `PAYLOAD_BYTES` (256 * 1024 = 262144) bytes via repeated 1024-byte
/// `printf` calls, then exits 0.
///
/// `spawn_sh_stub` calls `spawn_pipe_reader` on the stderr pipe before
/// returning, i.e. before this test can possibly wait on the child —
/// mirroring `lsp_full_interactive_loop_through_binary`'s ordering, and the
/// property under test: reading the pipe concurrently with the child's
/// writes is what prevents the child from blocking in `write()` once the
/// (possibly kernel-shrunk — see `spawn_pipe_reader`'s doc comment) pipe
/// buffer fills. Falsified by hand while writing this test (before the
/// spawn+reader boilerplate was extracted into `spawn_sh_stub`):
/// temporarily moving the `spawn_pipe_reader` call to after an unbounded
/// `try_wait` poll loop (i.e. waiting for the child to exit before ever
/// starting to drain it) made the test hang, exactly as
/// `spawn_pipe_reader`'s doc comment predicts ("a regression in this
/// ordering surfaces as a hang, not a failed assertion") — confirmed by
/// running it under an external `timeout` and observing it get killed
/// rather than complete, then reverted to this ordering.
///
/// Asserts `stderr.len() == BACKPRESSURE_MARKER.len() + 1 + PAYLOAD_BYTES`
/// (262175 today: a 31-byte marker-plus-newline followed by 262144 bytes of
/// payload) — the stub's output is fully deterministic, so there is no
/// reason to leave the slack the old `>= 128 * 1024` bound did: a drain
/// that silently drops or duplicates bytes (e.g. a short-read/partial-fold
/// bug in `spawn_pipe_reader`/`drain`) would still pass a half-payload
/// bound but fails this exact one — and that the marker survived, proving
/// the captured bytes came from this stub and not some other source.
/// Deriving the expectation from the same named constants the script is
/// built from (rather than transcribing the total) is deliberate: a rename
/// or reflow of the marker can no longer desync the assertion from what the
/// stub actually writes (task #6162's amendment review).
///
/// Does not take `acquire_lsp_test_lock()`: this stub is not an LSP
/// process, needs no tokio runtime, and taking the lock would serialise
/// this test behind the 30s LSP test for no benefit (same rationale as
/// `wait_for_exit_timeout_branch_drains_and_reports_stderr` above).
#[cfg(unix)]
#[test]
fn stderr_drain_survives_backpressure_from_a_chatty_stub_child() {
    let (mut guard, stderr_reader) = spawn_sh_stub(
        "printf 'REIFY_6162_BACKPRESSURE_MARKER\n' >&2; i=0; while [ $i -lt 256 ]; do printf '%1024s' '' >&2; i=$((i+1)); done",
    );

    let (status, stderr) = wait_for_exit(&mut guard, 30, stderr_reader);
    let stderr_summary = elide(&stderr);
    assert!(
        status.success(),
        "stub should exit cleanly after writing its deterministic payload (stderr: {stderr_summary})"
    );
    assert_eq!(
        stderr.len(),
        262_175,
        "expected exactly 262175 bytes of captured stderr (31-byte marker+newline plus the \
         stub's deterministic 256 * 1024 = 262144-byte payload) — a mismatch means the drain \
         dropped or duplicated bytes, not merely fell behind. Captured stderr: {stderr_summary}"
    );
    assert!(
        stderr.contains("REIFY_6162_BACKPRESSURE_MARKER"),
        "expected the captured stderr to contain the stub's marker, proving the bytes came \
         from this test's own trigger. Captured stderr: {stderr_summary}"
    );
}
