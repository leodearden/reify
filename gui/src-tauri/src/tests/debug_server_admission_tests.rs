#![cfg(feature = "gui")]
//! Served tests for `crate::debug_server::serve_debug_routes`: over a real
//! socket on an ephemeral `BoundDebugListener`, only requests addressed to that
//! bound listener reach a route handler; the rest get a 403 `{"error": ...}`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;

use crate::debug_server::{BoundDebugListener, serve_debug_routes};

/// A counting stub router served through `serve_debug_routes`; the server task
/// is aborted when this is dropped.
struct ServedStub {
    port: u16,
    handled: Arc<AtomicUsize>,
    server: JoinHandle<Result<(), String>>,
}

impl ServedStub {
    fn start() -> Self {
        let listener = BoundDebugListener::bind(0).expect("ephemeral debug listener binds");
        let port = listener.port();
        let handled = Arc::new(AtomicUsize::new(0));
        let server = tokio::spawn(serve_debug_routes(listener, counting_routes(&handled)));
        Self {
            port,
            handled,
            server,
        }
    }

    fn handled(&self) -> usize {
        self.handled.load(Ordering::SeqCst)
    }
}

impl Drop for ServedStub {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn counting_routes(handled: &Arc<AtomicUsize>) -> Router {
    let count = |handled: Arc<AtomicUsize>| {
        move || async move {
            handled.fetch_add(1, Ordering::SeqCst);
            Json(json!({"ok": true}))
        }
    };
    Router::new()
        .route("/mcp", post(count(handled.clone())))
        .route("/debug/{command}", post(count(handled.clone())))
}

const EXCHANGE_DEADLINE: Duration = Duration::from_secs(10);

/// POST `{}` to `127.0.0.1:port` with the given Host and optional Origin over
/// raw HTTP/1.1; returns the response status code and body. Panics, naming the
/// request, if the exchange does not complete within [`EXCHANGE_DEADLINE`].
async fn post_json(port: u16, host: &str, origin: Option<&str>, path: &str) -> (u16, String) {
    tokio::time::timeout(EXCHANGE_DEADLINE, exchange(port, host, origin, path))
        .await
        .unwrap_or_else(|_| {
            panic!(
                "POST {path} to 127.0.0.1:{port} (Host {host}, Origin {origin:?}) got no \
                 complete response within {EXCHANGE_DEADLINE:?}"
            )
        })
}

async fn exchange(port: u16, host: &str, origin: Option<&str>, path: &str) -> (u16, String) {
    let body = "{}";
    let origin_line = origin
        .map(|o| format!("Origin: {o}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\n{origin_line}\
         Content-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap_or_else(|e| panic!("connect to 127.0.0.1:{port}: {e}"));
    stream
        .write_all(request.as_bytes())
        .await
        .unwrap_or_else(|e| panic!("write request to 127.0.0.1:{port}: {e}"));
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .await
        .unwrap_or_else(|e| panic!("read response from 127.0.0.1:{port}: {e}"));
    let response = String::from_utf8(raw).expect("response is UTF-8");
    let (head, body) = response
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("response has no header/body separator: {response:?}"));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or_else(|| panic!("response has no status code: {head:?}"));
    (status, body.to_string())
}

fn error_message(body: &str) -> String {
    let parsed: Value = serde_json::from_str(body)
        .unwrap_or_else(|e| panic!("refusal body is not JSON ({e}): {body:?}"));
    parsed["error"]
        .as_str()
        .unwrap_or_else(|| panic!("refusal body has no string `error`: {body:?}"))
        .to_string()
}

const PATHS: [&str; 2] = ["/mcp", "/debug/health"];

#[tokio::test]
async fn loopback_requests_reach_the_handlers() {
    let served = ServedStub::start();
    let port = served.port;
    let hosts = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];

    let mut sent = 0;
    for host in &hosts {
        for path in PATHS {
            let (status, body) = post_json(port, host, None, path).await;
            sent += 1;
            assert_eq!(
                status, 200,
                "Host {host} on {path} names the bound listener and must be served: {body}"
            );
        }
    }
    assert_eq!(
        served.handled(),
        sent,
        "every loopback request must reach its handler"
    );
}

#[tokio::test]
async fn a_rebinding_host_is_refused_before_any_handler() {
    let served = ServedStub::start();
    let port = served.port;
    let host = format!("attacker.example:{port}");

    for path in PATHS {
        let (status, body) = post_json(port, &host, None, path).await;
        assert_eq!(status, 403, "Host {host} on {path} must be refused: {body}");
        let error = error_message(&body);
        assert!(
            error.contains(&host),
            "the refusal for {path} must name the refused Host {host}: {error:?}"
        );
    }
    assert_eq!(
        served.handled(),
        0,
        "a refused request must never reach a handler"
    );
}

#[tokio::test]
async fn a_foreign_origin_is_refused_before_any_handler() {
    let served = ServedStub::start();
    let port = served.port;
    let host = format!("127.0.0.1:{port}");
    let origin = "http://attacker.example";

    let (status, body) = post_json(port, &host, Some(origin), "/mcp").await;
    assert_eq!(status, 403, "Origin {origin} must be refused: {body}");
    let error = error_message(&body);
    assert!(
        error.contains(origin),
        "the refusal must name the refused Origin {origin}: {error:?}"
    );
    assert_eq!(
        served.handled(),
        0,
        "a refused request must never reach a handler"
    );
}
