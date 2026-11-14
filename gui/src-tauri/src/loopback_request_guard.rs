//! The admission rule for the reify-debug HTTP server: serve only requests whose
//! Host names the bound loopback listener and that carry no foreign Origin.
//! Rationale: docs/debug-mcp-contract.md §7.

use std::net::SocketAddr;

use axum::extract::{Request, State};
use axum::http::header::{HOST, ORIGIN};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde_json::json;

/// `router` with every request refused (403) unless it passes
/// [`check_loopback_request`] against `bound`.
pub fn admit_only_loopback_requests<S: Clone + Send + Sync + 'static>(
    router: Router<S>,
    bound: SocketAddr,
) -> Router<S> {
    router.layer(middleware::from_fn_with_state(
        bound,
        refuse_unless_loopback,
    ))
}

async fn refuse_unless_loopback(
    State(bound): State<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    match check_loopback_request(request.headers(), bound) {
        Ok(()) => next.run(request).await,
        Err(refusal) => {
            let message = refusal_message(&refusal, bound);
            tracing::warn!("{message}");
            (StatusCode::FORBIDDEN, Json(json!({ "error": message }))).into_response()
        }
    }
}

fn refusal_message(refusal: &Refusal, bound: SocketAddr) -> String {
    let [ip, localhost] = accepted_authorities(bound);
    match refusal {
        Refusal::MissingHost => format!(
            "reify-debug refused a request with no Host header; accepted Host: {ip} or {localhost}"
        ),
        Refusal::ForeignHost(host) => format!(
            "reify-debug refused Host {host:?}: it does not name this listener; \
             accepted Host: {ip} or {localhost}"
        ),
        Refusal::ForeignOrigin(origin) => format!(
            "reify-debug refused Origin {origin:?}: it is not this listener's origin; \
             accepted Origin: none, http://{ip} or http://{localhost}"
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    MissingHost,
    ForeignHost(String),
    ForeignOrigin(String),
}

fn accepted_authorities(bound: SocketAddr) -> [String; 2] {
    [bound.to_string(), format!("localhost:{}", bound.port())]
}

pub fn check_loopback_request(headers: &HeaderMap, bound: SocketAddr) -> Result<(), Refusal> {
    let accepted = accepted_authorities(bound);
    let hosts = headers.get_all(HOST);
    if hosts.iter().next().is_none() {
        return Err(Refusal::MissingHost);
    }
    if let Some(host) = first_not_naming_listener(hosts.iter(), "", &accepted) {
        return Err(Refusal::ForeignHost(host));
    }
    let origins = headers.get_all(ORIGIN);
    if let Some(origin) = first_not_naming_listener(origins.iter(), "http://", &accepted) {
        return Err(Refusal::ForeignOrigin(origin));
    }
    Ok(())
}

fn first_not_naming_listener<'a>(
    values: impl Iterator<Item = &'a HeaderValue>,
    prefix: &str,
    accepted: &[String; 2],
) -> Option<String> {
    values
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
        .find(|value| !names_listener(value, prefix, accepted))
}

fn names_listener(value: &str, prefix: &str, accepted: &[String; 2]) -> bool {
    accepted
        .iter()
        .any(|authority| value.eq_ignore_ascii_case(&format!("{prefix}{authority}")))
}
