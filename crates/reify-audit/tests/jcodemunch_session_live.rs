//! Live boundary assertions for the jcodemunch MCP session handshake.
//!
//! This file spawns its OWN transient `jcodemunch-mcp serve` via `uvx` and
//! drives a real handshake against it. Two complementary claims:
//!
//! * **B1** — our client honours the contract: [`JcodemunchClient::new`]
//!   completes the handshake against a real serve, and `list_tools()` returns
//!   the tools the detectors actually call.
//! * **B2** — the server *enforces* the contract, which is what makes B1
//!   mandatory rather than stylistic: an `initialize` carrying a
//!   client-minted session id is answered `404`, while the otherwise
//!   identical request with no session header is answered `200` and assigns
//!   one.
//!
//! B2 is driven with raw `ureq`, deliberately NOT through `JcodemunchClient`.
//! The claim under test is a property of jcodemunch, not of our client;
//! routing it through the client could only show that the client fails, and
//! once the client is fixed there would be no way to express the assertion at
//! all. Pairing it with the without-id control means a 404 caused by an
//! unhealthy serve cannot masquerade as the finding.
//!
//! ## No graceful skip, by design
//!
//! A missing `uvx`, an unreachable PyPI, or a serve that never becomes ready
//! is a hard FAIL here, never a silent return. A skip that reports PASS is
//! precisely the vacuity this task exists to remove — the seam was dead for
//! ten weeks behind exactly that shape. The complementary half is the
//! `#[ignore]` gate: a spawned serve needs `uvx` and network, both
//! legitimately absent in a task worktree, so this must never be
//! gate-resident.
//!
//! Run it deliberately:
//! ```sh
//! cargo test -p reify-audit --test jcodemunch_session_live -- --ignored --nocapture
//! ```
//!
//! ## The sibling live test still skips — deliberately not fixed here
//!
//! `tests/jcodemunch_live.rs` does the opposite of the paragraph above: it
//! requires an externally-running serve and graceful-skips when
//! `jcodemunch_serve_reachable` is false. Two contradictory policies for
//! live-serve tests against the same seam is a real inconsistency, and it is
//! knowingly left standing: that file belongs to task #6110 (the non-vacuous
//! live capstone), whose whole subject is replacing that skip, and it is
//! outside this task's locked module set. The [`Serve`] harness below —
//! spawn, `await_ready`, teardown — is the reusable half; #6110 should lift
//! it into `tests/common/` and drive `jcodemunch_live.rs` from it rather than
//! growing a second copy, keeping `JCODEMUNCH_URL` as an opt-in override for
//! an already-running serve.

use std::io::Read;
use std::net::TcpListener;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use reify_audit::jcodemunch_client::JcodemunchClient;
use serde_json::{json, Value};

/// The jcodemunch-mcp release this test pins.
///
/// 1.108.27 (the version some older notes cite) is no longer on PyPI — only
/// its git tag survives — so `uvx --from jcodemunch-mcp==1.108.27` cannot
/// resolve. 1.108.54 is on PyPI and matches the watcher pin.
const JCODEMUNCH_PIN: &str = "1.108.54";

/// Mirrors `jcodemunch_client`'s private `PROTOCOL_VERSION`. Duplicated
/// rather than exported: this test speaks the wire protocol directly, and a
/// `pub` const would widen the client's surface for a test-only reason.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// The tools `RealJCodemunchOps` actually calls. `list_tools()` must offer
/// every one of them or the detector seam is live in name only.
const REQUIRED_TOOLS: &[&str] = &[
    "get_changed_symbols",
    "find_references",
    "get_dead_code_v2",
    "get_untested_symbols",
    "get_layer_violations",
];

/// Cold start measured on this host: ~37 s with the wheels already in the uv
/// cache. The ceiling is generous because a cold uv cache must also fetch
/// from PyPI; exceeding it is a hard failure, not a skip.
const READY_TIMEOUT: Duration = Duration::from_secs(180);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(1000);

// -----------------------------------------------------------------------
// Serve harness
// -----------------------------------------------------------------------

/// Resolve the `uvx` binary, or panic naming the prerequisite.
fn resolve_uvx() -> PathBuf {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join("uvx");
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    let fallback = PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".local")
        .join("bin")
        .join("uvx");
    assert!(
        fallback.is_file(),
        "`uvx` not found on PATH nor at {} — this live test spawns its own \
         jcodemunch serve and cannot run without it. Install uv \
         (https://docs.astral.sh/uv/) rather than skipping: a skip here \
         reports PASS for a seam that was never exercised.",
        fallback.display(),
    );
    fallback
}

/// Ask the kernel for a free loopback port, then release it.
///
/// This leaves the classic drop-then-rebind window, which is why readiness is
/// decided by the serve's own identity (below) rather than by a bare TCP
/// connect — a squatter that wins the port would answer a connect happily.
fn pick_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback for port pick");
    listener.local_addr().expect("local_addr").port()
}

/// A spawned `jcodemunch-mcp serve`, torn down on drop.
struct Serve {
    child: Child,
    port: u16,
    logs: tempfile::TempDir,
}

impl Serve {
    /// Spawn a serve on a free loopback port and block until it answers a
    /// real `initialize` as jcodemunch. Panics — never skips — on any
    /// failure, printing whatever the serve wrote.
    fn spawn() -> Self {
        let uvx = resolve_uvx();
        let port = pick_port();
        let logs = tempfile::tempdir().expect("tempdir for serve logs");
        let out = std::fs::File::create(logs.path().join("serve.out")).expect("serve.out");
        let err = std::fs::File::create(logs.path().join("serve.err")).expect("serve.err");

        let child = Command::new(&uvx)
            .args([
                "--python",
                "3.12",
                "--from",
                &format!("jcodemunch-mcp=={JCODEMUNCH_PIN}"),
                "jcodemunch-mcp",
                "serve",
                "--transport",
                "streamable-http",
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                // The file watcher indexes the whole repo on start; this test
                // only needs the MCP session seam.
                "--watcher=false",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            // Put the serve in a process group of its own (pgid == this
            // child's pid), so teardown can signal `uvx` *and* the python it
            // fronts by group id alone. Without it the only handle on the
            // grandchild is a command-line pattern match across every process
            // on the host — see `Drop`.
            .process_group(0)
            .spawn()
            .unwrap_or_else(|e| panic!("spawn {} serve on port {port}: {e}", uvx.display()));

        let mut serve = Self { child, port, logs };
        serve.await_ready();
        serve
    }

    /// `/mcp` with NO trailing slash: a real serve 307-redirects `/mcp/` and
    /// the redirect drops `mcp-session-id`.
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/mcp", self.port)
    }

    /// Whatever the serve has written so far, for failure messages.
    fn output(&self) -> String {
        let read = |name: &str| {
            std::fs::read_to_string(self.logs.path().join(name))
                .unwrap_or_else(|e| format!("<unreadable: {e}>"))
        };
        format!(
            "--- serve stdout ---\n{}\n--- serve stderr ---\n{}",
            read("serve.out"),
            read("serve.err"),
        )
    }

    /// Block until the endpoint answers `initialize` as jcodemunch itself.
    ///
    /// Identity, not liveness: `result.serverInfo.name == "jcodemunch-mcp"`
    /// is positive proof that the endpoint under test is the serve this test
    /// spawned, which closes the port-recycling window `pick_port` opens.
    fn await_ready(&mut self) {
        let deadline = Instant::now() + READY_TIMEOUT;
        let url = self.url();
        let mut last: String = "no attempt completed".to_string();

        while Instant::now() < deadline {
            // A serve that died (bad pin, no network, port already taken) will
            // never become ready — fail now rather than burn the whole timeout
            // and then blame a timeout for what was really a spawn failure.
            if let Some(status) = self.child.try_wait().expect("try_wait on serve child") {
                panic!(
                    "jcodemunch serve exited early with {status} before becoming \
                     ready at {url}\n{}",
                    self.output(),
                );
            }

            match post_initialize(&url, None) {
                Ok((status, session, body)) => {
                    let name = body
                        .get("result")
                        .and_then(|r| r.get("serverInfo"))
                        .and_then(|s| s.get("name"))
                        .and_then(|n| n.as_str());
                    if status == 200 && name == Some("jcodemunch-mcp") {
                        assert!(
                            session.is_some_and(|s| !s.is_empty()),
                            "serve answered a healthy initialize at {url} but assigned \
                             no mcp-session-id — the session contract is not being \
                             honoured server-side, so nothing below can be trusted",
                        );
                        return;
                    }
                    last = format!(
                        "status={status}, serverInfo.name={name:?} (expected 200 / \
                         Some(\"jcodemunch-mcp\"))"
                    );
                }
                Err(e) => last = format!("POST {url}: {e}"),
            }
            std::thread::sleep(READY_POLL_INTERVAL);
        }

        panic!(
            "jcodemunch serve never became ready at {url} within {:?}; last probe: \
             {last}\n{}",
            READY_TIMEOUT,
            self.output(),
        );
    }
}

impl Drop for Serve {
    fn drop(&mut self) {
        // `uvx` fronts a child python that a bare `child.kill()` would orphan,
        // so signal the whole process GROUP: `spawn` put the serve in a fresh
        // group whose id is this child's pid, so the negated pid below names
        // that group and nothing else. A `pkill -f` pattern would not — it is
        // an unanchored substring match against every command line on the
        // host, so `--port 8917` also matches a `--port 89170` serve, and the
        // blast radius is somebody else's long-lived watcher.
        let group = format!("-{}", self.child.id());
        let signal_group = |sig: &str| {
            let _ = Command::new("kill")
                .args([sig, &group])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        };
        signal_group("-TERM");

        // Bounded wait for the port to actually stop accepting, so a
        // following test cannot be handed a half-dead listener; a group still
        // listening half-way through gets one KILL escalation. The child is
        // deliberately left unreaped until after this loop: an unreaped pid
        // cannot be recycled, so the group id stays unambiguously ours for as
        // long as we are signalling it.
        let deadline = Instant::now() + Duration::from_secs(10);
        let escalate_at = Instant::now() + Duration::from_secs(5);
        let mut escalated = false;
        let mut freed = false;
        while Instant::now() < deadline {
            let addr = format!("127.0.0.1:{}", self.port);
            match std::net::TcpStream::connect_timeout(
                &addr.parse().expect("loopback addr"),
                Duration::from_millis(100),
            ) {
                Ok(_) => {
                    if !escalated && Instant::now() >= escalate_at {
                        signal_group("-KILL");
                        escalated = true;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(_) => {
                    freed = true;
                    break;
                }
            }
        }

        let _ = self.child.kill();
        let _ = self.child.wait();

        if !freed {
            eprintln!(
                "warning: port {} still accepting connections after teardown",
                self.port,
            );
        }
    }
}

// -----------------------------------------------------------------------
// Raw MCP helpers (deliberately not JcodemunchClient)
// -----------------------------------------------------------------------

fn initialize_payload() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": PROTOCOL_VERSION,
            "clientInfo": {"name": "reify-audit-session-live-test", "version": "0.1"},
            "capabilities": {},
        },
    })
}

/// A live serve answers `initialize` with `content-type: text/event-stream`,
/// so the JSON-RPC envelope arrives on a `data:` line rather than as the whole
/// body.
///
/// **This is the third copy of this decode.** The canonical one is
/// `JcodemunchClient::post_raw` in `src/jcodemunch_client.rs`; a second lives
/// in `src/fused_memory_client.rs`. They have already drifted — both
/// production copies return `LoadError::Protocol` where this one panics,
/// which is right for a test harness (a body we cannot decode is a harness
/// fault, not a claim under test) but means a protocol fix (multi-line SSE
/// `data:`, `event:` filtering, chunked framing) has to be applied in three
/// places. Deduplicating it needs a shared module that both clients use, and
/// `fused_memory_client.rs` is explicitly out of this task's scope (PRD §9),
/// so the copy stands and the drift is at least written down. Filed as
/// follow-up work; check `tools/list`-adjacent MCP envelope handling in all
/// three before changing any one of them.
fn parse_mcp_body(ctype: &str, body: &str) -> Value {
    if ctype.contains("text/event-stream") {
        for line in body.lines() {
            if let Some(rest) = line.strip_prefix("data:") {
                return serde_json::from_str(rest.trim())
                    .unwrap_or_else(|e| panic!("parse SSE data line: {e}; body={body}"));
            }
        }
        panic!("no SSE data line in response body: {body}");
    }
    if body.trim().is_empty() {
        return Value::Null;
    }
    serde_json::from_str(body).unwrap_or_else(|e| panic!("parse JSON body: {e}; body={body}"))
}

/// POST `initialize` to `url`, attaching `mcp-session-id` only when `session`
/// is `Some`. Returns `(status, assigned session id, parsed body)`.
///
/// The error type stays `ureq::Error` rather than a rendered string so B2 can
/// match `Error::Status(404, _)` structurally — a substring match on the
/// Display text would also accept a 404 mentioned anywhere in a transport
/// error, which is exactly the looseness this assertion cannot afford. It is
/// boxed only to satisfy `clippy::result_large_err`; the variants stay
/// matchable through the box.
/// Failing to read the body of a response we did receive is a harness fault,
/// not a claim under test, so it panics rather than folding into this Result.
fn post_initialize(
    url: &str,
    session: Option<&str>,
) -> Result<(u16, Option<String>, Value), Box<ureq::Error>> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    let mut request = agent
        .post(url)
        .set("Content-Type", "application/json")
        .set("Accept", "application/json, text/event-stream");
    if let Some(session) = session {
        request = request.set("mcp-session-id", session);
    }
    let response = request.send_json(initialize_payload())?;

    let status = response.status();
    let assigned = response.header("mcp-session-id").map(|s| s.to_string());
    let ctype = response.header("content-type").unwrap_or("").to_string();
    let mut body = String::new();
    response
        .into_reader()
        .read_to_string(&mut body)
        .unwrap_or_else(|e| panic!("read initialize response body from {url}: {e}"));
    Ok((status, assigned, parse_mcp_body(&ctype, &body)))
}

// -----------------------------------------------------------------------
// The live test
// -----------------------------------------------------------------------

/// B1 + B2 against one freshly spawned serve.
///
/// Both boundary scenarios share a serve deliberately: B2's `404` is only
/// evidence that the *session id* was rejected if the very same endpoint
/// answers the without-id control `200` moments later.
#[ignore = "live integration: spawns a transient jcodemunch serve via uvx (needs network/PyPI); run via --ignored"]
#[test]
fn live_serve_assigns_the_session_and_rejects_a_client_minted_one() {
    let serve = Serve::spawn();
    let url = serve.url();

    // --- B1: our client completes a real handshake and sees real tools ---

    let client = JcodemunchClient::new(&url).unwrap_or_else(|e| {
        panic!(
            "JcodemunchClient::new must complete the handshake against a live \
             serve at {url}; got {e:?}\n{}",
            serve.output(),
        )
    });

    let tools = client
        .list_tools()
        .unwrap_or_else(|e| panic!("list_tools against {url}: {e:?}\n{}", serve.output()));

    assert!(
        !tools.is_empty(),
        "a live serve must advertise at least one tool; got an empty list",
    );
    for required in REQUIRED_TOOLS {
        assert!(
            tools.iter().any(|t| t == required),
            "live serve does not advertise `{required}`, which RealJCodemunchOps \
             calls — the detector seam would be live in name only. Advertised: \
             {tools:?}",
        );
    }

    // --- B2: the server is what makes the contract mandatory ---

    // (a) a client-minted id names a session the server never issued.
    let minted = "deadbeefdeadbeefdeadbeefdeadbeef";
    match post_initialize(&url, Some(minted)) {
        Ok((status, _, body)) => panic!(
            "initialize carrying a client-minted session id must be rejected by \
             {url}; got status {status} and body {body}",
        ),
        Err(err) => match *err {
            ureq::Error::Status(code, _) => assert_eq!(
                code, 404,
                "initialize with a client-minted session id must be rejected 404 \
                 (Invalid or expired session ID); got status {code}",
            ),
            ureq::Error::Transport(e) => panic!(
                "expected a 404 from {url} for the client-minted session id, got a \
                 transport error instead — the serve is unhealthy, so this arm \
                 proves nothing: {e}\n{}",
                serve.output(),
            ),
        },
    }

    // (b) the control: the SAME serve, the SAME payload, no session header.
    // Without this arm, (a)'s 404 could just mean the serve is unhealthy.
    let (status, assigned, body) = post_initialize(&url, None).unwrap_or_else(|e| {
        panic!(
            "the without-session-id control must succeed against {url}; got: {e}\n{}",
            serve.output(),
        )
    });
    assert_eq!(
        status, 200,
        "initialize with no session header must be accepted; body={body}",
    );
    let assigned = assigned.expect(
        "initialize with no session header must assign an mcp-session-id — that \
         assignment is the whole contract",
    );
    assert!(
        !assigned.is_empty(),
        "the assigned mcp-session-id must not be empty",
    );
    assert_ne!(
        assigned, minted,
        "the server must mint its own id, not echo the client's",
    );
}
