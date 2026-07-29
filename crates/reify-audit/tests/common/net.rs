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
//!   (Measured here: `bind("127.0.0.1:0")` came back bound to port 59193.)
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
//! `JcodemunchClient::initialize` (`src/jcodemunch_client.rs`) accepts a 202,
//! an empty body, or valid-JSON-that-is-not-an-initialize-result as a
//! successful MCP handshake. That is a real but *separate* fail-soft hole,
//! and it is not what caused the #5830 flake: the in-suite recycler
//! (`spawn_mock_mcp` in `cli.rs`) answers `initialize` with a fully
//! well-formed JSON-RPC result carrying `protocolVersion`, `capabilities` and
//! `serverInfo`, which any realistic response validation would accept — so
//! hardening the handshake would have left the flake untouched. A separate
//! follow-up task tracks that hardening; it needs live-wire verification
//! against a running jcodemunch serve, which is a different verification
//! budget from this test-only de-flake.
//!
//! Per the `common` partial-consumer contract, every item here carries
//! `#[allow(dead_code)]` so a test binary may consume only a subset.

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
