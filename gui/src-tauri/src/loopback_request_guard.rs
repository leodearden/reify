//! The admission rule for the reify-debug HTTP server: serve only requests whose
//! Host names the bound loopback listener and that carry no foreign Origin.
//! Rationale: docs/debug-mcp-contract.md §7.

use std::net::SocketAddr;

use axum::http::header::{HOST, ORIGIN};
use axum::http::{HeaderMap, HeaderValue};

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
