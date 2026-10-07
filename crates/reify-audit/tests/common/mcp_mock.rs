//! A hermetic mock MCP streamable-HTTP server, standing in for fused-memory
//! or jcodemunch so a test can drive reify-audit's MCP clients (and the real
//! binary) without a live serve.
//!
//! A tiny blocking HTTP/1.1 server on a loopback port speaks just enough of
//! MCP streamable-HTTP to answer `initialize`, `notifications/initialized`
//! and `tools/call`. A test-supplied responder maps each `tools/call`'s
//! `arguments` to the JSON-RPC `result` to send back, or to `None` for an
//! error envelope.
//!
//! ## Session id
//!
//! The `initialize` response carries an `Mcp-Session-Id` header
//! ([`MOCK_SESSION_ID`]), because `JcodemunchClient` treats an initialize
//! response with no server-assigned session as a hard `Protocol` failure.
//! The mock assigns the header but does not police it — see
//! [`write_response_with_session`] for why enforcement is off the table.
//!
//! ## Wire framing and result shape
//!
//! Two orthogonal axes, both chosen per server. [`Framing`] selects the wire
//! framing: [`Framing::Json`] drives `mcp_wire::decode_body`'s bare-body
//! branch and [`Framing::Sse`] its `contains("text/event-stream")` branch.
//! Under SSE the mock wraps the body as a realistic
//! `event: message\ndata: <json>\n\n` frame rather than a bare `data:` line,
//! so the decode's line scan is genuinely exercised rather than getting lucky
//! on a single-line body. [`Framing::SseNoData`] and
//! [`Framing::SseMalformedData`] are the SSE branch's two failure modes — a
//! data-less keep-alive-shaped frame, and a `data:` line whose payload is not
//! valid JSON — locking the decode's "no SSE data line in response" and "SSE
//! data parse" refusals respectively. The `notifications/initialized` leg
//! always answers 202 with an empty body under ALL framings: that matches
//! real MCP, and the clients short-circuit on status 202 before they ever
//! sniff content-type, so SSE-framing that leg would be untestable fiction.
//! [`ResultShape`] independently selects the `tools/call` result envelope
//! (`structuredContent` vs the `content[0].text` fallback), mirroring
//! `call_tool`'s two decode branches.
//!
//! ## Hang-proofing
//!
//! A test that cannot stop its mock hangs the whole runner, so teardown never
//! depends on one mechanism. The accept loop polls a NON-BLOCKING listener,
//! waking every few milliseconds to check the stop flag even if no wakeup
//! connection ever lands. [`MockServer::stop`] reaches the loop through the
//! `SocketAddr` the handle carries, never by re-parsing the URL — a URL-shape
//! change would otherwise leave the wakeup aimed nowhere.
//!
//! ## Signature stability
//!
//! [`spawn_mock_mcp`], [`spawn_mock_mcp_on`], [`write_response`] and
//! [`write_response_with_session`] keep fixed signatures and delegate into
//! the framing/shape-aware cores ([`spawn_mock_mcp_on_shaped`],
//! [`write_response_framed`]). They have many call sites across unrelated
//! suites, so a new axis goes into a core and is reached by a new entry
//! point, never by changing a wrapper's signature.
//!
//! Per the `common` partial-consumer contract, every item here carries
//! `#[allow(dead_code)]` so a test binary may consume only a subset.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

/// A single HTTP request the mock's accept loop observed, captured so
/// tests can assert on it (e.g. session-id-on-every-POST). Header names
/// are stored ASCII-lowercased at capture time (`ureq` sends them
/// lowercase anyway, but [`ObservedRequest::header`] does a
/// case-insensitive lookup regardless so an assertion never depends on
/// that).
#[allow(dead_code)]
pub struct ObservedRequest {
    /// The JSON-RPC `method` field from the request body (`initialize`,
    /// `notifications/initialized`, `tools/call`) — NOT the HTTP verb,
    /// which is always POST for every request this mock accepts.
    pub rpc_method: String,
    headers: Vec<(String, String)>,
}

#[allow(dead_code)]
impl ObservedRequest {
    /// Case-insensitive header lookup by name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Read a complete HTTP/1.1 request from `stream`, returning every request
/// header (name lowercased) alongside the body as a JSON Value. Assumes
/// Content-Length is present (which `ureq` always sets for `send_json`).
/// Returns `None` on EOF / parse failure.
#[allow(dead_code)]
pub fn read_request(stream: &mut TcpStream) -> Option<(Vec<(String, String)>, serde_json::Value)> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if let Some((name, value)) = trimmed.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "content-length" {
                content_length = value.parse().ok()?;
            }
            headers.push((name, value));
        }
    }
    let body = if content_length == 0 {
        serde_json::Value::Null
    } else {
        let mut buf = vec![0u8; content_length];
        reader.read_exact(&mut buf).ok()?;
        serde_json::from_slice(&buf).ok()?
    };
    Some((headers, body))
}

/// The session id this mock assigns on `initialize`.
///
/// `JcodemunchClient` requires the server to assign one — an `initialize`
/// response with no `Mcp-Session-Id` header is a hard `Protocol` failure —
/// so without this the mock would no longer stand in for a live seam at
/// all. Deliberately not 32 lowercase hex, so it can never be confused
/// with a client-minted id.
#[allow(dead_code)]
pub const MOCK_SESSION_ID: &str = "mock-mcp-session";

/// Which wire framing the mock's accept loop answers with. Mirrors the two
/// branches `mcp_wire::decode_body` distinguishes on `content-type`:
/// [`Framing::Json`] drives the bare-JSON-body branch,
/// [`Framing::Sse`] drives the `contains("text/event-stream")`
/// branch.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq)]
pub enum Framing {
    Json,
    Sse,
    /// A degenerate SSE frame carrying no `data:` line at all — just
    /// `event: message\n\n`, as a keep-alive/comment-only chunk might
    /// look. Exercises the decode's "no SSE data line in response"
    /// refusal — one of the SSE branch's two failure modes; see
    /// [`Framing::SseMalformedData`] for the other. The `body` argument
    /// passed to [`write_response_framed`] is ignored under this
    /// variant: there is by definition no data to carry.
    SseNoData,
    /// An SSE frame WITH a `data:` line, but whose payload is not valid
    /// JSON — `event: message\ndata: {not json\n\n`. Exercises the
    /// decode's "SSE data parse" refusal, the SSE branch's other failure
    /// mode alongside [`Framing::SseNoData`]. As with `SseNoData`, the
    /// `body` argument passed to [`write_response_framed`] is ignored
    /// under this variant: the payload is fixed garbage regardless of
    /// what the caller asked to send.
    SseMalformedData,
}

/// Which JSON-RPC `result` envelope shape the mock's `tools/call` arm
/// builds. Mirrors the two branches `FusedMemoryClient::call_tool`
/// distinguishes: [`ResultShape::StructuredContent`] drives the
/// `result.structuredContent` early return; [`ResultShape::ContentText`]
/// drives the `result.content[].text` fallback. The `None` /
/// error-envelope branch (task not found) is shape-independent and stays
/// the same under either variant.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq)]
pub enum ResultShape {
    StructuredContent,
    ContentText,
}

#[allow(dead_code)]
pub fn write_response(stream: &mut TcpStream, status: u16, body: &[u8]) {
    write_response_with_session(stream, status, None, body)
}

/// As [`write_response`], but additionally emits an `Mcp-Session-Id`
/// response header when `session` is `Some`.
///
/// Only the `initialize` arm needs it: assigning the session is the
/// server's job, and every later request carries the id back on the
/// request side. The mock deliberately does NOT *enforce* the contract
/// (no 404 on an inbound id, no 400 on a missing one) — the same responder
/// backs the `--fused-memory-url` loader tests, and `fused_memory_client`
/// still mints its own id and sends it on `initialize`, so enforcement
/// would turn all of those red. The jcodemunch-side contract is locked
/// gate-resident by the hermetic unit tests in `jcodemunch_client.rs`,
/// which assert the request headers directly.
///
/// Always [`Framing::Json`] — a thin wrapper over [`write_response_framed`]
/// whose signature stays fixed (see the module doc's "Signature
/// stability"). Call `write_response_framed` directly to answer with SSE
/// framing.
#[allow(dead_code)]
pub fn write_response_with_session(
    stream: &mut TcpStream,
    status: u16,
    session: Option<&str>,
    body: &[u8],
) {
    write_response_framed(stream, status, session, Framing::Json, body);
}

/// As [`write_response_with_session`], but with the wire framing an
/// explicit parameter rather than always JSON.
///
/// Under [`Framing::Sse`] the body is wrapped as a realistic MCP
/// streamable-HTTP frame — `event: message\ndata: <json>\n\n` — rather
/// than a bare `data:` line. The `event:` line and trailing blank line
/// matter: they prove `mcp_wire::decode_body`'s SSE `body.lines()`
/// scan actually skips a non-`data:` line rather than getting lucky on a
/// single-line body. `Content-Length` is computed over the WRAPPED bytes,
/// matching what a real server would send. [`Framing::SseNoData`] and
/// [`Framing::SseMalformedData`] instead ignore `body` entirely and wrap a
/// fixed, framing-specific payload — see their own doc comments.
#[allow(dead_code)]
pub fn write_response_framed(
    stream: &mut TcpStream,
    status: u16,
    session: Option<&str>,
    framing: Framing,
    body: &[u8],
) {
    let status_text = match status {
        200 => "OK",
        202 => "Accepted",
        _ => "OK",
    };
    let content_type = match framing {
        Framing::Json => "application/json",
        Framing::Sse | Framing::SseNoData | Framing::SseMalformedData => "text/event-stream",
    };
    let framed_body = match framing {
        Framing::Json => body.to_vec(),
        Framing::Sse => {
            let mut framed = Vec::with_capacity(body.len() + 32);
            framed.extend_from_slice(b"event: message\n");
            framed.extend_from_slice(b"data: ");
            framed.extend_from_slice(body);
            framed.extend_from_slice(b"\n\n");
            framed
        }
        Framing::SseNoData => b"event: message\n\n".to_vec(),
        Framing::SseMalformedData => b"event: message\ndata: {not json\n\n".to_vec(),
    };
    let mut header = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        framed_body.len()
    );
    if let Some(session) = session {
        header.push_str(&format!("Mcp-Session-Id: {session}\r\n"));
    }
    header.push_str("\r\n");
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(&framed_body);
}

/// Handle returned by [`spawn_mock_mcp`]. Carries the bound `SocketAddr`
/// directly so the stop helper doesn't need to re-parse the URL (a brittle
/// approach that hangs the test runner forever if the URL shape ever
/// changes).
#[allow(dead_code)]
pub struct MockServer {
    url: String,
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    /// Every request the accept loop has observed so far, under both
    /// framings. See [`MockServer::observed_handle`].
    observed: Arc<Mutex<Vec<ObservedRequest>>>,
}

/// Spawn a one-shot mock MCP server on an OS-assigned ephemeral port.
/// `task_responder` is given the `tools/call` arguments and returns the
/// JSON-RPC `result` value to send back (or `None` to return an error
/// envelope). Returns a [`MockServer`] handle; the caller calls
/// [`MockServer::stop`] (or lets it drop) to tear down.
///
/// Thin wrapper over [`spawn_mock_mcp_on`], which takes an already-bound
/// listener so a caller can place the responder at a *chosen* address.
#[allow(dead_code)]
pub fn spawn_mock_mcp<F>(task_responder: F) -> MockServer
where
    F: Fn(&serde_json::Value) -> Option<serde_json::Value> + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    spawn_mock_mcp_on(listener, task_responder)
}

/// Spawn a one-shot mock MCP server on an OS-assigned ephemeral port with
/// an explicit wire `framing` and `tools/call` result `shape`. The single
/// entry point for every SSE scenario — call sites name the
/// framing/shape combination directly, e.g.
/// `spawn_mock_mcp_shaped(Framing::Sse, ResultShape::ContentText, ...)`,
/// rather than through a differently-named wrapper per combination. See
/// [`spawn_mock_mcp_on_shaped`] for the accept-loop details, including
/// exactly which leg stays JSON-shaped regardless (the
/// `notifications/initialized` 202).
///
/// `spawn_mock_mcp` and `spawn_mock_mcp_on` are deliberately NOT built on
/// this: they predate the SSE axis and keep their exact signatures (see the
/// module doc's "Signature stability"), and this function exists precisely
/// so they can.
#[allow(dead_code)]
pub fn spawn_mock_mcp_shaped<F>(
    framing: Framing,
    shape: ResultShape,
    task_responder: F,
) -> MockServer
where
    F: Fn(&serde_json::Value) -> Option<serde_json::Value> + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    spawn_mock_mcp_on_shaped(listener, framing, shape, task_responder)
}

/// Spawn a one-shot mock MCP server on an already-bound `listener`, speaking
/// JSON framing with the `structuredContent` result shape. Thin
/// [`Framing::Json`]/[`ResultShape::StructuredContent`] wrapper over
/// [`spawn_mock_mcp_on_shaped`]; see that function for the accept-loop
/// details (non-blocking poll, stop-flag teardown, per-leg responses).
#[allow(dead_code)]
pub fn spawn_mock_mcp_on<F>(listener: TcpListener, task_responder: F) -> MockServer
where
    F: Fn(&serde_json::Value) -> Option<serde_json::Value> + Send + Sync + 'static,
{
    spawn_mock_mcp_on_shaped(
        listener,
        Framing::Json,
        ResultShape::StructuredContent,
        task_responder,
    )
}

/// Spawn a one-shot mock MCP server on an ALREADY-BOUND `listener`, deriving
/// the advertised `addr`/`url` from `listener.local_addr()`. Lets a caller
/// stand a real MCP responder at a specific address (e.g. to play the
/// adversary in a port-recycling regression lock) rather than at whatever
/// ephemeral port the OS hands out.
///
/// `framing` (see [`Framing`]) selects the wire framing every response in
/// the session is written with, EXCEPT the `notifications/initialized`
/// leg, which always answers 202 with an empty body regardless of framing
/// — that matches real MCP, and `FusedMemoryClient::post` short-circuits
/// on status 202 before sniffing content-type, so SSE-framing that leg
/// would be untestable fiction. `shape` (see [`ResultShape`]) selects the
/// `tools/call` result envelope shape independently of `framing`.
///
/// The accept loop uses a short `set_nonblocking` poll so it wakes
/// periodically to check the stop flag even without a wakeup connection —
/// that way a stop request can't hang the test runner.
#[allow(dead_code)]
pub fn spawn_mock_mcp_on_shaped<F>(
    listener: TcpListener,
    framing: Framing,
    shape: ResultShape,
    task_responder: F,
) -> MockServer
where
    F: Fn(&serde_json::Value) -> Option<serde_json::Value> + Send + Sync + 'static,
{
    let addr = listener.local_addr().expect("local_addr");
    let url = format!("http://127.0.0.1:{}/mcp/", addr.port());
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = Arc::clone(&stop);
    let observed: Arc<Mutex<Vec<ObservedRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let observed_clone = Arc::clone(&observed);
    let responder = Arc::new(task_responder);

    // Non-blocking accept with a short poll so the accept loop wakes
    // regularly enough to notice the stop flag even if the wakeup
    // connection in `stop()` never lands.
    listener
        .set_nonblocking(true)
        .expect("set_nonblocking on mock listener");

    let handle = thread::spawn(move || {
        loop {
            if stop_clone.load(Ordering::Relaxed) {
                return;
            }
            let mut stream = match listener.accept() {
                Ok((s, _)) => s,
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(_) => {
                    // Backoff for non-WouldBlock errors (e.g. EMFILE on
                    // a constrained CI box) so the accept loop doesn't
                    // peg a CPU until the test's overall timeout.
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
            };
            // Restore blocking semantics on the accepted stream so the
            // BufReader inside read_request() doesn't busy-loop.
            let _ = stream.set_nonblocking(false);
            let (headers, body) = match read_request(&mut stream) {
                Some(pair) => pair,
                None => continue,
            };
            let method = body
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string();
            let req_id = body.get("id").cloned();

            // Record the request BEFORE dispatching to the responder, and
            // release the lock immediately — the responder (a test
            // closure) never runs while this lock is held, so a panic
            // inside it cannot poison `observed`.
            {
                let mut log = observed_clone.lock().unwrap_or_else(|e| e.into_inner());
                log.push(ObservedRequest {
                    rpc_method: method.clone(),
                    headers,
                });
            }

            match method.as_str() {
                "initialize" => {
                    let resp = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "result": {
                            "protocolVersion": "2024-11-05",
                            "capabilities": {},
                            "serverInfo": {"name": "mock-mcp", "version": "0.1"}
                        }
                    });
                    write_response_framed(
                        &mut stream,
                        200,
                        Some(MOCK_SESSION_ID),
                        framing,
                        resp.to_string().as_bytes(),
                    );
                }
                "notifications/initialized" => {
                    // Always 202/empty regardless of `framing` — see the
                    // doc comment above for why SSE-framing this leg would
                    // be untestable fiction.
                    write_response(&mut stream, 202, b"");
                }
                "tools/call" => {
                    let args = body
                        .get("params")
                        .and_then(|p| p.get("arguments"))
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    let resp_value = match responder(&args) {
                        Some(structured) => {
                            let result = match shape {
                                ResultShape::StructuredContent => {
                                    serde_json::json!({"structuredContent": structured, "content": []})
                                }
                                ResultShape::ContentText => serde_json::json!({
                                    "content": [{"type": "text", "text": structured.to_string()}]
                                }),
                            };
                            serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": req_id,
                                "result": result
                            })
                        }
                        None => serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "error": {"code": -32000, "message": "task not found"}
                        }),
                    };
                    write_response_framed(
                        &mut stream,
                        200,
                        None,
                        framing,
                        resp_value.to_string().as_bytes(),
                    );
                }
                _ => {
                    write_response_framed(&mut stream, 200, None, framing, b"{}");
                }
            }
        }
    });

    MockServer {
        url,
        addr,
        stop,
        handle: Some(handle),
        observed,
    }
}

#[allow(dead_code)]
impl MockServer {
    pub fn url(&self) -> &str {
        &self.url
    }

    /// A handle to the observed-request log, rather than a snapshot copy.
    /// `stop` takes `self` by value, so a caller that wants a race-free
    /// read of the log AFTER stopping (which joins the accept thread) must
    /// grab this handle first and read through it once `stop()` returns.
    pub fn observed_handle(&self) -> Arc<Mutex<Vec<ObservedRequest>>> {
        Arc::clone(&self.observed)
    }

    /// Signal the accept loop to exit and join the thread. Uses the bound
    /// `SocketAddr` directly (no URL parsing) — a stop request will always
    /// reach the loop, plus the loop's own non-blocking poll guarantees it
    /// wakes even if the wakeup connection is dropped.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Best-effort wakeup; the non-blocking accept poll is the safety
        // net so this can fail without hanging the test.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[allow(dead_code)]
impl Drop for MockServer {
    fn drop(&mut self) {
        // Idempotent shutdown if the test never called `.stop()` (e.g. on
        // panic). Mirrors `stop()` minus the join — we let the thread
        // tear down on its own to avoid blocking the drop path.
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(50));
    }
}
