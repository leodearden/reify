//! The MCP `initialize` handshake contract both reify-audit MCP clients
//! share: a responder is a live MCP seam only if its `initialize` reply is an
//! MCP `InitializeResult`.
//!
//! Assigning a session id is not enough. Every non-MCP reply below still
//! carries one (see [`InitializeReply::Raw`]), so the only thing that can
//! reject it is an inspection of the body. A client that accepted such a
//! reply would hand the binary a "connected" seam whose every later call
//! fails, which the sweep then reports as a clean, zero-finding run (PRD
//! jcodemunch-substrate-restoration §4.1 item 5, boundary B3).
//!
//! Driven only through the public constructors, against the shared hermetic
//! mock in `common::mcp_mock`.
//!
//! User-observable signal:
//!   `cargo test -p reify-audit --test mcp_handshake`

mod common;

use std::net::TcpListener;

use common::mcp_mock::{
    Framing, InitializeReply, MockConfig, MockServer, spawn_mock_mcp_configured,
};
use reify_audit::fused_memory_client::{self, FusedMemoryClient};
use reify_audit::jcodemunch_client::{self, JcodemunchClient};

/// `initialize` replies that are not an MCP `InitializeResult`, each with the
/// label its panic message names.
const NON_MCP_REPLIES: &[(&str, InitializeReply)] = &[
    (
        "202 with an empty body",
        InitializeReply::Raw {
            status: 202,
            body: "",
        },
    ),
    (
        "200 with an empty body",
        InitializeReply::Raw {
            status: 200,
            body: "",
        },
    ),
    (
        "200 with a bare `{}`",
        InitializeReply::Raw {
            status: 200,
            body: "{}",
        },
    ),
    (
        "200 with an empty result object",
        InitializeReply::Raw {
            status: 200,
            body: r#"{"jsonrpc":"2.0","id":1,"result":{}}"#,
        },
    ),
    (
        "200 with a result missing serverInfo",
        InitializeReply::Raw {
            status: 200,
            body: r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
        },
    ),
    (
        "200 with a string result",
        InitializeReply::Raw {
            status: 200,
            body: r#"{"jsonrpc":"2.0","id":1,"result":"ok"}"#,
        },
    ),
    (
        "200 with a full result under jsonrpc 1.0",
        InitializeReply::Raw {
            status: 200,
            body: r#"{"jsonrpc":"1.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{},"serverInfo":{"name":"mock-mcp","version":"0.1"}}}"#,
        },
    ),
];

/// The two framings a live serve can answer a well-formed `initialize` with.
const WELL_FORMED_FRAMINGS: &[(&str, Framing)] = &[("JSON", Framing::Json), ("SSE", Framing::Sse)];

/// What constructing a client against one mock came to, with the two
/// clients' distinct `LoadError` types folded onto the distinctions these
/// tests draw.
enum Outcome {
    Accepted,
    Protocol(String),
    OtherError(String),
}

/// Stand one mock up on an ephemeral port. Construction is the whole test,
/// so no `tools/call` is ever reached and the responder is never consulted.
fn spawn(config: MockConfig) -> MockServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    spawn_mock_mcp_configured(listener, config, |_args| None)
}

fn replying(initialize: InitializeReply) -> MockConfig {
    MockConfig {
        initialize,
        ..MockConfig::default()
    }
}

fn well_formed_under(framing: Framing) -> MockConfig {
    MockConfig {
        framing,
        ..MockConfig::default()
    }
}

/// Construct a [`JcodemunchClient`] against a mock answering `config`. The
/// mock is stopped before the outcome is inspected, so a failing assertion
/// cannot leak its accept thread.
fn connect_jcodemunch(config: MockConfig) -> Outcome {
    let mock = spawn(config);
    let result = JcodemunchClient::new(mock.url());
    mock.stop();
    match result {
        Ok(_) => Outcome::Accepted,
        Err(jcodemunch_client::LoadError::Protocol(msg)) => Outcome::Protocol(msg),
        Err(other) => Outcome::OtherError(other.to_string()),
    }
}

/// As [`connect_jcodemunch`], for [`FusedMemoryClient`].
fn connect_fused_memory(config: MockConfig) -> Outcome {
    let mock = spawn(config);
    let result = FusedMemoryClient::new(mock.url());
    mock.stop();
    match result {
        Ok(_) => Outcome::Accepted,
        Err(fused_memory_client::LoadError::Protocol(msg)) => Outcome::Protocol(msg),
        Err(other) => Outcome::OtherError(other.to_string()),
    }
}

/// Every [`NON_MCP_REPLIES`] row `connect` failed to reject as a `Protocol`
/// error naming `initialize`, one line per row. Collected rather than
/// panicking on the first, so one run reports every row that slips through.
fn non_mcp_replies_not_rejected(client: &str, connect: fn(MockConfig) -> Outcome) -> Vec<String> {
    NON_MCP_REPLIES
        .iter()
        .filter_map(|(label, reply)| match connect(replying(*reply)) {
            Outcome::Protocol(msg) if msg.contains("initialize") => None,
            Outcome::Protocol(msg) => Some(format!(
                "{client}, reply `{label}`: rejected as Protocol, but the message \
                 does not name `initialize`: {msg:?}"
            )),
            Outcome::OtherError(msg) => Some(format!(
                "{client}, reply `{label}`: expected a Protocol rejection of the \
                 body, got a different error (the reply may never have been \
                 delivered): {msg}"
            )),
            Outcome::Accepted => Some(format!(
                "{client}, reply `{label}`: ACCEPTED as a completed MCP handshake. \
                 A session id was assigned, but the body is not an \
                 InitializeResult, so this is not a live seam"
            )),
        })
        .collect()
}

/// Every [`WELL_FORMED_FRAMINGS`] row `connect` failed to accept.
fn well_formed_replies_not_accepted(
    client: &str,
    connect: fn(MockConfig) -> Outcome,
) -> Vec<String> {
    WELL_FORMED_FRAMINGS
        .iter()
        .filter_map(|(label, framing)| {
            let refusal = match connect(well_formed_under(*framing)) {
                Outcome::Accepted => return None,
                Outcome::Protocol(msg) | Outcome::OtherError(msg) => msg,
            };
            Some(format!(
                "{client}, well-formed InitializeResult under {label} framing: \
                 REJECTED, so the validation would refuse a healthy server: {refusal}"
            ))
        })
        .collect()
}

#[test]
fn jcodemunch_client_rejects_every_non_mcp_initialize_reply() {
    let misses = non_mcp_replies_not_rejected("JcodemunchClient", connect_jcodemunch);
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}

#[test]
fn fused_memory_client_rejects_every_non_mcp_initialize_reply() {
    let misses = non_mcp_replies_not_rejected("FusedMemoryClient", connect_fused_memory);
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}

/// Positive control for the rejection table: the same mock, answering a
/// well-formed `initialize`, must connect under both framings a live serve
/// uses (a jcodemunch serve answers SSE). Without it, a client that rejected
/// EVERY reply would pass the table above.
#[test]
fn jcodemunch_client_accepts_a_well_formed_initialize_under_both_framings() {
    let misses = well_formed_replies_not_accepted("JcodemunchClient", connect_jcodemunch);
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}

/// As [`jcodemunch_client_accepts_a_well_formed_initialize_under_both_framings`],
/// for [`FusedMemoryClient`].
#[test]
fn fused_memory_client_accepts_a_well_formed_initialize_under_both_framings() {
    let misses = well_formed_replies_not_accepted("FusedMemoryClient", connect_fused_memory);
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}
