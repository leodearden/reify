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
/// 10-second `LspInbox::response` timeout to fire. Holding this lock for the
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
/// The sibling `mcp_integration` module in this harness is in the same
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
/// 30-second `LspInbox::response` timeout.  The fix is to:
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

/// How long any [`LspInbox`] wait may go without receiving ANY message before
/// it fails. An inactivity timeout, not a total deadline — see
/// [`LspInbox::wait_for`].
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// The stdout message stream plus a replay buffer, so no wait can starve
/// another of a message it still needs.
///
/// WHY THE BUFFER: `reify lsp` emits `window/logMessage` and
/// `textDocument/publishDiagnostics` for the SAME `didChange` from two
/// independent `tokio::spawn`ed tasks (see `ClientSink` in
/// `crates/reify-lsp/src/server.rs`), with no ordering guarantee between
/// them. A waiter reading straight from the `mpsc::Receiver` and DROPPING
/// every non-match — which is what `wait_for_response`/`wait_for_notification`
/// did before task #6329 — therefore swallows, at random, the message the
/// NEXT wait is about to block on. That surfaces as an intermittent 30s
/// timeout in whichever wait ran second, reading as "the server stopped
/// emitting" when the message had in fact arrived and been discarded.
///
/// Retaining each skipped message in `seen` and re-scanning it first makes
/// the order two waits are *written* in irrelevant. This is not premature
/// generality: it is the precondition for asserting on two concurrently
/// dispatched notification kinds at all.
///
/// `seen` grows only with messages no wait has claimed yet — at most a
/// handful per session here — so no eviction policy is needed.
struct LspInbox {
    rx: mpsc::Receiver<serde_json::Value>,
    seen: Vec<serde_json::Value>,
}

impl LspInbox {
    fn new(rx: mpsc::Receiver<serde_json::Value>) -> Self {
        Self {
            rx,
            seen: Vec::new(),
        }
    }

    /// Return (and consume) the first message satisfying `pred`, scanning the
    /// replay buffer before pulling from the channel and buffering every
    /// non-match for later waits.
    ///
    /// Uses a 30-second inactivity timeout to accommodate CPU saturation when many test
    /// binaries run in parallel (e.g., during `cargo test --workspace`).  Under
    /// heavy load the spawned tokio runtime may not be scheduled for several
    /// seconds before it can process the `initialize` request; 30 s gives ample
    /// headroom without making genuinely failing tests unreasonably slow.
    ///
    /// The timeout restarts on every received message, so it measures
    /// SILENCE, not total wait: a stream of non-matching messages (the burst
    /// test interleaves up to eight 160 KiB-URI `publishDiagnostics` with its
    /// log-message waits) never trips it, and the panic below says exactly
    /// that rather than claiming a total. A silence bound is also the one that
    /// does not invert under load: a descheduled server delays its next
    /// message, it does not make a finite stream arrive late in aggregate.
    /// A genuinely endless stream is nextest's slow-timeout/terminate-after
    /// to catch, not a hand-rolled `Instant` deadline (see
    /// `tests/infra/test_no_new_wallclock_rust_deadlines.sh`).
    ///
    /// `what` is the caller's own fully-formed noun phrase, interpolated into
    /// both panic messages below. Those messages are load-bearing diagnostics
    /// — a timeout here is this file's primary RED signal — so each wrapper
    /// below supplies the exact phrasing its failure needs rather than letting
    /// this helper invent a generic one.
    fn wait_for(
        &mut self,
        what: &str,
        pred: impl Fn(&serde_json::Value) -> bool,
    ) -> serde_json::Value {
        if let Some(idx) = self.seen.iter().position(&pred) {
            return self.seen.remove(idx);
        }
        loop {
            match self.rx.recv_timeout(IDLE_TIMEOUT) {
                Ok(msg) => {
                    if pred(&msg) {
                        return msg;
                    }
                    // Not ours — retain it for whichever wait is.
                    self.seen.push(msg);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    panic!("no message for 30s while waiting for {what}")
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    panic!(
                        "reader thread disconnected (LSP process may have crashed) \
                         while waiting for {what}"
                    )
                }
            }
        }
    }

    /// Wait until we receive a response with the given id from the message
    /// stream. See [`LspInbox::wait_for`] for the timeout rationale.
    fn response(&mut self, id: u64) -> serde_json::Value {
        self.wait_for(&format!("response with id={id}"), |msg| {
            msg.get("id").and_then(|v| v.as_u64()) == Some(id)
        })
    }

    /// Wait for a notification with the given `method` whose `params.uri` and
    /// `params.version` equal `uri`/`version`. Returns the matched
    /// notification so the caller can assert on its `params` (e.g.
    /// `diagnostics`).
    ///
    /// Used as a deterministic barrier in place of a fixed-duration sleep:
    /// observing the notification the server published *for the exact
    /// uri+version just sent* proves it finished handling that message, rather
    /// than hoping a wall-clock delay was long enough under CPU load. (What
    /// reify-lsp does internally between receiving the message and publishing
    /// is deliberately not restated here — that is production control flow, and
    /// the phase-4b assertions in `lsp_full_interactive_loop_through_binary`
    /// are the actual proof.)
    ///
    /// `version` is required, not optional, for correctness: the message
    /// stream is a FIFO of every notification the server has already
    /// published, so for a `uri` that was published before, a
    /// `method`+`uri`-only match can return a *stale* already-queued
    /// notification and provide no barrier at all.
    fn notification(&mut self, method: &str, uri: &str, version: i64) -> serde_json::Value {
        self.wait_for(
            &format!("{method} v{version} notification for uri={uri}"),
            |msg| {
                msg.get("method").and_then(|v| v.as_str()) == Some(method)
                    && msg["params"]["uri"].as_str() == Some(uri)
                    && msg["params"]["version"].as_i64() == Some(version)
            },
        )
    }

    /// Wait for a `window/logMessage` notification whose `params.message`
    /// contains `needle`. Returns the matched notification so the caller can
    /// assert on `params.type` and `params.message`.
    ///
    /// A separate matcher from [`LspInbox::notification`] because
    /// `window/logMessage`'s params are `{type, message}` (see the LSP spec's
    /// `LogMessageParams`) — there is no `uri` and no `version`, so the
    /// uri+version match that makes `notification` a sound barrier has
    /// nothing to key on here. `needle` carries the discrimination instead:
    /// callers pick a substring unique to the line (and, in the burst test
    /// below, unique to the specific notification) they are waiting for.
    ///
    /// Same 30s inactivity timeout and panic-message discipline as its siblings — see
    /// [`LspInbox::wait_for`].
    fn log_message(&mut self, needle: &str) -> serde_json::Value {
        self.wait_for(
            &format!("window/logMessage notification containing {needle:?}"),
            |msg| {
                msg.get("method").and_then(|v| v.as_str()) == Some("window/logMessage")
                    && msg["params"]["message"]
                        .as_str()
                        .is_some_and(|m| m.contains(needle))
            },
        )
    }

    /// Consume every message that is buffered or still queued, returning all
    /// of them.
    ///
    /// Sound ONLY after the child has exited: `spawn_reader`'s thread returns
    /// on EOF and drops its sender, which is what ends the loop below. Called
    /// before that it would wait for the rest of the session, so it is bounded
    /// by the same [`IDLE_TIMEOUT`] as [`LspInbox::wait_for`] and
    /// panics rather than hanging — this file's discipline is never hang,
    /// always fail. Takes `self` by value so a caller cannot reuse a drained
    /// inbox and mistake "the stream ended" for "nothing matched".
    ///
    /// Exists so a test can assert an EXACT notification count — "no message
    /// of this kind is left over" is only decidable once the stream is known
    /// to be complete.
    fn drain_after_exit(mut self) -> Vec<serde_json::Value> {
        let mut all = std::mem::take(&mut self.seen);
        loop {
            match self.rx.recv_timeout(IDLE_TIMEOUT) {
                Ok(msg) => all.push(msg),
                // The reader thread hit EOF and dropped its sender: the
                // stream really is complete, which is the whole precondition
                // an exact-count assertion rests on.
                Err(mpsc::RecvTimeoutError::Disconnected) => return all,
                Err(mpsc::RecvTimeoutError::Timeout) => panic!(
                    "no message for 30s while draining the message stream — the child had \
                     supposedly exited, so `spawn_reader` should have reached EOF and dropped \
                     its sender. Either it is still alive (drain_after_exit called too early) \
                     or the reader thread is stuck. Drained {} messages before giving up.",
                    all.len()
                ),
            }
        }
    }
}

/// Extract the ERROR-severity entries (LSP `severity == 1`, i.e.
/// `DiagnosticSeverity::ERROR`; see the numeric mapping already asserted by
/// `crates/reify-lsp/tests/in_process_bridge.rs`) from a
/// `textDocument/publishDiagnostics` notification returned by
/// [`LspInbox::notification`]. Returns owned clones (diagnostics are tiny) so
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
    inbox: LspInbox,
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
    inbox: LspInbox,
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
    LspInbox,
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

    let mut inbox = LspInbox::new(spawn_reader(stdout));
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
    let init_response = inbox.response(1);
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

    (child, stdin, inbox, init_response, stderr)
}

/// Spawns `reify lsp` with stderr drained the same way stdout is drained —
/// use this when the test needs the child to run to completion without
/// stderr backpressure. Pairs with `wait_for_exit`, which expects exactly
/// this `stderr_reader` shape. See `spawn_lsp_and_initialize`'s doc
/// comment for the shared spawn + handshake sequence.
fn spawn_lsp_drained() -> DrainedLspSession {
    let (child, stdin, inbox, init_response, stderr_reader) =
        spawn_lsp_and_initialize(spawn_pipe_reader);
    DrainedLspSession {
        child,
        stdin,
        inbox,
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
    let (child, stdin, inbox, _init_response, stderr_pipe) = spawn_lsp_and_initialize(|pipe| pipe);
    UndrainedLspSession {
        child,
        stdin,
        inbox,
        stderr_pipe,
    }
}

/// The leading, index-carrying segment of
/// [`huge_unknown_uri_did_change`]'s URI — what identifies WHICH burst
/// iteration produced a given log line.
///
/// Deliberately short and FRONT-loaded: `truncate_for_log`
/// (crates/reify-lsp/src/server.rs) keeps only the first
/// `LOG_STR_MAX_CHARS` characters of the URI, so a discriminator placed
/// anywhere later — including the `.ri` suffix — is gone by the time the
/// line reaches the client and cannot be matched against. Shared by the URI
/// builder and the burst test's matcher so the two cannot drift.
fn huge_unknown_uri_prefix(index: usize) -> String {
    format!("file:///tmp/u{index}-")
}

/// Builds task #6162's trigger: a `textDocument/didChange` for a
/// never-opened URI with a deliberately huge (160 KiB) path, so
/// `DocumentStore::update` returns `false` and `did_change`'s unknown-URI
/// `eprintln!` fires (see `crates/reify-lsp/src/server.rs`). Returns the URI
/// alongside the JSON-RPC body so callers can both send the notification and
/// match `LspInbox::notification`/`LspInbox::response` against the same
/// string.
///
/// Single definition shared by `lsp_full_interactive_loop_through_binary`'s
/// phase 4b,
/// `lsp_survives_huge_unknown_uri_didchange_with_undrained_stderr` and
/// `lsp_repeated_unknown_uri_didchange_logs_over_the_protocol_channel_without_wedging`
/// — those tests are halves of one regression guard (bounded logging, no
/// wedge, and one bounded log line per notification) for the *same*
/// trigger, so a future change to the URI's size or shape cannot update one
/// copy without the others (task #6162, extended by #6329).
///
/// `index` distinguishes the URIs within one burst (see
/// [`huge_unknown_uri_prefix`]); single-trigger callers pass 0.
fn huge_unknown_uri_did_change(index: usize, version: i64) -> (String, serde_json::Value) {
    let uri = format!(
        "{}{}.ri",
        huge_unknown_uri_prefix(index),
        "a".repeat(160 * 1024)
    );
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
/// which makes `did_change`'s unknown-URI server log line
/// (crates/reify-lsp/src/server.rs) fire.
///
/// Since task #6329 landed, that line travels as a `window/logMessage`
/// JSON-RPC notification on the same stdout stream the client must already
/// drain to receive responses — NOT on stderr, which is where task #6162
/// found it and bounded it. The phase-4b assertions at the end of this
/// function are the end-to-end guard for that channel: the notification
/// arrives, carries `MessageType::WARNING` (`type == 2`), is still bounded
/// by `truncate_for_log` (a low-single-digit-KiB bound DERIVED from
/// reify-lsp's own published `LOG_STR_MAX_BYTES`, not a number transcribed
/// here), still carries the elision marker so that bound is not vacuously
/// met — and the diagnostic no longer appears on stderr at all.
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
/// `LspInbox::notification`, blocking on that phase's own `publishDiagnostics`
/// notification rather than a fixed sleep: tower-lsp dispatches
/// requests/notifications with a concurrency level > 1, so a wall-clock
/// delay is not a reliable proxy for "the server has processed this
/// specific message" under the CPU-saturation conditions this file already
/// designs around (see `LspInbox::wait_for`'s doc comment). Phases 2-4 also
/// assert ERROR diagnostics are absent/present/absent across the
/// valid → violating → valid sequence, so this test would fail if the LSP
/// stopped wiring `did_open`/`did_change` to the diagnostics engine, not
/// just if it stopped draining stderr. That is deliberately the *wiring*
/// only — the diagnostic semantics are owned in-process by
/// `reify-lsp`'s `diagnostics` tests, and both payloads come from the
/// `reify_test_support` fixtures those tests use, so the two cannot drift
/// apart.
///
/// Phase 4b's non-vacuity anchor is the `window/logMessage` wait itself:
/// proof the unknown-URI diagnostic still fires at all, so the
/// bounded-length assertion beside it cannot pass simply because the log
/// line vanished entirely. What phase 4b does NOT assert is that the line
/// is absent from stderr: that is task #6329's migration guard, and its
/// single home is
/// `lsp_repeated_unknown_uri_didchange_logs_over_the_protocol_channel_without_wedging`,
/// which asserts it where it can actually bite — against an UNDRAINED
/// stderr pipe, under eight triggers rather than one, alongside the byte
/// budget that says WHY a stray stderr line matters.
#[test]
fn lsp_full_interactive_loop_through_binary() {
    let _lock = acquire_lsp_test_lock();
    // See `spawn_lsp_and_initialize`'s doc comment for the spawn +
    // handshake sequence, including why the reader-thread ordering is
    // load-bearing. Wrapped in KillOnDrop (see its doc comment above) so
    // every panic site below — inbox.notification and the stderr
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
        mut inbox,
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
    let diag_open = inbox.notification(
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
    let diag_violating = inbox.notification(
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
    let diag_valid_again = inbox.notification(
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
    // via didOpen, so did_change's unknown-URI log line (server.rs) fires.
    // The line is asserted below to arrive as a bounded window/logMessage
    // notification. Its ABSENCE from stderr — the other half of task
    // #6329's migration guard — is asserted once, in the burst test (see
    // rustdoc above), not a third time here.
    let (huge_uri, did_change_unknown_uri) = huge_unknown_uri_did_change(0, 4);
    send_jsonrpc(&mut stdin, &did_change_unknown_uri.to_string());

    // Deterministic barrier (see rustdoc above): block until the server
    // publishes diagnostics for the huge URI (version 4), proving
    // did_change's handler — including the unknown-URI log call — has
    // already run, instead of hoping a fixed sleep was long enough.
    // `child` is wrapped in `KillOnDrop` (see above), so a panic at any
    // wait below kills and reaps the `reify lsp` process as this function's
    // stack unwinds rather than leaving it for the test process to clean up.
    //
    // The barrier and the log-message wait below race by construction:
    // `ClientSink` dispatches each notification on its own spawned task, so
    // they can arrive in either order. `LspInbox` buffers whichever lands
    // first, which is exactly why the raw-receiver waits it replaced could
    // not be used here (see its doc comment).
    inbox.notification("textDocument/publishDiagnostics", &huge_uri, 4);

    // The task #6329 channel assertions. The wait itself is the non-vacuity
    // anchor: it can only return if the unknown-URI diagnostic actually
    // fired, over the protocol channel.
    let log_notification = inbox.log_message("didChange for unknown URI");
    assert_eq!(
        log_notification["params"]["type"].as_i64(),
        Some(2),
        "expected the unknown-URI log line to carry MessageType::WARNING (type 2) — a \
         didChange for a never-opened URI is a client protocol violation the user may need \
         to see, and must not be flattened onto the same level as an internal ERROR. Got: {}",
        serde_json::to_string_pretty(&log_notification["params"]).unwrap()
    );
    let logged_message = log_notification["params"]["message"]
        .as_str()
        .expect("window/logMessage params.message should be a JSON string");
    // Task #6162's truncation guard, re-pointed onto the channel task #6329
    // moved the line to. Derived, not hand-transcribed, so a future bump of
    // reify-lsp's LOG_STR_MAX_CHARS mechanically raises this bound instead
    // of silently under-covering the real worst case (task #6162 amendment
    // review): `reify_lsp::server::LOG_STR_MAX_BYTES` is truncate_for_log's
    // own published worst-case output length, plus this file's headroom for
    // the message prefix ("[reify-lsp] didChange for unknown URI: ",
    // 39 bytes) — comfortably below the 160 KiB URI either way. A failure
    // here means the truncation regressed (or never happened).
    let max_expected_message_bytes = reify_lsp::server::LOG_STR_MAX_BYTES + 128;
    assert!(
        logged_message.len() < max_expected_message_bytes,
        "expected <{max_expected_message_bytes} bytes in the window/logMessage message from \
         phase 4b's huge-URI didChange (reify_lsp::server::LOG_STR_MAX_BYTES = {}, +128 bytes \
         headroom for the message prefix), got {} bytes. This means the did_change unknown-URI \
         log line is not being truncated. Message: {}",
        reify_lsp::server::LOG_STR_MAX_BYTES,
        logged_message.len(),
        elide(logged_message)
    );
    // Proves the bounded-length assertion above isn't vacuously satisfied by
    // the URI shrinking — the elision marker must actually have fired.
    assert!(
        logged_message.contains("[truncated,"),
        "expected the window/logMessage message to contain truncate_for_log's elision marker \
         (\"[truncated, N bytes total]\"), proving the huge URI was actually truncated. \
         Message: {}",
        elide(logged_message)
    );

    // 5) Shutdown + exit
    let shutdown = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown",
        "params": null
    });
    send_jsonrpc(&mut stdin, &shutdown.to_string());
    let _shutdown_response = inbox.response(2);

    let exit = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null
    });
    send_jsonrpc(&mut stdin, &exit.to_string());

    drop(stdin);

    // 30s deadlock/flakiness backstop for contended CI (mirrors
    // LspInbox::wait_for's CPU-saturation rationale above), not a
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
}

/// Repetition guard for task #6329's REMAINING RISK 2: an already-bounded
/// log line repeated without bound. A burst of unknown-URI `didChange`s
/// must produce exactly one bounded `window/logMessage` per notification —
/// none dropped, none coalesced, no duplicates — and leave the server able
/// to shut down cleanly.
///
/// This is the end-to-end evidence for the decision NOT to add a per-URI
/// rate limiter: after the migration each unknown-URI `didChange` emits one
/// bounded notification on the same stdout JSON-RPC stream that already
/// carries one `publishDiagnostics` per `didChange`, so the log adds no new
/// hazard class beyond what the diagnostics path already imposes. If that
/// stops being true, this test is what says so.
///
/// Stderr is deliberately left UNDRAINED (`spawn_lsp_undrained`) for the
/// whole burst, and `stderr_pipe` is held to the end of the function to
/// sustain it. That is what makes "without wedging" a real claim rather
/// than a tautology: were the diagnostic still going to stderr, eight
/// ~1.1 KiB lines would exceed the ~4 KiB worst-case single-page pipe
/// capacity documented on `spawn_pipe_reader` and park the writer in
/// `pipe_write` — so this test would fail at the first wait below rather
/// than pass vacuously. (Measured in task #6162 at one trigger: with stderr
/// piped and never drained the pre-fix binary did not exit within 20s, its
/// main thread's `/proc/<pid>/task/<tid>/wchan` reading `pipe_write`; with
/// stderr drained it exited rc=0 in 0.05s having written 163_895 bytes.) On
/// stdout, which IS drained via `spawn_reader`, the same eight lines are far
/// inside any pipe budget — and leaving stdout undrained too would be a
/// different wedge that proves nothing about stderr.
///
/// Holding the stderr read end ALIVE rather than dropping it is load-bearing:
/// dropping it would give the child EPIPE/SIGPIPE on its next stderr write
/// instead of pipe-full backpressure — a different failure mode that would
/// make this test silently vacuous — so `stderr_pipe`'s lifetime must span
/// every assertion, and it is read only at the very end, after the child has
/// exited and closed its write end. That `read_to_string` therefore returns
/// immediately without ever having drained the pipe during the window under
/// test.
///
/// The two post-exit assertions are what keep all of that honest, and this
/// test is their single home (task #6329 amendment review). The absence
/// assertion is the migration's own guard — the unknown-URI diagnostic must
/// travel on the protocol channel INSTEAD of stderr, not on both — asserted
/// here, under eight triggers against an undrained pipe, rather than a third
/// time in a drained-stderr test where a stray line is harmless. The
/// `< 4096` bound polices the shared headroom the paragraph above depends
/// on: this test's trigger writes nothing to stderr today, so the whole
/// single-page capacity is nominally free, but any stderr line a future
/// change adds to reify-lsp spends directly against it — and once spent it
/// wedges this test at the first wait above with a message that would read
/// as "the migration regressed" for what is really a budget overrun. The
/// bound names that, so the diagnosis lands where the cost was incurred.
///
/// Not gated `#[cfg(unix)]`, unlike this file's `/bin/sh`-stub tests: it
/// drives `CARGO_BIN_EXE_reify` through the same portable `Command`/`Stdio`
/// surface as `lsp_full_interactive_loop_through_binary` and uses no
/// unix-only API, so gating it would silently drop task #6329's primary
/// end-to-end regression test on non-unix targets for no reason.
///
/// Each iteration uses a DISTINCT huge URI whose index-carrying prefix
/// (see `huge_unknown_uri_prefix`) survives `truncate_for_log`'s cut, so
/// every wait matches exactly one notification and a coalesced or dropped
/// line surfaces as a timeout naming the iteration that went missing —
/// not as an off-by-one in a bare count. The leftover scan after exit is
/// what turns "at least one each" into "exactly one each".
///
/// Eight is a fixed, deterministic count, not a tolerance: one log line per
/// notification is an exact property.
///
/// Expect this test to cost ~30s while RED — that is the first
/// `window/logMessage` wait timing out as designed, not a hang.
#[test]
fn lsp_repeated_unknown_uri_didchange_logs_over_the_protocol_channel_without_wedging() {
    let _lock = acquire_lsp_test_lock();
    let UndrainedLspSession {
        mut child,
        mut stdin,
        mut inbox,
        mut stderr_pipe,
    } = spawn_lsp_undrained();

    const BURST: usize = 8;

    let uris: Vec<String> = (1..=BURST)
        .map(|index| {
            let (uri, body) = huge_unknown_uri_did_change(index, index as i64);
            send_jsonrpc(&mut stdin, &body.to_string());
            uri
        })
        .collect();

    // One bounded WARNING per notification, matched by that iteration's own
    // URI prefix so a missing or coalesced line names itself.
    for index in 1..=BURST {
        let needle = format!(
            "didChange for unknown URI: {}",
            huge_unknown_uri_prefix(index)
        );
        let log_notification = inbox.log_message(&needle);
        assert_eq!(
            log_notification["params"]["type"].as_i64(),
            Some(2),
            "burst iteration {index}: expected MessageType::WARNING (type 2), got: {}",
            serde_json::to_string_pretty(&log_notification["params"]).unwrap()
        );
        let logged_message = log_notification["params"]["message"]
            .as_str()
            .expect("window/logMessage params.message should be a JSON string");
        assert!(
            logged_message.contains("[truncated,"),
            "burst iteration {index}: repetition must not cost truncation — every line in the \
             burst has to stay bounded, or the per-line bound is no defence at all. \
             Message: {}",
            elide(logged_message)
        );
    }

    // Each handler also ran to completion, so the shutdown below means
    // "after the burst" rather than "racing it".
    for (offset, uri) in uris.iter().enumerate() {
        inbox.notification("textDocument/publishDiagnostics", uri, offset as i64 + 1);
    }

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

    // Same discipline as `lsp_survives_huge_unknown_uri_didchange_with_undrained_stderr`:
    // not `wait_for_exit` (it drains stderr), no exit-code assertion and no
    // wait on the shutdown response (tower-lsp's dispatch ordering makes
    // both flaky) — but signal death is unambiguous and ordering-independent,
    // so it is checked.
    let status = wait_for_exit_no_stderr(&mut child, 30).unwrap_or_else(|| {
        panic!(
            "reify lsp did not exit within 30s after a burst of {BURST} unknown-URI \
             didChanges with stderr piped but never drained. Either repetition of the log \
             line has wedged the server, or this test's stderr budget (see the doc comment) \
             has been spent and the writer is parked in pipe_write on a full stderr pipe. \
             The post-exit budget assertion below is what distinguishes the two whenever \
             the process does exit; it cannot run here, so check reify-lsp for a newly \
             added stderr write before concluding the log channel wedged."
        )
    });
    assert!(
        status.code().is_some(),
        "reify lsp died from a signal ({status:?}) rather than exiting normally after a burst \
         of {BURST} unknown-URI didChanges"
    );

    // Exactly one per notification, not merely at least one: the child has
    // exited, so the stream is complete and anything still unclaimed is a
    // duplicate or an uncoalesced extra.
    let leftover_log_lines = inbox
        .drain_after_exit()
        .into_iter()
        .filter(|msg| {
            msg.get("method").and_then(|v| v.as_str()) == Some("window/logMessage")
                && msg["params"]["message"]
                    .as_str()
                    .is_some_and(|m| m.contains("didChange for unknown URI"))
        })
        .count();
    assert_eq!(
        leftover_log_lines, 0,
        "expected exactly {BURST} unknown-URI window/logMessage notifications for {BURST} \
         didChanges — one per notification — but {leftover_log_lines} further ones were left \
         unclaimed after the {BURST} matched waits above"
    );

    // Only now, with the child exited and its write end closed, is the
    // stderr pipe read — see the doc comment for why its lifetime had to
    // span every assertion above.
    let mut stderr_after_exit = String::new();
    stderr_pipe
        .read_to_string(&mut stderr_after_exit)
        .expect("reading stderr after the child has already exited should not fail");
    let stderr_summary = elide(&stderr_after_exit);
    assert!(
        !stderr_after_exit.contains("didChange for unknown URI"),
        "expected the unknown-URI diagnostic to have left stderr entirely for \
         window/logMessage (task #6329), but the child still wrote it to stderr — where, \
         under the undrained pipe this test sustains, {BURST} of them are exactly the \
         blocking write task #6162 was about. Captured stderr: {stderr_summary}"
    );
    assert!(
        stderr_after_exit.len() < 4096,
        "this test's stderr budget was spent: total stderr must stay under a kernel-shrunk \
         single-page pipe (4096 bytes), because that headroom is what lets the burst above \
         run against an UNDRAINED pipe without parking the writer in pipe_write. A stderr \
         write added here or in reify-lsp has spent it; got {} bytes. This is a budget \
         overrun, NOT a regression of the #6329 channel migration. Captured stderr: \
         {stderr_summary}",
        stderr_after_exit.len()
    );
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
/// designed around (see `LspInbox::wait_for`'s 30s). Under a 24-way-parallel
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
    const BACKPRESSURE_MARKER: &str = "REIFY_6162_BACKPRESSURE_MARKER";
    const CHUNK_BYTES: usize = 1024;
    const CHUNK_COUNT: usize = 256;
    const PAYLOAD_BYTES: usize = CHUNK_BYTES * CHUNK_COUNT;

    let script = format!(
        "printf '{BACKPRESSURE_MARKER}\n' >&2; i=0; while [ $i -lt {CHUNK_COUNT} ]; do printf '%{CHUNK_BYTES}s' '' >&2; i=$((i+1)); done"
    );
    let (mut guard, stderr_reader) = spawn_sh_stub(&script);

    let (status, stderr) = wait_for_exit(&mut guard, 30, stderr_reader);
    let stderr_summary = elide(&stderr);
    assert!(
        status.success(),
        "stub should exit cleanly after writing its deterministic payload (stderr: {stderr_summary})"
    );
    let marker_and_newline = BACKPRESSURE_MARKER.len() + 1;
    let expected_len = marker_and_newline + PAYLOAD_BYTES;
    assert_eq!(
        stderr.len(),
        expected_len,
        "expected exactly {expected_len} bytes of captured stderr ({marker_and_newline}-byte \
         marker+newline plus the stub's deterministic {CHUNK_COUNT} * {CHUNK_BYTES} = \
         {PAYLOAD_BYTES}-byte payload) — a mismatch means the drain dropped or duplicated \
         bytes, not merely fell behind. Captured stderr: {stderr_summary}"
    );
    assert!(
        stderr.contains(BACKPRESSURE_MARKER),
        "expected the captured stderr to contain the stub's marker, proving the bytes came \
         from this test's own trigger. Captured stderr: {stderr_summary}"
    );
}
