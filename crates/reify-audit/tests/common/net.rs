//! An endpoint that is unreachable BY CONSTRUCTION, for "connection refused"
//! tests.
//!
//! ## Why not "bind an ephemeral port, record it, drop the listener"
//!
//! That older idiom yields a port that is merely *unowned at the instant it
//! was minted*, which leaves a time-of-check/time-of-use window: anything
//! that binds an ephemeral port in the meantime can be handed the very port
//! the URL names. Inside this test suite the recycler is real — the mock MCP
//! server in `cli.rs` binds `127.0.0.1:0` — and when it landed on a recycled
//! port it answered `initialize` happily, so the binary's fail-soft
//! "jcodemunch unreachable" breadcrumb never fired and the assertion on it
//! failed. That was the #5830 flake.
//!
//! ## The identity this module rests on (not a heuristic)
//!
//! TCP port 0 is not an address a listener can occupy:
//!
//! * `bind()` with port 0 *means* "kernel, assign me an ephemeral port", so a
//!   listener that asks for `:0` is handed a **different** port. Nothing can
//!   ever end up listening on port 0, so there is no TOCTOU window to lose.
//! * `connect()` to port 0 is rejected by the kernel immediately — measured
//!   on Linux as `ECONNREFUSED` (errno 111). Other platforms return
//!   `EADDRNOTAVAIL`/`WSAEADDRNOTAVAIL` instead; still an error, which is all
//!   these tests require.
//! * `"127.0.0.1:0"` still *resolves* (measured: `getaddrinfo` →
//!   `('127.0.0.1', 0)`), so probes that resolve-then-connect reach their
//!   connect leg and report "unreachable" via connection refusal rather than
//!   short-circuiting on a resolution failure.
//!
//! Refusal is also immediate, so this sentinel is strictly faster than the
//! idiom it replaces.
//!
//! ## Deliberately out of scope here
//!
//! `JcodemunchClient::initialize` (`src/jcodemunch_client.rs`) does not
//! validate the *body* of the `initialize` response. Since #6106 it does
//! require the server to assign a session — a response with no
//! `Mcp-Session-Id` header is rejected — so the residual hole is narrower
//! than it was: a response that DOES carry a session id but whose body is a
//! 202, empty, or valid-JSON-that-is-not-an-initialize-result is still
//! accepted as a successful handshake.
//!
//! That residual is a real but *separate* fail-soft hole, and it is not what
//! caused the #5830 flake: the in-suite recycler (`spawn_mock_mcp` in
//! `cli.rs`) answers `initialize` with a fully well-formed JSON-RPC result
//! carrying `protocolVersion`, `capabilities` and `serverInfo` — and now a
//! session id too — which any realistic response validation would accept, so
//! hardening the handshake would have left the flake untouched. Task #5832
//! tracks that hardening; it needs live-wire verification against a running
//! jcodemunch serve, which is a different verification budget from this
//! test-only de-flake.
//!
//! Per the `common` partial-consumer contract, every item here carries
//! `#[allow(dead_code)]` so a test binary may consume only a subset.

use std::net::{SocketAddr, TcpListener};

/// `host:port` of an endpoint nothing can ever listen on. See the module doc
/// for why port 0 is unreachable by construction rather than by luck.
#[allow(dead_code)]
pub const UNREACHABLE_ADDR: &str = "127.0.0.1:0";

/// An MCP endpoint URL that is guaranteed to refuse connections.
///
/// Use this wherever a test needs "the server is down" semantics; it never
/// needs a teardown and never races another binder.
#[allow(dead_code)]
pub fn unreachable_mcp_url() -> String {
    format!("http://{UNREACHABLE_ADDR}/mcp")
}

/// Attempt to occupy the EXACT `host:port` that `url` names.
///
/// Returns the parsed address plus `Some(listener)` iff the kernel actually
/// handed back that same port — a bind request for port 0 is a request for an
/// *ephemeral* port, so it comes back bound somewhere else and is correctly
/// reported as "no hijack".
///
/// This is how the #5830 port-recycling race is made deterministic: the test
/// itself plays the adversary at the address under test instead of soaking and
/// hoping to catch a real recycler in the act.
///
/// The address is parsed back out of `url` rather than read from
/// [`UNREACHABLE_ADDR`] on purpose: it ties the adversary to the endpoint the
/// caller actually probes, so a future change to [`unreachable_mcp_url`] that
/// pointed somewhere else could not leave these locks quietly hijacking an
/// address nobody is testing.
#[allow(dead_code)]
pub fn try_hijack_url(url: &str) -> (SocketAddr, Option<TcpListener>) {
    let host_port = url
        .trim_start_matches("http://")
        .split('/')
        .next()
        .expect("url has a host:port segment");
    let addr: SocketAddr = host_port
        .parse()
        .unwrap_or_else(|e| panic!("'{host_port}' must parse as a SocketAddr: {e}"));
    let hijack = TcpListener::bind(addr)
        .ok()
        .filter(|l| l.local_addr().ok().map(|a| a.port()) == Some(addr.port()));
    (addr, hijack)
}
